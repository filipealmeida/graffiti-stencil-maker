//! Margin expansion: a ring of blank slabs around a finished plate, screwed to it through the plate's own mounting
//! holes with joiner plates across the seams. This module is the layout (pure geometry); the node builds the meshes.

type P = (f64, f64);

pub struct RingParams {
    /// Lower-left corner and size of the plate, in the frame of the printed tiles.
    pub lo: [f64; 2],
    pub w: f64,
    pub h: f64,
    /// Half the plate margin: the mounting holes sit this far inside the edge, and the new ones this far outside it.
    pub c: f64,
    pub ring: f64,
    pub max_x: f64,
    pub max_y: f64,
    /// Through-hole and joiner-plate radii.
    pub hole_r: f64,
    pub join_r: f64,
    /// The plate's mounting holes that no joiner plate uses yet.
    pub holes: Vec<P>,
    /// Holes along the outer edge of the ring (0 = none).
    pub wall_spacing: f64,
}

#[derive(Debug)]
pub struct Slab {
    pub name: String,
    pub lo: [f64; 2],
    pub hi: [f64; 2],
    /// Absolute hole centres inside the slab.
    pub holes: Vec<P>,
}

#[derive(Debug)]
pub struct Ring {
    pub slabs: Vec<Slab>,
    /// Every joiner plate: the two hole centres it spans.
    pub joints: Vec<(P, P)>,
    /// The old mounting holes now used by the ring.
    pub used: Vec<P>,
    pub wall_holes: usize,
    pub unused: usize,
}

struct Side {
    name: &'static str,
    horizontal: bool,
    // outward direction along the other axis, and where the plate edge is
    sign: f64,
    edge: f64,
}

fn d(a: P, b: P) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

