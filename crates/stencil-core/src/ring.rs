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
    /// Position of this ring (1 = next to the plate), used in the slab names.
    pub ring_no: usize,
    /// Slabs of the ring inside this one (empty for the first): the old holes must fall in one of them.
    pub prev: Vec<([f64; 2], [f64; 2])>,
    /// Holes already there that new ones must keep clear of.
    pub avoid: Vec<P>,
    /// Wanted distance between joint holes within one slab of the inner ring (0 = only what the joiners need).
    pub min_gap: f64,
}

/// Candidate screw holes for the next ring: evenly along each side of the rectangle, `c` inside its edge.
pub fn side_holes(lo: [f64; 2], w: f64, h: f64, c: f64, spacing: f64) -> Vec<P> {
    let mut v = vec![];
    for (len, horizontal) in [(w, true), (h, false)] {
        let n = (len / spacing.max(2.0)).ceil().max(1.0) as usize;
        for k in 0..n {
            let t = len * (k as f64 + 0.5) / n as f64;
            if horizontal {
                v.push((lo[0] + t, lo[1] + c));
                v.push((lo[0] + t, lo[1] + h - c));
            } else {
                v.push((lo[0] + c, lo[1] + t));
                v.push((lo[0] + w - c, lo[1] + t));
            }
        }
    }
    v
}

#[derive(Debug)]
pub struct Slab {
    pub name: String,
    pub lo: [f64; 2],
    pub hi: [f64; 2],
    /// Absolute hole centres inside the slab.
    pub holes: Vec<P>,
}

/// Seven-segment strokes (a top, b top right, c bottom right, d bottom, e bottom left, f top left, g middle).
const SEGMENTS: [&str; 10] = [
    "abcdef", "bc", "abdeg", "abcdg", "bcfg", "acdfg", "acdefg", "abc", "abcdefg", "abcdfg",
];

/// The number `n` as rectangles `[x0, y0, x1, y1]` for a pocket, `size` mm tall, centred on the origin, with its overall width.
pub fn number_rects(n: usize, size: f64) -> (Vec<[f64; 4]>, f64) {
    let (dw, t) = (size * 0.55, size * 0.17);
    let gap = size * 0.25;
    let digits: Vec<usize> = n.to_string().bytes().map(|b| (b - b'0') as usize).collect();
    let total = digits.len() as f64 * dw + (digits.len() - 1) as f64 * gap;
    let mut out = vec![];
    for (k, d) in digits.iter().enumerate() {
        let x0 = -total / 2.0 + k as f64 * (dw + gap);
        let (x1, y0, y1, ym) = (x0 + dw, -size / 2.0, size / 2.0, 0.0);
        for c in SEGMENTS[*d].chars() {
            out.push(match c {
                'a' => [x0, y1 - t, x1, y1],
                'b' => [x1 - t, ym, x1, y1],
                'c' => [x1 - t, y0, x1, ym],
                'd' => [x0, y0, x1, y0 + t],
                'e' => [x0, y0, x0 + t, ym],
                'f' => [x0, ym, x0 + t, y1],
                _ => [x0, ym - t / 2.0, x1, ym + t / 2.0],
            });
        }
    }
    (out, total)
}

