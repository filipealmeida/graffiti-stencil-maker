//! Typed node graph: node catalogue, validation and cached evaluation. The web studio and the CLI both run this.

use crate::colour::{self, Pattern, Tone, ToneParams};
use crate::{otsu, stencil_from_paint, Params, ZBridging};
use serde_json::{json, Map, Value as Json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

mod catalogue;
use catalogue::{catalogue, split_outputs, NodeDef, Outputs};
#[cfg(test)]
mod tests;

const CACHE_LIMIT: usize = 64;

pub struct RgbImage {
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<u8>,
}

/// `true` = paint (a hole in the stencil). Row 0 is the top, like an image.
pub struct Mask {
    pub w: usize,
    pub h: usize,
    pub paint: Vec<bool>,
}

pub struct Solid {
    pub stl: Vec<u8>,
    pub stats: Json,
}

pub struct Parts {
    pub files: Vec<(String, Vec<u8>)>,
    pub meta: Json,
}

#[derive(Clone)]
pub enum Value {
    Image(Rc<RgbImage>),
    Mask(Rc<Mask>),
    Shapes(Rc<crate::trace::Shapes>),
    Solid(Rc<Solid>),
    Parts(Rc<Parts>),
}

impl Value {
    fn kind(&self) -> &'static str {
        match self {
            Value::Image(_) => "image",
            Value::Mask(_) => "mask",
            Value::Shapes(_) => "shapes",
            Value::Solid(_) => "solid",
            Value::Parts(_) => "parts",
        }
    }

    /// The bytes a browser or file can use, with their MIME type.
    pub fn encode(&self) -> Result<(&'static str, Vec<u8>), String> {
        match self {
            Value::Image(i) => Ok(("image/png", png(i.w, i.h, image::ColorType::Rgb8, &i.rgb)?)),
            Value::Mask(m) => {
                let px: Vec<u8> = m.paint.iter().map(|&p| if p { 0 } else { 255 }).collect();
                Ok(("image/png", png(m.w, m.h, image::ColorType::L8, &px)?))
            }
            Value::Shapes(s) => Ok(("image/svg+xml", s.to_svg().into_bytes())),
            Value::Solid(s) => Ok(("model/stl", s.stl.clone())),
            Value::Parts(p) => match p.files.as_slice() {
                [(_, b)] => Ok(("model/stl", b.clone())),
                files => Ok(("application/zip", crate::zip::zip(files))),
            },
        }
    }

    pub fn info(&self) -> Json {
        match self {
            Value::Image(i) => json!({"kind": "image", "width": i.w, "height": i.h}),
            Value::Mask(m) => {
                let cov =
                    m.paint.iter().filter(|&&p| p).count() as f64 / m.paint.len().max(1) as f64;
                json!({"kind": "mask", "width": m.w, "height": m.h, "coverage": (cov * 1e4).round() / 1e4})
            }
            Value::Shapes(s) => {
                json!({"kind": "shapes", "width": s.w, "height": s.h, "loops": s.loops.len(), "segments": s.segment_count()})
            }
            Value::Solid(s) => {
                json!({"kind": "solid", "stats": s.stats, "bytes": s.stl.len(), "tiles": []})
            }
            Value::Parts(p) => {
                let files: Map<String, Json> = p
                    .files
                    .iter()
                    .map(|(n, b)| (n.clone(), json!(b.len())))
                    .collect();
                json!({"kind": "parts", "files": files, "meta": p.meta})
            }
        }
    }

    pub fn part(&self, name: &str) -> Option<Vec<u8>> {
        match self {
            Value::Parts(p) => p
                .files
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, b)| b.clone()),
            Value::Solid(s) if name == "plate" => Some(s.stl.clone()),
            _ => None,
        }
    }
}

fn png(w: usize, h: usize, color: image::ColorType, data: &[u8]) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(data, w as u32, h as u32, color.into())
        .map_err(|e| format!("cannot encode PNG: {e}"))?;
    Ok(out)
}

// ---- node catalogue -----------------------------------------------------------------------------------------------

/// Parameters after defaults and clamping.
pub struct P(Map<String, Json>);

