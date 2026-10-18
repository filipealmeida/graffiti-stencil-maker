//! Remesh: slices a stencil solid into cross-sections, simplifies the chosen layers
//! (fewer vertices, rounding towards material) without letting the layer above overhang more than
//! the allowed angle, rebuilds a watertight mesh, then decimates flat faces and straight edges.

use crate::mesh::Mesh;
use i_triangle::i_overlay::core::fill_rule::FillRule;
use i_triangle::i_overlay::core::overlay_rule::OverlayRule;
use i_triangle::i_overlay::core::single::SingleIntOverlay;
use i_triangle::i_overlay::i_float::int::point::IntPoint;
use i_triangle::i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_triangle::i_overlay::mesh::float::style::OutlineStyle;
use i_triangle::int::triangulatable::IntTriangulatable;
use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap, HashMap, HashSet};

const U: f64 = 10_000.0; // integer units per mm
const Z_TOL: f64 = 2e-4; // mm: heights closer than this are the same plane
const NOISE_UNITS2: f64 = 1e6; // 0.01 mm²: overlap slivers below this are rounding noise

type Contour = Vec<IntPoint>;
type Shape = Vec<Contour>;

mod rebuild;
mod simplify;
pub(crate) use rebuild::build;
use rebuild::*;
use simplify::*;
pub(crate) type Region = Vec<Shape>;

pub struct Params {
    pub layer_height: f64,
    pub from: u32,
    /// 0 = up to the top layer.
    pub to: u32,
    pub tolerance_mm: f64,
    /// 90 or more switches the overhang rule off.
    pub max_overhang_deg: f64,
    pub keep_material: bool,
    /// 0..1: how strongly outlines are smoothed before they are simplified.
    pub smoothing: f64,
    /// Largest surface shift allowed when decimating the mesh (mm); 0 switches decimation off.
    pub decimate_mm: f64,
}

pub struct Report {
    pub layers: u32,
    pub from: u32,
    pub to: u32,
    pub vertices_before: usize,
    pub vertices_after: usize,
    pub triangles_before: usize,
    pub triangles_after: usize,
    pub max_deviation_mm: f64,
    pub overhang_violation_mm2: f64,
    /// The tolerance actually applied (it is halved when the rebuild fails).
    pub tolerance_mm: f64,
    /// Set when the input was returned untouched, with the reason.
    pub skipped: Option<String>,
    /// Triangles after the layer step, before decimation.
    pub triangles_layers: usize,
    pub decimate_mm: f64,
}

fn flat(r: &Region) -> Vec<Contour> {
    r.iter().flatten().cloned().collect()
}

fn boolean(a: &Region, b: &Region, rule: OverlayRule) -> Region {
    match (a.is_empty(), b.is_empty(), rule) {
        (true, true, _) => vec![],
        (true, false, OverlayRule::Union) => b.clone(),
        (true, _, _) => vec![],
        (false, true, OverlayRule::Intersect) => vec![],
        (false, true, _) => a.clone(),
        _ => flat(a)
            .as_slice()
            .overlay(flat(b).as_slice(), rule, FillRule::NonZero),
    }
}

fn normalize(contours: Vec<Contour>) -> Region {
    if contours.is_empty() {
        return vec![];
    }
    let none: &[Contour] = &[];
    contours
        .as_slice()
        .overlay(none, OverlayRule::Union, FillRule::NonZero)
}

fn area2(c: &[IntPoint]) -> f64 {
    let n = c.len();
    (0..n)
        .map(|i| {
            let (a, b) = (c[i], c[(i + 1) % n]);
            a.x as f64 * b.y as f64 - b.x as f64 * a.y as f64
        })
        .sum()
}

/// Area in square units of a region (holes subtract).
fn region_area(r: &Region) -> f64 {
    r.iter()
        .map(|s| s.iter().map(|c| area2(c) / 2.0).sum::<f64>())
        .sum()
}

fn drop_noise(r: Region) -> Region {
    r.into_iter()
        .filter(|s| s.iter().map(|c| area2(c) / 2.0).sum::<f64>() >= NOISE_UNITS2)
        .collect()
}

