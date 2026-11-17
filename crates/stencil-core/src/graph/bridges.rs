//! The Bridges node: joins the separate solids of a stencil, or of each tile of some parts, to the biggest piece.

use super::types::Outputs;
use super::*;
use crate::mesh::Mesh;
use crate::tile_bridge::{bridge_mesh, components, BridgeParams, BridgeSpacing};

mod contact;

const SOLID_FILE: &str = "stencil.stl";

fn num(v: &Json) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

/// The leaning seam faces of one tile, in the tile's own coordinates: which sides are cut by a seam, and how the
/// cut leans and sits back from the seam line. Mirrors how Split volume cuts its tiles.
struct Sides {
    size: [f64; 2],
    /// Seam on the low and high side of x, then of y.
    seam: [bool; 4],
    lean: f64,
    inset: f64,
    z_mid: f64,
}

impl Sides {
    /// The solid a bridge may occupy: vertical at the plate's outer border, leaning along every seam.
    fn clip(&self, top: f64) -> Mesh {
        let rect = |z: f64| -> [[f64; 2]; 4] {
            let d = self.lean * (z - self.z_mid);
            let side = |seam: bool, edge: f64, out: f64, sign: f64| {
                if seam {
                    edge + sign * self.inset + d
                } else {
                    out
                }
            };
            let xa = side(self.seam[0], 0.0, -1.0, 1.0);
            let xb = side(self.seam[1], self.size[0], self.size[0] + 1.0, -1.0);
            let ya = side(self.seam[2], 0.0, -1.0, 1.0);
            let yb = side(self.seam[3], self.size[1], self.size[1] + 1.0, -1.0);
            [[xa, ya], [xb, ya], [xb, yb], [xa, yb]]
        };
        Mesh::loft_quads(rect(-1.0), -1.0, rect(top + 1.0), top + 1.0)
    }
}