impl P {
    fn f(&self, k: &str) -> f64 {
        self.0.get(k).and_then(Json::as_f64).unwrap_or(0.0)
    }
    fn b(&self, k: &str) -> bool {
        self.0.get(k).and_then(Json::as_bool).unwrap_or(false)
    }
    fn s(&self, k: &str) -> &str {
        self.0.get(k).and_then(Json::as_str).unwrap_or("")
    }
}

fn clean_params(def: &NodeDef, given: Option<&Json>) -> P {
    let mut out = Map::new();
    for d in &def.params {
        let name = d["name"].as_str().unwrap_or_default();
        let raw = given.and_then(|g| g.get(name));
        let v = match d["kind"].as_str().unwrap_or("") {
            "int" | "float" => {
                let x = raw
                    .and_then(Json::as_f64)
                    .unwrap_or_else(|| d["default"].as_f64().unwrap_or(0.0));
                let x = x.clamp(
                    d["min"].as_f64().unwrap_or(f64::MIN),
                    d["max"].as_f64().unwrap_or(f64::MAX),
                );
                json!(if d["kind"] == "int" { x.round() } else { x })
            }
            "bool" => json!(raw
                .and_then(Json::as_bool)
                .unwrap_or_else(|| d["default"].as_bool().unwrap_or(false))),
            "choice" => {
                let ok = raw.and_then(Json::as_str).filter(|r| {
                    d["choices"]
                        .as_array()
                        .is_some_and(|c| c.iter().any(|x| x == r))
                });
                json!(ok.unwrap_or_else(|| d["default"].as_str().unwrap_or("")))
            }
            _ => json!(raw
                .and_then(Json::as_str)
                .unwrap_or_else(|| d["default"].as_str().unwrap_or(""))),
        };
        out.insert(name.to_string(), v);
    }
    P(out)
}

