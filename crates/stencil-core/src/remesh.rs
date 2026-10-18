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

/// How far the points strictly between `a` and `b` lie outside and inside the chord a-b.
fn chord_cost(pts: &[[f64; 2]], a: usize, b: usize, outward_is_right: bool) -> (f64, f64) {
    let (dx, dy) = (pts[b][0] - pts[a][0], pts[b][1] - pts[a][1]);
    let len = (dx * dx + dy * dy).sqrt();
    let (mut out, mut inn) = (0.0f64, 0.0f64);
    for p in &pts[a + 1..b] {
        let d = if len == 0.0 {
            0.0
        } else {
            let left = (dx * (p[1] - pts[a][1]) - dy * (p[0] - pts[a][0])) / len;
            if outward_is_right {
                -left
            } else {
                left
            }
        };
        // A point outside the chord means the chord cuts into the material.
        if d > 0.0 {
            inn = inn.max(d);
        } else {
            out = out.max(-d);
        }
    }
    (out, inn)
}

/// Greedy vertex removal on an open polyline with fixed ends. The chord may add material up to
/// `limit` and remove at most `limit * loss`. Returns the kept indices and the largest error accepted.
fn simplify_open(
    pts: &[[f64; 2]],
    limit: f64,
    loss: f64,
    outward_is_right: bool,
) -> (Vec<usize>, f64) {
    let n = pts.len();
    let mut prev: Vec<usize> = (0..n).map(|i| i.saturating_sub(1)).collect();
    let mut next: Vec<usize> = (0..n).map(|i| (i + 1).min(n - 1)).collect();
    let mut alive = vec![true; n];
    let mut version = vec![0u32; n];
    let key = |c: f64| (c * 16.0) as u64;
    let eval = |i: usize, prev: &[usize], next: &[usize]| -> Option<f64> {
        let (out, inn) = chord_cost(pts, prev[i], next[i], outward_is_right);
        (out <= limit && inn <= limit * loss).then_some(out.max(inn))
    };
    let mut heap = BinaryHeap::new();
    for i in 1..n.saturating_sub(1) {
        if let Some(c) = eval(i, &prev, &next) {
            heap.push(Reverse((key(c), i, 0u32)));
        }
    }
    let mut deviation: f64 = 0.0;
    while let Some(Reverse((_, i, v))) = heap.pop() {
        if !alive[i] || version[i] != v {
            continue;
        }
        let Some(cost) = eval(i, &prev, &next) else {
            continue;
        };
        deviation = deviation.max(cost);
        let (a, b) = (prev[i], next[i]);
        alive[i] = false;
        next[a] = b;
        prev[b] = a;
        for x in [a, b] {
            if x == 0 || x == n - 1 {
                continue;
            }
            version[x] += 1;
            if let Some(c) = eval(x, &prev, &next) {
                heap.push(Reverse((key(c), x, version[x])));
            }
        }
    }
    ((0..n).filter(|&i| alive[i]).collect(), deviation)
}

/// How a chain of outline vertices is smoothed and simplified.
struct Knobs {
    smooth_limit: f64,
    passes: usize,
    chord: f64,
    loss: f64,
    outward_is_right: bool,
}

fn simplify_chain(pts: &[IntPoint], k: &Knobs, dev: &mut f64) -> Vec<IntPoint> {
    let n = pts.len();
    if n <= 2 {
        return pts.to_vec();
    }
    let orig: Vec<[f64; 2]> = pts.iter().map(|p| [p.x as f64, p.y as f64]).collect();
    let mut cur = orig.clone();
    for _ in 0..k.passes {
        let mut next = cur.clone();
        for i in 1..n - 1 {
            for d in 0..2 {
                next[i][d] = (cur[i - 1][d] + 2.0 * cur[i][d] + cur[i + 1][d]) / 4.0;
            }
        }
        cur = next;
    }
    for i in 1..n - 1 {
        let (dx, dy) = (cur[i][0] - orig[i][0], cur[i][1] - orig[i][1]);
        let l = (dx * dx + dy * dy).sqrt();
        if l > k.smooth_limit && l > 0.0 {
            cur[i][0] = orig[i][0] + dx * k.smooth_limit / l;
            cur[i][1] = orig[i][1] + dy * k.smooth_limit / l;
        }
    }
    let moved = (1..n - 1)
        .map(|i| ((cur[i][0] - orig[i][0]).powi(2) + (cur[i][1] - orig[i][1]).powi(2)).sqrt())
        .fold(0.0, f64::max);
    let (keep, cost) = simplify_open(&cur, k.chord, k.loss, k.outward_is_right);
    *dev = dev.max(moved + cost);
    keep.into_iter()
        .map(|i| {
            if i == 0 || i == n - 1 {
                pts[i]
            } else {
                IntPoint::new(cur[i][0].round() as i32, cur[i][1].round() as i32)
            }
        })
        .collect()
}

