//! Render state (layers, variables), op execution and compositing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tiny_skia::{
    BlendMode, FillRule, FilterQuality, GradientStop, LineCap, LineJoin, LinearGradient, Mask,
    MaskType, Paint, Pixmap, PixmapPaint, Point, RadialGradient, Rect, SpreadMode, Stroke,
    Transform,
};

use crate::color::{self, Rgba};
use crate::dsl::{self, parse_stops, Args, Pt, Scalar, ScriptError, Stmt};
use crate::marks;
use crate::raster;
use crate::shape::{self, Shape};

/// Blend mode names accepted by `layer blend=`.
pub const BLENDS: &[(&str, BlendMode)] = &[
    ("normal", BlendMode::SourceOver),
    ("multiply", BlendMode::Multiply),
    ("screen", BlendMode::Screen),
    ("overlay", BlendMode::Overlay),
    ("darken", BlendMode::Darken),
    ("lighten", BlendMode::Lighten),
    ("add", BlendMode::Plus),
    ("color-dodge", BlendMode::ColorDodge),
    ("color-burn", BlendMode::ColorBurn),
    ("soft-light", BlendMode::SoftLight),
    ("hard-light", BlendMode::HardLight),
    ("difference", BlendMode::Difference),
    ("hue", BlendMode::Hue),
    ("saturation", BlendMode::Saturation),
    ("color", BlendMode::Color),
    ("luminosity", BlendMode::Luminosity),
];

pub fn blend_by_name(name: &str) -> Option<BlendMode> {
    BLENDS.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

pub fn blend_name(mode: BlendMode) -> &'static str {
    BLENDS
        .iter()
        .find(|(_, b)| *b == mode)
        .map_or("normal", |(n, _)| n)
}

/// Layer properties without pixels (what the cache stores as JSON).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerMeta {
    pub name: String,
    pub opacity: f32,
    pub blend: String,
    pub visible: bool,
}

/// One raster layer: premultiplied RGBA at the render scale.
#[derive(Clone)]
pub struct Layer {
    pub name: String,
    pub opacity: f32,
    pub blend: BlendMode,
    pub visible: bool,
    pub pixmap: Pixmap,
}

impl Layer {
    pub fn meta(&self) -> LayerMeta {
        LayerMeta {
            name: self.name.clone(),
            opacity: self.opacity,
            blend: blend_name(self.blend).to_string(),
            visible: self.visible,
        }
    }
}

/// The part of the canvas a render covers, and at what resolution.
///
/// Layers are `width x height` device pixels; canvas point `p` lands at
/// device `(p - origin) * scale`. A full render has origin 0,0; a zoomed
/// crop renders only a region (plus a margin) at a higher scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    /// The whole `width x height` canvas at `scale`.
    pub fn full(width: u32, height: u32, scale: f32) -> Self {
        Viewport {
            x: 0.0,
            y: 0.0,
            scale,
            width: ((width as f32 * scale).round() as u32).max(1),
            height: ((height as f32 * scale).round() as u32).max(1),
        }
    }

    /// Canvas region `x0,y0 .. x1,y1` (whole pixels) at integer `scale`.
    pub fn region(x0: u32, y0: u32, x1: u32, y1: u32, scale: u32) -> Self {
        Viewport {
            x: x0 as f32,
            y: y0 as f32,
            scale: scale as f32,
            width: ((x1 - x0) * scale).max(1),
            height: ((y1 - y0) * scale).max(1),
        }
    }

    /// Device coordinates of canvas point `p`. Scaling first and then
    /// subtracting the (whole-pixel) offset is exact, so a region render
    /// places marks exactly where the full render at this scale does.
    pub fn device(self, p: Pt) -> (f32, f32) {
        let s = self.scale;
        (p.x * s - self.x * s, p.y * s - self.y * s)
    }

    /// The canvas-to-device transform.
    pub fn transform(&self) -> Transform {
        let s = self.scale;
        Transform::from_row(s, 0.0, 0.0, s, -self.x * s, -self.y * s)
    }
}

