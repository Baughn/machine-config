//! Shapes (`in=` regions) and path geometry, all in canvas coordinates.

use tiny_skia::{Path, PathBuilder, Rect, Transform};

use crate::color::call_form;
use crate::dsl::{parse_number, parse_path, Pt};

/// A closed region of the canvas.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Circle {
        c: Pt,
        r: f32,
    },
    Ellipse {
        c: Pt,
        rx: f32,
        ry: f32,
        rot_deg: f32,
        outline: Vec<Pt>,
    },
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    /// `poly` and `blob` both end up as a flattened closed outline.
    Poly {
        outline: Vec<Pt>,
    },
}

/// Axis-aligned bounds `(x0, y0, x1, y1)`.
pub type Bounds = (f32, f32, f32, f32);

fn nums(inner: &str, func: &str, counts: &[usize]) -> Result<Vec<f32>, String> {
    let v: Vec<f32> = inner
        .split(',')
        .map(parse_number)
        .collect::<Result<_, _>>()?;
    if counts.contains(&v.len()) {
        Ok(v)
    } else {
        let want: Vec<String> = counts.iter().map(|c| c.to_string()).collect();
        Err(format!(
            "{func}() takes {} numbers, got {}",
            want.join(" or "),
            v.len()
        ))
    }
}

impl Shape {
    /// Parse a shape value; `canvas` becomes a rect covering `canvas_size`.
    ///
    /// # Errors
    ///
    /// Returns a message describing the malformed shape.
    pub fn parse(s: &str, canvas_size: (f32, f32)) -> Result<Shape, String> {
        if s == "canvas" {
            return Ok(Shape::Rect {
                x: 0.0,
                y: 0.0,
                w: canvas_size.0,
                h: canvas_size.1,
            });
        }
        let (func, inner) = call_form(s).ok_or_else(|| {
            format!(
                "expected a shape (circle(), ellipse(), rect(), poly(), blob(), canvas), got '{s}'"
            )
        })?;
        let shape = match func {
            "circle" => {
                let v = nums(inner, func, &[3])?;
                Shape::Circle {
                    c: Pt::new(v[0], v[1]),
                    r: v[2],
                }
            }
            "ellipse" => {
                let v = nums(inner, func, &[4, 5])?;
                let (c, rx, ry) = (Pt::new(v[0], v[1]), v[2], v[3]);
                let rot_deg = v.get(4).copied().unwrap_or(0.0);
                let outline = ellipse_outline(c, rx, ry, rot_deg);
                Shape::Ellipse {
                    c,
                    rx,
                    ry,
                    rot_deg,
                    outline,
                }
            }
            "rect" => {
                let v = nums(inner, func, &[4])?;
                Shape::Rect {
                    x: v[0],
                    y: v[1],
                    w: v[2],
                    h: v[3],
                }
            }
            "poly" | "blob" => {
                let pts = parse_path(inner)?;
                if pts.len() < 3 {
                    return Err(format!("{func}() needs at least 3 points"));
                }
                let outline = if func == "blob" {
                    catmull_rom(&pts, true)
                } else {
                    pts
                };
                Shape::Poly { outline }
            }
            _ => return Err(format!("unknown shape '{func}()'")),
        };
        let degenerate = match &shape {
            Shape::Circle { r, .. } => *r <= 0.0,
            Shape::Ellipse { rx, ry, .. } => *rx <= 0.0 || *ry <= 0.0,
            Shape::Rect { w, h, .. } => *w <= 0.0 || *h <= 0.0,
            Shape::Poly { .. } => false,
        };
        if degenerate {
            return Err(format!("{func}() must have positive size"));
        }
        Ok(shape)
    }

    pub fn bounds(&self) -> Bounds {
        match self {
            Shape::Circle { c, r } => (c.x - r, c.y - r, c.x + r, c.y + r),
            Shape::Rect { x, y, w, h } => (*x, *y, x + w, y + h),
            Shape::Ellipse { outline, .. } | Shape::Poly { outline } => outline.iter().fold(
                (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
                |(x0, y0, x1, y1), p| (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)),
            ),
        }
    }

