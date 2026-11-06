use crate::mesh::{Mesh, P3};
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

const MARKERS: usize = 2000;
type Edges = BTreeMap<(usize, usize), Vec<(usize, bool)>>;

fn edge_map(m: &Mesh) -> Edges {
    let mut edges = Edges::new();
    for (i, t) in m.t.iter().enumerate() {
        if degenerate(m, t) {
            continue;
        }
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            edges
                .entry((a.min(b), a.max(b)))
                .or_default()
                .push((i, a < b));
        }
    }
    edges
}

fn cross(a: P3, b: P3) -> P3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: P3, b: P3) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
fn degenerate(m: &Mesh, t: &[usize; 3]) -> bool {
    let [a, b, c] = t.map(|i| m.v[i]);
    cross(sub(b, a), sub(c, a)).iter().all(|&x| x == 0.0)
}

/// Exact coordinate indexing does not remove faces or move any surface.
pub(super) fn prepare(mut m: Mesh) -> Result<Mesh, String> {
    if m.t.is_empty() {
        return Err("the model contains no triangles".into());
    }
    let mut ids = HashMap::new();
    let mut vertices = Vec::new();
    let mut remap = Vec::with_capacity(m.v.len());
    for p in &m.v {
        if p.iter().any(|v| !v.is_finite() || !(*v as f32).is_finite()) {
            return Err("the model has non-finite or out-of-range coordinates".into());
        }
        let p = p.map(|x| if x == 0.0 { 0.0 } else { (x as f32) as f64 });
        remap.push(*ids.entry(p.map(f64::to_bits)).or_insert_with(|| {
            vertices.push(p);
            vertices.len() - 1
        }));
    }
    for t in &mut m.t {
        *t = t.map(|i| remap[i]);
    }
    m.v = vertices;
    Ok(m)
}

pub(super) fn inspect(m: &Mesh) -> Json {
    let mut degenerate_count = 0;
    let mut duplicates = 0;
    let mut faces = HashSet::new();
    let mut points = Vec::<P3>::new();
    let mut segments = Vec::<[P3; 2]>::new();
    let mut marker_count = 0;
    for t in &m.t {
        let mut key = *t;
        key.sort_unstable();
        let bad = if degenerate(m, t) {
            degenerate_count += 1;
            true
        } else if !faces.insert(key) {
            duplicates += 1;
            true
        } else {
            false
        };
        if bad {
            marker_count += 1;
            if points.len() < MARKERS {
                points.push(m.v[t[0]]);
            }
        }
    }
    let (mut boundary, mut nonmanifold, mut winding) = (0, 0, 0);
    for ((a, b), uses) in edge_map(m) {
        let bad = match uses.len() {
            1 => {
                boundary += 1;
                true
            }
            2 if uses[0].1 == uses[1].1 => {
                winding += 1;
                true
            }
            2 => false,
            _ => {
                nonmanifold += 1;
                true
            }
        };
        if bad {
            marker_count += 1;
            if segments.len() < MARKERS {
                segments.push([m.v[a], m.v[b]]);
            }
        }
    }
    let volume = m.volume();
    let watertight = boundary == 0
        && nonmanifold == 0
        && winding == 0
        && degenerate_count == 0
        && duplicates == 0;
    let valid = watertight && volume > 0.0;
    let (lo, hi) = m.bounds();
    json!({
        "vertices": m.v.len(), "triangles": m.t.len(), "degenerate": degenerate_count,
        "duplicate_faces": duplicates, "boundary_edges": boundary, "nonmanifold_edges": nonmanifold,
        "winding_edges": winding, "signed_volume_mm3": volume, "inverted": volume < 0.0,
        "watertight": watertight, "valid": valid,
        "size_mm": sub(hi, lo), "bounds": [lo, hi],
        "components": crate::tile_bridge::components(m).0,
        "markers": {"points": points, "segments": segments, "total": marker_count,
            "truncated": marker_count > points.len() + segments.len()},
        "checks": "Edge topology, face degeneracy, duplicates and signed volume. Self-intersections are not checked."
    })
}

pub(super) fn validate(m: &Mesh) -> Result<(), String> {
    let info = inspect(m);
    if info["valid"] == true {
        return Ok(());
    }
    Err(format!(
        "solid output blocked: {} degenerate faces, {} duplicate faces, {} boundary edges, {} non-manifold edges, {} winding conflicts; signed volume {} mm3. Open the 3D import repair tools",
        info["degenerate"], info["duplicate_faces"], info["boundary_edges"],
        info["nonmanifold_edges"], info["winding_edges"], info["signed_volume_mm3"]
    ))
}

