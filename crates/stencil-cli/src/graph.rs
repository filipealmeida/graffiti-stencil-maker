use crate::{args::Args, fail};
use base64::Engine as _;
use serde_json::{json, Map, Value as Json};
use std::{
    fs,
    path::{Path, PathBuf},
};
use stencil_core::graph::Engine;

pub(crate) fn pipeline(a: &Args, image: &str) -> Json {
    let mut src = Map::new();
    src.insert("image_id".into(), json!(image));
    set(&mut src, "resolution", a.resolution);

    let mut thr = Map::new();
    set(&mut thr, "threshold", a.threshold);
    if a.invert {
        thr.insert("invert".into(), json!(true));
    }

    let mut st = Map::new();
    for (k, v) in [
        ("width_mm", a.width),
        ("thickness_mm", a.thickness),
        ("margin_mm", a.margin),
        ("layer_height_mm", a.layer_height),
        ("extra_top_layers", a.extra_top_layers),
        ("min_island_mm2", a.min_island),
        ("min_contact_mm", a.min_contact),
        ("min_first_layer_width_mm", a.min_first_layer_width),
        ("first_layer_taper_mm", a.first_layer_taper),
        ("max_overhang_deg", a.max_overhang),
    ] {
        set(&mut st, k, v);
    }
    if a.vector {
        set(&mut st, "detail", a.detail);
    }
    if a.flip {
        st.insert("flip".into(), json!(true));
    }
    // the stencil nodes leave islands; a Bridges node joins them
    let mut br = Map::new();
    set(&mut br, "bridge_mm", a.bridge);
    set(&mut br, "bridges_per_island", a.bridges_per_island);
    if a.raised_bridges {
        br.insert("raised_bridges".into(), json!(true));
    }
    if a.bridge_spread || a.bridge_min_angle.is_some() {
        br.insert("bridge_spacing".into(), json!("spread"));
    }
    set(&mut br, "bridge_min_angle_deg", a.bridge_min_angle);
    set(&mut br, "bridge_max_length", a.bridge_max_length);
    if let Some(z) = &a.z_bridging {
        br.insert("z_bridging".into(), json!(z));
    }

    let mut ex = Map::new();
    for (k, v) in [
        ("max_x", a.tile_width),
        ("max_y", a.tile_height),
        ("clearance", a.connector_clearance),
        ("seam_gap", a.seam_clearance),
        ("seam_angle_deg", a.seam_angle),
        ("segments", a.segments),
        ("pad_count", a.pads),
        ("pad_height", a.pad_height),
        ("mount_offset", a.mount_offset),
        ("mount_extend", a.mount_extend),
    ] {
        set(&mut ex, k, v);
    }
    for (k, v) in [
        ("connector", &a.connector),
        ("connector_d", &a.connector_d),
        ("pad_thread", &a.pad_thread),
        ("mount_screw", &a.mount_screw),
    ] {
        if let Some(v) = v {
            ex.insert(k.into(), json!(v));
        }
    }
    if a.mount_plain {
        ex.insert("mount_threaded".into(), json!(false));
    }

    let mut nodes = vec![
        json!({"id": "src", "type": "source", "params": src}),
        json!({"id": "thr", "type": "threshold", "params": thr}),
    ];
    let mut edges = vec![json!({"from": ["src", "image"], "to": ["thr", "image"]})];
    if a.vector {
        nodes.push(json!({"id": "trace", "type": "trace", "params": {}}));
        nodes.push(json!({"id": "st", "type": "vector_stencil", "params": st}));
        edges.push(json!({"from": ["thr", "mask"], "to": ["trace", "mask"]}));
        edges.push(json!({"from": ["trace", "shapes"], "to": ["st", "shapes"]}));
    } else {
        nodes.push(json!({"id": "st", "type": "stencil", "params": st}));
        edges.push(json!({"from": ["thr", "mask"], "to": ["st", "mask"]}));
    }
    nodes.push(json!({"id": "br", "type": "bridges", "params": br}));
    edges.push(json!({"from": ["st", "solid"], "to": ["br", "body"]}));
    nodes.push(json!({"id": "ex", "type": "export", "params": ex}));
    edges.push(json!({"from": ["br", "solid"], "to": ["ex", "solid"]}));
    if let Some(screw) = &a.hardware {
        let mut hw = json!({"screw": screw});
        if let Some(c) = a.connector.as_ref().filter(|c| *c != "none") {
            hw["connector"] = json!(c);
        }
        if let Some(d) = &a.connector_d {
            hw["connector_d"] = json!(d);
        }
        nodes.push(json!({"id": "hw", "type": "hardware", "params": hw}));
    }
    json!({"nodes": nodes, "edges": edges})
}

