//! Vector path: trace a mask into closed Bézier shapes, and rasterise shapes back to a mask.

use std::collections::HashMap;

pub type Pt = [f64; 2];

/// One cubic Bézier segment; a straight line has its handles on the chord.
#[derive(Clone, Copy, Debug)]
pub struct Cubic(pub [Pt; 4]);

/// Closed outline. Outlines are filled with the even-odd rule, so holes need no special handling.
#[derive(Clone, Debug)]
pub struct Loop {
    pub segs: Vec<Cubic>,
}

/// Closed shapes in the pixel space of the traced mask (`w` x `h`, y down).
#[derive(Clone, Debug)]
pub struct Shapes {
    pub w: usize,
    pub h: usize,
    pub loops: Vec<Loop>,
}

pub struct TraceParams {
    /// Gaussian smoothing of the outline, in source pixels.
    pub smooth_px: f64,
    /// Turning angle that counts as a sharp corner, in degrees.
    pub corner_deg: f64,
    /// Maximum deviation allowed when dropping points, in source pixels.
    pub tolerance_px: f64,
    /// Outlines enclosing fewer pixels than this are dropped as specks.
    pub min_area_px: f64,
}

fn sub(a: Pt, b: Pt) -> Pt {
    [a[0] - b[0], a[1] - b[1]]
}
fn len(a: Pt) -> f64 {
    a[0].hypot(a[1])
}

/// Boundary loops of the paint, as unit steps between pixel corners. Paint is on the right-hand side of every step.
fn crack_loops(paint: &[bool], w: usize, h: usize) -> Vec<Vec<Pt>> {
    let at = |x: i64, y: i64| {
        x >= 0
            && y >= 0
            && (x as usize) < w
            && (y as usize) < h
            && paint[y as usize * w + x as usize]
    };
    let mut out: HashMap<(i64, i64), Vec<(i64, i64)>> = HashMap::new();
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if !at(x, y) {
                continue;
            }
            if !at(x, y - 1) {
                out.entry((x, y)).or_default().push((x + 1, y));
            }
            if !at(x + 1, y) {
                out.entry((x + 1, y)).or_default().push((x + 1, y + 1));
            }
            if !at(x, y + 1) {
                out.entry((x + 1, y + 1)).or_default().push((x, y + 1));
            }
            if !at(x - 1, y) {
                out.entry((x, y + 1)).or_default().push((x, y));
            }
        }
    }
    let mut starts: Vec<(i64, i64)> = out.keys().copied().collect();
    starts.sort();
    let mut loops = Vec::new();
    for s in starts {
        while out.get(&s).is_some_and(|v| !v.is_empty()) {
            let mut pts = vec![s];
            let (mut cur, mut dir) = (s, (0i64, 0i64));
            loop {
                let nexts = out.get_mut(&cur).expect("boundary edges form closed loops");
                // at a pinch point keep the loop on the same paint pixel: turn right first
                let pick = if dir == (0, 0) || nexts.len() == 1 {
                    0
                } else {
                    let turn = |n: &(i64, i64)| {
                        let d = (n.0 - cur.0, n.1 - cur.1);
                        dir.0 * d.1 - dir.1 * d.0
                    };
                    (0..nexts.len())
                        .max_by_key(|&i| turn(&nexts[i]))
                        .unwrap_or(0)
                };
                let n = nexts.swap_remove(pick);
                dir = (n.0 - cur.0, n.1 - cur.1);
                cur = n;
                if cur == s {
                    break;
                }
                pts.push(cur);
            }
            loops.push(pts.iter().map(|p| [p.0 as f64, p.1 as f64]).collect());
        }
    }
    loops
}

fn signed_area(p: &[Pt]) -> f64 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (u, v) = (p[i], p[(i + 1) % p.len()]);
        a += u[0] * v[1] - v[0] * u[1];
    }
    a / 2.0
}