pub fn plan(q: &RingParams) -> Result<Ring, String> {
    let (lx, ly) = (q.lo[0], q.lo[1]);
    let (c, r) = (q.c, q.ring);
    if c <= 0.0 {
        return Err(
            "the plate has no margin: a margin expansion needs a margin greater than zero".into(),
        );
    }
    if r < 2.0 * c + 2.0 * q.join_r {
        return Err(format!(
            "the ring is {r:.0} mm wide but needs at least {:.0} mm to hold the screw holes",
            2.0 * c + 2.0 * q.join_r
        ));
    }
    if r > q.max_x || r > q.max_y {
        return Err(
            "the ring is wider than the largest slab: raise the maximum width and height".into(),
        );
    }
    let sides = [
        Side {
            name: "bottom",
            horizontal: true,
            sign: -1.0,
            edge: ly,
        },
        Side {
            name: "top",
            horizontal: true,
            sign: 1.0,
            edge: ly + q.h,
        },
        Side {
            name: "left",
            horizontal: false,
            sign: -1.0,
            edge: lx,
        },
        Side {
            name: "right",
            horizontal: false,
            sign: 1.0,
            edge: lx + q.w,
        },
    ];
    // the point `along` the side and `out` mm outside the plate edge
    let pt = |s: &Side, along: f64, out: f64| -> P {
        if s.horizontal {
            (along, s.edge + s.sign * out)
        } else {
            (s.edge + s.sign * out, along)
        }
    };
    let span = |s: &Side| -> (f64, f64) {
        if s.horizontal {
            (lx, lx + q.w)
        } else {
            (ly, ly + q.h)
        }
    };
    let clear = 2.0 * q.join_r + 1.0;
    let mut joints: Vec<(P, P)> = vec![];
    let mut used = vec![];
    let mut seams: Vec<Vec<f64>> = vec![];
    let mut taken = vec![false; q.holes.len()];

    // 1. which old holes along each outer edge become joints (greedy, none closer than two joiner plates allow)
    let mut plate_joints: Vec<Vec<(f64, P, P)>> = vec![];
    for s in &sides {
        let (a, b) = span(s);
        let mut cand: Vec<(f64, usize)> = q
            .holes
            .iter()
            .enumerate()
            .filter_map(|(i, h)| {
                let (along, inward) = if s.horizontal {
                    (h.0, (h.1 - s.edge) * -s.sign)
                } else {
                    (h.1, (h.0 - s.edge) * -s.sign)
                };
                (!taken[i] && (inward - c).abs() < 0.6 && along > a + c && along < b - c)
                    .then_some((along, i))
            })
            .collect();
        cand.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
        let mut chosen: Vec<(f64, P, P)> = vec![];
        for (along, i) in cand {
            if chosen.iter().all(|k| d(k.1, q.holes[i]) >= clear) {
                taken[i] = true;
                chosen.push((along, q.holes[i], pt(s, along, c)));
            }
        }
        plate_joints.push(chosen);
    }
    for list in &plate_joints {
        for &(_, old, new) in list {
            joints.push((old, new));
            used.push(old);
        }
    }

    // 2. slab seams along each side, nudged clear of the joints
    let mut all: Vec<P> = joints.iter().flat_map(|j| [j.0, j.1]).collect();
    for (si, s) in sides.iter().enumerate() {
        let (a, b) = span(s);
        let len = b - a;
        let cap = if s.horizontal { q.max_x } else { q.max_y };
        let n = (len / cap).ceil().max(1.0) as usize;
        let mut cuts = vec![];
        for k in 1..n {
            let want = a + len * k as f64 / n as f64;
            let reach = (len / n as f64 / 2.0 - c - 1.0).max(0.0);
            let mut best = None;
            let mut off = 0.0;
            while off <= reach && best.is_none() {
                for t in [want + off, want - off] {
                    let hs = [pt(s, t - c, q.ring / 2.0), pt(s, t + c, q.ring / 2.0)];
                    if hs.iter().all(|&h| all.iter().all(|&o| d(h, o) >= clear)) {
                        best = Some(t);
                        break;
                    }
                }
                off += 1.0;
            }
            let t = best.ok_or_else(|| {
                format!(
                    "no clear place for a joiner plate between the slabs along the {} edge: use a different maximum size",
                    s.name
                )
            })?;
            for h in [pt(s, t - c, q.ring / 2.0), pt(s, t + c, q.ring / 2.0)] {
                all.push(h);
            }
            cuts.push(t);
        }
        let _ = si;
        seams.push(cuts);
    }

    // 3. slabs: four corners, then each side cut at its seams
    let mut slabs: Vec<Slab> = vec![];
    let (x1, y1) = (lx + q.w, ly + q.h);
    for (name, xa, ya) in [
        ("bl", lx - r, ly - r),
        ("br", x1, ly - r),
        ("tl", lx - r, y1),
        ("tr", x1, y1),
    ] {
        slabs.push(Slab {
            name: format!("ring_corner_{name}"),
            lo: [xa, ya],
            hi: [xa + r, ya + r],
            holes: vec![],
        });
    }
    for (si, s) in sides.iter().enumerate() {
        let (a, b) = span(s);
        let mut edges = vec![a];
        edges.extend(&seams[si]);
        edges.push(b);
        for (k, w) in edges.windows(2).enumerate() {
            let (lo, hi) = if s.horizontal {
                let (y0, y1) = if s.sign < 0.0 {
                    (s.edge - r, s.edge)
                } else {
                    (s.edge, s.edge + r)
                };
                ([w[0], y0], [w[1], y1])
            } else {
                let (x0, x1) = if s.sign < 0.0 {
                    (s.edge - r, s.edge)
                } else {
                    (s.edge, s.edge + r)
                };
                ([x0, w[0]], [x1, w[1]])
            };
            slabs.push(Slab {
                name: format!("ring_{}_{}", s.name, k + 1),
                lo,
                hi,
                holes: vec![],
            });
        }
    }

    // 4. seam joints, including the ones against the corner slabs
    for (si, s) in sides.iter().enumerate() {
        let (a, b) = span(s);
        let mut ts = vec![a, b];
        ts.extend(&seams[si]);
        for t in ts {
            joints.push((pt(s, t - c, r / 2.0), pt(s, t + c, r / 2.0)));
        }
    }
    let inside = |sl: &Slab, h: P, m: f64| {
        h.0 > sl.lo[0] + m && h.0 < sl.hi[0] - m && h.1 > sl.lo[1] + m && h.1 < sl.hi[1] - m
    };
    for &(p1, p2) in &joints {
        for h in [p1, p2] {
            if let Some(sl) = slabs.iter_mut().find(|sl| inside(sl, h, 0.0)) {
                sl.holes.push(h);
            }
        }
    }
    for sl in &slabs {
        if sl.holes.is_empty() {
            return Err(format!(
                "slab {} would have no screw holes: the plate has no mounting hole along it",
                sl.name
            ));
        }
        if sl.name.starts_with("ring_") && !sl.name.contains("corner") {
            let plate_side = sl
                .holes
                .iter()
                .any(|h| joints.iter().take(used.len()).any(|j| j.1 == *h));
            if !plate_side {
                return Err(format!(
                    "slab {} is not joined to the plate: no mounting hole of the plate lies along it",
                    sl.name
                ));
            }
        }
    }

    // 5. optional mounting holes along the outer edge of the ring
    let mut wall = 0;
    if q.wall_spacing > 0.0 {
        let m = q.hole_r + 1.5;
        let near = r - c;
        let mut placed: Vec<P> = joints.iter().flat_map(|j| [j.0, j.1]).collect();
        for s in &sides {
            let (a, b) = span(s);
            let (t0, t1) = (a - near, b + near);
            let n = ((t1 - t0) / q.wall_spacing).ceil().max(1.0) as usize;
            let ks: Vec<usize> = if s.horizontal {
                (0..=n).collect()
            } else {
                (1..n).collect()
            };
            for k in ks {
                let want = t0 + (t1 - t0) * k as f64 / n as f64;
                let reach = (t1 - t0) / n as f64 / 2.0;
                let mut spot = None;
                let mut off = 0.0;
                while off <= reach && spot.is_none() {
                    for t in [want + off, want - off] {
                        let h = pt(s, t, near);
                        if slabs.iter().any(|sl| inside(sl, h, m))
                            && placed.iter().all(|&o| d(h, o) >= clear)
                        {
                            spot = Some(h);
                            break;
                        }
                    }
                    off += 1.0;
                }
                let h = spot.ok_or_else(|| {
                    format!(
                        "no clear spot for a mounting hole on the {} edge of the ring",
                        s.name
                    )
                })?;
                placed.push(h);
                if let Some(sl) = slabs.iter_mut().find(|sl| inside(sl, h, m)) {
                    sl.holes.push(h);
                }
                wall += 1;
            }
        }
    }
    Ok(Ring {
        slabs,
        joints,
        used,
        wall_holes: wall,
        unused: q.holes.len() - taken.iter().filter(|t| **t).count(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(holes: Vec<P>, wall: f64) -> RingParams {
        RingParams {
            lo: [0.0, 0.0],
            w: 200.0,
            h: 200.0,
            c: 5.0,
            ring: 40.0,
            max_x: 120.0,
            max_y: 120.0,
            hole_r: 2.3,
            join_r: 5.0,
            holes,
            wall_spacing: wall,
        }
    }

    fn edge_holes() -> Vec<P> {
        let mut v = vec![];
        for t in [50.0, 100.0, 150.0] {
            v.extend([(t, 5.0), (t, 195.0), (5.0, t), (195.0, t)]);
        }
        v
    }

    #[test]
    fn four_sides_and_corners_are_joined_through_the_old_holes() {
        let r = plan(&params(edge_holes(), 0.0)).unwrap();
        // 4 corners + 2 slabs along each side (200 mm with a 120 mm cap)
        assert_eq!(r.slabs.len(), 12);
        assert_eq!(r.used.len(), 12);
        for s in &r.slabs {
            assert!(!s.holes.is_empty(), "{}", s.name);
            for h in &s.holes {
                assert!(h.0 > s.lo[0] && h.0 < s.hi[0] && h.1 > s.lo[1] && h.1 < s.hi[1]);
            }
        }
        // each plate joint pairs an old hole with one the same distance outside the edge
        assert!(r.joints[..12]
            .iter()
            .all(|j| (d(j.0, j.1) - 10.0).abs() < 1e-9));
    }

    #[test]
    fn wall_holes_run_round_the_outer_edge() {
        let r = plan(&params(edge_holes(), 80.0)).unwrap();
        assert!(r.wall_holes >= 12, "{}", r.wall_holes);
    }

    #[test]
    fn no_margin_or_no_holes_is_refused() {
        let mut p = params(edge_holes(), 0.0);
        p.c = 0.0;
        assert!(plan(&p).unwrap_err().contains("margin"));
        assert!(plan(&params(vec![], 0.0)).is_err());
    }
}
