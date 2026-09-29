//! The painting DSL: tokenizer, statement validation and typed value access.

use std::collections::BTreeMap;
use std::fmt;

use crate::color::{self, Rgba};
use crate::ops;

/// An error tied to a script line, printed as `line 7: stipple: message`.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub struct ScriptError {
    pub line: usize,
    pub op: String,
    pub message: String,
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.op.is_empty() {
            write!(f, "line {}: {}", self.line, self.message)
        } else {
            write!(f, "line {}: {}: {}", self.line, self.op, self.message)
        }
    }
}

/// One parsed statement: `OPNAME key=value ...`, values still raw text.
#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    /// 1-based line number of the statement's first physical line.
    pub line: usize,
    pub op: String,
    pub args: Vec<(String, String)>,
}

impl Stmt {
    /// Wrap a message into a [`ScriptError`] for this statement.
    pub fn error(&self, message: impl Into<String>) -> ScriptError {
        ScriptError {
            line: self.line,
            op: self.op.clone(),
            message: message.into(),
        }
    }
}

/// Split `s` at `sep` characters that are not nested inside parentheses.
pub fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c == sep && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + sep.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Strip a trailing comment. `#` starts a comment only at the start of a
/// token (line start or after whitespace) and outside parentheses, so
/// `color=#c33` and `colors=#a00,#00a` are values, not comments.
/// `depth` carries the parenthesis depth across continued lines.
fn strip_comment(line: &str, depth: &mut i32) -> String {
    let mut prev_ws = true;
    for (i, ch) in line.char_indices() {
        match ch {
            '(' => *depth += 1,
            ')' => *depth -= 1,
            '#' if prev_ws && *depth <= 0 => return line[..i].to_string(),
            _ => {}
        }
        prev_ws = ch.is_whitespace();
    }
    line.to_string()
}

/// Split one logical line into whitespace-separated tokens, keeping
/// whitespace inside parentheses.
fn tokenize(line: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    for ch in line.chars() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return Err("unbalanced ')'".to_string());
                }
            }
            _ => {}
        }
        if !ch.is_whitespace() {
            current.push(ch);
        } else if depth == 0 && !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
        // Whitespace inside parentheses is dropped, so `rgb(1, 2, 3)` and
        // `rgb(1,2,3)` are the same token.
    }
    if depth != 0 {
        return Err("unbalanced '('".to_string());
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

/// Parse a script into statements, validating op names and keys.
///
/// Values are *not* parsed here, because variables defined earlier in the
/// same script must be substituted first; see [`Args`].
///
/// # Errors
///
/// Returns the first syntax error, unknown op, unknown key, duplicate key or
/// missing required key, with its line number.
pub fn parse_script(src: &str) -> Result<Vec<Stmt>, ScriptError> {
    let mut stmts = Vec::new();
    let mut pending = String::new();
    let mut pending_line = 0;
    let mut depth = 0i32;
    for (idx, raw) in src.lines().enumerate() {
        let line_no = idx + 1;
        if pending.is_empty() {
            pending_line = line_no;
            depth = 0;
        }
        let stripped = strip_comment(raw, &mut depth);
        let trimmed = stripped.trim_end();
        if let Some(cont) = trimmed.strip_suffix('\\') {
            pending.push_str(cont);
            pending.push(' ');
            continue;
        }
        pending.push_str(trimmed);
        let logical = std::mem::take(&mut pending);
        if let Some(stmt) = parse_line(&logical, pending_line)? {
            stmts.push(stmt);
        }
    }
    if !pending.trim().is_empty() {
        if let Some(stmt) = parse_line(&pending, pending_line)? {
            stmts.push(stmt);
        }
    }
    Ok(stmts)
}

fn parse_line(line: &str, line_no: usize) -> Result<Option<Stmt>, ScriptError> {
    let syntax = |message: String| ScriptError {
        line: line_no,
        op: String::new(),
        message,
    };
    let tokens = tokenize(line).map_err(syntax)?;
    let Some((op, rest)) = tokens.split_first() else {
        return Ok(None);
    };
    let Some(spec) = ops::spec(op) else {
        let hint = suggest(op, ops::OPS.iter().map(|s| s.name))
            .map(|s| format!(" (did you mean '{s}'?)"))
            .unwrap_or_default();
        return Err(syntax(format!(
            "unknown op '{op}'{hint}; run `easel ops` for the list"
        )));
    };
    let stmt_err = |message: String| ScriptError {
        line: line_no,
        op: spec.name.to_string(),
        message,
    };
    let mut args: Vec<(String, String)> = Vec::with_capacity(rest.len());
    for token in rest {
        let Some((key, value)) = token.split_once('=') else {
            let after_separator = args
                .last()
                .is_some_and(|(_, v)| v.ends_with(',') || v.ends_with(';'));
            let hint = if after_separator {
                "; whitespace separates tokens, so remove the space after ',' or ';'"
            } else {
                ""
            };
            return Err(stmt_err(format!("expected key=value, got '{token}'{hint}")));
        };
        if key.is_empty() || value.is_empty() {
            return Err(stmt_err(format!("expected key=value, got '{token}'")));
        }
        if args.iter().any(|(k, _)| k == key) {
            return Err(stmt_err(format!("key '{key}' given twice")));
        }
        if spec.keys.is_empty() {
            // `set`: keys are variable names.
            if !is_name(key) {
                return Err(stmt_err(format!(
                    "bad variable name '{key}' (use letters, digits, '_' and '-')"
                )));
            }
        } else if !spec.keys.contains(&key) {
            let hint = suggest(key, spec.keys.iter().copied().chain(alias_targets(key)))
                .map(|s| format!(" (did you mean '{s}'?)"))
                .unwrap_or_default();
            return Err(stmt_err(format!(
                "unknown key '{key}'{hint}; see `easel ops {}`",
                spec.name
            )));
        }
        args.push((key.to_string(), value.to_string()));
    }
    for required in spec.required {
        if !args.iter().any(|(k, _)| k == required) {
            return Err(stmt_err(format!("missing required key '{required}'")));
        }
    }
    Ok(Some(Stmt {
        line: line_no,
        op: spec.name.to_string(),
        args,
    }))
}

/// Keys that people (and models) commonly reach for, mapped to the real key.
fn alias_targets(key: &str) -> Option<&'static str> {
    const ALIASES: &[(&str, &str)] = &[
        ("radius", "r"),
        ("raduis", "r"),
        ("size", "r"),
        ("colour", "color"),
        ("colours", "colors"),
        ("opacity", "alpha"),
        ("n", "count"),
        ("shape", "in"),
        ("points", "path"),
        ("pos", "at"),
    ];
    ALIASES.iter().find(|(a, _)| *a == key).map(|(_, t)| *t)
}

