mod studio;

use base64::Engine as _;
use clap::Parser;
use serde_json::{json, Map, Value as Json};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};
use stencil_core::graph::Engine;

/// Image (png, jpg, bmp) or studio project to island-free, watertight graffiti stencil STLs.
///
/// With an image the pipeline is image -> threshold -> stencil -> tiles & mounting. With --project the
/// saved studio graph is evaluated as it is and every output that no node consumes is written.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// Picture to turn into a stencil
    input: Option<PathBuf>,
    /// A project saved from the studio (.json) to run instead of a picture
    #[arg(long, conflicts_with = "input")]
    project: Option<PathBuf>,
    /// Output STL for a single part, or the folder for several (default: next to the input)
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Trace the picture to curves and build the plate from them
    #[arg(long)]
    vector: bool,
    /// Vector mode: samples per mm of the plate
    #[arg(long)]
    detail: Option<f64>,
    /// Pixels on the longest side
    #[arg(long)]
    resolution: Option<f64>,
    #[arg(long)]
    threshold: Option<f64>,
    #[arg(long)]
    invert: bool,

    #[arg(long)]
    width: Option<f64>,
    #[arg(long)]
    thickness: Option<f64>,
    #[arg(long)]
    margin: Option<f64>,
    #[arg(long)]
    bridge: Option<f64>,
    /// Bridges from every island to the plate (1-5)
    #[arg(long)]
    bridges_per_island: Option<f64>,
    /// Widen any strip of material thinner than this (mm); 0 is off
    #[arg(long)]
    min_contact: Option<f64>,
    /// Worker threads (default: half of the cores, never all of them)
    #[arg(long)]
    threads: Option<usize>,
    #[arg(long)]
    layer_height: Option<f64>,
    /// Bridges rise layer by layer so the first layer keeps every island
    #[arg(long)]
    raised_bridges: bool,
    /// With --raised-bridges: 'steps' or 'easy'
    #[arg(long, value_parser = ["steps", "easy"])]
    z_bridging: Option<String>,
    #[arg(long)]
    extra_top_layers: Option<f64>,
    /// Print upside down (180 degrees about Y)
    #[arg(long)]
    flip: bool,
    /// Delete islands smaller than this area (mm2) instead of bridging
    #[arg(long)]
    min_island: Option<f64>,
    #[arg(long)]
    max_overhang: Option<f64>,

    /// Largest tile width in mm (0 = no tiling)
    #[arg(long)]
    tile_width: Option<f64>,
    /// Largest tile height in mm (0 = no tiling)
    #[arg(long)]
    tile_height: Option<f64>,
    #[arg(long, value_parser = ["hex", "round"])]
    connector: Option<String>,
    /// Connector diameter in mm: 2.5, 3, 4, 5 or 6
    #[arg(long)]
    connector_d: Option<String>,
    #[arg(long)]
    connector_clearance: Option<f64>,
    /// Round segments for connectors, holes and threads
    #[arg(long)]
    segments: Option<f64>,
    /// Raised threaded pads in the frame (0-4)
    #[arg(long)]
    pads: Option<f64>,
    /// M3, M4, M5, M6, M8 or M10
    #[arg(long)]
    pad_thread: Option<String>,
    #[arg(long)]
    pad_height: Option<f64>,
    /// Mounting holes at the seams: none, M3, M4, M5, M6, M8 or M10
    #[arg(long)]
    mount_screw: Option<String>,
    /// Plain holes instead of cut threads
    #[arg(long)]
    mount_plain: bool,
    #[arg(long)]
    mount_offset: Option<f64>,
    #[arg(long)]
    mount_extend: Option<f64>,
    /// Also write a bolt, nut and connector fit test of this screw size
    #[arg(long)]
    hardware: Option<String>,

    /// Set any node parameter: NODE.PARAM=VALUE, where NODE is a node id or a node type used once in the graph
    /// (for example --set tile_layout.max_x=200 --set vector_stencil.bridge_mm=2). Repeat it for more.
    #[arg(long = "set", value_name = "NODE.PARAM=VALUE")]
    set: Vec<String>,
    /// Write the picture of every tile layout in the graph (paint dark, connector blocks blue, orange where they
    /// cover paint, seams red) to this PNG; with several layouts the node id is added to the name
    #[arg(long, value_name = "FILE.png")]
    layout: Option<PathBuf>,
    /// Print the print-readiness report (findings and the slowest nodes)
    #[arg(long)]
    report: bool,
    /// Write the print-readiness report as JSON
    #[arg(long, value_name = "FILE.json")]
    report_file: Option<PathBuf>,
    /// Exit with an error when the report has warnings, not only errors
    #[arg(long)]
    strict: bool,
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    if argv.get(1).map(String::as_str) == Some("studio") {
        studio::run(&argv[2..]);
        return;
    }
    let a = Args::parse();
    stencil_core::init_threads(a.threads);
    let t = Instant::now();
    let mut engine = Engine::new();
    let (graph, stem, dir) = match (&a.project, &a.input) {
        (Some(p), _) => load_project(&mut engine, p),
        (None, Some(i)) => {
            let bytes =
                fs::read(i).unwrap_or_else(|e| fail(&format!("cannot read {}: {e}", i.display())));
            let id = engine.add_image(bytes).unwrap_or_else(|e| fail(&e))["id"]
                .as_str()
                .unwrap()
                .to_string();
            let stem = i
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "stencil".into());
            (
                pipeline(&a, &id),
                stem,
                i.parent().map(Path::to_path_buf).unwrap_or_default(),
            )
        }
        (None, None) => fail("give a picture or --project (see --help)"),
    };

    let mut graph = graph;
    apply_sets(&mut graph, &a.set);
    let result = engine.run(&graph, None, &|| t.elapsed().as_secs_f64() * 1000.0);
    for l in result["log"].as_array().into_iter().flatten() {
        if l["level"] == "error" {
            eprintln!(
                "error: {}: {}",
                l["node"].as_str().unwrap_or(""),
                l["msg"].as_str().unwrap_or("")
            );
        }
    }
    if result["status"] != "done" {
        fail(
            result["error"]
                .as_str()
                .unwrap_or("the graph did not finish"),
        );
    }

    // every plate a stencil node built has passed the island and watertight checks, or the run would have failed
    for n in graph["nodes"].as_array().into_iter().flatten() {
        let r = &result["nodes"][n["id"].as_str().unwrap_or("")];
        let (Some(key), Some(outs)) = (r["key"].as_str(), r["outputs"].as_object()) else {
            continue;
        };
        if let Some(st) = outs
            .contains_key("solid")
            .then(|| engine.value(key, "solid"))
            .flatten()
            .map(|v| v.info())
            .and_then(|i| i.get("stats").cloned())
            .filter(|_| matches!(n["type"].as_str(), Some("stencil" | "vector_stencil")))
        {
            eprintln!(
                "verified: {} islands found, {} remaining, watertight {}, {} triangles",
                st["islands_found"], st["islands_remaining"], st["watertight"], st["triangles"]
            );
        }
    }

    let report = engine.report(&graph, &result);
    if a.report {
        print_report(&report);
    }
    if let Some(p) = &a.report_file {
        write(p, report.to_string().as_bytes());
    }
    let mut layouts = 0;
    if let Some(base) = &a.layout {
        let mut found: Vec<(String, Vec<u8>)> = Vec::new();
        for n in graph["nodes"].as_array().into_iter().flatten() {
            let id = n["id"].as_str().unwrap_or("");
            let Some(v) = result["nodes"][id]["key"]
                .as_str()
                .and_then(|k| engine.value(k, "tiles"))
            else {
                continue;
            };
            found.push((id.to_string(), v.encode().map(|e| e.1).unwrap_or_default()));
        }
        if found.is_empty() {
            fail("--layout: the graph has no Tile layout node");
        }
        let many = found.len() > 1;
        for (id, bytes) in found {
            let path = if many {
                let stem = base.file_stem().map(|s| s.to_string_lossy().to_string());
                base.with_file_name(format!(
                    "{}-{id}.png",
                    stem.unwrap_or_else(|| "layout".into())
                ))
            } else {
                base.clone()
            };
            write(&path, &bytes);
            eprintln!("wrote the tile layout to {}", path.display());
            layouts += 1;
        }
    }
    let worst = report["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["level"] == "warn" || f["level"] == "error")
        .count();
    if a.strict && report["verdict"] != "ready" {
        fail(&format!(
            "--strict: the report has {worst} finding(s) that need a look (use --report to list them)"
        ));
    }

    let consumed: HashSet<&str> = graph["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e["from"][0].as_str())
        .collect();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for n in graph["nodes"].as_array().into_iter().flatten() {
        let id = n["id"].as_str().unwrap_or("");
        let ty = n["type"].as_str().unwrap_or("");
        if consumed.contains(id) || ty == "source" {
            continue;
        }
        let r = &result["nodes"][id];
        let Some(key) = r["key"].as_str() else {
            continue;
        };
        let Some(outs) = r["outputs"].as_object() else {
            continue;
        };
        // a node that offers parts has the solid as a convenience copy: write the parts only
        let prefer_parts = outs
            .get("parts")
            .is_some_and(|m| m["available"].as_bool() != Some(false));
        for (out, meta) in outs {
            let kind = meta["type"].as_str().unwrap_or("");
            if !matches!(kind, "solid" | "parts") || (prefer_parts && kind == "solid") {
                continue;
            }
            let Some(v) = engine.value(key, out) else {
                continue;
            };
            let info = v.info();
            match info["files"].as_object() {
                Some(fs) => {
                    for name in fs.keys() {
                        files.push((name.clone(), v.part(name).unwrap_or_default()));
                    }
                }
                None => files.push((
                    format!("{stem}.stl"),
                    v.encode().map(|e| e.1).unwrap_or_default(),
                )),
            }
            if let Some(s) = info.get("stats") {
                println!("{s}");
            }
            if let Some(m) = info
                .get("meta")
                .filter(|m| m.as_object().is_some_and(|o| !o.is_empty()))
            {
                println!("{m}");
            }
        }
    }
    if files.is_empty() && layouts > 0 {
        return;
    }
    if files.is_empty() {
        fail("the graph produced no STL (end it with a Stencil or a Tiles & mounting node)");
    }

    if files.len() == 1 {
        let path = a.output.clone().unwrap_or_else(|| dir.join(&files[0].0));
        write(&path, &files[0].1);
        eprintln!("wrote {} in {} ms", path.display(), t.elapsed().as_millis());
    } else {
        let out = a
            .output
            .clone()
            .unwrap_or_else(|| dir.join(format!("{stem}_parts")));
        fs::create_dir_all(&out)
            .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", out.display())));
        for (name, bytes) in &files {
            write(&out.join(name), bytes);
        }
        let zip = out.with_extension("zip");
        write(&zip, &stencil_core::zip::zip(&files));
        eprintln!(
            "wrote {} parts to {} and {} in {} ms",
            files.len(),
            out.display(),
            zip.display(),
            t.elapsed().as_millis()
        );
    }
}

fn pipeline(a: &Args, image: &str) -> Json {
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
        ("bridge_mm", a.bridge),
        ("bridges_per_island", a.bridges_per_island),
        ("layer_height_mm", a.layer_height),
        ("extra_top_layers", a.extra_top_layers),
        ("min_island_mm2", a.min_island),
        ("min_contact_mm", a.min_contact),
        ("max_overhang_deg", a.max_overhang),
    ] {
        set(&mut st, k, v);
    }
    if a.vector {
        set(&mut st, "detail", a.detail);
    }
    for (k, v) in [("flip", a.flip), ("raised_bridges", a.raised_bridges)] {
        if v {
            st.insert(k.into(), json!(true));
        }
    }
    if let Some(z) = &a.z_bridging {
        st.insert("z_bridging".into(), json!(z));
    }

    let mut ex = Map::new();
    for (k, v) in [
        ("max_x", a.tile_width),
        ("max_y", a.tile_height),
        ("clearance", a.connector_clearance),
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
    nodes.push(json!({"id": "ex", "type": "export", "params": ex}));
    edges.push(json!({"from": ["st", "solid"], "to": ["ex", "solid"]}));
    if let Some(screw) = &a.hardware {
        let mut hw = json!({"screw": screw});
        if let Some(c) = &a.connector {
            hw["connector"] = json!(c);
        }
        if let Some(d) = &a.connector_d {
            hw["connector_d"] = json!(d);
        }
        nodes.push(json!({"id": "hw", "type": "hardware", "params": hw}));
    }
    json!({"nodes": nodes, "edges": edges})
}

fn print_report(r: &Json) {
    eprintln!("print readiness: {}", r["verdict"].as_str().unwrap_or(""));
    for f in r["findings"].as_array().into_iter().flatten() {
        eprintln!(
            "  [{}] {}: {}",
            f["level"].as_str().unwrap_or(""),
            f["label"].as_str().unwrap_or(""),
            f["text"].as_str().unwrap_or("")
        );
    }
    eprintln!("slowest nodes:");
    for t in r["timing"].as_array().into_iter().flatten().take(5) {
        let when = if t["cached"] == true {
            "cached".to_string()
        } else {
            format!("{} ms", t["ms"].as_f64().unwrap_or(0.0).round())
        };
        eprintln!("  {}: {when}", t["label"].as_str().unwrap_or(""));
    }
}

/// Applies `NODE.PARAM=VALUE` settings to the graph, refusing anything the node does not have or accept.
fn apply_sets(graph: &mut Json, sets: &[String]) {
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
fn load_project(engine: &mut Engine, path: &Path) -> (Json, String, PathBuf) {
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
        json!({"nodes": nodes, "edges": edges}),
        stem,
        path.parent().map(Path::to_path_buf).unwrap_or_default(),
    )
}

fn write(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes)
        .unwrap_or_else(|e| fail(&format!("cannot write {}: {e}", path.display())));
}

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}
