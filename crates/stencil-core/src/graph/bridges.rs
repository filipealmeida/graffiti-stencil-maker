//! The Bridges node: joins the separate solids of a stencil, or of each tile of some parts, to the biggest piece.

use super::catalogue::Outputs;
use super::*;
use crate::mesh::Mesh;
use crate::tile_bridge::{bridge_mesh, components, BridgeParams};

const SOLID_FILE: &str = "stencil.stl";

fn num(v: &Json) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

/// Bridges the islands of one mesh to its biggest piece. Returns the mesh and the piece counts before and after.
fn bridge_piece(
    mut m: Mesh,
    origin: [f64; 2],
    pockets: &[[f64; 4]],
    bp: &BridgeParams,
) -> Result<(Mesh, usize, usize), String> {
    let top = bp.plate_top;
    let before = components(&m).0;
    if before < 2 {
        return Ok((m, before, before));
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
    let plane = if bp.flip { 0.0 } else { top };
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
    for _ in 0..bp.passes.max(1) {
        if components(&m).0 < 2 {
            break;
        }
        let Some(b) = bridge_mesh(&m, &avoid, bp)? else {
            break;
        };
        m = m.union(&b.intersect(&clip)?)?;
    }
    // the STL stores f32: round now so what is checked is what is written
    m.quantize_f32();
    let after = components(&m).0;
    if !m.is_watertight() {
        return Err("verification failed: not watertight after bridging".into());
    }
    Ok((m, before, after))
}

fn bridge_params(p: &P, top: f64, layer: f64, thickness: f64, flip: bool) -> BridgeParams {
    BridgeParams {
        bridge_mm: p.f("bridge_mm"),
        per_island: p.f("bridges_per_island") as usize,
        raised: p.b("raised_bridges"),
        easy: p.s("z_bridging") == "easy",
        base_layers: ((if thickness > 0.0 { thickness } else { top }) / layer).round() as u32,
        layer_height: layer,
        plate_top: top,
        flip,
        min_angle_deg: if p.s("bridge_spacing") == "spread" {
            p.f("bridge_min_angle_deg")
        } else {
            0.0
        },
        max_length: p.f("bridge_max_length"),
        passes: 3,
    }
}

fn warning(left_over: usize) -> Vec<String> {
    if left_over == 0 {
        return vec![];
    }
    vec![format!(
        "{left_over} island{} could not be bridged: they have no top face to carry a bridge, or the only way across is blocked by a connector pocket",
        if left_over > 1 { "s" } else { "" }
    )]
}

pub(super) fn run_bridges(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    match i.get("body") {
        Some(Value::Solid(s)) => bridge_solid(s, p),
        Some(Value::Parts(src)) => bridge_parts(src, p),
        _ => Err("nothing is connected to the 'body' input: connect a solid or parts".into()),
    }
}

fn bridge_solid(s: &Solid, p: &P) -> Result<Outputs, String> {
    let st = &s.stats;
    let (top, layer) = (num(&st["plate_top_mm"]), num(&st["layer_height_mm"]));
    if top <= 0.0 || layer <= 0.0 {
        return Err("the solid has no plate description: connect a Stencil node".into());
    }
    let flip = st["flip"] == true;
    let bp = bridge_params(p, top, layer, num(&st["thickness_mm"]), flip);
    let (m, before, after) = bridge_piece(Mesh::from_stl(&s.stl)?, [0.0, 0.0], &[], &bp)?;
    let stl = m.to_stl();
    let mut stats = s.stats.clone();
    stats["islands_found"] = json!(before.saturating_sub(1) + num(&st["islands_found"]) as usize);
    stats["islands_remaining"] = json!(after.saturating_sub(1));
    stats["triangles"] = json!(m.t.len());
    stats["stl_bytes"] = json!(stl.len());
    stats["watertight"] = json!(true);
    let (lo, hi) = m.bounds();
    let parts = Parts {
        files: vec![(SOLID_FILE.to_string(), stl.clone())],
        meta: json!({"plate": {
            "top": top, "layer": layer, "thickness": num(&st["thickness_mm"]), "flip": flip,
            "connectors": [],
            "parts": [{"file": SOLID_FILE, "role": "tile", "origin": [0.0, 0.0],
                "size": [hi[0] - lo[0], hi[1] - lo[1]]}],
        }}),
    };
    Ok(vec![
        ("solid", Value::Solid(Rc::new(Solid { stl, stats }))),
        ("parts", Value::Parts(Rc::new(parts))),
    ])
}

fn bridge_parts(src: &Parts, p: &P) -> Result<Outputs, String> {
    let plate = &src.meta["plate"];
    let (top, layer) = (num(&plate["top"]), num(&plate["layer"]));
    if plate.is_null() || top <= 0.0 || layer <= 0.0 {
        return Err(
            "the parts have no plate description: connect a Split volume or Tile layout node"
                .into(),
        );
    }
    let bp = bridge_params(
        p,
        top,
        layer,
        num(&plate["thickness"]),
        plate["flip"] == true,
    );
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
        let m = Mesh::from_stl(data)?;
        if components(&m).0 < 2 {
            files.push((name.clone(), data.clone()));
            continue;
        }
        let (m, before, after) =
            bridge_piece(m, *origin, &pockets, &bp).map_err(|e| format!("{name}: {e}"))?;
        islands_found += before - 1;
        islands_bridged += (before - after).min(before - 1);
        left_over += after - 1;
        tiles_bridged += usize::from(after < before);
        report.push(json!({"file": name, "islands": before - 1, "left": after - 1}));
        files.push((name.clone(), m.to_stl()));
    }
    let mut meta = src.meta.clone();
    meta["bridged_fragments"] = json!({
        "islands_found": islands_found, "islands_bridged": islands_bridged,
        "islands_left": left_over, "tiles_bridged": tiles_bridged,
        "tiles": report, "warnings": warning(left_over),
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
        i.insert("body".to_string(), Value::Parts(Rc::new(parts)));
        let p = P(serde_json::from_value(params).unwrap());
        match &run_bridges(&Engine::new(), &i, &p).unwrap()[0].1 {
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

    #[test]
    fn a_solid_in_gives_a_bridged_solid_and_one_part() {
        let mut m = Mesh::cuboid([0.0; 3], [30.0, 30.0, 5.0]);
        m.append(&Mesh::cuboid([33.0, 10.0, 0.0], [40.0, 20.0, 5.0]));
        let solid = Solid {
            stl: m.to_stl(),
            stats: json!({"plate_top_mm": 5.0, "layer_height_mm": 0.2, "thickness_mm": 5.0, "flip": false}),
        };
        let mut i = HashMap::new();
        i.insert("body".to_string(), Value::Solid(Rc::new(solid)));
        let p = P(serde_json::from_value(
            json!({"bridge_mm": 1.6, "bridges_per_island": 1, "raised_bridges": false, "z_bridging": "easy"}),
        )
        .unwrap());
        let out = run_bridges(&Engine::new(), &i, &p).unwrap();
        let (Value::Solid(s), Value::Parts(parts)) = (&out[0].1, &out[1].1) else {
            panic!()
        };
        assert_eq!(s.stats["islands_remaining"], 0);
        assert_eq!(components(&Mesh::from_stl(&s.stl).unwrap()).0, 1);
        assert_eq!(parts.files.len(), 1);
        assert_eq!(parts.meta["plate"]["parts"][0]["role"], "tile");
    }
}