type Pt = (i32, i32);

fn pt(p: IntPoint) -> Pt {
    (p.x, p.y)
}

fn edge_key(a: IntPoint, b: IntPoint) -> (Pt, Pt) {
    if pt(a) < pt(b) {
        (pt(a), pt(b))
    } else {
        (pt(b), pt(a))
    }
}

/// Inserts into every outline edge the vertices of any layer that lie exactly on it, so layers that
/// overlap along a straight line share identical edges.
fn node_units(units: &[Region]) -> Vec<Region> {
    let all: Vec<&Region> = units.iter().collect();
    let pv = PlaneVerts::new(&all);
    units
        .iter()
        .map(|r| {
            r.iter()
                .map(|shape| {
                    shape
                        .iter()
                        .map(|c| {
                            let mut out = Vec::with_capacity(c.len());
                            for i in 0..c.len() {
                                let (p, q) = (c[i], c[(i + 1) % c.len()]);
                                out.push(p);
                                out.extend(pv.on_edge(p, q, true));
                            }
                            out
                        })
                        .collect()
                })
                .collect()
        })
        .collect()
}

/// Simplifies the outlines of all `units` (layers of identical cross-section) together. Outline
/// edges used by the same set of layers form chains between junction vertices; every chain is
/// simplified once and shared by all its layers, so neighbouring layers keep identical vertices.
/// Chains touching a layer that must stay as it is are left alone.
fn simplify_units(units: &[Region], touched: &[bool], k: &Knobs, dev: &mut f64) -> Vec<Region> {
    let rings: Vec<(usize, &Contour)> = units
        .iter()
        .enumerate()
        .flat_map(|(u, r)| r.iter().flatten().map(move |c| (u, c)))
        .collect();
    let mut users: HashMap<(Pt, Pt), Vec<(u32, i8)>> = HashMap::new();
    for &(u, c) in &rings {
        for i in 0..c.len() {
            let (a, b) = (c[i], c[(i + 1) % c.len()]);
            let dir = if pt(a) < pt(b) { 1 } else { -1 };
            users
                .entry(edge_key(a, b))
                .or_default()
                .push((u as u32, dir));
        }
    }
    let mut sig_ids: HashMap<Vec<(u32, i8)>, u32> = HashMap::new();
    let mut frozen: Vec<bool> = Vec::new();
    let mut edge_sig: HashMap<(Pt, Pt), u32> = HashMap::new();
    let mut degree: HashMap<Pt, u32> = HashMap::new();
    for (e, mut v) in users {
        v.sort();
        let mixed = v.iter().any(|x| x.1 != v[0].1);
        let v: Vec<(u32, i8)> = v.into_iter().map(|x| (x.0, mixed as i8)).collect();
        *degree.entry(e.0).or_default() += 1;
        *degree.entry(e.1).or_default() += 1;
        let id = match sig_ids.get(&v) {
            Some(&id) => id,
            None => {
                let id = frozen.len() as u32;
                let repeated = v.windows(2).any(|w| w[0].0 == w[1].0);
                frozen.push(
                    repeated
                        || v.iter().any(|x| x.1 != 0)
                        || v.iter().any(|x| !touched[x.0 as usize]),
                );
                sig_ids.insert(v, id);
                id
            }
        };
        edge_sig.insert(e, id);
    }

    let mut memo: HashMap<(Pt, Pt, Pt), Vec<IntPoint>> = HashMap::new();
    let mut rebuilt: Vec<Vec<Contour>> = vec![Vec::new(); units.len()];
    for &(u, c) in &rings {
        if !touched[u] {
            continue;
        }
        let n = c.len();
        if n < 3 {
            continue;
        }
        let sig = |i: usize| edge_sig[&edge_key(c[i], c[(i + 1) % n])];
        let mut pinned: Vec<bool> = (0..n)
            .map(|i| {
                let (e0, e1) = (sig((i + n - 1) % n), sig(i));
                e0 != e1 || frozen[e0 as usize] || frozen[e1 as usize] || degree[&pt(c[i])] != 2
            })
            .collect();
        if !pinned.iter().any(|&x| x) {
            let start = (0..n).min_by_key(|&i| pt(c[i])).unwrap();
            let d2 = |i: usize| {
                let (dx, dy) = (
                    c[i].x as i64 - c[start].x as i64,
                    c[i].y as i64 - c[start].y as i64,
                );
                dx * dx + dy * dy
            };
            let far = (0..n).max_by_key(|&i| (d2(i), Reverse(pt(c[i])))).unwrap();
            pinned[start] = true;
            pinned[far] = true;
        }
        let idx: Vec<usize> = (0..n).filter(|&i| pinned[i]).collect();
        let mut ring: Contour = Vec::new();
        for (w, &a) in idx.iter().enumerate() {
            let b = idx[(w + 1) % idx.len()];
            let len = (b + n - a) % n;
            let len = if len == 0 { n } else { len };
            let pts: Vec<IntPoint> = (0..=len).map(|d| c[(a + d) % n]).collect();
            let key = (pt(pts[0]), pt(pts[1]), pt(pts[len]));
            let simple = memo
                .entry(key)
                .or_insert_with(|| simplify_chain(&pts, k, dev));
            ring.extend(&simple[..simple.len() - 1]);
        }
        if ring.len() >= 3 && area2(&ring) != 0.0 {
            rebuilt[u].push(ring);
        }
    }
    units
        .iter()
        .enumerate()
        .map(|(u, r)| {
            if touched[u] {
                normalize(std::mem::take(&mut rebuilt[u]))
            } else {
                r.clone()
            }
        })
        .collect()
}

