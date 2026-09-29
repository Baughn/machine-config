//! Producing images for the painter to look at: crop, zoom, grid, labels.

use tiny_skia::Pixmap;

/// A canvas-space rectangle (x, y, w, h) in whole pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Grid request: none, automatic spacing, or every N canvas px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Grid {
    Off,
    Auto,
    Every(f32),
}

/// A finished view image and how it maps to canvas coordinates.
pub struct ViewImage {
    pub pixmap: Pixmap,
    /// Output pixels per canvas pixel.
    pub zoom: f32,
    pub grid: Option<f32>,
}

/// A "nice" grid spacing (1, 2 or 5 × 10^k) giving about 8 lines across `span`.
pub fn auto_grid(span: f32) -> f32 {
    let raw = (span / 8.0).max(1.0);
    let mag = 10f32.powf(raw.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .iter()
        .map(|m| m * mag)
        .min_by(|a, b| (a - raw).abs().total_cmp(&(b - raw).abs()))
        .unwrap_or(raw)
}

/// Copy a rectangle of whole pixels out of a (premultiplied) pixmap.
fn crop_rgba(src: &Pixmap, c: Crop) -> Vec<u8> {
    let sw = src.width() as usize;
    let mut out = Vec::with_capacity((c.w * c.h * 4) as usize);
    for y in c.y..c.y + c.h {
        let start = (y as usize * sw + c.x as usize) * 4;
        out.extend_from_slice(&src.data()[start..start + c.w as usize * 4]);
    }
    out
}

/// Nearest-neighbour integer upscale.
fn upscale(src: &[u8], w: usize, h: usize, factor: usize) -> Vec<u8> {
    let (ow, oh) = (w * factor, h * factor);
    let mut out = vec![0u8; ow * oh * 4];
    for y in 0..oh {
        for x in 0..ow {
            let s = ((y / factor) * w + x / factor) * 4;
            let d = (y * ow + x) * 4;
            out[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
    out
}

/// Area-averaging downscale (a box filter with fractional pixel weights).
fn downscale(src: &[u8], w: usize, h: usize, ow: usize, oh: usize) -> Vec<u8> {
    let weights = |n_in: usize, n_out: usize| -> Vec<Vec<(usize, f32)>> {
        let ratio = n_in as f32 / n_out as f32;
        (0..n_out)
            .map(|o| {
                let (a, b) = (o as f32 * ratio, (o + 1) as f32 * ratio);
                (a.floor() as usize..(b.ceil() as usize).min(n_in))
                    .map(|i| {
                        let cover = (b.min(i as f32 + 1.0) - a.max(i as f32)).max(0.0);
                        (i, cover / ratio)
                    })
                    .collect()
            })
            .collect()
    };
    let (wx, wy) = (weights(w, ow), weights(h, oh));
    let mut rows = vec![0f32; ow * h * 4];
    for y in 0..h {
        for (ox, taps) in wx.iter().enumerate() {
            let mut acc = [0f32; 4];
            for &(ix, wt) in taps {
                let s = (y * w + ix) * 4;
                for c in 0..4 {
                    acc[c] += src[s + c] as f32 * wt;
                }
            }
            rows[(y * ow + ox) * 4..(y * ow + ox) * 4 + 4].copy_from_slice(&acc);
        }
    }
    let mut out = vec![0u8; ow * oh * 4];
    for (oy, taps) in wy.iter().enumerate() {
        for ox in 0..ow {
            let mut acc = [0f32; 4];
            for &(iy, wt) in taps {
                let s = (iy * ow + ox) * 4;
                for c in 0..4 {
                    acc[c] += rows[s + c] * wt;
                }
            }
            for c in 0..4 {
                out[(oy * ow + ox) * 4 + c] = acc[c].round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// Replace each pixel by its alpha as opaque grey.
pub fn alpha_to_grey(pixmap: &mut Pixmap) {
    for px in pixmap.data_mut().as_chunks_mut::<4>().0 {
        let a = px[3];
        *px = [a, a, a, 255];
    }
}

/// Composite premultiplied pixels over a grey checkerboard (in output space).
fn over_checkerboard(data: &mut [u8], w: usize) {
    for (i, px) in data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        if px[3] == 255 {
            continue;
        }
        let (x, y) = (i % w, i / w);
        let bg: f32 = if (x / 8 + y / 8) % 2 == 0 {
            0x99 as f32
        } else {
            0x66 as f32
        };
        let inv = 1.0 - px[3] as f32 / 255.0;
        for c in px.iter_mut().take(3) {
            *c = (*c as f32 + bg * inv).round().min(255.0) as u8;
        }
        px[3] = 255;
    }
}

/// The actual canvas pixels of `crop` (from a scale-1 composite), fitted
/// to `max_px`: whole-number nearest-neighbour zoom in (so pixels stay
/// square), area-average zoom out.
pub fn pixel_view(canvas: &Pixmap, crop: Crop, max_px: u32, grid: Grid) -> ViewImage {
    let region = crop_rgba(canvas, crop);
    let (cw, ch) = (crop.w as usize, crop.h as usize);
    let long = crop.w.max(crop.h);
    let (data, ow, oh) = if long <= max_px {
        let factor = (max_px / long).max(1) as usize;
        (upscale(&region, cw, ch, factor), cw * factor, ch * factor)
    } else {
        let z = max_px as f32 / long as f32;
        let (ow, oh) = (
            ((cw as f32 * z).round() as usize).max(1),
            ((ch as f32 * z).round() as usize).max(1),
        );
        (downscale(&region, cw, ch, ow, oh), ow, oh)
    };
    finish(data, ow, oh, crop, grid)
}

/// A crop from a render at a higher scale: `device` covers the viewport and
/// `rect` is the crop in its device pixels. The result is `zoom` output px
/// per canvas px (area-averaged down from the render scale if smaller).
pub fn rendered_view(device: &Pixmap, rect: Crop, crop: Crop, zoom: f32, grid: Grid) -> ViewImage {
    let region = crop_rgba(device, rect);
    let (rw, rh) = (rect.w as usize, rect.h as usize);
    let (ow, oh) = (
        ((crop.w as f32 * zoom).round() as usize).max(1),
        ((crop.h as f32 * zoom).round() as usize).max(1),
    );
    let data = if (ow, oh) == (rw, rh) {
        region
    } else {
        downscale(&region, rw, rh, ow, oh)
    };
    finish(data, ow, oh, crop, grid)
}

/// Flatten over a checkerboard and draw the grid.
fn finish(mut data: Vec<u8>, ow: usize, oh: usize, crop: Crop, grid: Grid) -> ViewImage {
    let zoom = ow as f32 / crop.w as f32;
    over_checkerboard(&mut data, ow);
    let size = tiny_skia::IntSize::from_wh(ow as u32, oh as u32).expect("non-empty view");
    let mut pixmap = Pixmap::from_vec(data, size).expect("buffer matches size");
    let spacing = match grid {
        Grid::Off => None,
        Grid::Auto => Some(auto_grid(crop.w.max(crop.h) as f32)),
        Grid::Every(n) => Some(n),
    };
    if let Some(n) = spacing {
        draw_grid(&mut pixmap, crop, zoom, n);
    }
    ViewImage {
        pixmap,
        zoom,
        grid: spacing,
    }
}

/// Blend a solid colour into one output pixel.
fn put(p: &mut Pixmap, x: i64, y: i64, rgb: [u8; 3], alpha: f32) {
    if x < 0 || y < 0 || x >= p.width() as i64 || y >= p.height() as i64 {
        return;
    }
    let i = (y as usize * p.width() as usize + x as usize) * 4;
    let d = p.data_mut();
    for c in 0..3 {
        d[i + c] = (d[i + c] as f32 * (1.0 - alpha) + rgb[c] as f32 * alpha).round() as u8;
    }
}

fn fmt_coord(v: f32) -> String {
    if (v - v.round()).abs() < 1e-3 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

/// Grid lines as a dark/light double line, labels on the top and left edges.
fn draw_grid(p: &mut Pixmap, crop: Crop, zoom: f32, n: f32) {
    let (w, h) = (p.width() as i64, p.height() as i64);
    // Lines that land inside the image: one at the crop's far edge would
    // fall just outside it, and a label for it would sit next to the
    // previous line instead.
    let lines = |start: u32, len: u32, size: i64| -> Vec<(f32, i64)> {
        let first = (start as f32 / n).ceil() as i64;
        let last = ((start + len) as f32 / n).floor() as i64;
        (first..=last)
            .map(|k| {
                let v = k as f32 * n;
                (v, ((v - start as f32) * zoom).round() as i64)
            })
            .filter(|&(_, o)| o < size)
            .collect()
    };
    let xs = lines(crop.x, crop.w, w);
    let ys = lines(crop.y, crop.h, h);
    for &(_, ox) in &xs {
        for y in 0..h {
            put(p, ox, y, [0, 0, 0], 0.45);
            put(p, ox + 1, y, [255, 255, 255], 0.45);
        }
    }
    for &(_, oy) in &ys {
        for x in 0..w {
            put(p, x, oy, [0, 0, 0], 0.45);
            put(p, x, oy + 1, [255, 255, 255], 0.45);
        }
    }
    // The first label on each axis carries the axis letter.
    let mut next_free = 0i64;
    for (i, &(v, ox)) in xs.iter().enumerate() {
        let text = format!("{}{}", if i == 0 { "x" } else { "" }, fmt_coord(v));
        let x = (ox + 3).min(w - text_width(&text) - 4).max(0);
        if x < next_free {
            continue;
        }
        draw_label(p, x, 2, &text);
        next_free = x + text_width(&text) + 8;
    }
    // Leave the top label strip to the x labels.
    let mut next_free = GLYPH_H * FONT_SCALE + 8;
    let mut first = true;
    for &(v, oy) in &ys {
        let y = (oy + 3).min(h - GLYPH_H * FONT_SCALE - 4).max(0);
        if y < next_free {
            continue;
        }
        let text = format!("{}{}", if first { "y" } else { "" }, fmt_coord(v));
        first = false;
        draw_label(p, 2, y, &text);
        next_free = y + GLYPH_H * FONT_SCALE + 6;
    }
}

const GLYPH_W: i64 = 5;
const GLYPH_H: i64 = 7;
const FONT_SCALE: i64 = 2;

/// 5×7 bitmap glyphs, one byte per row, high bit (0x10) = leftmost column.
fn glyph(c: char) -> [u8; 7] {
    match c {
        '0' => [0x0e, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0e],
        '1' => [0x04, 0x0c, 0x04, 0x04, 0x04, 0x04, 0x0e],
        '2' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1f],
        '3' => [0x1f, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0e],
        '4' => [0x02, 0x06, 0x0a, 0x12, 0x1f, 0x02, 0x02],
        '5' => [0x1f, 0x10, 0x1e, 0x01, 0x01, 0x11, 0x0e],
        '6' => [0x06, 0x08, 0x10, 0x1e, 0x11, 0x11, 0x0e],
        '7' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0e, 0x11, 0x11, 0x0e, 0x11, 0x11, 0x0e],
        '9' => [0x0e, 0x11, 0x11, 0x0f, 0x01, 0x02, 0x0c],
        '-' => [0x00, 0x00, 0x00, 0x1f, 0x00, 0x00, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0c, 0x0c],
        ',' => [0x00, 0x00, 0x00, 0x00, 0x0c, 0x04, 0x08],
        'x' => [0x00, 0x00, 0x11, 0x0a, 0x04, 0x0a, 0x11],
        'y' => [0x00, 0x00, 0x11, 0x11, 0x0f, 0x01, 0x0e],
        _ => [0x00; 7],
    }
}

fn text_width(text: &str) -> i64 {
    let n = text.chars().count() as i64;
    (n * (GLYPH_W + 1) - 1) * FONT_SCALE
}

/// White text on a dark backing box, top-left corner at (x, y).
fn draw_label(p: &mut Pixmap, x: i64, y: i64, text: &str) {
    let (bw, bh) = (text_width(text) + 4, GLYPH_H * FONT_SCALE + 4);
    for by in 0..bh {
        for bx in 0..bw {
            put(p, x + bx, y + by, [0, 0, 0], 0.72);
        }
    }
    for (i, ch) in text.chars().enumerate() {
        let gx = x + 2 + i as i64 * (GLYPH_W + 1) * FONT_SCALE;
        for (row, bits) in glyph(ch).iter().enumerate() {
            for col in 0..GLYPH_W {
                if bits & (0x10 >> col) == 0 {
                    continue;
                }
                for sy in 0..FONT_SCALE {
                    for sx in 0..FONT_SCALE {
                        put(
                            p,
                            gx + col * FONT_SCALE + sx,
                            y + 2 + row as i64 * FONT_SCALE + sy,
                            [255, 255, 255],
                            1.0,
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_grid_is_round() {
        assert_eq!(auto_grid(1000.0), 100.0);
        assert_eq!(auto_grid(200.0), 20.0);
        assert_eq!(auto_grid(64.0), 10.0);
        assert_eq!(auto_grid(5.0), 1.0);
    }

    #[test]
    fn zoom_in_is_integer_and_zoom_out_fits() {
        let canvas = Pixmap::new(400, 200).unwrap();
        let small = Crop {
            x: 10,
            y: 10,
            w: 64,
            h: 32,
        };
        let v = pixel_view(&canvas, small, 1000, Grid::Auto);
        assert_eq!(v.zoom, 15.0);
        assert_eq!((v.pixmap.width(), v.pixmap.height()), (960, 480));
        let whole = Crop {
            x: 0,
            y: 0,
            w: 400,
            h: 200,
        };
        let v = pixel_view(&canvas, whole, 100, Grid::Off);
        assert_eq!((v.pixmap.width(), v.pixmap.height()), (100, 50));
    }

    #[test]
    fn rendered_view_fits_fractional_zoom() {
        // A 50x25 crop rendered at scale 3, shown at 2.5 output px per px.
        let device = Pixmap::new(200, 100).unwrap();
        let rect = Crop {
            x: 30,
            y: 15,
            w: 150,
            h: 75,
        };
        let crop = Crop {
            x: 10,
            y: 5,
            w: 50,
            h: 25,
        };
        let v = rendered_view(&device, rect, crop, 2.5, Grid::Auto);
        assert_eq!((v.pixmap.width(), v.pixmap.height()), (125, 63));
        assert_eq!(v.zoom, 2.5);
    }

    #[test]
    fn grid_labels_only_drawn_lines() {
        // A 20x20 crop at 10x with lines every 10: 0 and 10 are inside,
        // 20 is the far edge (output x = 200, off the image) and gets no
        // label: the top-right corner stays clear of label boxes.
        let mut p = Pixmap::new(200, 200).unwrap();
        p.fill(tiny_skia::Color::WHITE);
        let crop = Crop {
            x: 0,
            y: 0,
            w: 20,
            h: 20,
        };
        draw_grid(&mut p, crop, 10.0, 10.0);
        let dark = |x: u32, y: u32| p.pixel(x, y).unwrap().red() < 128;
        assert!(dark(104, 3), "label box of the line at 10");
        assert!(
            (150..200).all(|x| !dark(x, 3)),
            "no label for the edge at 20"
        );
    }

    #[test]
    fn downscale_averages() {
        let src = [255u8, 255, 255, 255, 0, 0, 0, 255];
        let out = downscale(&src, 2, 1, 1, 1);
        assert_eq!(out, vec![128, 128, 128, 255]);
    }
}