/// Levenshtein edit distance.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != *cb);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Closest candidate to `word`, if it is plausibly a typo of it.
fn suggest<'a>(word: &str, candidates: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    if let Some(target) = alias_targets(word) {
        return Some(target);
    }
    candidates
        .map(|c| (edit_distance(word, c), c))
        .filter(|(d, c)| *d <= 2.max(c.len() / 3) && *d < c.len().max(word.len()))
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

/// Is `s` a valid layer/variable name (`[A-Za-z0-9_-]+`)?
pub fn is_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Replace `$name` references with variable values.
///
/// # Errors
///
/// Fails on a reference to an undefined variable.
pub fn substitute(raw: &str, vars: &BTreeMap<String, String>) -> Result<String, String> {
    if !raw.contains('$') {
        return Ok(raw.to_string());
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(after.len());
        let name = &after[..end];
        if name.is_empty() {
            return Err("'$' must be followed by a variable name".to_string());
        }
        let value = vars
            .get(name)
            .ok_or_else(|| format!("undefined variable ${name}"))?;
        out.push_str(value);
        rest = &after[end..];
    }
    out.push_str(rest);
    Ok(out)
}

/// A per-mark number: fixed, drawn uniformly from a range, or (dabs only)
/// a ramp that changes linearly along the path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Scalar {
    Fixed(f32),
    Range(f32, f32),
    /// `a->b`: `start` at the path start, `end` at its end. Each end is a
    /// `(lo, hi)` range (equal for a plain number); one random draw picks
    /// the same relative position in both ranges.
    Ramp {
        start: (f32, f32),
        end: (f32, f32),
    },
}

impl Scalar {
    /// Draw a value at path fraction `u` (0..1; only ramps use it). Only
    /// ranges, and ramps with a range at either end, consume randomness.
    pub fn sample_at(self, rng: &mut impl rand::Rng, u: f32) -> f32 {
        let lerp = |(a, b): (f32, f32), t: f32| a + (b - a) * t;
        match self {
            Scalar::Fixed(v) => v,
            Scalar::Range(lo, hi) => lo + (hi - lo) * rng.random::<f32>(),
            Scalar::Ramp { start, end } => {
                let t = if start.0 == start.1 && end.0 == end.1 {
                    0.0
                } else {
                    rng.random::<f32>()
                };
                lerp((lerp(start, t), lerp(end, t)), u)
            }
        }
    }

