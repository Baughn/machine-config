//! Mark-placing ops (`dot`, `stipple`, `pointillize`, `dabs`) and the mark
//! rasteriser.
//!
//! Determinism: every random draw happens in canvas coordinates and in a
//! fixed order that never depends on raster lookups, so rendering at another
//! scale consumes the same random sequence. Raster-dependent decisions
//! (weight, clip, source alpha) use pre-drawn rolls.

use std::f32::consts::{PI, SQRT_2, TAU};

use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rayon::prelude::*;
use tiny_skia::{Mask, Pixmap};

use crate::color::{self, Rgba};
use crate::dsl::{parse_path, Args, Pt, Scalar};
use crate::raster;
use crate::render::RenderState;
use crate::render::{self, Viewport};
use crate::shape::{self, PathSpec, Polyline, Shape};

/// Refuse ops that would place more candidates than this.
const MAX_CANDIDATES: usize = 5_000_000;

/// Bridson sampling at spacing `d` yields about `BRIDSON_DENSITY / d²`
/// points per px² (measured; see `poisson_density_calibration`). Derived
/// spacings aim ~15% above the requested count, then subsample down to it.
const BRIDSON_DENSITY: f32 = 0.63;
const EVEN_OVERSHOOT: f32 = 1.15;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MarkKind {
    Circle,
    Square,
    Dab,
    Line,
}

/// A single mark, in canvas coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mark {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    /// Radians, clockwise from +x (y points down).
    pub angle: f32,
    pub aspect: f32,
    pub alpha: f32,
    pub soft: f32,
    pub kind: MarkKind,
    pub color: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Angle {
    Value(Scalar),
    /// Along the source's contours (pointillize).
    Flow,
    /// Along the path (dabs).
    Follow,
}

enum Palette {
    Single(Rgba),
    Weighted {
        colors: Vec<Rgba>,
        cumulative: Vec<f32>,
    },
    /// Sample the colour from this layer (pointillize).
    Source(usize),
}

#[derive(Clone, Copy)]
enum WeightKind {
    Dark,
    Light,
    Alpha,
}

struct Weight {
    kind: WeightKind,
    layer: usize,
    gamma: f32,
}

/// Mark appearance parameters shared by all mark ops.
struct Style {
    r: Scalar,
    alpha: Scalar,
    aspect: Scalar,
    soft: Scalar,
    kind: MarkKind,
    angle: Angle,
    palette: Palette,
    jitter: [f32; 3],
    clip: Option<usize>,
}

/// Per-candidate random attributes, drawn before any acceptance test.
struct Attrs {
    r: f32,
    alpha: f32,
    aspect: f32,
    soft: f32,
    angle_deg: f32,
    pick: f32,
    /// dL, radial and tangential chroma offsets, dH (see `Rgba::jittered`).
    jitter: [f32; 4],
}

/// Where a mark may go, plus path-derived modifiers (dabs).
struct Site {
    p: Pt,
    tangent_deg: Option<f32>,
    size_mul: f32,
    /// Fraction of the path (0..1) for ramps; 0 off paths.
    u: f32,
}

impl Site {
    fn at(p: Pt) -> Self {
        Site {
            p,
            tangent_deg: None,
            size_mul: 1.0,
            u: 0.0,
        }
    }
}

/// Acceptance tests beyond the style's `clip`.
#[derive(Default)]
struct Acceptance<'a> {
    falloff: Option<(&'a Shape, f32)>,
    weight: Option<Weight>,
    min_source_alpha: Option<f32>,
}

#[derive(Default)]
struct Stats {
    candidates: usize,
    falloff: usize,
    weight: usize,
    clip: usize,
    source: usize,
}

/// A non-negative number or range; with `along`, also a ramp (dabs).
fn nonneg_scalar(args: &Args, key: &str, default: Scalar, along: bool) -> Result<Scalar, String> {
    let v = if along {
        args.ramp(key)?
    } else {
        args.scalar(key)?
    }
    .unwrap_or(default);
    if v.min() < 0.0 {
        return Err(format!("key '{key}': must not be negative"));
    }
    Ok(v)
}

fn layer_ref(state: &RenderState, name: &str) -> Result<usize, String> {
    state.layer_named(name)?;
    Ok(state.layer_index(name).expect("checked above"))
}

