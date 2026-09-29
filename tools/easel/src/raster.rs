//! Low-level raster helpers: Gaussian-approximating blurs and pixel sampling.

use rayon::prelude::*;
use tiny_skia::{Mask, Pixmap};

use crate::color::Rgba;

/// Box radii for three successive box blurs approximating a Gaussian of
/// standard deviation `sigma` (Kutskir's "boxes for Gauss").
fn gauss_box_radii(sigma: f32) -> [usize; 3] {
    const N: f32 = 3.0;
    let ideal = (12.0 * sigma * sigma / N + 1.0).sqrt();
    let mut lower = ideal.floor() as i64;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let lower = lower.max(1) as f32;
    let upper = lower + 2.0;
    let m_ideal = (12.0 * sigma * sigma - N * lower * lower - 4.0 * N * lower - 3.0 * N)
        / (-4.0 * lower - 4.0);
    let m = m_ideal.round() as i64;
    let mut radii = [0usize; 3];
    for (i, r) in radii.iter_mut().enumerate() {
        let width = if (i as i64) < m { lower } else { upper };
        *r = ((width - 1.0) / 2.0) as usize;
    }
    radii
}

/// One horizontal box blur over rows of `C`-channel interleaved,
/// non-negative fixed-point pixels, clamping at the edges.
///
/// Window sums are exact integers and each output is the rounded mean, so
/// a pixel's result does not depend on where its row starts. That keeps a
/// region render (see `Viewport`) identical to the full render.
fn box_rows<const C: usize>(data: &mut [i32], width: usize, radius: usize) {
    if radius == 0 || width == 0 {
        return;
    }
    let n = (2 * radius + 1) as i64;
    data.par_chunks_mut(width * C).for_each(|row| {
        let src = row.to_vec();
        let at = |x: isize, c: usize| src[x.clamp(0, width as isize - 1) as usize * C + c] as i64;
        let r = radius as isize;
        for c in 0..C {
            let mut sum: i64 = (-r..=r).map(|k| at(k, c)).sum();
            for x in 0..width as isize {
                row[x as usize * C + c] = ((sum + n / 2) / n) as i32;
                sum += at(x + r + 1, c) - at(x - r, c);
            }
        }
    });
}

fn transpose<const C: usize>(data: &[i32], width: usize, height: usize) -> Vec<i32> {
    let mut out = vec![0i32; data.len()];
    out.par_chunks_mut(height * C)
        .enumerate()
        .for_each(|(x, col)| {
            for y in 0..height {
                let src = (y * width + x) * C;
                col[y * C..y * C + C].copy_from_slice(&data[src..src + C]);
            }
        });
    out
}

/// Gaussian-approximate blur of an interleaved `C`-channel image of
/// non-negative fixed-point values (scale them up so rounding to whole
/// numbers after each pass loses nothing that matters).
pub fn blur<const C: usize>(data: &mut Vec<i32>, width: usize, height: usize, sigma: f32) {
    if sigma < 0.2 || width == 0 || height == 0 {
        return;
    }
    let radii = gauss_box_radii(sigma);
    for &r in &radii {
        box_rows::<C>(data, width, r);
    }
    let mut t = transpose::<C>(data, width, height);
    for &r in &radii {
        box_rows::<C>(&mut t, height, r);
    }
    *data = transpose::<C>(&t, height, width);
}

/// Fixed-point factor for blurring 8-bit data.
const BYTE_FIXED: i32 = 256;

fn byte_from_fixed(v: i32) -> u8 {
    ((v + BYTE_FIXED / 2) / BYTE_FIXED).clamp(0, 255) as u8
}

/// A pixel rectangle `x0, y0, x1, y1` (end-exclusive).
pub type PxRect = (usize, usize, usize, usize);

/// How far (px) a [`blur`] at `sigma` reaches: a pixel only depends on
/// pixels within this distance along each axis.
pub fn blur_support(sigma: f32) -> usize {
    if sigma < 0.2 {
        0
    } else {
        gauss_box_radii(sigma).iter().sum()
    }
}

/// `rect` grown by `by` px on every side, clipped to `width x height`.
pub fn grow_rect(rect: PxRect, by: usize, width: usize, height: usize) -> PxRect {
    (
        rect.0.saturating_sub(by),
        rect.1.saturating_sub(by),
        (rect.2 + by).min(width),
        (rect.3 + by).min(height),
    )
}