/// Bridges the islands of one mesh to its biggest piece. Returns the mesh and the piece counts before and after.
fn bridge_piece(
    mut m: Mesh,
    origin: [f64; 2],
    pockets: &[[f64; 4]],
    sides: Option<&Sides>,
    bp: &BridgeParams,
) -> Result<(Mesh, usize, usize, contact::Contacts), String> {
    let top = bp.plate_top;
    let before = components(&m).0;
    if before < 2 {
        return Ok((m, before, before, contact::Contacts::default()));
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
    let mut clip = Mesh::cuboid([fl[0], fl[1], -1.0], [fh[0], fh[1], top + 1.0]);
    // a bevelled seam face leans in from the bridging plane: a bridge must end on that face, not poke through it
    if let Some(sd) = sides.filter(|sd| sd.lean > 1e-9 && sd.seam.iter().any(|&b| b)) {
        clip = clip.intersect(&sd.clip(top))?;
    }
    for &(lo, hi) in &avoid {
        clip = clip.subtract(&Mesh::cuboid(lo, hi))?;
    }
    let original = m.clone();
    let mut added = Mesh::default();
    if let Some(b) = bridge_mesh(&original, &avoid, bp)? {
        let mut b = b.intersect(&clip)?;
        if bp.min_contact_mm > 0.0 {
            let mut quality = contact::measure(&original, &b, bp.layer_height);
            for factor in [1.5, 2.0] {
                if quality.weak(bp.min_contact_mm) == 0 && !quality.widths.is_empty() {
                    break;
                }
                let mut wider = *bp;
                wider.min_contact_mm *= factor;
                let Some(candidate) = bridge_mesh(&original, &avoid, &wider)? else {
                    break;
                };
                let candidate = candidate.intersect(&clip)?;
                let candidate_quality = contact::measure(&original, &candidate, bp.layer_height);
                let deficit = |mesh: &Mesh| {
                    contact::attachments(&original, mesh, plane, bp.layer_height, bp.flip)
                        .routes
                        .iter()
                        .map(|&(_, count)| bp.per_island.saturating_sub(count))
                        .sum::<usize>()
                };
                if candidate_quality.minimum().unwrap_or(0.0) > quality.minimum().unwrap_or(0.0)
                    && deficit(&candidate) <= deficit(&b)
                {
                    b = candidate;
                    quality = candidate_quality;
                }
            }
        }
        m = m.union(&b)?;
        added = b;
    }
    // the STL stores f32: round now so what is checked is what is written
    m.quantize_f32();
    let after = components(&m).0;
    if !m.is_watertight() {
        return Err("verification failed: not watertight after bridging".into());
    }
    added.quantize_f32();
    let mut contacts = if bp.min_contact_mm > 0.0 {
        contact::measure(&original, &added, bp.layer_height)
    } else {
        contact::Contacts::default()
    };
    let attachments = contact::attachments(&original, &added, plane, bp.layer_height, bp.flip);
    contacts.counts = attachments.counts;
    contacts.routes = attachments.routes;
    Ok((m, before, after, contacts))
}

fn bridge_params(p: &P, top: f64, layer: f64, thickness: f64, flip: bool) -> BridgeParams {
    BridgeParams {
        bridge_mm: p.f("bridge_mm"),
        min_contact_mm: p.f("bridge_min_contact_mm"),
        per_island: p.f("bridges_per_island") as usize,
        raised: p.b("raised_bridges"),
        easy: p.s("z_bridging") == "easy",
        base_layers: ((if thickness > 0.0 { thickness } else { top }) / layer).round() as u32,
        layer_height: layer,
        plate_top: top,
        flip,
        min_angle_deg: p.f("bridge_min_angle_deg"),
        spacing: match p.s("bridge_spacing") {
            "aligned" => BridgeSpacing::Aligned,
            "spread" => BridgeSpacing::Spread,
            _ => BridgeSpacing::Closest,
        },
        max_length: p.f("bridge_max_length"),
        passes: 3,
    }
}

fn contact_report(contacts: &contact::Contacts, target: f64, requested: usize) -> Json {
    json!({
        "target_mm": target,
        "minimum_mm": contacts.minimum(),
        "contacts_checked": contacts.widths.len(),
        "weak_contacts": contacts.weak(target),
        "bridges_requested": requested,
        "islands": contacts.counts.iter().zip(&contacts.routes).map(|(&(island, placed), &(_, routes))| json!({
            "island": island + 1, "placed": placed, "independent_routes": routes, "requested": requested,
        })).collect::<Vec<_>>(),
    })
}

fn count_warning(contacts: &contact::Contacts, bp: &BridgeParams) -> Vec<String> {
    let angle = if bp.spacing == BridgeSpacing::Aligned {
        "line-axis"
    } else {
        "attachment-position"
    };
    contacts.routes.iter().filter(|(_, count)| *count < bp.per_island).map(|&(island, count)| {
        format!("Island {}: {count} of {} independent bridge routes to the main piece; no further route found within the {:.0} degree minimum {angle} angle, bridge width/contact, length and pocket-clearance constraints",
            island + 1, bp.per_island, bp.min_angle_deg)
    }).collect()
}

fn contact_warning(contacts: &contact::Contacts, target: f64) -> Vec<String> {
    if target > 0.0 && contacts.widths.is_empty() {
        return vec!["No bridge contacts could be verified against the contact target".into()];
    }
    let weak = contacts.weak(target);
    if weak == 0 {
        return vec![];
    }
    vec![format!(
        "{weak} bridge contact{} below the {target:.2} mm target (minimum {:.2} mm); increase contact or bridge width, or change the tile layout",
        if weak == 1 { "" } else { "s" },
        contacts.minimum().unwrap_or(0.0)
    )]
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
    let (m, before, after, contacts) =
        bridge_piece(Mesh::from_stl(&s.stl)?, [0.0, 0.0], &[], None, &bp)?;
    let stl = m.to_stl();
    let mut stats = s.stats.clone();
    stats["islands_found"] = json!(before.saturating_sub(1) + num(&st["islands_found"]) as usize);
    stats["islands_remaining"] = json!(after.saturating_sub(1));
    stats["triangles"] = json!(m.t.len());
    stats["stl_bytes"] = json!(stl.len());
    stats["watertight"] = json!(true);
    let mut contact_stats = contact_report(&contacts, bp.min_contact_mm, bp.per_island);
    let mut warnings = if before > 1 {
        contact_warning(&contacts, bp.min_contact_mm)
    } else {
        vec![]
    };
    warnings.extend(count_warning(&contacts, &bp));
    contact_stats["warnings"] = json!(warnings);
    stats["bridge_contacts"] = contact_stats.clone();
    let (lo, hi) = m.bounds();
    let parts = Parts {
        files: vec![(SOLID_FILE.to_string(), stl.clone())],
        meta: json!({"bridge_contacts": contact_stats, "plate": {
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
            "the parts have no plate description: connect a Split volume or Tile plan node".into(),
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
    let seams: Vec<[f64; 4]> = plate["seams"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| [num(&r[0]), num(&r[1]), num(&r[2]), num(&r[3])])
                .collect()
        })
        .unwrap_or_default();
    let angle = num(&src.meta["tiling"]["seam_angle_deg"]).to_radians();
    let (lean, inset) = (
        angle.tan(),
        num(&src.meta["tiling"]["seam_gap_mm"]) / 2.0 / angle.cos(),
    );
    let z_mid = (if num(&plate["thickness"]) > 0.0 {
        num(&plate["thickness"])
    } else {
        top
    }) / 2.0;
    let tiles: Vec<(String, [f64; 2], Option<Sides>)> = entries
        .iter()
        .filter(|e| e["role"] == "tile")
        .filter_map(|e| {
            let f = e["file"].as_str()?;
            let origin = [num(&e["origin"][0]), num(&e["origin"][1])];
            let size = [num(&e["size"][0]), num(&e["size"][1])];
            // a seam lies along a tile's side when it runs at that coordinate across the tile's span
            let along = |axis: usize, at: f64| {
                seams.iter().any(|s| {
                    let (lo, hi) = (origin[1 - axis], origin[1 - axis] + size[1 - axis]);
                    s[0] == axis as f64 && (s[1] - at).abs() < 1e-3 && s[2] < hi && s[3] > lo
                })
            };
            let sides = (size[0] > 0.0 && size[1] > 0.0).then(|| Sides {
                size,
                seam: [
                    along(0, origin[0]),
                    along(0, origin[0] + size[0]),
                    along(1, origin[1]),
                    along(1, origin[1] + size[1]),
                ],
                lean,
                inset,
                z_mid,
            });
            Some((f.to_string(), origin, sides))
        })
        .collect();
    if tiles.is_empty() {
        return Err("the parts have no tiles: connect a Split volume or Tile plan node".into());
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
    let mut contact_warnings = Vec::new();
    let (mut islands_found, mut islands_bridged, mut tiles_bridged, mut left_over) =
        (0usize, 0usize, 0usize, 0usize);
    for (name, data) in &src.files {
        let Some((_, origin, sides)) = tiles.iter().find(|t| &t.0 == name) else {
            files.push((name.clone(), data.clone()));
            continue;
        };
        let m = Mesh::from_stl(data)?;
        if components(&m).0 < 2 {
            files.push((name.clone(), data.clone()));
            continue;
        }
        let (m, before, after, contacts) = bridge_piece(m, *origin, &pockets, sides.as_ref(), &bp)
            .map_err(|e| format!("{name}: {e}"))?;
        islands_found += before - 1;
        islands_bridged += (before - after).min(before - 1);
        left_over += after - 1;
        tiles_bridged += usize::from(after < before);
        contact_warnings.extend(
            contact_warning(&contacts, bp.min_contact_mm)
                .into_iter()
                .map(|w| format!("{name}: {w}")),
        );
        contact_warnings.extend(
            count_warning(&contacts, &bp)
                .into_iter()
                .map(|w| format!("{name}: {w}")),
        );
        report.push(
            json!({"file": name, "islands": before - 1, "left": after - 1,
            "bridge_contacts": contact_report(&contacts, bp.min_contact_mm, bp.per_island)}),
        );
        files.push((name.clone(), m.to_stl()));
    }
    let mut meta = src.meta.clone();
    let mut warnings = warning(left_over);
    warnings.extend(contact_warnings);
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

    fn nested() -> Mesh {
        let mut frame = Mesh::cuboid([0.0; 3], [80.0, 80.0, 5.0])
            .subtract(&Mesh::cuboid([5.0, 5.0, -1.0], [75.0, 75.0, 6.0]))
            .unwrap();
        let ring = Mesh::cuboid([20.0, 20.0, 0.0], [60.0, 60.0, 5.0])
            .subtract(&Mesh::cuboid([25.0, 25.0, -1.0], [55.0, 55.0, 6.0]))
            .unwrap();
        frame.append(&ring);
        frame.append(&Mesh::cuboid([35.0, 35.0, 0.0], [45.0, 45.0, 5.0]));
        frame
    }

    #[test]
    fn nested_components_have_four_verified_attachments_after_meshing() {
        for spacing in [
            BridgeSpacing::Closest,
            BridgeSpacing::Spread,
            BridgeSpacing::Aligned,
        ] {
            for flip in [false, true] {
                for easy in [false, true] {
                    let bp = BridgeParams {
                        bridge_mm: 2.0,
                        per_island: 4,
                        min_angle_deg: 30.0,
                        spacing,
                        flip,
                        easy,
                        base_layers: 25,
                        plate_top: 5.0,
                        ..Default::default()
                    };
                    let (mesh, _, after, contacts) =
                        bridge_piece(nested(), [0.0; 2], &[], None, &bp).unwrap();
                    assert_eq!(after, 1);
                    assert!(mesh.is_watertight());
                    assert_eq!(contacts.counts.len(), 2);
                    for &(_, count) in &contacts.counts {
                        assert!(
                            count >= 4,
                            "spacing={spacing:?}, flip={flip}, easy={easy}: {count}"
                        );
                    }
                    assert!(
                        count_warning(&contacts, &bp).is_empty(),
                        "{:?}",
                        count_warning(&contacts, &bp)
                    );
                }
            }
        }
    }

    #[test]
    fn unmet_counts_are_reported_in_all_modes() {
        for spacing in ["closest", "spread", "aligned"] {
            let p = run(
                &{
                    let mut m = Mesh::cuboid([0.0; 3], [80.0, 80.0, 5.0])
                        .subtract(&Mesh::cuboid([5.0, 5.0, -1.0], [75.0, 75.0, 6.0]))
                        .unwrap();
                    m.append(&Mesh::cuboid([35.0, 35.0, 0.0], [45.0, 45.0, 5.0]));
                    m
                },
                json!({
                    "bridge_mm": 2.0, "bridges_per_island": 5, "bridge_min_angle_deg": 90.0,
                    "bridge_spacing": spacing, "bridge_max_length": 4.0,
                    "raised_bridges": true, "z_bridging": "easy"
                }),
            );
            let report = &p.meta["bridged_fragments"];
            assert_eq!(report["islands_left"], 0, "spacing={spacing}: {report}");
            let islands = report["tiles"][0]["bridge_contacts"]["islands"]
                .as_array()
                .unwrap();
            assert_eq!(islands.len(), 1);
            assert!(islands.iter().all(|i| i["placed"].as_u64().unwrap() < 5));
            assert!(report["warnings"].as_array().unwrap().iter().any(|w| {
                w.as_str()
                    .unwrap()
                    .contains("of 5 independent bridge routes")
            }));
        }
    }

    #[test]
    fn connector_pockets_stay_open_with_contact_disabled() {
        let mut m = Mesh::cuboid([0.0; 3], [10.0, 30.0, 5.0]);
        m.append(&Mesh::cuboid([20.0, 0.0, 0.0], [30.0, 30.0, 5.0]));
        let bp = BridgeParams {
            bridge_mm: 2.0,
            min_contact_mm: 0.0,
            per_island: 4,
            plate_top: 5.0,
            raised: false,
            ..Default::default()
        };
        let (out, _, after, contacts) =
            bridge_piece(m.clone(), [0.0; 2], &[[10.0, 0.0, 20.0, 30.0]], None, &bp).unwrap();
        assert_eq!(after, 2);
        assert!((out.volume() - m.volume()).abs() < 1e-6);
        assert_eq!(contacts.counts.len(), 1);
        assert_eq!(contacts.counts[0].1, 0);
        assert_eq!(count_warning(&contacts, &bp).len(), 1);
    }

    #[test]
    fn every_spacing_mode_keeps_the_configured_angle() {
        for spacing in ["closest", "spread", "aligned"] {
            let p = P(serde_json::from_value(json!({
                "bridge_spacing": spacing, "bridge_min_angle_deg": 65.0,
            }))
            .unwrap());
            let bp = bridge_params(&p, 5.0, 0.2, 5.0, false);
            assert_eq!(bp.min_angle_deg, 65.0);
            assert_eq!(
                bp.spacing,
                match spacing {
                    "aligned" => BridgeSpacing::Aligned,
                    "spread" => BridgeSpacing::Spread,
                    _ => BridgeSpacing::Closest,
                }
            );
        }
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
    fn aligned_plans_each_tile_independently_and_keeps_global_pockets_open() {
        let mut mesh = Mesh::cuboid([0.0; 3], [8.0, 40.0, 5.0]);
        for x in [20.0, 40.0] {
            mesh.append(&Mesh::cuboid([x, 15.0, 0.0], [x + 8.0, 25.0, 5.0]));
        }
        let stl = mesh.to_stl();
        let parts = Parts {
            files: vec![
                ("blocked.stl".into(), stl.clone()),
                ("free.stl".into(), stl.clone()),
                ("rod.stl".into(), stl.clone()),
            ],
            meta: json!({"plate": {
                "top": 5.0, "layer": 0.2, "thickness": 5.0, "flip": false,
                "connectors": [[8.0, 0.0, 20.0, 40.0]],
                "parts": [
                    {"file": "blocked.stl", "role": "tile", "origin": [0.0, 0.0], "size": [48.0, 40.0]},
                    {"file": "free.stl", "role": "tile", "origin": [100.0, 0.0], "size": [48.0, 40.0]},
                ],
            }}),
        };
        let mut inputs = HashMap::new();
        inputs.insert("body".into(), Value::Parts(Rc::new(parts)));
        let params = P(serde_json::from_value(json!({
            "bridge_spacing": "aligned", "bridge_mm": 2.0, "bridges_per_island": 1,
            "bridge_min_angle_deg": 45.0, "bridge_max_length": 4.0,
        }))
        .unwrap());
        let outputs = run_bridges(&Engine::new(), &inputs, &params).unwrap();
        let Value::Parts(out) = &outputs[0].1 else {
            panic!()
        };
        assert_eq!(pieces(out, "blocked.stl"), 3);
        assert_eq!(pieces(out, "free.stl"), 1);
        assert_eq!(out.files[2].1, stl);
        assert_eq!(out.meta["bridged_fragments"]["islands_left"], 2);
        assert_eq!(out.meta["bridged_fragments"]["tiles_bridged"], 1);
        let free = Mesh::from_stl(&out.files[1].1).unwrap();
        assert!(free.is_watertight());
        let (lo, hi) = free.bounds();
        assert!(lo[0] >= 0.0 && hi[0] <= 48.0 && lo[1] >= 0.0 && hi[1] <= 40.0);
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
    fn reinforced_ends_meet_the_target_without_widening_the_whole_span() {
        let mut m = Mesh::cuboid([0.0; 3], [30.0, 30.0, 5.0]);
        m.append(&Mesh::cuboid([50.0, 5.0, 0.0], [60.0, 25.0, 5.0]));
        for flip in [false, true] {
            for easy in [false, true] {
                let bp = BridgeParams {
                    bridge_mm: 2.0,
                    min_contact_mm: 3.0,
                    raised: true,
                    easy,
                    base_layers: 25,
                    layer_height: 0.2,
                    plate_top: 5.0,
                    flip,
                    ..Default::default()
                };
                let (out, _, after, contacts) =
                    bridge_piece(m.clone(), [0.0; 2], &[], None, &bp).unwrap();
                assert_eq!(after, 1);
                assert!(out.is_watertight());
                assert!(!contacts.widths.is_empty());
                assert_eq!(
                    contacts.weak(3.0),
                    0,
                    "flip={flip}, easy={easy}: {:?}",
                    contacts.widths
                );
                assert!(out.volume() > m.volume());
                let mut mid = Vec::new();
                for t in &out.t {
                    for j in 0..3 {
                        let (a, b) = (out.v[t[j]], out.v[t[(j + 1) % 3]]);
                        if (a[0] > 40.0) != (b[0] > 40.0) {
                            let f = (40.0 - a[0]) / (b[0] - a[0]);
                            mid.push(a[1] + f * (b[1] - a[1]));
                        }
                    }
                }
                assert!(!mid.is_empty());
                let width = mid.iter().copied().fold(f64::MIN, f64::max)
                    - mid.iter().copied().fold(f64::MAX, f64::min);
                assert!(width <= 2.01, "middle of the span widened to {width}");
            }
        }
    }

    #[test]
    fn impossible_contact_targets_are_reported_per_tile() {
        let mut m = Mesh::cuboid([0.0; 3], [30.0, 30.0, 5.0]);
        m.append(&Mesh::cuboid([40.0, 14.5, 0.0], [40.5, 15.0, 5.0]));
        let out = run(
            &m,
            json!({
                "bridge_mm": 1.0, "bridge_min_contact_mm": 3.0,
                "bridges_per_island": 1, "raised_bridges": false
            }),
        );
        let report = &out.meta["bridged_fragments"];
        assert_eq!(report["islands_left"], 0);
        assert!(
            report["tiles"][0]["bridge_contacts"]["weak_contacts"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert!(report["warnings"][0]
            .as_str()
            .unwrap()
            .starts_with("stencil_r1c1.stl:"));
    }

    #[test]
    fn reinforced_ends_do_not_fill_connector_pockets() {
        let mut m = Mesh::cuboid([0.0; 3], [10.0, 30.0, 5.0]);
        m.append(&Mesh::cuboid([20.0, 5.0, 0.0], [30.0, 25.0, 5.0]));
        let pocket = [9.0, 9.0, 21.0, 12.0];
        let bp = BridgeParams {
            bridge_mm: 2.0,
            min_contact_mm: 3.0,
            raised: false,
            base_layers: 25,
            layer_height: 0.2,
            plate_top: 5.0,
            ..Default::default()
        };
        let (out, _, after, _) = bridge_piece(m.clone(), [0.0; 2], &[pocket], None, &bp).unwrap();
        assert_eq!(after, 1);
        assert!(out.is_watertight());
        let keep_open = Mesh::cuboid([pocket[0], pocket[1], 0.0], [pocket[2], pocket[3], 5.0]);
        let added_in_pocket =
            out.intersect(&keep_open).unwrap().volume() - m.intersect(&keep_open).unwrap().volume();
        assert!(added_in_pocket.abs() < 1e-4, "{added_in_pocket}");
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

    /// A tile whose high side is a 30 degree bevel, with an island right against that face: the strip that
    /// joins it runs along the face, and its lower half would stick through the bevel without the leaning clip.
    #[test]
    fn bridges_end_on_the_bevelled_face_of_the_tile() {
        let (lean, inset, top) = (30f64.to_radians().tan(), 0.1, 6.0);
        let sides = Sides {
            size: [60.0, 40.0],
            seam: [false, true, false, false],
            lean,
            inset,
            z_mid: top / 2.0,
        };
        let tile = sides
            .clip(top)
            .intersect(&Mesh::cuboid([0.0, 0.0, 0.0], [70.0, 40.0, top]))
            .unwrap();
        let part = |x0: f64, x1: f64, y0: f64, y1: f64| {
            tile.intersect(&Mesh::cuboid([x0, y0, -1.0], [x1, y1, top + 1.0]))
                .unwrap()
        };
        let mut m = part(0.0, 70.0, 20.0, 40.0);
        m.append(&part(60.0, 70.0, 0.0, 10.0));
        let parts = Parts {
            files: vec![("stencil_r1c1.stl".into(), m.to_stl())],
            meta: json!({
                "tiling": {"seam_angle_deg": 30.0, "seam_gap_mm": 0.2},
                "plate": {"top": top, "layer": 0.2, "thickness": top, "flip": false, "connectors": [],
                    "seams": [[0.0, 60.0, 0.0, 40.0]],
                    "parts": [{"file": "stencil_r1c1.stl", "role": "tile", "origin": [0.0, 0.0], "size": [60.0, 40.0]}]},
            }),
        };
        let mut i = HashMap::new();
        i.insert("body".to_string(), Value::Parts(Rc::new(parts)));
        let p = P(serde_json::from_value(
            json!({"bridge_max_length": 20.0, "raised_bridges": false}),
        )
        .unwrap());
        let Value::Parts(out) = &run_bridges(&Engine::new(), &i, &p).unwrap()[0].1 else {
            panic!()
        };
        let m = Mesh::from_stl(&out.files[0].1).unwrap();
        assert_eq!(components(&m).0, 1, "the island is joined");
        for v in &m.v {
            let face = 60.0 - inset + lean * (v[2] - top / 2.0);
            assert!(
                v[0] <= face + 2e-3,
                "vertex at z {:.2} is {:.3} mm through the bevel",
                v[2],
                v[0] - face
            );
        }
    }
}
