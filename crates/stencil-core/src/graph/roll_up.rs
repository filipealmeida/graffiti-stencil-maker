//! Experimental roll-up assembly adapter.

use super::{types::Outputs, Engine, Parts, Solid, Value, P};
use crate::{hinge, mesh::Mesh};
use serde_json::{json, Value as Json};
use std::{collections::HashMap, rc::Rc};

const WARNING: &str = "Experimental hinges: test a small print for free movement and paint leakage. Raised horizontal beads need slicer support assessment; place removable supports under unsupported bead undersides, not inside working clearances. No support-free or paint-tight guarantee. Remove stiffener rails before rolling.";

struct Assembly {
    mesh: Mesh,
    parts: Vec<Mesh>,
    rails: Vec<(String, Vec<u8>)>,
    report: Json,
}

fn checked_stl(mut mesh: Mesh, name: &str) -> Result<Vec<u8>, String> {
    mesh.quantize_f32();
    if !mesh.is_watertight() || mesh.volume() <= 0.0 {
        return Err(format!(
            "verification failed: {name} is not watertight with positive volume"
        ));
    }
    Ok(mesh.to_stl())
}

fn make_slats(
    mesh: &Mesh,
    cuts: &[(Mesh, Mesh)],
    positions: &[f64],
    hp: &hinge::Params,
) -> Result<Vec<Mesh>, String> {
    let (lo, hi) = mesh.bounds();
    let joint = |cut: &Mesh, position: f64| -> Result<Mesh, String> {
        let (a, b) = cut.bounds();
        let mut from = [lo[0] - 1.0, lo[1] - 1.0, a[2] - 1.0];
        let mut to = [hi[0] + 1.0, hi[1] + 1.0, b[2] + 1.0];
        from[hp.axis] = position - hp.reach() - 0.1;
        to[hp.axis] = position + hp.reach() + 0.1;
        cut.intersect(&Mesh::cuboid(from, to))
    };
    let mut slats = Vec::new();
    for k in 0..=cuts.len() {
        let mut slat = mesh.clone();
        let mut additions = Mesh::default();
        if k > 0 {
            slat = slat.intersect(&cuts[k - 1].1)?;
            // Retain the raised joint from each side without subtracting coplanar STL faces.
            additions = additions.union(&joint(&cuts[k - 1].1, positions[k - 1])?)?;
        }
        if k < cuts.len() {
            slat = slat.intersect(&cuts[k].0)?;
            additions = additions.union(&joint(&cuts[k].0, positions[k])?)?;
        }
        slat = slat.union(&additions)?;
        if crate::tile_bridge::components(&slat).0 != 1 {
            return Err(format!("slat {} has loose pieces after cutting: move seams, change pitch or add bridges before hinging", k + 1));
        }
        slats.push(slat);
    }
    Ok(slats)
}

