use super::{validate, Mesh, Output, Params, Stats, ZBridging};
use crate::complexity::grow;
use crate::segindex::SegIndex;
use crate::trace::{Pt, Shapes};
use i_triangle::i_overlay::core::fill_rule::FillRule;
use i_triangle::i_overlay::core::overlay_rule::OverlayRule;
use i_triangle::i_overlay::core::single::SingleIntOverlay;
use i_triangle::i_overlay::i_float::int::point::IntPoint;
use i_triangle::int::unchecked::IntUncheckedTriangulatable;
use std::collections::HashMap;

const UNITS_PER_MM: f64 = 10_000.0;

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
    if easy {
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

fn repair_point_contacts(shapes: &[Vec<Vec<IntPoint<i32>>>]) -> Vec<Vec<Vec<IntPoint<i32>>>> {
    let mut counts = HashMap::new();
    for contour in shapes.iter().flat_map(|shape| shape.iter()) {
        for p in contour {
            *counts.entry((p.x, p.y)).or_insert(0usize) += 1;
        }
    }
    let radius = (0.01 * UNITS_PER_MM) as i32;
    let patches: Vec<Vec<IntPoint<i32>>> = counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|((x, y), _)| {
            vec![
                IntPoint::new(x - radius, y - radius),
                IntPoint::new(x + radius, y - radius),
                IntPoint::new(x + radius, y + radius),
                IntPoint::new(x - radius, y + radius),
            ]
        })
        .collect();
    if patches.is_empty() {
        shapes.to_vec()
    } else {
        shapes.overlay(patches.as_slice(), OverlayRule::Union, FillRule::NonZero)
    }
}

/// Runs the closure over every item, on all cores where threads exist (not in the browser's WebAssembly).
fn each_mut<T: Send, F: Fn(&mut T) + Sync + Send>(items: &mut [T], f: F) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        items.par_iter_mut().for_each(f);
    }
    #[cfg(target_arch = "wasm32")]
    items.iter_mut().for_each(f);
}

/// Every run shares the vertices created where bridges cross the outlines: an edge passing within a unit or two
/// of another run's vertex gets that vertex too, so all runs agree on where the walls break.
fn node_runs(runs: &mut [Vec<Vec<Vec<IntPoint<i32>>>>]) {
    type Contour = Vec<IntPoint<i32>>;
    const CELL: i64 = 4096;
    let key = |x: i64, y: i64| (x.div_euclid(CELL), y.div_euclid(CELL));
    let mut grid: HashMap<(i64, i64), Vec<IntPoint<i32>>> = HashMap::new();
    let mut seen = std::collections::HashSet::new();
    for r in runs.iter().flatten().flatten().flatten() {
        if seen.insert((r.x, r.y)) {
            grid.entry(key(r.x as i64, r.y as i64))
                .or_default()
                .push(*r);
        }
    }
    let mut contours: Vec<&mut Contour> = runs.iter_mut().flatten().flatten().collect();
    each_mut(&mut contours, |contour| {
        let n = contour.len();
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let (p, q) = (contour[i], contour[(i + 1) % n]);
            out.push(p);
            let (dx, dy) = (
                (q.x as i64 - p.x as i64) as f64,
                (q.y as i64 - p.y as i64) as f64,
            );
            let l2 = dx * dx + dy * dy;
            if l2 == 0.0 {
                continue;
            }
            let (c0, c1) = (key(p.x as i64, p.y as i64), key(q.x as i64, q.y as i64));
            let mut cells = Vec::new();
            if (c0.0 - c1.0).abs() <= 1 && (c0.1 - c1.1).abs() <= 1 {
                // a short edge: the cells around its two ends cover every vertex that can lie near it
                for cx in c0.0.min(c1.0) - 1..=c0.0.max(c1.0) + 1 {
                    for cy in c0.1.min(c1.1) - 1..=c0.1.max(c1.1) + 1 {
                        cells.push((cx, cy));
                    }
                }
            } else {
                let samples = (l2.sqrt() / CELL as f64).ceil() as i64 + 1;
                for s in 0..=samples {
                    let t = s as f64 / samples as f64;
                    let (x, y) = (p.x as f64 + dx * t, p.y as f64 + dy * t);
                    let (cx, cy) = key(x as i64, y as i64);
                    for a in -1..=1 {
                        for b in -1..=1 {
                            cells.push((cx + a, cy + b));
                        }
                    }
                }
                cells.sort_unstable();
                cells.dedup();
            }
            let mut found: Vec<(f64, IntPoint<i32>)> = Vec::new();
            for cell in cells {
                for &r in grid.get(&cell).into_iter().flatten() {
                    if r == p || r == q {
                        continue;
                    }
                    let (rx, ry) = (
                        (r.x as i64 - p.x as i64) as f64,
                        (r.y as i64 - p.y as i64) as f64,
                    );
                    let t = (rx * dx + ry * dy) / l2;
                    if t <= 0.0 || t >= 1.0 {
                        continue;
                    }
                    if (rx * dy - ry * dx).abs() / l2.sqrt() <= 2.0 {
                        found.push((t, r));
                    }
                }
            }
            found.sort_by(|a, b| a.0.total_cmp(&b.0));
            out.extend(found.into_iter().map(|f| f.1));
        }
        **contour = out;
    });
}