/// Everything needed to continue rendering: canvas, layers, variables.
#[derive(Clone)]
pub struct RenderState {
    pub width: u32,
    pub height: u32,
    pub view: Viewport,
    pub background: Rgba,
    pub vars: BTreeMap<String, String>,
    pub layers: Vec<Layer>,
    pub current: Option<usize>,
    pub assets: PathBuf,
}

/// Which layers to show when compositing, overriding their `visible` flag.
#[derive(Debug, Clone, Default)]
pub struct LayerFilter {
    pub solo: Option<String>,
    pub hide: Vec<String>,
    pub show: Vec<String>,
}

/// Deterministic per-op seed from (commit, line), via splitmix64.
pub fn op_seed(commit: usize, line: usize) -> u64 {
    let mut z = ((commit as u64) << 32 | line as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn solid_paint(color: Rgba) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(color.to_skia());
    paint.anti_alias = true;
    paint
}

impl RenderState {
    /// A fresh canvas with one empty, selected layer named `paint`.
    pub fn new(width: u32, height: u32, background: Rgba, scale: f32, assets: PathBuf) -> Self {
        Self::with_viewport(
            width,
            height,
            background,
            Viewport::full(width, height, scale),
            assets,
        )
    }

    /// Like [`RenderState::new`], but rendering only `view`.
    pub fn with_viewport(
        width: u32,
        height: u32,
        background: Rgba,
        view: Viewport,
        assets: PathBuf,
    ) -> Self {
        let mut state = RenderState {
            width,
            height,
            view,
            background,
            vars: BTreeMap::new(),
            layers: Vec::new(),
            current: None,
            assets,
        };
        state.layers.push(state.blank_layer("paint"));
        state.current = Some(0);
        state
    }

    /// Device pixel size of layers (the viewport's size).
    pub fn device_size(&self) -> (u32, u32) {
        (self.view.width, self.view.height)
    }

    fn blank_layer(&self, name: &str) -> Layer {
        let (w, h) = self.device_size();
        Layer {
            name: name.to_string(),
            opacity: 1.0,
            blend: BlendMode::SourceOver,
            visible: true,
            pixmap: Pixmap::new(w, h).expect("canvas size is validated at creation"),
        }
    }

    pub fn layer_index(&self, name: &str) -> Option<usize> {
        self.layers.iter().position(|l| l.name == name)
    }

    /// Look up a layer by name, with an error listing the existing ones.
    pub fn layer_named(&self, name: &str) -> Result<&Layer, String> {
        self.layer_index(name)
            .map(|i| &self.layers[i])
            .ok_or_else(|| {
                let names: Vec<&str> = self.layers.iter().map(|l| l.name.as_str()).collect();
                format!("no layer '{name}' (layers: {})", names.join(", "))
            })
    }

    pub fn current_layer_mut(&mut self) -> Result<&mut Layer, String> {
        let idx = self
            .current
            .ok_or("no current layer; create one with `layer name=...`")?;
        Ok(&mut self.layers[idx])
    }

    pub fn canvas_bounds(&self) -> shape::Bounds {
        (0.0, 0.0, self.width as f32, self.height as f32)
    }

    pub fn parse_shape(&self, args: &Args, key: &str) -> Result<Option<Shape>, String> {
        let size = (self.width as f32, self.height as f32);
        args.get(key, |v| Shape::parse(v, size))
    }

    /// Run a parsed script as commit number `commit` (1-based).
    ///
    /// Returns one report line per statement.
    ///
    /// # Errors
    ///
    /// Stops at the first failing statement; the state is then partially
    /// updated and must be discarded by the caller.
    pub fn run(&mut self, stmts: &[Stmt], commit: usize) -> Result<Vec<String>, ScriptError> {
        stmts
            .iter()
            .map(|stmt| {
                self.exec(stmt, commit)
                    .map(|r| format!("{:<3} {} {}", stmt.line, stmt.op, r))
                    .map_err(|e| stmt.error(e))
            })
            .collect()
    }

    fn exec(&mut self, stmt: &Stmt, commit: usize) -> Result<String, String> {
        if stmt.op == "set" {
            return self.op_set(stmt);
        }
        let args = Args::new(stmt, &self.vars)?;
        let seed = || -> Result<u64, String> {
            Ok(match args.integer("seed")? {
                // Negative seeds are valid; reinterpret the bits.
                Some(s) => s as u64,
                None => op_seed(commit, stmt.line),
            })
        };
        match stmt.op.as_str() {
            "layer" => self.op_layer(&args),
            "delete-layer" => self.op_delete_layer(&args),
            "rename-layer" => self.op_rename_layer(&args),
            "copy-layer" => self.op_copy_layer(&args),
            "fill" => self.op_fill(&args),
            "gradient" => self.op_gradient(&args),
            "clear" => self.op_clear(&args),
            "blur" => self.op_blur(&args),
            "stroke" => self.op_stroke(&args),
            "image" => self.op_image(&args),
            "dot" | "stipple" | "pointillize" | "dabs" => {
                marks::run_mark_op(self, &stmt.op, &args, seed()?)
            }
            other => Err(format!("op '{other}' is not implemented")),
        }
    }

    fn op_layer(&mut self, args: &Args) -> Result<String, String> {
        let name = args.name("name")?.expect("required key checked by parser");
        args.exclusive(&["above", "below", "top"])?;
        // Validate the anchor before creating anything, so an error's layer
        // list does not include the layer this line was about to create.
        for key in ["above", "below"] {
            if let Some(anchor) = args.name(key)? {
                if anchor == name {
                    return Err(format!(
                        "layer '{name}' cannot be positioned relative to itself"
                    ));
                }
                self.layer_named(&anchor)?;
            }
        }
        let mut notes = Vec::new();
        let idx = match self.layer_index(&name) {
            Some(i) => {
                notes.push("selected");
                i
            }
            None => {
                self.layers.push(self.blank_layer(&name));
                notes.push("new");
                self.layers.len() - 1
            }
        };
        let layer = &mut self.layers[idx];
        if let Some(o) = args.number_in("opacity", 0.0, 1.0)? {
            layer.opacity = o;
        }
        if let Some(b) = args.get("blend", |v| {
            blend_by_name(v).ok_or_else(|| {
                let names: Vec<&str> = BLENDS.iter().map(|(n, _)| *n).collect();
                format!("unknown blend '{v}' (one of: {})", names.join(" "))
            })
        })? {
            layer.blend = b;
        }
        if let Some(v) = args.bool("visible")? {
            layer.visible = v;
        }
        if !layer.visible {
            notes.push("hidden");
        }
        let target = if let Some(anchor) = args.name("above")? {
            Some((anchor, 1))
        } else {
            args.name("below")?.map(|anchor| (anchor, 0))
        };
        let to_top = args.bool("top")? == Some(true);
        let moved = self.layers.remove(idx);
        let insert_at = match target {
            Some((anchor, offset)) => self.layer_index(&anchor).expect("checked above") + offset,
            None if to_top => self.layers.len(),
            None => idx,
        };
        self.layers.insert(insert_at, moved);
        self.current = Some(insert_at);
        let l = &self.layers[insert_at];
        Ok(format!(
            "{} ({}; opacity {}, {}, position {} of {})",
            l.name,
            notes.join(", "),
            l.opacity,
            blend_name(l.blend),
            insert_at + 1,
            self.layers.len()
        ))
    }

    fn op_delete_layer(&mut self, args: &Args) -> Result<String, String> {
        let name = args.name("name")?.expect("required key checked by parser");
        self.layer_named(&name)?;
        let idx = self.layer_index(&name).expect("checked above");
        let current_name = self.current.map(|c| self.layers[c].name.clone());
        self.layers.remove(idx);
        self.current = match current_name {
            Some(c) if c != name => self.layer_index(&c),
            _ => self.layers.len().checked_sub(1),
        };
        let now = self
            .current
            .map_or("none".to_string(), |c| self.layers[c].name.clone());
        Ok(format!("{name} (current layer: {now})"))
    }

    fn op_rename_layer(&mut self, args: &Args) -> Result<String, String> {
        let name = args.name("name")?.expect("required key checked by parser");
        let to = args.name("to")?.expect("required key checked by parser");
        if self.layer_index(&to).is_some() {
            return Err(format!("a layer named '{to}' already exists"));
        }
        self.layer_named(&name)?;
        let idx = self.layer_index(&name).expect("checked above");
        self.layers[idx].name = to.clone();
        Ok(format!("{name} -> {to}"))
    }

    /// Define variables left to right, so later ones may use earlier ones.
    /// The report names the variables only; their values are in the script.
    fn op_set(&mut self, stmt: &Stmt) -> Result<String, String> {
        let mut defined = Vec::with_capacity(stmt.args.len());
        for (k, raw) in &stmt.args {
            let v = dsl::substitute(raw, &self.vars).map_err(|e| format!("key '{k}': {e}"))?;
            defined.push(format!("${k}"));
            self.vars.insert(k.clone(), v);
        }
        Ok(defined.join(" "))
    }

    /// `copy-layer name=A to=B`: a visible copy of A, directly above it.
    fn op_copy_layer(&mut self, args: &Args) -> Result<String, String> {
        let name = args.name("name")?.expect("required key checked by parser");
        let to = args.name("to")?.expect("required key checked by parser");
        if self.layer_index(&to).is_some() {
            return Err(format!("a layer named '{to}' already exists"));
        }
        self.layer_named(&name)?;
        let idx = self.layer_index(&name).expect("checked above");
        let copy = Layer {
            name: to.clone(),
            visible: true,
            ..self.layers[idx].clone()
        };
        self.layers.insert(idx + 1, copy);
        self.current = Some(idx + 1);
        Ok(format!(
            "{name} -> {to} (visible, position {} of {})",
            idx + 2,
            self.layers.len()
        ))
    }

    /// `clip=L` for the non-mark ops: layer L's alpha as a coverage mask.
    /// It is copied before drawing, so clipping to the current layer itself
    /// works as an alpha lock.
    fn clip_mask(&self, args: &Args) -> Result<Option<Mask>, String> {
        match args.name("clip")? {
            None => Ok(None),
            Some(name) => {
                let layer = self.layer_named(&name)?;
                Ok(Some(Mask::from_pixmap(
                    layer.pixmap.as_ref(),
                    MaskType::Alpha,
                )))
            }
        }
    }

    /// Paint `shape` with `paint` on the current layer, optionally feathered
    /// and masked by `clip`.
    fn paint_shape(
        &mut self,
        shape: &Shape,
        paint: &Paint,
        feather: f32,
        clip: Option<&Mask>,
    ) -> Result<(), String> {
        let view = self.view;
        let (w, h) = self.device_size();
        let path = shape
            .to_path(view.transform())
            .ok_or("shape is degenerate")?;
        let layer = self.current_layer_mut()?;
        if feather > 0.0 {
            let mut mask = Mask::new(w, h).expect("non-zero canvas");
            mask.fill_path(&path, FillRule::EvenOdd, true, Transform::identity());
            // Only the shape's neighbourhood can become non-zero.
            let sigma = feather * view.scale / 2.0;
            let area = raster::path_rect(&path, raster::blur_support(sigma) + 1, w, h);
            raster::blur_mask(&mut mask, area, sigma);
            if let Some(clip) = clip {
                raster::multiply_mask(&mut mask, clip);
            }
            let (x0, y0, x1, y1) = area;
            if let Some(rect) = Rect::from_ltrb(x0 as f32, y0 as f32, x1 as f32, y1 as f32) {
                layer
                    .pixmap
                    .fill_rect(rect, paint, Transform::identity(), Some(&mask));
            }
        } else {
            layer
                .pixmap
                .fill_path(&path, paint, FillRule::EvenOdd, Transform::identity(), clip);
        }
        Ok(())
    }

    fn feather(args: &Args) -> Result<f32, String> {
        Ok(args.number_in("feather", 0.0, f32::MAX)?.unwrap_or(0.0))
    }

    fn op_fill(&mut self, args: &Args) -> Result<String, String> {
        let shape = self.parse_shape(args, "in")?.expect("required");
        let mut color = args.color("color")?.expect("required");
        color.a *= args.number_in("alpha", 0.0, 1.0)?.unwrap_or(1.0);
        let feather = Self::feather(args)?;
        let clip = self.clip_mask(args)?;
        self.paint_shape(&shape, &solid_paint(color), feather, clip.as_ref())?;
        Ok(format!("{} {}", shape_kind(args), color.hex()))
    }

    fn op_gradient(&mut self, args: &Args) -> Result<String, String> {
        let shape = self.parse_shape(args, "in")?.expect("required");
        let stops = args.need("stops", parse_stops)?;
        let alpha = args.number_in("alpha", 0.0, 1.0)?.unwrap_or(1.0);
        let view = self.view;
        let dev = |p: Pt| {
            let (x, y) = view.device(p);
            Point::from_xy(x, y)
        };
        let kind = match args.raw("kind") {
            Some(k) => k.to_string(),
            None if args.has("center") => "radial".to_string(),
            None => "linear".to_string(),
        };
        let skia_stops = oklab_stops(&stops, alpha);
        let shader = match kind.as_str() {
            "linear" => {
                if args.has("center") || args.has("radius") || args.has("focus") {
                    return Err(
                        "linear gradients take from= and to=, not center/radius/focus".to_string(),
                    );
                }
                let from = args.need("from", dsl::parse_point)?;
                let to = args.need("to", dsl::parse_point)?;
                LinearGradient::new(
                    dev(from),
                    dev(to),
                    skia_stops,
                    SpreadMode::Pad,
                    Transform::identity(),
                )
            }
            "radial" => {
                if args.has("from") || args.has("to") {
                    return Err(
                        "radial gradients take center= and radius=, not from/to".to_string()
                    );
                }
                let center = args.need("center", dsl::parse_point)?;
                let radius = args.need("radius", dsl::parse_number)?;
                if radius <= 0.0 {
                    return Err("key 'radius': must be > 0".to_string());
                }
                let focus = args.point("focus")?.unwrap_or(center);
                RadialGradient::new(
                    dev(focus),
                    0.0,
                    dev(center),
                    radius * view.scale,
                    skia_stops,
                    SpreadMode::Pad,
                    Transform::identity(),
                )
            }
            other => {
                return Err(format!(
                    "key 'kind': expected linear or radial, got '{other}'"
                ))
            }
        }
        .ok_or("degenerate gradient (from and to must differ)")?;
        let paint = Paint {
            shader,
            anti_alias: true,
            ..Paint::default()
        };
        let clip = self.clip_mask(args)?;
        self.paint_shape(&shape, &paint, Self::feather(args)?, clip.as_ref())?;
        Ok(format!(
            "{kind} in {}, {} stops",
            shape_kind(args),
            stops.len()
        ))
    }

    fn op_clear(&mut self, args: &Args) -> Result<String, String> {
        let clip = self.clip_mask(args)?;
        let shape = match (self.parse_shape(args, "in")?, &clip) {
            (Some(shape), _) => shape,
            (None, Some(_)) => Shape::parse("canvas", (self.width as f32, self.height as f32))?,
            (None, None) => {
                if args.has("feather") {
                    return Err("feather= needs in=".to_string());
                }
                let layer = self.current_layer_mut()?;
                layer.pixmap.fill(tiny_skia::Color::TRANSPARENT);
                return Ok(format!("whole layer '{}'", layer.name));
            }
        };
        let mut paint = solid_paint(Rgba::new(0.0, 0.0, 0.0, 1.0));
        paint.blend_mode = BlendMode::DestinationOut;
        self.paint_shape(&shape, &paint, Self::feather(args)?, clip.as_ref())?;
        Ok(shape_kind(args))
    }

    fn op_blur(&mut self, args: &Args) -> Result<String, String> {
        let r = args.number_in("r", 0.0, 1000.0)?.expect("required");
        let sigma = r * self.view.scale / 2.0;
        let (w, h) = self.device_size();
        let clip = self.clip_mask(args)?;
        let (mask, area) = match self.parse_shape(args, "in")? {
            Some(shape) => {
                let path = shape
                    .to_path(self.view.transform())
                    .ok_or("shape is degenerate")?;
                let mut mask = Mask::new(w, h).expect("non-zero canvas");
                mask.fill_path(&path, FillRule::EvenOdd, true, Transform::identity());
                if let Some(clip) = &clip {
                    raster::multiply_mask(&mut mask, clip);
                }
                let inside = raster::path_rect(&path, 0, w, h);
                (Some(mask), inside)
            }
            None => (clip, raster::full_rect(w, h)),
        };
        let layer = self.current_layer_mut()?;
        let Some(mask) = mask else {
            raster::blur_pixmap(&mut layer.pixmap, area, sigma);
            return Ok(format!("r={r} on '{}'", layer.name));
        };
        // Blur a copy around the masked area (exact there), then blend it in.
        let mut blurred = layer.pixmap.clone();
        let support = raster::blur_support(sigma) + 1;
        raster::blur_pixmap(
            &mut blurred,
            raster::grow_rect(area, support, w as usize, h as usize),
            sigma,
        );
        let (x0, y0, x1, y1) = area;
        let width = w as usize;
        for y in y0..y1 {
            let span = y * width + x0..y * width + x1;
            let dst = &mut layer.pixmap.data_mut()[span.start * 4..span.end * 4];
            let src = &blurred.data()[span.start * 4..span.end * 4];
            for ((d, b), &m) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src.as_chunks::<4>().0)
                .zip(&mask.data()[span])
            {
                let t = m as f32 / 255.0;
                for (dc, &bc) in d.iter_mut().zip(b) {
                    *dc = (*dc as f32 + (bc as f32 - *dc as f32) * t).round() as u8;
                }
            }
        }
        Ok(format!("r={r} on '{}'", layer.name))
    }

    fn op_stroke(&mut self, args: &Args) -> Result<String, String> {
        let path = args.need("path", |v| {
            shape::PathSpec::parse(v, (self.width as f32, self.height as f32))
        })?;
        if path.pts.len() < 2 {
            return Err("key 'path': a stroke needs at least 2 points".to_string());
        }
        let width = args.need("width", |v| fixed_ramp(v, 0.0, f32::MAX))?;
        if width.0.max(width.1) <= 0.0 {
            return Err("key 'width': must be > 0".to_string());
        }
        let alpha = args
            .get("alpha", |v| fixed_ramp(v, 0.0, 1.0))?
            .unwrap_or((1.0, 1.0));
        let mut color = args.color("color")?.expect("required");
        let cap = match args.raw("cap").unwrap_or("round") {
            "round" => LineCap::Round,
            "butt" => LineCap::Butt,
            other => return Err(format!("key 'cap': expected round or butt, got '{other}'")),
        };
        let taper = args.number_in("taper", 0.0, 1.0)?.unwrap_or(0.0);
        let smooth = path_smoothing(args, &path)?;
        let closed = path.closed;
        let polyline = path.into_polyline(smooth);
        let view = self.view;
        let (w, h) = self.device_size();
        let clip = self.clip_mask(args)?;
        let length = polyline.length();
        let ramped = width.0 != width.1 || alpha.0 != alpha.1;
        if !ramped {
            color.a *= alpha.0;
        }
        let paint = solid_paint(color);
        let layer = self.current_layer_mut()?;
        if taper > 0.0 || ramped {
            let max_half = width.0.max(width.1) / 2.0;
            let lerp = |(a, b): (f32, f32), u: f32| a + (b - a) * u;
            let profile = |u: f32| {
                let width_mul = if width.0 == width.1 {
                    1.0
                } else {
                    lerp(width, u) / (2.0 * max_half)
                };
                let a = if ramped { lerp(alpha, u) } else { 1.0 };
                (width_mul * marks::taper_profile(u, taper), a)
            };
            let mut coverage = marks::tapered_coverage(&polyline, max_half, profile, &view);
            if let Some(clip) = &clip {
                raster::multiply_mask(&mut coverage, clip);
            }
            let rect = Rect::from_xywh(0.0, 0.0, w as f32, h as f32).expect("non-zero canvas");
            layer
                .pixmap
                .fill_rect(rect, &paint, Transform::identity(), Some(&coverage));
        } else {
            let pts = polyline.points();
            // A closed path repeats its first point; drop it and close instead.
            let outline = if closed { &pts[..pts.len() - 1] } else { pts };
            let skia_path = shape::polygon_path(outline, closed).ok_or("degenerate path")?;
            // Stroked in canvas units; the transform scales width and path together.
            let stroke = Stroke {
                width: width.0,
                line_cap: cap,
                line_join: LineJoin::Round,
                ..Stroke::default()
            };
            layer
                .pixmap
                .stroke_path(&skia_path, &paint, &stroke, view.transform(), clip.as_ref());
        }
        let width_text = if width.0 == width.1 {
            format!("{}", width.0)
        } else {
            format!("{}->{}", width.0, width.1)
        };
        Ok(format!("length {length:.0}px, width {width_text}"))
    }

    fn op_image(&mut self, args: &Args) -> Result<String, String> {
        let name = args.name("src")?.expect("required");
        let file = self.assets.join(format!("{name}.png"));
        let img = Pixmap::load_png(&file)
            .map_err(|e| format!("cannot load asset '{name}' ({}): {e}", file.display()))?;
        let (iw, ih) = (img.width() as f32, img.height() as f32);
        let (cw, ch) = (self.width as f32, self.height as f32);
        let at = args.point("at")?;
        let size = args.get("size", |v| {
            let p = dsl::parse_point(v)?;
            if p.x <= 0.0 || p.y <= 0.0 {
                return Err("size must be positive".to_string());
            }
            Ok((p.x, p.y))
        })?;
        let ((x, y), (w, h)) = match (at, size) {
            (at, Some(size)) => (at.map_or((0.0, 0.0), |p| (p.x, p.y)), size),
            (Some(p), None) => ((p.x, p.y), (iw, ih)),
            (None, None) => {
                let fit = (cw / iw).min(ch / ih);
                let (w, h) = (iw * fit, ih * fit);
                (((cw - w) / 2.0, (ch - h) / 2.0), (w, h))
            }
        };
        let alpha = args.number_in("alpha", 0.0, 1.0)?.unwrap_or(1.0);
        let s = self.view.scale;
        let (dx, dy) = self.view.device(Pt::new(x, y));
        let transform = Transform::from_scale(w / iw * s, h / ih * s).post_translate(dx, dy);
        let paint = PixmapPaint {
            opacity: alpha,
            blend_mode: BlendMode::SourceOver,
            quality: FilterQuality::Bicubic,
        };
        self.current_layer_mut()?
            .pixmap
            .draw_pixmap(0, 0, img.as_ref(), &paint, transform, None);
        Ok(format!("{name} at {x:.0},{y:.0} size {w:.0}x{h:.0}"))
    }

    /// Composite the visible layers over the background (or one layer alone
    /// over transparency with `solo`).
    ///
    /// # Errors
    ///
    /// Fails if the filter names a layer that does not exist.
    pub fn composite(&self, filter: &LayerFilter) -> Result<Pixmap, String> {
        for name in filter.hide.iter().chain(&filter.show).chain(&filter.solo) {
            self.layer_named(name)?;
        }
        let (w, h) = self.device_size();
        let mut out = Pixmap::new(w, h).expect("non-zero canvas");
        if let Some(solo) = &filter.solo {
            let layer = self.layer_named(solo)?;
            out.data_mut().copy_from_slice(layer.pixmap.data());
            return Ok(out);
        }
        out.fill(self.background.to_skia());
        for layer in &self.layers {
            let shown = if filter.hide.contains(&layer.name) {
                false
            } else {
                layer.visible || filter.show.contains(&layer.name)
            };
            if !shown || layer.opacity <= 0.0 {
                continue;
            }
            let paint = PixmapPaint {
                opacity: layer.opacity,
                blend_mode: layer.blend,
                quality: FilterQuality::Nearest,
            };
            out.draw_pixmap(
                0,
                0,
                layer.pixmap.as_ref(),
                &paint,
                Transform::identity(),
                None,
            );
        }
        Ok(out)
    }
}

