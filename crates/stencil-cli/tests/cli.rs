use std::{fs, path::PathBuf, process::Command};

const RING: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="120"><rect width="120" height="120" fill="#fff"/><circle cx="60" cy="60" r="48" fill="#000"/><circle cx="60" cy="60" r="28" fill="#fff"/><circle cx="60" cy="60" r="12" fill="#000"/></svg>"##;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_stencil"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("stencil-cli-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// A binary STL is an 80 byte header, a u32 count and 50 bytes per triangle.
fn assert_stl(path: &std::path::Path) {
    let b = fs::read(path).unwrap();
    assert!(b.len() > 84, "{} is empty", path.display());
    let n = u32::from_le_bytes(b[80..84].try_into().unwrap()) as usize;
    assert!(n > 0);
    assert_eq!(
        b.len(),
        84 + 50 * n,
        "{} is not a well-formed STL",
        path.display()
    );
}

#[test]
fn raster_svg_makes_an_stl() {
    let d = scratch("raster");
    let input = d.join("ring.svg");
    fs::write(&input, RING).unwrap();
    let out = d.join("ring.stl");
    let r = bin()
        .args([
            input.to_str().unwrap(),
            "--width",
            "60",
            "--resolution",
            "200",
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert_stl(&out);
}

#[test]
fn vector_mode_makes_an_island_free_stl() {
    let d = scratch("vector");
    let input = d.join("ring.svg");
    fs::write(&input, RING).unwrap();
    let out = d.join("ring.stl");
    let r = bin()
        .args([
            input.to_str().unwrap(),
            "--vector",
            "--detail",
            "2",
            "--width",
            "60",
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert_stl(&out);
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(stderr.contains("0 remaining, watertight true"), "{stderr}");
}

#[test]
fn hardware_writes_parts_and_a_zip() {
    let d = scratch("hardware");
    let input = d.join("ring.svg");
    fs::write(&input, RING).unwrap();
    let out = d.join("parts");
    let r = bin()
        .args([
            input.to_str().unwrap(),
            "--width",
            "60",
            "--hardware",
            "M5",
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let stls: Vec<_> = fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert!(stls.len() > 1);
    for p in &stls {
        assert_stl(p);
    }
    assert!(d.join("parts.zip").exists());
}

#[test]
fn runs_a_saved_project_in_studio_format() {
    use base64::Engine as _;
    let d = scratch("project");
    let id = stencil_core::graph::Engine::new()
        .add_image(RING.as_bytes().to_vec())
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let b64 = base64::engine::general_purpose::STANDARD.encode(RING);
    let node = |nid: &str, ty: &str, params: &str| {
        format!(r#"{{"id":"{nid}","type":"studio","data":{{"type":"{ty}","params":{params}}}}}"#)
    };
    let edge = |a: &str, ao: &str, b: &str, bi: &str| {
        format!(r#"{{"source":"{a}","sourceHandle":"{ao}","target":"{b}","targetHandle":"{bi}"}}"#)
    };
    let nodes = [
        node(
            "s",
            "source",
            &format!(r#"{{"image_id":"{id}","resolution":160}}"#),
        ),
        node("t", "threshold", "{}"),
        node("tr", "trace", "{}"),
        node("v", "vector_stencil", r#"{"width_mm":60,"detail":2}"#),
    ]
    .join(",");
    let edges = [
        edge("s", "image", "t", "image"),
        edge("t", "mask", "tr", "mask"),
        edge("tr", "shapes", "v", "shapes"),
    ]
    .join(",");
    let project = format!(
        r#"{{"version":2,"images":{{"{id}":{{"name":"ring.svg","base64":"{b64}"}}}},"graph":{{"nodes":[{nodes}],"edges":[{edges}]}}}}"#
    );
    let path = d.join("ring.stencil.json");
    fs::write(&path, project).unwrap();
    let out = d.join("project.stl");
    let r = bin()
        .args([
            "--project",
            path.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert_stl(&out);
    assert!(String::from_utf8_lossy(&r.stderr).contains("0 remaining, watertight true"));
}

fn tiled_project(d: &std::path::Path) -> PathBuf {
    use base64::Engine as _;
    let id = stencil_core::graph::Engine::new()
        .add_image(RING.as_bytes().to_vec())
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let b64 = base64::engine::general_purpose::STANDARD.encode(RING);
    let node = |nid: &str, ty: &str, params: &str| {
        format!(r#"{{"id":"{nid}","type":"studio","data":{{"type":"{ty}","params":{params}}}}}"#)
    };
    let edge = |a: &str, ao: &str, b: &str, bi: &str| {
        format!(r#"{{"source":"{a}","sourceHandle":"{ao}","target":"{b}","targetHandle":"{bi}"}}"#)
    };
    let nodes = [
        node(
            "s",
            "source",
            &format!(r#"{{"image_id":"{id}","resolution":160}}"#),
        ),
        node("t", "threshold", "{}"),
        node("tr", "trace", "{}"),
        node(
            "lay",
            "tile_layout",
            r#"{"width_mm":100,"margin_mm":6,"max_x":60,"max_y":60}"#,
        ),
        node("v", "vector_stencil", r#"{"detail":2,"thickness_mm":6}"#),
    ]
    .join(",");
    let edges = [
        edge("s", "image", "t", "image"),
        edge("t", "mask", "tr", "mask"),
        edge("tr", "shapes", "lay", "shapes"),
        edge("tr", "shapes", "v", "shapes"),
        edge("lay", "tiles", "v", "tiles"),
    ]
    .join(",");
    let path = d.join("tiled.stencil.json");
    fs::write(
        &path,
        format!(
            r#"{{"version":2,"images":{{"{id}":{{"name":"ring.svg","base64":"{b64}"}}}},"graph":{{"nodes":[{nodes}],"edges":[{edges}]}}}}"#
        ),
    )
    .unwrap();
    path
}

#[test]
fn overrides_layout_and_report_flags_work() {
    let d = scratch("flags");
    let path = tiled_project(&d);
    let layout = d.join("layout.png");
    let report = d.join("report.json");
    let out = d.join("tiled.stl");
    let r = bin()
        .args(["--project", path.to_str().unwrap(), "-o"])
        .arg(&out)
        .args([
            "--set",
            "lay.max_x=50",
            "--set",
            "vector_stencil.bridge_mm=2.2",
        ])
        .arg("--layout")
        .arg(&layout)
        .arg("--report")
        .arg("--report-file")
        .arg(&report)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    // several parts are written as files inside a folder with the output's name
    let parts: Vec<_> = fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert!(parts.len() > 1);
    parts.iter().for_each(|p| assert_stl(p));
    assert!(fs::read(&layout).unwrap().starts_with(b"\x89PNG"));
    let json: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert!(["ready", "check", "blocked"].contains(&json["verdict"].as_str().unwrap()));
    assert!(json["timing"].as_array().is_some_and(|t| !t.is_empty()));
    assert!(String::from_utf8_lossy(&r.stderr).contains("print readiness:"));
    let strict = bin()
        .args(["--project", path.to_str().unwrap(), "--strict"])
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(
        strict.status.success(),
        json["verdict"] == "ready",
        "{}",
        String::from_utf8_lossy(&strict.stderr)
    );
}

#[test]
fn bad_overrides_are_rejected() {
    let d = scratch("badset");
    let path = tiled_project(&d);
    for set in [
        "lay.nope=1",
        "ghost.max_x=1",
        "lay.max_x=abc",
        "lay.max_x=100000",
        "lay.max_x",
    ] {
        let r = bin()
            .args(["--project", path.to_str().unwrap(), "--set", set])
            .output()
            .unwrap();
        assert!(!r.status.success(), "{set} should fail");
    }
}

#[test]
fn a_graph_without_output_is_reported() {
    let d = scratch("empty");
    let path = d.join("empty.stencil.json");
    fs::write(
        &path,
        r#"{"version":2,"images":{},"graph":{"nodes":[],"edges":[]}}"#,
    )
    .unwrap();
    let r = bin()
        .args(["--project", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("no STL"));
}

#[test]
fn bad_input_fails_with_a_message() {
    let r = bin().arg("/definitely/not/here.png").output().unwrap();
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("cannot read"));
    let r = bin().output().unwrap();
    assert!(!r.status.success());
    let r = bin()
        .args(["x.png", "--z-bridging", "nonsense"])
        .output()
        .unwrap();
    assert!(!r.status.success());
}