fn smooth_run(run: &[Pt], sigma: f64, closed: bool) -> Vec<Pt> {
    if sigma <= 0.0 || run.len() < 3 {
        return run.to_vec();
    }
    let r = (sigma * 3.0).ceil() as i64;
    let k: Vec<f64> = (-r..=r)
        .map(|i| (-((i * i) as f64) / (2.0 * sigma * sigma)).exp())
        .collect();
    let n = run.len() as i64;
    let idx = |i: i64| -> usize {
        if closed {
            i.rem_euclid(n) as usize
        } else {
            i.clamp(0, n - 1) as usize
        }
    };
    let mut out = Vec::with_capacity(run.len());
    for i in 0..n {
        let (mut sx, mut sy, mut sw) = (0.0, 0.0, 0.0);
        for (j, kv) in k.iter().enumerate() {
            let p = run[idx(i + j as i64 - r)];
            sx += kv * p[0];
            sy += kv * p[1];
            sw += kv;
        }
        out.push([sx / sw, sy / sw]);
    }
    if !closed {
        out[0] = run[0];
        out[run.len() - 1] = run[run.len() - 1];
    }
    out
}

fn rdp(p: &[Pt], tol: f64, keep: &mut Vec<bool>, lo: usize, hi: usize) {
    if hi <= lo + 1 {
        return;
    }
    let (a, b) = (p[lo], p[hi]);
    let d = sub(b, a);
    let l = len(d).max(1e-12);
    let (mut worst, mut wi) = (0.0, lo);
    for (i, q) in p.iter().enumerate().take(hi).skip(lo + 1) {
        let e = sub(*q, a);
        let dist = if len(d) < 1e-12 {
            len(e)
        } else {
            (e[0] * d[1] - e[1] * d[0]).abs() / l
        };
        if dist > worst {
            (worst, wi) = (dist, i);
        }
    }
    if worst > tol {
        keep[wi] = true;
        rdp(p, tol, keep, lo, wi);
        rdp(p, tol, keep, wi, hi);
    }
}

/// Anchors of an open run, ends included.
fn simplify_run(run: &[Pt], tol: f64) -> Vec<Pt> {
    if run.len() < 3 {
        return run.to_vec();
    }
    let mut keep = vec![false; run.len()];
    keep[0] = true;
    keep[run.len() - 1] = true;
    rdp(run, tol, &mut keep, 0, run.len() - 1);
    run.iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(p, _)| *p)
        .collect()
}

fn corners_of(p: &[Pt], k: usize, min_deg: f64) -> Vec<usize> {
    let n = p.len();
    if n < 4 * k.max(1) {
        return vec![];
    }
    let angle: Vec<f64> = (0..n)
        .map(|i| {
            let a = sub(p[i], p[(i + n - k) % n]);
            let b = sub(p[(i + k) % n], p[i]);
            let c = ((a[0] * b[0] + a[1] * b[1]) / (len(a) * len(b)).max(1e-12)).clamp(-1.0, 1.0);
            c.acos().to_degrees()
        })
        .collect();
    (0..n)
        .filter(|&i| {
            angle[i] >= min_deg
                && (1..=k).all(|d| {
                    let (l, r) = (angle[(i + n - d) % n], angle[(i + d) % n]);
                    angle[i] > l && angle[i] >= r
                })
        })
        .collect()
}

/// Cubic segments through the anchors; anchors flagged as corners get straight-in, straight-out tangents.
fn bezier_through(anchors: &[Pt], corner: &[bool], closed: bool) -> Vec<Cubic> {
    let n = anchors.len();
    let segs = if closed { n } else { n - 1 };
    let at = |i: isize| -> Pt {
        if closed {
            anchors[i.rem_euclid(n as isize) as usize]
        } else {
            anchors[i.clamp(0, n as isize - 1) as usize]
        }
    };
    let is_corner = |i: isize| {
        corner[i.rem_euclid(n as isize) as usize] || (!closed && (i == 0 || i == n as isize - 1))
    };
    // forward tangent at anchor i for a segment toward `other`, limited to the chord so the curve cannot overshoot
    let tangent = |i: isize, other: Pt, forward: bool| -> Pt {
        let p = at(i);
        let chord = sub(other, p);
        if is_corner(i) {
            return if forward {
                chord
            } else {
                [-chord[0], -chord[1]]
            };
        }
        let t = sub(at(i + 1), at(i - 1));
        let t = [t[0] / 2.0, t[1] / 2.0];
        let l = len(t);
        let c = len(chord);
        if l > c && l > 0.0 {
            [t[0] * c / l, t[1] * c / l]
        } else {
            t
        }
    };
    (0..segs as isize)
        .map(|i| {
            let (a, b) = (at(i), at(i + 1));
            let ta = tangent(i, b, true);
            let tb = tangent(i + 1, a, false);
            Cubic([
                a,
                [a[0] + ta[0] / 3.0, a[1] + ta[1] / 3.0],
                [b[0] - tb[0] / 3.0, b[1] - tb[1] / 3.0],
                b,
            ])
        })
        .collect()
}