pub(super) fn repair(mut m: Mesh, action: &str, tolerance: f64) -> Result<Mesh, String> {
    match action {
        "clean" => {
            let mut seen = HashSet::new();
            let mut counts = HashMap::<[usize; 3], (Vec<[usize; 3]>, Vec<[usize; 3]>)>::new();
            for t in &m.t {
                if degenerate(&m, t) {
                    continue;
                }
                let mut key = *t;
                key.sort_unstable();
                let forward = (0..3).any(|r| [t[r], t[(r + 1) % 3], t[(r + 2) % 3]] == key);
                if !seen.insert((key, forward)) {
                    continue;
                }
                let entry = counts.entry(key).or_default();
                if forward {
                    entry.0.push(*t);
                } else {
                    entry.1.push(*t);
                }
            }
            // Opposing coincident surfaces enclose no material; remove both, not an arbitrary side.
            m.t = counts
                .into_values()
                .flat_map(|(a, b)| {
                    if !a.is_empty() && !b.is_empty() {
                        vec![]
                    } else {
                        a.into_iter().chain(b).collect()
                    }
                })
                .collect();
            m.t.sort_unstable();
        }
        "orient" => orient(&mut m)?,
        "weld" => weld(&mut m, tolerance)?,
        "fill" => fill(&mut m, tolerance)?,
        _ => return Err(format!("unknown mesh repair '{action}'")),
    }
    prepare(m)
}

fn orient(m: &mut Mesh) -> Result<(), String> {
    let edges = edge_map(m);
    if edges.values().any(|u| u.len() > 2) {
        return Err(
            "cannot fix winding across non-manifold edges: clean or repair those first".into(),
        );
    }
    let mut neighbors = vec![Vec::new(); m.t.len()];
    for u in edges.values().filter(|u| u.len() == 2) {
        let ((a, ad), (b, bd)) = (u[0], u[1]);
        neighbors[a].push((b, ad == bd));
        neighbors[b].push((a, ad == bd));
    }
    let mut flip = vec![None; m.t.len()];
    for root in 0..m.t.len() {
        if flip[root].is_some() {
            continue;
        }
        flip[root] = Some(false);
        let mut queue = VecDeque::from([root]);
        while let Some(a) = queue.pop_front() {
            for &(b, different) in &neighbors[a] {
                let value = flip[a].unwrap() ^ different;
                match flip[b] {
                    Some(old) if old != value => {
                        return Err(
                            "the surface is not orientable: winding repair was not applied".into(),
                        )
                    }
                    Some(_) => {}
                    None => {
                        flip[b] = Some(value);
                        queue.push_back(b);
                    }
                }
            }
        }
    }
    for (t, flip) in m.t.iter_mut().zip(flip) {
        if flip == Some(true) {
            t.swap(1, 2);
        }
    }
    // Keep relative winding of separate shells, including cavity walls.
    if m.volume() < 0.0 {
        for t in &mut m.t {
            t.swap(1, 2);
        }
    }
    Ok(())
}

fn checked_tolerance(tolerance: f64) -> Result<(), String> {
    if !tolerance.is_finite() || !(0.000001..=10.0).contains(&tolerance) {
        return Err("repair tolerance must be between 0.000001 and 10 mm".into());
    }
    Ok(())
}

fn weld(m: &mut Mesh, tolerance: f64) -> Result<(), String> {
    checked_tolerance(tolerance)?;
    let mut cells = HashMap::<[i64; 3], Vec<usize>>::new();
    let mut vertices = Vec::<P3>::new();
    let mut remap = Vec::new();
    let (lo, _) = m.bounds();
    for &p in &m.v {
        let local = sub(p, lo);
        if local.iter().any(|x| x / tolerance > (i64::MAX / 2) as f64) {
            return Err("model coordinates are too large for this weld tolerance".into());
        }
        let cell = local.map(|x| (x / tolerance).floor() as i64);
        let mut best: Option<(usize, f64)> = None;
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    for &i in cells
                        .get(&[cell[0] + x, cell[1] + y, cell[2] + z])
                        .into_iter()
                        .flatten()
                    {
                        let d = sub(p, vertices[i]);
                        let d = dot(d, d);
                        if d <= tolerance * tolerance
                            && best.is_none_or(|(j, old)| d < old || (d == old && i < j))
                        {
                            best = Some((i, d));
                        }
                    }
                }
            }
        }
        let index = best.map(|(i, _)| i).unwrap_or_else(|| {
            let i = vertices.len();
            vertices.push(p);
            cells.entry(cell).or_default().push(i);
            i
        });
        remap.push(index);
    }
    for t in &mut m.t {
        *t = t.map(|i| remap[i]);
    }
    m.v = vertices;
    Ok(())
}