fn chain(segs: &[(IntPoint, IntPoint)]) -> Result<Vec<Contour>, String> {
    let mut from: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        from.entry((s.0.x, s.0.y)).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut loops = Vec::new();
    for start in 0..segs.len() {
        if used[start] {
            continue;
        }
        let mut ring = vec![segs[start].0];
        let mut cur = start;
        loop {
            used[cur] = true;
            let end = segs[cur].1;
            if end == segs[start].0 {
                break;
            }
            ring.push(end);
            let Some(nx) = from
                .get(&(end.x, end.y))
                .and_then(|l| l.iter().copied().find(|&j| !used[j]))
            else {
                return Err(
                    "the model has an open cross-section; it is not a closed stencil plate".into(),
                );
            };
            cur = nx;
        }
        loops.push(ring);
    }
    Ok(loops)
}

/// Distinct horizontal planes of the mesh and the cross-section of every slab between them.
fn slice(mesh: &Mesh) -> Result<(Vec<f64>, Vec<Region>), String> {
    let mut zs: Vec<f64> = mesh.v.iter().map(|v| v[2]).collect();
    zs.sort_by(f64::total_cmp);
    let mut planes: Vec<f64> = Vec::new();
    for z in zs {
        if planes.last().is_none_or(|&l| z - l > Z_TOL) {
            planes.push(z);
        }
    }
    if planes.len() < 2 {
        return Err("the solid is flat".into());
    }
    let plane_of = |z: f64| planes.partition_point(|&p| p < z - Z_TOL);
    let mut segs: Vec<Vec<(IntPoint, IntPoint)>> = vec![Vec::new(); planes.len() - 1];
    for t in &mesh.t {
        let v = [mesh.v[t[0]], mesh.v[t[1]], mesh.v[t[2]]];
        let (e1, e2) = (
            [v[1][0] - v[0][0], v[1][1] - v[0][1], v[1][2] - v[0][2]],
            [v[2][0] - v[0][0], v[2][1] - v[0][1], v[2][2] - v[0][2]],
        );
        let n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len < 1e-18 {
            continue;
        }
        let r = n[2].abs() / len;
        if r > 0.999 {
            continue;
        }
        if r > 1e-3 {
            return Err("the solid has sloped faces; the complexity reducer works on stencil plates with vertical walls".into());
        }
        let zlo = v.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
        let zhi = v.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max);
        let (s0, s1) = (plane_of(zlo), plane_of(zhi));
        // Inward direction is the left of the cut, which is z-axis x normal.
        let dir = [-n[1], n[0]];
        for s in s0..s1.min(planes.len() - 1) {
            let zm = (planes[s] + planes[s + 1]) / 2.0;
            let mut hits: Vec<[f64; 2]> = Vec::new();
            for (a, b) in [(0usize, 1usize), (1, 2), (2, 0)] {
                let (ia, ib) = if t[a] < t[b] { (a, b) } else { (b, a) };
                let (pa, pb) = (v[ia], v[ib]);
                if (pa[2] - zm) * (pb[2] - zm) < 0.0 {
                    let f = (zm - pa[2]) / (pb[2] - pa[2]);
                    hits.push([pa[0] + f * (pb[0] - pa[0]), pa[1] + f * (pb[1] - pa[1])]);
                }
            }
            if hits.len() != 2 {
                continue;
            }
            let (mut p, mut q) = (hits[0], hits[1]);
            if (q[0] - p[0]) * dir[0] + (q[1] - p[1]) * dir[1] < 0.0 {
                std::mem::swap(&mut p, &mut q);
            }
            let ip = IntPoint::new((p[0] * U).round() as i32, (p[1] * U).round() as i32);
            let iq = IntPoint::new((q[0] * U).round() as i32, (q[1] * U).round() as i32);
            if ip != iq {
                segs[s].push((ip, iq));
            }
        }
    }
    let regions = segs
        .iter()
        .map(|s| chain(s).map(normalize))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((planes, regions))
}

