use super::*;
use crate::mesh::Mesh;

pub(super) fn read(bytes: &[u8], format: &str) -> Result<Mesh, String> {
    let mut m = match format {
        "stl"
            if bytes.len() >= 84
                && (bytes.len() - 84) / 50
                    == u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize
                && (bytes.len() - 84) % 50 == 0 =>
        {
            Mesh::from_stl(bytes)?
        }
        "stl" => ascii_stl(bytes)?,
        "obj" => obj(bytes)?,
        _ => return Err("unsupported model format: use STL or triangulated OBJ".into()),
    };
    if m.t.is_empty() {
        return Err("the model contains no triangles".into());
    }
    if m.v
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || !(*v as f32).is_finite())
    {
        return Err("the model has non-finite or out-of-range coordinates".into());
    }
    // Weld the coordinates the downstream binary STL will actually store, including signed zero.
    for v in m.v.iter_mut().flatten() {
        if *v == 0.0 {
            *v = 0.0;
        }
    }
    let triangles = m.t.len();
    m.quantize_f32();
    if m.t.len() != triangles
        || m.t.iter().any(|t| {
            let [a, b, c] = t.map(|i| m.v[i]);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ]
            .iter()
            .all(|&x| x == 0.0)
        })
    {
        return Err(
            "the model has degenerate or duplicate opposing triangles: repair it before importing"
                .into(),
        );
    }
    if !m.is_watertight() {
        return Err(
            "the model is not watertight or consistently wound: repair the mesh before importing"
                .into(),
        );
    }
    if m.volume() == 0.0 {
        return Err("the model encloses no volume".into());
    }
    if m.volume() < 0.0 {
        for t in &mut m.t {
            t.swap(1, 2);
        }
    }
    Ok(m)
}

fn text(bytes: &[u8]) -> Result<&str, String> {
    std::str::from_utf8(bytes).map_err(|_| "not a valid binary STL, ASCII STL or OBJ model".into())
}

fn point(words: &mut std::str::SplitWhitespace<'_>) -> Result<[f64; 3], String> {
    let mut p = [0.0; 3];
    for x in &mut p {
        *x = words
            .next()
            .ok_or("missing vertex coordinate")?
            .parse()
            .map_err(|_| "invalid vertex coordinate")?;
    }
    Ok(p)
}

fn ascii_stl(bytes: &[u8]) -> Result<Mesh, String> {
    let s = text(bytes)?;
    let mut m = Mesh::default();
    let mut facet = false;
    let mut vertices = 0;
    if !s.trim_start().starts_with("solid") {
        return Err("not a valid STL: expected binary STL or an ASCII solid".into());
    }
    for line in s.lines() {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("facet") if !facet => {
                facet = true;
                vertices = 0;
            }
            Some("vertex") if facet && vertices < 3 => {
                m.v.push(point(&mut words)?);
                vertices += 1;
            }
            Some("endfacet") if facet && vertices == 3 => {
                let n = m.v.len();
                m.t.push([n - 3, n - 2, n - 1]);
                facet = false;
            }
            Some("solid" | "endsolid" | "outer" | "endloop") | None => {}
            _ => return Err("invalid or truncated ASCII STL facet".into()),
        }
    }
    if facet {
        return Err("truncated ASCII STL facet".into());
    }
    Ok(m)
}