    pub fn contains(&self, p: Pt) -> bool {
        match self {
            Shape::Circle { c, r } => (p.x - c.x).powi(2) + (p.y - c.y).powi(2) <= r * r,
            Shape::Rect { x, y, w, h } => p.x >= *x && p.x <= x + w && p.y >= *y && p.y <= y + h,
            Shape::Ellipse {
                c, rx, ry, rot_deg, ..
            } => {
                let (s, co) = (-rot_deg.to_radians()).sin_cos();
                let (dx, dy) = (p.x - c.x, p.y - c.y);
                let (lx, ly) = (dx * co - dy * s, dx * s + dy * co);
                (lx / rx).powi(2) + (ly / ry).powi(2) <= 1.0
            }
            Shape::Poly { outline } => polygon_contains(outline, p),
        }
    }

    /// Distance from `p` to the shape's boundary.
    pub fn edge_distance(&self, p: Pt) -> f32 {
        match self {
            Shape::Circle { c, r } => {
                (r - ((p.x - c.x).powi(2) + (p.y - c.y).powi(2)).sqrt()).abs()
            }
            Shape::Rect { x, y, w, h } => {
                let inside = (p.x - x).min(x + w - p.x).min(p.y - y).min(y + h - p.y);
                if inside >= 0.0 {
                    inside
                } else {
                    let dx = (x - p.x).max(p.x - x - w).max(0.0);
                    let dy = (y - p.y).max(p.y - y - h).max(0.0);
                    (dx * dx + dy * dy).sqrt()
                }
            }
            Shape::Ellipse { outline, .. } | Shape::Poly { outline } => {
                polyline_distance(outline, true, p)
            }
        }
    }

    /// Build a tiny-skia path in device pixels (canvas coords mapped by
    /// `transform`, see `Viewport::transform`).
    pub fn to_path(&self, transform: Transform) -> Option<Path> {
        let path = match self {
            Shape::Circle { c, r } => PathBuilder::from_circle(c.x, c.y, *r)?,
            Shape::Rect { x, y, w, h } => PathBuilder::from_rect(Rect::from_xywh(*x, *y, *w, *h)?),
            Shape::Ellipse {
                c, rx, ry, rot_deg, ..
            } => {
                let oval = PathBuilder::from_oval(Rect::from_xywh(-rx, -ry, 2.0 * rx, 2.0 * ry)?)?;
                oval.transform(Transform::from_rotate(*rot_deg).post_translate(c.x, c.y))?
            }
            Shape::Poly { outline } => polygon_path(outline, true)?,
        };
        path.transform(transform)
    }

    /// The closed boundary as a polyline (first point not repeated).
    pub fn outline(&self) -> Vec<Pt> {
        match self {
            Shape::Circle { c, r } => ellipse_outline(*c, *r, *r, 0.0),
            Shape::Rect { x, y, w, h } => vec![
                Pt::new(*x, *y),
                Pt::new(x + w, *y),
                Pt::new(x + w, y + h),
                Pt::new(*x, y + h),
            ],
            Shape::Ellipse { outline, .. } | Shape::Poly { outline } => outline.clone(),
        }
    }
}

/// A `path=` value for dabs and stroke: a point list `x,y;x,y;...`, a
/// shape (its closed outline) or `arc(cx,cy,r,a0,a1)` /
/// `arc(cx,cy,rx,ry,a0,a1)`.
#[derive(Debug, Clone, PartialEq)]
pub struct PathSpec {
    pub pts: Vec<Pt>,
    /// The path returns to its first point (shape outlines).
    pub closed: bool,
    /// Already an exact curve (shape or arc), so never smoothed.
    pub exact: bool,
}