/// A stroke's `width=`/`alpha=`: a number or a ramp `a->b` of plain
/// numbers, as (start, end), each within `lo..=hi`.
fn fixed_ramp(v: &str, lo: f32, hi: f32) -> Result<(f32, f32), String> {
    let (a, b) = match dsl::parse_ramp(v)? {
        Scalar::Fixed(x) => (x, x),
        Scalar::Ramp {
            start: (a0, a1),
            end: (b0, b1),
        } if a0 == a1 && b0 == b1 => (a0, b0),
        _ => return Err("expected a number or a ramp a->b of numbers, not a range".to_string()),
    };
    for x in [a, b] {
        if !(lo..=hi).contains(&x) {
            return Err(format!("{x} is outside {lo}..{hi}"));
        }
    }
    Ok((a, b))
}

/// Whether to smooth a `path=` through its points: `smooth=` (default
/// true) for point lists; shapes and arcs are exact curves already.
///
/// # Errors
///
/// `smooth=` given together with a shape or arc path.
pub fn path_smoothing(args: &Args, path: &shape::PathSpec) -> Result<bool, String> {
    let smooth = args.bool("smooth")?;
    if path.exact {
        if smooth.is_some() {
            return Err("smooth= only applies to point paths, not shapes or arc()".to_string());
        }
        return Ok(false);
    }
    Ok(smooth.unwrap_or(true))
}