/// Points the project's only Image node at a picture from disk.
pub(crate) fn use_picture(engine: &mut Engine, graph: &mut Json, pic: &Path) {
    let bytes =
        fs::read(pic).unwrap_or_else(|e| fail(&format!("cannot read {}: {e}", pic.display())));
    let id = engine.add_image(bytes).unwrap_or_else(|e| fail(&e))["id"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let sources: Vec<usize> = graph["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, n)| n["type"] == "source")
        .map(|(i, _)| i)
        .collect();
    match sources.as_slice() {
        [i] => {
            if !graph["nodes"][*i]["params"].is_object() {
                graph["nodes"][*i]["params"] = json!({});
            }
            graph["nodes"][*i]["params"]["image_id"] = json!(id);
        }
        [] => fail("--source: the project has no Image node"),
        _ => fail("--source: the project has several Image nodes"),
    }
}

/// Applies `NODE.PARAM=VALUE` settings to the graph, refusing anything the node does not have or accept.
pub(crate) fn apply_sets(graph: &mut Json, sets: &[String]) {
    if sets.is_empty() {
        return;
    }
    let types = stencil_core::graph::node_types();
    for s in sets {
        let (target, value) = s
            .split_once('=')
            .unwrap_or_else(|| fail(&format!("--set {s}: write NODE.PARAM=VALUE")));
        let (node, param) = target
            .rsplit_once('.')
            .unwrap_or_else(|| fail(&format!("--set {s}: write NODE.PARAM=VALUE")));
        let nodes = graph["nodes"].as_array_mut().unwrap();
        let by_id: Vec<usize> = (0..nodes.len())
            .filter(|&i| nodes[i]["id"] == node)
            .collect();
        let by_type: Vec<usize> = (0..nodes.len())
            .filter(|&i| nodes[i]["type"] == node)
            .collect();
        let at = match (by_id.as_slice(), by_type.as_slice()) {
            ([i], _) => *i,
            ([], [i]) => *i,
            ([], []) => fail(&format!("--set {s}: the graph has no node '{node}'")),
            _ => fail(&format!(
                "--set {s}: several nodes match '{node}'; use a node id"
            )),
        };
        let ty = nodes[at]["type"].as_str().unwrap_or("").to_string();
        let def = types
            .as_array()
            .and_then(|a| a.iter().find(|t| t["id"] == ty.as_str()))
            .and_then(|t| t["params"].as_array())
            .and_then(|ps| ps.iter().find(|p| p["name"] == param))
            .unwrap_or_else(|| {
                fail(&format!(
                    "--set {s}: a {ty} node has no parameter '{param}'"
                ))
            });
        let parsed: Json = match def["kind"].as_str().unwrap_or("") {
            "int" | "float" => {
                let x: f64 = value
                    .parse()
                    .unwrap_or_else(|_| fail(&format!("--set {s}: '{value}' is not a number")));
                let (lo, hi) = (
                    def["min"].as_f64().unwrap_or(f64::MIN),
                    def["max"].as_f64().unwrap_or(f64::MAX),
                );
                if x < lo || x > hi {
                    fail(&format!("--set {s}: {param} must be between {lo} and {hi}"));
                }
                json!(x)
            }
            "bool" => match value {
                "true" | "on" | "yes" | "1" => json!(true),
                "false" | "off" | "no" | "0" => json!(false),
                _ => fail(&format!("--set {s}: '{value}' is not true or false")),
            },
            "choice" => {
                let ok = def["choices"]
                    .as_array()
                    .is_some_and(|c| c.iter().any(|x| x == value));
                if !ok {
                    fail(&format!(
                        "--set {s}: {param} is one of {}",
                        def["choices"]
                            .as_array()
                            .map(|c| c
                                .iter()
                                .filter_map(Json::as_str)
                                .collect::<Vec<_>>()
                                .join(", "))
                            .unwrap_or_default()
                    ));
                }
                json!(value)
            }
            _ => json!(value),
        };
        if !nodes[at]["params"].is_object() {
            nodes[at]["params"] = json!({});
        }
        nodes[at]["params"][param] = parsed;
    }
}

fn set(m: &mut Map<String, Json>, k: &str, v: Option<f64>) {
    if let Some(v) = v {
        m.insert(k.into(), json!(v));
    }
}

/// Reads a studio project: nodes as `{id, data: {type, params}}` and edges as `{source, sourceHandle, target, targetHandle}`.
pub(crate) fn load_project(engine: &mut Engine, path: &Path) -> (Json, String, PathBuf) {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| fail(&format!("cannot read {}: {e}", path.display())));
    let p: Json = serde_json::from_str(&text)
        .unwrap_or_else(|e| fail(&format!("{} is not a project: {e}", path.display())));
    // pictures are stored under the id of the machine that saved them; remap to this engine's ids
    let mut remap: std::collections::HashMap<String, String> = Default::default();
    for (id, img) in p["images"].as_object().into_iter().flatten() {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(img["base64"].as_str().unwrap_or(""))
            .unwrap_or_else(|_| fail(&format!("image {id} is damaged")));
        let got = engine.add_image(bytes).unwrap_or_else(|e| fail(&e));
        if let Some(new) = got["id"].as_str() {
            remap.insert(id.clone(), new.to_string());
        }
    }
    for (id, model) in p["models"].as_object().into_iter().flatten() {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(model["base64"].as_str().unwrap_or(""))
            .unwrap_or_else(|_| fail(&format!("model {id} is damaged")));
        let got = engine
            .add_model(bytes, model["format"].as_str().unwrap_or(""))
            .unwrap_or_else(|e| fail(&e));
        if let Some(new) = got["id"].as_str() {
            remap.insert(id.clone(), new.to_string());
        }
    }
    let g = &p["graph"];
    let nodes: Vec<Json> = g["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|n| {
            let d = if n["data"].is_object() { &n["data"] } else { n };
            let mut params = d["params"].clone();
            if let Some(new) = params["image_id"].as_str().and_then(|i| remap.get(i)) {
                params["image_id"] = json!(new);
            }
            if let Some(new) = params["model_id"].as_str().and_then(|i| remap.get(i)) {
                params["model_id"] = json!(new);
            }
            json!({"id": n["id"], "type": d["type"], "params": params})
        })
        .collect();
    let edges: Vec<Json> = g["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| {
            if e["from"].is_array() {
                e.clone()
            } else {
                json!({"from": [e["source"], e["sourceHandle"]], "to": [e["target"], e["targetHandle"]]})
            }
        })
        .collect();
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".into());
    (
        stencil_core::graph::migrate_graph(&json!({"nodes": nodes, "edges": edges})),
        stem,
        path.parent().map(Path::to_path_buf).unwrap_or_default(),
    )
}

pub(crate) fn load_input(a: &Args, engine: &mut Engine) -> (Json, String, PathBuf) {
    match (&a.project, &a.input) {
        (Some(path), _) => load_project(engine, path),
        (None, Some(input)) => {
            let bytes = fs::read(input)
                .unwrap_or_else(|e| fail(&format!("cannot read {}: {e}", input.display())));
            let id = engine.add_image(bytes).unwrap_or_else(|e| fail(&e))["id"]
                .as_str()
                .unwrap()
                .to_string();
            let stem = input
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "stencil".into());
            (
                pipeline(a, &id),
                stem,
                input.parent().map(Path::to_path_buf).unwrap_or_default(),
            )
        }
        (None, None) => fail("give a picture or --project (see --help)"),
    }
}
