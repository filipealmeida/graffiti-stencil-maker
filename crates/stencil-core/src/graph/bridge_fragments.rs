//! The Bridge fragments node: joins the separate solids of each tile to the tile's biggest piece.

use super::catalogue::Outputs;
use super::*;
use crate::mesh::Mesh;
use crate::tile_bridge::{bridge_mesh, components, BridgeParams};

pub(super) fn run_bridge_fragments(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let Some(Value::Parts(src)) = i.get("parts") else {
        return Err("nothing is connected to the 'parts' input: connect parts".into());
    };
    let plate = &src.meta["plate"];
    let num = |v: &Json| v.as_f64().unwrap_or(0.0);
    let (top, layer) = (num(&plate["top"]), num(&plate["layer"]));
    if plate.is_null() || top <= 0.0 || layer <= 0.0 {
        return Err(
            "the parts have no plate description: connect a Split volume or Tile layout node"
                .into(),
        );
    }
    let thickness = num(&plate["thickness"]);
    let bp = BridgeParams {
        bridge_mm: p.f("bridge_mm"),
        per_island: p.f("bridges_per_island") as usize,
        raised: p.b("raised_bridges"),
        easy: p.s("z_bridging") == "easy",
        base_layers: ((if thickness > 0.0 { thickness } else { top }) / layer).round() as u32,
        layer_height: layer,
        plate_top: top,
        flip: plate["flip"] == true,
    };
    let entries = plate["parts"].as_array().cloned().unwrap_or_default();
    let tiles: Vec<(String, [f64; 2])> = entries
        .iter()
        .filter(|e| e["role"] == "tile")
        .filter_map(|e| {
            e["file"]
                .as_str()
                .map(|f| (f.to_string(), [num(&e["origin"][0]), num(&e["origin"][1])]))
        })
        .collect();
    if tiles.is_empty() {
        return Err("the parts have no tiles: connect a Split volume or Tile layout node".into());
    }
    // connector pockets must stay open: bridges keep out of them
    let pockets: Vec<[f64; 4]> = plate["connectors"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| [num(&r[0]), num(&r[1]), num(&r[2]), num(&r[3])])
                .collect()
        })
        .unwrap_or_default();
    let mut files = Vec::with_capacity(src.files.len());
    let mut report = Vec::new();
    let (mut islands_found, mut islands_bridged, mut tiles_bridged, mut left_over) =
        (0usize, 0usize, 0usize, 0usize);
    for (name, data) in &src.files {
        let Some((_, origin)) = tiles.iter().find(|t| &t.0 == name) else {
            files.push((name.clone(), data.clone()));
            continue;
        };
        let mut m = Mesh::from_stl(data)?;
        let before = components(&m).0;
        if before < 2 {
            files.push((name.clone(), data.clone()));
            continue;
        }
        let (ml, mh) = m.bounds();
        let avoid: Vec<([f64; 3], [f64; 3])> = pockets
            .iter()
            .map(|c| {
                (
                    [c[0] - origin[0], c[1] - origin[1], -1.0],
                    [c[2] - origin[0], c[3] - origin[1], top + 1.0],
                )
            })
            .filter(|(lo, hi)| lo[0] < mh[0] && hi[0] > ml[0] && lo[1] < mh[1] && hi[1] > ml[1])
            .collect();
        // A bevelled side makes the tile narrower at one face than its bounding box: bridges, which lie on the
        // bridging plane, must stay inside the outline the tile has there.
        let plane = if bp.flip { 0.0 } else { bp.plate_top };
        let (mut fl, mut fh) = ([f64::MAX; 2], [f64::MIN; 2]);
        for v in m.v.iter().filter(|v| (v[2] - plane).abs() < 1e-3) {
            for k in 0..2 {
                fl[k] = fl[k].min(v[k]);
                fh[k] = fh[k].max(v[k]);
            }
        }
        let (fl, fh) = if fl[0] < fh[0] && fl[1] < fh[1] {
            (fl, fh)
        } else {
            ([ml[0], ml[1]], [mh[0], mh[1]])
        };
        let clip = Mesh::cuboid([fl[0], fl[1], -1.0], [fh[0], fh[1], top + 1.0]);
        for _ in 0..3 {
            if components(&m).0 < 2 {
                break;
            }
            let Some(b) = bridge_mesh(&m, &avoid, &bp)? else {
                break;
            };
            m = m.union(&b.intersect(&clip)?)?;
        }
        // the STL stores f32: round now so what is checked is what is written
        m.quantize_f32();
        let after = components(&m).0;
        if !m.is_watertight() {
            return Err(format!(
                "verification failed: {name} is not watertight after bridging"
            ));
        }
        islands_found += before - 1;
        islands_bridged += (before - after).min(before - 1);
        left_over += after - 1;
        tiles_bridged += usize::from(after < before);
        report.push(json!({"file": name, "islands": before - 1, "left": after - 1}));
        files.push((name.clone(), m.to_stl()));
    }
    let mut meta = src.meta.clone();
    let mut warnings = Vec::new();
    if left_over > 0 {
        warnings.push(format!(
            "{left_over} island{} could not be bridged: they have no top face to carry a bridge, or the only way across is blocked by a connector pocket",
            if left_over > 1 { "s" } else { "" }
        ));
    }
    meta["bridged_fragments"] = json!({
        "islands_found": islands_found, "islands_bridged": islands_bridged,
        "islands_left": left_over, "tiles_bridged": tiles_bridged,
        "tiles": report, "warnings": warnings,
    });
    Ok(vec![(
        "parts",
        Value::Parts(Rc::new(Parts { files, meta })),
    )])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(m: &Mesh, params: Json) -> Rc<Parts> {
        let parts = Parts {
            files: vec![
                ("stencil_r1c1.stl".into(), m.to_stl()),
                ("rod.stl".into(), m.to_stl()),
            ],
            meta: json!({"plate": {"top": 5.0, "layer": 0.2, "thickness": 5.0, "flip": false,
                "connectors": [], "parts": [{"file": "stencil_r1c1.stl", "role": "tile", "origin": [0.0, 0.0]}]}}),
        };
        let mut i = HashMap::new();
        i.insert("parts".to_string(), Value::Parts(Rc::new(parts)));
        let p = P(serde_json::from_value(params).unwrap());
        match &run_bridge_fragments(&Engine::new(), &i, &p).unwrap()[0].1 {
            Value::Parts(p) => p.clone(),
            _ => panic!(),
        }
    }

    fn pieces(p: &Parts, name: &str) -> usize {
        components(&Mesh::from_stl(&p.files.iter().find(|f| f.0 == name).unwrap().1).unwrap()).0
    }

    #[test]
    fn islands_are_bridged_to_the_biggest_piece() {
        let mut m = Mesh::cuboid([0.0; 3], [30.0, 30.0, 5.0]);
        m.append(&Mesh::cuboid([33.0, 10.0, 0.0], [40.0, 20.0, 5.0]));
        assert_eq!(components(&m).0, 2);
        let p = run(
            &m,
            json!({"bridge_mm": 1.6, "bridges_per_island": 1, "raised_bridges": false, "z_bridging": "easy"}),
        );
        assert_eq!(pieces(&p, "stencil_r1c1.stl"), 1);
        assert_eq!(p.meta["bridged_fragments"]["islands_bridged"], 1);
        assert_eq!(p.meta["bridged_fragments"]["islands_left"], 0);
        // files that are not tiles are left alone
        assert_eq!(pieces(&p, "rod.stl"), 2);
        let tile = Mesh::from_stl(&p.files[0].1).unwrap();
        assert!(tile.is_watertight() && tile.volume() > m.volume());
    }

    #[test]
    fn an_island_with_no_top_face_is_reported_not_hidden() {
        let mut m = Mesh::cuboid([0.0; 3], [30.0, 30.0, 5.0]);
        m.append(&Mesh::cuboid([33.0, 10.0, 1.0], [40.0, 20.0, 4.0]));
        let p = run(
            &m,
            json!({"bridge_mm": 1.6, "bridges_per_island": 1, "raised_bridges": false, "z_bridging": "easy"}),
        );
        assert_eq!(p.meta["bridged_fragments"]["islands_left"], 1);
        assert_eq!(pieces(&p, "stencil_r1c1.stl"), 2);
    }
}