/// Device bounding box of `path` grown by `by` px, clipped to the image.
pub fn path_rect(path: &tiny_skia::Path, by: usize, width: u32, height: u32) -> PxRect {
    let b = path.bounds();
    let clamp = |v: f32, hi: u32| v.clamp(0.0, hi as f32) as usize;
    grow_rect(
        (
            clamp(b.left().floor(), width),
            clamp(b.top().floor(), height),
            clamp(b.right().ceil() + 1.0, width),
            clamp(b.bottom().ceil() + 1.0, height),
        ),
        by,
        width as usize,
        height as usize,
    )
}

/// Blur the `rect` part of an interleaved `C`-channel 8-bit image in place.
/// Pixels inside `rect` that are at least [`blur_support`] from its
/// border (or on the image border) get exactly the whole-image result;
/// pixels outside `rect` are left alone.
fn blur_bytes<const C: usize>(data: &mut [u8], width: usize, rect: PxRect, sigma: f32) {
    let (x0, y0, x1, y1) = rect;
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let (rw, rh) = (x1 - x0, y1 - y0);
    let mut fixed: Vec<i32> = Vec::with_capacity(rw * rh * C);
    for y in y0..y1 {
        let row = &data[(y * width + x0) * C..(y * width + x1) * C];
        fixed.extend(row.iter().map(|&b| b as i32 * BYTE_FIXED));
    }
    blur::<C>(&mut fixed, rw, rh, sigma);
    for (y, src) in (y0..y1).zip(fixed.chunks(rw * C)) {
        let row = &mut data[(y * width + x0) * C..(y * width + x1) * C];
        for (d, &v) in row.iter_mut().zip(src) {
            *d = byte_from_fixed(v);
        }
    }
}

/// Blur the `rect` part of a premultiplied pixmap in place (see
/// `blur_bytes` for which pixels are exact).
pub fn blur_pixmap(pixmap: &mut Pixmap, rect: PxRect, sigma: f32) {
    let w = pixmap.width() as usize;
    blur_bytes::<4>(pixmap.data_mut(), w, rect, sigma);
    let (x0, y0, x1, y1) = rect;
    for y in y0..y1 {
        let row = &mut pixmap.data_mut()[(y * w + x0) * 4..(y * w + x1) * 4];
        for px in row.as_chunks_mut::<4>().0 {
            // Keep the premultiplied invariant (colour <= alpha).
            for c in 0..3 {
                px[c] = px[c].min(px[3]);
            }
        }
    }
}

/// Blur the `rect` part of a coverage mask in place.
pub fn blur_mask(mask: &mut Mask, rect: PxRect, sigma: f32) {
    let w = mask.width() as usize;
    blur_bytes::<1>(mask.data_mut(), w, rect, sigma);
}

/// The whole image as a [`PxRect`].
pub fn full_rect(width: u32, height: u32) -> PxRect {
    (0, 0, width as usize, height as usize)
}

/// Multiply `mask` by `other` (same size) in place: the intersection of
/// two coverages.
pub fn multiply_mask(mask: &mut Mask, other: &Mask) {
    for (m, &o) in mask.data_mut().iter_mut().zip(other.data()) {
        *m = ((*m as u16 * o as u16 + 127) / 255) as u8;
    }
}

/// Premultiplied pixel at device coordinates, transparent outside.
pub fn pixel_at(pixmap: &Pixmap, x: f32, y: f32) -> [u8; 4] {
    let (xi, yi) = (x.floor(), y.floor());
    if xi < 0.0 || yi < 0.0 || xi >= pixmap.width() as f32 || yi >= pixmap.height() as f32 {
        return [0; 4];
    }
    let i = (yi as usize * pixmap.width() as usize + xi as usize) * 4;
    let d = pixmap.data();
    [d[i], d[i + 1], d[i + 2], d[i + 3]]
}

/// Straight-alpha colour at device coordinates.
pub fn color_at(pixmap: &Pixmap, x: f32, y: f32) -> Rgba {
    Rgba::from_premultiplied_u8(pixel_at(pixmap, x, y))
}

