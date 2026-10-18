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
    g["nodes"][2]["params"] = json!({"thickness_mm": 8.0, "margin_mm": 12.0, "width_mm": 120.0});
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

#[test]
fn unreadable_and_oversized_pictures_are_refused() {
    let mut e = Engine::new();
    assert!(e.add_image(b"not a picture".to_vec()).is_err());
    let huge = br#"<svg xmlns="http://www.w3.org/2000/svg" width="20000" height="20000"/>"#;
    assert!(e
        .add_image(huge.to_vec())
        .unwrap_err()
        .contains("megapixels"));
}

#[test]
fn picture_ids_are_stable_across_platforms() {
    // pinned so a 32-bit wasm build and a 64-bit native build agree
    let id = Engine::new()
        .add_image(b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4\" height=\"4\"/>".to_vec())
        .unwrap();
    assert_eq!(id["id"], "2dfafcadf659590b");
}

#[test]
fn morphology_node_runs_each_operation_and_reports_bad_input() {
    let mut e = Engine::new();
    let id = e.add_image(test_png()).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let run = |e: &mut Engine, params: Json| {
        let g = json!({
            "nodes": [
                {"id": "s", "type": "source", "params": {"image_id": id, "resolution": 120}},
                {"id": "t", "type": "threshold", "params": {}},
                {"id": "m", "type": "morphology", "params": params},
            ],
            "edges": [
                {"from": ["s", "image"], "to": ["t", "image"]},
                {"from": ["t", "mask"], "to": ["m", "mask"]},
            ]
        });
        let r = e.run(&g, None, &|| 0.0);
        let state = r["nodes"]["m"]["state"].clone();
        let count = r["nodes"]["m"]["key"]
            .as_str()
            .and_then(|k| match e.value(k, "mask") {
                Some(Value::Mask(m)) => Some(m.paint.iter().filter(|&&v| v).count()),
                _ => None,
            });
        (state, count)
    };
    let (_, base) = run(
        &mut e,
        json!({"op": "dilate", "radius": 1, "kernel": "square", "iterations": 1}),
    );
    let base = base.unwrap();
    let (s, eroded) = run(&mut e, json!({"op": "erode", "radius": 2}));
    assert_eq!(s, "done");
    let (_, dilated) = run(&mut e, json!({"op": "dilate", "radius": 2}));
    assert!(eroded.unwrap() < base && base < dilated.unwrap());
    for op in [
        "open",
        "close",
        "gradient",
        "white_hat",
        "black_hat",
        "thin",
        "skeleton",
    ] {
        let (s, c) = run(&mut e, json!({"op": op, "radius": 2}));
        assert_eq!(s, "done", "{op}");
        assert!(c.is_some(), "{op}");
    }
    let (s, ends) = run(&mut e, json!({"op": "hit_or_miss", "pattern": "isolated"}));
    assert_eq!(s, "done");
    assert_eq!(ends, Some(0));
    let (s, _) = run(
        &mut e,
        json!({"op": "hit_or_miss", "pattern": "custom", "custom": "11"}),
    );
    assert_eq!(s, "error");
}

#[test]
fn smooth_node_scales_the_mask_and_feeds_the_stencil() {
    let mut e = Engine::new();
    let id = e.add_image(test_png()).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let g = json!({
        "nodes": [
            {"id": "s", "type": "source", "params": {"image_id": id, "resolution": 120}},
            {"id": "t", "type": "threshold", "params": {}},
            {"id": "m", "type": "smooth", "params": {"radius": 1.5, "supersample": 2}},
            {"id": "st", "type": "stencil", "params": {"width_mm": 100}},
        ],
        "edges": [
            {"from": ["s", "image"], "to": ["t", "image"]},
            {"from": ["t", "mask"], "to": ["m", "mask"]},
            {"from": ["m", "mask"], "to": ["st", "mask"]},
        ]
    });
    let r = e.run(&g, None, &|| 0.0);
    assert_eq!(r["nodes"]["m"]["state"], "done");
    assert_eq!(r["nodes"]["st"]["state"], "done", "{r}");
    let size = |n: &str| {
        let k = r["nodes"][n]["key"].as_str().unwrap();
        match e.value(k, "mask") {
            Some(Value::Mask(m)) => (m.w, m.h),
            _ => panic!(),
        }
    };
    let (a, b) = (size("t"), size("m"));
    assert_eq!((a.0 * 2, a.1 * 2), b);
}

