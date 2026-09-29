//! Colour values: parsing, sRGB <-> OKLab/OKLCH conversion and mixing.

use crate::dsl::split_top_level;

/// A colour in gamma-encoded sRGB with straight (non-premultiplied) alpha,
/// every channel in 0..=1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// OKLab coordinates (L 0..1, a/b roughly -0.4..0.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba::new(0.0, 0.0, 0.0, 0.0);

    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Rgba { r, g, b, a }
    }

    /// Build from premultiplied 8-bit channels (as stored in a layer).
    pub fn from_premultiplied_u8(px: [u8; 4]) -> Self {
        let a = px[3] as f32 / 255.0;
        if px[3] == 0 {
            return Rgba::TRANSPARENT;
        }
        let un = |c: u8| ((c as f32 / 255.0) / a).min(1.0);
        Rgba::new(un(px[0]), un(px[1]), un(px[2]), a)
    }

    /// Convert to a tiny-skia colour, clamping every channel into range.
    pub fn to_skia(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba(
            self.r.clamp(0.0, 1.0),
            self.g.clamp(0.0, 1.0),
            self.b.clamp(0.0, 1.0),
            self.a.clamp(0.0, 1.0),
        )
        .expect("clamped channels are always valid")
    }

    /// `#rrggbb` form (alpha is reported separately by callers).
    pub fn hex(self) -> String {
        let q = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("#{:02x}{:02x}{:02x}", q(self.r), q(self.g), q(self.b))
    }

    pub fn to_oklab(self) -> Oklab {
        let r = srgb_to_linear(self.r);
        let g = srgb_to_linear(self.g);
        let b = srgb_to_linear(self.b);
        let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
        let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
        let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
        Oklab {
            l: 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
            a: 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
            b: 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
        }
    }

    /// Lightness (OKLab L), the perceptual luminance used by `dark()`/`light()`.
    pub fn lightness(self) -> f32 {
        self.to_oklab().l
    }

    /// OKLCH as (L, C, H in degrees 0..360).
    pub fn to_oklch(self) -> (f32, f32, f32) {
        let lab = self.to_oklab();
        let c = (lab.a * lab.a + lab.b * lab.b).sqrt();
        let h = lab.b.atan2(lab.a).to_degrees().rem_euclid(360.0);
        (lab.l, c, h)
    }

    /// Mix two colours in OKLab (`t` = 0 gives `self`, 1 gives `other`).
    ///
    /// Interpolation is premultiplied: each endpoint's OKLab is weighted by
    /// its alpha, so mixing toward `transparent` fades the colour out instead
    /// of dragging it toward black.
    pub fn mix(self, other: Rgba, t: f32) -> Rgba {
        let (p, q) = (self.to_oklab(), other.to_oklab());
        let (w0, w1) = (self.a * (1.0 - t), other.a * t);
        let alpha = self.a + (other.a - self.a) * t;
        let total = w0 + w1;
        if total <= 1e-6 {
            // Both weights vanish: pick the nearer endpoint's colour.
            let lab = if t < 0.5 { p } else { q };
            return Oklab::to_rgba_gamut(lab, alpha);
        }
        let avg = |x: f32, y: f32| (x * w0 + y * w1) / total;
        let lab = Oklab {
            l: avg(p.l, q.l),
            a: avg(p.a, q.a),
            b: avg(p.b, q.b),
        };
        Oklab::to_rgba_gamut(lab, alpha)
    }

    /// Apply per-mark colour jitter.
    ///
    /// `d` is (dL, radial chroma offset, tangential chroma offset, dH deg),
    /// with both chroma offsets drawn from +-`chroma_amp`. The chroma change
    /// is an offset in the OKLab a/b plane: radial along the colour's hue,
    /// plus a tangential part that fades in as chroma drops below
    /// `chroma_amp`. Near-neutral colours therefore scatter in every hue
    /// direction around grey (no fixed cast), saturated colours only change
    /// chroma, and nothing is clamped, so the mean colour is unchanged.
    pub fn jittered(self, d: [f32; 4], chroma_amp: f32) -> Rgba {
        let (l, c, h) = self.to_oklch();
        let hr = (h + d[3]).to_radians();
        let k = if chroma_amp > 0.0 {
            (1.0 - c / chroma_amp).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (radial, tangential) = (c + d[1], d[2] * k);
        let (sin, cos) = hr.sin_cos();
        Oklab {
            l: l + d[0],
            a: radial * cos - tangential * sin,
            b: radial * sin + tangential * cos,
        }
        .to_rgba_gamut(self.a)
    }
}