fn fill(m: &mut Mesh, tolerance: f64) -> Result<(), String> {
    checked_tolerance(tolerance)?;
    let edges = edge_map(m);
    if edges
        .values()
        .any(|u| u.len() > 2 || (u.len() == 2 && u[0].1 == u[1].1))
    {
        return Err("fix winding and non-manifold edges before filling holes".into());
    }
    let mut next = BTreeMap::new();
    let mut incoming = HashSet::new();
    for ((a, b), uses) in edges.iter().filter(|(_, u)| u.len() == 1) {
        let (a, b) = if uses[0].1 { (*b, *a) } else { (*a, *b) };
        if next.insert(a, b).is_some() || !incoming.insert(b) {
            return Err(
                "the boundary branches: automatic hole filling cannot safely patch it".into(),
            );
        }
    }
    let mut patches = Vec::new();
    while let Some((&start, _)) = next.first_key_value() {
        let mut ring = vec![start];
        let mut at = start;
        loop {
            at = next
                .remove(&at)
                .ok_or("the boundary is not a closed loop: hole filling was not applied")?;
            if at == start {
                break;
            }
            ring.push(at);
            if ring.len() > 2048 {
                return Err(
                    "hole boundary exceeds 2048 vertices: repair it in a dedicated mesh editor"
                        .into(),
                );
            }
        }
        patches.extend(triangulate(m, &ring, tolerance)?);
    }
    m.t.extend(patches);
    Ok(())
}