/// Where the number `n` fits on a slab clear of its holes: its centre and the strokes, or None if even a small one does not fit.
pub fn place_number(sl: &Slab, n: usize, want: f64, hole_r: f64) -> Option<(P, Vec<[f64; 4]>)> {
    let (w, h) = (sl.hi[0] - sl.lo[0], sl.hi[1] - sl.lo[1]);
    let mut size = want.min(h * 0.6).min(w * 0.6);
    while size >= 4.0 {
        let (rects, tw) = number_rects(n, size);
        if tw + 4.0 <= w {
            let tries = 9;
            for k in 0..tries {
                // from the middle outwards, along the long side
                let f =
                    0.5 + ((k + 1) / 2) as f64 / tries as f64 * if k % 2 == 1 { 1.0 } else { -1.0 };
                let (cx, cy) = if w >= h {
                    (
                        sl.lo[0] + tw / 2.0 + 2.0 + f * (w - tw - 4.0),
                        (sl.lo[1] + sl.hi[1]) / 2.0,
                    )
                } else {
                    ((sl.lo[0] + sl.hi[0]) / 2.0, (sl.lo[1] + sl.hi[1]) / 2.0)
                };
                let clear = sl.holes.iter().all(|&hp| {
                    let dx = (hp.0 - cx).abs() - tw / 2.0;
                    let dy = (hp.1 - cy).abs() - size / 2.0;
                    dx.max(0.0).hypot(dy.max(0.0)) > hole_r + 2.0
                });
                if clear && cx - tw / 2.0 > sl.lo[0] + 1.0 && cx + tw / 2.0 < sl.hi[0] - 1.0 {
                    return Some(((cx, cy), rects));
                }
            }
        }
        size -= 1.0;
    }
    None
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
        let group = |h: P| {
            q.prev
                .iter()
                .position(|p| h.0 > p.0[0] && h.0 < p.1[0] && h.1 > p.0[1] && h.1 < p.1[1])
        };
        for (along, i) in cand {
            let h = q.holes[i];
            let fits = q.prev.is_empty()
                || q.prev.iter().any(|p| {
                    let m = q.hole_r + 1.5;
                    h.0 > p.0[0] + m && h.0 < p.1[0] - m && h.1 > p.0[1] + m && h.1 < p.1[1] - m
                });
            if fits
                && q.avoid.iter().all(|&o| d(o, h) >= clear)
                && chosen.iter().all(|k| {
                    d(k.1, h) >= clear && (group(k.1) != group(h) || d(k.1, h) >= q.min_gap)
                })
            {
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
    let mut all: Vec<P> = joints
        .iter()
        .flat_map(|j| [j.0, j.1])
        .chain(q.avoid.iter().copied())
        .collect();
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
            name: format!("ring_{}_corner_{name}", q.ring_no),
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
                name: format!("ring_{}_{}_{}", q.ring_no, s.name, k + 1),
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
    }
    // every slab must be held to the plate, directly or through the slabs next to it
    let at = |h: P| {
        slabs
            .iter()
            .position(|sl| h.0 > sl.lo[0] && h.0 < sl.hi[0] && h.1 > sl.lo[1] && h.1 < sl.hi[1])
    };
    let mut root: Vec<usize> = (0..slabs.len()).collect();
    fn find(r: &mut [usize], mut i: usize) -> usize {
        while r[i] != i {
            r[i] = r[r[i]];
            i = r[i];
        }
        i
    }
    let mut anchored = vec![false; slabs.len()];
    for (k, j) in joints.iter().enumerate() {
        if k < used.len() {
            if let Some(i) = at(j.1) {
                anchored[i] = true;
            }
        } else if let (Some(a), Some(b)) = (at(j.0), at(j.1)) {
            let (ra, rb) = (find(&mut root, a), find(&mut root, b));
            root[ra] = rb;
        }
    }
    let held: Vec<bool> = (0..slabs.len())
        .map(|i| {
            let r = find(&mut root, i);
            (0..slabs.len()).any(|k| anchored[k] && find(&mut root, k) == r)
        })
        .collect();
    if let Some(i) = held.iter().position(|h| !h) {
        return Err(format!(
            "slab {} cannot be held to the plate: the plate has no mounting hole along this side of the ring or the next ones",
            slabs[i].name
        ));
    }

    // 5. optional mounting holes along the outer edge of the ring
    let mut wall = 0;
    if q.wall_spacing > 0.0 {
        let m = q.hole_r + 1.5;
        let near = r - c;
        let mut placed: Vec<P> = joints
            .iter()
            .flat_map(|j| [j.0, j.1])
            .chain(q.avoid.iter().copied())
            .collect();
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
            ring_no: 1,
            prev: vec![],
            avoid: vec![],
            min_gap: 0.0,
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
    fn a_second_ring_uses_holes_added_inside_the_first() {
        let r1 = plan(&params(edge_holes(), 0.0)).unwrap();
        let prev: Vec<_> = r1.slabs.iter().map(|s| (s.lo, s.hi)).collect();
        let avoid: Vec<P> = r1.slabs.iter().flat_map(|s| s.holes.clone()).collect();
        let mut p = params(side_holes([-40.0, -40.0], 280.0, 280.0, 5.0, 5.0), 0.0);
        p.lo = [-40.0, -40.0];
        p.w = 280.0;
        p.h = 280.0;
        p.ring_no = 2;
        p.prev = prev;
        p.avoid = avoid;
        p.min_gap = 100.0;
        let r2 = plan(&p).unwrap();
        assert_eq!(r2.slabs.len(), 16);
        assert!(r2.slabs.iter().all(|s| s.name.starts_with("ring_2_")));
    }

    #[test]
    fn a_slab_without_a_plate_hole_is_held_by_its_neighbours() {
        // a 220 mm side with one hole in the middle is cut in two slabs, and only one of them can get the hole
        let mut p = params(
            vec![
                (5.0, 110.0),
                (50.0, 5.0),
                (150.0, 5.0),
                (50.0, 215.0),
                (150.0, 215.0),
                (195.0, 110.0),
            ],
            0.0,
        );
        p.h = 220.0;
        let r = plan(&p).unwrap();
        assert!(r.slabs.iter().all(|s| !s.holes.is_empty()));
    }

    #[test]
    fn numbers_fit_clear_of_the_holes() {
        let r = plan(&params(edge_holes(), 0.0)).unwrap();
        for (i, sl) in r.slabs.iter().enumerate() {
            let (c, rects) =
                place_number(sl, i + 1, 10.0, 2.3).unwrap_or_else(|| panic!("{}", sl.name));
            assert!(!rects.is_empty());
            assert!(c.0 > sl.lo[0] && c.0 < sl.hi[0] && c.1 > sl.lo[1] && c.1 < sl.hi[1]);
        }
        assert_eq!(number_rects(8, 10.0).0.len(), 7);
        assert_eq!(number_rects(11, 10.0).0.len(), 4);
    }

    #[test]
    fn no_margin_or_no_holes_is_refused() {
        let mut p = params(edge_holes(), 0.0);
        p.c = 0.0;
        assert!(plan(&p).unwrap_err().contains("margin"));
        assert!(plan(&params(vec![], 0.0)).is_err());
    }
}