impl Oklab {
    fn to_linear(self) -> [f32; 3] {
        let l = (self.l + 0.396_337_78 * self.a + 0.215_803_76 * self.b).powi(3);
        let m = (self.l - 0.105_561_346 * self.a - 0.063_854_17 * self.b).powi(3);
        let s = (self.l - 0.089_484_18 * self.a - 1.291_485_5 * self.b).powi(3);
        [
            4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
            -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
            -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
        ]
    }

    /// Convert to sRGB, reducing chroma (keeping L and hue) until the colour
    /// fits the sRGB gamut.
    pub fn to_rgba_gamut(self, alpha: f32) -> Rgba {
        const EPS: f32 = 1e-4;
        let in_gamut = |lin: [f32; 3]| lin.iter().all(|&c| (-EPS..=1.0 + EPS).contains(&c));
        let l = self.l.clamp(0.0, 1.0);
        let mut lin = Oklab { l, ..self }.to_linear();
        if !in_gamut(lin) {
            let (mut lo, mut hi) = (0.0f32, 1.0f32);
            for _ in 0..14 {
                let mid = (lo + hi) * 0.5;
                let probe = Oklab {
                    l,
                    a: self.a * mid,
                    b: self.b * mid,
                }
                .to_linear();
                if in_gamut(probe) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            lin = Oklab {
                l,
                a: self.a * lo,
                b: self.b * lo,
            }
            .to_linear();
        }
        let enc = |c: f32| linear_to_srgb(c.clamp(0.0, 1.0));
        Rgba::new(enc(lin[0]), enc(lin[1]), enc(lin[2]), alpha.clamp(0.0, 1.0))
    }
}

/// Build a colour from OKLCH (hue in degrees), gamut-mapped into sRGB.
pub fn oklch(l: f32, c: f32, h: f32, alpha: f32) -> Rgba {
    let hr = h.to_radians();
    Oklab {
        l,
        a: c * hr.cos(),
        b: c * hr.sin(),
    }
    .to_rgba_gamut(alpha)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    (r + m, g + m, b + m)
}

fn parse_num(s: &str) -> Result<f32, String> {
    let t = s.trim();
    t.parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("expected a number, got '{t}'"))
}

/// Parse a percentage-or-fraction: `50%` -> 0.5, `0.5` -> 0.5.
fn parse_fraction(s: &str) -> Result<f32, String> {
    let t = s.trim();
    match t.strip_suffix('%') {
        Some(p) => Ok(parse_num(p)? / 100.0),
        None => parse_num(t),
    }
}

/// Split `name(args)` into (name, args) when `s` has that form.
pub fn call_form(s: &str) -> Option<(&str, &str)> {
    let open = s.find('(')?;
    let inner = s[open + 1..].strip_suffix(')')?;
    Some((&s[..open], inner))
}

fn expect_args<'a>(func: &str, inner: &'a str, count: &[usize]) -> Result<Vec<&'a str>, String> {
    let args = split_top_level(inner, ',');
    if count.contains(&args.len()) {
        Ok(args)
    } else {
        let want: Vec<String> = count.iter().map(|c| c.to_string()).collect();
        Err(format!(
            "{func}() takes {} arguments, got {}",
            want.join(" or "),
            args.len()
        ))
    }
}