fn assembly(mesh: &Mesh, thickness: f64, p: &P) -> Result<Assembly, String> {
    let hp = hinge::Params {
        axis: usize::from(p.s("axis") == "y"),
        thickness,
        clearance: p.f("clearance"),
        paint_out: p.b("paint_out"),
    };
    hp.check()?;
    if crate::tile_bridge::components(mesh).0 != 1 {
        return Err("join the stencil islands with Bridges before adding roll-up seams".into());
    }
    if !hp.paint_out && p.b("stiffeners") {
        return Err("stiffener rails currently support paint-face-out rolling only; turn off stiffeners for paint-face-in".into());
    }
    let (lo, hi) = mesh.bounds();
    let length = hi[hp.axis] - lo[hp.axis];
    let pitch = p.f("pitch");
    if pitch < 2.0 * hp.reach() + 1.0 {
        return Err("hinge pitch is too small for the barrels and their attachment walls".into());
    }
    let count = (length / pitch).ceil() as usize;
    if !(2..=200).contains(&count) {
        return Err("roll-up seams need 2 to 200 slats; reduce pitch for a small plate or increase it for a large one".into());
    }
    let step = length / count as f64;
    if step < 2.0 * hp.reach() + 1.0 {
        return Err("the evenly spaced slats would be too narrow: increase the pitch".into());
    }
    let mut seams = Vec::new();
    let mut cuts = Vec::new();
    let mut positions = Vec::new();
    let max_shift = p.f("seam_shift").min((step - 2.0 * hp.reach() - 1.0) / 2.0);
    for k in 1..count {
        let nominal = lo[hp.axis] + k as f64 * step;
        let mut best: Option<(f64, f64, Vec<[f64; 2]>)> = None;
        let mut last_error = String::new();
        for j in -4i32..=4 {
            let position = nominal + max_shift * j as f64 / 4.0;
            match hinge::runs(mesh, position, &hp) {
                Ok(runs) => {
                    let coverage: f64 = runs.iter().map(|r| r[1] - r[0]).sum();
                    // More safe material wins; keep the nominal seam when tied.
                    let score = coverage - (position - nominal).abs() * 1e-6;
                    if best.as_ref().is_none_or(|b| score > b.0) {
                        best = Some((score, position, runs));
                    }
                }
                Err(e) => last_error = e,
            }
        }
        let Some((_, position, runs)) = best else {
            return Err(format!("seam {k}: {last_error}"));
        };
        let coverage: f64 = runs.iter().map(|r| r[1] - r[0]).sum();
        seams.push(json!({"axis": hp.axis, "position": position, "runs": runs,
            "material_coverage_percent": coverage / (hi[1 - hp.axis] - lo[1 - hp.axis]) * 100.0}));
        positions.push(position);
        cuts.push(hinge::split(mesh, position, &hp, true)?);
        crate::progress::step("make experimental hinges", k as f64 / count as f64 * 0.8);
    }
    let mut slats = make_slats(mesh, &cuts, &positions, &hp)?;
    let maximum = p.f("max_assembly_length");
    let mut groups = Vec::new();
    let mut start = 0;
    let mut min = f64::MAX;
    let mut max = f64::MIN;
    for (k, slat) in slats.iter().enumerate() {
        let (a, b) = slat.bounds();
        if b[hp.axis] - a[hp.axis] > maximum + 1e-5 {
            return Err("a slat exceeds the largest printed assembly: increase that limit or reduce hinge pitch".into());
        }
        let (next_min, next_max) = (min.min(a[hp.axis]), max.max(b[hp.axis]));
        if k > start && next_max - next_min > maximum + 1e-5 {
            groups.push(start..k);
            start = k;
            min = a[hp.axis];
            max = b[hp.axis];
        } else {
            min = next_min;
            max = next_max;
        }
    }
    groups.push(start..count);
    for seam in &mut seams {
        seam["joint"] = json!("print-in-place");
    }
    for group in groups.iter().skip(1) {
        let k = group.start - 1;
        cuts[k] = hinge::split(mesh, positions[k], &hp, false)?;
        seams[k]["joint"] = json!("slide-in");
    }
    if groups.len() > 1 {
        slats = make_slats(mesh, &cuts, &positions, &hp)?;
    }
    let mut rails = Vec::new();
    if p.b("stiffeners") {
        let (runners, rail_files) = stiffeners(mesh, &positions, &hp, p.f("rail_clearance"))?;
        for (slat, runner) in slats.iter_mut().zip(runners) {
            *slat = slat.union(&runner)?;
        }
        rails = rail_files;
    }
    let mut parts = Vec::new();
    for group in &groups {
        let mut part = Mesh::default();
        for slat in &slats[group.clone()] {
            part.append(slat);
        }
        parts.push(part);
    }
    if groups.len() > 1 && !rails.is_empty() {
        let full_rails = rails;
        rails = Vec::new();
        for (k, group) in groups.iter().enumerate() {
            let start = if group.start == 0 {
                lo[hp.axis]
            } else {
                positions[group.start - 1] + hp.clearance / 2.0
            };
            let end = if group.end == count {
                hi[hp.axis]
            } else {
                positions[group.end - 1] - hp.clearance / 2.0
            };
            for (name, stl) in &full_rails {
                let full = Mesh::from_stl(stl)?;
                let (mut clip_lo, mut clip_hi) = full.bounds();
                for axis in 0..3 {
                    clip_lo[axis] -= 1.0;
                    clip_hi[axis] += 1.0;
                }
                clip_lo[hp.axis] = start - lo[hp.axis];
                clip_hi[hp.axis] = end - lo[hp.axis];
                let mut rail = full.intersect(&Mesh::cuboid(clip_lo, clip_hi))?;
                let origin = rail.bounds().0;
                rail.translate([-origin[0], -origin[1], -origin[2]]);
                rails.push((
                    format!("assembly_{}_{name}", k + 1),
                    checked_stl(rail, "stiffener")?,
                ));
            }
        }
    }
    let mut combined = Mesh::default();
    for part in &parts {
        let (a, b) = part.bounds();
        if b[hp.axis] - a[hp.axis] > maximum + 1e-4 {
            return Err("assembly verification failed: printed hinge geometry exceeds the configured length".into());
        }
        combined.append(part);
    }
    let mut limits = vec![lo[hp.axis]];
    limits.extend(positions);
    limits.push(hi[hp.axis]);
    let widest = limits.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max);
    let minimum_core = widest / (hinge::FOLD_DEG.to_radians() / 2.0).sin();
    let core = p.f("core_diameter").max(minimum_core);
    let turn_angle = 2.0 * (widest / core).asin().to_degrees();
    // Envelope bound includes raised barrels and runners, not just plate thickness.
    let envelope = combined.bounds().1[2] - combined.bounds().0[2];
    let outer =
        (core * core + 4.0 * length * envelope / std::f64::consts::PI).sqrt() + 2.0 * envelope;
    let rail_count = rails.len();
    let couplers = if rail_count > 0 {
        2 * (groups.len() - 1)
    } else {
        0
    };
    if couplers > 0 {
        rails.push((
            "stiffener_coupler.stl".into(),
            checked_stl(
                stiffener_coupler(hp.axis, p.f("rail_clearance"))?,
                "stiffener coupler",
            )?,
        ));
    }
    let report = json!({
        "experimental": true, "pitch_mm": pitch, "actual_pitch_mm": step,
        "axis": hp.axis, "slats": count, "seams": seams,
        "axis_height_mm": hp.axis_height(), "hinge_height_mm": hp.back_height(),
        "fold_limit_deg": hinge::FOLD_DEG, "required_fold_deg": turn_angle,
        "minimum_core_diameter_mm": minimum_core, "core_diameter_mm": core,
        "estimated_outer_diameter_mm": outer, "diameter_is_estimate": true,
        "printed_assemblies": parts.len(), "maximum_assembly_length_mm": maximum,
        "slide_in_joints": groups.len() - 1, "stiffeners": rail_count,
        "stiffener_couplers": couplers, "warnings": [
            WARNING,
            if core > p.f("core_diameter") { "Requested roll core is too small for the fold limit; the reported core diameter was increased." } else { "Roll diameter is an envelope estimate, not a collision-checked spiral simulation." },
            if hp.paint_out { "Raised barrels and runners are on the back; print the flat paint face on the bed." } else { "Paint-face-in mirrors the barrels below Z=0; reorient the assembly in the slicer and verify overhangs." }
        ],
    });
    let mut report = report;
    if groups.len() > 1 {
        report["warnings"].as_array_mut().unwrap().push(json!("Print-boundary joints slide together axially; secure them with removable tape/clamps because they have no integral end locks."));
        if couplers > 0 {
            report["warnings"].as_array_mut().unwrap().push(json!(format!("Print {couplers} copies of stiffener_coupler.stl; slide one across each rail junction to lock the split boundary flat. Remove couplers and rails before rolling.")));
        }
    }
    Ok(Assembly {
        mesh: combined,
        parts,
        rails,
        report,
    })
}

