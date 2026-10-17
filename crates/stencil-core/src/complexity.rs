//! Complexity reducer: slices a stencil solid into cross-sections, simplifies the chosen layers
//! (fewer vertices, rounding towards material) without letting the layer above overhang more than
//! the allowed angle, and rebuilds a watertight mesh.

use crate::mesh::Mesh;
use i_triangle::i_overlay::core::fill_rule::FillRule;
use i_triangle::i_overlay::core::overlay_rule::OverlayRule;
use i_triangle::i_overlay::core::single::SingleIntOverlay;
use i_triangle::i_overlay::i_float::int::point::IntPoint;
use i_triangle::i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_triangle::i_overlay::mesh::float::style::OutlineStyle;
use i_triangle::int::triangulatable::IntTriangulatable;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

const U: f64 = 10_000.0; // integer units per mm
const Z_TOL: f64 = 2e-4; // mm: heights closer than this are the same plane
const NOISE_UNITS2: f64 = 1e6; // 0.01 mm²: overlap slivers below this are rounding noise
/// Rounding towards material never removes a vertex whose removal cuts into the material.
const LOSS_FRACTION: f64 = 0.0;

type Contour = Vec<IntPoint>;
type Shape = Vec<Contour>;
type Region = Vec<Shape>;

pub struct Params {
    pub layer_height: f64,
    pub from: u32,
    /// 0 = up to the top layer.
    pub to: u32,
    pub tolerance_mm: f64,
    /// 90 or more switches the overhang rule off.
    pub max_overhang_deg: f64,
    pub keep_material: bool,
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
    /// Set when the input was returned untouched, with the reason.
    pub skipped: Option<String>,
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

fn grow(r: &Region, d: f64) -> Region {
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

fn dist_to_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 {
        0.0
    } else {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
    };
    ((p[0] - a[0] - t * dx).powi(2) + (p[1] - a[1] - t * dy).powi(2)).sqrt()
}

/// Cost of dropping vertex `i`: the largest distance of the original vertices it would hide to the new
/// chord, and whether the chord loses material (the removed run turns counter-clockwise).
fn removal(pts: &[[f64; 2]], a: usize, b: usize) -> (f64, bool) {
    let n = pts.len();
    let mut cost: f64 = 0.0;
    let mut twice_area = 0.0;
    let mut prev = a;
    let mut k = (a + 1) % n;
    loop {
        cost = cost.max(dist_to_segment(pts[k], pts[a], pts[b]));
        twice_area += pts[prev][0] * pts[k][1] - pts[k][0] * pts[prev][1];
        prev = k;
        if k == b {
            break;
        }
        k = (k + 1) % n;
    }
    twice_area += pts[b][0] * pts[a][1] - pts[a][0] * pts[b][1];
    (cost, twice_area > 2.0)
}

/// Greedy vertex removal on a closed ring whose material lies on the left. Returns the ring and the
/// largest distance any removed vertex ended up from the new outline (in units).
fn simplify_ring(ring: &[IntPoint], tol: f64, loss: f64) -> (Contour, f64) {
    let n = ring.len();
    if n <= 3 || tol <= 0.0 {
        return (ring.to_vec(), 0.0);
    }
    let pts: Vec<[f64; 2]> = ring.iter().map(|p| [p.x as f64, p.y as f64]).collect();
    let mut prev: Vec<usize> = (0..n).map(|i| (i + n - 1) % n).collect();
    let mut next: Vec<usize> = (0..n).map(|i| (i + 1) % n).collect();
    let mut alive = vec![true; n];
    let mut version = vec![0u32; n];
    let mut count = n;
    let eval = |i: usize, prev: &[usize], next: &[usize]| -> Option<f64> {
        let (cost, loses) = removal(&pts, prev[i], next[i]);
        let limit = if loses { tol * loss } else { tol };
        (cost <= limit).then_some(cost)
    };
    let key = |c: f64| (c * 16.0) as u64;
    let mut heap = BinaryHeap::new();
    for i in 0..n {
        if let Some(c) = eval(i, &prev, &next) {
            heap.push(Reverse((key(c), ring[i].x, ring[i].y, i, 0u32)));
        }
    }
    let mut deviation: f64 = 0.0;
    while let Some(Reverse((_, _, _, i, v))) = heap.pop() {
        if !alive[i] || version[i] != v {
            continue;
        }
        if count <= 3 {
            break;
        }
        let Some(cost) = eval(i, &prev, &next) else {
            continue;
        };
        deviation = deviation.max(cost);
        let (a, b) = (prev[i], next[i]);
        alive[i] = false;
        next[a] = b;
        prev[b] = a;
        count -= 1;
        for x in [a, b] {
            version[x] += 1;
            if let Some(c) = eval(x, &prev, &next) {
                heap.push(Reverse((key(c), ring[x].x, ring[x].y, x, version[x])));
            }
        }
    }
    let out = (0..n).filter(|&i| alive[i]).map(|i| ring[i]).collect();
    (out, deviation)
}

fn simplify_region(r: &Region, tol: f64, loss: f64, dev: &mut f64) -> Region {
    let mut out = Vec::new();
    for c in r.iter().flatten() {
        let (s, d) = simplify_ring(c, tol, loss);
        *dev = dev.max(d);
        if s.len() >= 3 && area2(&s).abs() > 0.0 {
            out.push(s);
        }
    }
    normalize(out)
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
    ids: HashMap<(i32, i32, usize), usize>,
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
fn build(planes: &[f64], runs: &[Region]) -> Mesh {
    let none: Region = vec![];
    let mut b = Builder {
        mesh: Mesh::default(),
        ids: HashMap::new(),
    };
    let mut verts: Vec<PlaneVerts> = Vec::new();
    for j in 0..=runs.len() {
        let a = if j == 0 { &none } else { &runs[j - 1] };
        let c = if j == runs.len() { &none } else { &runs[j] };
        let up = boolean(a, c, OverlayRule::Difference);
        let down = boolean(c, a, OverlayRule::Difference);
        let pv = PlaneVerts::new(&[a, c, &up, &down]);
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
                            perimeter.extend(pv.on_edge(pt(u), pt(v), true));
                        }
                    }
                    for mut tr in clip_ears(perimeter) {
                        if flip {
                            tr.swap(1, 2);
                        }
                        let ids = tr.map(|q| b.vid(q, j, planes[j]));
                        b.mesh.t.push(ids);
                    }
                }
            }
        }
        verts.push(pv);
    }
    for (j, region) in runs.iter().enumerate() {
        let (zb, zt) = (planes[j], planes[j + 1]);
        for ring in region.iter().flatten() {
            let n = ring.len();
            for i in 0..n {
                let (p, q) = (ring[i], ring[(i + 1) % n]);
                let bot = verts[j].polyline(p, q);
                let top = verts[j + 1].polyline(p, q);
                let tb: Vec<f64> = bot.iter().map(|&r| param(p, q, r)).collect();
                let tt: Vec<f64> = top.iter().map(|&r| param(p, q, r)).collect();
                let (mut i0, mut j0) = (0, 0);
                while i0 + 1 < bot.len() || j0 + 1 < top.len() {
                    let advance_bottom =
                        j0 + 1 == top.len() || (i0 + 1 < bot.len() && tb[i0 + 1] <= tt[j0 + 1]);
                    if advance_bottom {
                        let (a, c, d) = (
                            b.vid(bot[i0], j, zb),
                            b.vid(bot[i0 + 1], j, zb),
                            b.vid(top[j0], j + 1, zt),
                        );
                        b.mesh.t.push([a, c, d]);
                        i0 += 1;
                    } else {
                        let (a, c, d) = (
                            b.vid(top[j0], j + 1, zt),
                            b.vid(bot[i0], j, zb),
                            b.vid(top[j0 + 1], j + 1, zt),
                        );
                        b.mesh.t.push([a, c, d]);
                        j0 += 1;
                    }
                }
            }
        }
    }
    b.mesh
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
    let tol = p.tolerance_mm * U;
    let loss = if p.keep_material { LOSS_FRACTION } else { 1.0 };
    let mut deviation: f64 = 0.0;
    let mut simplified: HashMap<usize, Region> = HashMap::new();
    let mut segs: Vec<Segment> = Vec::new();
    crate::progress::step("simplify layers", 0.3);
    for w in cuts.windows(2) {
        let mid = (w[0] + w[1]) / 2.0;
        let slab = planes.partition_point(|&z| z < mid) - 1;
        let touched = mid > zlo && mid < zhi;
        let original = slabs[slab].clone();
        let region = if touched {
            simplified
                .entry(slab)
                .or_insert_with(|| simplify_region(&original, tol, loss, &mut deviation))
                .clone()
        } else {
            original.clone()
        };
        segs.push(Segment {
            z0: w[0],
            z1: w[1],
            original,
            region,
            touched,
        });
    }

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
        for i in (0..segs.len().saturating_sub(1)).rev() {
            if !segs[i].touched && !segs[i + 1].touched {
                continue;
            }
            let allowed = allowed_above(&segs, i);
            let bad = drop_noise(boolean(
                &segs[i + 1].region,
                &allowed,
                OverlayRule::Difference,
            ));
            if bad.is_empty() {
                continue;
            }
            if segs[i].touched {
                segs[i].region = boolean(&segs[i].region, &bad, OverlayRule::Union);
            } else {
                segs[i + 1].region = boolean(&segs[i + 1].region, &allowed, OverlayRule::Intersect);
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
    if skip.is_some() {
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
                skipped: skip,
            },
        ));
    }
    let bytes = out.to_stl();
    crate::progress::step("write STL", 1.0);
    Ok((
        bytes,
        Report {
            layers,
            from,
            to,
            vertices_before: input.v.len(),
            vertices_after: out.v.len(),
            triangles_before: input.t.len(),
            triangles_after: out.t.len(),
            max_deviation_mm: deviation / U,
            overhang_violation_mm2: violation,
            skipped: None,
        },
    ))
}