#[test]
fn transform_node_crops_and_rotates_before_tracing() {
    let mut e = Engine::new();
    let id = e.add_image(test_png()).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let g = json!({
        "nodes": [
            {"id": "s", "type": "source", "params": {"image_id": id, "resolution": 120}},
            {"id": "x", "type": "transform", "params": {"crop_left": 10, "crop_right": 10, "turn": "90"}},
            {"id": "t", "type": "threshold", "params": {}},
            {"id": "st", "type": "stencil", "params": {"width_mm": 100}},
        ],
        "edges": [
            {"from": ["s", "image"], "to": ["x", "image"]},
            {"from": ["x", "image"], "to": ["t", "image"]},
            {"from": ["t", "mask"], "to": ["st", "mask"]},
        ]
    });
    let r = e.run(&g, None, &|| 0.0);
    assert_eq!(r["nodes"]["x"]["state"], "done", "{r}");
    assert_eq!(r["nodes"]["st"]["state"], "done", "{r}");
    let dims = |n: &str| match e.value(r["nodes"][n]["key"].as_str().unwrap(), "image") {
        Some(Value::Image(i)) => (i.w, i.h),
        _ => panic!(),
    };
    let (a, b) = (dims("s"), dims("x"));
    assert_eq!(b.1, a.0 - 2 * (a.0 as f64 * 0.1).round() as usize);
    assert_eq!(b.0, a.1);
}

#[test]
fn text_node_makes_a_mask_that_becomes_an_island_free_stencil() {
    let mut e = Engine::new();
    let g = json!({
        "nodes": [
            {"id": "t", "type": "text", "params": {"text": "BOA\nO", "size": 120}},
            {"id": "st", "type": "stencil", "params": {"width_mm": 100}},
        ],
        "edges": [{"from": ["t", "mask"], "to": ["st", "mask"]}]
    });
    let r = e.run(&g, None, &|| 0.0);
    assert_eq!(r["nodes"]["t"]["state"], "done", "{r}");
    assert_eq!(r["nodes"]["st"]["state"], "done", "{r}");
    let k = r["nodes"]["st"]["key"].as_str().unwrap();
    let info = e.value(k, "solid").unwrap().info();
    assert_eq!(info["stats"]["islands_remaining"], 0, "{info}");
    assert!(info["stats"]["islands_found"].as_u64().unwrap() >= 3);
}

fn remesh_run(stencil: Json, reducer: Json) -> (Vec<u8>, Vec<u8>, Json) {
    let mut e = Engine::new();
    let g = json!({
        "nodes": [
            {"id": "t", "type": "text", "params": {"text": "BOA", "size": 200}},
            {"id": "st", "type": "stencil", "params": stencil},
            {"id": "cx", "type": "remesh", "params": reducer},
        ],
        "edges": [
            {"from": ["t", "mask"], "to": ["st", "mask"]},
            {"from": ["st", "solid"], "to": ["cx", "solid"]}
        ]
    });
    let r = e.run(&g, None, &|| 0.0);
    assert_eq!(r["nodes"]["cx"]["state"], "done", "{r}");
    let bytes = |id: &str| {
        let k = r["nodes"][id]["key"].as_str().unwrap();
        e.value(k, "solid").unwrap().encode().unwrap().1
    };
    let k = r["nodes"]["cx"]["key"].as_str().unwrap();
    let info = e.value(k, "solid").unwrap().info();
    (bytes("st"), bytes("cx"), info["stats"]["remesh"].clone())
}

fn assert_same_plate(before: &[u8], after: &[u8], volume_tol: f64, shift: f64) {
    use crate::mesh::Mesh;
    let (a, b) = (
        Mesh::from_stl(before).unwrap(),
        Mesh::from_stl(after).unwrap(),
    );
    assert!(b.is_watertight());
    assert_eq!(
        crate::tile_bridge::components(&a).0,
        crate::tile_bridge::components(&b).0
    );
    let (la, ha) = a.bounds();
    let (lb, hb) = b.bounds();
    for k in 0..3 {
        assert!(
            (la[k] - lb[k]).abs() < shift + 1e-3 && (ha[k] - hb[k]).abs() < shift + 1e-3,
            "bounds {k}"
        );
    }
    assert!(b.volume() > 0.0);
    let change = (b.volume() - a.volume()) / a.volume();
    assert!(change.abs() < volume_tol, "volume changed by {change}");
}