fn obj(bytes: &[u8]) -> Result<Mesh, String> {
    let mut m = Mesh::default();
    for line in text(bytes)?.lines() {
        let mut words = line.split('#').next().unwrap_or("").split_whitespace();
        match words.next() {
            Some("v") => {
                let p = point(&mut words)?;
                if words.next().is_some() {
                    return Err("OBJ vertices must have exactly three coordinates".into());
                }
                m.v.push(p);
            }
            Some("f") => {
                let mut t = [0; 3];
                for i in &mut t {
                    let word = words.next().ok_or("OBJ faces must have three vertices")?;
                    let n: i64 = word
                        .split('/')
                        .next()
                        .unwrap_or("")
                        .parse()
                        .map_err(|_| "invalid OBJ vertex index")?;
                    let index = if n < 0 { m.v.len() as i64 + n } else { n - 1 };
                    if n == 0 || index < 0 || index >= m.v.len() as i64 {
                        return Err("OBJ vertex index is out of range".into());
                    }
                    *i = index as usize;
                }
                if words.next().is_some() {
                    return Err("OBJ polygon faces are not supported: triangulate the model before importing".into());
                }
                m.t.push(t);
            }
            Some("vt" | "vn" | "o" | "g" | "s" | "usemtl" | "mtllib") | None => {}
            _ => return Err("unsupported OBJ record: export a triangulated surface mesh".into()),
        }
    }
    let mut used = vec![false; m.v.len()];
    for &i in m.t.iter().flatten() {
        used[i] = true;
    }
    let mut remap = vec![0; m.v.len()];
    let mut next = 0;
    m.v =
        m.v.into_iter()
            .enumerate()
            .filter_map(|(i, v)| {
                if !used[i] {
                    return None;
                }
                remap[i] = next;
                next += 1;
                Some(v)
            })
            .collect();
    for t in &mut m.t {
        *t = t.map(|i| remap[i]);
    }
    Ok(m)
}