pub(crate) fn grow(r: &Region, d: f64) -> Region {
    if r.is_empty() || d < 0.5 {
        return r.clone();
    }
    let style = OutlineStyle::new(d);
    let mut out: Vec<Contour> = Vec::new();
    for shape in r {
        let fl: Vec<Vec<[f64; 2]>> = shape
            .iter()
            .map(|c| c.iter().map(|p| [p.x as f64, p.y as f64]).collect())
            .collect();
        for s in fl.outline(&style) {
            for c in s {
                out.push(
                    c.iter()
                        .map(|p| IntPoint::new(p[0].round() as i32, p[1].round() as i32))
                        .collect(),
                );
            }
        }
    }
    normalize(out)
}

pub fn reduce(stl: &[u8], p: &Params) -> Result<(Vec<u8>, Report), String> {
    if !(p.layer_height.is_finite() && p.layer_height > 0.0) {
        return Err("layer height must be positive".into());
    }
    if !(p.tolerance_mm.is_finite() && p.tolerance_mm > 0.0) {
        return Err("tolerance must be positive".into());
    }
    let input = Mesh::from_stl(stl)?;
    let (pieces_before, _) = crate::tile_bridge::components(&input);
    crate::progress::step("slice layers", 0.1);
    let (planes, slabs) = slice(&input)?;
    let (zmin, zmax) = (planes[0], planes[planes.len() - 1]);
    let layers = (((zmax - zmin) / p.layer_height).round() as u32).max(1);
    let (from, to) = (
        p.from.max(1),
        if p.to == 0 { layers } else { p.to.min(layers) },
    );
    if from > to {
        return Err(format!(
            "the layer interval {from}-{to} is empty: the model has {layers} layers"
        ));
    }
    let zlo = zmin + (from - 1) as f64 * p.layer_height;
    let zhi = if to == layers {
        zmax
    } else {
        zmin + to as f64 * p.layer_height
    };

    let mut cuts = planes.clone();
    for z in [zlo, zhi] {
        if cuts.iter().all(|c| (c - z).abs() > Z_TOL) {
            cuts.push(z);
        }
    }
    cuts.sort_by(f64::total_cmp);
    let mut result = None;
    for scale in [1.0, 0.5, 0.25] {
        let tol = p.tolerance_mm * scale * U;
        let mut deviation: f64 = 0.0;
        crate::progress::step("simplify layers", 0.3);
        let windows: Vec<(f64, f64, usize, bool)> = cuts
            .windows(2)
            .map(|w| {
                let mid = (w[0] + w[1]) / 2.0;
                let slab = planes.partition_point(|&z| z < mid) - 1;
                (w[0], w[1], slab, mid > zlo && mid < zhi)
            })
            .collect();
        let originals: Vec<Region> = node_units(
            &windows
                .iter()
                .map(|w| slabs[w.2].clone())
                .collect::<Vec<_>>(),
        );
        let touched: Vec<bool> = windows.iter().map(|w| w.3).collect();
        let smooth = p.smoothing.clamp(0.0, 1.0);
        let smooth_limit = if smooth > 0.0 { tol / 3.0 } else { 0.0 };
        let knobs = Knobs {
            smooth_limit,
            passes: (smooth * 20.0).round() as usize,
            chord: tol - smooth_limit,
            loss: if p.keep_material { 0.25 } else { 1.0 },
            outward_is_right: originals
                .iter()
                .flatten()
                .find_map(|s| s.first())
                .is_some_and(|outer| area2(outer) > 0.0),
        };
        let regions = simplify_units(&originals, &touched, &knobs, &mut deviation);
        let mut segs: Vec<Segment> = windows
            .iter()
            .zip(originals)
            .zip(regions)
            .map(|((w, original), region)| Segment {
                z0: w.0,
                z1: w.1,
                original,
                region,
                touched: w.3,
            })
            .collect();

        crate::progress::step("check overhangs", 0.5);
        let mut violation = 0.0;
        if p.max_overhang_deg < 90.0 {
            let d = (p.layer_height * p.max_overhang_deg.max(0.0).to_radians().tan() * U).max(1.0);
            let allowed_above = |s: &[Segment], i: usize| -> Region {
                let original_overhang =
                    boolean(&s[i + 1].original, &s[i].original, OverlayRule::Difference);
                boolean(
                    &grow(&s[i].region, d),
                    &grow(&original_overhang, tol + 2.0),
                    OverlayRule::Union,
                )
            };
            for i in 0..segs.len().saturating_sub(1) {
                if !segs[i + 1].touched {
                    continue;
                }
                let allowed = allowed_above(&segs, i);
                let bad = drop_noise(boolean(
                    &segs[i + 1].region,
                    &allowed,
                    OverlayRule::Difference,
                ));
                if !bad.is_empty() {
                    segs[i + 1].region =
                        boolean(&segs[i + 1].region, &allowed, OverlayRule::Intersect);
                }
            }
            for i in 0..segs.len().saturating_sub(1) {
                if segs[i].touched || segs[i + 1].touched {
                    let allowed = allowed_above(&segs, i);
                    let bad = drop_noise(boolean(
                        &segs[i + 1].region,
                        &allowed,
                        OverlayRule::Difference,
                    ));
                    violation += region_area(&bad) / (U * U);
                }
            }
        }

        // Layers with an identical cross-section become one run.
        let mut run_planes = vec![segs[0].z0];
        let mut runs: Vec<Region> = Vec::new();
        for s in &segs {
            if runs.last() == Some(&s.region) {
                *run_planes.last_mut().unwrap() = s.z1;
            } else {
                runs.push(s.region.clone());
                run_planes.push(s.z1);
            }
        }
        crate::progress::step("build mesh", 0.7);
        let out = build(&run_planes, &runs);
        let skip = if !out.is_watertight() {
            Some("the simplified layers could not be joined into a watertight mesh".to_string())
        } else if crate::tile_bridge::components(&out).0 != pieces_before {
            Some("reducing would split or merge the stencil; lower the tolerance".to_string())
        } else if out.v.len() >= input.v.len() {
            Some("no layer could be simplified without adding vertices".to_string())
        } else {
            None
        };
        let retry = !out.is_watertight() || crate::tile_bridge::components(&out).0 != pieces_before;
        result = Some((out, skip, deviation, violation, scale));
        if !retry {
            break;
        }
    }
    let (layer_mesh, skip, mut deviation, violation, scale) = result.expect("at least one attempt");
    let layers_done = skip.is_none();
    let mut mesh = if layers_done {
        layer_mesh
    } else {
        input.clone()
    };
    let triangles_layers = mesh.t.len();
    let mut decimate_used = 0.0;
    if p.decimate_mm > 0.0 {
        crate::progress::step("decimate", 0.85);
        let mut trial = mesh.clone();
        let dev = crate::decimate::decimate(&mut trial, p.decimate_mm, (zlo, zhi));
        if trial.is_watertight()
            && crate::tile_bridge::components(&trial).0 == pieces_before
            && trial.t.len() < mesh.t.len()
        {
            mesh = trial;
            decimate_used = p.decimate_mm;
            deviation = deviation.max(dev * U);
        }
    }
    if !layers_done && decimate_used == 0.0 {
        return Ok((
            stl.to_vec(),
            Report {
                layers,
                from,
                to,
                vertices_before: input.v.len(),
                vertices_after: input.v.len(),
                triangles_before: input.t.len(),
                triangles_after: input.t.len(),
                max_deviation_mm: 0.0,
                overhang_violation_mm2: 0.0,
                tolerance_mm: 0.0,
                skipped: skip,
                triangles_layers: input.t.len(),
                decimate_mm: 0.0,
            },
        ));
    }
    let bytes = mesh.to_stl();
    crate::progress::step("write STL", 1.0);
    Ok((
        bytes,
        Report {
            layers,
            from,
            to,
            vertices_before: input.v.len(),
            vertices_after: mesh.v.len(),
            triangles_before: input.t.len(),
            triangles_after: mesh.t.len(),
            max_deviation_mm: deviation / U,
            overhang_violation_mm2: violation,
            tolerance_mm: if layers_done {
                p.tolerance_mm * scale
            } else {
                0.0
            },
            skipped: None,
            triangles_layers,
            decimate_mm: decimate_used,
        },
    ))
}
