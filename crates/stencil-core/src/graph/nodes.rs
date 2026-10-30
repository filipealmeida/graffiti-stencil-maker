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

pub(super) fn run_threshold(
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

pub(super) fn stencil_params_from(p: &P) -> Params {
    Params {
        width_mm: p.f("width_mm"),
        thickness_mm: p.f("thickness_mm"),
        margin_mm: p.f("margin_mm"),
        bridge_mm: p.f("bridge_mm"),
        bridges_per_island: p.f("bridges_per_island") as u32,
        bridge_min_angle_deg: if p.s("bridge_spacing") == "spread" {
            p.f("bridge_min_angle_deg")
        } else {
            0.0
        },
        bridge_max_length: p.f("bridge_max_length"),
        bridge_passes: p.f("bridge_passes") as u32,
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
        min_first_layer_width_mm: p.f("min_first_layer_width_mm"),
        first_layer_taper_mm: p.f("first_layer_taper_mm"),
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
    stats["min_first_layer_width_mm"] = json!(p.f("min_first_layer_width_mm"));
    stats["first_layer_taper_mm"] = json!(p.f("first_layer_taper_mm"));
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

pub(super) fn run_round_corners(
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

pub(super) fn run_remove_islands(
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

pub(super) fn run_shapes_to_mask(
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

pub(super) fn run_vector_stencil(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let q = stencil_params_from(p);
    if let Some(Value::Tiles(l)) = i.get("tiles") {
        let q = crate::Params {
            width_mm: l.width,
            margin_mm: l.margin,
            frame_largest: true,
            ..q
        };
        let detail = p.f("detail");
        let method = if p.s("tile_method") == "cut_whole" {
            crate::tile_first::Method::Cut(&q, detail)
        } else {
            crate::tile_first::Method::Vector(&q, detail)
        };
        return tiled_parts(l, method, p);
    }
    let Some(Value::Shapes(s)) = i.get("shapes") else {
        return Err(
            "nothing is connected to the 'shapes' input: connect shapes, or a tile layout".into(),
        );
    };
    finish_solid(crate::vector_stencil::build(s, p.f("detail"), &q)?, p)
}

pub(super) fn input_image(i: &HashMap<String, Value>) -> Result<&Rc<RgbImage>, String> {
    match i.get("image") {
        Some(Value::Image(img)) => Ok(img),
        _ => Err("nothing is connected to the 'image' input: connect an image".into()),
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
    if let Some(Value::Tiles(l)) = i.get("tiles") {
        return tiled_parts(
            l,
            crate::tile_first::Method::Raster(&stencil_params_from(p)),
            p,
        );
    }
    let Some(Value::Mask(m)) = i.get("mask") else {
        return Err(
            "nothing is connected to the 'mask' input: connect a mask, or a tile layout".into(),
        );
    };
    finish_solid(
        stencil_from_paint(&m.paint, m.w, m.h, 0, &stencil_params_from(p))?,
        p,
    )
}

pub(super) fn run_remesh(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let Some(Value::Solid(s)) = i.get("solid") else {
        return Err("nothing is connected to the 'solid' input: connect a solid".into());
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
    let (bolt, nut) = hw::screw_parts(
        p.s("screw"),
        p.f("length"),
        p.f("nut_clearance"),
        seg,
        p.f("tip_taper"),
        p.f("tip_pitches"),
    )?;
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
    if p.b("plate") {
        let dist = p.f("plate_distance");
        let (two, four) = hw::join_plates_padded(
            p.s("screw"),
            p.b("plate_threaded"),
            p.f("plate_clearance"),
            dist / 2.0,
            p.f("extend"),
            p.f("plate_height"),
            seg,
            p.f("plate_padding"),
        )?;
        let (name2, name4) = (
            format!("joiner_plate_{}_{}mm.stl", p.s("screw"), dist),
            format!(
                "joiner_plate_4holes_{}_{}x{}mm.stl",
                p.s("screw"),
                dist,
                p.f("extend")
            ),
        );
        for (mesh, n) in [(two, name2), (four, name4)] {
            if !mesh.is_watertight() {
                return Err(format!("verification failed: {n} is not watertight"));
            }
            files.push((n, mesh.to_stl()));
        }
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
                "the parts have no mounting description: connect the Add pads & mounting holes node"
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
        return Err("nothing is connected to the 'solid' input: connect a solid".into());
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
        seam_gap: p.f("seam_gap"),
        seam_angle_deg: 0.0,
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
                ..Default::default()
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
        return Err("nothing is connected to the 'solid' input: connect a solid".into());
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
            ..Default::default()
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
        seam_gap: p.f("seam_gap"),
        seam_angle_deg: p.f("seam_angle_deg"),
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
        return Err("nothing is connected to the 'parts' input: connect parts".into());
    };
    let plate = &src.meta["plate"];
    if plate.is_null() {
        return Err(
            "the parts have no plate description: connect a Split volume or Tile layout node"
                .into(),
        );
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
        return Err("the plate has no tiles: connect a Split volume node (or a Tile layout) that cuts it into tiles first".into());
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
    let no_pads = p.s("pad_thread") == "none";
    let (thread_d, pitch) = if no_pads {
        (0.0, 0.0)
    } else {
        hw::thread(p.s("pad_thread"))?
    };
    let dia = if no_pads {
        0.0
    } else {
        hw::pad_diameter(p.s("pad_thread"))?
    };
    if !no_pads && margin < dia - 1e-6 {
        return Err(format!(
            "the margin is {margin:.1} mm but a {} pad is {dia:.0} mm wide: raise the margin on the stencil node",
            p.s("pad_thread")
        ));
    }
    let height = p.f("pad_height");
    let thick = num(&plate["thickness"]);
    if !no_pads && !flip && (height < layer || thick + height < 4.0 * pitch) {
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
    let hole_r = hole_d / 2.0;
    let mount_boss_r = if mount && !flip {
        (hole_r + 1.5 + height).min(margin / 2.0)
    } else {
        hole_r
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
    // a bevelled or clear seam face reaches past the seam line at the plate's faces, where pads and bosses stand
    let reach = {
        let t = &src.meta["tiling"];
        let secant = 1.0 / num(&t["seam_angle_deg"]).to_radians().cos();
        let lean = num(&t["seam_angle_deg"]).to_radians().tan();
        lean * thick / 2.0 + num(&t["seam_gap_mm"]) / 2.0 * secant
    };
    let keep = hw::Keepout {
        seams: &seams,
        boxes: &boxes,
        reach,
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
            mount_boss_r,
            fixed,
            &keep,
        )
    };
    let mut layout = layout_args(0.0, &[])?;
    if no_pads {
        layout.pads.clear();
    }
    let mut joins: Vec<Json> = vec![];
    let mut off = p.f("joiner_offset");
    if mount && off > 0.0 && hole_spacing > 0.0 {
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
                    let ok =
                        [a, b].iter().all(|&q| {
                            !keep.hits(q.0, q.1, mount_boss_r + 1.5)
                                && layout.holes.iter().chain(&holes).all(|o| {
                                    (q.0 - o.0).hypot(q.1 - o.1) >= 2.0 * mount_boss_r + 1.5
                                })
                                && layout.pads.iter().all(|o| {
                                    (q.0 - o.0).hypot(q.1 - o.1) >= mount_boss_r + pad_r + 1.5
                                })
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
        if no_pads {
            layout.pads.clear();
        }
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
        (&layout.holes, mount_boss_r, "hole"),
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
    let done = crate::par::map(
        &src.files,
        |(name, data)| -> Result<((String, Vec<u8>), bool), String> {
            let Some(t) = tiles.iter().find(|t| &t.0 == name) else {
                return Ok(((name.clone(), data.clone()), false));
            };
            let local = |list: &[(f64, f64)], r: f64| -> Vec<(f64, f64)> {
                list.iter()
                    .filter(|&&c| inside(t, c, r))
                    .map(|&c| (c.0 - t.1[0], c.1 - t.1[1]))
                    .collect()
            };
            let (ps, hs) = (
                local(&layout.pads, pad_r),
                local(&layout.holes, mount_boss_r),
            );
            if ps.is_empty() && hs.is_empty() {
                return Ok(((name.clone(), data.clone()), false));
            }
            let mut m = crate::mesh::Mesh::from_stl(data)?;
            let plate_top = m.bounds().1[2];
            if !ps.is_empty() {
                m = hw::add_pads_at(&m, &ps, &pp, pad_r, layer)?;
            }
            if !hs.is_empty() {
                if !flip {
                    m = hw::add_mount_bosses(
                        &m,
                        &hs,
                        hole_r,
                        height,
                        margin,
                        mp.segments,
                        plate_top,
                    )?;
                }
                m = hw::drill_at(&m, &hs, &mp)?;
            }
            m.quantize_f32();
            if !m.is_watertight() {
                return Err(format!("verification failed: {name} is not watertight"));
            }
            if crate::tile_bridge::components(&m).0 != 1 {
                return Err(format!("verification failed: {name} fell apart"));
            }
            Ok(((name.clone(), m.to_stl()), true))
        },
    );
    let mut files = Vec::new();
    for r in done {
        files.push(r?.0);
    }
    crate::progress::step("fit pads", 0.95);
    // the plates that screw two tiles together across a seam, on the mounting holes either side of it
    if !joins.is_empty() {
        let (j, e) = hw::join_plates(
            p.s("mount_screw"),
            p.b("mount_threaded"),
            p.f("mount_clearance") / 2.0,
            off,
            p.f("extend"),
            height,
            p.f("segments") as usize,
        )?;
        for (mesh, n) in [(j, "joiner"), (e, "extender")] {
            if !mesh.is_watertight() {
                return Err(format!("verification failed: {n} plate is not watertight"));
            }
            files.push((format!("{n}_{}.stl", p.s("mount_screw")), mesh.to_stl()));
        }
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

pub(super) fn run_ring(_: &Engine, i: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    use crate::{hardware as hw, ring};
    let Some(Value::Parts(src)) = i.get("parts") else {
        return Err("nothing is connected to the 'parts' input: connect parts".into());
    };
    let plate = &src.meta["plate"];
    let m = &plate["mounting"];
    if plate.is_null() || m.is_null() {
        return Err(
            "the parts have no mounting description: connect the Add pads & mounting holes node"
                .into(),
        );
    }
    let screw = m["screw"].as_str().unwrap_or("none").to_string();
    if screw == "none" {
        return Err("the plate has no mounting holes to join the ring to: pick a mounting hole screw in the Pads node".into());
    }
    let num = |v: &Json| v.as_f64().unwrap_or(0.0);
    let margin = num(&plate["margin"]);
    let threaded = m["threaded"] == true;
    let clearance = m["clearance"].as_f64().unwrap_or(0.4);
    let seg = p.f("segments") as usize;
    let lo = {
        let t: Vec<&Json> = plate["parts"]
            .as_array()
            .map(|a| a.iter().filter(|e| e["role"] == "tile").collect())
            .unwrap_or_default();
        if t.is_empty() {
            return Err("the plate has no tiles: connect a Split volume node (or a Tile layout) that cuts it into tiles first".into());
        }
        [
            t.iter()
                .map(|e| num(&e["origin"][0]))
                .fold(f64::MAX, f64::min),
            t.iter()
                .map(|e| num(&e["origin"][1]))
                .fold(f64::MAX, f64::min),
        ]
    };
    let at = |v: &Json| (num(&v["x"]), num(&v["y"]));
    let joined: Vec<(f64, f64)> = plate["joins"]
        .as_array()
        .map(|a| {
            a.iter()
                .flat_map(|j| [&j["a"], &j["b"]])
                .map(|v| (num(&v[0]), num(&v[1])))
                .collect()
        })
        .unwrap_or_default();
    let holes: Vec<(f64, f64)> = plate["holes"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(at)
                .filter(|h| joined.iter().all(|j| (j.0 - h.0).hypot(j.1 - h.1) > 0.05))
                .collect()
        })
        .unwrap_or_default();
    let (d, _) = hw::thread(&screw)?;
    let c = margin / 2.0;
    let (pw, ph) = (num(&plate["w"]), num(&plate["h"]));
    let width = p.f("width");
    let rings = p.f("rings").max(1.0) as usize;
    let thick = num(&plate["thickness"]);
    let mp = hw::MountParams {
        screw: screw.clone(),
        clearance,
        threaded,
        offset: 0.0,
        segments: seg,
    };
    let mut slabs: Vec<ring::Slab> = vec![];
    let (mut plate_joints, mut joiner_plates, mut wall_holes, mut unused) = (0, 0, 0, 0);
    let mut cands = holes;
    for k in 1..=rings {
        let grow = (k - 1) as f64 * width;
        let (rlo, rw, rh) = (
            [lo[0] - grow, lo[1] - grow],
            pw + 2.0 * grow,
            ph + 2.0 * grow,
        );
        let first = slabs.len();
        let prev: Vec<_> = slabs.iter().map(|s| (s.lo, s.hi)).collect();
        let avoid: Vec<(f64, f64)> = slabs.iter().flat_map(|s| s.holes.clone()).collect();
        if k > 1 {
            cands = ring::side_holes(rlo, rw, rh, c, 5.0);
        }
        let layout = ring::plan(&ring::RingParams {
            lo: rlo,
            w: rw,
            h: rh,
            c,
            ring: width,
            max_x: p.f("slab_length"),
            max_y: p.f("slab_length"),
            hole_r: (d + clearance) / 2.0,
            join_r: d / 2.0 + hw::JOIN_WALL_MM,
            holes: cands.clone(),
            wall_spacing: if p.b("wall_holes") && k == rings {
                p.f("hole_spacing").max(10.0)
            } else {
                0.0
            },
            ring_no: k,
            prev,
            avoid,
            min_gap: if k > 1 { p.f("hole_spacing") } else { 0.0 },
        })
        .map_err(|e| {
            if rings > 1 {
                format!("ring {k}: {e}")
            } else {
                e
            }
        })?;
        // the inner side of each joint is a hole in the ring inside this one
        for &old in &layout.used {
            if let Some(sl) = slabs[..first]
                .iter_mut()
                .find(|s| old.0 > s.lo[0] && old.0 < s.hi[0] && old.1 > s.lo[1] && old.1 < s.hi[1])
            {
                sl.holes.push(old);
            }
        }
        plate_joints += layout.used.len();
        joiner_plates += layout.joints.len();
        wall_holes += layout.wall_holes;
        unused += layout.unused;
        slabs.extend(layout.slabs);
    }
    let mut files = src.files.clone();
    // 0 = one layer of the plate, which is what a printed number needs to show
    let depth = match (p.f("pocket_depth"), num(&plate["layer"])) {
        (d, _) if d > 0.0 => d,
        (_, l) if l > 0.0 => l,
        _ => 0.2,
    };
    if p.b("number_parts") && depth > thick - 1.0 {
        return Err(format!(
            "the number pockets are {depth} mm deep but the slabs are only {thick} mm thick: lower the pocket depth"
        ));
    }
    let mut unnumbered: Vec<String> = vec![];
    let gap = p.f("gap");
    for sl_full in &slabs {
        // each slab gives up half the gap on every side, the first ring the whole gap towards the plate; the
        // holes stay where they are, so the joiner plates stay centred on the seams
        let mut sl = sl_full.clone();
        for a in 0..2 {
            sl.lo[a] += gap / 2.0;
            sl.hi[a] -= gap / 2.0;
        }
        if let Some(rest) = sl.name.strip_prefix("ring_1_") {
            match rest.split('_').next() {
                Some("bottom") => sl.hi[1] -= gap / 2.0,
                Some("top") => sl.lo[1] += gap / 2.0,
                Some("left") => sl.hi[0] -= gap / 2.0,
                Some("right") => sl.lo[0] += gap / 2.0,
                _ => {}
            }
        }
        let sl = &sl;
        // drilled around the origin: the boolean is robust there, far from it it can fail
        let (cx, cy) = ((sl.lo[0] + sl.hi[0]) / 2.0, (sl.lo[1] + sl.hi[1]) / 2.0);
        let block = crate::mesh::Mesh::cuboid(
            [sl.lo[0] - cx, sl.lo[1] - cy, 0.0],
            [sl.hi[0] - cx, sl.hi[1] - cy, thick],
        );
        let local: Vec<(f64, f64)> = sl.holes.iter().map(|h| (h.0 - cx, h.1 - cy)).collect();
        let mut mesh = hw::drill_at(&block, &local, &mp)?;
        if p.b("number_parts") {
            let no = slabs.iter().position(|s| s.name == sl.name).unwrap() + 1;
            match ring::place_number(sl, no, p.f("number_size"), (d + clearance) / 2.0) {
                Some(((nx, ny), rects)) => {
                    // a pocket one layer deep in the top face
                    // one stroke at a time: overlapping strokes in one cutter break the boolean
                    for r in &rects {
                        let stroke = crate::mesh::Mesh::cuboid(
                            [r[0] + nx - cx, r[1] + ny - cy, thick - depth],
                            [r[2] + nx - cx, r[3] + ny - cy, thick + 1.0],
                        );
                        mesh = mesh.subtract(&stroke)?;
                    }
                }
                None => unnumbered.push(sl.name.clone()),
            }
        }
        mesh.translate([cx, cy, 0.0]);
        mesh.quantize_f32();
        if !mesh.is_watertight() || crate::tile_bridge::components(&mesh).0 != 1 {
            return Err(format!(
                "verification failed: slab {} is not one watertight piece",
                sl.name
            ));
        }
        files.push((format!("{}.stl", sl.name), mesh.to_stl()));
    }
    // the pair plate joins one seam; the square one has four holes, two each side, for a stronger seam
    let (joiner, square) = hw::join_plates(
        &screw,
        threaded,
        clearance / 2.0,
        c,
        2.0 * c,
        p.f("joiner_thickness"),
        seg,
    )?;
    for (mesh, name) in [(joiner, "ring_joiner"), (square, "ring_joiner_square")] {
        if !mesh.is_watertight() {
            return Err(format!(
                "verification failed: {name} plate is not watertight"
            ));
        }
        files.push((format!("{name}_{screw}.stl"), mesh.to_stl()));
    }
    // bolt and nut for the joiner plates: the head sits on the plate, the nut ends flush with the far face
    let mut hardware = json!(null);
    if p.b("hardware") {
        let nut_h = (0.8 * hw::thread(&screw)?.0 * 10.0).round() / 10.0;
        let auto = (p.f("joiner_thickness") + thick + nut_h).ceil();
        let length = if p.f("screw_length") > 0.0 {
            p.f("screw_length")
        } else {
            auto
        };
        let (bolt, nut) = hw::screw_parts(
            &screw,
            length,
            p.f("nut_clearance"),
            seg,
            p.f("tip_taper"),
            p.f("tip_pitches"),
        )?;
        let (bn, nn) = (
            format!("ring_bolt_{screw}x{length}.stl"),
            format!("ring_nut_{screw}.stl"),
        );
        for (mesh, name) in [(bolt, &bn), (nut, &nn)] {
            if !mesh.is_watertight() {
                return Err(format!("verification failed: {name} is not watertight"));
            }
            files.push((name.clone(), mesh.to_stl()));
        }
        hardware = json!({
            "bolt_file": bn, "nut_file": nn, "length_mm": length, "auto_length_mm": auto,
            "bolts_needed": 2 * joiner_plates, "nuts_needed": 2 * joiner_plates,
            "nut_clearance_mm": p.f("nut_clearance"),
        });
    }
    let mut meta = src.meta.clone();
    meta["ring"] = json!({
        "hardware": hardware,
        "rings": rings, "slabs": slabs.len(), "joiner_plates": joiner_plates, "plate_joints": plate_joints,
        "wall_holes": wall_holes, "unused_plate_holes": unused, "width_mm": width,
        "numbered": p.b("number_parts"), "unnumbered": unnumbered,
        "thickness_mm": thick, "gap_mm": gap, "screw": screw, "joiner_file": format!("ring_joiner_{screw}.stl"), "square_joiner_file": format!("ring_joiner_square_{screw}.stl"),
    });
    Ok(vec![(
        "parts",
        Value::Parts(Rc::new(Parts { files, meta })),
    )])
}

pub(super) fn run_tile_layout(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    use crate::tile_first as tf;
    use crate::tiles::Shape;
    let diameter: f64 = p.s("connector_d").parse().unwrap_or(3.0);
    let lp = tf::LayoutParams {
        edits: tf::Edits::parse(p.s("edits"))?,
        width: p.f("width_mm"),
        margin: p.f("margin_mm"),
        max_x: p.f("max_x"),
        max_y: p.f("max_y"),
        seam_shift: p.f("seam_shift"),
        spec: tf::Connectors {
            shape: if p.s("connector") == "round" {
                Shape::Round
            } else {
                Shape::Hex
            },
            diameter,
            clearance: p.f("clearance"),
            depth: p.f("depth"),
            segments: p.f("segments") as usize,
            spacing: p.f("spacing"),
        },
    };
    let layout = match (i.get("mask"), i.get("shapes")) {
        (Some(Value::Mask(m)), None) => tf::from_mask(&m.paint, m.w, m.h, &lp)?,
        (None, Some(Value::Shapes(s))) => tf::from_shapes(s, &lp)?,
        (None, None) => {
            return Err("connect a mask (for a Stencil) or shapes (for a Vector stencil)".into())
        }
        _ => return Err("connect either a mask or shapes, not both".into()),
    };
    Ok(vec![("tiles", Value::Tiles(Rc::new(layout)))])
}

/// Builds every tile of the layout with the stencil `method` and describes the plate the way a Tiles node does, so
/// Pads and Screws can follow.
fn tiled_parts(
    l: &crate::tile_first::Layout,
    method: crate::tile_first::Method,
    p: &P,
) -> Result<Outputs, String> {
    let b = crate::tile_first::build(l, &method)?;
    crate::progress::step("build tiles", 0.9);
    let mut files = Vec::new();
    let mut parts = Vec::new();
    for t in &b.tiles {
        let file = format!("{}.stl", t.name);
        parts.push(json!({"file": file, "role": "tile", "origin": t.origin, "size": t.size}));
        files.push((file, t.mesh.to_stl()));
    }
    if !l.pockets.is_empty() {
        let c = &l.spec;
        files.push((
            format!(
                "connector_{}_d{}mm.stl",
                if c.shape == crate::tiles::Shape::Round {
                    "round"
                } else {
                    "hex"
                },
                c.diameter
            ),
            crate::tiles::connector_rod(c.shape, c.diameter, c.depth, c.segments).to_stl(),
        ));
    }
    let meta = json!({
        "tiling": b.info,
        "plate": {
            "w": l.width, "h": l.height, "thickness": p.f("thickness_mm"), "layer": b.layer,
            "top": b.top, "flip": p.b("flip"), "margin": l.margin,
            "seams": b.seams, "connectors": b.connectors, "parts": parts,
        },
    });
    Ok(vec![(
        "parts",
        Value::Parts(Rc::new(Parts { files, meta })),
    )])
}
