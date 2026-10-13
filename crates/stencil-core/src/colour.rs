//! Colour nodes: reduction to a flat palette, splitting a palette into masks, and tone patterns.

use crate::graph::{Mask, RgbImage};
use std::collections::HashMap;

fn srgb_to_lab(rgb: [u8; 3]) -> [f32; 3] {
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c > 0.04045 {
            ((c + 0.055) / 1.055).powf(2.4)
        } else {
            c / 12.92
        }
    };
    let (r, g, b) = (lin(rgb[0]), lin(rgb[1]), lin(rgb[2]));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn gaussian_blur(img: &RgbImage, sigma: f32) -> RgbImage {
    let r = (sigma * 3.0).ceil() as i32;
    let k: Vec<f32> = (-r..=r)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f32 = k.iter().sum();
    let (w, h) = (img.w as i32, img.h as i32);
    let pass = |src: &[f32], horizontal: bool| {
        let mut dst = vec![0f32; src.len()];
        for y in 0..h {
            for x in 0..w {
                for c in 0..3 {
                    let mut acc = 0.0;
                    for (i, kv) in k.iter().enumerate() {
                        let o = i as i32 - r;
                        let (sx, sy) = if horizontal {
                            ((x + o).clamp(0, w - 1), y)
                        } else {
                            (x, (y + o).clamp(0, h - 1))
                        };
                        acc += kv * src[((sy * w + sx) * 3) as usize + c];
                    }
                    dst[((y * w + x) * 3) as usize + c] = acc / total;
                }
            }
        }
        dst
    };
    let start: Vec<f32> = img.rgb.iter().map(|&v| v as f32).collect();
    let out = pass(&pass(&start, true), false);
    RgbImage {
        w: img.w,
        h: img.h,
        rgb: out
            .iter()
            .map(|v| v.round().clamp(0.0, 255.0) as u8)
            .collect(),
    }
}

