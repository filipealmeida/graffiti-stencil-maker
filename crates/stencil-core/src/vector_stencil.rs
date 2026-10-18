use super::{validate, Mesh, Output, Params, Stats, ZBridging};
use crate::fasthash::{FastMap, FastSet};
use crate::remesh::grow;
use crate::segindex::SegIndex;
use crate::trace::{Pt, Shapes};
use i_triangle::i_overlay::core::fill_rule::FillRule;
use i_triangle::i_overlay::core::overlay_rule::OverlayRule;
use i_triangle::i_overlay::core::single::SingleIntOverlay;
use i_triangle::i_overlay::i_float::int::point::IntPoint;
use i_triangle::int::unchecked::IntUncheckedTriangulatable;
use std::collections::HashMap;

mod repair;
#[cfg(test)]
mod tests;
use repair::*;

const UNITS_PER_MM: f64 = 10_000.0;
/// Bridges shorter than this are one flat strip instead of a ramp of steps.
const MIN_RAMP_MM: f64 = 0.15;

fn area(poly: &[Pt]) -> f64 {
    poly.iter()
        .zip(poly.iter().cycle().skip(1))
        .take(poly.len())
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum::<f64>()
        * 0.5
}

fn contains(poly: &[Pt], p: Pt) -> bool {
    let mut inside = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + poly.len() - 1) % poly.len()]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
    }
    inside
}

fn closest_on_segment(p: Pt, a: Pt, b: Pt) -> Pt {
    let d = [b[0] - a[0], b[1] - a[1]];
    let den = d[0] * d[0] + d[1] * d[1];
    let t = if den > 0.0 {
        (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / den).clamp(0.0, 1.0)
    } else {
        0.0
    };
    [a[0] + t * d[0], a[1] + t * d[1]]
}

fn distance_sq(a: Pt, b: Pt) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

/// The closest points of an island outline and the frame boundaries: the best of a sample of outline points,
/// refined onto both complete polylines.
fn closest_pair(a: &[Pt], frame: &SegIndex) -> Option<(Pt, Pt)> {
    let sample_count = a.len().min(96);
    let mut best: Option<(f64, Pt, Pt)> = None;
    for sample in 0..sample_count {
        let q = a[sample * a.len() / sample_count];
        if let Some((d, c)) = frame.nearest(q) {
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, q, c));
            }
        }
    }
    let (_, mut p, mut q) = best?;
    for _ in 0..3 {
        p = (0..a.len())
            .map(|j| closest_on_segment(q, a[j], a[(j + 1) % a.len()]))
            .min_by(|x, y| distance_sq(*x, q).total_cmp(&distance_sq(*y, q)))
            .unwrap_or(p);
        q = frame.nearest(p).map_or(q, |(_, c)| c);
    }
    Some((p, q))
}

fn bridge_polygon(a: Pt, b: Pt, width: f64) -> Vec<Pt> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len = d[0].hypot(d[1]).max(1e-9);
    let (ux, uy) = (d[0] / len, d[1] / len);
    let (nx, ny) = (-uy * width * 0.5, ux * width * 0.5);
    let extend = width * 0.55;
    let (ax, ay) = (a[0] - ux * extend, a[1] - uy * extend);
    let (bx, by) = (b[0] + ux * extend, b[1] + uy * extend);
    vec![
        [ax + nx, ay + ny],
        [bx + nx, by + ny],
        [bx - nx, by - ny],
        [ax - nx, ay - ny],
    ]
}

