//! Triangle meshes: STL in/out, primitives, watertightness and 3D booleans (via boolmesh).
use boolmesh::prelude::*;
use std::collections::HashMap;

#[derive(Clone, Default)]
pub struct Mesh {
    pub v: Vec<[f64; 3]>,
    pub t: Vec<[usize; 3]>,
}

pub type P3 = [f64; 3];

impl Mesh {
    pub fn from_stl(b: &[u8]) -> Result<Mesh, String> {
        if b.len() < 84 {
            return Err("not a binary STL".into());
        }
        let n = u32::from_le_bytes([b[80], b[81], b[82], b[83]]) as usize;
        if b.len() < 84 + n * 50 {
            return Err("truncated STL".into());
        }
        let mut m = Mesh::default();
        let mut ids: HashMap<[u32; 3], usize> = HashMap::new();
        for i in 0..n {
            let o = 84 + i * 50 + 12;
            let mut tri = [0usize; 3];
            for (k, slot) in tri.iter_mut().enumerate() {
                let f = |j: usize| {
                    f32::from_le_bytes([
                        b[o + k * 12 + j],
                        b[o + k * 12 + j + 1],
                        b[o + k * 12 + j + 2],
                        b[o + k * 12 + j + 3],
                    ])
                };
                let p = [f(0), f(4), f(8)];
                let key = [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()];
                *slot = *ids.entry(key).or_insert_with(|| {
                    m.v.push([p[0] as f64, p[1] as f64, p[2] as f64]);
                    m.v.len() - 1
                });
            }
            m.t.push(tri);
        }
        Ok(m)
    }

