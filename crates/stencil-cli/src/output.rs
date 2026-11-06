use crate::{args::Args, fail};
use serde_json::Value as Json;
use std::{collections::HashSet, fs, path::Path, time::Instant};
use stencil_core::graph::Engine;

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

/// Prints every parameter --set accepts, for all node types or the one named.
pub(crate) fn list_params(only: &str, format: &str) {
    if !matches!(format, "text" | "markdown") {
        fail(&format!(
            "--params-format: '{format}' is not text or markdown"
        ));
    }
    let markdown = format == "markdown";
    let types = stencil_core::graph::node_types();
    let mut any = false;
    for t in types.as_array().into_iter().flatten() {
        let id = t["id"].as_str().unwrap_or("");
        if !only.is_empty() && id != only {
            continue;
        }
        if markdown && any {
            println!();
        }
        any = true;
        if markdown {
            println!("### {} (`{id}`)\n", t["label"].as_str().unwrap_or(""));
            println!(
                "| Parameter | Label | Type | Default | Range or choices |\n|---|---|---|---|---|"
            );
        } else {
            println!("{id}  ({})", t["label"].as_str().unwrap_or(""));
        }
        for p in t["params"].as_array().into_iter().flatten() {
            let range = match p["kind"].as_str().unwrap_or("") {
                "int" | "float" => format!("{} to {}", p["min"], p["max"]),
                "choice" => p["choices"]
                    .as_array()
                    .map(|c| {
                        c.iter()
                            .filter_map(Json::as_str)
                            .collect::<Vec<_>>()
                            .join(" | ")
                    })
                    .unwrap_or_default(),
                "bool" => "true | false".into(),
                _ => "text".into(),
            };
            if markdown {
                let cell = |s: &str| s.replace('|', "\\|");
                println!(
                    "| `{}` | {} | {} | `{}` | {} |",
                    p["name"].as_str().unwrap_or(""),
                    cell(p["label"].as_str().unwrap_or("")),
                    p["kind"].as_str().unwrap_or(""),
                    p["default"],
                    cell(&range)
                );
            } else {
                println!(
                    "  {}={}  default {}  [{}]",
                    p["name"].as_str().unwrap_or(""),
                    p["kind"].as_str().unwrap_or(""),
                    p["default"],
                    range
                );
            }
        }
    }
    if !any {
        fail(&format!("--list-params: no node type '{only}'"));
    }
}

/// --tile-drawings: every tile of every Tile plan as `NAME.svg` and `NAME.dxf` (with the node id when there are several plans).
fn write_tile_drawings(engine: &Engine, graph: &Json, result: &Json, dir: &Path) -> usize {
    let plans: Vec<(String, &stencil_core::graph::Value)> = graph["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| {
            let id = n["id"].as_str()?;
            let key = result["nodes"][id]["key"].as_str()?;
            Some((id.to_string(), engine.value(key, "tiles")?))
        })
        .collect();
    if plans.is_empty() {
        fail("--tile-drawings: the graph has no Tile plan node");
    }
    fs::create_dir_all(dir)
        .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", dir.display())));
    let many = plans.len() > 1;
    let mut n = 0;
    for (id, v) in plans {
        for name in v.info()["tile_files"].as_array().into_iter().flatten() {
            let name = name.as_str().unwrap_or("");
            let file = if many {
                format!("{id}-{name}")
            } else {
                name.to_string()
            };
            write(&dir.join(file), &v.part(name).unwrap_or_default());
            n += 1;
        }
    }
    eprintln!("wrote {n} tile drawings to {}", dir.display());
    1
}

/// --cut-sheets: every part and sheet of every Cut sheet as `NAME.svg` and `NAME.dxf` (with the node id when there are several).
fn write_cut_sheets(engine: &Engine, graph: &Json, result: &Json, dir: &Path) -> usize {
    let cuts: Vec<(String, &stencil_core::graph::Value)> = graph["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| {
            let id = n["id"].as_str()?;
            let key = result["nodes"][id]["key"].as_str()?;
            Some((id.to_string(), engine.value(key, "cuts")?))
        })
        .collect();
    if cuts.is_empty() {
        fail("--cut-sheets: the graph has no Cut sheet node");
    }
    fs::create_dir_all(dir)
        .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", dir.display())));
    let many = cuts.len() > 1;
    let mut n = 0;
    for (id, v) in cuts {
        for name in v.info()["cut_files"].as_array().into_iter().flatten() {
            let name = name.as_str().unwrap_or("");
            let file = if many {
                format!("{id}-{name}")
            } else {
                name.to_string()
            };
            write(&dir.join(file), &v.part(name).unwrap_or_default());
            n += 1;
        }
    }
    eprintln!("wrote {n} cut files to {}", dir.display());
    1
}