/// The bridge cut across its length into steps. Each step starts one layer higher than the one before, from
/// the island up to the plate; the two ends run on into the island and the plate. Every corner sits on a lattice
/// of whole step vectors, so the sides of neighbouring steps are exactly collinear and the mesh stays watertight.
fn bridge_steps(
    a: Pt,
    b: Pt,
    width: f64,
    base_layers: usize,
    easy: bool,
) -> Vec<(u32, Vec<IntPoint<i32>>)> {
    let whole = || {
        vec![(
            (base_layers / 2).max(1) as u32,
            to_int(&bridge_polygon(a, b, width)),
        )]
    };
    // An island hairline-close to the plate cannot hold a ramp: every step would be a few lattice units long, and
    // the stacked outlines pinch. A single strip does the job there.
    if easy || distance_sq(a, b) < MIN_RAMP_MM * MIN_RAMP_MM {
        return whole();
    }
    let n = (base_layers - 1) as i64;
    let to_units = |p: Pt| {
        [
            (p[0] * UNITS_PER_MM).round() as i64,
            (p[1] * UNITS_PER_MM).round() as i64,
        ]
    };
    let (ia, ib) = (to_units(a), to_units(b));
    let step = [
        ((ib[0] - ia[0]) as f64 / n as f64).round() as i64,
        ((ib[1] - ia[1]) as f64 / n as f64).round() as i64,
    ];
    let step_len = (step[0] as f64).hypot(step[1] as f64);
    if step_len < 1.0 {
        return whole();
    }
    let extra = (width * 0.55 * UNITS_PER_MM / step_len).ceil() as i64;
    let half = width * 0.5 * UNITS_PER_MM / step_len;
    let side = [
        (-step[1] as f64 * half).round() as i64,
        (step[0] as f64 * half).round() as i64,
    ];
    let corner = |i: i64, s: i64| {
        IntPoint::new(
            (ia[0] + step[0] * i + s * side[0]) as i32,
            (ia[1] + step[1] * i + s * side[1]) as i32,
        )
    };
    (1..=n)
        .map(|j| {
            let from = if j == 1 { -extra } else { j - 1 };
            let to = if j == n { n + extra } else { j };
            (
                j as u32,
                vec![
                    corner(from, 1),
                    corner(to, 1),
                    corner(to, -1),
                    corner(from, -1),
                ],
            )
        })
        .collect()
}

fn to_int(poly: &[Pt]) -> Vec<IntPoint<i32>> {
    poly.iter()
        .map(|p| {
            IntPoint::new(
                (p[0] * UNITS_PER_MM).round() as i32,
                (p[1] * UNITS_PER_MM).round() as i32,
            )
        })
        .collect()
}

fn to_mm(contour: &[IntPoint<i32>]) -> Vec<Pt> {
    contour
        .iter()
        .map(|p| [p.x as f64 / UNITS_PER_MM, p.y as f64 / UNITS_PER_MM])
        .collect()
}

fn outer_contour(shape: &[Vec<IntPoint<i32>>]) -> Option<&[IntPoint<i32>]> {
    shape
        .iter()
        .max_by(|a, b| area(&to_mm(a)).abs().total_cmp(&area(&to_mm(b)).abs()))
        .map(Vec::as_slice)
}

fn shape_area(shape: &[Vec<IntPoint<i32>>]) -> f64 {
    let mut areas: Vec<f64> = shape.iter().map(|c| area(&to_mm(c)).abs()).collect();
    areas.sort_by(|a, b| b.total_cmp(a));
    (areas.first().copied().unwrap_or(0.0) - areas.iter().skip(1).sum::<f64>()).max(0.0)
}

fn bounds(shape: &[Vec<IntPoint<i32>>]) -> [i32; 4] {
    let mut b = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    for p in shape.iter().flatten() {
        b = [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)];
    }
    b
}

