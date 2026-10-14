//! Mask smoothing: nearest-neighbour supersampling, gaussian blur of the 0/1 mask, threshold at 0.5.

pub const MAX_PIXELS: usize = 25_000_000;

fn kernel(sigma: f32) -> Vec<f32> {
    let r = (sigma * 3.0).ceil() as i64;
    let mut k: Vec<f32> = (-r..=r)
        .map(|i| (-((i * i) as f32) / (2.0 * sigma * sigma)).exp())
        .collect();
    let s: f32 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= s);
    k
}

fn blur_pass(src: &[f32], w: usize, h: usize, k: &[f32], horizontal: bool) -> Vec<f32> {
    let r = (k.len() / 2) as i64;
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for (j, kv) in k.iter().enumerate() {
                let d = j as i64 - r;
                let (sx, sy) = if horizontal {
                    ((x as i64 + d).clamp(0, w as i64 - 1), y as i64)
                } else {
                    (x as i64, (y as i64 + d).clamp(0, h as i64 - 1))
                };
                acc += kv * src[sy as usize * w + sx as usize];
            }
            out[y * w + x] = acc;
        }
    }
    out
}

/// Smooths `paint` (w x h). `radius_px` is the gaussian sigma in source pixels; the result is `factor` times larger
/// in each direction. Returns (width, height, mask).
pub fn smooth(
    w: usize,
    h: usize,
    paint: &[bool],
    radius_px: f32,
    factor: usize,
) -> Result<(usize, usize, Vec<bool>), String> {
    let f = factor.clamp(1, 4);
    let (nw, nh) = (w * f, h * f);
    if nw * nh > MAX_PIXELS {
        return Err(format!(
            "the smoothed mask would be {nw}x{nh} pixels: lower the supersample or the source resolution"
        ));
    }
    if radius_px <= 0.0 && f == 1 {
        return Ok((w, h, paint.to_vec()));
    }
    let up: Vec<f32> = (0..nw * nh)
        .map(|i| paint[(i / nw / f) * w + (i % nw) / f] as u8 as f32)
        .collect();
    if radius_px <= 0.0 {
        return Ok((nw, nh, up.iter().map(|&v| v > 0.5).collect()));
    }
    crate::progress::step("supersample", 0.15);
    let k = kernel(radius_px * f as f32);
    let across = blur_pass(&up, nw, nh, &k, true);
    crate::progress::step("blur rows", 0.55);
    let blurred = blur_pass(&across, nw, nh, &k, false);
    crate::progress::step("blur columns", 0.95);
    Ok((nw, nh, blurred.iter().map(|&v| v > 0.5).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_corners_and_scales() {
        let (w, h) = (20, 20);
        let paint: Vec<bool> = (0..w * h)
            .map(|i| (5..15).contains(&(i % w)) && (5..15).contains(&(i / w)))
            .collect();
        let (nw, nh, m) = smooth(w, h, &paint, 2.0, 2).unwrap();
        assert_eq!((nw, nh), (40, 40));
        assert!(!m[10 * nw + 10]);
        assert!(m[20 * nw + 20]);
        let n = m.iter().filter(|&&v| v).count();
        assert!(n < 400 && n > 300);
        let (_, _, same) = smooth(w, h, &paint, 0.0, 1).unwrap();
        assert_eq!(same, paint);
    }
}