/// Like `repair_point_contacts`, but also for vertices a few units from another vertex of the outlines: rounding
/// turns such near-touches and tiny jogs into pinch points that no manifold mesh can have.
fn repair_near_contacts(
    shapes: &[Vec<Vec<IntPoint<i32>>>],
    radius: i32,
) -> (Vec<Vec<Vec<IntPoint<i32>>>>, usize) {
    const NEAR: i32 = 3;
    let mut grid: HashMap<(i32, i32), Vec<(usize, usize, IntPoint<i32>)>> = HashMap::new();
    let mut id = 0;
    let mut lens: Vec<usize> = Vec::new();
    for contour in shapes.iter().flatten() {
        lens.push(contour.len());
        for (i, p) in contour.iter().enumerate() {
            grid.entry((p.x.div_euclid(8), p.y.div_euclid(8)))
                .or_default()
                .push((id, i, *p));
        }
        id += 1;
    }
    let mut at: std::collections::HashSet<(i32, i32)> = Default::default();
    for cell in grid.values() {
        for &(ca, ia, pa) in cell {
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let key = (pa.x.div_euclid(8) + dx, pa.y.div_euclid(8) + dy);
                    for &(cb, ib, pb) in grid.get(&key).into_iter().flatten() {
                        // neighbours along one outline are an edge, not a contact
                        let n = lens[ca];
                        let next = ca == cb && ((ia + 1) % n == ib || (ib + 1) % n == ia);
                        if (cb, ib) == (ca, ia)
                            || next
                            || (pa.x - pb.x).abs() > NEAR
                            || (pa.y - pb.y).abs() > NEAR
                        {
                            continue;
                        }
                        at.insert((pa.x, pa.y));
                    }
                }
            }
        }
    }
    let patches: Vec<Vec<IntPoint<i32>>> = at
        .into_iter()
        .map(|(x, y)| {
            vec![
                IntPoint::new(x - radius, y - radius),
                IntPoint::new(x + radius, y - radius),
                IntPoint::new(x + radius, y + radius),
                IntPoint::new(x - radius, y + radius),
            ]
        })
        .collect();
    let count = patches.len();
    if patches.is_empty() {
        (shapes.to_vec(), 0)
    } else {
        (
            shapes.overlay(patches.as_slice(), OverlayRule::Union, FillRule::NonZero),
            count,
        )
    }
}