impl Style {
    fn parse(state: &RenderState, op: &str, args: &Args) -> Result<Style, String> {
        let kind = match args.raw("mark").unwrap_or("circle") {
            "circle" => MarkKind::Circle,
            "square" => MarkKind::Square,
            "dab" => MarkKind::Dab,
            "line" => MarkKind::Line,
            other => {
                return Err(format!(
                    "key 'mark': expected circle, square, dab or line, got '{other}'"
                ))
            }
        };
        let default_angle = match kind {
            MarkKind::Dab | MarkKind::Line => Scalar::Range(0.0, 180.0),
            MarkKind::Circle | MarkKind::Square => Scalar::Fixed(0.0),
        };
        let angle = match args.raw("angle") {
            None => Angle::Value(default_angle),
            Some("flow") if op == "pointillize" => Angle::Flow,
            Some("follow") if op == "dabs" => Angle::Follow,
            Some(v @ ("flow" | "follow")) => {
                return Err(format!(
                    "key 'angle': '{v}' is only valid for {}",
                    if v == "flow" { "pointillize" } else { "dabs" }
                ))
            }
            Some(_) => Angle::Value(args.scalar("angle")?.expect("present")),
        };
        args.exclusive(&["color", "colors"])?;
        let palette = if op == "pointillize" {
            if args.has("color") || args.has("colors") {
                return Err("pointillize takes its colours from source=, not color=".to_string());
            }
            let source = args
                .name("source")?
                .expect("required key checked by parser");
            Palette::Source(layer_ref(state, &source)?)
        } else if let Some(c) = args.color("color")? {
            Palette::Single(c)
        } else if let Some(list) = args.get("colors", color::parse_colors)? {
            let mut total = 0.0;
            let cumulative = list
                .iter()
                .map(|(_, w)| {
                    total += w;
                    total
                })
                .collect();
            Palette::Weighted {
                colors: list.iter().map(|(c, _)| *c).collect(),
                cumulative,
            }
        } else {
            return Err("needs color= or colors=".to_string());
        };
        let jitter = args
            .get("jitter", |v| {
                let parts: Vec<f32> = v
                    .split(',')
                    .map(crate::dsl::parse_number)
                    .collect::<Result<_, _>>()?;
                match parts.as_slice() {
                    [l, c, h] if *l >= 0.0 && *c >= 0.0 && *h >= 0.0 => Ok([*l, *c, *h]),
                    _ => Err(format!(
                        "expected three non-negative numbers dL,dC,dH, got '{v}'"
                    )),
                }
            })?
            .unwrap_or([0.0; 3]);
        let clip = match args.name("clip")? {
            Some(name) => Some(layer_ref(state, &name)?),
            None => None,
        };
        let along = op == "dabs";
        let alpha = nonneg_scalar(args, "alpha", Scalar::Fixed(1.0), along)?;
        if alpha.max() > 1.0 {
            return Err("key 'alpha': must be within 0..1".to_string());
        }
        let aspect = nonneg_scalar(args, "aspect", Scalar::Fixed(2.5), false)?;
        if aspect.min() <= 0.0 {
            return Err("key 'aspect': must be > 0".to_string());
        }
        Ok(Style {
            r: nonneg_scalar(args, "r", Scalar::Fixed(2.0), along)?,
            alpha,
            aspect,
            soft: nonneg_scalar(args, "soft", Scalar::Fixed(0.0), false)?,
            kind,
            angle,
            palette,
            jitter,
            clip,
        })
    }

    /// Draw every random attribute of one mark, in a fixed order; `u` is
    /// the mark's path fraction, for ramps.
    fn draw_attrs(&self, rng: &mut impl Rng, u: f32) -> Attrs {
        let r = self.r.sample_at(rng, u);
        let alpha = self.alpha.sample_at(rng, u);
        let aspect = self.aspect.sample(rng);
        let soft = self.soft.sample(rng);
        let angle_deg = match self.angle {
            Angle::Value(s) => s.sample(rng),
            // Fallback for flat regions / unused: drawn so the sequence is stable.
            Angle::Flow | Angle::Follow => Scalar::Range(0.0, 180.0).sample(rng),
        };
        let pick = match self.palette {
            Palette::Weighted { .. } => rng.random::<f32>(),
            Palette::Single(_) | Palette::Source(_) => 0.0,
        };
        let jitter = if self.jitter == [0.0; 3] {
            [0.0; 4]
        } else {
            let [dl, dc, dh] = self.jitter;
            let mut j = [0.0; 4];
            for (v, amp) in j.iter_mut().zip([dl, dc, dc, dh]) {
                *v = (rng.random::<f32>() * 2.0 - 1.0) * amp;
            }
            j
        };
        Attrs {
            r,
            alpha,
            aspect,
            soft,
            angle_deg,
            pick,
            jitter,
        }
    }
}

/// Luminance field for `angle=flow`: the source composited over mid grey,
/// blurred so marks follow smoothed contours. Only the area around the
/// marks is computed. Fixed point, so a region render computes exactly the
/// same angles as the full render.
///
/// The gradient is taken across one canvas pixel (`step` device pixels) on
/// each side, so its size does not shrink with the render scale; across one
/// device pixel, a 16x render's gradient was mostly blur rounding.
struct FlowField {
    /// Source (layer) size, for clamping lookups.
    width: usize,
    height: usize,
    /// Device pixels between a cell and the neighbours its gradient uses.
    step: usize,
    /// The computed area, in layer pixels.
    area: raster::PxRect,
    lum: Vec<i32>,
}

/// Fixed-point factor for luminance (0..1.5) in the flow field.
const LUM_FIXED: f32 = 65536.0;

impl FlowField {
    /// The field of `source` (rendered at `scale`) around the device
    /// points `at`.
    fn new(source: &Pixmap, sigma: f32, scale: f32, at: impl Iterator<Item = (f32, f32)>) -> Self {
        let (width, height) = (source.width() as usize, source.height() as usize);
        let step = (scale.round() as usize).max(1);
        let mut area: Option<raster::PxRect> = None;
        // Too small for a gradient: `angle_at` always falls back.
        let at = at.filter(|_| Self::fits(width, height, step));
        for (x, y) in at {
            let (xi, yi) = Self::cell(width, height, step, x, y);
            area = Some(match area {
                None => (xi, yi, xi + 1, yi + 1),
                Some((x0, y0, x1, y1)) => (x0.min(xi), y0.min(yi), x1.max(xi + 1), y1.max(yi + 1)),
            });
        }
        let area = raster::grow_rect(
            area.unwrap_or((0, 0, 0, 0)),
            raster::blur_support(sigma) + step + 1,
            width,
            height,
        );
        let (x0, y0, x1, y1) = area;
        let mut lum = Vec::with_capacity((x1 - x0) * (y1 - y0));
        for y in y0..y1 {
            let row = &source.data()[(y * width + x0) * 4..(y * width + x1) * 4];
            lum.extend(row.as_chunks::<4>().0.iter().map(|p| {
                let (r, g, b, a) = (
                    p[0] as f32 / 255.0,
                    p[1] as f32 / 255.0,
                    p[2] as f32 / 255.0,
                    p[3] as f32 / 255.0,
                );
                let l = 0.2126 * r + 0.7152 * g + 0.0722 * b + 0.5 * (1.0 - a);
                (l * LUM_FIXED).round() as i32
            }));
        }
        raster::blur::<1>(&mut lum, x1 - x0, y1 - y0, sigma);
        FlowField {
            width,
            height,
            step,
            area,
            lum,
        }
    }