fn stiffeners(
    mesh: &Mesh,
    positions: &[f64],
    hp: &hinge::Params,
    clearance: f64,
) -> Result<(Vec<Mesh>, Vec<(String, Vec<u8>)>), String> {
    let (lo, hi) = mesh.bounds();
    let axis = hp.axis;
    let along = 1 - axis;
    let mut limits = vec![lo[axis]];
    limits.extend_from_slice(positions);
    limits.push(hi[axis]);
    let mut runners = vec![Mesh::default(); limits.len() - 1];
    let top = hp.back_height() + 2.2;
    let xyz = |x, y, z| if axis == 0 { [x, y, z] } else { [y, x, z] };
    let b = |x0, x1, y0, y1, z0, z1| Mesh::cuboid(xyz(x0, y0, z0), xyz(x1, y1, z1));
    let mut files = Vec::new();
    for (side, centre) in [lo[along] + 3.0, hi[along] - 3.0].into_iter().enumerate() {
        for (k, w) in limits.windows(2).enumerate() {
            let a = w[0] + if k == 0 { 0.5 } else { hp.reach() + 0.5 };
            let end = w[1]
                - if k + 2 == limits.len() {
                    0.5
                } else {
                    hp.reach() + 0.5
                };
            if end - a < 2.0 {
                return Err("slats are too narrow for stiffener runners: increase pitch or turn off stiffeners".into());
            }
            if !mesh.box_inside(
                xyz(a, centre - 2.0, 1e-4),
                xyz(end, centre + 2.0, hp.thickness - 1e-4),
            ) {
                return Err("stiffener runners need two continuous blank border strips at least 5 mm wide; enlarge the margin or turn off stiffeners".into());
            }
            let stem = b(
                a,
                end,
                centre - 1.0,
                centre + 1.0,
                hp.thickness - 0.1,
                top + 0.5,
            );
            let head = b(a, end, centre - 2.0, centre + 2.0, top, top + 1.2);
            runners[k] = runners[k].union(&stem.union(&head)?)?;
        }
        let a = lo[axis];
        let end = hi[axis];
        let mut rail = b(
            a,
            end,
            centre - 3.0 - clearance,
            centre + 3.0 + clearance,
            top - 0.8,
            top + 2.0 + clearance,
        );
        rail = rail.subtract(&b(
            a - 0.1,
            end + 0.1,
            centre - 2.0 - clearance,
            centre + 2.0 + clearance,
            top - clearance,
            top + 1.2 + clearance,
        ))?;
        rail = rail.subtract(&b(
            a - 0.1,
            end + 0.1,
            centre - 1.0 - clearance,
            centre + 1.0 + clearance,
            top - 1.0,
            top + 0.1,
        ))?;
        let origin = rail.bounds().0;
        rail.translate([-origin[0], -origin[1], -origin[2]]);
        files.push((
            format!("stiffener_{}.stl", side + 1),
            checked_stl(rail, "stiffener")?,
        ));
    }
    Ok((runners, files))
}