#[test]
fn remesh_cuts_vertices_and_keeps_the_plate() {
    let (before, after, c) = remesh_run(
        json!({"width_mm": 100, "thickness_mm": 2}),
        json!({"tolerance_mm": 0.15}),
    );
    assert_same_plate(&before, &after, 0.03, 0.15);
    let (vb, va) = (
        c["vertices_before"].as_u64().unwrap(),
        c["vertices_after"].as_u64().unwrap(),
    );
    assert!(va <= vb, "{c}");
    assert!(after.len() < before.len());
    assert_eq!(c["overhang_violation_mm2"].as_f64().unwrap(), 0.0, "{c}");
    assert!(
        c["max_deviation_mm"].as_f64().unwrap() <= 0.15 + 1e-6,
        "{c}"
    );
}

#[test]
fn remesh_rounds_towards_material() {
    use crate::mesh::Mesh;
    let args = |keep: bool| {
        (
            json!({"width_mm": 100, "thickness_mm": 2}),
            json!({"tolerance_mm": 0.15, "keep_material": keep}),
        )
    };
    let (s, r) = args(true);
    let (before, keep, _) = remesh_run(s, r);
    let (s, r) = args(false);
    let (_, neutral, _) = remesh_run(s, r);
    let v = |b: &[u8]| Mesh::from_stl(b).unwrap().volume();
    assert!(
        v(&keep) >= v(&before) * 0.995,
        "{} {}",
        v(&keep),
        v(&before)
    );
    assert!(v(&keep) >= v(&neutral) - 1e-6);
}

#[test]
fn remesh_on_raised_bridges_is_valid_or_leaves_the_solid_alone() {
    let st =
        json!({"width_mm": 100, "thickness_mm": 2, "raised_bridges": true, "layer_height_mm": 0.2});
    for params in [
        json!({"tolerance_mm": 0.15}),
        json!({"tolerance_mm": 0.15, "layer_from": 2, "layer_to": 4}),
    ] {
        let (before, after, c) = remesh_run(st.clone(), params.clone());
        assert_same_plate(&before, &after, 0.05, 0.15);
        assert_eq!(c["overhang_violation_mm2"].as_f64().unwrap(), 0.0, "{c}");
        if c["skipped"].is_null() {
            assert!(
                c["vertices_after"].as_u64() < c["vertices_before"].as_u64(),
                "{c}"
            );
        } else {
            assert_eq!(before, after, "{c}");
        }
    }
}

#[test]
fn remesh_interval_is_recorded_and_the_overhang_limit_holds() {
    let st = json!({"width_mm": 100, "thickness_mm": 2});
    let (before, after, c) = remesh_run(
        st.clone(),
        json!({"tolerance_mm": 0.15, "layer_from": 3, "layer_to": 9, "max_overhang_deg": 30}),
    );
    assert_same_plate(&before, &after, 0.05, 0.15);
    assert_eq!((c["from"].as_u64(), c["to"].as_u64()), (Some(3), Some(9)));
    assert_eq!(c["overhang_violation_mm2"].as_f64().unwrap(), 0.0, "{c}");
    let (_, _, free) = remesh_run(st, json!({"tolerance_mm": 0.15, "max_overhang_deg": 90}));
    assert_eq!(
        free["overhang_violation_mm2"].as_f64().unwrap(),
        0.0,
        "{free}"
    );
}

#[test]
fn remesh_rejects_an_empty_interval() {
    let mut e = Engine::new();
    let g = json!({
        "nodes": [
            {"id": "t", "type": "text", "params": {"text": "O"}},
            {"id": "st", "type": "stencil", "params": {"thickness_mm": 2}},
            {"id": "cx", "type": "remesh", "params": {"layer_from": 500}},
        ],
        "edges": [
            {"from": ["t", "mask"], "to": ["st", "mask"]},
            {"from": ["st", "solid"], "to": ["cx", "solid"]}
        ]
    });
    let r = e.run(&g, None, &|| 0.0);
    assert_eq!(r["nodes"]["cx"]["state"], "error", "{r}");
}