fn parse_hex(hex: &str) -> Result<Rgba, String> {
    let digits: Vec<u8> = hex
        .chars()
        .map(|c| c.to_digit(16).map(|d| d as u8))
        .collect::<Option<_>>()
        .ok_or_else(|| format!("bad hex colour '#{hex}'"))?;
    let byte = |hi: u8, lo: u8| (hi * 16 + lo) as f32 / 255.0;
    match digits.as_slice() {
        [r, g, b] => Ok(Rgba::new(byte(*r, *r), byte(*g, *g), byte(*b, *b), 1.0)),
        [r1, r2, g1, g2, b1, b2] => Ok(Rgba::new(
            byte(*r1, *r2),
            byte(*g1, *g2),
            byte(*b1, *b2),
            1.0,
        )),
        [r1, r2, g1, g2, b1, b2, a1, a2] => Ok(Rgba::new(
            byte(*r1, *r2),
            byte(*g1, *g2),
            byte(*b1, *b2),
            byte(*a1, *a2),
        )),
        _ => Err(format!("hex colour '#{hex}' must have 3, 6 or 8 digits")),
    }
}

/// Parse one colour value (variables must already be substituted).
///
/// # Errors
///
/// Returns a human-readable message naming the malformed part.
pub fn parse_color(s: &str) -> Result<Rgba, String> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    match s {
        "black" => return Ok(Rgba::new(0.0, 0.0, 0.0, 1.0)),
        "white" => return Ok(Rgba::new(1.0, 1.0, 1.0, 1.0)),
        "transparent" => return Ok(Rgba::TRANSPARENT),
        _ => {}
    }
    let Some((func, inner)) = call_form(s) else {
        return Err(format!(
            "unknown colour '{s}' (use #rrggbb, rgb(), rgba(), hsl(), oklch(), mix(), \
             black, white, transparent)"
        ));
    };
    match func {
        "rgb" | "rgba" => {
            let args = expect_args(func, inner, &[3, 4])?;
            let ch = |v: &str| parse_num(v).map(|n| (n / 255.0).clamp(0.0, 1.0));
            let a = match args.get(3) {
                Some(v) => parse_fraction(v)?.clamp(0.0, 1.0),
                None => 1.0,
            };
            Ok(Rgba::new(ch(args[0])?, ch(args[1])?, ch(args[2])?, a))
        }
        "hsl" => {
            let args = expect_args(func, inner, &[3])?;
            let (r, g, b) = hsl_to_rgb(
                parse_num(args[0])?,
                parse_fraction(args[1])?.clamp(0.0, 1.0),
                parse_fraction(args[2])?.clamp(0.0, 1.0),
            );
            Ok(Rgba::new(r, g, b, 1.0))
        }
        "oklch" => {
            let args = expect_args(func, inner, &[3, 4])?;
            let a = match args.get(3) {
                Some(v) => parse_fraction(v)?,
                None => 1.0,
            };
            Ok(oklch(
                parse_fraction(args[0])?,
                parse_num(args[1])?.max(0.0),
                parse_num(args[2])?,
                a,
            ))
        }
        "mix" => {
            let args = expect_args(func, inner, &[3])?;
            let t = parse_fraction(args[2])?;
            Ok(parse_color(args[0])?.mix(parse_color(args[1])?, t))
        }
        _ => Err(format!("unknown colour function '{func}()'")),
    }
}

