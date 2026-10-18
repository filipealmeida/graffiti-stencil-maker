//! Layered solid to watertight triangle mesh: boundary tracing, simplification and extrusion.

use i_triangle::i_overlay::i_float::int::point::IntPoint;
use i_triangle::int::unchecked::IntUncheckedTriangulatable;
use std::collections::HashMap;

pub(crate) type Vid = u32;

pub(crate) struct Edge {
    from: Vid,
    to: Vid,
    tag: u32,
}

pub(crate) struct Loop {
    v: Vec<Vid>,
    tag: Vec<u32>, // tag of the edge leaving vertex i
}

pub(crate) fn trace(edges: &[Edge], vw: usize) -> Vec<Loop> {
    let xy = |v: Vid| ((v as usize % vw) as i64, (v as usize / vw) as i64);
    let mut by_from: HashMap<Vid, Vec<usize>> = HashMap::new();
    for (i, e) in edges.iter().enumerate() {
        by_from.entry(e.from).or_default().push(i);
    }
    let mut used = vec![false; edges.len()];
    let mut loops = Vec::new();
    for s in 0..edges.len() {
        if used[s] {
            continue;
        }
        let (mut v, mut tag) = (Vec::new(), Vec::new());
        let mut cur = s;
        loop {
            used[cur] = true;
            v.push(edges[cur].from);
            tag.push(edges[cur].tag);
            let (a, b) = (xy(edges[cur].from), xy(edges[cur].to));
            let u = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
            // Leftmost turn keeps pinched corners as separate loops.
            let next = by_from[&edges[cur].to]
                .iter()
                .copied()
                .filter(|&j| !used[j] || j == s)
                .max_by(|&p, &q| {
                    let ang = |j: usize| {
                        let (c, d) = (xy(edges[j].from), xy(edges[j].to));
                        let wv = ((d.0 - c.0) as f64, (d.1 - c.1) as f64);
                        (u.0 * wv.1 - u.1 * wv.0).atan2(u.0 * wv.0 + u.1 * wv.1)
                    };
                    ang(p).partial_cmp(&ang(q)).unwrap()
                });
            match next {
                Some(j) if j != s => cur = j,
                _ => break,
            }
        }
        loops.push(Loop { v, tag });
    }
    loops
}

/// Drop vertices on straight runs, but only where the wall height on both sides of the vertex is the same.
pub(crate) fn simplify(lp: Loop, vw: usize) -> Loop {
    let xy = |v: Vid| ((v as usize % vw) as i64, (v as usize / vw) as i64);
    let n = lp.v.len();
    let (mut v, mut tag) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for i in 0..n {
        let (a, b, c) = (
            xy(lp.v[(i + n - 1) % n]),
            xy(lp.v[i]),
            xy(lp.v[(i + 1) % n]),
        );
        let straight = (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0) == 0
            && (b.0 - a.0) * (c.0 - b.0) + (b.1 - a.1) * (c.1 - b.1) > 0;
        if !(straight && lp.tag[(i + n - 1) % n] == lp.tag[i]) {
            v.push(lp.v[i]);
            tag.push(lp.tag[i]);
        }
    }
    Loop { v, tag }
}

pub(crate) struct Mesh {
    pub(crate) verts: Vec<[f32; 3]>,
    pub(crate) tris: Vec<[u32; 3]>,
}

impl Mesh {
    pub(crate) fn is_watertight(&self) -> bool {
        let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
        for t in &self.tris {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                if a == b {
                    return false;
                }
                *edges.entry((a, b)).or_insert(0) += 1;
            }
        }
        let bad = edges
            .iter()
            .filter(|(&(a, b), &n)| !(n == 1 && edges.get(&(b, a)) == Some(&1)))
            .count();
        if std::env::var("DBG").is_ok() {
            eprintln!("bad edges {bad} of {}", edges.len());
            for (&(a, b), &n) in edges
                .iter()
                .filter(|(&(a, b), &n)| !(n == 1 && edges.get(&(b, a)) == Some(&1)))
                .take(40)
            {
                eprintln!(
                    "{:?}->{:?} n={} rev={:?}",
                    self.verts[a as usize],
                    self.verts[b as usize],
                    n,
                    edges.get(&(b, a))
                );
            }
        }
        bad == 0
    }

    pub(crate) fn to_stl(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(84 + self.tris.len() * 50);
        out.extend_from_slice(&[0u8; 80]);
        out.extend_from_slice(&(self.tris.len() as u32).to_le_bytes());
        for t in &self.tris {
            let (a, b, c) = (
                self.verts[t[0] as usize],
                self.verts[t[1] as usize],
                self.verts[t[2] as usize],
            );
            let (u, v) = (
                [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
            );
            let mut n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len > 0.0 {
                n = [n[0] / len, n[1] / len, n[2] / len];
            }
            for f in n.iter().chain(a.iter()).chain(b.iter()).chain(c.iter()) {
                out.extend_from_slice(&f.to_le_bytes());
            }
            out.extend_from_slice(&[0, 0]);
        }
        out
    }
}