    /// Whether a `width x height` layer has room for a gradient.
    fn fits(width: usize, height: usize, step: usize) -> bool {
        width > 2 * step && height > 2 * step
    }

    /// The layer pixel whose neighbours give the gradient at `x, y`.
    fn cell(width: usize, height: usize, step: usize, x: f32, y: f32) -> (usize, usize) {
        let s = step as isize;
        (
            (x.floor() as isize).clamp(s, width as isize - 1 - s) as usize,
            (y.floor() as isize).clamp(s, height as isize - 1 - s) as usize,
        )
    }

    /// Contour direction in degrees at device coordinates, if well defined.
    fn angle_at(&self, x: f32, y: f32) -> Option<f32> {
        let s = self.step;
        if !Self::fits(self.width, self.height, s) {
            return None;
        }
        let (xi, yi) = Self::cell(self.width, self.height, s, x, y);
        let (x0, y0, x1, _) = self.area;
        let at = |x: usize, y: usize| self.lum[(y - y0) * (x1 - x0) + x - x0];
        let gx = at(xi + s, yi) - at(xi - s, yi);
        let gy = at(xi, yi + s) - at(xi, yi - s);
        if gx == 0 && gy == 0 {
            return None;
        }
        Some((gy as f32).atan2(gx as f32).to_degrees() + 90.0)
    }
}

/// Run one of the mark ops against `state`, drawing onto the current layer.
///
/// # Errors
///
/// Returns a message for invalid keys/values or missing layers.
pub fn run_mark_op(
    state: &mut RenderState,
    op: &str,
    args: &Args,
    seed: u64,
) -> Result<String, String> {
    state
        .current
        .ok_or("no current layer; create one with `layer name=...`")?;
    let style = Style::parse(state, op, args)?;
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut note = String::new();
    let region;
    let mut accept = Acceptance::default();
    let mut unplaced = 0;
    let sites = match op {
        "dot" => args
            .need("at", parse_path)?
            .into_iter()
            .map(Site::at)
            .collect(),
        "dabs" => dab_sites(state, args, &style, &mut rng)?,
        "stipple" | "pointillize" => {
            region = match state.parse_shape(args, "in")? {
                Some(s) => s,
                None => Shape::parse("canvas", (state.width as f32, state.height as f32))?,
            };
            let placed = area_sites(state, args, &region, op == "pointillize", &mut rng)?;
            note = placed.note;
            unplaced = placed.unplaced;
            if let Some(f) = args.number_in("falloff", 0.0, f32::MAX)? {
                accept.falloff = (f > 0.0).then_some((&region, f));
            }
            accept.weight = parse_weight(state, args)?;
            if op == "pointillize" {
                accept.min_source_alpha =
                    Some(args.number_in("min-alpha", 0.0, 1.0)?.unwrap_or(0.5));
            }
            placed.points.into_iter().map(Site::at).collect()
        }
        other => return Err(format!("'{other}' is not a mark op")),
    };
    let (marks, mut stats) = generate(state, &style, &accept, &sites, &mut rng);
    stats.candidates += unplaced;
    let view = state.view;
    let layer = state.current_layer_mut()?;
    draw_marks(&mut layer.pixmap, &marks, &view);
    Ok(report(marks.len(), &stats, &note))
}

fn report(placed: usize, stats: &Stats, note: &str) -> String {
    let mut out = format!("-> {placed} marks");
    let rejected: Vec<String> = [
        (stats.falloff, "falloff"),
        (stats.weight, "weight"),
        (stats.clip, "clip"),
        (stats.source, "source alpha"),
    ]
    .iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, what)| format!("{n} by {what}"))
    .collect();
    let mut details = Vec::new();
    if !rejected.is_empty() {
        details.push(format!(
            "{} candidates, rejected {}",
            stats.candidates,
            rejected.join(", ")
        ));
    }
    if !note.is_empty() {
        details.push(note.to_string());
    }
    if !details.is_empty() {
        out.push_str(&format!(" ({})", details.join("; ")));
    }
    out
}

fn parse_weight(state: &RenderState, args: &Args) -> Result<Option<Weight>, String> {
    let gamma = args.number_in("gamma", 0.01, 100.0)?;
    let Some(raw) = args.raw("weight") else {
        if gamma.is_some() {
            return Err("gamma= only applies together with weight=".to_string());
        }
        return Ok(None);
    };
    let (func, inner) = color::call_form(raw).ok_or_else(|| {
        format!("key 'weight': expected dark(L), light(L) or alpha(L), got '{raw}'")
    })?;
    let kind = match func {
        "dark" => WeightKind::Dark,
        "light" => WeightKind::Light,
        "alpha" => WeightKind::Alpha,
        other => return Err(format!("key 'weight': unknown function '{other}()'")),
    };
    Ok(Some(Weight {
        kind,
        layer: layer_ref(state, inner).map_err(|e| format!("key 'weight': {e}"))?,
        gamma: gamma.unwrap_or(1.0),
    }))
}