fn on_pixmap(pixmap: &Pixmap, x: f32, y: f32) -> bool {
    (0.0..pixmap.width() as f32).contains(&x) && (0.0..pixmap.height() as f32).contains(&y)
}

/// Average premultiplied colour over a disc of `radius` device pixels,
/// returned as straight-alpha colour. Only on-canvas pixels count; `None`
/// when the whole disc (or the point) is off the canvas.
pub fn average_disc(pixmap: &Pixmap, x: f32, y: f32, radius: f32) -> Option<Rgba> {
    if radius <= 0.5 {
        return on_pixmap(pixmap, x, y).then(|| color_at(pixmap, x, y));
    }
    let mut sum = [0.0f64; 4];
    let mut n = 0.0f64;
    let r = radius.ceil() as i64;
    for dy in -r..=r {
        for dx in -r..=r {
            let (px, py) = (x + dx as f32, y + dy as f32);
            if (dx * dx + dy * dy) as f32 > radius * radius || !on_pixmap(pixmap, px, py) {
                continue;
            }
            let px = pixel_at(pixmap, px, py);
            for (s, v) in sum.iter_mut().zip(px) {
                *s += v as f64;
            }
            n += 1.0;
        }
    }
    if n == 0.0 {
        return None;
    }
    let avg = sum.map(|s| (s / n).round().clamp(0.0, 255.0) as u8);
    Some(Rgba::from_premultiplied_u8(avg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_preserves_mass_and_spreads() {
        let (w, h) = (41, 41);
        let mut data = vec![0i32; w * h];
        data[20 * w + 20] = 1_000_000;
        blur::<1>(&mut data, w, h, 3.0);
        let total: i32 = data.iter().sum();
        assert!((total - 1_000_000).abs() < 1000, "{total}");
        assert!(data[20 * w + 20] < 100_000);
        assert!(data[20 * w + 23] > 0);
        assert_eq!(data[20 * w + 19], data[20 * w + 21]);
    }

    #[test]
    fn blur_does_not_depend_on_where_rows_start() {
        let (w, h) = (60, 9);
        let data: Vec<i32> = (0..w * h).map(|i| ((i * 7919) % 5003) as i32).collect();
        let mut full = data.clone();
        blur::<1>(&mut full, w, h, 2.0);
        // The same rows without their first 20 columns (as a region render
        // would see them): identical away from the new left edge.
        let mut part: Vec<i32> = data.chunks(w).flat_map(|row| row[20..].to_vec()).collect();
        blur::<1>(&mut part, w - 20, h, 2.0);
        for y in 2..h - 2 {
            for x in 40..w {
                assert_eq!(full[y * w + x], part[y * (w - 20) + x - 20]);
            }
        }
    }

    #[test]
    fn region_blur_matches_whole_blur() {
        let mut whole = Mask::new(80, 60).unwrap();
        for (i, m) in whole.data_mut().iter_mut().enumerate() {
            let (x, y) = (i % 80, i / 80);
            *m = if (30..40).contains(&x) && (20..30).contains(&y) {
                255
            } else {
                0
            };
        }
        let mut region = whole.clone();
        let sigma = 3.0;
        blur_mask(&mut whole, full_rect(80, 60), sigma);
        let rect = grow_rect((30, 20, 40, 30), blur_support(sigma) + 1, 80, 60);
        blur_mask(&mut region, rect, sigma);
        assert_eq!(whole.data(), region.data());
    }

    #[test]
    fn disc_average_ignores_off_canvas() {
        let mut pm = Pixmap::new(10, 10).unwrap();
        pm.fill(tiny_skia::Color::WHITE);
        let edge = average_disc(&pm, 0.5, 0.5, 3.0).unwrap();
        assert!((edge.a - 1.0).abs() < 1e-6);
        assert!(average_disc(&pm, 20.0, 20.0, 3.0).is_none());
        assert!(average_disc(&pm, -1.0, 5.0, 0.0).is_none());
    }

    #[test]
    fn box_radii_grow_with_sigma() {
        let small = gauss_box_radii(1.0);
        let large = gauss_box_radii(10.0);
        assert!(large.iter().sum::<usize>() > small.iter().sum::<usize>());
    }
}