fn fit_loop(pts: &[Pt], p: &TraceParams) -> Option<Loop> {
    let k = ((2.0 * p.smooth_px).round() as usize).max(3);
    let corners = if p.corner_deg < 180.0 {
        corners_of(pts, k, p.corner_deg)
    } else {
        vec![]
    };
    let mut anchors: Vec<Pt> = Vec::new();
    let mut is_corner: Vec<bool> = Vec::new();
    if corners.is_empty() {
        let sm = smooth_run(pts, p.smooth_px, true);
        let far = (0..sm.len())
            .max_by(|&a, &b| len(sub(sm[a], sm[0])).total_cmp(&len(sub(sm[b], sm[0]))))
            .unwrap_or(0);
        let mut first: Vec<Pt> = sm[..=far].to_vec();
        let mut second: Vec<Pt> = sm[far..].to_vec();
        second.push(sm[0]);
        first = simplify_run(&first, p.tolerance_px);
        second = simplify_run(&second, p.tolerance_px);
        anchors.extend_from_slice(&first[..first.len() - 1]);
        anchors.extend_from_slice(&second[..second.len() - 1]);
        is_corner = vec![false; anchors.len()];
    } else {
        let n = pts.len();
        for (ci, &c) in corners.iter().enumerate() {
            let next = corners[(ci + 1) % corners.len()];
            let len_run = (next + n - c) % n + 1;
            let run: Vec<Pt> = (0..if corners.len() == 1 { n + 1 } else { len_run })
                .map(|d| pts[(c + d) % n])
                .collect();
            let run = simplify_run(&smooth_run(&run, p.smooth_px, false), p.tolerance_px);
            anchors.extend_from_slice(&run[..run.len() - 1]);
            is_corner.push(true);
            is_corner.extend(std::iter::repeat(false).take(run.len() - 2));
        }
    }
    if anchors.len() < 3 {
        return None;
    }
    Some(Loop {
        segs: bezier_through(&anchors, &is_corner, true),
    })
}

/// Traces the paint of a mask into smooth closed shapes that keep sharp corners.
pub fn trace(paint: &[bool], w: usize, h: usize, p: &TraceParams) -> Shapes {
    let mut loops = Vec::new();
    for pts in crack_loops(paint, w, h) {
        if signed_area(&pts).abs() < p.min_area_px.max(0.5) {
            continue;
        }
        if let Some(l) = fit_loop(&pts, p) {
            loops.push(l);
        }
    }
    Shapes { w, h, loops }
}

fn flatten_cubic(c: &Cubic, tol: f64, out: &mut Vec<Pt>) {
    let [p0, p1, p2, p3] = c.0;
    let dd = len([p0[0] - 2.0 * p1[0] + p2[0], p0[1] - 2.0 * p1[1] + p2[1]]).max(len([
        p1[0] - 2.0 * p2[0] + p3[0],
        p1[1] - 2.0 * p2[1] + p3[1],
    ]));
    let n = ((0.75 * dd / tol.max(1e-9)).sqrt().ceil() as usize).clamp(1, 256);
    for i in 0..n {
        let t = i as f64 / n as f64;
        let u = 1.0 - t;
        let b = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
        out.push([
            b[0] * p0[0] + b[1] * p1[0] + b[2] * p2[0] + b[3] * p3[0],
            b[0] * p0[1] + b[1] * p1[1] + b[2] * p2[1] + b[3] * p3[1],
        ]);
    }
}

impl Shapes {
    /// Closed polylines that stay within `tol` (in source pixels) of the curves.
    pub fn flatten(&self, tol: f64) -> Vec<Vec<Pt>> {
        self.loops
            .iter()
            .map(|l| {
                let mut v = Vec::new();
                for s in &l.segs {
                    flatten_cubic(s, tol, &mut v);
                }
                v
            })
            .collect()
    }

