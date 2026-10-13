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