/// The shape function name used in a statement, for reports.
fn shape_kind(args: &Args) -> String {
    let raw = args.raw("in").unwrap_or("canvas");
    raw.split('(').next().unwrap_or(raw).to_string()
}

/// Expand stops with intermediate stops so tiny-skia's sRGB interpolation
/// follows an OKLab mix between the given colours.
fn oklab_stops(stops: &[(Rgba, f32)], alpha: f32) -> Vec<GradientStop> {
    const STEPS: usize = 12;
    let with_alpha = |c: Rgba| {
        Rgba {
            a: c.a * alpha,
            ..c
        }
        .to_skia()
    };
    let mut out = Vec::with_capacity(stops.len() * STEPS);
    for pair in stops.windows(2) {
        let ((c0, t0), (c1, t1)) = (pair[0], pair[1]);
        for k in 0..STEPS {
            let f = k as f32 / STEPS as f32;
            out.push(GradientStop::new(
                t0 + (t1 - t0) * f,
                with_alpha(c0.mix(c1, f)),
            ));
        }
    }
    let (last, t) = stops[stops.len() - 1];
    out.push(GradientStop::new(t, with_alpha(last)));
    out
}

/// Convert a colour string (e.g. the document background) or explain why not.
pub fn parse_background(s: &str) -> Result<Rgba, String> {
    color::parse_color(s)
}