/// Parse a weighted colour list: `#c33*3,#36c,oklch(.7,.1,40)*2`.
///
/// # Errors
///
/// Fails on an empty list, a malformed colour or a non-positive weight.
pub fn parse_colors(s: &str) -> Result<Vec<(Rgba, f32)>, String> {
    let items = split_top_level(s, ',');
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let item = item.trim();
        if item.is_empty() {
            return Err("empty item in list; whitespace ends a value, so remove any space after ',' (and note that ' #' starts a comment)".to_string());
        }
        let (color, weight) = match item.rfind('*') {
            // A '*' after the last ')' is a weight; one inside parens is not.
            Some(star) if item.rfind(')').is_none_or(|close| star > close) => {
                (&item[..star], parse_num(&item[star + 1..])?)
            }
            _ => (item, 1.0),
        };
        if weight <= 0.0 {
            return Err(format!("colour weight must be > 0 in '{item}'"));
        }
        out.push((parse_color(color)?, weight));
    }
    if out.is_empty() {
        return Err("empty colour list".to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 2e-3
    }

    #[test]
    fn hex_forms() {
        assert_eq!(parse_color("#fff").unwrap(), Rgba::new(1.0, 1.0, 1.0, 1.0));
        let c = parse_color("#ff000080").unwrap();
        assert!(close(c.r, 1.0) && close(c.a, 128.0 / 255.0));
        assert!(parse_color("#12345").is_err());
    }

    #[test]
    fn functional_forms() {
        let c = parse_color("rgb(255, 0, 0)").unwrap();
        assert_eq!(c.hex(), "#ff0000");
        assert_eq!(parse_color("rgba(0,0,255,0.5)").unwrap().a, 0.5);
        assert_eq!(parse_color("hsl(120,100%,50%)").unwrap().hex(), "#00ff00");
        let w = parse_color("oklch(1,0,0)").unwrap();
        assert_eq!(w.hex(), "#ffffff");
        let m = parse_color("mix(black,white,0.5)").unwrap();
        assert!(close(m.to_oklab().l, 0.5));
        assert!(parse_color("rgb(1,2)").is_err());
        assert!(parse_color("banana").is_err());
    }

    #[test]
    fn oklab_round_trip() {
        let c = Rgba::new(0.8, 0.4, 0.2, 1.0);
        let (l, ch, h) = c.to_oklch();
        let back = oklch(l, ch, h, 1.0);
        assert!(close(back.r, c.r) && close(back.g, c.g) && close(back.b, c.b));
    }

    #[test]
    fn out_of_gamut_keeps_lightness() {
        let c = oklch(0.7, 0.4, 150.0, 1.0);
        assert!(close(c.lightness(), 0.7));
    }

    #[test]
    fn mix_toward_transparent_keeps_colour() {
        let white = Rgba::new(1.0, 1.0, 1.0, 1.0);
        for t in [0.1, 0.5, 0.9] {
            let m = white.mix(Rgba::TRANSPARENT, t);
            assert_eq!(m.hex(), "#ffffff", "t={t}");
            assert!(close(m.a, 1.0 - t));
        }
        let half_red = Rgba::new(1.0, 0.0, 0.0, 0.5);
        assert_eq!(half_red.mix(Rgba::TRANSPARENT, 0.5).hex(), "#ff0000");
    }

    #[test]
    fn jitter_on_grey_has_no_cast() {
        let grey = Rgba::new(0.5, 0.5, 0.5, 1.0);
        let (mut sa, mut sb) = (0.0f32, 0.0f32);
        let n = 20;
        for i in 0..n {
            for j in 0..n {
                let u = |k: usize| (k as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                let lab = grey
                    .jittered([0.0, 0.04 * u(i), 0.04 * u(j), 0.0], 0.04)
                    .to_oklab();
                sa += lab.a;
                sb += lab.b;
            }
        }
        let count = (n * n) as f32;
        assert!(
            (sa / count).abs() < 2e-3 && (sb / count).abs() < 2e-3,
            "{sa} {sb}"
        );
    }

    #[test]
    fn weighted_list() {
        let list = parse_colors("#c33*3,#36c,rgb(1,2,3)*2").unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].1, 3.0);
        assert_eq!(list[1].1, 1.0);
        assert_eq!(list[2].1, 2.0);
        assert!(parse_colors("#c33*0").is_err());
        assert!(parse_colors("#fff,")
            .unwrap_err()
            .contains("remove any space"));
    }
}
