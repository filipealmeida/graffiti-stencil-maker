//! The node implementations: one `run_*` function per node type of the catalogue.

use super::*;

pub(super) fn run_source(e: &Engine, _: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
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

pub(super) fn run_morphology(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
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

pub(super) fn run_text(_: &Engine, _: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let m = crate::text::render(
        p.s("text"),
        p.s("font"),
        p.f("size") as f32,
        p.f("padding") as usize,
        p.f("line_spacing") as f32,
    )?;
    Ok(vec![("mask", Value::Mask(Rc::new(m)))])
}

pub(super) fn run_transform(
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

pub(super) fn run_smooth(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
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

pub(super) fn run_threshold(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
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

pub(super) fn stencil_params_from(p: &P) -> Params {
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
        min_contact_mm: p.f("min_contact_mm"),
        ..Params::default()
    }
}

pub(super) fn finish_solid(o: crate::Output, p: &P) -> Result<Outputs, String> {
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
    if let Some(f) = &o.fallback {
        stats["fallback"] = json!(f);
    }
    Ok(vec![(
        "solid",
        Value::Solid(Rc::new(Solid { stl: o.stl, stats })),
    )])
}

pub(super) fn run_trace(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
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

pub(super) fn run_round_corners(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
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

pub(super) fn run_remove_islands(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
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

pub(super) fn run_shapes_to_mask(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
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

pub(super) fn run_vector_stencil(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let Some(Value::Shapes(s)) = i.get("shapes") else {
        return Err("input 'shapes' missing".into());
    };
    let q = stencil_params_from(p);
    finish_solid(crate::vector_stencil::build(s, p.f("detail"), &q)?, p)
}

pub(super) fn input_image(i: &HashMap<String, Value>) -> Result<&Rc<RgbImage>, String> {
    match i.get("image") {
        Some(Value::Image(img)) => Ok(img),
        _ => Err("input 'image' missing".into()),
    }
}

pub(super) fn run_reduce(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let r = colour::reduce(
        input_image(i)?,
        p.f("colours") as usize,
        p.f("smooth") as f32,
    );
    Ok(vec![("image", Value::Image(Rc::new(r)))])
}

pub(super) fn run_split(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
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

pub(super) fn run_tones(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
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

pub(super) fn run_stencil(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let Some(Value::Mask(m)) = i.get("mask") else {
        return Err("input 'mask' missing".into());
    };
    finish_solid(
        stencil_from_paint(&m.paint, m.w, m.h, 0, &stencil_params_from(p))?,
        p,
    )
}

pub(super) fn run_remesh(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let Some(Value::Solid(s)) = i.get("solid") else {
        return Err("input 'solid' missing".into());
    };
    let lh = s.stats["layer_height_mm"]
        .as_f64()
        .filter(|&h| h > 0.0)
        .unwrap_or(0.2);
    let q = crate::remesh::Params {
        layer_height: lh,
        from: p.f("layer_from") as u32,
        to: p.f("layer_to") as u32,
        tolerance_mm: p.f("tolerance_mm"),
        max_overhang_deg: p.f("max_overhang_deg"),
        keep_material: p.b("keep_material"),
        smoothing: p.f("smoothing"),
        decimate_mm: if p.b("decimate") {
            p.f("decimate_mm")
        } else {
            0.0
        },
    };
    let (stl, r) = crate::remesh::reduce(&s.stl, &q)?;
    let mut stats = s.stats.clone();
    stats["triangles"] = json!(r.triangles_after);
    stats["stl_bytes"] = json!(stl.len());
    stats["remesh"] = json!({
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
        "triangles_after_layers": r.triangles_layers,
        "decimate_mm": r.decimate_mm,
    });
    Ok(vec![("solid", Value::Solid(Rc::new(Solid { stl, stats })))])
}

pub(super) fn run_hardware(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
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
    let mut meta = json!({});
    if let Some(Value::Parts(src)) = i.get("parts") {
        let plate = &src.meta["plate"];
        let joins = plate["joins"].as_array().map_or(0, |j| j.len());
        let m = &plate["mounting"];
        if plate.is_null() || m.is_null() {
            return Err(
                "the parts have no mounting description: connect a Pads & mounting holes node"
                    .into(),
            );
        }
        if joins > 0 {
            let screw = m["screw"].as_str().unwrap_or("M4");
            let (j, e) = hw::join_plates(
                screw,
                m["threaded"] == true,
                m["clearance"].as_f64().unwrap_or(0.4) / 2.0,
                m["offset"].as_f64().unwrap_or(0.0),
                p.f("extend"),
                p.f("joiner_thickness"),
                seg,
            )?;
            for (mesh, n) in [(j, "joiner"), (e, "extender")] {
                if !mesh.is_watertight() {
                    return Err(format!("verification failed: {n} plate is not watertight"));
                }
                files.push((format!("{n}_{screw}.stl"), mesh.to_stl()));
            }
        }
        meta["joiners"] = json!({"seams_joined": joins, "screw": m["screw"]});
    }
    Ok(vec![(
        "parts",
        Value::Parts(Rc::new(Parts { files, meta })),
    )])
}

pub(super) fn run_export(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
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
        shift_mm: 0.0,
        avoid: Vec::new(),
        reinforce: false,
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

pub(super) fn run_tiles(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    use crate::tiles;
    let Some(Value::Solid(s)) = i.get("solid") else {
        return Err("input 'solid' missing".into());
    };
    let st = &s.stats;
    let (lh, thickness) = (
        st["layer_height_mm"].as_f64().unwrap_or(0.2),
        st["thickness_mm"].as_f64().unwrap_or(0.0),
    );
    let bridge = match (p.b("bridge_islands"), st["plate_top_mm"].as_f64()) {
        (true, Some(top)) => Some(crate::tile_bridge::BridgeParams {
            bridge_mm: p.f("bridge_mm"),
            per_island: p.f("bridges_per_island") as usize,
            raised: p.b("raised_bridges"),
            easy: p.s("z_bridging") == "easy",
            base_layers: (thickness / lh).round() as u32,
            layer_height: lh,
            plate_top: top,
            flip: st["flip"].as_bool().unwrap_or(false),
        }),
        _ => None,
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
        thickness,
        shift_mm: p.f("seam_shift"),
        avoid: Vec::new(),
        reinforce: p.b("reinforce"),
        bridge,
    };
    let mesh = crate::mesh::Mesh::from_stl(&s.stl)?;
    crate::progress::step("read plate", 0.05);
    let t = tiles::tile(&mesh, &tp)?;
    crate::progress::step("cut tiles", 0.9);
    let mut files = Vec::new();
    let mut parts = Vec::new();
    for x in &t.tiles {
        if !x.mesh.is_watertight() {
            return Err(format!("verification failed: {} is not watertight", x.name));
        }
        let file = format!("{}.stl", x.name);
        parts.push(json!({"file": file, "role": "tile", "origin": x.origin, "size": x.size}));
        files.push((file, x.mesh.to_stl()));
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
    let (lo, hi) = mesh.bounds();
    let meta = json!({
        "tiling": t.info,
        "plate": {
            "w": hi[0] - lo[0], "h": hi[1] - lo[1], "thickness": thickness, "layer": lh,
            "top": st["plate_top_mm"], "flip": st["flip"], "margin": st["margin_mm"],
            "seams": t.seams, "connectors": t.connectors, "parts": parts,
        },
    });
    Ok(vec![(
        "parts",
        Value::Parts(Rc::new(Parts { files, meta })),
    )])
}

pub(super) fn run_pads(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    use crate::hardware as hw;
    let Some(Value::Parts(src)) = i.get("parts") else {
        return Err("input 'parts' missing".into());
    };
    let plate = &src.meta["plate"];
    if plate.is_null() {
        return Err("the parts have no plate description: connect a Tiles node".into());
    }
    let num = |v: &Json| v.as_f64().unwrap_or(0.0);
    let entries = plate["parts"].as_array().cloned().unwrap_or_default();
    let tiles: Vec<(String, [f64; 2], [f64; 2])> = entries
        .iter()
        .filter(|e| e["role"] == "tile")
        .map(|e| {
            (
                e["file"].as_str().unwrap_or("").to_string(),
                [num(&e["origin"][0]), num(&e["origin"][1])],
                [num(&e["size"][0]), num(&e["size"][1])],
            )
        })
        .collect();
    if tiles.is_empty() {
        return Err("the plate has no tiles".into());
    }
    let quad = |k: &str| -> Vec<[f64; 4]> {
        plate[k]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|r| [num(&r[0]), num(&r[1]), num(&r[2]), num(&r[3])])
                    .collect()
            })
            .unwrap_or_default()
    };
    let (seams, boxes) = (quad("seams"), quad("connectors"));
    let lo = [
        tiles.iter().map(|t| t.1[0]).fold(f64::MAX, f64::min),
        tiles.iter().map(|t| t.1[1]).fold(f64::MAX, f64::min),
    ];
    let (w, h) = (num(&plate["w"]), num(&plate["h"]));
    let margin = num(&plate["margin"]);
    let (layer, flip) = (num(&plate["layer"]).max(0.05), plate["flip"] == true);
    let (thread_d, pitch) = hw::thread(p.s("pad_thread"))?;
    let dia = hw::pad_diameter(p.s("pad_thread"))?;
    if margin < dia - 1e-6 {
        return Err(format!(
            "the margin is {margin:.1} mm but a {} pad is {dia:.0} mm wide: raise the margin on the stencil node",
            p.s("pad_thread")
        ));
    }
    let height = p.f("pad_height");
    let thick = num(&plate["thickness"]);
    if !flip && (height < layer || thick + height < 4.0 * pitch) {
        return Err(format!(
            "pad height too small for an {} thread: plate + pad must be at least {} mm",
            p.s("pad_thread"),
            4.0 * pitch
        ));
    }
    let mount = p.s("mount_screw") != "none";
    let mp = hw::MountParams {
        screw: p.s("mount_screw").into(),
        clearance: p.f("mount_clearance"),
        threaded: p.b("mount_threaded"),
        offset: 0.0,
        segments: p.f("segments") as usize,
    };
    let hole_d = if mount {
        hw::thread(&mp.screw)?.0 + mp.clearance
    } else {
        0.0
    };
    let mut warnings = vec![];
    let hole_spacing = if mount && margin < hole_d + 3.0 {
        warnings.push(format!(
            "margin of {margin:.1} mm is too small for {} holes (needs {:.1} mm)",
            mp.screw,
            hole_d + 3.0
        ));
        0.0
    } else if mount {
        p.f("hole_spacing")
    } else {
        0.0
    };
    let pad_r = dia / 2.0;
    let keep = hw::Keepout {
        seams: &seams,
        boxes: &boxes,
    };
    let layout_args = |spacing: f64, fixed: &[(f64, f64)]| {
        hw::frame_layout(
            lo,
            w,
            h,
            margin,
            p.f("long_edge"),
            spacing,
            pad_r,
            hole_d / 2.0,
            fixed,
            &keep,
        )
    };
    let layout = layout_args(0.0, &[])?;
    let mut layout = layout;
    let mut joins: Vec<Json> = vec![];
    let mut off = p.f("joiner_offset");
    if mount && off > 0.0 && hole_spacing > 0.0 {
        let hole_r = hole_d / 2.0;
        let c = margin / 2.0;
        let tol = 0.5;
        // one offset serves every joiner plate, so widen it until the connectors at the seams are cleared
        let place = |off: f64| -> (Vec<(f64, f64)>, Vec<Json>, usize) {
            let (mut holes, mut joins, mut skipped) = (vec![], vec![], 0);
            for s in &seams {
                let (across, from, to) = (s[1], s[2], s[3]);
                let (lo_a, hi_a) = if s[0] == 0.0 {
                    (lo[1], lo[1] + h)
                } else {
                    (lo[0], lo[0] + w)
                };
                let (lo_c, hi_c) = if s[0] == 0.0 {
                    (lo[1] + c, lo[1] + h - c)
                } else {
                    (lo[0] + c, lo[0] + w - c)
                };
                for (touches, band) in [
                    ((from - lo_a).abs() < tol, lo_c),
                    ((to - hi_a).abs() < tol, hi_c),
                ] {
                    if !touches {
                        continue;
                    }
                    let pt = |t: f64| if s[0] == 0.0 { (t, band) } else { (band, t) };
                    let (a, b) = (pt(across - off), pt(across + off));
                    let ok = [a, b].iter().all(|&q| {
                        !keep.hits(q.0, q.1, hole_r + 1.5)
                            && layout
                                .holes
                                .iter()
                                .chain(&holes)
                                .all(|o| (q.0 - o.0).hypot(q.1 - o.1) >= 2.0 * hole_r + 1.5)
                            && layout
                                .pads
                                .iter()
                                .all(|o| (q.0 - o.0).hypot(q.1 - o.1) >= hole_r + pad_r + 1.5)
                    });
                    if ok {
                        holes.push(a);
                        holes.push(b);
                        joins.push(json!({"axis": s[0], "a": [a.0, a.1], "b": [b.0, b.1]}));
                    } else {
                        skipped += 1;
                    }
                }
            }
            (holes, joins, skipped)
        };
        let mut best = place(off);
        let mut best_off = off;
        let mut k = 1.0;
        while best.2 > 0 && k <= 20.0 {
            let t = place(off + k);
            if t.2 < best.2 {
                best = t;
                best_off = off + k;
            }
            k += 1.0;
        }
        off = best_off;
        if best.2 > 0 {
            warnings.push(format!(
                "{} seam(s) crossing the frame have no clear spot for a joiner plate",
                best.2
            ));
        }
        layout.holes = best.0;
        joins = best.1;
    }
    if hole_spacing > 0.0 {
        layout = layout_args(hole_spacing, &layout.holes)?;
    }
    crate::progress::step("lay out pads", 0.05);
    let pp = hw::PadParams {
        count: 0,
        thread: p.s("pad_thread").into(),
        height,
        clearance: p.f("thread_clearance"),
        flip,
        segments: p.f("segments") as usize,
    };
    let inside = |t: &(String, [f64; 2], [f64; 2]), c: (f64, f64), r: f64| {
        c.0 - r >= t.1[0]
            && c.0 + r <= t.1[0] + t.2[0]
            && c.1 - r >= t.1[1]
            && c.1 + r <= t.1[1] + t.2[1]
    };
    for (list, r, what) in [
        (&layout.pads, pad_r, "pad"),
        (&layout.holes, hole_d / 2.0, "hole"),
    ] {
        for &c in list {
            if !tiles.iter().any(|t| inside(t, c, r)) {
                return Err(format!(
                    "a {what} at ({:.0}, {:.0}) mm would cross the edge of a tile: use another tile size",
                    c.0, c.1
                ));
            }
        }
    }
    let mut files = Vec::new();
    let mut changed = 0usize;
    for (name, data) in &src.files {
        let Some(t) = tiles.iter().find(|t| &t.0 == name) else {
            files.push((name.clone(), data.clone()));
            continue;
        };
        let local = |list: &[(f64, f64)], r: f64| -> Vec<(f64, f64)> {
            list.iter()
                .filter(|&&c| inside(t, c, r))
                .map(|&c| (c.0 - t.1[0], c.1 - t.1[1]))
                .collect()
        };
        let (ps, hs) = (
            local(&layout.pads, pad_r),
            local(&layout.holes, hole_d / 2.0),
        );
        if ps.is_empty() && hs.is_empty() {
            files.push((name.clone(), data.clone()));
            continue;
        }
        let mut m = crate::mesh::Mesh::from_stl(data)?;
        if !ps.is_empty() {
            m = hw::add_pads_at(&m, &ps, &pp, pad_r, layer)?;
        }
        if !hs.is_empty() {
            m = hw::drill_at(&m, &hs, &mp)?;
        }
        m.quantize_f32();
        if !m.is_watertight() {
            return Err(format!("verification failed: {name} is not watertight"));
        }
        if crate::tile_bridge::components(&m).0 != 1 {
            return Err(format!("verification failed: {name} fell apart"));
        }
        changed += 1;
        files.push((name.clone(), m.to_stl()));
        crate::progress::step("fit pads", 0.05 + 0.9 * changed as f64 / tiles.len() as f64);
    }
    let pts = |l: &[(f64, f64)], r: f64| -> Vec<Json> {
        l.iter()
            .map(|c| json!({"x": c.0, "y": c.1, "r": r}))
            .collect()
    };
    let mut meta = src.meta.clone();
    meta["plate"]["pads"] = json!(pts(&layout.pads, pad_r));
    meta["plate"]["holes"] = json!(pts(&layout.holes, hole_d / 2.0));
    meta["plate"]["joins"] = json!(joins);
    meta["plate"]["mounting"] = json!({
        "screw": p.s("mount_screw"), "threaded": p.b("mount_threaded"),
        "clearance": p.f("mount_clearance"), "offset": off,
    });
    meta["pads"] = json!({
        "pads": layout.pads.len(), "pad_diameter_mm": dia, "pad_thread": p.s("pad_thread"),
        "thread_major_mm": thread_d, "joins": joins.len(), "holes": layout.holes.len(), "hole_diameter_mm": hole_d,
        "warnings": warnings,
    });
    Ok(vec![(
        "parts",
        Value::Parts(Rc::new(Parts { files, meta })),
    )])
}