impl PathSpec {
    /// Parse a `path=` value.
    ///
    /// # Errors
    ///
    /// Returns a message describing the malformed path, shape or arc.
    pub fn parse(s: &str, canvas_size: (f32, f32)) -> Result<PathSpec, String> {
        let exact = |pts, closed| PathSpec {
            pts,
            closed,
            exact: true,
        };
        match call_form(s) {
            Some(("arc", inner)) => {
                let v = nums(inner, "arc", &[5, 6])?;
                let (c, rx, ry, a0, a1) = match v.as_slice() {
                    [x, y, r, a0, a1] => (Pt::new(*x, *y), *r, *r, *a0, *a1),
                    [x, y, rx, ry, a0, a1] => (Pt::new(*x, *y), *rx, *ry, *a0, *a1),
                    _ => unreachable!("nums() checked the count"),
                };
                if rx <= 0.0 || ry <= 0.0 {
                    return Err("arc() must have a positive radius".to_string());
                }
                if a0 == a1 {
                    return Err("arc() needs different start and end angles".to_string());
                }
                Ok(exact(arc_points(c, rx, ry, a0, a1), false))
            }
            Some(_) => Ok(exact(Shape::parse(s, canvas_size)?.outline(), true)),
            None if s == "canvas" => Ok(exact(Shape::parse(s, canvas_size)?.outline(), true)),
            None => Ok(PathSpec {
                pts: parse_path(s)?,
                closed: false,
                exact: false,
            }),
        }
    }

    /// The path as a polyline to walk along, smoothed through its points if
    /// `smooth`; a closed path ends with its first point again.
    pub fn into_polyline(self, smooth: bool) -> Polyline {
        let mut pts = if smooth {
            catmull_rom(&self.pts, false)
        } else {
            self.pts
        };
        if self.closed {
            if let Some(&first) = pts.first() {
                pts.push(first);
            }
        }
        Polyline::new(pts)
    }
}

/// Points along an elliptical arc from `a0` to `a1` degrees (0 = +x,
/// clockwise on screen), about 2 canvas px apart.
fn arc_points(c: Pt, rx: f32, ry: f32, a0: f32, a1: f32) -> Vec<Pt> {
    let sweep = (a1 - a0).to_radians();
    let length = sweep.abs() * rx.max(ry);
    let steps = ((length / 2.0).ceil() as usize).clamp(8, 4096);
    (0..=steps)
        .map(|i| {
            let t = (a0.to_radians() + sweep * i as f32 / steps as f32).sin_cos();
            Pt::new(c.x + rx * t.1, c.y + ry * t.0)
        })
        .collect()
}

fn ellipse_outline(c: Pt, rx: f32, ry: f32, rot_deg: f32) -> Vec<Pt> {
    let segments = ((rx.max(ry) * 0.75) as usize).clamp(48, 720);
    let (s, co) = rot_deg.to_radians().sin_cos();
    (0..segments)
        .map(|i| {
            let t = i as f32 / segments as f32 * std::f32::consts::TAU;
            let (lx, ly) = (rx * t.cos(), ry * t.sin());
            Pt::new(c.x + lx * co - ly * s, c.y + lx * s + ly * co)
        })
        .collect()
}

/// A tiny-skia path through `pts` (closed or open polyline).
pub fn polygon_path(pts: &[Pt], closed: bool) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let (first, rest) = pts.split_first()?;
    pb.move_to(first.x, first.y);
    for p in rest {
        pb.line_to(p.x, p.y);
    }
    if closed {
        pb.close();
    }
    pb.finish()
}

/// Even-odd point-in-polygon test.
fn polygon_contains(poly: &[Pt], p: Pt) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for (i, a) in poly.iter().enumerate() {
        let b = poly[j];
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn segment_distance(a: Pt, b: Pt, p: Pt) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (ex, ey) = (a.x + t * dx - p.x, a.y + t * dy - p.y);
    (ex * ex + ey * ey).sqrt()
}

/// Minimum distance from `p` to a polyline (closing edge included if `closed`).
fn polyline_distance(pts: &[Pt], closed: bool, p: Pt) -> f32 {
    let open = pts.windows(2).map(|w| segment_distance(w[0], w[1], p));
    let closing = closed
        .then(|| segment_distance(pts[pts.len() - 1], pts[0], p))
        .into_iter();
    open.chain(closing).fold(f32::MAX, f32::min)
}

