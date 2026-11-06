//! Image, mask, text, outline and colour node adapters.

use super::super::catalogue::split_outputs;
use super::super::types::Outputs;
use super::super::{Engine, Mask, RgbImage, Value, P};
use crate::{
    colour::{self, Pattern, Tone, ToneParams},
    otsu,
};
use std::{collections::HashMap, rc::Rc};

pub(in crate::graph) fn run_source(
    e: &Engine,
    _: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let id = p.s("image_id");
    if id.is_empty() {
        return Err("no image loaded: drop a picture on the canvas".into());
    }
    let bytes = e
        .uploads
        .get(id)
        .ok_or("the image is no longer loaded: drop it again")?;
    let rgb = crate::decode::decode_rgb(bytes, p.f("resolution") as u32)?;
    Ok(vec![("image", Value::Image(Rc::new(rgb)))])
}

pub(in crate::graph) fn run_morphology(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    use crate::morphology as m;
    let Some(Value::Mask(src)) = i.get("mask") else {
        return Err("nothing is connected to the 'mask' input: connect a mask".into());
    };
    let (w, h, a) = (src.w, src.h, &src.paint[..]);
    let shape = m::Shape::parse(p.s("kernel")).ok_or("unknown kernel shape")?;
    let k = m::kernel(shape, p.f("radius") as u32, p.f("angle"));
    let passes = p.f("iterations") as u32;
    let n = passes.max(1);
    let paint = match p.s("op") {
        "erode" => m::erode(w, h, a, &k, n),
        "dilate" => m::dilate(w, h, a, &k, n),
        "open" => m::open(w, h, a, &k, n),
        "close" => m::close(w, h, a, &k, n),
        "gradient" => m::gradient(w, h, a, &k, n),
        "white_hat" => m::white_hat(w, h, a, &k, n),
        "black_hat" => m::black_hat(w, h, a, &k, n),
        "thin" => m::thin(w, h, a, passes),
        "skeleton" => m::skeleton(w, h, a),
        "hit_or_miss" => {
            let patterns = match p.s("pattern") {
                "custom" => m::custom_patterns(p.s("custom"), p.b("rotate"))?,
                name => m::preset(name).ok_or("unknown pattern")?,
            };
            m::hit_or_miss(w, h, a, &patterns)
        }
        other => return Err(format!("unknown operation '{other}'")),
    };
    Ok(vec![("mask", Value::Mask(Rc::new(Mask { w, h, paint })))])
}

pub(in crate::graph) fn run_text(
    e: &Engine,
    _: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let (size, pad, gap) = (
        p.f("size") as f32,
        p.f("padding") as usize,
        p.f("line_spacing") as f32,
    );
    let m = match p.s("system_font") {
        "" => crate::text::render(p.s("text"), p.s("font"), size, pad, gap)?,
        name => {
            let bytes = e.fonts.get(name).ok_or_else(|| {
                format!("the font '{name}' is not loaded: in the Text node pick it again under 'System font', or choose a built-in font")
            })?;
            crate::text::render_with(p.s("text"), bytes, size, pad, gap)?
        }
    };
    Ok(vec![("mask", Value::Mask(Rc::new(m)))])
}

pub(in crate::graph) fn run_transform(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let params = crate::transform::Params {
        left: p.f("crop_left") / 100.0,
        top: p.f("crop_top") / 100.0,
        right: p.f("crop_right") / 100.0,
        bottom: p.f("crop_bottom") / 100.0,
        flip_h: p.b("flip_h"),
        flip_v: p.b("flip_v"),
        quarter_turns: p.s("turn").parse::<u32>().unwrap_or(0) / 90,
        angle: p.f("angle"),
    };
    let r = crate::transform::transform(input_image(i)?, &params)?;
    Ok(vec![("image", Value::Image(Rc::new(r)))])
}

pub(in crate::graph) fn run_smooth(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let Some(Value::Mask(src)) = i.get("mask") else {
        return Err("nothing is connected to the 'mask' input: connect a mask".into());
    };
    let (w, h, paint) = crate::smooth::smooth(
        src.w,
        src.h,
        &src.paint,
        p.f("radius") as f32,
        p.f("supersample") as usize,
    )?;
    Ok(vec![("mask", Value::Mask(Rc::new(Mask { w, h, paint })))])
}

pub(in crate::graph) fn run_threshold(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let Some(Value::Image(img)) = i.get("image") else {
        return Err("nothing is connected to the 'image' input: connect an image".into());
    };
    let gray: Vec<u8> = img
        .rgb
        .chunks_exact(3)
        .map(|c| ((299 * c[0] as u32 + 587 * c[1] as u32 + 114 * c[2] as u32) / 1000) as u8)
        .collect();
    let t = p.f("threshold");
    let cut: u32 = if t >= 256.0 {
        otsu(&gray) as u32
    } else {
        t as u32 + 1
    };
    let invert = p.b("invert");
    let paint = gray.iter().map(|&g| ((g as u32) < cut) != invert).collect();
    Ok(vec![(
        "mask",
        Value::Mask(Rc::new(Mask {
            w: img.w,
            h: img.h,
            paint,
        })),
    )])
}

