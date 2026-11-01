//! The Place and Collage nodes: layers of pictures, paint masks and outlines positioned on one canvas.

use super::catalogue::Outputs;
use super::*;
use crate::trace::{Cubic, Loop, Shapes};

pub(super) const MAX_LAYERS: usize = 12;
pub(super) const INPUTS: [&str; MAX_LAYERS] = [
    "layer_1", "layer_2", "layer_3", "layer_4", "layer_5", "layer_6", "layer_7", "layer_8",
    "layer_9", "layer_10", "layer_11", "layer_12",
];

const MAX_SIDE: f64 = 16000.0;
const MAX_PIXELS: f64 = 80e6;

fn art_of(v: &Value) -> Option<(Art, f64, f64, f64)> {
    Some(match v {
        Value::Image(i) => (Art::Image(i.clone()), 0.0, 0.0, 1.0),
        Value::Mask(m) => (Art::Mask(m.clone()), 0.0, 0.0, 1.0),
        Value::Shapes(s) => (Art::Shapes(s.clone()), 0.0, 0.0, 1.0),
        Value::Layer(l) => {
            let art = match &l.art {
                Art::Image(i) => Art::Image(i.clone()),
                Art::Mask(m) => Art::Mask(m.clone()),
                Art::Shapes(s) => Art::Shapes(s.clone()),
            };
            (art, l.x, l.y, l.scale)
        }
        _ => return None,
    })
}

pub(super) fn run_place(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let (art, x, y, scale) = i.get("item").and_then(art_of).ok_or(
        "nothing is connected to the 'content' input: connect a picture, a paint mask or outlines",
    )?;
    let width = p.f("width_px");
    let scale = if width > 0.0 {
        width / art.size().0.max(1.0)
    } else {
        scale * p.f("scale_pct") / 100.0
    };
    Ok(vec![(
        "layer",
        Value::Layer(Rc::new(Layer {
            art,
            x: x + p.f("move_x"),
            y: y + p.f("move_y"),
            scale,
        })),
    )])
}

/// A layer with its slot on the canvas, in canvas pixels before the origin is chosen.
struct Placed {
    art: Art,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    scale: f64,
}

fn arrange(layers: &mut [Placed], how: &str, gap: f64, align: &str) {
    if how == "manual" {
        return;
    }
    let row = how == "row";
    let cross = layers
        .iter()
        .map(|l| if row { l.h } else { l.w })
        .fold(0.0, f64::max);
    let mut at = 0.0;
    for l in layers.iter_mut() {
        let (main, side) = if row { (l.w, l.h) } else { (l.h, l.w) };
        let off = match align {
            "centre" => (cross - side) / 2.0,
            "end" => cross - side,
            _ => 0.0,
        };
        if row {
            (l.x, l.y) = (l.x + at, l.y + off);
        } else {
            (l.x, l.y) = (l.x + off, l.y + at);
        }
        at += main + gap;
    }
}

fn overlaps(a: &Placed, b: &Placed) -> bool {
    let e = 1e-6;
    a.x + a.w > b.x + e && b.x + b.w > a.x + e && a.y + a.h > b.y + e && b.y + b.h > a.y + e
}

fn resized_rgb(img: &RgbImage, w: usize, h: usize) -> Vec<u8> {
    if img.w == w && img.h == h {
        return img.rgb.clone();
    }
    let buf = image::RgbImage::from_raw(img.w as u32, img.h as u32, img.rgb.clone())
        .expect("pixel buffer matches its size");
    image::imageops::resize(
        &buf,
        w as u32,
        h as u32,
        image::imageops::FilterType::Triangle,
    )
    .into_raw()
}

/// The paint of a layer at its placed size.
fn paint_of(art: &Art, w: usize, h: usize) -> Vec<bool> {
    match art {
        Art::Mask(m) if m.w == w && m.h == h => m.paint.clone(),
        Art::Mask(m) => {
            let px: Vec<u8> = m.paint.iter().map(|&p| if p { 255 } else { 0 }).collect();
            let buf = image::GrayImage::from_raw(m.w as u32, m.h as u32, px)
                .expect("pixel buffer matches its size");
            image::imageops::resize(
                &buf,
                w as u32,
                h as u32,
                image::imageops::FilterType::Triangle,
            )
            .into_raw()
            .into_iter()
            .map(|v| v >= 128)
            .collect()
        }
        Art::Shapes(s) => crate::trace::rasterise(s, w, h),
        Art::Image(_) => vec![false; w * h],
    }
}

/// Visits the pixels of a `tw` x `th` tile at (`x0`, `y0`) that fall inside a `cw` x `ch` canvas.
fn each_pixel(
    cw: usize,
    ch: usize,
    x0: i64,
    y0: i64,
    tw: usize,
    th: usize,
    mut f: impl FnMut(usize, usize),
) {
    for ty in 0..th {
        let cy = y0 + ty as i64;
        if cy < 0 || cy >= ch as i64 {
            continue;
        }
        for tx in 0..tw {
            let cx = x0 + tx as i64;
            if cx >= 0 && cx < cw as i64 {
                f(cy as usize * cw + cx as usize, ty * tw + tx);
            }
        }
    }
}