pub(super) fn run(e: &Engine, _: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    let id = p.s("model_id");
    if id.is_empty() {
        return Err("no 3D model loaded: open an STL or triangulated OBJ in this node".into());
    }
    let bytes = e
        .uploads
        .get(id)
        .ok_or("the model is no longer loaded: open it again")?;
    let mut m = read(bytes, p.s("format"))?;
    let (lo, _) = m.bounds();
    for v in &mut m.v {
        for k in 0..3 {
            v[k] = (v[k] - lo[k]) * p.f("scale");
        }
    }
    // Revalidate after scaling and conversion to STL precision.
    m = read(&m.to_stl(), "stl")?;
    let (lo, hi) = m.bounds();
    let size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    let layer = p.f("layer_height_mm");
    let margin = p.f("margin_mm");
    if margin * 2.0 > size[0].min(size[1]) {
        return Err(
            "the existing border width cannot exceed half the model's width or height".into(),
        );
    }
    let stl = m.to_stl();
    let stats = json!({
        "size_mm": size, "width_mm": size[0], "height_mm": size[1],
        "thickness_mm": size[2], "plate_top_mm": hi[2], "layer_height_mm": layer,
        "layers": (size[2] / layer).ceil() as u64,
        "margin_mm": margin, "flip": false, "watertight": true,
        "triangles": m.t.len(), "stl_bytes": stl.len(),
        "volume_mm3": m.volume(),
        "islands_remaining": crate::tile_bridge::components(&m).0.saturating_sub(1),
    });
    Ok(vec![("solid", Value::Solid(Rc::new(Solid { stl, stats })))])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube() -> Mesh {
        Mesh::cuboid([-10.0, -5.0, -2.0], [30.0, 15.0, 0.0])
    }

    fn ascii(m: &Mesh) -> Vec<u8> {
        let mut s = String::from("solid test\n");
        for t in &m.t {
            s.push_str("facet normal 0 0 0\nouter loop\n");
            for &i in t {
                let [x, y, z] = m.v[i];
                s.push_str(&format!("vertex {x} {y} {z}\n"));
            }
            s.push_str("endloop\nendfacet\n");
        }
        s.push_str("endsolid test\n");
        s.into_bytes()
    }

    #[test]
    fn reads_binary_and_ascii_stl_including_solid_binary_header_and_signed_zero() {
        let m = cube();
        let mut binary = m.to_stl();
        binary[..5].copy_from_slice(b"solid");
        let text = String::from_utf8(ascii(&m))
            .unwrap()
            .replace(" 0\n", " -0\n");
        for bytes in [binary, text.into_bytes()] {
            let r = read(&bytes, "stl").unwrap();
            assert!(r.is_watertight());
            assert_eq!(r.t.len(), 12);
            assert!((r.volume() - 1600.0).abs() < 1e-6);
        }
    }

    #[test]
    fn reads_obj_indices_and_ignores_materials() {
        let m = cube();
        for negative in [false, true] {
            let mut s = String::from("# test\nmtllib unused.mtl\nusemtl ignored\n");
            for [x, y, z] in &m.v {
                s.push_str(&format!("v {x} {y} {z}\n"));
            }
            for t in &m.t {
                let [a, b, c] = t.map(|i| {
                    if negative {
                        i as i64 - m.v.len() as i64
                    } else {
                        i as i64 + 1
                    }
                });
                s.push_str(&format!("f {a}/1/1 {b}//1 {c} # face\n"));
            }
            s.push_str("v 10000 10000 10000\n");
            let r = read(s.as_bytes(), "obj").unwrap();
            assert!(r.is_watertight());
            assert!((r.volume() - m.volume()).abs() < 1e-6);
            assert_eq!(r.bounds(), m.bounds());
        }
    }

    #[test]
    fn rejects_invalid_meshes_without_panicking() {
        let mut open = cube();
        open.t.pop();
        let mut nan = cube();
        nan.v[0][0] = f64::NAN;
        let mut degenerate = cube();
        degenerate.t.push([0, 0, 1]);
        let mut bad_winding = cube();
        bad_winding.t[0].swap(1, 2);
        let mut truncated = cube().to_stl();
        truncated.pop();
        let mut excessive_count = cube().to_stl();
        excessive_count[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
        for bytes in [
            open.to_stl(),
            nan.to_stl(),
            degenerate.to_stl(),
            bad_winding.to_stl(),
            truncated,
            excessive_count,
            vec![],
            vec![0; 84],
            b"solid x\nfacet normal 0 0 0\nvertex 0 0 0".to_vec(),
        ] {
            assert!(read(&bytes, "stl").is_err());
        }
        for s in [
            "v 0 0 0\nf 0 1 1",
            "v 0 0 0\nf 1 2 3",
            "v 0 0 0\nf -2 -1 -1",
            "v 0 0 0\nf 1 1 1 1",
        ] {
            assert!(read(s.as_bytes(), "obj").is_err(), "{s}");
        }
        assert!(read(&cube().to_stl(), "3mf").is_err());
    }

    #[test]
    fn corrects_globally_inverted_winding() {
        let mut m = cube();
        for t in &mut m.t {
            t.swap(1, 2);
        }
        let r = read(&m.to_stl(), "stl").unwrap();
        assert!(r.is_watertight());
        assert!((r.volume() - 1600.0).abs() < 1e-6);
    }

    #[test]
    fn import_is_a_cached_solid_with_printing_metadata() {
        let mut e = Engine::new();
        let id = e.add_model(cube().to_stl(), "stl").unwrap()["id"].clone();
        let g = json!({"nodes": [{"id": "m", "type": "mesh_import", "params": {
            "model_id": id, "scale": 2, "layer_height_mm": 0.25
        }}], "edges": []});
        let r = e.run(&g, None, &|| 0.0);
        assert_eq!(r["nodes"]["m"]["state"], "done", "{r}");
        let s = e
            .value(r["nodes"]["m"]["key"].as_str().unwrap(), "solid")
            .unwrap();
        let Value::Solid(s) = s else { panic!() };
        assert_eq!(s.stats["size_mm"], json!([80.0, 40.0, 4.0]));
        assert_eq!(s.stats["plate_top_mm"], 4.0);
        assert_eq!(s.stats["layers"], 16);
        assert_eq!(s.stats["islands_remaining"], 0);
        let m = Mesh::from_stl(&s.stl).unwrap();
        assert_eq!(m.bounds(), ([0.0; 3], [80.0, 40.0, 4.0]));
        assert!((m.volume() - 12800.0).abs() < 1e-6);
        let r = e.run(&g, None, &|| 0.0);
        assert_eq!(r["nodes"]["m"]["cached"], true, "{r}");
        for params in [json!({}), json!({"model_id": "missing"})] {
            let g = json!({"nodes": [{"id": "m", "type": "mesh_import", "params": params}], "edges": []});
            let r = e.run(&g, None, &|| 0.0);
            assert_eq!(r["nodes"]["m"]["state"], "error", "{r}");
        }
    }

    #[test]
    fn imported_solids_feed_bridges_split_volume_and_fragment_workflows() {
        let mut m = cube();
        m.append(&Mesh::cuboid([31.0, 0.0, -2.0], [35.0, 4.0, 0.0]));
        let mut e = Engine::new();
        let id = e.add_model(m.to_stl(), "stl").unwrap()["id"].clone();
        let g = json!({
            "nodes": [
                {"id": "m", "type": "mesh_import", "params": {"model_id": id}},
                {"id": "b", "type": "bridges", "params": {"raised_bridges": false}},
                {"id": "t", "type": "tiles", "params": {"max_x": 25, "max_y": 0, "connector": "none", "depth": 4}},
                {"id": "tb", "type": "bridges", "params": {}},
                {"id": "f", "type": "fragments", "params": {}}
            ],
            "edges": [
                {"from": ["m", "solid"], "to": ["b", "body"]},
                {"from": ["b", "solid"], "to": ["t", "solid"]},
                {"from": ["t", "parts"], "to": ["tb", "body"]},
                {"from": ["tb", "parts"], "to": ["f", "parts"]}
            ]
        });
        let r = e.run(&g, None, &|| 0.0);
        for id in ["m", "b", "t", "tb", "f"] {
            assert_eq!(r["nodes"][id]["state"], "done", "{id}: {r}");
        }
        let Value::Solid(s) = e
            .value(r["nodes"]["b"]["key"].as_str().unwrap(), "solid")
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(s.stats["islands_found"], 1);
        assert_eq!(s.stats["islands_remaining"], 0);
        let Value::Parts(p) = e
            .value(r["nodes"]["f"]["key"].as_str().unwrap(), "parts")
            .unwrap()
        else {
            panic!()
        };
        assert!(p.files.len() >= 2);
        for (_, bytes) in &p.files {
            assert!(Mesh::from_stl(bytes).unwrap().is_watertight());
        }
    }

    #[test]
    fn imported_volume_supports_simplification_and_connector_pockets() {
        let m = Mesh::cuboid([-50.0, -30.0, -8.0], [50.0, 30.0, 0.0]);
        let mut e = Engine::new();
        let id = e.add_model(m.to_stl(), "stl").unwrap()["id"].clone();
        let g = json!({
            "nodes": [
                {"id": "m", "type": "mesh_import", "params": {"model_id": id, "margin_mm": 12}},
                {"id": "r", "type": "remesh", "params": {}},
                {"id": "t", "type": "tiles", "params": {"max_x": 60, "max_y": 100}},
                {"id": "p", "type": "pads", "params": {"pad_thread": "none"}}
            ],
            "edges": [
                {"from": ["m", "solid"], "to": ["r", "solid"]},
                {"from": ["r", "solid"], "to": ["t", "solid"]},
                {"from": ["t", "parts"], "to": ["p", "parts"]}
            ]
        });
        let r = e.run(&g, None, &|| 0.0);
        for id in ["m", "r", "t", "p"] {
            assert_eq!(r["nodes"][id]["state"], "done", "{id}: {r}");
        }
        let Value::Parts(p) = e
            .value(r["nodes"]["p"]["key"].as_str().unwrap(), "parts")
            .unwrap()
        else {
            panic!()
        };
        assert!(p.meta["plate"]["connectors"].as_array().unwrap().len() > 0);
        assert!(p
            .files
            .iter()
            .any(|(name, _)| name.starts_with("connector_")));
        for (_, bytes) in &p.files {
            assert!(Mesh::from_stl(bytes).unwrap().is_watertight());
        }
    }
}