/// --svg and --dxf: every outline output of the graph, to `base` or, when there are several, `base-NODE-PORT.ext`.
fn write_outlines(engine: &Engine, graph: &Json, result: &Json, base: &Path, ext: &str) -> usize {
    let mut found: Vec<(String, Vec<u8>)> = Vec::new();
    for n in graph["nodes"].as_array().into_iter().flatten() {
        let id = n["id"].as_str().unwrap_or("");
        let Some(key) = result["nodes"][id]["key"].as_str() else {
            continue;
        };
        for (port, meta) in result["nodes"][id]["outputs"]
            .as_object()
            .into_iter()
            .flatten()
        {
            if meta["type"] != "shapes" || meta["available"].as_bool() == Some(false) {
                continue;
            }
            let Some(v) = engine.value(key, port) else {
                continue;
            };
            let bytes = if ext == "dxf" {
                v.dxf()
            } else {
                v.encode().ok().map(|e| e.1)
            };
            found.extend(bytes.map(|b| (format!("{id}-{port}"), b)));
        }
    }
    if found.is_empty() {
        fail(&format!(
            "--{ext}: the graph has no outlines (add a Trace node)"
        ));
    }
    let many = found.len() > 1;
    for (name, bytes) in &found {
        let path = if many {
            let stem = base.file_stem().map(|s| s.to_string_lossy().to_string());
            base.with_file_name(format!(
                "{}-{name}.{ext}",
                stem.unwrap_or_else(|| "outlines".into())
            ))
        } else {
            base.to_path_buf()
        };
        write(&path, bytes);
        eprintln!("wrote the outlines to {}", path.display());
    }
    found.len()
}

pub(crate) fn write(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes)
        .unwrap_or_else(|e| fail(&format!("cannot write {}: {e}", path.display())));
}

pub(crate) fn write_outputs(
    a: &Args,
    engine: &Engine,
    graph: &Json,
    result: &Json,
    stem: &str,
    dir: &Path,
    t: Instant,
) {
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
            let svg = base
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
            let encoded = if svg { v.encode() } else { v.encode_png() };
            found.push((id.to_string(), encoded.map(|e| e.1).unwrap_or_default()));
        }
        if found.is_empty() {
            fail("--layout: the graph has no Tile plan node");
        }
        let many = found.len() > 1;
        let svg = base
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
        for (id, bytes) in found {
            let path = if many {
                let stem = base.file_stem().map(|s| s.to_string_lossy().to_string());
                base.with_file_name(format!(
                    "{}-{id}.{}",
                    stem.unwrap_or_else(|| "layout".into()),
                    if svg { "svg" } else { "png" }
                ))
            } else {
                base.clone()
            };
            write(&path, &bytes);
            eprintln!("wrote the tile plan to {}", path.display());
            layouts += 1;
        }
    }
    if let Some(dir) = &a.tile_drawings {
        layouts += write_tile_drawings(&engine, &graph, &result, dir);
    }
    if let Some(dir) = &a.cut_sheets {
        layouts += write_cut_sheets(&engine, &graph, &result, dir);
    }
    for (flag, ext) in [(&a.svg, "svg"), (&a.dxf, "dxf")] {
        if let Some(base) = flag {
            layouts += write_outlines(&engine, &graph, &result, base, ext);
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
        let path = match &a.output {
            Some(o) if o.is_dir() => o.join(&files[0].0),
            Some(o) => o.clone(),
            None => dir.join(&files[0].0),
        };
        write(&path, &files[0].1);
        eprintln!("wrote {} in {} ms", path.display(), t.elapsed().as_millis());
    } else {
        let out = a
            .output
            .clone()
            .unwrap_or_else(|| dir.join(format!("{stem}_parts")));
        let into_folder = out.is_dir();
        fs::create_dir_all(&out)
            .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", out.display())));
        for (name, bytes) in &files {
            write(&out.join(name), bytes);
        }
        let zip = if into_folder {
            out.join(format!("{stem}.zip"))
        } else {
            out.with_extension("zip")
        };
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