/// Candidate positions for stipple/pointillize.
struct AreaSites {
    points: Vec<Pt>,
    note: String,
    /// Requested candidates that random placement could not fit in the shape.
    unplaced: usize,
}

fn area_sites(
    state: &RenderState,
    args: &Args,
    region: &Shape,
    default_even: bool,
    rng: &mut ChaCha8Rng,
) -> Result<AreaSites, String> {
    let sites = |points, note: String| AreaSites {
        points,
        note,
        unplaced: 0,
    };
    args.exclusive(&["count", "density"])?;
    let even = match args.raw("placement") {
        None => default_even,
        Some("even") => true,
        Some("random") => false,
        Some(other) => {
            return Err(format!(
                "key 'placement': expected random or even, got '{other}'"
            ))
        }
    };
    let canvas = state.canvas_bounds();
    let area = shape::clipped_area(region, canvas);
    let target = match (args.number("count")?, args.number("density")?) {
        (Some(c), _) if c < 0.0 => return Err("key 'count': must not be negative".to_string()),
        (Some(c), _) => Some(c.round() as usize),
        (None, Some(d)) if d <= 0.0 => return Err("key 'density': must be > 0".to_string()),
        (None, Some(d)) => Some((d * area / 1000.0).round() as usize),
        (None, None) => None,
    };
    if target.is_some_and(|t| t > MAX_CANDIDATES) {
        return Err(format!(
            "{} candidates requested; the limit is {MAX_CANDIDATES}",
            target.unwrap_or_default()
        ));
    }
    let (sx0, sy0, sx1, sy1) = region.bounds();
    let bounds = (
        sx0.max(canvas.0),
        sy0.max(canvas.1),
        sx1.min(canvas.2),
        sy1.min(canvas.3),
    );
    if bounds.2 <= bounds.0 || bounds.3 <= bounds.1 || area <= 0.0 {
        return Ok(sites(Vec::new(), "shape is outside the canvas".to_string()));
    }
    if !even {
        if args.has("spacing") {
            return Err("spacing= only applies to placement=even".to_string());
        }
        let target = target.ok_or("needs count= or density=")?;
        let bbox_area = (bounds.2 - bounds.0) * (bounds.3 - bounds.1);
        let points = random_in_shape(region, bounds, target, bbox_area / area, rng);
        let unplaced = target - points.len();
        let note = if unplaced > 0 {
            format!(
                "{unplaced} of {target} not placed: the shape fills only {:.1}% of its bbox",
                100.0 * area / bbox_area
            )
        } else {
            String::new()
        };
        return Ok(AreaSites {
            points,
            note,
            unplaced,
        });
    }
    let spacing = match (args.number("spacing")?, target) {
        (Some(s), _) if s <= 0.0 => return Err("key 'spacing': must be > 0".to_string()),
        (Some(s), _) => s,
        (None, Some(0)) => return Ok(sites(Vec::new(), String::new())),
        (None, Some(t)) => (BRIDSON_DENSITY * area / (t as f32 * EVEN_OVERSHOOT)).sqrt(),
        (None, None) => return Err("needs count=, density= or spacing=".to_string()),
    };
    let mut points = poisson(bounds, spacing, rng)?;
    points.retain(|p| region.contains(*p));
    // Bridson grows outwards from a seed; shuffling hides that order.
    points.shuffle(rng);
    if let Some(t) = target {
        points.truncate(t);
    }
    Ok(sites(points, format!("even spacing {spacing:.2}px")))
}

/// Uniform rejection sampling of `count` points inside `region`.
///
/// `sparsity` is bbox area / shape area. The attempt budget scales with it,
/// so thin or diagonal shapes still get their full count; only degenerate
/// shapes (or budget exhaustion) return fewer points.
fn random_in_shape(
    region: &Shape,
    bounds: shape::Bounds,
    count: usize,
    sparsity: f32,
    rng: &mut ChaCha8Rng,
) -> Vec<Pt> {
    const MAX_ATTEMPTS: usize = 200_000_000;
    let (x0, y0, x1, y1) = bounds;
    // The grid-sampled area overestimates slivers, so allow generous slack.
    let per_point = (sparsity * 16.0).ceil().clamp(64.0, 1e6) as usize;
    let budget = count
        .saturating_mul(per_point)
        .min(MAX_ATTEMPTS.max(count.saturating_mul(64)));
    let mut out = Vec::with_capacity(count);
    for _ in 0..budget {
        if out.len() == count {
            break;
        }
        let p = Pt::new(
            x0 + (x1 - x0) * rng.random::<f32>(),
            y0 + (y1 - y0) * rng.random::<f32>(),
        );
        if region.contains(p) {
            out.push(p);
        }
    }
    out
}

