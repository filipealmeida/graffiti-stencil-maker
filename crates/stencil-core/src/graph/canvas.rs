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

fn art_of(v: &Value) -> Option<(Art, f64, f64, f64, f64)> {
    Some(match v {
        Value::Image(i) => (Art::Image(i.clone()), 0.0, 0.0, 1.0, 1.0),
        Value::Mask(m) => (Art::Mask(m.clone()), 0.0, 0.0, 1.0, 1.0),
        Value::Shapes(s) => (Art::Shapes(s.clone()), 0.0, 0.0, 1.0, 1.0),
        Value::Layer(l) => (l.art.clone_art(), l.x, l.y, l.sx, l.sy),
        _ => return None,
    })
}

pub(super) fn run_place(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let (art, x, y, sx, sy) = i.get("item").and_then(art_of).ok_or(
        "nothing is connected to the 'content' input: connect a picture, a paint mask or outlines",
    )?;
    let (aw, ah) = art.size();
    // an exact size wins over the scale and over earlier Place nodes in a chain
    let (width, height) = (p.f("width_px"), p.f("height_px"));
    // an exact size on one side alone keeps the proportions
    let by_scale = |k: f64, pct: f64| k * pct / 100.0;
    let (sx, sy) = match (width > 0.0, height > 0.0) {
        (true, true) => (width / aw.max(1.0), height / ah.max(1.0)),
        (true, false) => (width / aw.max(1.0), width / aw.max(1.0) * sy / sx.max(1e-9)),
        (false, true) => (
            height / ah.max(1.0) * sx / sy.max(1e-9),
            height / ah.max(1.0),
        ),
        (false, false) => (
            by_scale(sx, p.f("scale_w_pct")),
            by_scale(sy, p.f("scale_h_pct")),
        ),
    };
    Ok(vec![(
        "layer",
        Value::Layer(Rc::new(Layer {
            art,
            x: x + p.f("move_x"),
            y: y + p.f("move_y"),
            sx,
            sy,
        })),
    )])
}

/// A layer with its slot on the canvas, in canvas pixels before the origin is chosen.
struct Placed {
    label: String,
    art: Art,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    sx: f64,
    sy: f64,
    /// the hand move already included in x and y, reported so the studio can tell what the result was made with
    moved: [f64; 2],
}

impl Art {
    fn clone_art(&self) -> Art {
        match self {
            Art::Image(i) => Art::Image(i.clone()),
            Art::Mask(m) => Art::Mask(m.clone()),
            Art::Shapes(s) => Art::Shapes(s.clone()),
        }
    }
}