pub(crate) fn signed_area(lp: &[Vid], vw: usize) -> f64 {
    let xy = |v: Vid| ((v as usize % vw) as f64, (v as usize / vw) as f64);
    let mut a = 0.0;
    for i in 0..lp.len() {
        let (p, q) = (xy(lp[i]), xy(lp[(i + 1) % lp.len()]));
        a += p.0 * q.1 - q.0 * p.1;
    }
    a / 2.0
}

pub(crate) fn point_in(lp: &[Vid], vw: usize, pt: (f64, f64)) -> bool {
    let xy = |v: Vid| ((v as usize % vw) as f64, (v as usize / vw) as f64);
    let mut inside = false;
    let mut j = lp.len() - 1;
    for i in 0..lp.len() {
        let (a, b) = (xy(lp[i]), xy(lp[j]));
        if (a.1 > pt.1) != (b.1 > pt.1) && pt.0 < (b.0 - a.0) * (pt.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Boundary edges (solid on the left) of the pixel set `sel`; `tag(inside, outside)` labels each edge.
pub(crate) fn boundary_edges(
    pixels: &[usize],
    inside: impl Fn(usize) -> bool,
    tag: impl Fn(usize, i64, i64) -> u32,
    w: usize,
    h: usize,
) -> Vec<Edge> {
    let vw = w + 1;
    let vid = |x: usize, y: usize| (y * vw + x) as Vid;
    let mut edges = Vec::new();
    for &i in pixels {
        let (x, y) = (i % w, i / w);
        let side = |dx: i64, dy: i64| -> bool {
            let (nx, ny) = (x as i64 + dx, y as i64 + dy);
            nx >= 0
                && ny >= 0
                && (nx as usize) < w
                && (ny as usize) < h
                && inside(ny as usize * w + nx as usize)
        };
        if !side(0, -1) {
            edges.push(Edge {
                from: vid(x, y),
                to: vid(x + 1, y),
                tag: tag(i, 0, -1),
            });
        }
        if !side(1, 0) {
            edges.push(Edge {
                from: vid(x + 1, y),
                to: vid(x + 1, y + 1),
                tag: tag(i, 1, 0),
            });
        }
        if !side(0, 1) {
            edges.push(Edge {
                from: vid(x + 1, y + 1),
                to: vid(x, y + 1),
                tag: tag(i, 0, 1),
            });
        }
        if !side(-1, 0) {
            edges.push(Edge {
                from: vid(x, y + 1),
                to: vid(x, y),
                tag: tag(i, -1, 0),
            });
        }
    }
    edges
}

pub(crate) fn layered_mesh(
    kmin: &[u32],
    w: usize,
    h: usize,
    l: u32,
    s: f64,
    lh: f64,
) -> Result<Mesh, String> {
    let vw = w + 1;
    let xy = |v: Vid| ((v as usize % vw) as i64, (v as usize / vw) as i64);
    let mut mesh = Mesh {
        verts: Vec::new(),
        tris: Vec::new(),
    };
    let mut ids: HashMap<(Vid, u32), u32> = HashMap::new();
    let mut vertex = |v: Vid, k: u32, mesh: &mut Mesh| -> u32 {
        *ids.entry((v, k)).or_insert_with(|| {
            let (x, y) = xy(v);
            mesh.verts.push([
                (x as f64 * s) as f32,
                (y as f64 * s) as f32,
                (k as f64 * lh) as f32,
            ]);
            (mesh.verts.len() - 1) as u32
        })
    };
    let kn = |i: usize, dx: i64, dy: i64| -> u32 {
        let (x, y) = ((i % w) as i64 + dx, (i / w) as i64 + dy);
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            l
        } else {
            kmin[y as usize * w + x as usize]
        }
    };

    let column_levels = |v: Vid, lo: u32, hi: u32| -> Vec<u32> {
        let (x, y) = xy(v);
        let mut out = Vec::new();
        for (dx, dy) in [(-1, -1), (0, -1), (-1, 0), (0, 0)] {
            let (px, py) = (x + dx, y + dy);
            if px >= 0 && py >= 0 && px < w as i64 && py < h as i64 {
                let k = kmin[py as usize * w + px as usize];
                if k > lo && k < hi {
                    out.push(k);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    };

    // Planes: one cap per distinct kmin value (facing down) and the top cap at plane l (facing up).
    let mut by_k: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, &k) in kmin.iter().enumerate() {
        if k < l {
            by_k.entry(k).or_default().push(i);
        }
    }
    let mut planes: Vec<u32> = by_k.keys().copied().collect();
    planes.sort_unstable();
    let all: Vec<usize> = by_k.values().flatten().copied().collect();

    let mut jobs: Vec<(u32, bool, Vec<Loop>)> = Vec::new();
    for &k in &planes {
        let edges = boundary_edges(&by_k[&k], |j| kmin[j] == k, kn, w, h);
        jobs.push((
            k,
            false,
            trace(&edges, vw)
                .into_iter()
                .map(|q| simplify(q, vw))
                .collect(),
        ));
    }
    let edges = boundary_edges(&all, |j| kmin[j] < l, |i, _, _| kmin[i], w, h);
    jobs.push((
        l,
        true,
        trace(&edges, vw)
            .into_iter()
            .map(|q| simplify(q, vw))
            .collect(),
    ));

    for (plane, up, loops) in &jobs {
        let areas: Vec<f64> = loops.iter().map(|q| signed_area(&q.v, vw)).collect();
        let outers: Vec<usize> = (0..loops.len()).filter(|&i| areas[i] > 0.0).collect();
        let bbox = |q: &Loop| {
            let (mut a, mut b, mut c, mut d) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
            for &v in &q.v {
                let (x, y) = xy(v);
                a = a.min(x);
                b = b.min(y);
                c = c.max(x);
                d = d.max(y);
            }
            (a, b, c, d)
        };
        let boxes: Vec<_> = loops.iter().map(bbox).collect();
        let mut holes_of: HashMap<usize, Vec<usize>> = HashMap::new();
        for hi in (0..loops.len()).filter(|&i| areas[i] < 0.0) {
            let pt = xy(loops[hi].v[0]);
            let ptf = (pt.0 as f64, pt.1 as f64);
            let hb = boxes[hi];
            let owner = outers
                .iter()
                .copied()
                .filter(|&o| {
                    let ob = boxes[o];
                    ob.0 <= hb.0
                        && ob.1 <= hb.1
                        && ob.2 >= hb.2
                        && ob.3 >= hb.3
                        && (point_in(&loops[o].v, vw, (ptf.0 + 1e-3, ptf.1 + 1e-3))
                            || point_in(&loops[o].v, vw, (ptf.0 - 1e-3, ptf.1 - 1e-3))
                            || point_in(&loops[o].v, vw, (ptf.0 + 1e-3, ptf.1 - 1e-3))
                            || point_in(&loops[o].v, vw, (ptf.0 - 1e-3, ptf.1 + 1e-3)))
                })
                .min_by(|&a, &b| areas[a].partial_cmp(&areas[b]).unwrap());
            holes_of
                .entry(owner.ok_or("hole without outer loop")?)
                .or_default()
                .push(hi);
        }
        for &o in &outers {
            let group: Vec<usize> = std::iter::once(o)
                .chain(holes_of.get(&o).into_iter().flatten().copied())
                .collect();
            let shape: Vec<Vec<IntPoint>> = group
                .iter()
                .map(|&li| {
                    loops[li]
                        .v
                        .iter()
                        .map(|&v| {
                            let (x, y) = xy(v);
                            IntPoint::new(x as i32, y as i32)
                        })
                        .collect()
                })
                .collect();
            let tri = shape.uncheck_triangulate().into_triangulation::<u32>();
            for c in tri.indices.chunks(3) {
                let (pa, pb, pc) = (
                    tri.points[c[0] as usize],
                    tri.points[c[1] as usize],
                    tri.points[c[2] as usize],
                );
                let cross = (pb.x as i64 - pa.x as i64) * (pc.y as i64 - pa.y as i64)
                    - (pb.y as i64 - pa.y as i64) * (pc.x as i64 - pa.x as i64);
                if cross == 0 {
                    continue;
                }
                let vid_of = |q: &IntPoint| (q.y as usize * vw + q.x as usize) as Vid;
                let (a, b, d) = (
                    vertex(vid_of(&pa), *plane, &mut mesh),
                    vertex(vid_of(&pb), *plane, &mut mesh),
                    vertex(vid_of(&pc), *plane, &mut mesh),
                );
                let ccw = (cross > 0) == *up;
                mesh.tris.push(if ccw { [a, b, d] } else { [a, d, b] });
            }
        }
        if !*up {
            for lp in loops {
                let n = lp.v.len();
                for i in 0..n {
                    let (v0, v1) = (lp.v[i], lp.v[(i + 1) % n]);
                    let j = lp.tag[i];
                    if j > *plane {
                        // Vertical edges carry every plane that touches the column, so neighbouring walls share vertices.
                        let mut poly =
                            vec![vertex(v0, *plane, &mut mesh), vertex(v1, *plane, &mut mesh)];
                        for m in column_levels(v1, *plane, j) {
                            poly.push(vertex(v1, m, &mut mesh));
                        }
                        poly.push(vertex(v1, j, &mut mesh));
                        poly.push(vertex(v0, j, &mut mesh));
                        for m in column_levels(v0, *plane, j).into_iter().rev() {
                            poly.push(vertex(v0, m, &mut mesh));
                        }
                        for t in 1..poly.len() - 1 {
                            mesh.tris.push([poly[0], poly[t], poly[t + 1]]);
                        }
                    }
                }
            }
        }
    }
    Ok(mesh)
}