/// Bridson's Poisson-disk sampling over `bounds` with minimum distance `d`.
fn poisson(bounds: shape::Bounds, d: f32, rng: &mut ChaCha8Rng) -> Result<Vec<Pt>, String> {
    const ATTEMPTS: usize = 30;
    let (x0, y0, x1, y1) = bounds;
    let (w, h) = (x1 - x0, y1 - y0);
    let expected = (w * h * BRIDSON_DENSITY / (d * d)) as usize;
    if expected > MAX_CANDIDATES {
        return Err(format!(
            "spacing {d:.3}px would place about {expected} marks; the limit is {MAX_CANDIDATES}"
        ));
    }
    let cell = d / SQRT_2;
    let gw = ((w / cell).ceil() as usize).max(1);
    let gh = ((h / cell).ceil() as usize).max(1);
    let mut grid = vec![u32::MAX; gw * gh];
    let cell_of = |p: Pt| {
        (
            (((p.x - x0) / cell) as usize).min(gw - 1),
            (((p.y - y0) / cell) as usize).min(gh - 1),
        )
    };
    let mut points = Vec::with_capacity(expected + 16);
    let mut active = Vec::new();
    let first = Pt::new(x0 + w * rng.random::<f32>(), y0 + h * rng.random::<f32>());
    let (cx, cy) = cell_of(first);
    grid[cy * gw + cx] = 0;
    points.push(first);
    active.push(0u32);
    let d2 = d * d;
    while !active.is_empty() {
        let slot = rng.random_range(0..active.len());
        let base = points[active[slot] as usize];
        let mut found = false;
        for _ in 0..ATTEMPTS {
            let theta = rng.random::<f32>() * TAU;
            let rad = d * (1.0 + rng.random::<f32>());
            let q = Pt::new(base.x + rad * theta.cos(), base.y + rad * theta.sin());
            if q.x < x0 || q.x >= x1 || q.y < y0 || q.y >= y1 {
                continue;
            }
            let (qx, qy) = cell_of(q);
            let clear = (qy.saturating_sub(2)..(qy + 3).min(gh)).all(|gy| {
                (qx.saturating_sub(2)..(qx + 3).min(gw)).all(|gx| {
                    let idx = grid[gy * gw + gx];
                    idx == u32::MAX || {
                        let o = points[idx as usize];
                        (o.x - q.x).powi(2) + (o.y - q.y).powi(2) >= d2
                    }
                })
            });
            if clear {
                grid[qy * gw + qx] = points.len() as u32;
                active.push(points.len() as u32);
                points.push(q);
                found = true;
                break;
            }
        }
        if !found {
            active.swap_remove(slot);
        }
    }
    Ok(points)
}

/// Positions along a path for `dabs`.
fn dab_sites(
    state: &RenderState,
    args: &Args,
    style: &Style,
    rng: &mut ChaCha8Rng,
) -> Result<Vec<Site>, String> {
    let path = args.need("path", |v| {
        PathSpec::parse(v, (state.width as f32, state.height as f32))
    })?;
    if path.pts.is_empty() {
        return Err("key 'path': needs at least one point".to_string());
    }
    args.exclusive(&["spacing", "count"])?;
    let smooth = render::path_smoothing(args, &path)?;
    let taper = args.number_in("taper", 0.0, 1.0)?.unwrap_or(0.0);
    let scatter = nonneg_scalar(args, "scatter", Scalar::Fixed(0.0), true)?;
    let closed = path.closed;
    let line = path.into_polyline(smooth);
    let length = line.length();
    let offsets: Vec<f32> = match (args.number("count")?, args.number("spacing")?) {
        (Some(n), _) if n < 1.0 => return Err("key 'count': must be at least 1".to_string()),
        (Some(n), _) if n > MAX_CANDIDATES as f32 => {
            return Err(format!("key 'count': the limit is {MAX_CANDIDATES}"))
        }
        (Some(n), _) if n < 2.0 && !closed => vec![length / 2.0],
        // A closed path's end is its start: n even gaps all the way round.
        (Some(n), _) if closed => {
            let n = n.round() as usize;
            (0..n).map(|i| length * i as f32 / n as f32).collect()
        }
        (Some(n), _) => {
            let n = n.round() as usize;
            (0..n).map(|i| length * i as f32 / (n - 1) as f32).collect()
        }
        (None, spacing) => {
            let s = spacing.unwrap_or(style.r.max().max(0.5));
            if s <= 0.0 {
                return Err("key 'spacing': must be > 0".to_string());
            }
            let n = if closed {
                (length / s).round().max(1.0) as usize
            } else {
                (length / s).floor() as usize + 1
            };
            if n > MAX_CANDIDATES {
                return Err(format!("{n} dabs requested; the limit is {MAX_CANDIDATES}"));
            }
            // Round a closed path in whole steps, so the seam gets no
            // extra (or missing) dab.
            let step = if closed { length / n as f32 } else { s };
            (0..n).map(|i| i as f32 * step).collect()
        }
    };
    Ok(offsets
        .into_iter()
        .map(|s| {
            let (mut p, tangent) = line.at(s);
            let u = if length > 0.0 { s / length } else { 0.5 };
            if scatter.max() > 0.0 {
                let reach = scatter.sample_at(rng, u);
                let theta = rng.random::<f32>() * TAU;
                let dist = reach * rng.random::<f32>().sqrt();
                p = Pt::new(p.x + dist * theta.cos(), p.y + dist * theta.sin());
            }
            Site {
                p,
                tangent_deg: Some(tangent.to_degrees()),
                size_mul: taper_profile(u, taper),
                u,
            }
        })
        .collect())
}

/// Radius multiplier at path fraction `u`: 1 in the middle, `1 - taper` at
/// both ends, following a sine arch.
pub fn taper_profile(u: f32, taper: f32) -> f32 {
    1.0 - taper * (1.0 - (PI * u.clamp(0.0, 1.0)).sin())
}