struct Segment {
    z0: f64,
    z1: f64,
    original: Region,
    region: Region,
    touched: bool,
}

/// The integer vertices on one horizontal plane, to split any edge that passes through them.
struct PlaneVerts {
    pts: Vec<IntPoint>,
    cells: HashMap<(i32, i32), Vec<usize>>,
}

const CELL: i32 = 10_000;

impl PlaneVerts {
    fn new(sources: &[&Region]) -> Self {
        let mut seen = HashSet::new();
        let mut pts = Vec::new();
        for r in sources {
            for p in r.iter().flatten().flatten() {
                if seen.insert((p.x, p.y)) {
                    pts.push(*p);
                }
            }
        }
        let mut cells: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, p) in pts.iter().enumerate() {
            cells
                .entry((p.x.div_euclid(CELL), p.y.div_euclid(CELL)))
                .or_default()
                .push(i);
        }
        PlaneVerts { pts, cells }
    }

    /// Vertices lying on the open segment p-q, ordered from p to q.
    fn on_edge(&self, p: IntPoint, q: IntPoint, exact: bool) -> Vec<IntPoint> {
        let (dx, dy) = ((q.x - p.x) as f64, (q.y - p.y) as f64);
        let l2 = dx * dx + dy * dy;
        if l2 == 0.0 {
            return vec![];
        }
        let (x0, x1) = (p.x.min(q.x) - 2, p.x.max(q.x) + 2);
        let (y0, y1) = (p.y.min(q.y) - 2, p.y.max(q.y) + 2);
        let (cx0, cx1) = (x0.div_euclid(CELL), x1.div_euclid(CELL));
        let (cy0, cy1) = (y0.div_euclid(CELL), y1.div_euclid(CELL));
        let mut found: Vec<(f64, IntPoint)> = Vec::new();
        let mut test = |r: IntPoint| {
            if r == p || r == q {
                return;
            }
            let t = ((r.x - p.x) as f64 * dx + (r.y - p.y) as f64 * dy) / l2;
            if t <= 0.0 || t >= 1.0 {
                return;
            }
            let cross = (r.x as i64 - p.x as i64) * (q.y as i64 - p.y as i64)
                - (r.y as i64 - p.y as i64) * (q.x as i64 - p.x as i64);
            let off = cross.abs() as f64 / l2.sqrt();
            if (exact && cross == 0) || (!exact && off <= 2.0) {
                found.push((t, r));
            }
        };
        let cells = (cx1 - cx0 + 1) as i64 * (cy1 - cy0 + 1) as i64;
        if cells > 4096 {
            for r in &self.pts {
                test(*r);
            }
        } else {
            for cx in cx0..=cx1 {
                for cy in cy0..=cy1 {
                    for &i in self.cells.get(&(cx, cy)).into_iter().flatten() {
                        test(self.pts[i]);
                    }
                }
            }
        }
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        found.into_iter().map(|f| f.1).collect()
    }

    /// A wall edge also passes through the crossing points that rounding left a unit or two off its line.
    fn polyline(&self, p: IntPoint, q: IntPoint) -> Vec<IntPoint> {
        let mut v = vec![p];
        v.extend(self.on_edge(p, q, false));
        v.push(q);
        v
    }
}