    pub fn to_svg(&self) -> String {
        let mut d = String::new();
        for l in &self.loops {
            let s = l.segs[0].0[0];
            d += &format!("M{:.2} {:.2}", s[0], s[1]);
            for c in &l.segs {
                let [_, a, b, e] = c.0;
                d += &format!(
                    "C{:.2} {:.2} {:.2} {:.2} {:.2} {:.2}",
                    a[0], a[1], b[0], b[1], e[0], e[1]
                );
            }
            d += "Z";
        }
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\"><rect width=\"{w}\" height=\"{h}\" fill=\"#fff\"/><path d=\"{d}\" fill=\"#000\" fill-rule=\"evenodd\"/></svg>",
            w = self.w,
            h = self.h
        )
    }

    pub fn segment_count(&self) -> usize {
        self.loops.iter().map(|l| l.segs.len()).sum()
    }
}

/// Even-odd fill of the shapes on a `gw` x `gh` grid; pixel centres are sampled. `true` = paint.
pub fn rasterise(s: &Shapes, gw: usize, gh: usize) -> Vec<bool> {
    let (sx, sy) = (gw as f64 / s.w as f64, gh as f64 / s.h as f64);
    let tol = 0.1 / sx.max(sy).max(1e-9);
    let polys = s.flatten(tol.min(0.25));
    let mut out = vec![false; gw * gh];
    let mut xs: Vec<f64> = Vec::new();
    for y in 0..gh {
        let yc = (y as f64 + 0.5) / sy;
        xs.clear();
        for poly in &polys {
            for i in 0..poly.len() {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                if (a[1] <= yc) != (b[1] <= yc) {
                    xs.push(a[0] + (yc - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
                }
            }
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        for pair in xs.chunks_exact(2) {
            let x0 = ((pair[0] * sx - 0.5).ceil().max(0.0)) as usize;
            let x1 = ((pair[1] * sx - 0.5).floor() + 1.0).clamp(0.0, gw as f64) as usize;
            for x in x0..x1.max(x0) {
                if x < gw {
                    out[y * gw + x] = true;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disc_with_hole(n: usize) -> Vec<bool> {
        let c = n as f64 / 2.0;
        (0..n * n)
            .map(|i| {
                let d = ((i % n) as f64 + 0.5 - c).hypot((i / n) as f64 + 0.5 - c);
                d < c * 0.9 && d > c * 0.4
            })
            .collect()
    }

    fn params() -> TraceParams {
        TraceParams {
            smooth_px: 1.5,
            corner_deg: 60.0,
            tolerance_px: 0.4,
            min_area_px: 2.0,
        }
    }

    #[test]
    fn ring_traces_to_two_loops_and_round_trips() {
        let n = 80;
        let m = disc_with_hole(n);
        let s = trace(&m, n, n, &params());
        assert_eq!(s.loops.len(), 2, "outer edge and hole");
        let back = rasterise(&s, n, n);
        let diff = m.iter().zip(&back).filter(|(a, b)| a != b).count();
        assert!(diff * 100 < m.iter().filter(|&&v| v).count(), "diff {diff}");
    }

    #[test]
    fn square_keeps_four_sharp_corners() {
        let n = 40;
        let m: Vec<bool> = (0..n * n)
            .map(|i| (10..30).contains(&(i % n)) && (10..30).contains(&(i / n)))
            .collect();
        let s = trace(&m, n, n, &params());
        assert_eq!(s.loops.len(), 1);
        let poly = &s.flatten(0.05)[0];
        let (mn, mx) = poly
            .iter()
            .fold(([f64::MAX; 2], [f64::MIN; 2]), |(a, b), p| {
                (
                    [a[0].min(p[0]), a[1].min(p[1])],
                    [b[0].max(p[0]), b[1].max(p[1])],
                )
            });
        assert!(
            (mn[0] - 10.0).abs() < 0.2
                && (mx[0] - 30.0).abs() < 0.2
                && (mn[1] - 10.0).abs() < 0.2
                && (mx[1] - 30.0).abs() < 0.2,
            "{mn:?} {mx:?}"
        );
    }

    #[test]
    fn rasterising_at_higher_resolution_scales_the_area() {
        let n = 60;
        let s = trace(&disc_with_hole(n), n, n, &params());
        let lo = rasterise(&s, n, n).iter().filter(|&&v| v).count() as f64;
        let hi = rasterise(&s, n * 4, n * 4).iter().filter(|&&v| v).count() as f64 / 16.0;
        assert!((lo - hi).abs() / lo < 0.03, "{lo} vs {hi}");
    }
}
