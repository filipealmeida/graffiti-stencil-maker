//! Slicing a mesh into planes and stitching the simplified planes back into a watertight mesh.

use super::*;

pub(super) fn chain(segs: &[(IntPoint, IntPoint)]) -> Result<Vec<Contour>, String> {
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
pub(super) fn slice(mesh: &Mesh) -> Result<(Vec<f64>, Vec<Region>), String> {
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
            return Err("the solid has sloped faces; the remesh works on stencil plates with vertical walls".into());
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

pub(super) struct Segment {
    pub(super) z0: f64,
    pub(super) z1: f64,
    pub(super) original: Region,
    pub(super) region: Region,
    pub(super) touched: bool,
}

/// The integer vertices on one horizontal plane, to split any edge that passes through them.
pub(super) struct PlaneVerts {
    pub(super) pts: Vec<IntPoint>,
    pub(super) cells: HashMap<(i32, i32), Vec<usize>>,
}

pub(super) const CELL: i32 = 10_000;

impl PlaneVerts {
    pub(super) fn new(sources: &[&Region]) -> Self {
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
    pub(super) fn on_edge(&self, p: IntPoint, q: IntPoint, exact: bool) -> Vec<IntPoint> {
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
    pub(super) fn polyline(&self, p: IntPoint, q: IntPoint) -> Vec<IntPoint> {
        let mut v = vec![p];
        v.extend(self.on_edge(p, q, false));
        v.push(q);
        v
    }
}

pub(super) fn param(p: IntPoint, q: IntPoint, r: IntPoint) -> f64 {
    let (dx, dy) = ((q.x - p.x) as f64, (q.y - p.y) as f64);
    ((r.x - p.x) as f64 * dx + (r.y - p.y) as f64 * dy) / (dx * dx + dy * dy)
}

/// Ear clipping of a triangle whose sides carry extra points (exactly or nearly collinear): no ear may
/// contain another vertex, so every point stays a vertex of the result and no triangle overlaps another.
pub(super) fn clip_ears(mut poly: Vec<IntPoint>) -> Vec<[IntPoint; 3]> {
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

pub(super) struct Builder {
    pub(super) mesh: Mesh,
    pub(super) ids: crate::fasthash::FastMap<(i32, i32, usize), usize>,
}

impl Builder {
    pub(super) fn vid(&mut self, p: IntPoint, plane: usize, z: f64) -> usize {
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
pub(super) fn cancel_folds(mesh: &mut Mesh) {
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
