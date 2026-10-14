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