fn param(p: IntPoint, q: IntPoint, r: IntPoint) -> f64 {
    let (dx, dy) = ((q.x - p.x) as f64, (q.y - p.y) as f64);
    ((r.x - p.x) as f64 * dx + (r.y - p.y) as f64 * dy) / (dx * dx + dy * dy)
}

/// Ear clipping of a triangle whose sides carry extra points (exactly or nearly collinear): no ear may
/// contain another vertex, so every point stays a vertex of the result and no triangle overlaps another.
fn clip_ears(mut poly: Vec<IntPoint>) -> Vec<[IntPoint; 3]> {
    let cross = |a: IntPoint, b: IntPoint, c: IntPoint| {
        (b.x as i64 - a.x as i64) * (c.y as i64 - a.y as i64)
            - (b.y as i64 - a.y as i64) * (c.x as i64 - a.x as i64)
    };
    let inside = |a: IntPoint, b: IntPoint, c: IntPoint, p: IntPoint, sign: i64| {
        cross(a, b, p) * sign >= 0 && cross(b, c, p) * sign >= 0 && cross(c, a, p) * sign >= 0
    };
    let sign = {
        let n = poly.len();
        let twice: i128 = (0..n)
            .map(|i| {
                let (a, b) = (poly[i], poly[(i + 1) % n]);
                a.x as i128 * b.y as i128 - b.x as i128 * a.y as i128
            })
            .sum();
        if twice < 0 {
            -1
        } else {
            1
        }
    };
    let mut out = Vec::new();
    while poly.len() > 3 {
        let n = poly.len();
        let convex = |i: usize| cross(poly[(i + n - 1) % n], poly[i], poly[(i + 1) % n]) * sign > 0;
        let clean = |i: usize| {
            let (a, b, c) = (poly[(i + n - 1) % n], poly[i], poly[(i + 1) % n]);
            (0..n)
                .filter(|&k| k != i && k != (i + n - 1) % n && k != (i + 1) % n)
                .all(|k| {
                    !inside(a, b, c, poly[k], sign) || poly[k] == a || poly[k] == b || poly[k] == c
                })
        };
        let Some(i) = (0..n)
            .find(|&i| convex(i) && clean(i))
            .or_else(|| (0..n).find(|&i| convex(i)))
        else {
            break;
        };
        out.push([poly[(i + n - 1) % n], poly[i], poly[(i + 1) % n]]);
        poly.remove(i);
    }
    if poly.len() == 3 {
        out.push([poly[0], poly[1], poly[2]]);
    }
    out
}

struct Builder {
    mesh: Mesh,
    ids: crate::fasthash::FastMap<(i32, i32, usize), usize>,
}

impl Builder {
    fn vid(&mut self, p: IntPoint, plane: usize, z: f64) -> usize {
        if let Some(&i) = self.ids.get(&(p.x, p.y, plane)) {
            return i;
        }
        self.mesh.v.push([p.x as f64 / U, p.y as f64 / U, z]);
        let i = self.mesh.v.len() - 1;
        self.ids.insert((p.x, p.y, plane), i);
        i
    }
}

