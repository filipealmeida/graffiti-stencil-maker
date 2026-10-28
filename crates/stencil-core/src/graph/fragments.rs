//! The Remove fragments node: keeps only the biggest separate solids of each tile.

use super::catalogue::Outputs;
use super::*;
use crate::mesh::Mesh;

/// Volume (mm³) of every connected piece of the mesh, with the piece each triangle belongs to.
fn piece_volumes(m: &Mesh) -> (Vec<f64>, Vec<usize>) {
    let (count, of) = crate::tile_bridge::components(m);
    let mut vol = vec![0.0; count];
    for (t, &o) in m.t.iter().zip(&of) {
        let (a, b, c) = (m.v[t[0]], m.v[t[1]], m.v[t[2]]);
        vol[o] += (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0]))
            / 6.0;
    }
    (vol.into_iter().map(f64::abs).collect(), of)
}

/// The mesh restricted to the pieces for which `keep` is true.
fn keep_pieces(m: &Mesh, of: &[usize], keep: &[bool]) -> Mesh {
    let mut out = Mesh::default();
    let mut map = vec![usize::MAX; m.v.len()];
    for (t, &o) in m.t.iter().zip(of) {
        if !keep[o] {
            continue;
        }
        let mut tri = [0usize; 3];
        for (slot, &vi) in tri.iter_mut().zip(t) {
            if map[vi] == usize::MAX {
                map[vi] = out.v.len();
                out.v.push(m.v[vi]);
            }
            *slot = map[vi];
        }
        out.t.push(tri);
    }
    out
}

pub(super) fn run_fragments(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let Some(Value::Parts(src)) = i.get("parts") else {
        return Err("nothing is connected to the 'parts' input: connect parts".into());
    };
    let n = (p.f("keep") as usize).max(1);
    // with a plate description only its tiles are cleaned; connector and joiner files are left alone
    let tiles: Option<Vec<String>> = src.meta["plate"]["parts"].as_array().map(|a| {
        a.iter()
            .filter(|e| e["role"] == "tile")
            .filter_map(|e| e["file"].as_str().map(str::to_string))
            .collect()
    });
    let mut files = Vec::with_capacity(src.files.len());
    let mut removed = Vec::new();
    let (mut pieces_removed, mut volume_removed, mut tiles_cleaned) = (0usize, 0.0f64, 0usize);
    for (name, data) in &src.files {
        let wanted = match &tiles {
            Some(t) => t.contains(name),
            None => name.ends_with(".stl"),
        };
        if !wanted {
            files.push((name.clone(), data.clone()));
            continue;
        }
        let mesh = Mesh::from_stl(data)?;
        let (vol, of) = piece_volumes(&mesh);
        if vol.len() <= n {
            files.push((name.clone(), data.clone()));
            continue;
        }
        let mut order: Vec<usize> = (0..vol.len()).collect();
        order.sort_by(|&a, &b| vol[b].total_cmp(&vol[a]));
        let mut keep = vec![false; vol.len()];
        for &c in &order[..n] {
            keep[c] = true;
        }
        let gone: f64 = (0..vol.len()).filter(|&c| !keep[c]).map(|c| vol[c]).sum();
        let cleaned = keep_pieces(&mesh, &of, &keep);
        if !cleaned.is_watertight() {
            return Err(format!(
                "verification failed: {name} is not watertight after removing fragments"
            ));
        }
        pieces_removed += vol.len() - n;
        volume_removed += gone;
        tiles_cleaned += 1;
        removed.push(json!({
            "file": name, "pieces": vol.len() - n, "volume_mm3": gone,
            "kept_volume_mm3": order[..n].iter().map(|&c| vol[c]).sum::<f64>(),
        }));
        files.push((name.clone(), cleaned.to_stl()));
    }
    if tiles.as_ref().is_some_and(|t| t.is_empty()) {
        return Err("the parts have no tiles: connect a Split volume or Tile layout node".into());
    }
    let mut meta = src.meta.clone();
    meta["fragments"] = json!({
        "keep": n, "tiles_cleaned": tiles_cleaned, "pieces_removed": pieces_removed,
        "volume_removed_mm3": volume_removed, "removed": removed,
    });
    Ok(vec![(
        "parts",
        Value::Parts(Rc::new(Parts { files, meta })),
    )])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts() -> Parts {
        let mut m = Mesh::cuboid([0.0; 3], [20.0, 20.0, 5.0]);
        m.append(&Mesh::cuboid([30.0, 0.0, 0.0], [34.0, 4.0, 5.0]));
        m.append(&Mesh::cuboid([40.0, 0.0, 0.0], [43.0, 3.0, 5.0]));
        let whole = Mesh::cuboid([0.0; 3], [10.0, 10.0, 5.0]);
        Parts {
            files: vec![
                ("stencil_r1c1.stl".into(), m.to_stl()),
                ("stencil_r1c2.stl".into(), whole.to_stl()),
                ("connector.stl".into(), m.to_stl()),
            ],
            meta: json!({"plate": {"parts": [
                {"file": "stencil_r1c1.stl", "role": "tile"},
                {"file": "stencil_r1c2.stl", "role": "tile"},
            ]}}),
        }
    }

    fn run(keep: f64) -> Rc<Parts> {
        let mut i = HashMap::new();
        i.insert("parts".to_string(), Value::Parts(Rc::new(parts())));
        let params = P(serde_json::from_value(json!({"keep": keep})).unwrap());
        let out = run_fragments(&Engine::new(), &i, &params).unwrap();
        match &out[0].1 {
            Value::Parts(p) => p.clone(),
            _ => panic!(),
        }
    }

    fn pieces(p: &Parts, name: &str) -> usize {
        let f = p.files.iter().find(|f| f.0 == name).unwrap();
        crate::tile_bridge::components(&Mesh::from_stl(&f.1).unwrap()).0
    }

    #[test]
    fn keeps_only_the_biggest_solid_of_a_tile() {
        let p = run(1.0);
        assert_eq!(pieces(&p, "stencil_r1c1.stl"), 1);
        let f = p.files.iter().find(|f| f.0 == "stencil_r1c1.stl").unwrap();
        assert!((Mesh::from_stl(&f.1).unwrap().volume() - 2000.0).abs() < 1e-3);
        assert_eq!(p.meta["fragments"]["pieces_removed"], 2);
        assert_eq!(p.meta["fragments"]["tiles_cleaned"], 1);
        // other files and tiles in one piece are untouched
        assert_eq!(pieces(&p, "connector.stl"), 3);
        assert_eq!(pieces(&p, "stencil_r1c2.stl"), 1);
    }

    #[test]
    fn keeps_the_requested_number_of_fragments() {
        let p = run(2.0);
        assert_eq!(pieces(&p, "stencil_r1c1.stl"), 2);
        assert_eq!(p.meta["fragments"]["pieces_removed"], 1);
        assert_eq!(pieces(&run(5.0), "stencil_r1c1.stl"), 3);
    }
}
