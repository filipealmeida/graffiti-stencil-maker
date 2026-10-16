//! Image transform: crop, flip, quarter turns and a free rotation that grows the canvas to fit.

use crate::graph::RgbImage;

pub struct Params {
    /// Fractions (0..1) of the width/height removed from each side.
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub flip_h: bool,
    pub flip_v: bool,
    /// Clockwise quarter turns.
    pub quarter_turns: u32,
    /// Free clockwise rotation in degrees; the empty corners are filled white.
    pub angle: f64,
}

const MAX_PIXELS: usize = 25_000_000;

fn crop(img: &RgbImage, p: &Params) -> Result<RgbImage, String> {
    let x0 = (p.left * img.w as f64).round() as usize;
    let y0 = (p.top * img.h as f64).round() as usize;
    let x1 = img.w - (p.right * img.w as f64).round() as usize;
    let y1 = img.h - (p.bottom * img.h as f64).round() as usize;
    if x1 <= x0 || y1 <= y0 || x1 - x0 < 2 || y1 - y0 < 2 {
        return Err("the crop leaves nothing of the picture: reduce the crop margins".into());
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let mut rgb = Vec::with_capacity(w * h * 3);
    for y in y0..y1 {
        rgb.extend_from_slice(&img.rgb[(y * img.w + x0) * 3..(y * img.w + x1) * 3]);
    }
    Ok(RgbImage { w, h, rgb })
}

fn flip(img: &RgbImage, h_flip: bool, v_flip: bool) -> RgbImage {
    let mut rgb = vec![0u8; img.rgb.len()];
    for y in 0..img.h {
        for x in 0..img.w {
            let sx = if h_flip { img.w - 1 - x } else { x };
            let sy = if v_flip { img.h - 1 - y } else { y };
            let (d, s) = ((y * img.w + x) * 3, (sy * img.w + sx) * 3);
            rgb[d..d + 3].copy_from_slice(&img.rgb[s..s + 3]);
        }
    }
    RgbImage {
        w: img.w,
        h: img.h,
        rgb,
    }
}

fn quarter(img: &RgbImage) -> RgbImage {
    let (w, h) = (img.h, img.w);
    let mut rgb = vec![0u8; img.rgb.len()];
    for y in 0..img.h {
        for x in 0..img.w {
            // clockwise: source (x, y) lands at (h-1-y, x)
            let (dx, dy) = (img.h - 1 - y, x);
            let (d, s) = ((dy * w + dx) * 3, (y * img.w + x) * 3);
            rgb[d..d + 3].copy_from_slice(&img.rgb[s..s + 3]);
        }
    }
    RgbImage { w, h, rgb }
}

fn rotate(img: &RgbImage, deg: f64) -> Result<RgbImage, String> {
    let (s, c) = deg.to_radians().sin_cos();
    let (sw, sh) = (img.w as f64, img.h as f64);
    let w = (sw * c.abs() + sh * s.abs()).ceil() as usize;
    let h = (sw * s.abs() + sh * c.abs()).ceil() as usize;
    if w * h > MAX_PIXELS {
        return Err("the rotated picture would be too large".into());
    }
    let mut rgb = vec![255u8; w * h * 3];
    let (cx, cy, ox, oy) = (sw / 2.0, sh / 2.0, w as f64 / 2.0, h as f64 / 2.0);
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f64 + 0.5 - ox, y as f64 + 0.5 - oy);
            // inverse of a clockwise rotation (y points down)
            let sx = dx * c + dy * s + cx - 0.5;
            let sy = -dx * s + dy * c + cy - 0.5;
            if sx < -0.5 || sy < -0.5 || sx > sw - 0.5 || sy > sh - 0.5 {
                continue;
            }
            let (fx, fy) = (sx.clamp(0.0, sw - 1.0), sy.clamp(0.0, sh - 1.0));
            let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
            let (x1, y1) = ((x0 + 1).min(img.w - 1), (y0 + 1).min(img.h - 1));
            let (tx, ty) = (fx - x0 as f64, fy - y0 as f64);
            for k in 0..3 {
                let px = |xx: usize, yy: usize| img.rgb[(yy * img.w + xx) * 3 + k] as f64;
                let top = px(x0, y0) * (1.0 - tx) + px(x1, y0) * tx;
                let bot = px(x0, y1) * (1.0 - tx) + px(x1, y1) * tx;
                rgb[(y * w + x) * 3 + k] = (top * (1.0 - ty) + bot * ty).round() as u8;
            }
        }
    }
    Ok(RgbImage { w, h, rgb })
}

pub fn transform(img: &RgbImage, p: &Params) -> Result<RgbImage, String> {
    let mut out = crop(img, p)?;
    if p.flip_h || p.flip_v {
        out = flip(&out, p.flip_h, p.flip_v);
    }
    for _ in 0..p.quarter_turns % 4 {
        out = quarter(&out);
    }
    if p.angle.abs() > 0.01 {
        out = rotate(&out, p.angle)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img() -> RgbImage {
        // 4 x 2, pixel value = index
        RgbImage {
            w: 4,
            h: 2,
            rgb: (0..8u8).flat_map(|v| [v, v, v]).collect(),
        }
    }
    fn p() -> Params {
        Params {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            flip_h: false,
            flip_v: false,
            quarter_turns: 0,
            angle: 0.0,
        }
    }
    fn px(i: &RgbImage) -> Vec<u8> {
        i.rgb.iter().step_by(3).copied().collect()
    }

    #[test]
    fn crops_margins() {
        let o = transform(&img(), &Params { left: 0.25, ..p() }).unwrap();
        assert_eq!((o.w, o.h), (3, 2));
        assert_eq!(px(&o), vec![1, 2, 3, 5, 6, 7]);
        assert!(transform(
            &img(),
            &Params {
                left: 0.5,
                right: 0.5,
                ..p()
            }
        )
        .is_err());
    }

    #[test]
    fn quarter_turn_is_clockwise_and_four_is_identity() {
        let o = transform(
            &img(),
            &Params {
                quarter_turns: 1,
                ..p()
            },
        )
        .unwrap();
        assert_eq!((o.w, o.h), (2, 4));
        assert_eq!(px(&o), vec![4, 0, 5, 1, 6, 2, 7, 3]);
        assert_eq!(
            px(&transform(
                &img(),
                &Params {
                    quarter_turns: 4,
                    ..p()
                }
            )
            .unwrap()),
            px(&img())
        );
    }

    #[test]
    fn flips_mirror() {
        let o = transform(
            &img(),
            &Params {
                flip_h: true,
                ..p()
            },
        )
        .unwrap();
        assert_eq!(px(&o), vec![3, 2, 1, 0, 7, 6, 5, 4]);
    }

    #[test]
    fn free_rotation_grows_the_canvas_and_fills_white() {
        let src = RgbImage {
            w: 20,
            h: 20,
            rgb: vec![0; 20 * 20 * 3],
        };
        let o = transform(&src, &Params { angle: 45.0, ..p() }).unwrap();
        assert!(o.w >= 28 && o.w <= 30 && o.w == o.h);
        assert_eq!(&o.rgb[0..3], &[255, 255, 255]);
        let mid = (o.h / 2 * o.w + o.w / 2) * 3;
        assert_eq!(&o.rgb[mid..mid + 3], &[0, 0, 0]);
    }
}