impl Placed {
    fn new(label: &str, art: Art, x: f64, y: f64, sx: f64, sy: f64) -> Self {
        let (w, h) = art.size();
        Placed {
            label: label.into(),
            art,
            x,
            y,
            w: (w * sx).max(1.0),
            h: (h * sy).max(1.0),
            sx,
            sy,
            moved: [0.0, 0.0],
        }
    }
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

/// Which source a layer draws from, so copies of one picture are resized once.
fn art_id(a: &Art) -> usize {
    match a {
        Art::Image(i) => Rc::as_ptr(i) as *const u8 as usize,
        Art::Mask(m) => Rc::as_ptr(m) as *const u8 as usize,
        Art::Shapes(s) => Rc::as_ptr(s) as *const u8 as usize,
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

fn moved(s: &Shapes, kx: f64, ky: f64, dx: f64, dy: f64) -> Vec<Loop> {
    let m = |p: [f64; 2]| [p[0] * kx + dx, p[1] * ky + dy];
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
        .filter_map(|n| Some((*n, art_of(i.get(*n)?)?)))
        .map(|(n, (art, x, y, sx, sy))| Placed::new(n, art, x, y, sx, sy))
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
    // hand moves from the preview, in canvas pixels, on top of the arrangement
    if let Ok(Json::Object(moves)) = serde_json::from_str::<Json>(p.s("layer_moves")) {
        for l in layers.iter_mut() {
            let by = moves
                .get(&l.label)
                .and_then(|v| serde_json::from_value::<[f64; 2]>(v.clone()).ok());
            if let Some([dx, dy]) = by {
                l.x += dx;
                l.y += dy;
                l.moved = [dx, dy];
            }
        }
    }
    let fixed =
        (p.s("canvas_size") == "fixed").then(|| (p.f("canvas_width"), p.f("canvas_height")));
    compose(
        layers,
        p.f("canvas_margin"),
        fixed,
        p.s("overlap") == "later on top",
    )
}

const MAX_COPIES: usize = 1000;

pub(super) fn run_repeat(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let (art, x, y, sx, sy) = i.get("item").and_then(art_of).ok_or(
        "nothing is connected to the 'content' input: connect a picture, a paint mask or outlines",
    )?;
    let (cols, rows) = (p.f("columns") as usize, p.f("rows") as usize);
    if cols * rows > MAX_COPIES {
        return Err(format!(
            "{cols} x {rows} is {} copies; the most is {MAX_COPIES}",
            cols * rows
        ));
    }
    let one = Placed::new("", art, 0.0, 0.0, sx, sy);
    let (gx, gy) = (p.f("gap_x"), p.f("gap_y"));
    let stagger = p.f("stagger_pct") / 100.0;
    let (stepx, stepy) = (one.w + gx, one.h + gy);
    let layers = (0..rows * cols)
        .map(|k| {
            let (r, c) = (k / cols, k % cols);
            let shift = if r % 2 == 1 { stagger * stepx } else { 0.0 };
            let mut l = Placed::new(
                &format!("copy {}", k + 1),
                one.art.clone_art(),
                x + c as f64 * stepx + shift,
                y + r as f64 * stepy,
                sx,
                sy,
            );
            l.label = format!("row {} column {}", r + 1, c + 1);
            l
        })
        .collect();
    compose(
        layers,
        p.f("repeat_margin"),
        None,
        p.s("overlap") == "later on top",
    )
}

/// Draws the layers on a canvas and offers what they can make.
fn compose(
    layers: Vec<Placed>,
    margin: f64,
    fixed: Option<(f64, f64)>,
    cover: bool,
) -> Result<Outputs, String> {
    let (ox, oy, cw, ch) = if let Some((w, h)) = fixed {
        (0.0, 0.0, w, h)
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
    let tile = |l: &Placed| {
        (
            (l.x - ox).round() as i64,
            (l.y - oy).round() as i64,
            l.w.round().max(1.0) as usize,
            l.h.round().max(1.0) as usize,
        )
    };
    let has_picture = layers.iter().any(|l| matches!(l.art, Art::Image(_)));
    let mut rgb_cache: HashMap<(usize, usize, usize), Rc<Vec<u8>>> = HashMap::new();
    let mut paint_cache: HashMap<(usize, usize, usize), Rc<Vec<bool>>> = HashMap::new();

    let mut out: Outputs = Vec::new();
    if has_picture {
        let mut rgb = vec![255u8; cw * ch * 3];
        for l in &layers {
            let (x0, y0, tw, th) = tile(l);
            let key = (art_id(&l.art), tw, th);
            match &l.art {
                Art::Image(img) => {
                    let src = rgb_cache
                        .entry(key)
                        .or_insert_with(|| Rc::new(resized_rgb(img, tw, th)))
                        .clone();
                    each_pixel(cw, ch, x0, y0, tw, th, |d, s| {
                        for c in 0..3 {
                            let v = src[s * 3 + c];
                            let t = &mut rgb[d * 3 + c];
                            *t = if cover { v } else { (*t).min(v) };
                        }
                    });
                }
                art => {
                    let paint = paint_cache
                        .entry(key)
                        .or_insert_with(|| Rc::new(paint_of(art, tw, th)))
                        .clone();
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
            let key = (art_id(&l.art), tw, th);
            let src = paint_cache
                .entry(key)
                .or_insert_with(|| Rc::new(paint_of(&l.art, tw, th)))
                .clone();
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
                Art::Shapes(s) => moved(s, l.sx, l.sy, l.x - ox, l.y - oy),
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
    let kind = |a: &Art| match a {
        Art::Image(_) => "image",
        Art::Mask(_) => "mask",
        Art::Shapes(_) => "shapes",
    };
    let list: Vec<Json> = layers
        .iter()
        .map(|l| json!({"input": l.label, "kind": kind(&l.art), "x": (l.x - ox).round(), "y": (l.y - oy).round(), "w": l.w.round(), "h": l.h.round(), "move_x": l.moved[0], "move_y": l.moved[1], "scale_x": l.sx, "scale_y": l.sy}))
        .collect();
    out.push((
        "layout",
        Value::Layout(Rc::new(json!({"width": cw, "height": ch, "layers": list}))),
    ));
    Ok(out)
}
