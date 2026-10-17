//! Edits on traced shapes: rounding sharp corners and removing small islands or holes.

use crate::trace::{Cubic, Loop, Pt, Shapes};

fn sub(a: Pt, b: Pt) -> Pt {
    [a[0] - b[0], a[1] - b[1]]
}
fn len(a: Pt) -> f64 {
    a[0].hypot(a[1])
}
fn unit(a: Pt) -> Option<Pt> {
    let l = len(a);
    (l > 1e-9).then(|| [a[0] / l, a[1] / l])
}
fn cross(a: Pt, b: Pt) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn lerp(a: Pt, b: Pt, t: f64) -> Pt {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn signed_area(p: &[Pt]) -> f64 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (u, v) = (p[i], p[(i + 1) % p.len()]);
        a += cross(u, v);
    }
    a / 2.0
}

fn inside(poly: &[Pt], p: Pt) -> bool {
    let mut c = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
        j = i;
    }
    c
}

/// How the loops contain each other; even-odd fill means an even number of containers is paint.
struct Nest {
    signed: Vec<f64>,
    ancestors: Vec<Vec<usize>>,
}

fn nest(flat: &[Vec<Pt>]) -> Nest {
    let signed: Vec<f64> = flat.iter().map(|p| signed_area(p)).collect();
    let boxes: Vec<[f64; 4]> = flat
        .iter()
        .map(|p| {
            p.iter()
                .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, q| {
                    [
                        b[0].min(q[0]),
                        b[1].min(q[1]),
                        b[2].max(q[0]),
                        b[3].max(q[1]),
                    ]
                })
        })
        .collect();
    let ancestors = (0..flat.len())
        .map(|i| {
            let n = flat[i].len();
            if n < 3 {
                return Vec::new();
            }
            let probes = [flat[i][0], flat[i][n / 3], flat[i][2 * n / 3]];
            (0..flat.len())
                .filter(|&j| j != i && flat[j].len() >= 3 && signed[j].abs() > signed[i].abs())
                .filter(|&j| {
                    let b = boxes[j];
                    probes
                        .iter()
                        .all(|q| q[0] >= b[0] && q[0] <= b[2] && q[1] >= b[1] && q[1] <= b[3])
                })
                .filter(|&j| probes.iter().filter(|&&q| inside(&flat[j], q)).count() >= 2)
                .collect()
        })
        .collect();
    Nest { signed, ancestors }
}

fn split(c: &Cubic, t: f64) -> (Cubic, Cubic) {
    let [p0, p1, p2, p3] = c.0;
    let (a, b, d) = (lerp(p0, p1, t), lerp(p1, p2, t), lerp(p2, p3, t));
    let (e, f) = (lerp(a, b, t), lerp(b, d, t));
    let m = lerp(e, f, t);
    (Cubic([p0, a, e, m]), Cubic([m, f, d, p3]))
}

fn sub_curve(c: &Cubic, a: f64, b: f64) -> Cubic {
    let right = if a > 0.0 { split(c, a).1 } else { *c };
    if b >= 1.0 {
        return right;
    }
    let t = if a >= 1.0 { 1.0 } else { (b - a) / (1.0 - a) };
    split(&right, t).0
}

fn at(c: &Cubic, t: f64) -> Pt {
    let [p0, p1, p2, p3] = c.0;
    let (a, b, d) = (lerp(p0, p1, t), lerp(p1, p2, t), lerp(p2, p3, t));
    lerp(lerp(a, b, t), lerp(b, d, t), t)
}

fn arc_length(c: &Cubic) -> f64 {
    (0..32)
        .map(|i| len(sub(at(c, (i + 1) as f64 / 32.0), at(c, i as f64 / 32.0))))
        .sum()
}

