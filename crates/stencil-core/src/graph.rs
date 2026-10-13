//! Typed node graph: node catalogue, validation and cached evaluation. The web studio and the CLI both run this.

use crate::colour::{self, Pattern, Tone, ToneParams};
use crate::{otsu, stencil_from_paint, Params, ZBridging};
use serde_json::{json, Map, Value as Json};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

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

type Outputs = Vec<(&'static str, Value)>;
type RunFn = fn(&Engine, &HashMap<String, Value>, &P) -> Result<Outputs, String>;

struct NodeDef {
    id: &'static str,
    label: &'static str,
    stage: &'static str,
    inputs: Vec<(&'static str, &'static str, bool)>,
    outputs: Vec<(&'static str, &'static str)>,
    params: Vec<Json>,
    doc: &'static str,
    run: RunFn,
}

fn num(
    name: &str,
    label: &str,
    default: f64,
    lo: f64,
    hi: f64,
    step: f64,
    group: Option<&str>,
    int: bool,
) -> Json {
    json!({"name": name, "label": label, "kind": if int {"int"} else {"float"}, "default": default, "min": lo, "max": hi, "step": step, "group": group})
}
fn flag(name: &str, label: &str, default: bool, group: Option<&str>) -> Json {
    json!({"name": name, "label": label, "kind": "bool", "default": default, "group": group})
}
fn choice(name: &str, label: &str, default: &str, choices: &[&str], group: Option<&str>) -> Json {
    json!({"name": name, "label": label, "kind": "choice", "default": default, "choices": choices, "group": group})
}

const THREAD_NAMES: [&str; 6] = ["M3", "M4", "M5", "M6", "M8", "M10"];

fn catalogue() -> Vec<NodeDef> {
    vec![
        NodeDef {
            id: "source",
            label: "Image",
            stage: "trace",
            inputs: vec![],
            outputs: vec![("image", "image")],
            params: vec![
                json!({"name": "image_id", "label": "Image", "kind": "text", "default": "", "group": null}),
                num("resolution", "Working resolution (px, longest side)", 500.0, 20.0, 2000.0, 10.0, None, true),
            ],
            doc: "The input picture, resized to the working resolution that every mask downstream is traced at.",
            run: run_source,
        },
        NodeDef {
            id: "threshold",
            label: "Threshold",
            stage: "trace",
            inputs: vec![("image", "image", true)],
            outputs: vec![("mask", "mask")],
            params: vec![
                num("threshold", "Threshold (256 = automatic)", 256.0, 0.0, 256.0, 1.0, None, true),
                flag("invert", "Invert", false, None),
            ],
            doc: "Dark areas become paint (holes); invert to make light areas the holes.",
            run: run_threshold,
        },
        NodeDef {
            id: "reduce",
            label: "Reduce colours",
            stage: "trace",
            inputs: vec![("image", "image", true)],
            outputs: vec![("image", "image")],
            params: vec![
                num("colours", "Colours", 3.0, 2.0, 10.0, 1.0, None, true),
                num("smooth", "Pre-blur (px)", 0.0, 0.0, 6.0, 0.5, None, false),
            ],
            doc: "Quantizes the picture to N colours (k-means in Lab space) so every region has exactly one flat colour.",
            run: run_reduce,
        },
        NodeDef {
            id: "split_colours",
            label: "Split colours",
            stage: "trace",
            inputs: vec![("image", "image", true)],
            outputs: split_outputs(),
            params: vec![
                flag("invert", "Invert order (lightest first)", false, None),
                num("colours", "Colours if the input has more than this many (2-10)", 10.0, 2.0, 10.0, 1.0, None, true),
                num("min_area", "Drop specks smaller than (% of image, 0 = keep)", 0.0, 0.0, 5.0, 0.05, None, false),
            ],
            doc: "Splits an image into one mask per flat colour, darkest first (colour 1), so each colour can feed its own Stencil. An image with more colours than the setting is reduced first. Every mask is true where that colour is (paint). Outputs beyond the number of colours are hidden.",
            run: run_split,
        },
        NodeDef {
            id: "tone_patterns",
            label: "Tone patterns",
            stage: "trace",
            inputs: vec![("image", "image", true)],
            outputs: vec![("mask", "mask"), ("tones", "image")],
            params: tone_params(),
            doc: "Ranks the colours of a reduced image by lightness. The darkest becomes solid paint, the lightest stays empty, and the tones between get patterns that thin out step by step. Dots leave islands that the stencil bridges. One tone per colour of the input (up to 10).",
            run: run_tones,
        },
        NodeDef {
            id: "trace",
            label: "Trace",
            stage: "trace",
            inputs: vec![("mask", "mask", true)],
            outputs: vec![("shapes", "shapes")],
            params: vec![
                num("smooth", "Smoothing (px)", 1.5, 0.0, 8.0, 0.25, None, false),
                num("corner_deg", "Sharp corner angle (deg, 180 = round everything)", 60.0, 20.0, 180.0, 5.0, None, false),
                num("tolerance", "Simplify tolerance (px)", 0.4, 0.05, 3.0, 0.05, None, false),
                num("min_area", "Drop specks smaller than (px²)", 4.0, 0.0, 1000.0, 1.0, None, false),
            ],
            doc: "Turns a mask into vector shapes: closed Bézier outlines with holes, smooth curves and sharp corners kept. Feed it to a Vector stencil, so the outlines need no pixel smoothing.",
            run: run_trace,
        },
        NodeDef {
            id: "shapes_to_mask",
            label: "Shapes to mask",
            stage: "trace",
            inputs: vec![("shapes", "shapes", true)],
            outputs: vec![("mask", "mask")],
            params: vec![num("resolution", "Resolution (px, longest side)", 1000.0, 20.0, 4000.0, 10.0, None, true)],
            doc: "Rasterises vector shapes back to a mask at any resolution, so curves can feed the raster nodes.",
            run: run_shapes_to_mask,
        },
        NodeDef {
            id: "stencil",
            label: "Stencil",
            stage: "stencil",
            inputs: vec![("mask", "mask", true)],
            outputs: vec![("solid", "solid")],
            params: stencil_params(false),
            doc: "Turns a mask into a watertight, island-free 3D plate with bridges. Raised bridges keep the first layer true to the artwork: islands and all, with the paint filling underneath.",
            run: run_stencil,
        },
        NodeDef {
            id: "vector_stencil",
            label: "Vector stencil",
            stage: "stencil",
            inputs: vec![("shapes", "shapes", true)],
            outputs: vec![("solid", "solid")],
            params: stencil_params(true),
            doc: "Builds the plate directly from vector outlines with polygon booleans, bridges enclosed material islands, and extrudes the result. Mesh detail controls curve flattening accuracy. Raised bridges use the layered raster bridge engine to preserve their stepped Z profile.",
            run: run_vector_stencil,
        },
        NodeDef {
            id: "export",
            label: "Tiles & mounting",
            stage: "post",
            inputs: vec![("solid", "solid", true)],
            outputs: vec![("parts", "parts"), ("solid", "solid")],
            params: vec![
                num("max_x", "Largest tile width (mm, 0 = no tiling)", 0.0, 0.0, 2000.0, 5.0, Some("Tiles"), false),
                num("max_y", "Largest tile height (mm, 0 = no tiling)", 0.0, 0.0, 2000.0, 5.0, Some("Tiles"), false),
                choice("connector", "Connector shape", "hex", &["hex", "round"], Some("Tiles")),
                choice("connector_d", "Connector diameter (mm)", "3", &["2.5", "3", "4", "5", "6"], Some("Tiles")),
                num("clearance", "Connector hole clearance (mm)", 0.2, 0.0, 0.6, 0.05, Some("Tiles"), false),
                num("segments", "Round segments (connectors, holes, threads)", 48.0, 24.0, 128.0, 4.0, Some("Tiles"), true),
                num("pad_count", "Raised pads (0-4)", 0.0, 0.0, 4.0, 1.0, Some("Pads"), true),
                choice("pad_thread", "Pad thread", "M8", &THREAD_NAMES, Some("Pads")),
                num("pad_height", "Pad height (mm)", 6.0, 1.0, 30.0, 0.5, Some("Pads"), false),
                num("thread_clearance", "Thread clearance (mm, radial)", 0.2, 0.0, 0.6, 0.05, Some("Pads"), false),
                choice("mount_screw", "Mounting screw", "none", &["none", "M3", "M4", "M5", "M6", "M8", "M10"], Some("Mounting holes")),
                num("mount_clearance", "Hole clearance (mm, diametral)", 0.3, 0.0, 1.0, 0.05, Some("Mounting holes"), false),
                flag("mount_threaded", "Cut a real thread", true, Some("Mounting holes")),
                num("mount_offset", "Distance from a seam to the hole (mm)", 10.0, 4.0, 60.0, 1.0, Some("Mounting holes"), false),
                num("mount_extend", "Extender plate: outer row distance (mm)", 20.0, 8.0, 100.0, 1.0, Some("Mounting holes"), false),
            ],
            doc: "Splits the plate into printable tiles on a seam grid, with glued connector rods in blind seam holes. Adds raised threaded pads and threaded or plain mounting holes in the frame, and joiner and extender plates that bridge the seams. Download everything as a ZIP.",
            run: run_export,
        },
        NodeDef {
            id: "hardware",
            label: "Screws, nuts & fit test",
            stage: "post",
            inputs: vec![],
            outputs: vec![("parts", "parts")],
            params: vec![
                choice("screw", "Screw size", "M4", &THREAD_NAMES, Some("Screw and nut")),
                num("length", "Screw length (mm)", 20.0, 4.0, 120.0, 1.0, Some("Screw and nut"), false),
                num("nut_clearance", "Nut thread clearance (mm, radial)", 0.2, 0.0, 0.6, 0.05, Some("Screw and nut"), false),
                flag("fit_test", "Connector fit-test strip", true, Some("Fit test")),
                choice("connector", "Connector shape", "hex", &["hex", "round"], Some("Fit test")),
                choice("connector_d", "Connector diameter (mm)", "3", &["2.5", "3", "4", "5", "6"], Some("Fit test")),
                num("segments", "Round segments (threads, round holes)", 48.0, 24.0, 128.0, 4.0, None, true),
            ],
            doc: "Printable hex-head bolt and hex nut with real threads (M3 to M10), plus a calibration strip of connector holes with growing clearance: try the rod in each and keep the snuggest.",
            run: run_hardware,
        },
    ]
}

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
    let rgb = crate::decode_rgb(bytes, p.f("resolution") as u32)?;
    Ok(vec![("image", Value::Image(Rc::new(rgb)))])
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

fn stencil_params(vector: bool) -> Vec<Json> {
    let mut v = vec![
        num(
            "width_mm",
            "Plate width (mm)",
            100.0,
            20.0,
            2000.0,
            1.0,
            Some("Plate"),
            false,
        ),
        num(
            "thickness_mm",
            "Thickness (mm)",
            2.0,
            0.2,
            100.0,
            0.2,
            Some("Plate"),
            false,
        ),
        num(
            "margin_mm",
            "Margin (mm)",
            8.0,
            1.0,
            500.0,
            1.0,
            Some("Plate"),
            false,
        ),
        flag("flip", "Print upside down", false, Some("Plate")),
        num(
            "bridge_mm",
            "Bridge width (mm)",
            1.6,
            0.6,
            50.0,
            0.1,
            Some("Bridges"),
            false,
        ),
        flag("raised_bridges", "Raised bridges", false, Some("Bridges")),
        choice(
            "z_bridging",
            "Z bridging",
            "steps",
            &["steps", "easy"],
            Some("Bridges"),
        ),
        num(
            "layer_height_mm",
            "Layer height (mm)",
            0.2,
            0.05,
            5.0,
            0.05,
            Some("Bridges"),
            false,
        ),
        num(
            "extra_top_layers",
            "Extra top layers",
            1.0,
            0.0,
            50.0,
            1.0,
            Some("Bridges"),
            true,
        ),
        num(
            "max_overhang_deg",
            "Overhang check angle (deg, 0 = 45)",
            0.0,
            0.0,
            89.0,
            1.0,
            Some("Bridges"),
            false,
        ),
        num(
            "min_island_mm2",
            "Delete islands under (mm²)",
            0.0,
            0.0,
            100000.0,
            1.0,
            Some("Bridges"),
            false,
        ),
    ];
    if vector {
        v.insert(
            0,
            num(
                "detail",
                "Mesh detail (samples per mm)",
                3.0,
                0.5,
                10.0,
                0.5,
                Some("Plate"),
                false,
            ),
        );
    }
    v
}

fn stencil_params_from(p: &P) -> Params {
    Params {
        width_mm: p.f("width_mm"),
        thickness_mm: p.f("thickness_mm"),
        margin_mm: p.f("margin_mm"),
        bridge_mm: p.f("bridge_mm"),
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
    if !q.raised_bridges {
        return finish_solid(crate::vector_stencil::build(s, p.f("detail"), &q)?, p);
    }
    let gw = (q.width_mm * p.f("detail")).round() as usize;
    let gh = ((gw as f64 * s.h as f64 / s.w as f64).round() as usize).max(1);
    if gw * gh > 60_000_000 {
        return Err(
            "mesh detail too high for this plate size: lower the detail or the width".into(),
        );
    }
    let paint = crate::trace::rasterise(s, gw.max(1), gh);
    finish_solid(stencil_from_paint(&paint, gw.max(1), gh, 0, &q)?, p)
}

fn split_outputs() -> Vec<(&'static str, &'static str)> {
    const NAMES: [&str; 10] = [
        "colour_1",
        "colour_2",
        "colour_3",
        "colour_4",
        "colour_5",
        "colour_6",
        "colour_7",
        "colour_8",
        "colour_9",
        "colour_10",
    ];
    NAMES
        .iter()
        .map(|n| (*n, "mask"))
        .chain([("palette", "image")])
        .collect()
}

fn tone_params() -> Vec<Json> {
    let kinds = [
        "auto",
        "lines",
        "dots",
        "crosshatch",
        "checker",
        "solid",
        "empty",
    ];
    let mut v = vec![
        num(
            "plate_width_mm",
            "Plate width (mm, match the Stencil node)",
            100.0,
            20.0,
            2000.0,
            1.0,
            None,
            true,
        ),
        flag(
            "invert",
            "Invert (lightest colour solid, darkest empty)",
            false,
            None,
        ),
        choice("kind", "Default pattern type", "auto", &kinds[..5], None),
        num(
            "spacing_mm",
            "Default pattern spacing (mm, 0 = solid)",
            2.5,
            0.0,
            20.0,
            0.1,
            None,
            false,
        ),
        num(
            "angle",
            "Default pattern angle (deg)",
            45.0,
            0.0,
            90.0,
            5.0,
            None,
            true,
        ),
    ];
    for k in 1..=colour::MAX_RUNGS {
        let g = format!("Tone {k}");
        v.push(choice(
            &format!("kind_{k}"),
            "Type",
            "auto",
            &kinds,
            Some(&g),
        ));
        v.push(num(
            &format!("spacing_{k}"),
            "Spacing (mm, 0 = default)",
            0.0,
            0.0,
            20.0,
            0.1,
            Some(&g),
            false,
        ));
        v.push(num(
            &format!("cover_{k}"),
            "Coverage (%, 0 = automatic)",
            0.0,
            0.0,
            95.0,
            1.0,
            Some(&g),
            false,
        ));
    }
    v
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
        clearance: p.f("clearance"),
        segments: p.f("segments") as usize,
        thickness: s.stats["thickness_mm"].as_f64().unwrap_or(0.0),
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
        pairs = mounts.pairs.len();
        meta["mounting"] = info;
    }
    let t = tiles::tile(&mesh, &tp)?;
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

fn hash_hex(parts: &[&[u8]]) -> String {
    let mut h = DefaultHasher::new();
    for p in parts {
        p.hash(&mut h);
    }
    format!("{:016x}", h.finish())
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a picture and returns `{id, width, height}`; the id is a hash of the bytes.
    pub fn add_image(&mut self, bytes: Vec<u8>) -> Result<Json, String> {
        let (width, height) = crate::svg::size(&bytes)?;
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

    /// Outputs computed since the last call, as `(key, output)`.
    pub fn take_fresh(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.fresh)
    }

    /// Evaluates the graph, or only the ancestors of `targets`. `now` returns epoch milliseconds.
    pub fn run(&mut self, graph: &Json, targets: Option<&[String]>, now: &dyn Fn() -> f64) -> Json {
        let defs: HashMap<&str, NodeDef> = catalogue().into_iter().map(|d| (d.id, d)).collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_png() -> Vec<u8> {
        let img = image::GrayImage::from_fn(120, 120, |x, y| {
            let d = (((x as f64 - 60.0).powi(2) + (y as f64 - 60.0).powi(2)).sqrt()) as u32;
            image::Luma([if (30..40).contains(&d) { 0 } else { 255 }])
        });
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn graph(image_id: &str, thickness: f64) -> Json {
        json!({
            "nodes": [
                {"id": "s", "type": "source", "params": {"image_id": image_id, "resolution": 120}},
                {"id": "t", "type": "threshold", "params": {}},
                {"id": "st", "type": "stencil", "params": {"thickness_mm": thickness}},
                {"id": "ex", "type": "export", "params": {}},
            ],
            "edges": [
                {"from": ["s", "image"], "to": ["t", "image"]},
                {"from": ["t", "mask"], "to": ["st", "mask"]},
                {"from": ["st", "solid"], "to": ["ex", "solid"]},
            ]
        })
    }

    #[test]
    fn svg_pictures_run_through_the_graph() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40" viewBox="0 0 40 40"><rect width="40" height="40" fill="white"/><circle cx="20" cy="20" r="12" fill="none" stroke="black" stroke-width="5"/></svg>"#.to_vec();
        let mut e = Engine::new();
        let added = e.add_image(svg).unwrap();
        assert_eq!(added["width"], 40);
        let g = graph(added["id"].as_str().unwrap(), 2.0);
        let r = e.run(&g, None, &|| 0.0);
        assert_eq!(r["nodes"]["st"]["state"], "done", "{r}");
        let key = r["nodes"]["st"]["key"].as_str().unwrap().to_string();
        let Some(Value::Solid(s)) = e.value(&key, "solid") else {
            panic!()
        };
        assert_eq!(s.stats["islands_remaining"], 0);
    }

    #[test]
    fn vector_polygon_path_builds_island_free_plate_and_detail_adds_triangles() {
        let mut e = Engine::new();
        let id = e.add_image(test_png()).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let run = |e: &mut Engine, detail: f64, raised_bridges: bool| {
            let g = json!({
                "nodes": [
                    {"id": "s", "type": "source", "params": {"image_id": id, "resolution": 120}},
                    {"id": "t", "type": "threshold", "params": {}},
                    {"id": "tr", "type": "trace", "params": {}},
                    {"id": "v", "type": "vector_stencil", "params": {"detail": detail, "raised_bridges": raised_bridges}},
                ],
                "edges": [
                    {"from": ["s", "image"], "to": ["t", "image"]},
                    {"from": ["t", "mask"], "to": ["tr", "mask"]},
                    {"from": ["tr", "shapes"], "to": ["v", "shapes"]},
                ]
            });
            let r = e.run(&g, None, &|| 0.0);
            assert_eq!(r["nodes"]["v"]["state"], "done", "{r}");
            let key = r["nodes"]["v"]["key"].as_str().unwrap().to_string();
            let Some(Value::Solid(s)) = e.value(&key, "solid") else {
                panic!()
            };
            let tris = s.stats["triangles"].as_u64().unwrap();
            assert_eq!(s.stats["islands_remaining"], 0);
            assert_eq!(s.stats["watertight"], true);
            assert!(
                s.stats["islands_found"].as_u64().unwrap() > 0,
                "{:?}",
                s.stats
            );
            assert!(
                s.stats["bridge_pixels"].as_u64().unwrap() > 0,
                "{:?}",
                s.stats
            );
            (tris, s.stats["first_layer_islands"].as_u64().unwrap())
        };
        let (lo, _) = run(&mut e, 1.0, false);
        let (hi, _) = run(&mut e, 4.0, false);
        assert!(hi > lo, "{lo} -> {hi}");
        let (_, first_layer_islands) = run(&mut e, 3.0, true);
        assert!(first_layer_islands > 0);
    }

    #[test]
    fn tiles_a_real_stencil_into_watertight_parts_and_zip() {
        let mut e = Engine::new();
        let id = e.add_image(test_png()).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let mut g = graph(&id, 6.0);
        g["nodes"][3]["params"] = json!({"max_x": 60.0, "max_y": 60.0});
        let t0 = std::time::Instant::now();
        let r = e.run(&g, None, &|| 0.0);
        assert_eq!(r["nodes"]["ex"]["state"], "done", "{r}");
        eprintln!("tiling took {:?}", t0.elapsed());
        let key = r["nodes"]["ex"]["key"].as_str().unwrap().to_string();
        let Some(Value::Parts(p)) = e.value(&key, "parts") else {
            panic!()
        };
        assert!(
            p.files.len() >= 5,
            "{:?}",
            p.files.iter().map(|f| &f.0).collect::<Vec<_>>()
        );
        assert!(p.meta["tiling"]["connectors"].as_u64().unwrap() > 0);
        let (mime, z) = Value::Parts(p.clone()).encode().unwrap();
        assert_eq!(mime, "application/zip");
        assert_eq!(&z[..4], &[0x50, 0x4b, 3, 4]);
    }

    #[test]
    fn mounting_pads_threads_and_hardware_nodes_run() {
        let mut e = Engine::new();
        let id = e.add_image(test_png()).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let mut g = graph(&id, 8.0);
        g["nodes"][2]["params"] =
            json!({"thickness_mm": 8.0, "margin_mm": 12.0, "width_mm": 120.0});
        g["nodes"][3]["params"] = json!({"max_x": 70.0, "max_y": 0.0, "pad_count": 2, "pad_thread": "M6", "mount_screw": "M4", "mount_offset": 16.0});
        g["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "hw", "type": "hardware", "params": {"screw": "M8"}}));
        let r = e.run(&g, None, &|| 0.0);
        assert_eq!(r["nodes"]["ex"]["state"], "done", "{r}");
        assert_eq!(r["nodes"]["hw"]["state"], "done", "{r}");
        let key = r["nodes"]["ex"]["key"].as_str().unwrap().to_string();
        let Some(Value::Parts(p)) = e.value(&key, "parts") else {
            panic!()
        };
        let names: Vec<_> = p.files.iter().map(|f| f.0.clone()).collect();
        assert!(names.iter().any(|n| n.starts_with("joiner_")), "{names:?}");
        assert_eq!(p.meta["pads"]["pads"], 2);
    }

    #[test]
    fn runs_caches_and_reruns_only_what_changed() {
        let mut e = Engine::new();
        let id = e.add_image(test_png()).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let now = || 0.0;
        let r = e.run(&graph(&id, 2.0), None, &now);
        assert_eq!(r["status"], "done");
        assert_eq!(r["nodes"]["st"]["state"], "done");
        assert_eq!(r["nodes"]["st"]["cached"], false);
        let key = r["nodes"]["st"]["key"].as_str().unwrap();
        let Some(Value::Solid(s)) = e.value(key, "solid") else {
            panic!("no solid")
        };
        assert_eq!(s.stats["islands_remaining"], 0);
        assert_eq!(s.stats["watertight"], true);

        let again = e.run(&graph(&id, 2.0), None, &now);
        assert_eq!(again["nodes"]["st"]["cached"], true);
        let changed = e.run(&graph(&id, 3.0), None, &now);
        assert_eq!(changed["nodes"]["t"]["cached"], true);
        assert_eq!(changed["nodes"]["st"]["cached"], false);
    }

    #[test]
    fn targets_limit_evaluation_and_errors_stay_local() {
        let mut e = Engine::new();
        let id = e.add_image(test_png()).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let r = e.run(&graph(&id, 2.0), Some(&["t".to_string()]), &|| 0.0);
        assert_eq!(r["nodes"]["t"]["state"], "done");
        assert_eq!(r["nodes"]["st"]["state"], "pending");

        let missing = e.run(&graph("nope", 2.0), None, &|| 0.0);
        assert_eq!(missing["nodes"]["s"]["state"], "error");
        assert_eq!(
            missing["nodes"]["st"]["error"],
            "waiting on a failed node upstream"
        );
    }

    #[test]
    fn rejects_bad_connections_and_cycles() {
        let mut e = Engine::new();
        let bad = json!({"nodes": [{"id": "a", "type": "threshold"}, {"id": "b", "type": "threshold"}],
            "edges": [{"from": ["a", "mask"], "to": ["b", "image"]}]});
        assert_eq!(e.run(&bad, None, &|| 0.0)["status"], "error");
        let cyc = json!({"nodes": [{"id": "a", "type": "export"}, {"id": "b", "type": "export"}],
            "edges": [{"from": ["a", "solid"], "to": ["b", "solid"]}, {"from": ["b", "solid"], "to": ["a", "solid"]}]});
        assert!(e.run(&cyc, None, &|| 0.0)["error"]
            .as_str()
            .unwrap()
            .contains("cycle"));
    }
}