    pub fn to_stl(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(84 + self.t.len() * 50);
        out.extend_from_slice(&[0u8; 80]);
        out.extend_from_slice(&(self.t.len() as u32).to_le_bytes());
        for t in &self.t {
            let (a, b, c) = (self.v[t[0]], self.v[t[1]], self.v[t[2]]);
            let (u, w) = (
                [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
            );
            let mut n = [
                u[1] * w[2] - u[2] * w[1],
                u[2] * w[0] - u[0] * w[2],
                u[0] * w[1] - u[1] * w[0],
            ];
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if l > 0.0 {
                n = [n[0] / l, n[1] / l, n[2] / l];
            }
            for f in n.iter().chain(a.iter()).chain(b.iter()).chain(c.iter()) {
                out.extend_from_slice(&(*f as f32).to_le_bytes());
            }
            out.extend_from_slice(&[0, 0]);
        }
        out
    }

    pub fn bounds(&self) -> (P3, P3) {
        let mut lo = [f64::MAX; 3];
        let mut hi = [f64::MIN; 3];
        for p in &self.v {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }

    pub fn volume(&self) -> f64 {
        self.t
            .iter()
            .map(|t| {
                let (a, b, c) = (self.v[t[0]], self.v[t[1]], self.v[t[2]]);
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]))
                    / 6.0
            })
            .sum()
    }

    /// True when the box lies entirely inside the solid: no surface triangle touches it
    /// and its centre is inside. Far cheaper than a boolean for placement tests.
    pub fn box_inside(&self, lo: P3, hi: P3) -> bool {
        let c = [
            (lo[0] + hi[0]) / 2.0,
            (lo[1] + hi[1]) / 2.0,
            (lo[2] + hi[2]) / 2.0,
        ];
        let he = [
            (hi[0] - lo[0]) / 2.0,
            (hi[1] - lo[1]) / 2.0,
            (hi[2] - lo[2]) / 2.0,
        ];
        // ray towards +z with a tiny xy offset to dodge shared edges
        let (rx, ry) = (c[0] + 1.3e-7, c[1] + 0.7e-7);
        let mut crossings = 0;
        for t in &self.t {
            let v = [self.v[t[0]], self.v[t[1]], self.v[t[2]]];
            if tri_box(&v, c, he) {
                return false;
            }
            let d = |a: P3, b: P3| (b[0] - a[0]) * (ry - a[1]) - (b[1] - a[1]) * (rx - a[0]);
            let (e0, e1, e2) = (d(v[0], v[1]), d(v[1], v[2]), d(v[2], v[0]));
            if (e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0) || (e0 <= 0.0 && e1 <= 0.0 && e2 <= 0.0) {
                let area = e0 + e1 + e2;
                if area.abs() > 1e-18 {
                    let z = (v[0][2] * e1 + v[1][2] * e2 + v[2][2] * e0) / area;
                    if z > c[2] {
                        crossings += 1;
                    }
                }
            }
        }
        crossings % 2 == 1
    }

    /// Every directed edge appears once and its reverse appears once too (a closed, consistently wound surface).
    pub fn is_watertight(&self) -> bool {
        let key = |a: usize, b: usize| ((a as u64) << 32) | b as u64;
        let mut fwd = Vec::with_capacity(self.t.len() * 3);
        let mut rev = Vec::with_capacity(self.t.len() * 3);
        for t in &self.t {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                if a == b {
                    return false;
                }
                fwd.push(key(a, b));
                rev.push(key(b, a));
            }
        }
        fwd.sort_unstable();
        rev.sort_unstable();
        fwd.windows(2).all(|w| w[0] != w[1]) && fwd == rev
    }

    /// Rounds the vertices to the single precision an STL file stores, welds the ones that become identical and
    /// drops the triangles that collapse, so the mesh checked is the mesh a slicer reads.
    pub fn quantize_f32(&mut self) {
        let mut ids: std::collections::HashMap<[u32; 3], usize> = Default::default();
        let mut v: Vec<P3> = Vec::new();
        let map: Vec<usize> = self
            .v
            .iter()
            .map(|p| {
                let q = [p[0] as f32, p[1] as f32, p[2] as f32];
                let key = q.map(f32::to_bits);
                *ids.entry(key).or_insert_with(|| {
                    v.push(q.map(f64::from));
                    v.len() - 1
                })
            })
            .collect();
        self.t = self
            .t
            .iter()
            .map(|t| [map[t[0]], map[t[1]], map[t[2]]])
            .filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
            .collect();
        self.v = v;
        self.drop_cancelling_pairs();
    }

    /// Welding the two sides of a sliver thinner than single precision leaves a zero-thickness fin: a triangle and
    /// its exact reverse. They enclose nothing, so each such pair is removed.
    fn drop_cancelling_pairs(&mut self) {
        let mut seen: std::collections::HashMap<[usize; 3], Vec<(usize, bool)>> =
            Default::default();
        for (i, t) in self.t.iter().enumerate() {
            let mut key = *t;
            key.sort_unstable();
            // the winding is even when the sorted order is a rotation of the triangle
            let forward = (0..3).any(|r| [t[r], t[(r + 1) % 3], t[(r + 2) % 3]] == key);
            seen.entry(key).or_default().push((i, forward));
        }
        let mut gone = vec![false; self.t.len()];
        for list in seen.values() {
            let mut fwd: Vec<usize> = list.iter().filter(|e| e.1).map(|e| e.0).collect();
            let mut rev: Vec<usize> = list.iter().filter(|e| !e.1).map(|e| e.0).collect();
            while let (Some(a), Some(b)) = (fwd.pop(), rev.pop()) {
                gone[a] = true;
                gone[b] = true;
            }
        }
        if gone.iter().any(|&g| g) {
            let mut i = 0;
            self.t.retain(|_| {
                i += 1;
                !gone[i - 1]
            });
        }
    }

    /// Up to `limit` edges that break watertightness, as (from, to, times used, times reversed used).
    pub fn bad_edges(&self, limit: usize) -> Vec<(P3, P3, usize, usize)> {
        let mut edges: std::collections::HashMap<(usize, usize), usize> = Default::default();
        for t in &self.t {
            for k in 0..3 {
                *edges.entry((t[k], t[(k + 1) % 3])).or_default() += 1;
            }
        }
        let mut bad: Vec<_> = edges
            .iter()
            .filter(|&(&(a, b), &n)| n != 1 || edges.get(&(b, a)) != Some(&1))
            .map(|(&(a, b), &n)| (a, b, n))
            .collect();
        bad.sort_unstable();
        bad.truncate(limit);
        bad.into_iter()
            .map(|(a, b, n)| {
                (
                    self.v[a],
                    self.v[b],
                    n,
                    edges.get(&(b, a)).copied().unwrap_or(0),
                )
            })
            .collect()
    }

    pub fn translate(&mut self, d: P3) {
        for p in &mut self.v {
            for k in 0..3 {
                p[k] += d[k];
            }
        }
    }

    pub fn append(&mut self, o: &Mesh) {
        let base = self.v.len();
        self.v.extend_from_slice(&o.v);
        self.t
            .extend(o.t.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
    }

    /// Axis-aligned box.
    pub fn cuboid(lo: P3, hi: P3) -> Mesh {
        let v = (0..8)
            .map(|i| {
                [
                    if i & 1 == 0 { lo[0] } else { hi[0] },
                    if i & 2 == 0 { lo[1] } else { hi[1] },
                    if i & 4 == 0 { lo[2] } else { hi[2] },
                ]
            })
            .collect();
        let q = |a, b, c, d| [[a, b, c], [a, c, d]];
        let f = [
            q(0, 2, 3, 1),
            q(4, 5, 7, 6),
            q(0, 1, 5, 4),
            q(2, 6, 7, 3),
            q(0, 4, 6, 2),
            q(1, 3, 7, 5),
        ];
        Mesh {
            v,
            t: f.iter().flatten().copied().collect(),
        }
    }

    /// A prism over the convex quadrilateral `c` (counter-clockwise in plan), extruded along z from `z0` to `z1`.
    pub fn quad_prism(c: [[f64; 2]; 4], z0: f64, z1: f64) -> Mesh {
        let mut v: Vec<P3> = c.iter().map(|p| [p[0], p[1], z0]).collect();
        v.extend(c.iter().map(|p| [p[0], p[1], z1]));
        let mut t = vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7]];
        for k in 0..4 {
            let n = (k + 1) % 4;
            t.push([k, n, n + 4]);
            t.push([k, n + 4, k + 4]);
        }
        Mesh { v, t }
    }

    /// Prism with a regular n-gon profile (circumradius r), extruded along z from z0 to z1.
    pub fn prism_z(n: usize, r: f64, z0: f64, z1: f64) -> Mesh {
        let mut v = Vec::new();
        for z in [z0, z1] {
            for i in 0..n {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                v.push([r * a.cos(), r * a.sin(), z]);
            }
        }
        let mut t = Vec::new();
        for i in 0..n {
            let j = (i + 1) % n;
            t.push([i, j, n + j]);
            t.push([i, n + j, n + i]);
        }
        for i in 1..n - 1 {
            t.push([0, i + 1, i]);
            t.push([n, n + i, n + i + 1]);
        }
        Mesh { v, t }
    }

    /// Convex polygon (counter-clockwise) extruded from z0 to z1.
    pub fn extrude_convex(poly: &[[f64; 2]], z0: f64, z1: f64) -> Mesh {
        let n = poly.len();
        let mut v: Vec<P3> = poly.iter().map(|p| [p[0], p[1], z0]).collect();
        v.extend(poly.iter().map(|p| [p[0], p[1], z1]));
        let mut t = Vec::new();
        for i in 0..n {
            let j = (i + 1) % n;
            t.push([i, j, n + j]);
            t.push([i, n + j, n + i]);
        }
        for i in 1..n - 1 {
            t.push([0, i + 1, i]);
            t.push([n, n + i, n + i + 1]);
        }
        Mesh { v, t }
    }

    /// Rotates (x, y, z) -> new axes: maps the prism's z axis onto `axis` (0 = x, 1 = y, 2 = z).
    pub fn along(mut self, axis: usize) -> Mesh {
        let map = |p: P3| match axis {
            0 => [p[2], p[0], p[1]],
            1 => [p[0], p[2], p[1]],
            _ => p,
        };
        for p in &mut self.v {
            *p = map(*p);
        }
        if axis == 1 {
            for t in &mut self.t {
                t.swap(1, 2);
            }
        }
        self
    }

    fn manifold(&self) -> Result<Manifold, String> {
        let pos: Vec<f64> = self.v.iter().flat_map(|p| p.iter().copied()).collect();
        let idx: Vec<usize> = self.t.iter().flat_map(|t| t.iter().copied()).collect();
        Manifold::new(&pos, &idx)
    }

    fn from_manifold(m: &Manifold) -> Mesh {
        let v = m.ps.iter().map(|p| [p.x, p.y, p.z]).collect();
        let t =
            m.hs.chunks(3)
                .map(|h| [h[0].tail, h[1].tail, h[2].tail])
                .collect();
        Mesh { v, t }
    }

    fn op(&self, o: &Mesh, op: OpType) -> Result<Mesh, String> {
        // the boolean engine rejects empty inputs; the result is known without it
        if self.t.is_empty() || o.t.is_empty() {
            return Ok(match op {
                OpType::Add if self.t.is_empty() => o.clone(),
                OpType::Add | OpType::Subtract if !self.t.is_empty() => self.clone(),
                _ => Mesh::default(),
            });
        }
        match compute_boolean(&self.manifold()?, &o.manifold()?, op) {
            Ok(r) => Ok(Mesh::from_manifold(&r)),
            // the engine reports a result with no geometry as an error
            Err(e) if e.contains("empty pos matrix") => Ok(Mesh::default()),
            Err(e) => Err(e),
        }
    }

    pub fn subtract(&self, o: &Mesh) -> Result<Mesh, String> {
        self.op(o, OpType::Subtract)
    }
    pub fn intersect(&self, o: &Mesh) -> Result<Mesh, String> {
        self.op(o, OpType::Intersect)
    }
    pub fn union(&self, o: &Mesh) -> Result<Mesh, String> {
        self.op(o, OpType::Add)
    }
}

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: P3, b: P3) -> P3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Triangle / axis-aligned box overlap (separating axis test), box given by centre and half-extents.
fn tri_box(v: &[P3; 3], c: P3, h: P3) -> bool {
    let p = [sub(v[0], c), sub(v[1], c), sub(v[2], c)];
    for k in 0..3 {
        let (mn, mx) = (
            p[0][k].min(p[1][k]).min(p[2][k]),
            p[0][k].max(p[1][k]).max(p[2][k]),
        );
        if mn > h[k] || mx < -h[k] {
            return false;
        }
    }
    let e = [sub(p[1], p[0]), sub(p[2], p[1]), sub(p[0], p[2])];
    for ed in &e {
        for k in 0..3 {
            let mut ax = [0.0; 3];
            ax[k] = 1.0;
            let a = cross(ax, *ed);
            let r = h[0] * a[0].abs() + h[1] * a[1].abs() + h[2] * a[2].abs();
            let d: Vec<f64> = p
                .iter()
                .map(|q| q[0] * a[0] + q[1] * a[1] + q[2] * a[2])
                .collect();
            if d[0].min(d[1]).min(d[2]) > r || d[0].max(d[1]).max(d[2]) < -r {
                return false;
            }
        }
    }
    let n = cross(e[0], e[1]);
    let r = h[0] * n[0].abs() + h[1] * n[1].abs() + h[2] * n[2].abs();
    let d = p[0][0] * n[0] + p[0][1] * n[1] + p[0][2] * n[2];
    d.abs() <= r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantizing_drops_zero_thickness_fins() {
        let mut m = Mesh::cuboid([0.0; 3], [10.0, 10.0, 4.0]);
        let n = m.v.len();
        m.v.push([20.0, 0.0, 0.0]);
        m.v.push([20.0, 1.0, 0.0]);
        m.v.push([20.0, 0.0, 1.0]);
        // two triangles that are exact reverses of each other, attached to nothing
        m.t.push([n, n + 1, n + 2]);
        m.t.push([n, n + 2, n + 1]);
        let whole = m.t.len() - 2;
        m.quantize_f32();
        assert_eq!(m.t.len(), whole);
        assert!(m.is_watertight());
    }

    #[test]
    fn box_boolean_roundtrip() {
        let a = Mesh::cuboid([0.0; 3], [10.0, 10.0, 4.0]);
        assert!(a.is_watertight());
        assert!((a.volume() - 400.0).abs() < 1e-9);
        let hole = Mesh::prism_z(6, 1.5, -1.0, 5.0);
        assert!(hole.is_watertight() && hole.volume() > 0.0);
        let mut h = hole.clone();
        h.translate([5.0, 5.0, 0.0]);
        let r = a.subtract(&h).unwrap();
        assert!(r.is_watertight());
        assert!(r.volume() < 400.0 - 20.0);
        let mut side = Mesh::prism_z(16, 1.0, -3.0, 3.0).along(0);
        side.translate([5.0, 5.0, 2.0]);
        assert!(side.volume() > 0.0 && side.is_watertight());
        assert!(r.subtract(&side).unwrap().is_watertight());
    }
}
