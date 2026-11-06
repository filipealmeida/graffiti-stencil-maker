use crate::{args::Args, fail, fonts, graph};
use serde_json::Value as Json;
use std::{fs, path::PathBuf, time::Instant};
use stencil_core::graph::Engine;

pub(crate) fn run(a: &Args, engine: &mut Engine, graph: &mut Json, t: Instant) -> Json {
    if let Some(pic) = &a.source {
        graph::use_picture(engine, graph, pic);
    }
    graph::apply_sets(graph, &a.set);
    fonts::prepare(engine, graph, &a.font, &a.font_dir);
    let result = engine.run(graph, None, &|| t.elapsed().as_secs_f64() * 1000.0);
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

    // a stencil leaves its islands for a Bridges node: report the plates as they end up
    let has_bridges = graph["nodes"]
        .as_array()
        .is_some_and(|ns| ns.iter().any(|n| n["type"] == "bridges"));
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
            .filter(|_| match n["type"].as_str() {
                Some("bridges") => true,
                Some("stencil" | "vector_stencil") => !has_bridges,
                _ => false,
            })
        {
            if a.project.is_none() && st["islands_remaining"].as_u64().unwrap_or(0) > 0 {
                fail("islands are left that no bridge could reach: use a wider --bridge or --raised-bridges");
            }
            eprintln!(
                "verified: {} islands found, {} remaining, watertight {}, {} triangles",
                st["islands_found"], st["islands_remaining"], st["watertight"], st["triangles"]
            );
        }
    }
    result
}

/// A number from the report's metrics for the summary table, or a dash when the run has none.
fn metric(report: &Json, key: &str) -> String {
    report["metrics"][key]
        .as_f64()
        .map_or("-".into(), |x| format!("{x:.1}"))
}

/// The arguments of this run without the ones a single child run must not get.
fn child_args(argv: &[String]) -> Vec<String> {
    let with_value = [
        "--batch",
        "--sweep",
        "-o",
        "--output",
        "--report-file",
        "--layout",
        "--source",
    ];
    let mut out = Vec::new();
    let mut skip = false;
    for x in &argv[1..] {
        if skip {
            skip = false;
        } else if with_value.contains(&x.as_str()) {
            skip = true;
        } else if !with_value.iter().any(|f| x.starts_with(&format!("{f}="))) {
            out.push(x.clone());
        }
    }
    out
}

/// --batch and --sweep: one child run per picture and value, each in its own folder, then a summary.
pub(crate) fn run_many(a: &Args, argv: &[String]) {
    let mut sweeps: Vec<(String, Vec<String>)> = Vec::new();
    for s in &a.sweep {
        let (target, values) = s
            .split_once('=')
            .unwrap_or_else(|| fail(&format!("--sweep {s}: write NODE.PARAM=a,b,c")));
        let values: Vec<String> = values.split(',').map(|v| v.trim().to_string()).collect();
        if values.iter().any(String::is_empty) {
            fail(&format!("--sweep {s}: empty value"));
        }
        sweeps.push((target.to_string(), values));
    }
    let mut pictures: Vec<Option<PathBuf>> = vec![None];
    if let Some(dir) = &a.batch {
        let mut found: Vec<PathBuf> = fs::read_dir(dir)
            .unwrap_or_else(|e| fail(&format!("cannot read {}: {e}", dir.display())))
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension().and_then(|x| x.to_str()).is_some_and(|x| {
                    matches!(
                        x.to_lowercase().as_str(),
                        "png" | "jpg" | "jpeg" | "bmp" | "svg"
                    )
                })
            })
            .collect();
        found.sort();
        if found.is_empty() {
            fail(&format!("--batch: no pictures in {}", dir.display()));
        }
        pictures = found.into_iter().map(Some).collect();
    }
    let mut combos: Vec<Vec<(String, String)>> = vec![Vec::new()];
    for (target, values) in &sweeps {
        combos = combos
            .into_iter()
            .flat_map(|c| {
                values.iter().map(move |v| {
                    let mut c = c.clone();
                    c.push((target.clone(), v.clone()));
                    c
                })
            })
            .collect();
    }
    let root = a.output.clone().unwrap_or_else(|| {
        PathBuf::from(format!(
            "{}_runs",
            a.project
                .as_ref()
                .or(a.input.as_ref())
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "stencil".into())
        ))
    });
    let exe = std::env::current_exe().unwrap_or_else(|e| fail(&format!("cannot find myself: {e}")));
    let base = child_args(argv);
    let mut rows =
        vec!["run,status,verdict,findings,seam_step_mm,paint_under_blocks_mm2,seconds".to_string()];
    let mut failed = 0;
    let mut fewest_steps: Option<(f64, String)> = None;
    for pic in &pictures {
        for combo in &combos {
            let mut name: Vec<String> = Vec::new();
            if let Some(p) = pic {
                name.push(
                    p.file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default(),
                );
            }
            for (t, v) in combo {
                name.push(format!("{}={v}", t.rsplit('.').next().unwrap_or(t)));
            }
            let name = name.join("_").replace(['/', '\\'], "-");
            let name = if name.is_empty() {
                "run".to_string()
            } else {
                name
            };
            let dir = root.join(&name);
            fs::create_dir_all(&dir)
                .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", dir.display())));
            let mut cmd = std::process::Command::new(&exe);
            cmd.args(&base)
                .arg("-o")
                .arg(&dir)
                .arg("--report-file")
                .arg(dir.join("report.json"));
            if let Some(p) = pic {
                cmd.arg("--source").arg(p);
            }
            for (t, v) in combo {
                cmd.arg("--set").arg(format!("{t}={v}"));
            }
            eprintln!("== {name}");
            let t = Instant::now();
            let ok = cmd
                .stdout(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            let report: Json = fs::read_to_string(dir.join("report.json"))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or(Json::Null);
            if !ok {
                failed += 1;
            }
            if let Some(s) = report["metrics"]["seam_step_mm"].as_f64().filter(|_| ok) {
                if fewest_steps.as_ref().is_none_or(|b| s < b.0) {
                    fewest_steps = Some((s, name.clone()));
                }
            }
            rows.push(format!(
                "{name},{},{},{},{},{},{:.1}",
                if ok { "ok" } else { "failed" },
                report["verdict"].as_str().unwrap_or("-"),
                report["findings"].as_array().map_or(0, Vec::len),
                metric(&report, "seam_step_mm"),
                metric(&report, "paint_under_blocks_mm2"),
                t.elapsed().as_secs_f64()
            ));
        }
    }
    let csv = root.join("summary.csv");
    crate::output::write(&csv, rows.join("\n").as_bytes());
    for r in &rows {
        eprintln!("{}", r.replace(',', "  "));
    }
    eprintln!("wrote {}", csv.display());
    if let Some((s, name)) = fewest_steps.filter(|_| rows.len() > 2) {
        eprintln!("fewest seam steps: {name} ({s:.1} mm)");
    }
    if failed > 0 {
        fail(&format!("{failed} of {} runs failed", rows.len() - 1));
    }
}