    /// Draw a value (a ramp gives its start value).
    pub fn sample(self, rng: &mut impl rand::Rng) -> f32 {
        self.sample_at(rng, 0.0)
    }

    /// Largest value this scalar can produce.
    pub fn max(self) -> f32 {
        match self {
            Scalar::Fixed(v) => v,
            Scalar::Range(lo, hi) => lo.max(hi),
            Scalar::Ramp { start, end } => start.1.max(end.1),
        }
    }

    pub fn min(self) -> f32 {
        match self {
            Scalar::Fixed(v) => v,
            Scalar::Range(lo, hi) => lo.min(hi),
            Scalar::Ramp { start, end } => start.0.min(end.0),
        }
    }
}

/// A point in canvas coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

impl Pt {
    pub const fn new(x: f32, y: f32) -> Self {
        Pt { x, y }
    }
}

pub fn parse_number(s: &str) -> Result<f32, String> {
    s.trim()
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("expected a number, got '{s}'"))
}

/// Parse a number or a range `a..b`.
///
/// # Errors
///
/// Malformed numbers, reversed ranges and ramps (`a->b`, see
/// [`parse_ramp`]) are errors.
pub fn parse_scalar(s: &str) -> Result<Scalar, String> {
    if s.contains("->") {
        return Err(format!(
            "'{s}' is a ramp; ramps (a->b, changing along a path) only work for dabs r, \
             alpha and scatter, and stroke width and alpha"
        ));
    }
    match s.split_once("..") {
        Some((lo, hi)) => {
            let (lo, hi) = (parse_number(lo)?, parse_number(hi)?);
            if lo > hi {
                return Err(format!(
                    "range {lo}..{hi} is reversed; did you mean {hi}..{lo}? \
                     (to fade along a dabs or stroke path, use a ramp: {lo}->{hi})"
                ));
            }
            Ok(Scalar::Range(lo, hi))
        }
        None => parse_number(s).map(Scalar::Fixed),
    }
}

/// Parse a number, a range, or a ramp `a->b` whose ends are numbers or
/// ranges (`r=4..6->1..2`).
///
/// # Errors
///
/// Fails on a malformed end, or a ramp with more than one `->`.
pub fn parse_ramp(s: &str) -> Result<Scalar, String> {
    let Some((a, b)) = s.split_once("->") else {
        return parse_scalar(s);
    };
    if b.contains("->") {
        return Err(format!("expected a ramp a->b with one arrow, got '{s}'"));
    }
    let end = |v: &str| -> Result<(f32, f32), String> {
        match parse_scalar(v)? {
            Scalar::Fixed(x) => Ok((x, x)),
            Scalar::Range(lo, hi) => Ok((lo, hi)),
            Scalar::Ramp { .. } => Err(format!("expected a ramp a->b, got '{s}'")),
        }
    };
    Ok(Scalar::Ramp {
        start: end(a)?,
        end: end(b)?,
    })
}

/// Parse a whole number such as a seed.
///
/// # Errors
///
/// Fails on anything that is not an `i64`, including fractions like `2.5`.
pub fn parse_integer(s: &str) -> Result<i64, String> {
    s.trim()
        .parse::<i64>()
        .map_err(|_| format!("expected an integer, got '{s}'"))
}

pub fn parse_point(s: &str) -> Result<Pt, String> {
    match s.split(',').collect::<Vec<_>>().as_slice() {
        [x, y] => Ok(Pt::new(parse_number(x)?, parse_number(y)?)),
        _ => Err(format!("expected a point x,y, got '{s}'")),
    }
}

pub fn parse_path(s: &str) -> Result<Vec<Pt>, String> {
    s.split(';')
        .filter(|p| !p.trim().is_empty())
        .map(parse_point)
        .collect()
}

pub fn parse_bool(s: &str) -> Result<bool, String> {
    match s {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("expected true or false, got '{s}'")),
    }
}

/// Gradient stops `C@t,C@t,...`, sorted by position.
pub fn parse_stops(s: &str) -> Result<Vec<(Rgba, f32)>, String> {
    let mut stops = Vec::new();
    for item in split_top_level(s, ',') {
        if item.trim().is_empty() {
            return Err("empty item in list; whitespace ends a value, so remove any space after ',' (and note that ' #' starts a comment)".to_string());
        }
        let (c, t) = item
            .rsplit_once('@')
            .ok_or_else(|| format!("gradient stop '{item}' must look like COLOR@POSITION"))?;
        let t = parse_number(t)?;
        if !(0.0..=1.0).contains(&t) {
            return Err(format!("stop position {t} must be within 0..1"));
        }
        stops.push((color::parse_color(c)?, t));
    }
    if stops.len() < 2 {
        return Err("a gradient needs at least two stops".to_string());
    }
    stops.sort_by(|a, b| a.1.total_cmp(&b.1));
    Ok(stops)
}