#[test]
fn shape_cleanup_nodes_feed_a_vector_stencil() {
    let mut e = Engine::new();
    let g = json!({
        "nodes": [
            {"id": "t", "type": "text", "params": {"text": "AK"}},
            {"id": "tr", "type": "trace", "params": {}},
            {"id": "rc", "type": "round_corners", "params": {"radius": 2, "max_angle": 120}},
            {"id": "ri", "type": "remove_islands", "params": {"max_area": 400}},
            {"id": "vs", "type": "vector_stencil", "params": {"width_mm": 80, "thickness_mm": 2}},
        ],
        "edges": [
            {"from": ["t", "mask"], "to": ["tr", "mask"]},
            {"from": ["tr", "shapes"], "to": ["rc", "shapes"]},
            {"from": ["rc", "shapes"], "to": ["ri", "shapes"]},
            {"from": ["ri", "shapes"], "to": ["vs", "shapes"]}
        ]
    });
    let r = e.run(&g, None, &|| 0.0);
    for n in ["rc", "ri", "vs"] {
        assert_eq!(r["nodes"][n]["state"], "done", "{n}: {r}");
    }
    let info = |n: &str, o: &str| {
        e.value(r["nodes"][n]["key"].as_str().unwrap(), o)
            .unwrap()
            .info()
    };
    assert!(info("rc", "rounded")["loops"].as_u64().unwrap() > 0);
    assert!(info("ri", "removed")["kind"] == "shapes");
}

fn colour_set_graph(id: &str, margin: f64, second_margin: f64, params: Json) -> Json {
    json!({
        "nodes": [
            {"id": "s", "type": "source", "params": {"image_id": id, "resolution": 120}},
            {"id": "t", "type": "threshold", "params": {}},
            {"id": "a", "type": "stencil", "params": {"margin_mm": margin}},
            {"id": "b", "type": "stencil", "params": {"margin_mm": second_margin, "flip": true, "width_mm": if second_margin == margin { 100.0 } else { 120.0 }}},
            {"id": "cs", "type": "colour_set", "params": params},
        ],
        "edges": [
            {"from": ["s", "image"], "to": ["t", "image"]},
            {"from": ["t", "mask"], "to": ["a", "mask"]},
            {"from": ["t", "mask"], "to": ["b", "mask"]},
            {"from": ["a", "solid"], "to": ["cs", "colour_1"]},
            {"from": ["b", "solid"], "to": ["cs", "colour_2"]},
        ]
    })
}

#[test]
fn colour_set_drills_matching_holes_and_adds_pins() {
    let mut e = Engine::new();
    let id = e.add_image(test_png()).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = e.run(
        &colour_set_graph(&id, 10.0, 10.0, json!({"holes": 4})),
        None,
        &|| 0.0,
    );
    assert_eq!(r["nodes"]["cs"]["state"], "done", "{r}");
    let key = r["nodes"]["cs"]["key"].as_str().unwrap().to_string();
    let Some(Value::Parts(parts)) = e.value(&key, "parts") else {
        panic!()
    };
    let parts = parts.clone();
    let names: Vec<&str> = parts.files.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(
        names[0] == "colour_1.stl" && names[1] == "colour_2.stl" && names[2].starts_with("pins_4x")
    );
    let plain = e.run(
        &colour_set_graph(&id, 10.0, 10.0, json!({"holes": 0})),
        None,
        &|| 0.0,
    );
    let pkey = plain["nodes"]["cs"]["key"].as_str().unwrap().to_string();
    let Some(Value::Parts(base)) = e.value(&pkey, "parts") else {
        panic!()
    };
    let vol = |b: &[u8]| crate::mesh::Mesh::from_stl(b).unwrap().volume();
    // four 4 mm holes through a 2.2 mm plate remove about 4 * pi * 2^2 * 2.2 mm3, even on the flipped plate
    let removed = 4.0 * std::f64::consts::PI * 4.0 * 2.2;
    for k in 0..2 {
        let cut = vol(&base.files[k].1) - vol(&parts.files[k].1);
        assert!(
            (cut - removed).abs() < removed * 0.15,
            "plate {k}: {cut} vs {removed}"
        );
    }
}