/// Turn sites into marks: draw attributes, apply acceptance, resolve colour.
fn generate(
    state: &RenderState,
    style: &Style,
    accept: &Acceptance,
    sites: &[Site],
    rng: &mut ChaCha8Rng,
) -> (Vec<Mark>, Stats) {
    let view = state.view;
    let flow = match (style.angle, &style.palette) {
        (Angle::Flow, Palette::Source(src)) => {
            let reach = style.r.max() * style.aspect.max().max(1.0);
            Some(FlowField::new(
                &state.layers[*src].pixmap,
                reach.max(2.0) * view.scale,
                view.scale,
                sites.iter().map(|site| view.device(site.p)),
            ))
        }
        _ => None,
    };
    let mut stats = Stats {
        candidates: sites.len(),
        ..Stats::default()
    };
    let mut marks = Vec::with_capacity(sites.len());
    for site in sites {
        let attrs = style.draw_attrs(rng, site.u);
        let falloff_roll = accept.falloff.map(|_| rng.random::<f32>());
        let weight_roll = accept.weight.as_ref().map(|_| rng.random::<f32>());
        let clip_roll = style.clip.map(|_| rng.random::<f32>());
        let (dx, dy) = view.device(site.p);

        if let (Some((region, width)), Some(roll)) = (accept.falloff, falloff_roll) {
            let inside = region.contains(site.p);
            let prob = if inside {
                (region.edge_distance(site.p) / width).min(1.0)
            } else {
                0.0
            };
            if roll >= prob {
                stats.falloff += 1;
                continue;
            }
        }
        if let (Some(w), Some(roll)) = (&accept.weight, weight_roll) {
            let c = raster::color_at(&state.layers[w.layer].pixmap, dx, dy);
            let prob = match w.kind {
                WeightKind::Dark => (1.0 - c.lightness()).clamp(0.0, 1.0).powf(w.gamma) * c.a,
                WeightKind::Light => c.lightness().clamp(0.0, 1.0).powf(w.gamma) * c.a,
                WeightKind::Alpha => c.a.powf(w.gamma),
            };
            if roll >= prob {
                stats.weight += 1;
                continue;
            }
        }
        if let (Some(layer), Some(roll)) = (style.clip, clip_roll) {
            let a = raster::pixel_at(&state.layers[layer].pixmap, dx, dy)[3] as f32 / 255.0;
            if roll >= a {
                stats.clip += 1;
                continue;
            }
        }
        let base = match &style.palette {
            Palette::Single(c) => *c,
            Palette::Weighted { colors, cumulative } => {
                let total = cumulative.last().copied().unwrap_or(1.0);
                let target = attrs.pick * total;
                let i = cumulative.partition_point(|&c| c <= target);
                colors[i.min(colors.len() - 1)]
            }
            Palette::Source(src) => {
                let c = raster::color_at(&state.layers[*src].pixmap, dx, dy);
                if c.a < accept.min_source_alpha.unwrap_or(0.0) || c.a <= 0.0 {
                    stats.source += 1;
                    continue;
                }
                Rgba { a: 1.0, ..c }
            }
        };
        let color = if attrs.jitter == [0.0; 4] {
            base
        } else {
            base.jittered(attrs.jitter, style.jitter[1])
        };
        let angle_deg = match style.angle {
            Angle::Value(_) => attrs.angle_deg,
            Angle::Follow => site.tangent_deg.unwrap_or(attrs.angle_deg),
            Angle::Flow => flow
                .as_ref()
                .and_then(|f| f.angle_at(dx, dy))
                .unwrap_or(attrs.angle_deg),
        };
        marks.push(Mark {
            x: site.p.x,
            y: site.p.y,
            r: attrs.r * site.size_mul,
            angle: angle_deg.to_radians(),
            aspect: attrs.aspect,
            alpha: attrs.alpha,
            soft: attrs.soft,
            kind: style.kind,
            color,
        });
    }
    (marks, stats)
}

/// Rows per band when drawing marks in parallel.
const BAND_ROWS: usize = 16;

/// Draw marks in order onto a premultiplied pixmap covering `view`.
///
/// Horizontal bands of the pixmap are drawn in parallel, each with every
/// mark that touches it in the original order, so every pixel sees the
/// same sequence of blends as a sequential draw.
pub fn draw_marks(pixmap: &mut Pixmap, marks: &[Mark], view: &Viewport) {
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    let splats: Vec<Splat> = marks
        .par_iter()
        .filter_map(|m| Splat::new(m, view, w, h))
        .collect();
    let bands = h.div_ceil(BAND_ROWS);
    let mut by_band: Vec<Vec<usize>> = vec![Vec::new(); bands];
    for (i, sp) in splats.iter().enumerate() {
        for band in by_band[sp.y0 / BAND_ROWS..sp.y1.div_ceil(BAND_ROWS)].iter_mut() {
            band.push(i);
        }
    }
    pixmap
        .data_mut()
        .par_chunks_mut(w * 4 * BAND_ROWS)
        .zip(by_band)
        .enumerate()
        .for_each(|(b, (rows, indices))| {
            let top = b * BAND_ROWS;
            for i in indices {
                splats[i].draw(rows, w, top);
            }
        });
}

/// Signed distance (px) from a point in the mark's local frame to its edge.
#[inline]
fn mark_sd(kind: MarkKind, lx: f32, ly: f32, r: f32, aspect: f32) -> f32 {
    match kind {
        MarkKind::Circle => (lx * lx + ly * ly).sqrt() - r,
        MarkKind::Square => {
            let (qx, qy) = (lx.abs() - r, ly.abs() - r);
            let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
            outside + qx.max(qy).min(0.0)
        }
        MarkKind::Dab => {
            let (a, b) = (r * aspect, r);
            let k0 = ((lx / a).powi(2) + (ly / b).powi(2)).sqrt();
            let k1 = ((lx / (a * a)).powi(2) + (ly / (b * b)).powi(2)).sqrt();
            if k1 < 1e-9 {
                -a.min(b)
            } else {
                k0 * (k0 - 1.0) / k1
            }
        }
        MarkKind::Line => {
            let rc = r * 0.5;
            let half = (r * aspect - rc).max(0.0);
            let px = lx - lx.clamp(-half, half);
            (px * px + ly * ly).sqrt() - rc
        }
    }
}