pub(in crate::graph) fn run_trace(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let Some(Value::Mask(m)) = i.get("mask") else {
        return Err("nothing is connected to the 'mask' input: connect a mask".into());
    };
    let s = crate::trace::trace(
        &m.paint,
        m.w,
        m.h,
        &crate::trace::TraceParams {
            smooth_px: p.f("smooth"),
            corner_deg: p.f("corner_deg"),
            tolerance_px: p.f("tolerance"),
            min_area_px: p.f("min_area"),
        },
    );
    Ok(vec![("shapes", Value::Shapes(Rc::new(s)))])
}

pub(in crate::graph) fn run_round_corners(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let Some(Value::Shapes(s)) = i.get("shapes") else {
        return Err("nothing is connected to the 'shapes' input: connect shapes".into());
    };
    let (out, rounded, _) = crate::shape_edit::round_corners(
        s,
        &crate::shape_edit::CornerParams {
            max_angle_deg: p.f("max_angle"),
            radius_px: p.f("radius"),
            round_concave: p.b("round_concave"),
        },
    );
    Ok(vec![
        ("shapes", Value::Shapes(Rc::new(out))),
        ("rounded", Value::Shapes(Rc::new(rounded))),
    ])
}

pub(in crate::graph) fn run_remove_islands(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let Some(Value::Shapes(s)) = i.get("shapes") else {
        return Err("nothing is connected to the 'shapes' input: connect shapes".into());
    };
    let (out, removed, _) = crate::shape_edit::remove_islands(
        s,
        &crate::shape_edit::IslandParams {
            delete_specks: p.b("delete_specks"),
            fill_holes: p.b("fill_holes"),
            max_area_px: p.f("max_area"),
            keep_largest: p.f("keep_largest") as usize,
        },
    );
    Ok(vec![
        ("shapes", Value::Shapes(Rc::new(out))),
        ("removed", Value::Shapes(Rc::new(removed))),
    ])
}

pub(in crate::graph) fn run_shapes_to_mask(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let Some(Value::Shapes(s)) = i.get("shapes") else {
        return Err("nothing is connected to the 'shapes' input: connect shapes".into());
    };
    let k = p.f("resolution") / s.w.max(s.h) as f64;
    let (gw, gh) = (
        ((s.w as f64 * k).round() as usize).max(1),
        ((s.h as f64 * k).round() as usize).max(1),
    );
    Ok(vec![(
        "mask",
        Value::Mask(Rc::new(Mask {
            w: gw,
            h: gh,
            paint: crate::trace::rasterise(s, gw, gh),
        })),
    )])
}

pub(in crate::graph) fn input_image(i: &HashMap<String, Value>) -> Result<&Rc<RgbImage>, String> {
    match i.get("image") {
        Some(Value::Image(img)) => Ok(img),
        _ => Err("nothing is connected to the 'image' input: connect an image".into()),
    }
}

pub(in crate::graph) fn run_reduce(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let r = colour::reduce(
        input_image(i)?,
        p.f("colours") as usize,
        p.f("smooth") as f32,
    );
    Ok(vec![("image", Value::Image(Rc::new(r)))])
}

pub(in crate::graph) fn run_split(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let s = colour::split(
        input_image(i)?,
        p.b("invert"),
        p.f("colours") as usize,
        p.f("min_area"),
    );
    let names = split_outputs();
    let mut out: Outputs = s
        .masks
        .into_iter()
        .zip(names)
        .map(|(m, (n, _))| (n, Value::Mask(Rc::new(m))))
        .collect();
    out.push(("palette", Value::Image(Rc::new(s.palette))));
    Ok(out)
}

pub(in crate::graph) fn run_tones(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let tones = (1..=colour::MAX_RUNGS)
        .map(|k| Tone {
            kind: Pattern::parse(p.s(&format!("kind_{k}"))),
            spacing_mm: p.f(&format!("spacing_{k}")),
            cover_pct: p.f(&format!("cover_{k}")),
        })
        .collect();
    let t = colour::tone_patterns(
        input_image(i)?,
        &ToneParams {
            plate_width_mm: p.f("plate_width_mm"),
            invert: p.b("invert"),
            default_kind: Pattern::parse(p.s("kind")),
            spacing_mm: p.f("spacing_mm"),
            angle: p.f("angle"),
            tones,
        },
    )?;
    Ok(vec![
        ("mask", Value::Mask(Rc::new(t.mask))),
        ("tones", Value::Image(Rc::new(t.tones))),
    ])
}