/// Widens every stretch of material narrower than `min_mm` that is big enough to matter (a rope between two
/// parts, a thin tip) to that width. Thin parts are what an opening by half the width removes; they grow into
/// the paint side only, never past the plate.
fn widen_thin(
    material: &[Vec<Vec<IntPoint<i32>>>],
    plate: &[IntPoint<i32>],
    min_mm: f64,
) -> Vec<Vec<Vec<IntPoint<i32>>>> {
    let r = min_mm * 0.5 * UNITS_PER_MM;
    let plate_region = vec![vec![plate.to_vec()]];
    let plate_region = &plate_region[0];
    let paint =
        plate_region
            .as_slice()
            .overlay(material, OverlayRule::Difference, FillRule::EvenOdd);
    let eroded = plate_region.as_slice().overlay(
        grow(&paint, r).as_slice(),
        OverlayRule::Difference,
        FillRule::EvenOdd,
    );
    let opening = grow(&eroded, r);
    let min_area = 0.5 * min_mm * min_mm;
    let boxes: Vec<[i32; 4]> = opening.iter().map(|s| bounds(s)).collect();
    // A rope joins two different thick parts; slivers along one part's outline and thin tips touch only one.
    let thin: Vec<_> = material
        .overlay(
            opening.as_slice(),
            OverlayRule::Difference,
            FillRule::EvenOdd,
        )
        .into_iter()
        .filter(|s| shape_area(s) >= min_area)
        .filter(|s| {
            let reach = grow(&vec![s.clone()], 50.0);
            let Some(first) = reach.first() else {
                return false;
            };
            let b = bounds(first);
            let touching = (0..opening.len())
                .filter(|&j| {
                    let o = boxes[j];
                    o[0] <= b[2] && b[0] <= o[2] && o[1] <= b[3] && b[1] <= o[3]
                })
                .filter(|&j| {
                    !reach
                        .as_slice()
                        .overlay(
                            opening[j].as_slice(),
                            OverlayRule::Intersect,
                            FillRule::NonZero,
                        )
                        .is_empty()
                })
                .count();
            touching >= 2
        })
        .collect();
    if thin.is_empty() {
        return material.to_vec();
    }
    let widened = material.overlay(
        grow(&thin, r).as_slice(),
        OverlayRule::Union,
        FillRule::NonZero,
    );
    widened.as_slice().overlay(
        plate_region.as_slice(),
        OverlayRule::Intersect,
        FillRule::NonZero,
    )
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

pub(crate) fn build(shapes: &Shapes, detail: f64, p: &Params) -> Result<Output, String> {
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
        let mut jobs: Vec<(Vec<Vec<IntPoint<i32>>>, Vec<Vec<Vec<IntPoint<i32>>>>)> = Vec::new();
        for (k, polys) in &steps {
            cumulative.extend(polys.iter().cloned());
            planes.push(*k as f64 * layer_height);
            jobs.push((cumulative.clone(), Vec::new()));
        }
        each_mut(&mut jobs, |(strips, out)| {
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
        for round in 0..12 {
            node_runs(&mut runs);
            let mut work: Vec<(Vec<Vec<Vec<IntPoint<i32>>>>, usize, bool)> =
                runs.drain(..).map(|r| (r, 0, false)).collect();
            each_mut(&mut work, |(run, patched, changed)| {
                let (next, n) = repair_near_contacts(run, radius);
                *changed = n > 0 && next != *run;
                *patched = n;
                *run = next;
            });
            let patched: usize = work.iter().map(|w| w.1).sum();
            let changed = work.iter().any(|w| w.2);
            runs.extend(work.into_iter().map(|w| w.0));
            let stalled = patched > 0 && patched == last_patched && round >= 2;
            last_patched = patched;
            if !changed || (stalled && tried < 3) {
                let mesh = crate::complexity::build(&planes, &runs);
                if !changed || mesh.is_watertight() {
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
        let mut stack = done.unwrap_or_else(|| crate::complexity::build(&planes, &runs));
        if p.flip {
            for v in &mut stack.v {
                v[0] = p.width_mm - v[0];
                v[2] = thickness - v[2];
            }
        }
        if !stack.is_watertight() {
            return Err("vector mesh with raised bridges failed watertight verification".into());
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
        return Ok(Output { stl, stats });
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
    Ok(Output { stl, stats })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::{trace, TraceParams};
    use crate::ZBridging;

    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 33) as f64 / (1u64 << 31) as f64
        }
    }

    /// Random discs, rings and rectangles: nested shapes and loose pieces both create islands.
    fn blobs(rng: &mut Lcg, n: usize) -> Vec<bool> {
        let mut m = vec![false; n * n];
        for _ in 0..(3 + (rng.next() * 6.0) as usize) {
            let (cx, cy) = (rng.next() * n as f64, rng.next() * n as f64);
            let r = 4.0 + rng.next() * n as f64 * 0.25;
            let hole = if rng.next() < 0.5 {
                rng.next() * 0.7
            } else {
                0.0
            };
            let rect = rng.next() < 0.3;
            for y in 0..n {
                for x in 0..n {
                    let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
                    let d = if rect {
                        dx.abs().max(dy.abs())
                    } else {
                        dx.hypot(dy)
                    };
                    if d < r && d >= r * hole {
                        m[y * n + x] = true;
                    }
                }
            }
        }
        m
    }

    #[test]
    fn random_shapes_always_build_island_free_watertight_plates() {
        let tp = TraceParams {
            smooth_px: 1.0,
            corner_deg: 60.0,
            tolerance_px: 0.4,
            min_area_px: 4.0,
        };
        let mut built = 0;
        for seed in 0..24u64 {
            let mut rng = Lcg(seed * 7919 + 1);
            let n = 90;
            let mask = blobs(&mut rng, n);
            let shapes = trace(&mask, n, n, &tp);
            for (raised, z) in [
                (false, ZBridging::Steps),
                (true, ZBridging::Steps),
                (true, ZBridging::Easy),
            ] {
                let p = Params {
                    width_mm: 80.0,
                    margin_mm: 4.0,
                    bridges_per_island: 1 + (seed as u32 % 5),
                    raised_bridges: raised,
                    z_bridging: z,
                    ..Params::default()
                };
                match build(&shapes, 2.0, &p) {
                    Ok(o) => {
                        assert_eq!(o.stats.islands_remaining, 0, "seed {seed} raised {raised}");
                        assert!(o.stats.watertight, "seed {seed} raised {raised}");
                        built += 1;
                    }
                    Err(e) => assert!(
                        shapes.loops.is_empty(),
                        "seed {seed} raised {raised} {z:?}: {e}"
                    ),
                }
            }
        }
        assert!(built >= 40, "only {built} plates built");
    }

    #[test]
    fn extra_bridges_leave_an_island_in_different_directions() {
        let tp = TraceParams {
            smooth_px: 1.0,
            corner_deg: 60.0,
            tolerance_px: 0.4,
            min_area_px: 4.0,
        };
        let n = 120;
        let mask: Vec<bool> = (0..n * n)
            .map(|i| {
                let (x, y) = ((i % n) as f64 - 60.0, (i / n) as f64 - 60.0);
                (30.0..40.0).contains(&x.hypot(y))
            })
            .collect();
        let shapes = trace(&mask, n, n, &tp);
        let count = |k| {
            let p = Params {
                width_mm: 80.0,
                margin_mm: 4.0,
                bridges_per_island: k,
                ..Params::default()
            };
            build(&shapes, 2.0, &p).unwrap().stats.bridge_pixels
        };
        assert_eq!((count(1), count(3), count(5)), (1, 3, 5));
    }

    #[test]
    fn delete_islands_threshold_is_in_square_millimetres() {
        let tp = TraceParams {
            smooth_px: 1.0,
            corner_deg: 60.0,
            tolerance_px: 0.4,
            min_area_px: 4.0,
        };
        let n = 120;
        // a ring: its inside is one island of about 25 mm² at 100 mm wide
        let mask: Vec<bool> = (0..n * n)
            .map(|i| {
                let (x, y) = ((i % n) as f64 - 60.0, (i / n) as f64 - 60.0);
                (20.0..30.0).contains(&x.hypot(y))
            })
            .collect();
        let shapes = trace(&mask, n, n, &tp);
        let run = |min: f64| {
            let p = Params {
                width_mm: 100.0,
                margin_mm: 4.0,
                min_island_mm2: min,
                ..Params::default()
            };
            build(&shapes, 2.0, &p).unwrap().stats
        };
        assert_eq!(run(0.0).islands_deleted, 0);
        assert_eq!(
            run(1.0).islands_deleted,
            0,
            "an island above the limit must stay"
        );
        assert_eq!(run(1.0e6).islands_deleted, 1);
    }

    #[test]
    fn thin_rope_is_widened_by_minimum_contact() {
        let (w, h) = (200usize, 100usize);
        let mask: Vec<bool> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let blob =
                    ((20..60).contains(&x) || (140..180).contains(&x)) && (30..70).contains(&y);
                !(blob || ((60..140).contains(&x) && (49..51).contains(&y)))
            })
            .collect();
        let tp = TraceParams {
            smooth_px: 0.0,
            corner_deg: 60.0,
            tolerance_px: 0.3,
            min_area_px: 4.0,
        };
        let shapes = trace(&mask, w, h, &tp);
        let run = |min_contact_mm: f64, raised: bool| {
            let p = Params {
                width_mm: 100.0,
                margin_mm: 4.0,
                min_contact_mm,
                raised_bridges: raised,
                ..Params::default()
            };
            build(&shapes, 4.0, &p).unwrap()
        };
        for raised in [false, true] {
            let (off, on) = (run(0.0, raised), run(3.0, raised));
            assert!(on.stats.watertight && on.stats.islands_remaining == 0);
            let (v0, v1) = (crate::stl_volume(&off.stl), crate::stl_volume(&on.stl));
            assert!(v1 - v0 > 100.0, "raised {raised}: grew by only {}", v1 - v0);
        }
    }

    #[test]
    fn rejects_degenerate_input() {
        let empty = Shapes {
            w: 10,
            h: 10,
            loops: vec![],
        };
        assert!(build(&empty, 2.0, &Params::default()).is_err());
        let tp = TraceParams {
            smooth_px: 1.0,
            corner_deg: 60.0,
            tolerance_px: 0.4,
            min_area_px: 1.0,
        };
        let mut m = vec![false; 400];
        for y in 5..15 {
            for x in 5..15 {
                m[y * 20 + x] = true;
            }
        }
        let s = trace(&m, 20, 20, &tp);
        assert!(build(&s, 0.0, &Params::default()).is_err());
        assert!(build(&s, f64::NAN, &Params::default()).is_err());
        assert!(build(
            &s,
            2.0,
            &Params {
                width_mm: 5.0,
                margin_mm: 4.0,
                ..Params::default()
            }
        )
        .is_err());
    }
}