fn stiffener_coupler(axis: usize, c: f64) -> Result<Mesh, String> {
    let width = 6.0 + 2.0 * c;
    let height = 2.8 + c;
    let wall = 0.8;
    let centre = width / 2.0 + c + wall;
    let b = |a, b, z0, z1| {
        if axis == 0 {
            Mesh::cuboid([0.0, a, z0], [12.0, b, z1])
        } else {
            Mesh::cuboid([a, 0.0, z0], [b, 12.0, z1])
        }
    };
    let outer = b(0.0, 2.0 * centre, 0.0, height + 2.0 * c + 2.0 * wall);
    let inner = b(wall, 2.0 * centre - wall, wall, wall + height + 2.0 * c);
    let slot = b(centre - 1.0 - c, centre + 1.0 + c, -0.1, wall + 0.1);
    outer.subtract(&inner)?.subtract(&slot)
}

pub(super) fn run(_: &Engine, inputs: &HashMap<String, Value>, p: &P) -> Result<Outputs, String> {
    match inputs.get("body") {
        Some(Value::Solid(s)) => {
            let mesh = Mesh::from_stl(&s.stl)?;
            let thickness = s.stats["plate_top_mm"]
                .as_f64()
                .unwrap_or(mesh.bounds().1[2]);
            let Assembly {
                mesh,
                parts,
                rails,
                report,
            } = assembly(&mesh, thickness, p)?;
            let (lo, hi) = mesh.bounds();
            let stl = checked_stl(mesh, "roll-up assembly")?;
            let mut stats = s.stats.clone();
            stats["roll_up"] = report.clone();
            stats["watertight"] = json!(true);
            stats["triangles"] = json!((stl.len() - 84) / 50);
            stats["stl_bytes"] = json!(stl.len());
            let mut files = Vec::new();
            let mut entries = Vec::new();
            let count = parts.len();
            for (k, mut part) in parts.into_iter().enumerate() {
                let name = if count == 1 {
                    "roll_up_stencil.stl".to_string()
                } else {
                    format!("roll_up_stencil_{}.stl", k + 1)
                };
                let (a, b) = part.bounds();
                entries.push(json!({"file": name, "role": "hinged_assembly",
                    "origin": [a[0], a[1]], "size": [b[0] - a[0], b[1] - a[1]]}));
                part.translate([-a[0], -a[1], 0.0]);
                files.push((name.clone(), checked_stl(part, &name)?));
            }
            let parts = Parts {
                files,
                meta: json!({"roll_up": report, "plate": {
                    "w": hi[0] - lo[0], "h": hi[1] - lo[1], "top": thickness,
                    "thickness": thickness, "layer": s.stats["layer_height_mm"],
                    "margin": s.stats["margin_mm"], "flip": s.stats["flip"],
                    "parts": entries,
                }}),
            };
            let mut outputs = vec![
                ("solid", Value::Solid(Rc::new(Solid { stl, stats }))),
                ("parts", Value::Parts(Rc::new(parts))),
            ];
            if !rails.is_empty() {
                outputs.push((
                    "stiffeners",
                    Value::Parts(Rc::new(Parts {
                        files: rails,
                        meta: json!({"experimental": true}),
                    })),
                ));
            }
            Ok(outputs)
        }
        Some(Value::Parts(src)) => {
            let entries = src.meta["plate"]["parts"]
                .as_array()
                .ok_or("parts have no tile layout: connect Split volume or a tiled Stencil")?;
            let mut files = Vec::new();
            let mut new_entries = Vec::new();
            let mut rail_files = Vec::new();
            let mut reports = Vec::new();
            let mut meta = src.meta.clone();
            for (k, e) in entries.iter().enumerate() {
                if e["role"] != "tile" {
                    new_entries.push(e.clone());
                    continue;
                }
                let name = e["file"].as_str().ok_or("tile metadata has no filename")?;
                let f = src
                    .files
                    .iter()
                    .find(|(n, _)| n == name)
                    .ok_or_else(|| format!("tile {name} is missing"))?;
                let mesh = Mesh::from_stl(&f.1)?;
                let thickness = src.meta["plate"]["top"]
                    .as_f64()
                    .or_else(|| src.meta["plate"]["thickness"].as_f64())
                    .ok_or("tile metadata has no plate thickness")?;
                let Assembly {
                    parts,
                    rails,
                    report,
                    ..
                } = assembly(&mesh, thickness, p).map_err(|e| format!("{name}: {e}"))?;
                let count = parts.len();
                for (part_index, mut part) in parts.into_iter().enumerate() {
                    let file = if count == 1 {
                        name.to_string()
                    } else {
                        format!(
                            "{}_roll_{}.stl",
                            name.strip_suffix(".stl").unwrap_or(name),
                            part_index + 1
                        )
                    };
                    let (a, b) = part.bounds();
                    let origin_x = e["origin"][0]
                        .as_f64()
                        .ok_or("tile metadata has no X origin")?;
                    let origin_y = e["origin"][1]
                        .as_f64()
                        .ok_or("tile metadata has no Y origin")?;
                    new_entries.push(json!({"file": file, "role": "hinged_assembly",
                        "origin": [origin_x + a[0], origin_y + a[1]],
                        "size": [b[0] - a[0], b[1] - a[1]]}));
                    part.translate([-a[0], -a[1], 0.0]);
                    files.push((file.clone(), checked_stl(part, &file)?));
                }
                for (rail, stl) in rails {
                    rail_files.push((format!("tile_{}_{}", k + 1, rail), stl));
                }
                reports.push(json!({"file": name, "report": report}));
            }
            if reports.is_empty() {
                return Err("the parts contain no stencil tiles to hinge".into());
            }
            for (name, stl) in &src.files {
                if !entries
                    .iter()
                    .any(|e| e["role"] == "tile" && e["file"] == name.as_str())
                {
                    files.push((name.clone(), stl.clone()));
                }
            }
            meta["plate"]["parts"] = json!(new_entries);
            meta["roll_up"] = json!({"experimental": true, "tiles": reports, "warnings": [WARNING, "Each tile is a separate print-in-place assembly; existing split connectors are unchanged."]});
            let mut out = vec![("parts", Value::Parts(Rc::new(Parts { files, meta })))];
            if !rail_files.is_empty() {
                out.push((
                    "stiffeners",
                    Value::Parts(Rc::new(Parts {
                        files: rail_files,
                        meta: json!({"experimental": true}),
                    })),
                ));
            }
            Ok(out)
        }
        _ => Err("connect a solid or stencil parts to Roll-up seams".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> P {
        let def = super::super::catalogue::catalogue()
            .into_iter()
            .find(|d| d.id == "roll_up_seams")
            .unwrap();
        super::super::params::clean_params(&def, None)
    }
    #[test]
    fn roll_up_assembly_and_stiffeners_are_printable_meshes() {
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [60.0, 30.0, 3.0]);
        for axis in ["x", "y"] {
            let mut params = p();
            params.0.insert("axis".into(), json!(axis));
            let Assembly {
                mesh,
                rails,
                report,
                ..
            } = assembly(&plate, 3.0, &params).unwrap();
            assert_eq!(rails.len(), 2);
            assert_eq!(
                crate::tile_bridge::components(&mesh).0,
                report["slats"].as_u64().unwrap() as usize
            );
            assert!(Mesh::from_stl(&checked_stl(mesh, "test").unwrap())
                .unwrap()
                .is_watertight());
            assert!(report["required_fold_deg"].as_f64().unwrap() <= hinge::FOLD_DEG + 1e-6);
            for (_, stl) in rails {
                assert!(Mesh::from_stl(&stl).unwrap().is_watertight());
            }
        }
    }

    #[test]
    fn runners_do_not_block_folding_with_rails_removed() {
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [60.0, 30.0, 3.0]);
        let Assembly { mesh, report, .. } = assembly(&plate, 3.0, &p()).unwrap();
        let (n, owners) = crate::tile_bridge::components(&mesh);
        let mut slats = vec![Mesh::default(); n];
        for (t, &owner) in mesh.t.iter().zip(&owners) {
            let m = &mut slats[owner];
            let start = m.v.len();
            m.v.extend(t.iter().map(|&i| mesh.v[i]));
            m.t.push([start, start + 1, start + 2]);
        }
        for m in &mut slats {
            m.quantize_f32();
        }
        slats.sort_by(|a, b| a.bounds().0[0].total_cmp(&b.bounds().0[0]));
        let position = report["seams"][0]["position"].as_f64().unwrap();
        let z = report["axis_height_mm"].as_f64().unwrap();
        for deg in (0..=60).step_by(5) {
            let mut folded = slats[0].clone();
            let angle = (deg as f64).to_radians();
            for v in &mut folded.v {
                let x0 = v[0] - position;
                let z0 = v[2] - z;
                v[0] = position + x0 * angle.cos() + z0 * angle.sin();
                v[2] = z - x0 * angle.sin() + z0 * angle.cos();
            }
            assert!(
                folded.intersect(&slats[1]).unwrap().volume().abs() < 1e-5,
                "runner collision at {deg} degrees"
            );
        }
    }

    #[test]
    fn refuses_unbridged_and_raised_inputs() {
        let mut plate = Mesh::cuboid([0.0, 0.0, 0.0], [60.0, 30.0, 3.0]);
        plate.append(&Mesh::cuboid([80.0, 0.0, 0.0], [90.0, 10.0, 3.0]));
        assert!(assembly(&plate, 3.0, &p())
            .err()
            .unwrap()
            .contains("islands"));
        let raised = Mesh::cuboid([0.0, 0.0, 0.0], [60.0, 30.0, 4.0]);
        assert!(assembly(&raised, 3.0, &p())
            .err()
            .unwrap()
            .contains("flat plate"));
    }

    #[test]
    fn stiffener_couplers_fit_rails_at_clearance_extremes() {
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [60.0, 30.0, 3.0]);
        for axis in [0, 1] {
            for clearance in [0.15, 0.5] {
                let hp = hinge::Params {
                    axis,
                    thickness: 3.0,
                    clearance: 0.25,
                    paint_out: true,
                };
                let (_, rails) = stiffeners(&plate, &[], &hp, clearance).unwrap();
                let mut rail = Mesh::from_stl(&rails[0].1).unwrap();
                let coupler = stiffener_coupler(axis, clearance).unwrap();
                let mut offset = [0.0; 3];
                offset[1 - axis] = 0.8 + clearance;
                offset[2] = 0.8 + clearance;
                rail.translate(offset);
                assert!(rail.intersect(&coupler).unwrap().volume().abs() < 1e-5);
                offset = [0.0; 3];
                offset[1 - axis] = 2.0 * clearance;
                rail.translate(offset);
                assert!(rail.intersect(&coupler).unwrap().volume() > 0.01);
                assert!(
                    1.4 - 0.8 - clearance > 0.05,
                    "coupler must clear the raised barrel"
                );
            }
        }
    }

    #[test]
    fn print_sized_groups_keep_internal_hinges_and_slide_in_boundary_joints() {
        for axis in ["x", "y"] {
            let hi = if axis == "x" {
                [100.0, 30.0, 3.0]
            } else {
                [30.0, 100.0, 3.0]
            };
            let plate = Mesh::cuboid([0.0, 0.0, 0.0], hi);
            let mut params = p();
            params.0.insert("axis".into(), json!(axis));
            params.0.insert("max_assembly_length".into(), json!(65.0));
            let Assembly {
                mesh,
                parts,
                rails,
                report,
            } = assembly(&plate, 3.0, &params).unwrap();
            assert_eq!(parts.len(), 2);
            assert_eq!(rails.len(), 5);
            assert_eq!(report["stiffener_couplers"], 2);
            assert_eq!(report["slide_in_joints"], 1);
            assert_eq!(report["seams"][0]["joint"], "print-in-place");
            assert_eq!(report["seams"][2]["joint"], "slide-in");
            let coordinate = usize::from(axis == "y");
            for part in &parts {
                let (a, b) = part.bounds();
                assert!(b[coordinate] - a[coordinate] <= 65.0);
                assert!(Mesh::from_stl(&checked_stl(part.clone(), "group").unwrap())
                    .unwrap()
                    .is_watertight());
            }
            assert_eq!(crate::tile_bridge::components(&mesh).0, 5);
            // At the print boundary, axial sliding is possible (no integral caps).
            let mut low = parts[0].clone();
            let along = 1 - coordinate;
            let mut shift = [0.0; 3];
            shift[along] = 1.0;
            low.translate(shift);
            assert!(low.intersect(&parts[1]).unwrap().volume().abs() < 1e-5);
            for (_, stl) in rails {
                let rail = Mesh::from_stl(&stl).unwrap();
                assert!(rail.bounds().1[coordinate] <= 65.0);
            }
        }
    }

    #[test]
    fn preserves_non_tile_files_and_optional_outputs() {
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [60.0, 30.0, 3.0]);
        let hardware = Mesh::cuboid([0.0, 0.0, 0.0], [3.0, 3.0, 3.0]).to_stl();
        let mut inputs = HashMap::new();
        inputs.insert("body".into(), Value::Parts(Rc::new(Parts {
            files: vec![("tile.stl".into(), plate.to_stl()), ("rod.stl".into(), hardware.clone())],
            meta: json!({"plate": {"thickness": 3.0, "parts": [
                {"file": "tile.stl", "role": "tile", "origin": [0.0, 0.0], "size": [60.0, 30.0]},
                {"file": "rod.stl", "role": "connector"}
            ]}}),
        })));
        let mut params = p();
        params.0.insert("stiffeners".into(), json!(false));
        let out = run(&Engine::new(), &inputs, &params).unwrap();
        assert_eq!(out.len(), 1);
        let Value::Parts(parts) = &out[0].1 else {
            panic!()
        };
        assert_eq!(parts.files[1].1, hardware);
        assert_eq!(parts.meta["plate"]["parts"][0]["role"], "hinged_assembly");
        assert_eq!(parts.meta["roll_up"]["experimental"], true);
    }

    #[test]
    fn catalogue_marks_node_experimental_and_guard_protects_joints() {
        let defs = super::super::catalogue::node_types();
        let roll = defs
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["id"] == "roll_up_seams")
            .unwrap();
        assert_eq!(roll["experimental"], true);
        let mut e = Engine::new();
        let image = e.add_image(br#"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="30"><rect width="60" height="30" fill="white"/></svg>"#.to_vec()).unwrap();
        let graph = json!({"version": 2, "nodes": [
            {"id": "image", "type": "source", "params": {"image_id": image["id"]}},
            {"id": "threshold", "type": "threshold"},
            {"id": "plate", "type": "stencil", "params": {"width_mm": 60, "thickness_mm": 3}},
            {"id": "roll", "type": "roll_up_seams"},
            {"id": "bridges", "type": "bridges"}
        ], "edges": [
            {"from": ["image", "image"], "to": ["threshold", "image"]},
            {"from": ["threshold", "mask"], "to": ["plate", "mask"]},
            {"from": ["plate", "solid"], "to": ["roll", "body"]},
            {"from": ["roll", "solid"], "to": ["bridges", "body"]}
        ]});
        let result = e.run(&graph, None, &|| 0.0);
        assert_eq!(result["nodes"]["roll"]["state"], "done", "{result}");
        assert_eq!(result["nodes"]["bridges"]["state"], "error");
        assert!(result["nodes"]["bridges"]["error"]
            .as_str()
            .unwrap()
            .contains("hinges"));
        assert!(e.report(&graph, &result)["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["text"]
                .as_str()
                .is_some_and(|s| s.contains("Experimental hinges"))));
    }
}