fn triangulate(m: &Mesh, ring: &[usize], tolerance: f64) -> Result<Vec<[usize; 3]>, String> {
    if ring.len() < 3 {
        return Err("hole boundary has fewer than three vertices".into());
    }
    let origin = m.v[ring[0]];
    let mut normal = [0.0; 3];
    for i in 0..ring.len() {
        let c = cross(
            sub(m.v[ring[i]], origin),
            sub(m.v[ring[(i + 1) % ring.len()]], origin),
        );
        for k in 0..3 {
            normal[k] += c[k];
        }
    }
    let length = dot(normal, normal).sqrt();
    if length == 0.0 {
        return Err("hole boundary has no stable plane".into());
    }
    normal = normal.map(|x| x / length);
    if ring
        .iter()
        .any(|&i| dot(sub(m.v[i], origin), normal).abs() > tolerance)
    {
        return Err("hole is not planar within the tolerance: no patch was applied".into());
    }
    let axis = (0..3)
        .max_by(|&a, &b| normal[a].abs().total_cmp(&normal[b].abs()))
        .unwrap();
    let p = |i: usize| {
        let v = sub(m.v[i], origin);
        [v[(axis + 1) % 3], v[(axis + 2) % 3]]
    };
    let area = |a: usize, b: usize, c: usize| {
        let (a, b, c) = (p(a), p(b), p(c));
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let sign = normal[axis].signum();
    for i in 0..ring.len() {
        for j in i + 1..ring.len() {
            if j == i + 1 || (i == 0 && j == ring.len() - 1) {
                continue;
            }
            let (a, b, c, d) = (
                ring[i],
                ring[(i + 1) % ring.len()],
                ring[j],
                ring[(j + 1) % ring.len()],
            );
            let (ac, ad, ca, cb) = (area(a, b, c), area(a, b, d), area(c, d, a), area(c, d, b));
            let overlap = (0..2).all(|k| {
                p(a)[k].min(p(b)[k]) <= p(c)[k].max(p(d)[k])
                    && p(c)[k].min(p(d)[k]) <= p(a)[k].max(p(b)[k])
            });
            if overlap && ac * ad <= 0.0 && ca * cb <= 0.0 {
                return Err("hole boundary is self-crossing: no patch was applied".into());
            }
        }
    }
    let mut left = ring.to_vec();
    let mut triangles = Vec::new();
    while left.len() > 3 {
        let ear = (0..left.len())
            .find(|&i| {
                let (a, b, c) = (
                    left[(i + left.len() - 1) % left.len()],
                    left[i],
                    left[(i + 1) % left.len()],
                );
                area(a, b, c) * sign > 0.0
                    && !left.iter().any(|&q| {
                        q != a
                            && q != b
                            && q != c
                            && area(a, b, q) * sign >= 0.0
                            && area(b, c, q) * sign >= 0.0
                            && area(c, a, q) * sign >= 0.0
                    })
            })
            .ok_or("hole boundary is self-crossing or cannot be triangulated safely")?;
        triangles.push([
            left[(ear + left.len() - 1) % left.len()],
            left[ear],
            left[(ear + 1) % left.len()],
        ]);
        left.remove(ear);
    }
    if area(left[0], left[1], left[2]) * sign <= 0.0 {
        return Err("hole triangulation would create a degenerate face".into());
    }
    triangles.push([left[0], left[1], left[2]]);
    Ok(triangles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Engine;

    fn cube() -> Mesh {
        Mesh::cuboid([0.0; 3], [10.0, 8.0, 2.0])
    }

    #[test]
    fn defects_are_retained_reported_and_solid_output_is_blocked_until_cleaned() {
        let mut m = cube();
        m.t.push([0, 0, 1]);
        m.t.push(m.t[0]);
        let mut e = Engine::new();
        let source = m.to_stl();
        let id = e.add_model(source.clone(), "stl").unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let info = e.model_info(&id, "stl").unwrap();
        assert_eq!(info["triangles"], 14);
        assert_eq!(info["degenerate"], 1);
        assert_eq!(info["duplicate_faces"], 1);
        assert_eq!(info["valid"], false);
        assert_eq!(info["markers"]["points"].as_array().unwrap().len(), 2);
        assert_eq!(e.model_preview(&id, "stl").unwrap().len(), source.len());
        let graph = |id: &str| {
            json!({"nodes": [
            {"id": "m", "type": "mesh_import", "params": {"model_id": id}},
            {"id": "b", "type": "bridges", "params": {}}
        ], "edges": [{"from": ["m", "solid"], "to": ["b", "body"]}]})
        };
        let result = e.run(&graph(&id), None, &|| 0.0);
        assert_eq!(result["nodes"]["m"]["state"], "error");
        assert!(result["nodes"]["m"]["key"].is_null());
        assert_eq!(result["nodes"]["b"]["state"], "error");
        let repaired = e.repair_model(&id, "stl", "clean", 0.01).unwrap();
        let new_id = e.add_model(repaired, "stl").unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let info = e.model_info(&new_id, "stl").unwrap();
        assert_eq!(info["triangles"], 12);
        assert_eq!(info["valid"], true);
        assert_eq!(e.model_info(&id, "stl").unwrap()["triangles"], 14);
        let result = e.run(&graph(&new_id), None, &|| 0.0);
        assert_eq!(result["nodes"]["b"]["state"], "done", "{result}");
    }

    #[test]
    fn repairs_winding_without_moving_vertices() {
        let mut m = cube();
        m.t[3].swap(1, 2);
        assert!(inspect(&m)["winding_edges"].as_u64().unwrap() > 0);
        let r = repair(m.clone(), "orient", 0.01).unwrap();
        assert_eq!(inspect(&r)["valid"], true);
        assert_eq!(r.v, m.v);
        assert!((r.volume() - 160.0).abs() < 1e-8);
    }

    #[test]
    fn fills_planar_holes_and_rejects_nonplanar_boundaries_transactionally() {
        let mut m = cube();
        m.t.drain(2..4);
        assert_eq!(inspect(&m)["boundary_edges"], 4);
        let r = repair(m.clone(), "fill", 0.01).unwrap();
        assert_eq!(inspect(&r)["valid"], true);
        assert!((r.volume() - 160.0).abs() < 1e-8);
        m.v[7][2] += 1.0;
        assert!(repair(m, "fill", 0.01)
            .err()
            .unwrap()
            .contains("not planar"));
    }

    #[test]
    fn weld_uses_explicit_distance_and_does_not_silently_drop_faces() {
        let mut m = cube();
        let index = m.v.len();
        let mut shifted = m.v[0];
        shifted[0] += 0.001;
        m.v.push(shifted);
        m.t[0][0] = index;
        assert_eq!(inspect(&m)["valid"], false);
        let r = repair(m.clone(), "weld", 0.01).unwrap();
        assert_eq!(r.t.len(), m.t.len());
        assert_eq!(inspect(&r)["valid"], true);
        assert_eq!(
            inspect(&repair(m, "weld", 0.00001).unwrap())["valid"],
            false
        );
        for tolerance in [0.0, -0.1, f64::NAN, 11.0] {
            assert!(repair(cube(), "weld", tolerance).is_err());
        }
        assert!(repair(cube(), "unknown", 0.01).is_err());
    }

    #[test]
    fn cleaning_opposing_pairs_does_not_keep_an_arbitrary_surface() {
        let mut m = cube();
        m.v.extend([[20.0, 0.0, 0.0], [20.0, 1.0, 0.0], [21.0, 0.0, 0.0]]);
        m.t.extend([[8, 9, 10], [10, 9, 8]]);
        let r = repair(m, "clean", 0.01).unwrap();
        assert_eq!(r.t.len(), 12);
        assert_eq!(inspect(&r)["valid"], true);
    }

    #[test]
    fn markers_are_bounded_but_statistics_are_complete() {
        let mut m = cube();
        m.t.extend(std::iter::repeat_n([0, 0, 1], 3000));
        let info = inspect(&m);
        assert_eq!(info["degenerate"], 3000);
        assert_eq!(info["markers"]["points"].as_array().unwrap().len(), MARKERS);
        assert_eq!(info["markers"]["truncated"], true);
    }
}