/// Sample a uniform Catmull-Rom spline through `pts` into a dense polyline.
/// Open curves duplicate their end points so the curve starts and ends on them.
pub fn catmull_rom(pts: &[Pt], closed: bool) -> Vec<Pt> {
    let n = pts.len();
    if n < 3 && closed || n < 2 {
        return pts.to_vec();
    }
    let at = |i: isize| -> Pt {
        if closed {
            pts[i.rem_euclid(n as isize) as usize]
        } else {
            pts[i.clamp(0, n as isize - 1) as usize]
        }
    };
    let segments = if closed { n } else { n - 1 };
    let mut out = Vec::new();
    for s in 0..segments as isize {
        let (p0, p1, p2, p3) = (at(s - 1), at(s), at(s + 1), at(s + 2));
        let len = ((p2.x - p1.x).powi(2) + (p2.y - p1.y).powi(2)).sqrt();
        let steps = ((len / 2.0) as usize).clamp(8, 256);
        for k in 0..steps {
            let t = k as f32 / steps as f32;
            let (t2, t3) = (t * t, t * t * t);
            let blend = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * (2.0 * b
                    + (c - a) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (3.0 * b - a - 3.0 * c + d) * t3)
            };
            out.push(Pt::new(
                blend(p0.x, p1.x, p2.x, p3.x),
                blend(p0.y, p1.y, p2.y, p3.y),
            ));
        }
    }
    if !closed {
        out.push(pts[n - 1]);
    }
    out
}

/// A polyline with cumulative arc length, for walking along paths.
pub struct Polyline {
    pts: Vec<Pt>,
    cumulative: Vec<f32>,
}

impl Polyline {
    pub fn new(pts: Vec<Pt>) -> Self {
        let mut cumulative = Vec::with_capacity(pts.len());
        let mut total = 0.0;
        cumulative.push(0.0);
        for w in pts.windows(2) {
            total += ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt();
            cumulative.push(total);
        }
        Polyline { pts, cumulative }
    }

    pub fn length(&self) -> f32 {
        *self
            .cumulative
            .last()
            .expect("a polyline has at least one point")
    }

    pub fn points(&self) -> &[Pt] {
        &self.pts
    }

    /// Position and tangent angle (radians) at arc length `s`.
    pub fn at(&self, s: f32) -> (Pt, f32) {
        if self.pts.len() < 2 {
            return (self.pts[0], 0.0);
        }
        let s = s.clamp(0.0, self.length());
        let i = self
            .cumulative
            .partition_point(|&c| c <= s)
            .clamp(1, self.pts.len() - 1);
        let (a, b) = (self.pts[i - 1], self.pts[i]);
        let seg = self.cumulative[i] - self.cumulative[i - 1];
        let t = if seg > 0.0 {
            (s - self.cumulative[i - 1]) / seg
        } else {
            0.0
        };
        (
            Pt::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t),
            (b.y - a.y).atan2(b.x - a.x),
        )
    }
}