fn polygon_mesh(
    shapes: &[Vec<Vec<IntPoint<i32>>>],
    width: f64,
    thickness: f64,
    flip: bool,
) -> Result<Mesh, String> {
    let mut mesh = Mesh {
        verts: Vec::new(),
        tris: Vec::new(),
    };
    let mut ids = HashMap::new();
    let top = thickness as f32;
    let mut vertex = |p: IntPoint<i32>, z: u8, mesh: &mut Mesh| -> u32 {
        *ids.entry((p.x, p.y, z)).or_insert_with(|| {
            mesh.verts.push([
                (p.x as f64 / UNITS_PER_MM) as f32,
                (p.y as f64 / UNITS_PER_MM) as f32,
                if z == 0 { 0.0 } else { top },
            ]);
            (mesh.verts.len() - 1) as u32
        })
    };

    for shape in shapes {
        let tri = shape.uncheck_triangulate().into_triangulation::<u32>();
        for indices in tri.indices.chunks(3) {
            let (a, b, c) = (
                tri.points[indices[0] as usize],
                tri.points[indices[1] as usize],
                tri.points[indices[2] as usize],
            );
            let cross = (b.x as i64 - a.x as i64) * (c.y as i64 - a.y as i64)
                - (b.y as i64 - a.y as i64) * (c.x as i64 - a.x as i64);
            if cross == 0 {
                continue;
            }
            let (a0, b0, c0) = (
                vertex(a, 0, &mut mesh),
                vertex(b, 0, &mut mesh),
                vertex(c, 0, &mut mesh),
            );
            let (a1, b1, c1) = (
                vertex(a, 1, &mut mesh),
                vertex(b, 1, &mut mesh),
                vertex(c, 1, &mut mesh),
            );
            if cross > 0 {
                mesh.tris.push([a0, c0, b0]);
                mesh.tris.push([a1, b1, c1]);
            } else {
                mesh.tris.push([a0, b0, c0]);
                mesh.tris.push([a1, c1, b1]);
            }
        }

        for contour in shape {
            for i in 0..contour.len() {
                let a = contour[i];
                let b = contour[(i + 1) % contour.len()];
                let (a0, b0) = (vertex(a, 0, &mut mesh), vertex(b, 0, &mut mesh));
                let (a1, b1) = (vertex(a, 1, &mut mesh), vertex(b, 1, &mut mesh));
                mesh.tris.extend([[a0, b0, b1], [a0, b1, a1]]);
            }
        }
    }

    if flip {
        for p in &mut mesh.verts {
            p[0] = width as f32 - p[0];
            p[2] = top - p[2];
        }
    }
    if !mesh.is_watertight() {
        let mut edges = HashMap::new();
        for tri in &mesh.tris {
            for i in 0..3 {
                *edges.entry((tri[i], tri[(i + 1) % 3])).or_insert(0usize) += 1;
            }
        }
        let bad: Vec<_> = edges
            .iter()
            .filter(|&(&(a, b), &n)| n != 1 || edges.get(&(b, a)) != Some(&1))
            .take(4)
            .map(|(&(a, b), &n)| {
                (
                    mesh.verts[a as usize],
                    mesh.verts[b as usize],
                    n,
                    edges.get(&(b, a)).copied(),
                )
            })
            .collect();
        return Err(format!(
            "vector mesh failed watertight verification ({} triangles; bad edges: {bad:?})",
            mesh.tris.len()
        ));
    }
    Ok(mesh)
}

/// Both ends of a bridge moved sideways by `by` bridge widths.
fn slide(a: Pt, b: Pt, by: f64, width: f64) -> (Pt, Pt) {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len = d[0].hypot(d[1]).max(1e-9);
    let (nx, ny) = (-d[1] / len * by * width, d[0] / len * by * width);
    ([a[0] + nx, a[1] + ny], [b[0] + nx, b[1] + ny])
}

/// Builds the plate. Where a bridge lands, rounding to the integer grid can leave a sliver pinch that no repair
/// resolves (a strip corner a few lattice units from an outline vertex); the plate is then built again with every
/// bridge slid sideways by a fraction of its width.
pub(crate) fn build(shapes: &Shapes, detail: f64, p: &Params) -> Result<Output, String> {
    let first = build_once(shapes, detail, p, 0.0);
    if !matches!(&first, Err(e) if e.contains("watertight verification")) {
        return first;
    }
    // In order of preference: a slightly moved bridge keeps the design; flat strips instead of ramps have far fewer
    // corners to pinch; another width moves the corners relative to the outline.
    let mut tries: Vec<(f64, bool, f64)> = vec![];
    for (widen, easy) in [
        (1.0, false),
        (1.0, true),
        (1.15, false),
        (0.87, false),
        (1.15, true),
        (0.87, true),
    ] {
        for k in 0..=4 {
            if widen == 1.0 && !easy && k == 0 {
                continue;
            }
            let side = if k % 2 == 1 { 1.0 } else { -1.0 };
            tries.push((widen, easy, side * 0.06 * ((k + 1) / 2) as f64));
        }
    }
    let attempt = |&(widen, easy, nudge): &(f64, bool, f64)| {
        let mut q = p.clone();
        q.bridge_mm = p.bridge_mm * widen;
        if easy {
            q.z_bridging = ZBridging::Easy;
        }
        let r = build_once(shapes, detail, &q, nudge);
        (widen, easy, q.bridge_mm, r)
    };
    // the attempts are independent, so a batch runs side by side and the first success in preference order wins
    #[cfg(not(target_arch = "wasm32"))]
    let lanes = rayon::current_num_threads().clamp(1, 8);
    #[cfg(target_arch = "wasm32")]
    let lanes = 1;
    let mut last = first;
    for batch in tries.chunks(lanes) {
        #[cfg(not(target_arch = "wasm32"))]
        let results: Vec<_> = {
            use rayon::prelude::*;
            batch.par_iter().map(attempt).collect()
        };
        #[cfg(target_arch = "wasm32")]
        let results: Vec<_> = batch.iter().map(attempt).collect();
        for (widen, easy, bridge, r) in results {
            match r {
                Ok(mut o) => {
                    let mut what = vec![];
                    if widen != 1.0 {
                        what.push(format!(
                            "bridges {bridge:.2} mm wide instead of {:.2} mm",
                            p.bridge_mm
                        ));
                    }
                    if easy && p.z_bridging != ZBridging::Easy {
                        what.push("flat bridge strips instead of stepped ramps".to_string());
                    }
                    if what.is_empty() {
                        what.push("bridges moved sideways".to_string());
                    }
                    o.fallback = Some(format!("the plate only verified with {}", what.join(", ")));
                    return Ok(o);
                }
                Err(e) => last = Err(e),
            }
        }
    }
    last
}