#[test]
fn colour_set_rejects_mismatched_plates_and_small_margins() {
    let mut e = Engine::new();
    let id = e.add_image(test_png()).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = e.run(
        &colour_set_graph(&id, 10.0, 12.0, json!({"holes": 3})),
        None,
        &|| 0.0,
    );
    assert_eq!(r["nodes"]["cs"]["state"], "error", "{r}");
    assert!(
        r["nodes"]["cs"]["error"]
            .as_str()
            .unwrap()
            .contains("same plate width"),
        "{r}"
    );
    let r = e.run(
        &colour_set_graph(&id, 4.0, 4.0, json!({"holes": 3, "hole_d": 6.0})),
        None,
        &|| 0.0,
    );
    assert!(
        r["nodes"]["cs"]["error"]
            .as_str()
            .unwrap()
            .contains("does not fit"),
        "{r}"
    );
}

#[test]
fn tiles_node_cuts_a_stencil_and_describes_the_plate() {
    let mut e = Engine::new();
    let id = e.add_image(test_png()).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut g = graph(&id, 6.0);
    g["nodes"][3] = json!({"id": "ex", "type": "tiles", "params": {"max_x": 60.0, "max_y": 60.0}});
    let r = e.run(&g, None, &|| 0.0);
    assert_eq!(r["nodes"]["ex"]["state"], "done", "{r}");
    let key = r["nodes"]["ex"]["key"].as_str().unwrap().to_string();
    let Some(Value::Parts(p)) = e.value(&key, "parts") else {
        panic!()
    };
    let plate = &p.meta["plate"];
    assert!(plate["w"].as_f64().unwrap() > 60.0);
    let parts = plate["parts"].as_array().unwrap();
    assert!(parts.len() >= 2 && parts.iter().all(|x| x["role"] == "tile"));
    assert!(!plate["seams"].as_array().unwrap().is_empty());
    for (name, bytes) in &p.files {
        let m = crate::mesh::Mesh::from_stl(bytes).unwrap();
        assert!(m.is_watertight(), "{name}");
        if name.starts_with("stencil_") {
            assert_eq!(crate::tile_bridge::components(&m).0, 1, "{name}");
        }
    }
}

#[test]
fn pads_node_fits_pads_and_holes_clear_of_seams() {
    for flip in [false, true] {
        let mut e = Engine::new();
        let id = e.add_image(test_png()).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let mut g = graph(&id, 6.0);
        g["nodes"][2]["params"] =
            json!({"thickness_mm": 6.0, "margin_mm": 12.0, "flip": flip, "width_mm": 140.0});
        g["nodes"][3] =
            json!({"id": "ex", "type": "tiles", "params": {"max_x": 80.0, "max_y": 80.0}});
        g["nodes"].as_array_mut().unwrap().push(json!({"id": "pd", "type": "pads", "params": {"long_edge": 100.0, "hole_spacing": 60.0}}));
        g["edges"]
            .as_array_mut()
            .unwrap()
            .push(json!({"from": ["ex", "parts"], "to": ["pd", "parts"]}));
        let r = e.run(&g, None, &|| 0.0);
        assert_eq!(r["nodes"]["pd"]["state"], "done", "{r}");
        let key = r["nodes"]["pd"]["key"].as_str().unwrap().to_string();
        let Some(Value::Parts(p)) = e.value(&key, "parts") else {
            panic!()
        };
        let plate = &p.meta["plate"];
        let pads = plate["pads"].as_array().unwrap();
        assert!(pads.len() > 4, "corners plus edge pads: {}", pads.len());
        assert!(!plate["holes"].as_array().unwrap().is_empty());
        for pad in pads {
            let x = pad["x"].as_f64().unwrap();
            for s in plate["seams"].as_array().unwrap() {
                if s[0] == 0.0 {
                    assert!((x - s[1].as_f64().unwrap()).abs() > pad["r"].as_f64().unwrap());
                }
            }
        }
        for (name, bytes) in &p.files {
            let m = crate::mesh::Mesh::from_stl(bytes).unwrap();
            assert!(m.is_watertight(), "{name}");
            if name.starts_with("stencil_") {
                assert_eq!(crate::tile_bridge::components(&m).0, 1, "{name}");
            }
        }
    }
}