/// Stacks the runs into a watertight solid. Between two runs the horizontal faces are exactly the part
/// of one run that the other does not cover; every vertex of either run splits the edges it lies on.
pub(crate) fn build(planes: &[f64], runs: &[Region]) -> Mesh {
    let none: Region = vec![];
    let mut b = Builder {
        mesh: Mesh::default(),
        ids: Default::default(),
    };
    // Each plane is triangulated on its own (in parallel where threads exist); ids are handed out in plane order.
    let indices: Vec<usize> = (0..=runs.len()).collect();
    let per_plane = crate::par::map(&indices, |&j| {
        let a = if j == 0 { &none } else { &runs[j - 1] };
        let c = if j == runs.len() { &none } else { &runs[j] };
        let up = boolean(a, c, OverlayRule::Difference);
        let down = boolean(c, a, OverlayRule::Difference);
        let pv = PlaneVerts::new(&[a, c, &up, &down]);
        let mut tris: Vec<[IntPoint; 3]> = Vec::new();
        for (region, facing_up) in [(&up, true), (&down, false)] {
            for shape in region {
                let tri = shape.triangulate().into_triangulation::<u32>();
                let pt = |k: u32| tri.points[k as usize];
                let cross = |t: &[u32]| {
                    let (p, q, r) = (pt(t[0]), pt(t[1]), pt(t[2]));
                    (q.x as i64 - p.x as i64) * (r.y as i64 - p.y as i64)
                        - (q.y as i64 - p.y as i64) * (r.x as i64 - p.x as i64)
                };
                let flip = tri
                    .indices
                    .chunks(3)
                    .map(&cross)
                    .find(|&c| c != 0)
                    .is_some_and(|c| (c > 0) != facing_up);
                let mut uses: HashMap<(u32, u32), u32> = HashMap::new();
                for t in tri.indices.chunks(3) {
                    for k in 0..3 {
                        let (u, v) = (t[k], t[(k + 1) % 3]);
                        *uses.entry((u.min(v), u.max(v))).or_default() += 1;
                    }
                }
                for t in tri.indices.chunks(3) {
                    let mut perimeter: Vec<IntPoint> = Vec::new();
                    for k in 0..3 {
                        let (u, v) = (t[k], t[(k + 1) % 3]);
                        perimeter.push(pt(u));
                        if uses[&(u.min(v), u.max(v))] == 1 {
                            perimeter.extend(pv.on_edge(pt(u), pt(v), false));
                        }
                    }
                    for mut tr in clip_ears(perimeter) {
                        if flip {
                            tr.swap(1, 2);
                        }
                        tris.push(tr);
                    }
                }
            }
        }
        (pv, tris)
    });
    let mut verts: Vec<PlaneVerts> = Vec::new();
    for (j, (pv, tris)) in per_plane.into_iter().enumerate() {
        for tr in tris {
            let ids = tr.map(|q| b.vid(q, j, planes[j]));
            b.mesh.t.push(ids);
        }
        verts.push(pv);
    }
    // An edge that several consecutive runs share is one wall from where it starts to where it ends.
    let edges: Vec<HashSet<(Pt, Pt)>> = runs
        .iter()
        .map(|r| {
            r.iter()
                .flatten()
                .flat_map(|c| (0..c.len()).map(move |i| (pt(c[i]), pt(c[(i + 1) % c.len()]))))
                .collect()
        })
        .collect();
    let mut walls: Vec<(usize, usize, IntPoint, IntPoint)> = Vec::new();
    for (j, region) in runs.iter().enumerate() {
        for ring in region.iter().flatten() {
            let n = ring.len();
            for i in 0..n {
                let (p, q) = (ring[i], ring[(i + 1) % n]);
                if j > 0 && edges[j - 1].contains(&(pt(p), pt(q))) {
                    continue;
                }
                let mut end = j;
                while end + 1 < runs.len() && edges[end + 1].contains(&(pt(p), pt(q))) {
                    end += 1;
                }
                walls.push((j, end + 1, p, q));
            }
        }
    }
    // Neighbouring walls of a vertex may start or end at other planes: the vertical seam is split there.
    let mut at: HashMap<Pt, BTreeSet<usize>> = HashMap::new();
    for &(lo, hi, p, q) in &walls {
        for v in [p, q] {
            at.entry(pt(v)).or_default().extend([lo, hi]);
        }
    }
    for &(lo, hi, p, q) in &walls {
        let bot = verts[lo].polyline(p, q);
        let top = verts[hi].polyline(p, q);
        let tb: Vec<f64> = bot.iter().map(|&r| param(p, q, r)).collect();
        let tt: Vec<f64> = top.iter().map(|&r| param(p, q, r)).collect();
        let mut tris: Vec<[usize; 3]> = Vec::new();
        let (mut i0, mut j0) = (0, 0);
        while i0 + 1 < bot.len() || j0 + 1 < top.len() {
            let advance_bottom =
                j0 + 1 == top.len() || (i0 + 1 < bot.len() && tb[i0 + 1] <= tt[j0 + 1]);
            if advance_bottom {
                tris.push([
                    b.vid(bot[i0], lo, planes[lo]),
                    b.vid(bot[i0 + 1], lo, planes[lo]),
                    b.vid(top[j0], hi, planes[hi]),
                ]);
                i0 += 1;
            } else {
                tris.push([
                    b.vid(top[j0], hi, planes[hi]),
                    b.vid(bot[i0], lo, planes[lo]),
                    b.vid(top[j0 + 1], hi, planes[hi]),
                ]);
                j0 += 1;
            }
        }
        // Split the first side (top -> bottom at p) and the last side (bottom -> top at q).
        let between =
            |v: IntPoint| -> Vec<usize> { at[&pt(v)].range(lo + 1..hi).copied().collect() };
        let down: Vec<usize> = between(p)
            .into_iter()
            .rev()
            .map(|k| b.vid(p, k, planes[k]))
            .collect();
        let up: Vec<usize> = between(q)
            .into_iter()
            .map(|k| b.vid(q, k, planes[k]))
            .collect();
        let (first, last) = (0, tris.len() - 1);
        for (ti, from, to, mid) in [
            (
                first,
                b.vid(top[0], hi, planes[hi]),
                b.vid(bot[0], lo, planes[lo]),
                down,
            ),
            (
                last,
                b.vid(bot[bot.len() - 1], lo, planes[lo]),
                b.vid(top[top.len() - 1], hi, planes[hi]),
                up,
            ),
        ] {
            if mid.is_empty() {
                continue;
            }
            let t = tris[ti];
            let k = (0..3)
                .find(|&k| t[k] == from && t[(k + 1) % 3] == to)
                .expect("side edge");
            let w = t[(k + 2) % 3];
            let mut chain = vec![from];
            chain.extend(mid);
            chain.push(to);
            tris[ti] = [chain[0], chain[1], w];
            for pair in chain[1..].windows(2) {
                tris.push([pair[0], pair[1], w]);
            }
        }
        b.mesh.t.extend(tris);
    }
    cancel_folds(&mut b.mesh);
    b.mesh
}

