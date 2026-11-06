use serde_json::json;
use std::rc::Rc;
use stencil_core::graph::{
    migrate_graph, node_types, Art, Engine, Layer, Mask, Parts, RgbImage, Solid, Value, P,
};

#[test]
fn graph_facade_preserves_public_value_types_and_encoding() {
    let _: Option<P> = None;
    let image = Rc::new(RgbImage {
        w: 2,
        h: 1,
        rgb: vec![0, 0, 0, 255, 255, 255],
    });
    let layer = Value::Layer(Rc::new(Layer {
        art: Art::Image(image.clone()),
        x: 3.0,
        y: 4.0,
        sx: 2.0,
        sy: 3.0,
    }));
    assert_eq!(layer.info()["width"], 4.0);
    assert_eq!(layer.info()["height"], 3.0);
    assert_eq!(
        layer.encode().unwrap(),
        Value::Image(image).encode().unwrap()
    );

    let mask = Value::Mask(Rc::new(Mask {
        w: 2,
        h: 1,
        paint: vec![true, false],
    }));
    assert_eq!(mask.info()["coverage"], 0.5);
    let (mime, png) = mask.encode_png().unwrap();
    assert_eq!(mime, "image/png");
    assert_eq!(
        image::load_from_memory(&png).unwrap().to_luma8().as_raw(),
        &[0, 255]
    );
    assert!(mask.dxf().is_none());
    assert!(mask.part("missing").is_none());

    let solid = Value::Solid(Rc::new(Solid {
        stl: vec![1, 2, 3],
        stats: json!({"watertight": true}),
    }));
    assert_eq!(solid.encode().unwrap(), ("model/stl", vec![1, 2, 3]));
    assert_eq!(solid.part("plate"), Some(vec![1, 2, 3]));
    let parts = Value::Parts(Rc::new(Parts {
        files: vec![("tile.stl".into(), vec![4, 5, 6])],
        meta: json!({}),
    }));
    assert_eq!(parts.encode().unwrap(), ("model/stl", vec![4, 5, 6]));
    assert_eq!(parts.part("tile.stl"), Some(vec![4, 5, 6]));
    assert_eq!(parts.info()["files"]["tile.stl"], 3);
}

#[test]
fn execution_keeps_stable_keys_clamped_params_fresh_outputs_and_fifo_eviction() {
    let mut engine = Engine::new();
    let bytes = br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="2" height="2"/></svg>"#;
    let id = engine.add_image(bytes.to_vec()).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(engine.has_image(&id));
    let mut graph = json!({
        "nodes": [
            {"id": "source", "type": "source", "params": {"image_id": id, "resolution": 20}},
            {"id": "mask", "type": "threshold", "params": {"threshold": 0}}
        ],
        "edges": [{"from": ["source", "image"], "to": ["mask", "image"]}]
    });
    assert_eq!(migrate_graph(&graph), graph);
    assert!(node_types()
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["id"] == "threshold"));
    let first = engine.run(&graph, None, &|| 0.0);
    assert_eq!(first["nodes"]["mask"]["state"], "done");
    let first_key = first["nodes"]["mask"]["key"].as_str().unwrap().to_string();
    let source_key = first["nodes"]["source"]["key"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(engine.take_fresh().len(), 2);
    assert!(engine.take_fresh().is_empty());

    graph["nodes"][1]["params"]["threshold"] = json!(-10);
    let cached = engine.run(&graph, None, &|| 0.0);
    assert_eq!(cached["nodes"]["mask"]["key"], first_key);
    assert_eq!(cached["nodes"]["mask"]["cached"], true);
    assert!(cached["fresh"].as_array().unwrap().is_empty());
    assert!(engine.take_fresh().is_empty());
    assert!(engine.report(&graph, &cached)["timing"].is_array());

    // Cache hits do not promote entries: the limit is 64 insertions, not LRU.
    for threshold in 1..=63 {
        graph["nodes"][1]["params"]["threshold"] = json!(threshold);
        let result = engine.run(&graph, None, &|| 0.0);
        assert_eq!(result["nodes"]["mask"]["state"], "done");
        assert_eq!(engine.take_fresh().len(), 1);
    }
    assert!(engine.value(&source_key, "image").is_none());
    assert!(engine.value(&first_key, "mask").is_some());
    graph["nodes"][1]["params"]["threshold"] = json!(64);
    engine.run(&graph, None, &|| 0.0);
    assert!(engine.value(&first_key, "mask").is_none());
}