fn build_once(shapes: &Shapes, detail: f64, p: &Params, nudge: f64) -> Result<Output, String> {
    validate(p)?;
    if shapes.w == 0 || shapes.h == 0 || shapes.loops.is_empty() {
        return Err("vector input contains no closed outlines".into());
    }
    if !detail.is_finite() || detail <= 0.0 {
        return Err("mesh detail must be positive and finite".into());
    }

    let margin = p.margin_mm;
    let art_width = p.width_mm - 2.0 * margin;
    let mm_per_px = art_width / shapes.w as f64;
    let art_height = shapes.h as f64 * mm_per_px;
    let height = art_height + 2.0 * margin;
    let tolerance_px = (0.25 / detail) / mm_per_px;
    let polys: Vec<Vec<Pt>> = shapes
        .flatten(tolerance_px.max(1e-4))
        .into_iter()
        .filter(|poly| poly.len() >= 3)
        .map(|poly| {
            poly.into_iter()
                .map(|pt| {
                    [
                        margin + pt[0] * mm_per_px,
                        margin + (shapes.h as f64 - pt[1]) * mm_per_px,
                    ]
                })
                .collect()
        })
        .collect();
    if polys.iter().map(Vec::len).sum::<usize>() > 4_000_000 {
        return Err("vector outline is too complex; lower Mesh detail".into());
    }

    let paint: Vec<Vec<IntPoint<i32>>> = polys.iter().map(|poly| to_int(poly)).collect();
    let plate = to_int(&[
        [0.0, 0.0],
        [p.width_mm, 0.0],
        [p.width_mm, height],
        [0.0, height],
    ]);
    let mut material = if paint.is_empty() {
        vec![vec![plate.clone()]]
    } else {
        plate
            .as_slice()
            .overlay(paint.as_slice(), OverlayRule::Difference, FillRule::EvenOdd)
    };
    if material.is_empty() {
        return Err("vector boolean produced an empty stencil".into());
    }

    if p.min_contact_mm > 0.0 {
        material = widen_thin(&material, &plate, p.min_contact_mm);
    }

    let islands_found = material.len().saturating_sub(1);
    let main = (0..material.len())
        .find(|&i| outer_contour(&material[i]).is_some_and(|c| contains(&to_mm(c), [1e-6, 1e-6])))
        .or_else(|| {
            (0..material.len())
                .max_by(|&a, &b| shape_area(&material[a]).total_cmp(&shape_area(&material[b])))
        })
        .ok_or("vector boolean produced no stencil components")?;
    let main_boundaries: Vec<Vec<Pt>> = material[main].iter().map(|c| to_mm(c)).collect();
    let frame = SegIndex::new(&main_boundaries, p.bridge_mm.max(2.0));
    let mut deleted = 0usize;
    let mut bridges = Vec::new();
    let mut links: Vec<(Pt, Pt)> = Vec::new();
    let per_island = p.bridges_per_island.clamp(1, 5) as usize;
    let mut keep = vec![true; material.len()];
    for i in 0..material.len() {
        if i == main {
            continue;
        }
        if p.min_island_mm2 > 0.0 && shape_area(&material[i]) < p.min_island_mm2 {
            keep[i] = false;
            deleted += 1;
            continue;
        }
        let outline = outer_contour(&material[i]).ok_or("vector island has no outline")?;
        let outline = to_mm(outline);
        let (a, b) = closest_pair(&outline, &frame).ok_or("frame has no boundary")?;
        let (a, b) = slide(a, b, nudge, p.bridge_mm);
        bridges.push(to_int(&bridge_polygon(a, b, p.bridge_mm)));
        links.push((a, b));
        let mut chosen = vec![a];
        let reach = 4.0 * distance_sq(a, b).sqrt().max(p.bridge_mm) + 4.0 * p.bridge_mm;
        let spacing = 3.0 * p.bridge_mm.max(1.0);
        let step = (outline.len() / 256).max(1);
        // further bridges: the shortest remaining links that leave the island away from the earlier ones
        let mut options: Vec<(f64, Pt, Pt)> = (0..outline.len())
            .step_by(step)
            .filter_map(|k| frame.nearest(outline[k]).map(|(d, c)| (d, outline[k], c)))
            .filter(|o| o.0.sqrt() <= reach)
            .collect();
        options.sort_by(|x, y| x.0.total_cmp(&y.0));
        for (_, from, to) in options {
            if chosen.len() >= per_island {
                break;
            }
            let apart = chosen
                .iter()
                .all(|c| distance_sq(*c, from) >= spacing * spacing);
            if apart {
                chosen.push(from);
                let (from, to) = slide(from, to, nudge, p.bridge_mm);
                bridges.push(to_int(&bridge_polygon(from, to, p.bridge_mm)));
                links.push((from, to));
            }
        }
    }
    material = material
        .into_iter()
        .zip(keep)
        .filter_map(|(shape, keep)| keep.then_some(shape))
        .collect();
    let layer_height = p.layer_height_mm;
    let base_layers = ((p.thickness_mm / layer_height).round() as usize).max(1);
    let layers = base_layers + p.extra_top_layers as usize;
    let thickness = layers as f64 * layer_height;
    let raised = p.raised_bridges && base_layers > 1 && !links.is_empty();
    let mut first_layer_islands = 0;
    let mesh = if raised {
        // The plate runs the full height with its islands; each bridge joins it from a higher layer, so the first
        // layer keeps the artwork and the paint passes under the bridges.
        material = repair_point_contacts(&material);
        first_layer_islands = material.len().saturating_sub(1);
        let mut steps: std::collections::BTreeMap<u32, Vec<Vec<IntPoint<i32>>>> =
            Default::default();
        for &(a, b) in &links {
            for (k, poly) in bridge_steps(
                a,
                b,
                p.bridge_mm,
                base_layers,
                p.z_bridging == ZBridging::Easy,
            ) {
                steps.entry(k).or_default().push(poly);
            }
        }
        let mut planes = vec![0.0];
        let mut runs = vec![material.clone()];
        // Each run is built from the plate and the strips themselves, never from the previous run, so that every
        // crossing is computed from the same two original segments in all of them.
        let mut cumulative: Vec<Vec<IntPoint<i32>>> = Vec::new();
        let mut jobs: Vec<Job> = Vec::new();
        for (k, polys) in &steps {
            cumulative.extend(polys.iter().cloned());
            planes.push(*k as f64 * layer_height);
            jobs.push((cumulative.clone(), Vec::new()));
        }
        crate::par::each_mut(&mut jobs, |(strips, out)| {
            let merged = material.as_slice().overlay(
                strips.as_slice(),
                OverlayRule::Union,
                FillRule::NonZero,
            );
            *out = repair_point_contacts(&merged);
        });
        runs.extend(jobs.into_iter().map(|(_, run)| run));
        planes.push(thickness);
        // Noding can put one run's vertex on another's edge, which is a pinch; patch and node again until stable.
        // The patching rarely settles on its own (it can keep nudging the same few places), so once it stops
        // making headway the mesh is tried and kept if it is already watertight.
        let mut radius = (0.01 * UNITS_PER_MM) as i32;
        let mut tried = 0;
        let mut last_patched = usize::MAX;
        let mut done: Option<crate::mesh::Mesh> = None;
        let mut verified = false;
        let mut dirty = vec![true; runs.len()];
        let mut known = Seen::default();
        for round in 0..12 {
            node_runs(&mut runs, &mut dirty, &mut known);
            let mut work: Vec<(Vec<Shape>, usize, bool, bool)> = runs
                .drain(..)
                .zip(&dirty)
                .map(|(r, &d)| (r, 0, false, d))
                .collect();
            // a run nothing touched since its last repair would repair to itself
            crate::par::each_mut(&mut work, |(run, patched, changed, dirty)| {
                if !*dirty {
                    return;
                }
                let (next, n) = repair_near_contacts(run, radius);
                *changed = n > 0 && next != *run;
                *patched = n;
                *run = next;
            });
            let patched: usize = work.iter().map(|w| w.1).sum();
            let changed = work.iter().any(|w| w.2);
            dirty = work.iter().map(|w| w.2).collect();
            runs.extend(work.into_iter().map(|w| w.0));
            let stalled = patched > 0 && patched == last_patched && round >= 2;
            last_patched = patched;
            if !changed || (stalled && tried < 3) {
                let mesh = crate::remesh::build(&planes, &runs);
                let ok = mesh.is_watertight();
                if !changed || ok {
                    verified = ok;
                    done = Some(mesh);
                    break;
                }
                tried += 1;
            }
            radius += radius / 2;
        }
        crate::progress::step("bridge islands", 0.5);
        let last = runs.last().expect("one run exists");
        if last.len() != 1 {
            return Err(format!(
                "vector bridging left {} disconnected stencil components",
                last.len()
            ));
        }
        let mut stack = done.unwrap_or_else(|| crate::remesh::build(&planes, &runs));
        if p.flip {
            for v in &mut stack.v {
                v[0] = p.width_mm - v[0];
                v[2] = thickness - v[2];
            }
        }
        // mirroring keeps the topology, so a mesh already verified above needs no second check
        if !verified && !stack.is_watertight() {
            return Err(format!(
                "vector mesh with raised bridges failed watertight verification (bad edges: {:?})",
                stack.bad_edges(4)
            ));
        }
        let stl = stack.to_stl();
        let stats = Stats {
            width_mm: p.width_mm,
            height_mm: height,
            thickness_mm: thickness,
            layers,
            islands_found,
            islands_deleted: deleted,
            bridge_pixels: bridges.len(),
            islands_remaining: 0,
            first_layer_islands,
            watertight: true,
            triangles: stack.t.len(),
            stl_bytes: stl.len(),
            ..Stats::default()
        };
        crate::progress::step("extrude mesh", 0.9);
        return Ok(Output {
            stl,
            stats,
            fallback: None,
        });
    } else {
        if !bridges.is_empty() {
            material = material.as_slice().overlay(
                bridges.as_slice(),
                OverlayRule::Union,
                FillRule::NonZero,
            );
        }
        crate::progress::step("bridge islands", 0.5);
        // A tiny union makes point-touching outlines share a manifold boundary.
        material = repair_point_contacts(&material);
        if material.len() != 1 {
            return Err(format!(
                "vector bridging left {} disconnected stencil components",
                material.len()
            ));
        }
        polygon_mesh(&material, p.width_mm, thickness, p.flip)?
    };
    crate::progress::step("extrude mesh", 0.9);
    let stl = mesh.to_stl();
    let stats = Stats {
        width_mm: p.width_mm,
        height_mm: height,
        thickness_mm: thickness,
        layers,
        islands_found,
        islands_deleted: deleted,
        bridge_pixels: bridges.len(),
        islands_remaining: material.len().saturating_sub(1),
        first_layer_islands,
        watertight: true,
        triangles: mesh.tris.len(),
        stl_bytes: stl.len(),
        ..Stats::default()
    };
    Ok(Output {
        stl,
        stats,
        fallback: None,
    })
}
