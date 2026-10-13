use super::{validate, Mesh, Output, Params, Stats};
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

fn closest_pair(a: &[Pt], b: &[Pt]) -> (Pt, Pt) {
    let sample_count = a.len().min(96);
    let mut best = (a[0], b[0]);
    let mut best_d = f64::INFINITY;
    for sample in 0..sample_count {
        let q = a[sample * a.len() / sample_count];
        for j in 0..b.len() {
            let c = closest_on_segment(q, b[j], b[(j + 1) % b.len()]);
            let d = distance_sq(q, c);
            if d < best_d {
                best = (q, c);
                best_d = d;
            }
        }
    }
    let (mut p, mut q) = best;
    // Refine the sampled point onto both complete polylines.
    for _ in 0..3 {
        p = (0..a.len())
            .map(|j| closest_on_segment(q, a[j], a[(j + 1) % a.len()]))
            .min_by(|x, y| distance_sq(*x, q).total_cmp(&distance_sq(*y, q)))
            .unwrap_or(p);
        q = (0..b.len())
            .map(|j| closest_on_segment(p, b[j], b[(j + 1) % b.len()]))
            .min_by(|x, y| distance_sq(*x, p).total_cmp(&distance_sq(*y, p)))
            .unwrap_or(q);
    }
    (p, q)
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

    let islands_found = material.len().saturating_sub(1);
    let main = (0..material.len())
        .find(|&i| outer_contour(&material[i]).is_some_and(|c| contains(&to_mm(c), [1e-6, 1e-6])))
        .or_else(|| {
            (0..material.len())
                .max_by(|&a, &b| shape_area(&material[a]).total_cmp(&shape_area(&material[b])))
        })
        .ok_or("vector boolean produced no stencil components")?;
    let main_boundaries: Vec<Vec<Pt>> = material[main].iter().map(|c| to_mm(c)).collect();
    let mut deleted = 0usize;
    let mut bridges = Vec::new();
    let mut keep = vec![true; material.len()];
    for i in 0..material.len() {
        if i == main {
            continue;
        }
        if p.min_island_mm2 > 0.0
            && shape_area(&material[i]) / (UNITS_PER_MM * UNITS_PER_MM) < p.min_island_mm2
        {
            keep[i] = false;
            deleted += 1;
            continue;
        }
        let outline = outer_contour(&material[i]).ok_or("vector island has no outline")?;
        let outline = to_mm(outline);
        let (a, b) = main_boundaries
            .iter()
            .map(|boundary| closest_pair(&outline, boundary))
            .min_by(|(a0, b0), (a1, b1)| distance_sq(*a0, *b0).total_cmp(&distance_sq(*a1, *b1)))
            .ok_or("frame has no boundary")?;
        bridges.push(to_int(&bridge_polygon(a, b, p.bridge_mm)));
    }
    material = material
        .into_iter()
        .zip(keep)
        .filter_map(|(shape, keep)| keep.then_some(shape))
        .collect();
    if !bridges.is_empty() {
        material =
            material
                .as_slice()
                .overlay(bridges.as_slice(), OverlayRule::Union, FillRule::NonZero);
    }
    // A tiny union makes point-touching outlines share a manifold boundary.
    material = repair_point_contacts(&material);
    if material.len() != 1 {
        return Err(format!(
            "vector bridging left {} disconnected stencil components",
            material.len()
        ));
    }

    let layer_height = p.layer_height_mm;
    let base_layers = ((p.thickness_mm / layer_height).round() as usize).max(1);
    let layers = base_layers + p.extra_top_layers as usize;
    let thickness = layers as f64 * layer_height;
    let mesh = polygon_mesh(&material, p.width_mm, thickness, p.flip)?;
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
        first_layer_islands: 0,
        watertight: true,
        triangles: mesh.tris.len(),
        stl_bytes: stl.len(),
        ..Stats::default()
    };
    Ok(Output { stl, stats })
}