fn run_source(e: &Engine, _: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
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

fn run_morphology(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    use crate::morphology as m;
    let Some(Value::Mask(src)) = i.get("mask") else {
        return Err("input 'mask' missing".into());
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

fn run_text(_: &Engine, _: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let m = crate::text::render(
        p.s("text"),
        p.s("font"),
        p.f("size") as f32,
        p.f("padding") as usize,
        p.f("line_spacing") as f32,
    )?;
    Ok(vec![("mask", Value::Mask(Rc::new(m)))])
}

fn run_transform(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
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

fn run_smooth(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let Some(Value::Mask(src)) = i.get("mask") else {
        return Err("input 'mask' missing".into());
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

fn run_threshold(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let Some(Value::Image(img)) = i.get("image") else {
        return Err("input 'image' missing".into());
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

fn stencil_params_from(p: &P) -> Params {
    Params {
        width_mm: p.f("width_mm"),
        thickness_mm: p.f("thickness_mm"),
        margin_mm: p.f("margin_mm"),
        bridge_mm: p.f("bridge_mm"),
        bridges_per_island: p.f("bridges_per_island") as u32,
        flip: p.b("flip"),
        raised_bridges: p.b("raised_bridges"),
        z_bridging: if p.s("z_bridging") == "easy" {
            ZBridging::Easy
        } else {
            ZBridging::Steps
        },
        layer_height_mm: p.f("layer_height_mm"),
        extra_top_layers: p.f("extra_top_layers") as u32,
        max_overhang_deg: p.f("max_overhang_deg"),
        min_island_mm2: p.f("min_island_mm2"),
        ..Params::default()
    }
}

fn finish_solid(o: crate::Output, p: &P) -> Result<Outputs, String> {
    if o.stats.islands_remaining != 0 || !o.stats.watertight {
        return Err("verification failed: the stencil has islands or is not watertight".into());
    }
    let mut stats: Json = serde_json::from_str(&o.stats.to_json()).map_err(|e| e.to_string())?;
    stats["margin_mm"] = json!(p.f("margin_mm"));
    stats["flip"] = json!(p.b("flip"));
    stats["thickness_mm"] = json!(p.f("thickness_mm"));
    stats["layer_height_mm"] = json!(p.f("layer_height_mm"));
    stats["plate_top_mm"] = json!(o.stats.thickness_mm);
    stats["bridge_mm"] = json!(p.f("bridge_mm"));
    stats["bridges_per_island"] = json!(p.f("bridges_per_island"));
    stats["raised_bridges"] = json!(p.b("raised_bridges"));
    stats["z_bridging"] = json!(p.s("z_bridging"));
    Ok(vec![(
        "solid",
        Value::Solid(Rc::new(Solid { stl: o.stl, stats })),
    )])
}

fn run_trace(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let Some(Value::Mask(m)) = i.get("mask") else {
        return Err("input 'mask' missing".into());
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

fn run_round_corners(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let Some(Value::Shapes(s)) = i.get("shapes") else {
        return Err("input 'shapes' missing".into());
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

fn run_remove_islands(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let Some(Value::Shapes(s)) = i.get("shapes") else {
        return Err("input 'shapes' missing".into());
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

fn run_shapes_to_mask(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let Some(Value::Shapes(s)) = i.get("shapes") else {
        return Err("input 'shapes' missing".into());
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

fn run_vector_stencil(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let Some(Value::Shapes(s)) = i.get("shapes") else {
        return Err("input 'shapes' missing".into());
    };
    let q = stencil_params_from(p);
    finish_solid(crate::vector_stencil::build(s, p.f("detail"), &q)?, p)
}

fn input_image<'a>(i: &'a HashMap<String, Value>) -> Result<&'a Rc<RgbImage>, String> {
    match i.get("image") {
        Some(Value::Image(img)) => Ok(img),
        _ => Err("input 'image' missing".into()),
    }
}

fn run_reduce(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let r = colour::reduce(
        input_image(i)?,
        p.f("colours") as usize,
        p.f("smooth") as f32,
    );
    Ok(vec![("image", Value::Image(Rc::new(r)))])
}

fn run_split(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
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

fn run_tones(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
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

fn run_stencil(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let Some(Value::Mask(m)) = i.get("mask") else {
        return Err("input 'mask' missing".into());
    };
    finish_solid(
        stencil_from_paint(&m.paint, m.w, m.h, 0, &stencil_params_from(p))?,
        p,
    )
}

fn run_complexity(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let Some(Value::Solid(s)) = i.get("solid") else {
        return Err("input 'solid' missing".into());
    };
    let lh = s.stats["layer_height_mm"]
        .as_f64()
        .filter(|&h| h > 0.0)
        .unwrap_or(0.2);
    let q = crate::complexity::Params {
        layer_height: lh,
        from: p.f("layer_from") as u32,
        to: p.f("layer_to") as u32,
        tolerance_mm: p.f("tolerance_mm"),
        max_overhang_deg: p.f("max_overhang_deg"),
        keep_material: p.b("keep_material"),
        smoothing: p.f("smoothing"),
    };
    let (stl, r) = crate::complexity::reduce(&s.stl, &q)?;
    let mut stats = s.stats.clone();
    stats["triangles"] = json!(r.triangles_after);
    stats["stl_bytes"] = json!(stl.len());
    stats["complexity"] = json!({
        "skipped": r.skipped,
        "layer_height_mm": lh,
        "layers": r.layers,
        "from": r.from,
        "to": r.to,
        "vertices_before": r.vertices_before,
        "vertices_after": r.vertices_after,
        "triangles_before": r.triangles_before,
        "triangles_after": r.triangles_after,
        "max_deviation_mm": r.max_deviation_mm,
        "tolerance_used_mm": r.tolerance_mm,
        "overhang_violation_mm2": r.overhang_violation_mm2,
    });
    Ok(vec![("solid", Value::Solid(Rc::new(Solid { stl, stats })))])
}

fn run_hardware(_: &Engine, _: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    use crate::hardware as hw;
    let seg = p.f("segments") as usize;
    let (bolt, nut) = hw::screw_parts(p.s("screw"), p.f("length"), p.f("nut_clearance"), seg)?;
    let mut files = vec![];
    for (m, n) in [
        (bolt, format!("bolt_{}x{}.stl", p.s("screw"), p.f("length"))),
        (nut, format!("nut_{}.stl", p.s("screw"))),
    ] {
        if !m.is_watertight() {
            return Err(format!("verification failed: {n} is not watertight"));
        }
        files.push((n, m.to_stl()));
    }
    if p.b("fit_test") {
        let d: f64 = p.s("connector_d").parse().unwrap_or(3.0);
        let m = hw::fit_test(
            p.s("connector") == "hex",
            d,
            &[0.0, 0.1, 0.2, 0.3, 0.4, 0.5],
            seg,
        )?;
        if !m.is_watertight() {
            return Err("verification failed: fit-test strip is not watertight".into());
        }
        files.push((
            format!(
                "fit_test_{}_d{}mm.stl",
                p.s("connector"),
                p.s("connector_d")
            ),
            m.to_stl(),
        ));
    }
    Ok(vec![(
        "parts",
        Value::Parts(Rc::new(Parts {
            files,
            meta: json!({}),
        })),
    )])
}

fn run_export(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    use crate::{hardware as hw, tiles};
    let Some(Value::Solid(s)) = i.get("solid") else {
        return Err("input 'solid' missing".into());
    };
    let tp = tiles::TileParams {
        max_x: p.f("max_x"),
        max_y: p.f("max_y"),
        shape: if p.s("connector") == "round" {
            tiles::Shape::Round
        } else {
            tiles::Shape::Hex
        },
        diameter: p.s("connector_d").parse().unwrap_or(3.0),
        depth: p.f("depth"),
        clearance: p.f("clearance"),
        segments: p.f("segments") as usize,
        thickness: s.stats["thickness_mm"].as_f64().unwrap_or(0.0),
        bridge: s.stats["plate_top_mm"].as_f64().map(|top| {
            let lh = s.stats["layer_height_mm"].as_f64().unwrap_or(0.2);
            crate::tile_bridge::BridgeParams {
                bridge_mm: s.stats["bridge_mm"].as_f64().unwrap_or(1.6),
                per_island: s.stats["bridges_per_island"].as_f64().unwrap_or(1.0) as usize,
                raised: s.stats["raised_bridges"].as_bool().unwrap_or(false),
                easy: s.stats["z_bridging"].as_str() == Some("easy"),
                base_layers: (s.stats["thickness_mm"].as_f64().unwrap_or(top) / lh).round() as u32,
                layer_height: lh,
                plate_top: top,
                flip: s.stats["flip"].as_bool().unwrap_or(false),
            }
        }),
    };
    let (pads, mount) = (p.f("pad_count") as usize, p.s("mount_screw") != "none");
    let tiled = tp.max_x > 0.0 || tp.max_y > 0.0;
    if !tiled && pads == 0 && !mount {
        let parts = Parts {
            files: vec![("stencil.stl".into(), s.stl.clone())],
            meta: json!({}),
        };
        return Ok(vec![
            ("parts", Value::Parts(Rc::new(parts))),
            ("solid", Value::Solid(s.clone())),
        ]);
    }
    let seg = p.f("segments") as usize;
    let margin = s.stats["margin_mm"].as_f64().unwrap_or(0.0);
    let flip = s.stats["flip"].as_bool().unwrap_or(false);
    let layer = s.stats["layer_height_mm"].as_f64().unwrap_or(0.2);
    let mut mesh = crate::mesh::Mesh::from_stl(&s.stl)?;
    crate::progress::step("read plate", 0.02);
    let (lo, hi) = mesh.bounds();
    let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
    let mut meta = json!({});
    let mut pad_r = vec![];
    if pads > 0 {
        let pp = hw::PadParams {
            count: pads,
            thread: p.s("pad_thread").into(),
            height: p.f("pad_height"),
            clearance: p.f("thread_clearance"),
            flip,
            segments: seg,
        };
        let (m, info) = hw::add_pads(&mesh, &pp, margin, layer)?;
        mesh = m;
        crate::progress::step("add pads", 0.05);
        meta["pads"] = info;
        let r = hw::pad_diameter(p.s("pad_thread"))? / 2.0;
        pad_r = hw::pad_centres(pads, w, h, margin)
            .into_iter()
            .map(|(x, y)| (x, y, r))
            .collect();
    }
    let mut pairs = 0;
    if mount {
        let (xs, ys) = (tiles::grid(w, tp.max_x), tiles::grid(h, tp.max_y));
        let d = hw::thread(p.s("mount_screw"))?.0 + p.f("mount_clearance");
        if tiled && p.f("mount_offset") < tp.diameter / 2.0 + d / 2.0 + 2.0 {
            return Err(format!("mounting holes would hit the seam connectors: use a hole offset of at least {:.1} mm", tp.diameter / 2.0 + d / 2.0 + 2.0));
        }
        let mp = hw::MountParams {
            screw: p.s("mount_screw").into(),
            clearance: p.f("mount_clearance"),
            threaded: p.b("mount_threaded"),
            offset: p.f("mount_offset"),
            segments: seg,
        };
        let (m, mounts, info) = hw::drill_mounts(&mesh, &mp, margin, &xs, &ys, &pad_r)?;
        mesh = m;
        crate::progress::step("drill mounting holes", 0.08);
        pairs = mounts.pairs.len();
        meta["mounting"] = info;
    }
    crate::progress::step("hardware", 0.1);
    let t = tiles::tile(&mesh, &tp)?;
    crate::progress::step("cut tiles", 0.9);
    if tiled {
        meta["tiling"] = t.info.clone();
    }
    let mut files = Vec::new();
    for x in &t.tiles {
        if !x.mesh.is_watertight() {
            return Err(format!("verification failed: {} is not watertight", x.name));
        }
        files.push((format!("{}.stl", x.name), x.mesh.to_stl()));
    }
    if let Some(r) = &t.rod {
        files.push((
            format!(
                "connector_{}_d{}mm.stl",
                p.s("connector"),
                p.s("connector_d")
            ),
            r.to_stl(),
        ));
    }
    if pairs > 0 {
        let (j, e) = hw::join_plates(
            p.s("mount_screw"),
            p.b("mount_threaded"),
            p.f("mount_clearance") / 2.0,
            p.f("mount_offset"),
            p.f("mount_extend"),
            4.0,
            seg,
        )?;
        for (m, n) in [(j, "joiner"), (e, "extender")] {
            if !m.is_watertight() {
                return Err(format!("verification failed: {n} plate is not watertight"));
            }
            files.push((format!("{n}_{}.stl", p.s("mount_screw")), m.to_stl()));
        }
    }
    let parts = Parts { files, meta };
    Ok(vec![
        ("parts", Value::Parts(Rc::new(parts))),
        ("solid", Value::Solid(s.clone())),
    ])
}

// ---- engine -------------------------------------------------------------------------------------------------------

const MAX_IMAGE_BYTES: usize = 100 << 20;
const MAX_IMAGE_PIXELS: u64 = 100_000_000;

pub fn node_types() -> Json {
    Json::Array(
        catalogue()
            .iter()
            .map(|d| {
                json!({
                    "id": d.id, "label": d.label, "stage": d.stage,
                    "inputs": d.inputs.iter().map(|(n, t, r)| json!({"name": n, "type": t, "required": r})).collect::<Vec<_>>(),
                    "outputs": d.outputs.iter().map(|(n, t)| json!({"name": n, "type": t})).collect::<Vec<_>>(),
                    "params": d.params, "doc": d.doc,
                })
            })
            .collect(),
    )
}

#[derive(Default)]
pub struct Engine {
    uploads: HashMap<String, Vec<u8>>,
    cache: HashMap<String, Rc<Outputs>>,
    order: VecDeque<String>,
    fresh: Vec<(String, String)>,
}

/// FNV-1a over length-prefixed parts. Unlike `DefaultHasher` it gives the same id on 32-bit wasm and 64-bit native,
/// so a project saved in the browser finds its pictures again in the CLI.
fn hash_hex(parts: &[&[u8]]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for &b in bytes {
            h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for p in parts {
        eat(&(p.len() as u64).to_le_bytes());
        eat(p);
    }
    format!("{h:016x}")
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a picture and returns `{id, width, height}`; the id is a hash of the bytes.
    pub fn add_image(&mut self, bytes: Vec<u8>) -> Result<Json, String> {
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err("the picture file is larger than 100 MB".into());
        }
        let (width, height) = crate::svg::size(&bytes)?;
        if (width as u64) * (height as u64) > MAX_IMAGE_PIXELS {
            return Err("the picture is larger than 100 megapixels".into());
        }
        let id = hash_hex(&[&bytes]);
        let out = json!({"id": id, "width": width, "height": height});
        self.uploads.insert(id, bytes);
        Ok(out)
    }

    pub fn has_image(&self, id: &str) -> bool {
        self.uploads.contains_key(id)
    }

    pub fn value(&self, key: &str, out: &str) -> Option<&Value> {
        self.cache
            .get(key)?
            .iter()
            .find(|(n, _)| *n == out)
            .map(|(_, v)| v)
    }

    /// Outputs computed by the last run, as `(key, output)`.
    pub fn take_fresh(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.fresh)
    }

    /// Evaluates the graph, or only the ancestors of `targets`. `now` returns epoch milliseconds.
    pub fn run(&mut self, graph: &Json, targets: Option<&[String]>, now: &dyn Fn() -> f64) -> Json {
        let defs: HashMap<&str, NodeDef> = catalogue().into_iter().map(|d| (d.id, d)).collect();
        // `fresh` reports only what this call computed; otherwise every run would re-export all earlier results
        self.fresh.clear();
        let mut log: Vec<Json> = Vec::new();
        let t0 = now();
        let mut say = |level: &str, node: &str, ty: &str, msg: String, ms: Option<f64>| {
            log.push(json!({"at": now(), "t": ((now() - t0) / 10.0).round() / 100.0, "level": level, "node": node, "type": ty, "msg": msg, "ms": ms}));
        };

        let order = match plan(graph, &defs, targets) {
            Ok(o) => o,
            Err(e) => {
                return json!({"status": "error", "error": e, "nodes": {}, "log": log, "fresh": []})
            }
        };
        let mut nodes_json = Map::new();
        for n in graph["nodes"].as_array().into_iter().flatten() {
            nodes_json.insert(
                n["id"].as_str().unwrap_or("").to_string(),
                json!({"state": "pending"}),
            );
        }
        let feeds: HashMap<(String, String), (String, String)> = graph["edges"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| {
                let s = |v: &Json, i: usize| v.get(i).and_then(Json::as_str).map(str::to_string);
                Some((
                    (s(&e["to"], 0)?, s(&e["to"], 1)?),
                    (s(&e["from"], 0)?, s(&e["from"], 1)?),
                ))
            })
            .collect();
        let by_id: HashMap<&str, &Json> = graph["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| Some((n["id"].as_str()?, n)))
            .collect();

        let mut results: HashMap<String, Option<(String, Rc<Outputs>)>> = HashMap::new();
        for id in order {
            let node = by_id[id.as_str()];
            let ty = node["type"].as_str().unwrap_or("");
            let def = &defs[ty];
            let params = clean_params(def, node.get("params"));
            let fail = |nodes: &mut Map<String, Json>,
                        results: &mut HashMap<String, Option<(String, Rc<Outputs>)>>,
                        msg: String| {
                nodes.insert(id.clone(), json!({"state": "error", "error": msg}));
                results.insert(id.clone(), None);
            };

            let mut inputs = HashMap::new();
            let mut keys = Vec::new();
            let mut problem = None;
            for (port, _, required) in &def.inputs {
                match feeds.get(&(id.clone(), port.to_string())) {
                    None if *required => {
                        problem = Some(format!("input '{port}' is not connected"));
                        break;
                    }
                    None => {}
                    Some((src, out)) => match results.get(src) {
                        Some(Some((key, vals))) => match vals.iter().find(|(n, _)| n == out) {
                            Some((_, v)) => {
                                inputs.insert(port.to_string(), v.clone());
                                keys.push(format!("{key}:{out}"));
                            }
                            None => {
                                problem = Some(format!("'{out}' is not available from {src}"));
                                break;
                            }
                        },
                        _ => {
                            problem = Some("waiting on a failed node upstream".to_string());
                            break;
                        }
                    },
                }
            }
            if let Some(m) = problem {
                say("error", &id, ty, m.clone(), None);
                fail(&mut nodes_json, &mut results, m);
                continue;
            }

            let pj = Json::Object(params.0.clone()).to_string();
            let key = hash_hex(&[ty.as_bytes(), pj.as_bytes(), keys.join("|").as_bytes()]);
            let cached = self.cache.get(&key).cloned();
            let t1 = now();
            let vals = match cached {
                Some(v) => Ok((v, true)),
                None => (def.run)(self, &inputs, &params).map(|v| (Rc::new(v), false)),
            };
            match vals {
                Err(msg) => {
                    say("error", &id, ty, msg.clone(), None);
                    fail(&mut nodes_json, &mut results, msg);
                }
                Ok((vals, was_cached)) => {
                    let ms = (now() - t1).round();
                    if !was_cached {
                        for (n, _) in vals.iter() {
                            self.fresh.push((key.clone(), n.to_string()));
                        }
                        self.cache.insert(key.clone(), vals.clone());
                        self.order.push_back(key.clone());
                        while self.order.len() > CACHE_LIMIT {
                            if let Some(old) = self.order.pop_front() {
                                self.cache.remove(&old);
                            }
                        }
                    }
                    say(
                        "info",
                        &id,
                        ty,
                        if was_cached {
                            "cached".into()
                        } else {
                            "done".into()
                        },
                        if was_cached { None } else { Some(ms) },
                    );
                    let outputs: Map<String, Json> = def
                        .outputs
                        .iter()
                        .map(|(n, t)| {
                            (
                                n.to_string(),
                                json!({"type": t, "available": vals.iter().any(|(k, _)| k == n)}),
                            )
                        })
                        .collect();
                    nodes_json.insert(id.clone(), json!({"state": "done", "key": key, "cached": was_cached, "ms": ms, "outputs": outputs}));
                    results.insert(id.clone(), Some((key, vals)));
                }
            }
        }
        let fresh: Vec<Json> = self
            .fresh
            .iter()
            .filter_map(|(k, o)| {
                self.value(k, o)
                    .map(|v| json!({"key": k, "out": o, "kind": v.kind()}))
            })
            .collect();
        json!({"status": "done", "nodes": nodes_json, "log": log, "fresh": fresh})
    }
}

/// Validates the graph and returns the nodes to evaluate, parents first.
fn plan(
    graph: &Json,
    defs: &HashMap<&str, NodeDef>,
    targets: Option<&[String]>,
) -> Result<Vec<String>, String> {
    let nodes = graph["nodes"].as_array().ok_or("graph has no nodes list")?;
    let mut types: HashMap<&str, &str> = HashMap::new();
    for n in nodes {
        let id = n["id"].as_str().ok_or("node without id")?;
        let ty = n["type"].as_str().unwrap_or("");
        if !defs.contains_key(ty) {
            return Err(format!("unknown node type '{ty}'"));
        }
        if types.insert(id, ty).is_some() {
            return Err(format!("duplicate node id '{id}'"));
        }
    }
    let mut parents: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in graph["edges"].as_array().into_iter().flatten() {
        let (a, ao, b, bi) = (
            e["from"][0].as_str().unwrap_or(""),
            e["from"][1].as_str().unwrap_or(""),
            e["to"][0].as_str().unwrap_or(""),
            e["to"][1].as_str().unwrap_or(""),
        );
        let out = types
            .get(a)
            .and_then(|t| defs[t].outputs.iter().find(|(n, _)| *n == ao));
        let inp = types
            .get(b)
            .and_then(|t| defs[t].inputs.iter().find(|(n, _, _)| *n == bi));
        match (out, inp) {
            (Some((_, ot)), Some((_, it, _)))
                if it == ot || (*it == "any" && (*ot == "image" || *ot == "mask")) => {}
            _ => return Err(format!("invalid connection {a}.{ao} -> {b}.{bi}")),
        }
        parents.entry(b).or_default().push(a);
    }
    let mut needed: HashSet<&str> = HashSet::new();
    match targets {
        Some(t) if !t.is_empty() => {
            let mut todo: Vec<&str> = t.iter().map(String::as_str).collect();
            while let Some(i) = todo.pop() {
                if types.contains_key(i) && needed.insert(i) {
                    todo.extend(parents.get(i).into_iter().flatten().copied());
                }
            }
        }
        _ => needed.extend(types.keys().copied()),
    }
    let mut ids: Vec<&str> = needed.iter().copied().collect();
    ids.sort_unstable();
    let (mut order, mut done, mut open): (Vec<String>, HashSet<&str>, HashSet<&str>) =
        (vec![], HashSet::new(), HashSet::new());
    fn visit<'a>(
        i: &'a str,
        parents: &HashMap<&'a str, Vec<&'a str>>,
        done: &mut HashSet<&'a str>,
        open: &mut HashSet<&'a str>,
        order: &mut Vec<String>,
    ) -> Result<(), String> {
        if done.contains(i) {
            return Ok(());
        }
        if !open.insert(i) {
            return Err("the graph has a cycle".into());
        }
        for p in parents.get(i).into_iter().flatten() {
            visit(p, parents, done, open, order)?;
        }
        open.remove(i);
        done.insert(i);
        order.push(i.to_string());
        Ok(())
    }
    for i in ids {
        visit(i, &parents, &mut done, &mut open, &mut order)?;
    }
    Ok(order)
}