fn moved(s: &Shapes, k: f64, dx: f64, dy: f64) -> Vec<Loop> {
    let m = |p: [f64; 2]| [p[0] * k + dx, p[1] * k + dy];
    s.loops
        .iter()
        .map(|l| Loop {
            segs: l
                .segs
                .iter()
                .map(|c| Cubic([m(c.0[0]), m(c.0[1]), m(c.0[2]), m(c.0[3])]))
                .collect(),
        })
        .collect()
}

pub(super) fn run_canvas(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let mut layers: Vec<Placed> = INPUTS
        .iter()
        .filter_map(|n| i.get(*n))
        .filter_map(art_of)
        .map(|(art, x, y, scale)| {
            let (w, h) = art.size();
            Placed {
                art,
                x,
                y,
                w: (w * scale).max(1.0),
                h: (h * scale).max(1.0),
                scale,
            }
        })
        .collect();
    if layers.is_empty() {
        return Err("connect at least one picture, paint mask or outlines to layer_1".into());
    }
    arrange(
        &mut layers,
        p.s("arrange"),
        p.f("layer_gap"),
        p.s("layer_align"),
    );

    let margin = p.f("canvas_margin");
    let (ox, oy, cw, ch) = if p.s("canvas_size") == "fixed" {
        (0.0, 0.0, p.f("canvas_width"), p.f("canvas_height"))
    } else {
        let x0 = layers.iter().map(|l| l.x).fold(f64::MAX, f64::min) - margin;
        let y0 = layers.iter().map(|l| l.y).fold(f64::MAX, f64::min) - margin;
        let x1 = layers.iter().map(|l| l.x + l.w).fold(f64::MIN, f64::max) + margin;
        let y1 = layers.iter().map(|l| l.y + l.h).fold(f64::MIN, f64::max) + margin;
        (x0, y0, (x1 - x0).ceil(), (y1 - y0).ceil())
    };
    if cw > MAX_SIDE || ch > MAX_SIDE || cw * ch > MAX_PIXELS {
        return Err(format!(
            "the canvas would be {cw:.0} x {ch:.0} px, more than this studio can hold: scale the layers down with a Place node"
        ));
    }
    let (cw, ch) = (cw as usize, ch as usize);
    let cover = p.s("overlap") == "later on top";
    let tile = |l: &Placed| {
        (
            (l.x - ox).round() as i64,
            (l.y - oy).round() as i64,
            l.w.round().max(1.0) as usize,
            l.h.round().max(1.0) as usize,
        )
    };
    let has_picture = layers.iter().any(|l| matches!(l.art, Art::Image(_)));

    let mut out: Outputs = Vec::new();
    if has_picture {
        let mut rgb = vec![255u8; cw * ch * 3];
        for l in &layers {
            let (x0, y0, tw, th) = tile(l);
            match &l.art {
                Art::Image(img) => {
                    let src = resized_rgb(img, tw, th);
                    each_pixel(cw, ch, x0, y0, tw, th, |d, s| {
                        for c in 0..3 {
                            let v = src[s * 3 + c];
                            let t = &mut rgb[d * 3 + c];
                            *t = if cover { v } else { (*t).min(v) };
                        }
                    });
                }
                art => {
                    let paint = paint_of(art, tw, th);
                    each_pixel(cw, ch, x0, y0, tw, th, |d, s| {
                        if paint[s] {
                            rgb[d * 3..d * 3 + 3].fill(0);
                        } else if cover {
                            rgb[d * 3..d * 3 + 3].fill(255);
                        }
                    });
                }
            }
        }
        out.push((
            "image",
            Value::Image(Rc::new(RgbImage { w: cw, h: ch, rgb })),
        ));
    } else {
        let mut paint = vec![false; cw * ch];
        for l in &layers {
            let (x0, y0, tw, th) = tile(l);
            let src = paint_of(&l.art, tw, th);
            each_pixel(cw, ch, x0, y0, tw, th, |d, s| {
                paint[d] = if cover { src[s] } else { paint[d] || src[s] };
            });
        }
        let rgb: Vec<u8> = paint
            .iter()
            .flat_map(|&p| [if p { 0 } else { 255 }; 3])
            .collect();
        out.push((
            "image",
            Value::Image(Rc::new(RgbImage { w: cw, h: ch, rgb })),
        ));
        out.push((
            "mask",
            Value::Mask(Rc::new(Mask {
                w: cw,
                h: ch,
                paint,
            })),
        ));
    }

    let outlines = layers.iter().all(|l| matches!(l.art, Art::Shapes(_)))
        && !layers
            .iter()
            .enumerate()
            .any(|(k, a)| layers[k + 1..].iter().any(|b| overlaps(a, b)));
    if outlines {
        let loops = layers
            .iter()
            .flat_map(|l| match &l.art {
                Art::Shapes(s) => moved(s, l.scale, l.x - ox, l.y - oy),
                _ => Vec::new(),
            })
            .collect();
        out.push((
            "shapes",
            Value::Shapes(Rc::new(Shapes {
                w: cw,
                h: ch,
                loops,
            })),
        ));
    }
    Ok(out)
}