/// Curve parameter at arc length `l` from the start.
fn param_at_length(c: &Cubic, l: f64) -> f64 {
    let (mut acc, mut prev) = (0.0, c.0[0]);
    for i in 1..=64 {
        let t = i as f64 / 64.0;
        let p = at(c, t);
        let d = len(sub(p, prev));
        if acc + d >= l {
            return (i as f64 - 1.0 + (l - acc) / d.max(1e-12)) / 64.0;
        }
        acc += d;
        prev = p;
    }
    1.0
}

fn tangent_start(c: &Cubic) -> Option<Pt> {
    let [p0, p1, p2, p3] = c.0;
    unit(sub(p1, p0))
        .or_else(|| unit(sub(p2, p0)))
        .or_else(|| unit(sub(p3, p0)))
}
fn tangent_end(c: &Cubic) -> Option<Pt> {
    let [p0, p1, p2, p3] = c.0;
    unit(sub(p3, p2))
        .or_else(|| unit(sub(p3, p1)))
        .or_else(|| unit(sub(p3, p0)))
}

fn line(a: Pt, b: Pt) -> Cubic {
    Cubic([a, lerp(a, b, 1.0 / 3.0), lerp(a, b, 2.0 / 3.0), b])
}

pub struct CornerParams {
    pub max_angle_deg: f64,
    pub radius_px: f64,
    pub round_concave: bool,
}

struct Corner {
    trim: f64,
    radius: f64,
}

/// Rounds sharp corners with tangent arcs. Returns the new shapes and a preview of the cut-off material.
pub fn round_corners(s: &Shapes, p: &CornerParams) -> (Shapes, Shapes, usize) {
    let flat = s.flatten(0.25);
    let nest = nest(&flat);
    let max_angle = p.max_angle_deg.to_radians();
    let mut loops = Vec::new();
    let mut preview = Vec::new();
    let mut count = 0;
    for (li, l) in s.loops.iter().enumerate() {
        let n = l.segs.len();
        let paint_inside = nest.ancestors[li].len().is_multiple_of(2);
        let lens: Vec<f64> = l.segs.iter().map(arc_length).collect();
        let mut corners: Vec<Option<Corner>> = (0..n).map(|_| None).collect();
        for k in 0..n {
            let (prev, next) = (&l.segs[(k + n - 1) % n], &l.segs[k]);
            let (Some(a), Some(b)) = (tangent_end(prev), tangent_start(next)) else {
                continue;
            };
            let turn = cross(a, b).atan2(a[0] * b[0] + a[1] * b[1]);
            let interior = std::f64::consts::PI - turn.abs();
            if turn.abs() < 1e-3 || interior >= max_angle {
                continue;
            }
            let convex_region = (turn > 0.0) == (nest.signed[li] > 0.0);
            if convex_region != paint_inside && !p.round_concave {
                continue;
            }
            let half = (interior / 2.0).tan();
            let limit = 0.5 * lens[(k + n - 1) % n].min(lens[k]);
            let trim = (p.radius_px / half).min(limit);
            if trim >= 0.05 {
                corners[k] = Some(Corner {
                    trim,
                    radius: trim * half,
                });
            }
        }
        if corners.iter().all(Option::is_none) {
            loops.push(l.clone());
            continue;
        }
        let trimmed: Vec<Cubic> = (0..n)
            .map(|i| {
                let a = corners[i]
                    .as_ref()
                    .map_or(0.0, |c| param_at_length(&l.segs[i], c.trim));
                let b = corners[(i + 1) % n]
                    .as_ref()
                    .map_or(1.0, |c| param_at_length(&l.segs[i], lens[i] - c.trim));
                sub_curve(&l.segs[i], a, b.max(a))
            })
            .collect();
        let mut segs = Vec::new();
        for i in 0..n {
            segs.push(trimmed[i]);
            let j = (i + 1) % n;
            let Some(c) = &corners[j] else { continue };
            let (e, st) = (trimmed[i].0[3], trimmed[j].0[0]);
            let (Some(d1), Some(d2)) = (tangent_end(&trimmed[i]), tangent_start(&trimmed[j]))
            else {
                segs.push(line(e, st));
                continue;
            };
            let phi = cross(d1, d2).atan2(d1[0] * d2[0] + d1[1] * d2[1]).abs();
            let k = 4.0 / 3.0 * (phi / 4.0).tan() * c.radius;
            segs.push(Cubic([
                e,
                [e[0] + d1[0] * k, e[1] + d1[1] * k],
                [st[0] - d2[0] * k, st[1] - d2[1] * k],
                st,
            ]));
            let tip = l.segs[j].0[0];
            preview.push(Loop {
                segs: vec![line(e, tip), line(tip, st), line(st, e)],
            });
            count += 1;
        }
        loops.push(Loop { segs });
    }
    (
        Shapes {
            w: s.w,
            h: s.h,
            loops,
        },
        Shapes {
            w: s.w,
            h: s.h,
            loops: preview,
        },
        count,
    )
}