/// One anti-aliased mark, prepared for source-over blending in device
/// pixels: its clipped bounding box and everything its coverage needs.
struct Splat {
    kind: MarkKind,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    cx: f32,
    cy: f32,
    r: f32,
    aspect: f32,
    edge: f32,
    sin: f32,
    cos: f32,
    alpha: f32,
    norm: f32,
    src: [f32; 4],
}

impl Splat {
    /// Prepare `m` for a `pw x ph` pixmap covering `view`; `None` if it
    /// draws nothing there.
    fn new(m: &Mark, view: &Viewport, pw: usize, ph: usize) -> Option<Splat> {
        let scale = view.scale;
        let r = m.r * scale;
        let sa = m.color.a * m.alpha;
        if r <= 0.0 || sa <= 0.0 {
            return None;
        }
        let (cx, cy) = view.device(Pt::new(m.x, m.y));
        let edge = (m.soft * scale).max(1.0);
        let (extent, area) = match m.kind {
            MarkKind::Circle => (r, PI * r * r),
            MarkKind::Square => (r * SQRT_2, 4.0 * r * r),
            MarkKind::Dab => ((r * m.aspect).max(r), PI * r * r * m.aspect),
            MarkKind::Line => {
                let rc = r * 0.5;
                let half = (r * m.aspect - rc).max(0.0);
                (half + rc, 4.0 * half * rc + PI * rc * rc)
            }
        };
        let reach = extent + edge * 0.5 + 0.5;
        let x0 = (cx - reach).floor().max(0.0) as usize;
        let y0 = (cy - reach).floor().max(0.0) as usize;
        let x1 = ((cx + reach).ceil().max(0.0) as usize).min(pw);
        let y1 = ((cy + reach).ceil().max(0.0) as usize).min(ph);
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        let (sin, cos) = m.angle.sin_cos();
        let mut splat = Splat {
            kind: m.kind,
            x0,
            y0,
            x1,
            y1,
            cx,
            cy,
            r,
            aspect: m.aspect,
            edge,
            sin,
            cos,
            alpha: sa,
            norm: 1.0,
            src: [
                m.color.r.clamp(0.0, 1.0) * sa * 255.0,
                m.color.g.clamp(0.0, 1.0) * sa * 255.0,
                m.color.b.clamp(0.0, 1.0) * sa * 255.0,
                sa * 255.0,
            ],
        };
        // Sub-pixel marks: distance-based coverage overestimates their area,
        // so scale coverage down to the analytic area (when the whole mark
        // is on the canvas; clipped tiny marks just keep the raw coverage).
        let full = (x1 - x0) as f32 >= 2.0 * reach - 2.0 && (y1 - y0) as f32 >= 2.0 * reach - 2.0;
        if (x1 - x0) * (y1 - y0) <= 64 && full {
            let sum: f32 = (y0..y1)
                .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                .map(|(x, y)| splat.coverage(x, y))
                .sum();
            if sum > area {
                splat.norm = area / sum;
            }
        }
        Some(splat)
    }

    #[inline]
    fn coverage(&self, x: usize, y: usize) -> f32 {
        let (dx, dy) = (x as f32 + 0.5 - self.cx, y as f32 + 0.5 - self.cy);
        let (lx, ly) = (
            dx * self.cos + dy * self.sin,
            -dx * self.sin + dy * self.cos,
        );
        (0.5 - mark_sd(self.kind, lx, ly, self.r, self.aspect) / self.edge).clamp(0.0, 1.0)
    }

    /// Blend the mark into `rows`, a band of full pixmap rows starting at
    /// row `top`, `pw` pixels wide.
    fn draw(&self, rows: &mut [u8], pw: usize, top: usize) {
        let bottom = top + rows.len() / (pw * 4);
        for y in self.y0.max(top)..self.y1.min(bottom) {
            let start = ((y - top) * pw + self.x0) * 4;
            let row = &mut rows[start..start + (self.x1 - self.x0) * 4];
            for (px, x) in row.as_chunks_mut::<4>().0.iter_mut().zip(self.x0..self.x1) {
                let k = self.coverage(x, y) * self.norm;
                if k <= 0.0 {
                    continue;
                }
                let inv = 1.0 - self.alpha * k;
                for (d, &sc) in px.iter_mut().zip(&self.src) {
                    *d = (sc * k + *d as f32 * inv).round().min(255.0) as u8;
                }
            }
        }
    }
}