/// Zero-thickness slivers that a floor and a ceiling both produced come as a pair of identical triangles facing
/// opposite ways; together they enclose nothing, so both go and the walls beside them meet directly.
fn cancel_folds(mesh: &mut Mesh) {
    let key = |t: &[usize; 3]| {
        let mut k = *t;
        k.sort_unstable();
        k
    };
    let mut seen: HashMap<[usize; 3], Vec<usize>> = HashMap::new();
    for (i, t) in mesh.t.iter().enumerate() {
        seen.entry(key(t)).or_default().push(i);
    }
    let mut drop = vec![false; mesh.t.len()];
    for ids in seen.values().filter(|v| v.len() >= 2) {
        let flip = |t: &[usize; 3]| {
            let m = t
                .iter()
                .position(|&x| x == t.iter().copied().min().unwrap())
                .unwrap();
            [t[m], t[(m + 1) % 3], t[(m + 2) % 3]]
        };
        let (mut fwd, mut back): (Vec<usize>, Vec<usize>) = (vec![], vec![]);
        let reference = flip(&mesh.t[ids[0]]);
        for &i in ids {
            if flip(&mesh.t[i]) == reference {
                fwd.push(i);
            } else {
                back.push(i);
            }
        }
        for (&a, &b) in fwd.iter().zip(back.iter()) {
            drop[a] = true;
            drop[b] = true;
        }
    }
    let mut i = 0;
    mesh.t.retain(|_| {
        i += 1;
        !drop[i - 1]
    });
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
