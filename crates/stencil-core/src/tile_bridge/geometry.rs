use super::{BridgeParams, Mesh};
use i_triangle::i_overlay::core::fill_rule::FillRule;
use i_triangle::i_overlay::core::overlay_rule::OverlayRule;
use i_triangle::i_overlay::core::single::SingleIntOverlay;
use i_triangle::i_overlay::i_float::int::point::IntPoint;
use std::collections::BTreeMap;

type Point = [f64; 2];
const UNITS_PER_MM: f64 = 10_000.0;

fn polygon(points: &[Point]) -> Vec<IntPoint> {
    points
        .iter()
        .map(|p| {
            IntPoint::new(
                (p[0] * UNITS_PER_MM).round() as i32,
                (p[1] * UNITS_PER_MM).round() as i32,
            )
        })
        .collect()
}

fn distance(a: Point, b: Point) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// Remove the sampling staircase, but retain real bends needed to route around obstacles.
fn simplify(points: &[Point], tolerance: f64, out: &mut Vec<Point>) {
    let (a, b) = (points[0], points[points.len() - 1]);
    let d = [b[0] - a[0], b[1] - a[1]];
    let length_sq = d[0] * d[0] + d[1] * d[1];
    let farthest = points
        .iter()
        .enumerate()
        .skip(1)
        .take(points.len() - 2)
        .map(|(i, &p)| {
            let t = if length_sq > 0.0 {
                ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / length_sq
            } else {
                0.0
            }
            .clamp(0.0, 1.0);
            (i, distance(p, [a[0] + t * d[0], a[1] + t * d[1]]))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((i, _)) = farthest.filter(|(_, gap)| *gap > tolerance) {
        simplify(&points[..=i], tolerance, out);
        simplify(&points[i..], tolerance, out);
    } else {
        out.push(b);
    }
}

pub(super) fn build(
    path: &[usize],
    w: usize,
    origin: Point,
    cell: f64,
    bp: &BridgeParams,
    straight: bool,
) -> Result<Mesh, String> {
    let points: Vec<Point> = path
        .iter()
        .map(|&i| {
            [
                origin[0] + (i % w) as f64 * cell + cell * 0.5,
                origin[1] + (i / w) as f64 * cell + cell * 0.5,
            ]
        })
        .collect();
    if points.len() < 2 {
        return Err("bridge path has fewer than two endpoints".into());
    }
    let mut line = vec![points[0]];
    if straight {
        line.push(*points.last().unwrap());
    } else {
        simplify(&points, cell * 0.8, &mut line);
    }
    build_line(&line, cell, bp)
}

pub(super) fn build_axis(
    axis: [[f64; 2]; 2],
    origin: Point,
    cell: f64,
    bp: &BridgeParams,
) -> Result<Mesh, String> {
    let line = axis.map(|p| {
        [
            origin[0] + (p[0] + 0.5) * cell,
            origin[1] + (p[1] + 0.5) * cell,
        ]
    });
    build_line(&line, cell, bp)
}

fn build_line(line: &[Point], cell: f64, bp: &BridgeParams) -> Result<Mesh, String> {
    let total: f64 = line.windows(2).map(|p| distance(p[0], p[1])).sum();
    let span_width = bp.bridge_mm.max(cell);
    let end_width = if bp.min_contact_mm > 0.0 {
        ((bp.min_contact_mm / cell).ceil() + 4.0) * cell
    } else {
        span_width
    };
    let end_width = end_width.max(span_width);
    let layers = ((bp.plate_top / bp.layer_height).round() as u32).max(1);
    let base = bp.base_layers.min(layers);
    let raised = bp.raised && base > 1;
    let extension = cell;
    let width = |s: f64| (end_width - s.max(0.0).min((total - s).max(0.0))).max(span_width);
    let mut events: BTreeMap<u32, Vec<Vec<IntPoint>>> = BTreeMap::new();
    let mut offset = 0.0;
    for (segment, points) in line.windows(2).enumerate() {
        let (a, b) = (points[0], points[1]);
        let len = distance(a, b);
        if len < 1e-9 {
            return Err("bridge path contains a zero-length segment".into());
        }
        let dir = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
        let at = |s: f64, side: f64| {
            let half = width(offset + s) * 0.5 * side;
            [
                a[0] + dir[0] * s - dir[1] * half,
                a[1] + dir[1] * s + dir[0] * half,
            ]
        };
        let from = if segment == 0 { -extension } else { 0.0 };
        let to = if segment + 2 == line.len() {
            len + extension
        } else {
            len
        };
        let mut cuts = vec![from, to];
        let flare = (end_width - span_width).min(total * 0.5);
        if flare > 1e-8 {
            for s in [0.0, flare, total - flare, total] {
                let local = s - offset;
                if local > from && local < to {
                    cuts.push(local);
                }
            }
        }
        if raised && !bp.easy {
            for k in 1..base - 1 {
                let local = total * k as f64 / (base - 1) as f64 - offset;
                if local > from && local < to {
                    cuts.push(local);
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-8);
        for cut in cuts.windows(2) {
            let k = if !raised {
                0
            } else if bp.easy {
                (base / 2).max(1)
            } else {
                (((offset + (cut[0] + cut[1]) * 0.5) / total * (base - 1) as f64).floor() as i64
                    + 1)
                .clamp(1, base as i64 - 1) as u32
            };
            let poly = [
                at(cut[0], -1.0),
                at(cut[1], -1.0),
                at(cut[1], 1.0),
                at(cut[0], 1.0),
            ];
            events.entry(k).or_default().push(polygon(&poly));
        }
        // Adjacent strips overlap at real bends, rather than meeting at an edge.
        if segment + 2 < line.len() {
            let half = width(offset + len) * 0.5;
            let k = if raised {
                if bp.easy {
                    (base / 2).max(1)
                } else {
                    ((offset + len) / total * (base - 1) as f64)
                        .ceil()
                        .clamp(1.0, (base - 1) as f64) as u32
                }
            } else {
                0
            };
            events.entry(k).or_default().push(polygon(&[
                [b[0] - half, b[1] - half],
                [b[0] + half, b[1] - half],
                [b[0] + half, b[1] + half],
                [b[0] - half, b[1] + half],
            ]));
        }
        offset += len;
    }
    let mut planes = Vec::new();
    let mut runs = Vec::new();
    let mut cumulative = Vec::new();
    for (k, polygons) in events {
        cumulative.extend(polygons);
        planes.push(k as f64 * bp.layer_height);
        runs.push(cumulative.as_slice().overlay(
            &[] as &[Vec<IntPoint>],
            OverlayRule::Union,
            FillRule::NonZero,
        ));
    }
    planes.push(bp.plate_top);
    let mut mesh = crate::remesh::build(&planes, &runs);
    if bp.flip {
        for p in &mut mesh.v {
            p[2] = bp.plate_top - p[2];
        }
        for t in &mut mesh.t {
            t.swap(1, 2);
        }
    }
    if !mesh.is_watertight() {
        return Err("straight bridge mesh failed watertight verification".into());
    }
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagonal(steps: usize) -> Vec<usize> {
        let w = 512;
        let (mut x, mut y) = (10, 10);
        let mut path = vec![y * w + x];
        for _ in 0..steps {
            x += 1;
            path.push(y * w + x);
            y += 1;
            path.push(y * w + x);
        }
        path
    }

    #[test]
    fn diagonal_spans_have_two_straight_sides_not_pixel_stairs() {
        for steps in [20, 200] {
            for flip in [false, true] {
                for raised in [false, true] {
                    for easy in [false, true] {
                        let bp = BridgeParams {
                            bridge_mm: 2.0,
                            plate_top: 8.2,
                            base_layers: 40,
                            flip,
                            raised,
                            easy,
                            ..Default::default()
                        };
                        let mesh = build(&diagonal(steps), 512, [0.0; 2], 0.5, &bp, false).unwrap();
                        assert!(mesh.is_watertight());
                        assert!(mesh.volume() > 0.0);
                        // All vertices must lie on one of the two exact side planes.
                        // A raster staircase has many different normal projections.
                        for p in &mesh.v {
                            let normal = (p[1] - p[0]) / 2f64.sqrt();
                            assert!(
                                (normal.abs() - 1.0).abs() < 0.00015,
                                "non-straight edge at {p:?}"
                            );
                        }
                        if !raised || easy {
                            assert_eq!(
                                mesh.t.len(),
                                12,
                                "flat spans must not get more triangles when longer"
                            );
                        }
                        let low = if raised {
                            if easy {
                                4.0
                            } else {
                                0.2
                            }
                        } else {
                            0.0
                        };
                        let (lo, hi) = mesh.bounds();
                        assert!((lo[2] - if flip { 0.0 } else { low }).abs() < 1e-6);
                        assert!((hi[2] - if flip { 8.2 - low } else { 8.2 }).abs() < 1e-6);
                    }
                }
            }
        }
    }

    #[test]
    fn aligned_gap_meshes_share_the_exact_axis_not_rounded_cell_endpoints() {
        let bp = BridgeParams {
            bridge_mm: 2.0,
            raised: false,
            plate_top: 5.0,
            ..Default::default()
        };
        let direction = [0.8, 0.6];
        let origin = [12.0, -4.0];
        let cell = 0.5;
        let anchor = [3.25, 7.5];
        let physical = [
            origin[0] + (anchor[0] + 0.5) * cell,
            origin[1] + (anchor[1] + 0.5) * cell,
        ];
        for [start, end] in [[0.0, 12.0], [28.0, 49.0], [65.0, 100.0]] {
            let axis =
                [start, end].map(|s| [anchor[0] + s * direction[0], anchor[1] + s * direction[1]]);
            let mesh = build_axis(axis, origin, cell, &bp).unwrap();
            assert!(mesh.is_watertight());
            assert_eq!(mesh.t.len(), 12);
            for p in &mesh.v {
                let normal =
                    -(p[0] - physical[0]) * direction[1] + (p[1] - physical[1]) * direction[0];
                assert!((normal.abs() - 1.0).abs() < 0.00015, "{p:?}");
            }
        }
    }

    #[test]
    fn simplification_preserves_real_obstacle_avoiding_bends() {
        let points = [[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [2.0, 1.0], [2.0, 2.0]];
        let mut line = vec![points[0]];
        simplify(&points, 0.4, &mut line);
        assert_eq!(line, vec![[0.0, 0.0], [2.0, 0.0], [2.0, 2.0]]);
        let path = [0, 1, 2, 12, 22];
        for easy in [false, true] {
            let bp = BridgeParams {
                bridge_mm: 0.5,
                plate_top: 4.0,
                base_layers: 20,
                easy,
                ..Default::default()
            };
            let mesh = build(&path, 10, [0.0; 2], 0.5, &bp, false).unwrap();
            assert!(mesh.is_watertight());
            assert!(mesh.volume() > 0.0);
        }
    }

    #[test]
    fn sampled_rays_export_as_single_straight_strips_at_every_planned_angle() {
        let bp = BridgeParams {
            bridge_mm: 2.0,
            plate_top: 4.0,
            raised: false,
            ..Default::default()
        };
        for n in 0..72 {
            let angle = n as f64 * std::f64::consts::TAU / 72.0;
            let (dx, dy) = (angle.cos(), angle.sin());
            for reach in [5.0, 20.0, 100.0] {
                let (mut x, mut y) = (256i64, 256i64);
                let (mut tx, mut ty) = (0.5 / dx.abs(), 0.5 / dy.abs());
                let mut path = vec![(y * 512 + x) as usize];
                while tx.min(ty) < reach {
                    if tx <= ty {
                        x += if dx >= 0.0 { 1 } else { -1 };
                        tx += 1.0 / dx.abs();
                    } else {
                        y += if dy >= 0.0 { 1 } else { -1 };
                        ty += 1.0 / dy.abs();
                    }
                    path.push((y * 512 + x) as usize);
                }
                let mesh = build(&path, 512, [0.0; 2], 0.5, &bp, true).unwrap();
                assert_eq!(
                    mesh.t.len(),
                    12,
                    "ray at {angle} radians with reach {reach} was not straightened"
                );
            }
        }
    }

    #[test]
    fn reinforced_diagonal_spans_keep_a_straight_constant_width_middle() {
        let bp = BridgeParams {
            bridge_mm: 2.0,
            min_contact_mm: 3.0,
            plate_top: 8.2,
            base_layers: 40,
            ..Default::default()
        };
        let mesh = build(&diagonal(80), 512, [0.0; 2], 0.5, &bp, false).unwrap();
        assert!(mesh.is_watertight());
        for t in &mesh.t {
            for j in 0..3 {
                let (a, b) = (mesh.v[t[j]], mesh.v[t[(j + 1) % 3]]);
                let (sa, sb) = (a[0] + a[1], b[0] + b[1]);
                if (sa > 50.0) != (sb > 50.0) {
                    let f = (50.0 - sa) / (sb - sa);
                    let normal = (a[1] - a[0] + f * (b[1] - a[1] - b[0] + a[0])) / 2f64.sqrt();
                    assert!(
                        normal.abs() <= 1.0 + 0.00015,
                        "middle of bridge widened: {normal}"
                    );
                }
            }
        }
    }
}