/// Coverage mask of a tapered or ramped stroke along `line`, built from a
/// dense chain of discs combined with max (so overlaps do not darken).
///
/// `profile(u)` gives, at path fraction `u`, the radius as a fraction of
/// `half_width` and the stroke's alpha there.
pub fn tapered_coverage(
    line: &Polyline,
    half_width: f32,
    profile: impl Fn(f32) -> (f32, f32),
    view: &Viewport,
) -> Mask {
    let (w, h) = (view.width as usize, view.height as usize);
    let scale = view.scale;
    let mut mask = Mask::new(view.width, view.height).expect("non-zero canvas");
    let data = mask.data_mut();
    let length = line.length();
    let max_r = half_width * scale;
    let step = (0.2 * max_r).clamp(0.35, 4.0) / scale;
    let n = (length / step).ceil() as usize + 1;
    for i in 0..n {
        let s = (i as f32 * step).min(length);
        let (p, _) = line.at(s);
        let u = if length > 0.0 { s / length } else { 0.5 };
        let (radius_mul, alpha) = profile(u);
        let r = max_r * radius_mul;
        if r <= 0.0 || alpha <= 0.0 {
            continue;
        }
        let (cx, cy) = view.device(p);
        let reach = r + 1.0;
        let x0 = (cx - reach).floor().max(0.0) as usize;
        let y0 = (cy - reach).floor().max(0.0) as usize;
        let x1 = ((cx + reach).ceil().max(0.0) as usize).min(w);
        let y1 = ((cy + reach).ceil().max(0.0) as usize).min(h);
        for y in y0..y1 {
            for x in x0..x1 {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                let cov = (0.5 - ((dx * dx + dy * dy).sqrt() - r)).clamp(0.0, 1.0);
                let v = (cov * alpha * 255.0).round() as u8;
                let slot = &mut data[y * w + x];
                *slot = (*slot).max(v);
            }
        }
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poisson_density_calibration() {
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        let pts = poisson((0.0, 0.0, 1000.0, 1000.0), 10.0, &mut rng).unwrap();
        let measured = pts.len() as f32 * 100.0 / 1_000_000.0;
        assert!(
            (measured - BRIDSON_DENSITY).abs() < 0.05,
            "measured density constant {measured}"
        );
        for (i, a) in pts.iter().enumerate().take(300) {
            for b in &pts[i + 1..] {
                assert!((a.x - b.x).powi(2) + (a.y - b.y).powi(2) >= 99.99);
            }
        }
    }

    /// A grey ramp `v(x, y)` (canvas px) rendered at `scale`, 8-bit.
    fn grey_ramp(size: u32, scale: u32, v: impl Fn(f32, f32) -> f32) -> Pixmap {
        let n = size * scale;
        let mut pm = Pixmap::new(n, n).unwrap();
        for (i, px) in pm.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (x, y) = ((i as u32 % n) as f32, (i as u32 / n) as f32);
            let g = v((x + 0.5) / scale as f32, (y + 0.5) / scale as f32).round() as u8;
            *px = [g, g, g, 255];
        }
        pm
    }

    #[test]
    fn flow_angles_are_stable_at_high_scale() {
        // At 16x, neighbouring device pixels of this ramp differ by far
        // less than one 8-bit level. A gradient across one device pixel
        // was then so small that a one-level change in some source pixels
        // (e.g. gradient-shader rounding in a region render) turned the
        // angles by degrees; across one canvas pixel it stays as stable
        // as at 1x.
        let v = |x: f32, y: f32| 80.0 + 0.6 * x + 0.25 * y;
        let want = 0.25f32.atan2(0.6).to_degrees() + 90.0;
        let pts: Vec<(f32, f32)> = (0..25)
            .map(|i| (12.0 + (i % 5) as f32 * 9.0, 12.0 + (i / 5) as f32 * 9.0))
            .collect();
        for scale in [1u32, 16] {
            let s = scale as f32;
            let clean = grey_ramp(64, scale, v);
            let mut noisy = clean.clone();
            for px in noisy
                .data_mut()
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .step_by(7)
            {
                for c in &mut px[..3] {
                    *c = c.saturating_add(1);
                }
            }
            let field = |pm: &Pixmap| {
                FlowField::new(pm, 2.0 * s, s, pts.iter().map(|&(x, y)| (x * s, y * s)))
            };
            let (a, b) = (field(&clean), field(&noisy));
            for &(x, y) in &pts {
                let got = a.angle_at(x * s, y * s).expect("a gradient");
                let other = b.angle_at(x * s, y * s).expect("a gradient");
                assert!((got - want).abs() < 2.0, "scale {scale} at {x},{y}: {got}");
                assert!(
                    (got - other).abs() < 1.0,
                    "scale {scale} at {x},{y}: {got} vs {other} after noise"
                );
            }
        }
    }

    #[test]
    fn taper_profile_shape() {
        assert_eq!(taper_profile(0.5, 1.0), 1.0);
        assert!(taper_profile(0.0, 1.0).abs() < 1e-6);
        assert_eq!(taper_profile(0.0, 0.0), 1.0);
    }

    #[test]
    fn tiny_marks_keep_their_area() {
        let mut pm = Pixmap::new(10, 10).unwrap();
        let mark = Mark {
            x: 5.0,
            y: 5.0,
            r: 0.3,
            angle: 0.0,
            aspect: 1.0,
            alpha: 1.0,
            soft: 0.0,
            kind: MarkKind::Circle,
            color: Rgba::new(1.0, 1.0, 1.0, 1.0),
        };
        draw_marks(&mut pm, &[mark], &Viewport::full(10, 10, 1.0));
        let ink: f32 = pm.data().chunks(4).map(|p| p[3] as f32 / 255.0).sum();
        let area = PI * 0.09;
        assert!((ink - area).abs() < 0.05, "ink {ink} vs area {area}");
    }

    #[test]
    fn mark_shapes_have_expected_extent() {
        assert!(mark_sd(MarkKind::Circle, 2.0, 0.0, 2.0, 1.0).abs() < 1e-6);
        assert!(mark_sd(MarkKind::Square, 2.0, 1.0, 2.0, 1.0).abs() < 1e-6);
        assert!(mark_sd(MarkKind::Dab, 5.0, 0.0, 2.0, 2.5).abs() < 1e-4);
        assert!(mark_sd(MarkKind::Dab, 0.0, 2.0, 2.0, 2.5).abs() < 1e-4);
        // Line: total length r*aspect*2 = 10, width r = 2.
        assert!(mark_sd(MarkKind::Line, 5.0, 0.0, 2.0, 2.5).abs() < 1e-6);
        assert!(mark_sd(MarkKind::Line, 0.0, 1.0, 2.0, 2.5).abs() < 1e-6);
    }
}