/// Statement arguments after variable substitution, with typed getters.
///
/// Every getter returns `Ok(None)` when the key is absent and an error
/// message naming the key when the value is malformed.
pub struct Args {
    values: Vec<(String, String)>,
}

impl Args {
    /// Substitute variables into every value of `stmt`.
    ///
    /// # Errors
    ///
    /// Fails if a value references an undefined variable.
    pub fn new(stmt: &Stmt, vars: &BTreeMap<String, String>) -> Result<Self, String> {
        let values = stmt
            .args
            .iter()
            .map(|(k, v)| {
                substitute(v, vars)
                    .map(|s| (k.clone(), s))
                    .map_err(|e| format!("key '{k}': {e}"))
            })
            .collect::<Result<_, _>>()?;
        Ok(Args { values })
    }

    pub fn raw(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn has(&self, key: &str) -> bool {
        self.raw(key).is_some()
    }

    /// Parse `key` with `parse`, prefixing any error with the key name.
    pub fn get<T>(
        &self,
        key: &str,
        parse: impl FnOnce(&str) -> Result<T, String>,
    ) -> Result<Option<T>, String> {
        self.raw(key)
            .map(|v| parse(v).map_err(|e| format!("key '{key}': {e}")))
            .transpose()
    }

    /// Like [`Args::get`] but the key must be present.
    pub fn need<T>(
        &self,
        key: &str,
        parse: impl FnOnce(&str) -> Result<T, String>,
    ) -> Result<T, String> {
        self.get(key, parse)?
            .ok_or_else(|| format!("missing required key '{key}'"))
    }

    pub fn number(&self, key: &str) -> Result<Option<f32>, String> {
        self.get(key, parse_number)
    }

    /// An integer value (e.g. `seed`); fractions are errors, not truncated.
    pub fn integer(&self, key: &str) -> Result<Option<i64>, String> {
        self.get(key, parse_integer)
    }

    /// A number constrained to `lo..=hi`.
    pub fn number_in(&self, key: &str, lo: f32, hi: f32) -> Result<Option<f32>, String> {
        match self.number(key)? {
            Some(v) if !(lo..=hi).contains(&v) => {
                Err(format!("key '{key}': {v} is outside {lo}..{hi}"))
            }
            other => Ok(other),
        }
    }

    pub fn scalar(&self, key: &str) -> Result<Option<Scalar>, String> {
        self.get(key, parse_scalar)
    }

    /// A number, range or ramp (see [`parse_ramp`]).
    pub fn ramp(&self, key: &str) -> Result<Option<Scalar>, String> {
        self.get(key, parse_ramp)
    }

    pub fn color(&self, key: &str) -> Result<Option<Rgba>, String> {
        self.get(key, color::parse_color)
    }

    pub fn bool(&self, key: &str) -> Result<Option<bool>, String> {
        self.get(key, parse_bool)
    }

    pub fn point(&self, key: &str) -> Result<Option<Pt>, String> {
        self.get(key, parse_point)
    }

    pub fn name(&self, key: &str) -> Result<Option<String>, String> {
        self.get(key, |v| {
            if is_name(v) {
                Ok(v.to_string())
            } else {
                Err(format!("bad name '{v}' (use letters, digits, '_' and '-')"))
            }
        })
    }

    /// Error unless at most one of `keys` is present.
    pub fn exclusive(&self, keys: &[&str]) -> Result<(), String> {
        let present: Vec<&str> = keys.iter().copied().filter(|k| self.has(k)).collect();
        if present.len() > 1 {
            return Err(format!("keys {} cannot be combined", present.join(" and ")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_continuations_and_hex() {
        let src = "# heading\nfill in=circle(10, 20, 5) color=#c33 # trailing\n\n\
                   dot at=1,2;3,4 \\\n  r=1..2 colors=#a00*2,#00a\n";
        let stmts = parse_script(src).unwrap();
        assert_eq!(stmts.len(), 2);
        assert_eq!(stmts[0].line, 2);
        assert_eq!(stmts[0].args[0], ("in".into(), "circle(10,20,5)".into()));
        assert_eq!(stmts[0].args[1], ("color".into(), "#c33".into()));
        assert_eq!(stmts[1].line, 4);
        assert_eq!(stmts[1].args.len(), 3);
        assert_eq!(stmts[1].args[2].1, "#a00*2,#00a");
    }

    #[test]
    fn unknown_key_names_line_and_suggests() {
        let err = parse_script("layer name=a\n\nstipple in=canvas count=5 raduis=3 color=red\n")
            .unwrap_err();
        assert_eq!(err.line, 3);
        assert_eq!(err.op, "stipple");
        assert!(err.message.contains("unknown key 'raduis'"), "{err}");
        assert!(err.message.contains("did you mean 'r'"), "{err}");
    }

    #[test]
    fn unknown_op_and_missing_key() {
        let err = parse_script("stiple in=canvas").unwrap_err();
        assert!(err.to_string().contains("did you mean 'stipple'"), "{err}");
        let err = parse_script("fill color=red").unwrap_err();
        assert_eq!(err.to_string(), "line 1: fill: missing required key 'in'");
        let err = parse_script("fill in=canvas in=canvas color=red").unwrap_err();
        assert!(err.message.contains("given twice"));
        let err = parse_script("fill in=canvas bogus").unwrap_err();
        assert!(err.message.contains("expected key=value"));
        let err = parse_script("dot at=10, 10 color=#fff").unwrap_err();
        assert!(err.message.contains("remove the space after ','"), "{err}");
    }

    #[test]
    fn unbalanced_parens() {
        assert!(parse_script("fill in=circle(1,2,3 color=red").is_err());
        assert!(parse_script("fill in=circle1,2,3) color=red").is_err());
    }

    #[test]
    fn value_types() {
        assert_eq!(parse_scalar("2").unwrap(), Scalar::Fixed(2.0));
        assert_eq!(parse_scalar("-1..0.5").unwrap(), Scalar::Range(-1.0, 0.5));
        assert!(parse_scalar("1..x").is_err());
        let err = parse_scalar("3..1").unwrap_err();
        assert!(err.contains("reversed") && err.contains("1..3"), "{err}");
        assert!(parse_scalar("1->2").unwrap_err().contains("ramp"));
        assert_eq!(
            parse_ramp("0.9->0.2").unwrap(),
            Scalar::Ramp {
                start: (0.9, 0.9),
                end: (0.2, 0.2)
            }
        );
        let ramp = parse_ramp("4..6->1..2").unwrap();
        assert_eq!((ramp.min(), ramp.max()), (1.0, 6.0));
        let mut rng = <rand_chacha::ChaCha8Rng as rand::SeedableRng>::seed_from_u64(1);
        let mid = parse_ramp("10->0").unwrap().sample_at(&mut rng, 0.25);
        assert!((mid - 7.5).abs() < 1e-5);
        assert!(parse_ramp("1->2->3").unwrap_err().contains("one arrow"));
        assert_eq!(parse_integer("-5").unwrap(), -5);
        assert!(parse_integer("2.9").is_err());
        assert_eq!(parse_point("3,-4.5").unwrap(), Pt::new(3.0, -4.5));
        assert!(parse_point("3").is_err());
        assert_eq!(parse_path("0,0;1,1;2,0").unwrap().len(), 3);
        assert!(parse_bool("yes").is_err());
        let stops = parse_stops("white@1,rgb(1,2,3)@0").unwrap();
        assert_eq!(stops[0].1, 0.0);
        assert!(parse_stops("red@0").is_err());
        assert!(parse_stops("red@0,blue@2").is_err());
    }

    #[test]
    fn variables() {
        let mut vars = BTreeMap::new();
        vars.insert("skin".to_string(), "#d9a07a".to_string());
        vars.insert("r-small".to_string(), "1..2".to_string());
        assert_eq!(
            substitute("mix($skin,black,0.5)", &vars).unwrap(),
            "mix(#d9a07a,black,0.5)"
        );
        assert_eq!(substitute("$r-small", &vars).unwrap(), "1..2");
        assert_eq!(substitute("$skin*2,#fff", &vars).unwrap(), "#d9a07a*2,#fff");
        assert!(substitute("$nope", &vars).unwrap_err().contains("$nope"));
    }

    #[test]
    fn set_accepts_any_name() {
        let stmts = parse_script("set skin=#fff shadow=mix($skin,black,0.3)").unwrap();
        assert_eq!(stmts[0].args.len(), 2);
        assert!(parse_script("set b@d=1").is_err());
    }
}