pub struct IslandParams {
    pub delete_specks: bool,
    pub fill_holes: bool,
    pub max_area_px: f64,
    pub keep_largest: usize,
}

pub struct IslandReport {
    pub specks: usize,
    pub holes: usize,
    pub area_px: f64,
}

/// Removes small paint specks (even nesting depth) and/or small holes (odd depth); a removed loop takes
/// everything nested inside it along. The second shape set holds what was removed.
pub fn remove_islands(s: &Shapes, p: &IslandParams) -> (Shapes, Shapes, IslandReport) {
    let flat = s.flatten(0.25);
    let nest = nest(&flat);
    let n = s.loops.len();
    let area: Vec<f64> = nest.signed.iter().map(|a| a.abs()).collect();
    let depth: Vec<usize> = nest.ancestors.iter().map(Vec::len).collect();
    let mut gone = vec![false; n];
    for i in 0..n {
        let small = area[i] < p.max_area_px;
        gone[i] = small
            && if depth[i].is_multiple_of(2) {
                p.delete_specks
            } else {
                p.fill_holes
            };
    }
    if p.keep_largest > 0 {
        let mut outer: Vec<usize> = (0..n).filter(|&i| depth[i] == 0).collect();
        outer.sort_by(|&a, &b| area[b].total_cmp(&area[a]));
        for &i in outer.iter().skip(p.keep_largest) {
            gone[i] = true;
        }
    }
    let direct = gone.clone();
    let mut keep = Vec::new();
    let mut removed = Vec::new();
    let mut rep = IslandReport {
        specks: 0,
        holes: 0,
        area_px: 0.0,
    };
    for i in 0..n {
        let dropped = direct[i] || nest.ancestors[i].iter().any(|&a| direct[a]);
        if !dropped {
            keep.push(s.loops[i].clone());
        } else if direct[i] && !nest.ancestors[i].iter().any(|&a| direct[a]) {
            removed.push(s.loops[i].clone());
            rep.area_px += area[i];
            if depth[i].is_multiple_of(2) {
                rep.specks += 1;
            } else {
                rep.holes += 1;
            }
        }
    }
    (
        Shapes {
            w: s.w,
            h: s.h,
            loops: keep,
        },
        Shapes {
            w: s.w,
            h: s.h,
            loops: removed,
        },
        rep,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poly(pts: &[Pt]) -> Loop {
        Loop {
            segs: (0..pts.len())
                .map(|i| line(pts[i], pts[(i + 1) % pts.len()]))
                .collect(),
        }
    }
    fn rect(x: f64, y: f64, w: f64, h: f64) -> Loop {
        poly(&[[x, y], [x + w, y], [x + w, y + h], [x, y + h]])
    }
    fn area(s: &Shapes) -> f64 {
        let f = s.flatten(0.1);
        let nest = nest(&f);
        f.iter()
            .enumerate()
            .map(|(i, p)| {
                signed_area(p).abs()
                    * if nest.ancestors[i].len() % 2 == 0 {
                        1.0
                    } else {
                        -1.0
                    }
            })
            .sum()
    }

    #[test]
    fn a_spike_is_rounded_and_a_square_corner_is_kept() {
        let spike = poly(&[
            [0.0, 0.0],
            [100.0, 0.0],
            [100.0, 40.0],
            [140.0, 50.0],
            [100.0, 60.0],
            [100.0, 100.0],
            [0.0, 100.0],
        ]);
        let s = Shapes {
            w: 160,
            h: 120,
            loops: vec![spike],
        };
        let p = CornerParams {
            max_angle_deg: 60.0,
            radius_px: 3.0,
            round_concave: false,
        };
        let (r, prev, n) = round_corners(&s, &p);
        assert_eq!(n, 1);
        assert_eq!(prev.loops.len(), 1);
        assert_eq!(r.loops[0].segs.len(), s.loops[0].segs.len() + 1);
        let sq = Shapes {
            w: 120,
            h: 120,
            loops: vec![rect(10.0, 10.0, 50.0, 50.0)],
        };
        assert_eq!(round_corners(&sq, &p).2, 0);
    }

    #[test]
    fn a_paint_tip_loses_material_and_a_notch_is_left_unless_asked() {
        // thin wedge of paint pointing right: convex tip
        let wedge = Shapes {
            w: 100,
            h: 100,
            loops: vec![poly(&[[10.0, 40.0], [90.0, 50.0], [10.0, 60.0]])],
        };
        let p = CornerParams {
            max_angle_deg: 60.0,
            radius_px: 2.0,
            round_concave: false,
        };
        let (r, _, n) = round_corners(&wedge, &p);
        assert_eq!(n, 1);
        assert!(area(&r) < area(&wedge));
        // the same wedge as a hole in a plate is an empty spike (concave for the paint)
        let plate = Shapes {
            w: 100,
            h: 100,
            loops: vec![rect(0.0, 0.0, 100.0, 100.0), wedge.loops[0].clone()],
        };
        assert_eq!(round_corners(&plate, &p).2, 0);
        let q = CornerParams {
            round_concave: true,
            ..p
        };
        let (r, _, n) = round_corners(&plate, &q);
        assert_eq!(n, 1);
        assert!(area(&r) > area(&plate));
    }

    #[test]
    fn small_specks_and_holes_are_removed_with_their_contents() {
        let s = Shapes {
            w: 200,
            h: 200,
            loops: vec![
                rect(0.0, 0.0, 100.0, 100.0),
                rect(40.0, 40.0, 4.0, 4.0),
                rect(60.0, 60.0, 20.0, 20.0),
                rect(65.0, 65.0, 2.0, 2.0),
                rect(150.0, 150.0, 3.0, 3.0),
            ],
        };
        let p = IslandParams {
            delete_specks: true,
            fill_holes: false,
            max_area_px: 30.0,
            keep_largest: 0,
        };
        let (k, rm, rep) = remove_islands(&s, &p);
        assert_eq!(
            (k.loops.len(), rm.loops.len(), rep.specks, rep.holes),
            (3, 2, 2, 0)
        );
        let p = IslandParams {
            fill_holes: true,
            delete_specks: false,
            max_area_px: 30.0,
            keep_largest: 0,
        };
        let (k, _, rep) = remove_islands(&s, &p);
        assert_eq!((k.loops.len(), rep.holes), (4, 1));
        let p = IslandParams {
            fill_holes: true,
            delete_specks: true,
            max_area_px: 1000.0,
            keep_largest: 0,
        };
        let (k, _, _) = remove_islands(&s, &p);
        assert_eq!(k.loops.len(), 1);
        let p = IslandParams {
            fill_holes: false,
            delete_specks: false,
            max_area_px: 0.0,
            keep_largest: 1,
        };
        let (k, _, _) = remove_islands(&s, &p);
        assert_eq!(k.loops.len(), 4);
    }
}