/// Estimate the area of `shape ∩ clip` by sampling a grid of up to 256×256.
pub fn clipped_area(shape: &Shape, clip: Bounds) -> f32 {
    let (bx0, by0, bx1, by1) = shape.bounds();
    let (x0, y0) = (bx0.max(clip.0), by0.max(clip.1));
    let (x1, y1) = (bx1.min(clip.2), by1.min(clip.3));
    if x1 <= x0 || y1 <= y0 {
        return 0.0;
    }
    const N: usize = 256;
    let (dx, dy) = ((x1 - x0) / N as f32, (y1 - y0) / N as f32);
    let hits = (0..N * N)
        .filter(|i| {
            let p = Pt::new(
                x0 + (i % N) as f32 * dx + dx * 0.5,
                y0 + (i / N) as f32 * dy + dy * 0.5,
            );
            shape.contains(p)
        })
        .count();
    hits as f32 * dx * dy
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANVAS: (f32, f32) = (100.0, 80.0);

    #[test]
    fn parse_every_shape() {
        assert!(matches!(
            Shape::parse("circle(1,2,3)", CANVAS),
            Ok(Shape::Circle { .. })
        ));
        assert!(matches!(
            Shape::parse("ellipse(50,40,20,10,30)", CANVAS),
            Ok(Shape::Ellipse { .. })
        ));
        assert!(matches!(
            Shape::parse("rect(0,0,5,5)", CANVAS),
            Ok(Shape::Rect { .. })
        ));
        assert!(matches!(
            Shape::parse("poly(0,0;10,0;0,10)", CANVAS),
            Ok(Shape::Poly { .. })
        ));
        assert!(matches!(
            Shape::parse("blob(0,0;10,0;10,10;0,10)", CANVAS),
            Ok(Shape::Poly { .. })
        ));
        assert_eq!(
            Shape::parse("canvas", CANVAS).unwrap().bounds(),
            (0.0, 0.0, 100.0, 80.0)
        );
        assert!(Shape::parse("circle(1,2)", CANVAS).is_err());
        assert!(Shape::parse("poly(0,0;1,1)", CANVAS).is_err());
        assert!(Shape::parse("star(1,2,3)", CANVAS).is_err());
        assert!(Shape::parse("circle(1,2,-3)", CANVAS).is_err());
    }

    #[test]
    fn containment_and_distance() {
        let e = Shape::parse("ellipse(50,40,20,10,90)", CANVAS).unwrap();
        assert!(e.contains(Pt::new(50.0, 58.0)));
        assert!(!e.contains(Pt::new(68.0, 40.0)));
        let r = Shape::parse("rect(10,10,20,20)", CANVAS).unwrap();
        assert_eq!(r.edge_distance(Pt::new(15.0, 20.0)), 5.0);
        let p = Shape::parse("poly(0,0;10,0;10,10;0,10)", CANVAS).unwrap();
        assert!(p.contains(Pt::new(5.0, 5.0)));
        assert!((p.edge_distance(Pt::new(5.0, 3.0)) - 3.0).abs() < 1e-4);
    }

    #[test]
    fn area_estimate() {
        let c = Shape::parse("circle(50,40,20)", CANVAS).unwrap();
        let a = clipped_area(&c, (0.0, 0.0, 100.0, 80.0));
        assert!((a - std::f32::consts::PI * 400.0).abs() < 15.0, "{a}");
        let clipped = clipped_area(&c, (0.0, 0.0, 50.0, 80.0));
        assert!((clipped - a / 2.0).abs() < 15.0);
    }

    #[test]
    fn path_values() {
        let pts = PathSpec::parse("0,0;10,0;10,10", CANVAS).unwrap();
        assert!(!pts.exact && !pts.closed && pts.pts.len() == 3);
        let arc = PathSpec::parse("arc(50,40,10,0,90)", CANVAS).unwrap();
        assert!(arc.exact && !arc.closed);
        let first = arc.pts[0];
        let last = *arc.pts.last().unwrap();
        assert!((first.x - 60.0).abs() < 1e-4 && (first.y - 40.0).abs() < 1e-4);
        // 90 degrees is clockwise on screen: straight down (y grows).
        assert!((last.x - 50.0).abs() < 1e-4 && (last.y - 50.0).abs() < 1e-4);
        let ring = PathSpec::parse("circle(50,40,10)", CANVAS).unwrap();
        assert!(ring.exact && ring.closed);
        let line = ring.into_polyline(false);
        let circumference = 2.0 * std::f32::consts::PI * 10.0;
        assert!(
            (line.length() - circumference).abs() < 0.2,
            "{}",
            line.length()
        );
        assert!(PathSpec::parse("arc(1,2,3,4)", CANVAS).is_err());
        assert!(PathSpec::parse("arc(1,2,3,40,40)", CANVAS).is_err());
    }

    #[test]
    fn polyline_walk() {
        let pl = Polyline::new(vec![
            Pt::new(0.0, 0.0),
            Pt::new(10.0, 0.0),
            Pt::new(10.0, 10.0),
        ]);
        assert_eq!(pl.length(), 20.0);
        let (p, angle) = pl.at(15.0);
        assert_eq!(p, Pt::new(10.0, 5.0));
        assert!((angle - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    }

    #[test]
    fn catmull_rom_passes_through_points() {
        let pts = vec![Pt::new(0.0, 0.0), Pt::new(20.0, 10.0), Pt::new(40.0, 0.0)];
        let curve = catmull_rom(&pts, false);
        assert_eq!(curve[0], pts[0]);
        assert_eq!(*curve.last().unwrap(), pts[2]);
        assert!(curve.contains(&pts[1]));
    }
}