/// Deterministic xorshift so the same picture always reduces to the same palette.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn dist2(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// Quantises to `k` colours with k-means in Lab space; every region ends up one flat colour.
pub fn reduce(img: &RgbImage, k: usize, blur_px: f32) -> RgbImage {
    let src = if blur_px > 0.0 {
        gaussian_blur(img, blur_px)
    } else {
        RgbImage {
            w: img.w,
            h: img.h,
            rgb: img.rgb.clone(),
        }
    };
    let lab: Vec<[f32; 3]> = src
        .rgb
        .chunks_exact(3)
        .map(|c| srgb_to_lab([c[0], c[1], c[2]]))
        .collect();
    let step = (lab.len() / 20000).max(1);
    let sample: Vec<[f32; 3]> = lab.iter().step_by(step).copied().collect();

    let mut rng = Rng(0x9E3779B97F4A7C15);
    let mut centres = vec![sample[(rng.next() * sample.len() as f64) as usize % sample.len()]];
    while centres.len() < k {
        let d: Vec<f32> = sample
            .iter()
            .map(|s| centres.iter().map(|c| dist2(s, c)).fold(f32::MAX, f32::min))
            .collect();
        let total: f32 = d.iter().sum();
        if total <= 0.0 {
            break;
        }
        let mut pick = rng.next() as f32 * total;
        let mut chosen = sample.len() - 1;
        for (i, v) in d.iter().enumerate() {
            if pick <= *v {
                chosen = i;
                break;
            }
            pick -= v;
        }
        centres.push(sample[chosen]);
    }
    let nearest = |p: &[f32; 3], centres: &[[f32; 3]]| {
        centres
            .iter()
            .enumerate()
            .map(|(i, c)| (i, dist2(p, c)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(0, |x| x.0)
    };
    for _ in 0..25 {
        let mut sum = vec![[0f32; 3]; centres.len()];
        let mut cnt = vec![0usize; centres.len()];
        for s in &sample {
            let i = nearest(s, &centres);
            for c in 0..3 {
                sum[i][c] += s[c];
            }
            cnt[i] += 1;
        }
        let mut moved = 0.0;
        for i in 0..centres.len() {
            if cnt[i] > 0 {
                let n = [
                    sum[i][0] / cnt[i] as f32,
                    sum[i][1] / cnt[i] as f32,
                    sum[i][2] / cnt[i] as f32,
                ];
                moved += dist2(&n, &centres[i]);
                centres[i] = n;
            }
        }
        if moved < 1e-3 {
            break;
        }
    }
    let idx: Vec<usize> = lab.iter().map(|p| nearest(p, &centres)).collect();
    let mut sum = vec![[0f64; 3]; centres.len()];
    let mut cnt = vec![0usize; centres.len()];
    for (p, &i) in src.rgb.chunks_exact(3).zip(&idx) {
        for c in 0..3 {
            sum[i][c] += p[c] as f64;
        }
        cnt[i] += 1;
    }
    let palette: Vec<[u8; 3]> = (0..centres.len())
        .map(|i| {
            if cnt[i] == 0 {
                [0; 3]
            } else {
                [0, 1, 2].map(|c| (sum[i][c] / cnt[i] as f64).round() as u8)
            }
        })
        .collect();
    let mut rgb = Vec::with_capacity(src.rgb.len());
    for &i in &idx {
        rgb.extend_from_slice(&palette[i]);
    }
    RgbImage {
        w: src.w,
        h: src.h,
        rgb,
    }
}

fn luminance(c: [u8; 3]) -> f64 {
    0.299 * c[0] as f64 + 0.587 * c[1] as f64 + 0.114 * c[2] as f64
}

/// The flat colours of an image with the index of each pixel, darkest first (lightest first if `invert`).
pub fn palette_of(img: &RgbImage, invert: bool) -> (Vec<[u8; 3]>, Vec<usize>) {
    let mut seen: HashMap<[u8; 3], usize> = HashMap::new();
    let mut colours: Vec<[u8; 3]> = Vec::new();
    let raw: Vec<usize> = img
        .rgb
        .chunks_exact(3)
        .map(|c| {
            *seen.entry([c[0], c[1], c[2]]).or_insert_with(|| {
                colours.push([c[0], c[1], c[2]]);
                colours.len() - 1
            })
        })
        .collect();
    let mut order: Vec<usize> = (0..colours.len()).collect();
    order.sort_by(|&a, &b| {
        let (la, lb) = (luminance(colours[a]), luminance(colours[b]));
        let o = la.total_cmp(&lb);
        if invert {
            o.reverse()
        } else {
            o
        }
    });
    let mut rank = vec![0; colours.len()];
    for (r, &o) in order.iter().enumerate() {
        rank[o] = r;
    }
    (
        order.iter().map(|&o| colours[o]).collect(),
        raw.iter().map(|&i| rank[i]).collect(),
    )
}

fn keep_large_regions(m: &mut [bool], w: usize, h: usize, min_px: usize) {
    let mut seen = vec![false; m.len()];
    let mut stack = Vec::new();
    for s in 0..m.len() {
        if !m[s] || seen[s] {
            continue;
        }
        let mut region = vec![s];
        seen[s] = true;
        stack.push(s);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            let mut visit = |j: usize| {
                if m[j] && !seen[j] {
                    seen[j] = true;
                    region.push(j);
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(i - 1)
            }
            if x + 1 < w {
                visit(i + 1)
            }
            if y > 0 {
                visit(i - w)
            }
            if y + 1 < h {
                visit(i + w)
            }
        }
        if region.len() < min_px {
            for j in region {
                m[j] = false;
            }
        }
    }
}

pub struct Split {
    pub masks: Vec<Mask>,
    pub palette: RgbImage,
    pub notes: Vec<String>,
}

/// One mask per flat colour; an image with more colours than `max_colours` is reduced first.
pub fn split(img: &RgbImage, invert: bool, max_colours: usize, min_area_pct: f64) -> Split {
    let mut notes = Vec::new();
    let (mut colours, mut idx) = palette_of(img, invert);
    if colours.len() > max_colours {
        notes.push(format!(
            "{} colours found: reducing to {max_colours}",
            colours.len()
        ));
        let r = reduce(img, max_colours, 0.0);
        (colours, idx) = palette_of(&r, invert);
    }
    let total = img.w * img.h;
    let mut masks = Vec::new();
    for (k, c) in colours.iter().enumerate() {
        let mut paint: Vec<bool> = idx.iter().map(|&i| i == k).collect();
        if min_area_pct > 0.0 {
            keep_large_regions(
                &mut paint,
                img.w,
                img.h,
                (min_area_pct / 100.0 * total as f64).ceil() as usize,
            );
        }
        let pct = paint.iter().filter(|&&p| p).count() as f64 * 100.0 / total as f64;
        notes.push(format!(
            "colour {}: ({}, {}, {}), {pct:.1}% of the image",
            k + 1,
            c[0],
            c[1],
            c[2]
        ));
        masks.push(Mask {
            w: img.w,
            h: img.h,
            paint,
        });
    }
    let sw = 40;
    let (pw, ph) = (sw * colours.len().max(1), sw);
    let mut rgb = vec![0u8; pw * ph * 3];
    for y in 0..ph {
        for x in 0..pw {
            rgb[(y * pw + x) * 3..][..3]
                .copy_from_slice(&colours[(x / sw).min(colours.len().saturating_sub(1))]);
        }
    }
    Split {
        masks,
        palette: RgbImage { w: pw, h: ph, rgb },
        notes,
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Pattern {
    Lines,
    Dots,
    Cross,
    Checker,
    Solid,
    Empty,
}

impl Pattern {
    pub fn parse(s: &str) -> Option<Pattern> {
        Some(match s {
            "lines" => Pattern::Lines,
            "dots" => Pattern::Dots,
            "crosshatch" => Pattern::Cross,
            "checker" => Pattern::Checker,
            "solid" => Pattern::Solid,
            "empty" => Pattern::Empty,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Pattern::Lines => "lines",
            Pattern::Dots => "dots",
            Pattern::Cross => "crosshatch",
            Pattern::Checker => "checker",
            Pattern::Solid => "solid",
            Pattern::Empty => "empty",
        }
    }
}

fn lines(x: f64, y: f64, spacing: f64, cov: f64, angle: f64) -> bool {
    let a = angle.to_radians();
    (x * a.cos() + y * a.sin()).rem_euclid(spacing) < cov * spacing
}

fn dots(x: f64, y: f64, spacing: f64, cov: f64, angle: f64) -> bool {
    let a = angle.to_radians();
    let (u, v) = (x * a.cos() + y * a.sin(), -x * a.sin() + y * a.cos());
    let row = spacing * 3f64.sqrt() / 2.0;
    let r = (cov * spacing * row / std::f64::consts::PI)
        .sqrt()
        .min(spacing * 0.5);
    [(0.0, 0.0), (spacing / 2.0, row)].iter().any(|&(ou, ov)| {
        let du = (u - ou + spacing / 2.0).rem_euclid(spacing) - spacing / 2.0;
        let dv = (v - ov + row).rem_euclid(2.0 * row) - row;
        du * du + dv * dv <= r * r
    })
}

fn checker(x: f64, y: f64, spacing: f64, angle: f64) -> bool {
    let a = angle.to_radians();
    let (u, v) = (x * a.cos() + y * a.sin(), -x * a.sin() + y * a.cos());
    ((u / spacing).floor() + (v / spacing).floor()).rem_euclid(2.0) == 0.0
}

pub fn pattern_at(kind: Pattern, x: f64, y: f64, spacing: f64, cov: f64, angle: f64) -> bool {
    match kind {
        Pattern::Lines => lines(x, y, spacing, cov, angle),
        Pattern::Dots => dots(x, y, spacing, cov, angle),
        Pattern::Cross => {
            let c = 1.0 - (1.0 - cov).sqrt();
            lines(x, y, spacing, c, angle) || lines(x, y, spacing, c, angle + 90.0)
        }
        Pattern::Checker => checker(x, y, spacing, angle),
        Pattern::Solid => true,
        Pattern::Empty => false,
    }
}

/// Per-tone overrides; `kind: None` means automatic, `spacing`/`cover` of 0 mean the defaults.
#[derive(Clone, Copy)]
pub struct Tone {
    pub kind: Option<Pattern>,
    pub spacing_mm: f64,
    pub cover_pct: f64,
}

pub struct ToneParams {
    pub plate_width_mm: f64,
    pub invert: bool,
    pub default_kind: Option<Pattern>,
    pub spacing_mm: f64,
    pub angle: f64,
    pub tones: Vec<Tone>,
}

pub struct Toned {
    pub mask: Mask,
    pub tones: RgbImage,
    pub notes: Vec<String>,
}

pub const MAX_RUNGS: usize = 8;

/// Ranks the colours by lightness: darkest solid, lightest empty, the tones between get thinning patterns.
pub fn tone_patterns(img: &RgbImage, p: &ToneParams) -> Result<Toned, String> {
    let (colours, tone_idx) = palette_of(img, p.invert);
    let n = colours.len();
    if n > MAX_RUNGS + 2 {
        return Err(format!(
            "{n} colours found, at most {} are supported: reduce the colours first",
            MAX_RUNGS + 2
        ));
    }
    let mm = p.plate_width_mm / img.w as f64;
    let mut notes = Vec::new();
    let mut levels: Vec<(Pattern, f64, f64)> = Vec::new();
    for r in 0..n {
        let (kind, mut cov, spacing);
        if r == 0 {
            (kind, cov) = (Some(Pattern::Solid), 1.0);
            spacing = 0.0;
        } else if r == n - 1 {
            (kind, cov) = (Some(Pattern::Empty), 0.0);
            spacing = 0.0;
        } else {
            let t = p.tones.get(r - 1).copied().unwrap_or(Tone {
                kind: None,
                spacing_mm: 0.0,
                cover_pct: 0.0,
            });
            kind = t.kind.or(p.default_kind);
            cov = t.cover_pct / 100.0;
            if cov == 0.0 {
                cov = 1.0 - r as f64 / (n - 1) as f64;
            }
            spacing = if t.spacing_mm > 0.0 {
                t.spacing_mm
            } else {
                p.spacing_mm
            };
        }
        let mut kind = kind.unwrap_or(if cov >= 0.5 {
            Pattern::Lines
        } else {
            Pattern::Dots
        });
        if spacing <= 0.0 && kind != Pattern::Solid && kind != Pattern::Empty {
            kind = Pattern::Solid;
        }
        if r > 0 && r < n - 1 {
            notes.push(format!(
                "tone {r}: {} {spacing} mm, {:.0}%",
                kind.name(),
                cov * 100.0
            ));
        }
        levels.push((kind, cov, spacing));
    }
    let mut paint = vec![false; img.w * img.h];
    for (i, &r) in tone_idx.iter().enumerate() {
        let (kind, cov, spacing) = levels[r];
        let angle = if kind == Pattern::Dots {
            p.angle
        } else {
            p.angle + (r % 2) as f64 * 90.0
        };
        let (x, y) = ((i % img.w) as f64 * mm, (i / img.w) as f64 * mm);
        paint[i] = pattern_at(kind, x, y, spacing, cov, angle);
    }
    let rgb = tone_idx
        .iter()
        .flat_map(|&r| {
            let g = (255.0 * (1.0 - levels[r].1)) as u8;
            [g, g, g]
        })
        .collect();
    Ok(Toned {
        mask: Mask {
            w: img.w,
            h: img.h,
            paint,
        },
        tones: RgbImage {
            w: img.w,
            h: img.h,
            rgb,
        },
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient() -> RgbImage {
        let (w, h) = (60, 20);
        let mut rgb = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                let g = (x * 255 / (w - 1)) as u8;
                rgb.extend_from_slice(&[g, g, g]);
            }
        }
        RgbImage { w, h, rgb }
    }

    #[test]
    fn reduce_gives_n_flat_colours_and_split_orders_them() {
        let r = reduce(&gradient(), 3, 0.0);
        let (colours, _) = palette_of(&r, false);
        assert_eq!(colours.len(), 3);
        assert!(luminance(colours[0]) < luminance(colours[2]));
        let s = split(&r, false, 10, 0.0);
        assert_eq!(s.masks.len(), 3);
        let covered: usize = (0..r.w * r.h)
            .filter(|&i| s.masks.iter().filter(|m| m.paint[i]).count() == 1)
            .count();
        assert_eq!(covered, r.w * r.h);
    }

    #[test]
    fn tones_make_darkest_solid_and_lightest_empty() {
        let r = reduce(&gradient(), 3, 0.0);
        let t = tone_patterns(
            &r,
            &ToneParams {
                plate_width_mm: 60.0,
                invert: false,
                default_kind: None,
                spacing_mm: 2.0,
                angle: 45.0,
                tones: vec![],
            },
        )
        .unwrap();
        assert!(t.mask.paint[0], "darkest is solid paint");
        assert!(!t.mask.paint[r.w - 1], "lightest is empty");
        let mid_count = (0..r.w).filter(|&x| t.mask.paint[x]).count();
        assert!(mid_count > 0 && mid_count < r.w);
    }

    #[test]
    fn too_many_colours_for_tones_is_an_error() {
        assert!(tone_patterns(
            &gradient(),
            &ToneParams {
                plate_width_mm: 60.0,
                invert: false,
                default_kind: None,
                spacing_mm: 2.0,
                angle: 45.0,
                tones: vec![]
            }
        )
        .is_err());
    }
}
