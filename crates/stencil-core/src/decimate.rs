//! Mesh decimation: edge collapses ranked by the quadric error of the planes around a vertex (Garland and Heckbert).
//! A collapse is only taken while the accumulated error stays below the tolerance, so flat faces and straight edges
//! lose their redundant vertices and creases stay put. A collapse never changes the topology (link condition) and
//! never flips a face.
use crate::mesh::Mesh;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

type Quadric = [f64; 10];

fn plane_quadric(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Option<Quadric> {
    let n = cross(sub(b, a), sub(c, a));
    let len = dot(n, n).sqrt();
    if len < 1e-12 {
        return None;
    }
    let (x, y, z) = (n[0] / len, n[1] / len, n[2] / len);
    let d = -(x * a[0] + y * a[1] + z * a[2]);
    Some([
        x * x,
        x * y,
        x * z,
        x * d,
        y * y,
        y * z,
        y * d,
        z * z,
        z * d,
        d * d,
    ])
}

fn eval(q: &Quadric, p: [f64; 3]) -> f64 {
    let [x, y, z] = p;
    (q[0] * x * x
        + 2.0 * q[1] * x * y
        + 2.0 * q[2] * x * z
        + 2.0 * q[3] * x
        + q[4] * y * y
        + 2.0 * q[5] * y * z
        + 2.0 * q[6] * y
        + q[7] * z * z
        + 2.0 * q[8] * z
        + q[9])
        .max(0.0)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

struct State {
    pos: Vec<[f64; 3]>,
    tri: Vec<[usize; 3]>,
    alive: Vec<bool>,
    vf: Vec<Vec<usize>>,
    q: Vec<Quadric>,
    stamp: Vec<u32>,
    gone: Vec<bool>,
}

impl State {
    fn faces_of(&mut self, v: usize) -> Vec<usize> {
        let alive = &self.alive;
        self.vf[v].retain(|&f| alive[f]);
        self.vf[v].clone()
    }

    fn neighbours(&mut self, v: usize) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        for f in self.faces_of(v) {
            for &w in &self.tri[f] {
                if w != v && !out.contains(&w) {
                    out.push(w);
                }
            }
        }
        out
    }

    fn cost(&self, v: usize, u: usize) -> f64 {
        let mut q = self.q[v];
        for (a, b) in q.iter_mut().zip(&self.q[u]) {
            *a += b;
        }
        eval(&q, self.pos[u])
    }

    /// Whether moving `v` onto `u` keeps the surface a closed manifold without folding any face over.
    fn can_collapse(&mut self, v: usize, u: usize) -> bool {
        let fv = self.faces_of(v);
        let shared: Vec<usize> = fv
            .iter()
            .copied()
            .filter(|&f| self.tri[f].contains(&u))
            .collect();
        if shared.is_empty() {
            return false;
        }
        let nv = self.neighbours(v);
        let nu = self.neighbours(u);
        let common = nv.iter().filter(|w| nu.contains(w)).count();
        if common != shared.len() {
            return false;
        }
        for &f in &fv {
            let t = self.tri[f];
            if t.contains(&u) {
                continue;
            }
            let old = cross(
                sub(self.pos[t[1]], self.pos[t[0]]),
                sub(self.pos[t[2]], self.pos[t[0]]),
            );
            let moved = t.map(|w| if w == v { u } else { w });
            let new = cross(
                sub(self.pos[moved[1]], self.pos[moved[0]]),
                sub(self.pos[moved[2]], self.pos[moved[0]]),
            );
            let (lo, ln) = (dot(old, old).sqrt(), dot(new, new).sqrt());
            if lo < 1e-12 {
                continue;
            }
            if ln < 1e-12 || dot(old, new) < 0.2 * lo * ln {
                return false;
            }
        }
        true
    }

    fn collapse(&mut self, v: usize, u: usize) {
        for f in self.faces_of(v) {
            if self.tri[f].contains(&u) {
                self.alive[f] = false;
            } else {
                for w in self.tri[f].iter_mut() {
                    if *w == v {
                        *w = u;
                    }
                }
                self.vf[u].push(f);
            }
        }
        let qv = self.q[v];
        for (a, b) in self.q[u].iter_mut().zip(&qv) {
            *a += b;
        }
        self.gone[v] = true;
        self.stamp[v] += 1;
        self.stamp[u] += 1;
    }
}

type Entry = (Reverse<u64>, usize, usize, u32, u32);

/// Removes vertices whose loss moves the surface by less than `tolerance` (mm). Vertices outside `z_range` are
/// kept. Returns the estimated largest deviation in mm.
pub fn decimate(mesh: &mut Mesh, tolerance: f64, z_range: (f64, f64)) -> f64 {
    let n = mesh.v.len();
    let mut s = State {
        pos: mesh.v.clone(),
        tri: mesh.t.clone(),
        alive: vec![true; mesh.t.len()],
        vf: vec![Vec::new(); n],
        q: vec![[0.0; 10]; n],
        stamp: vec![0; n],
        gone: vec![false; n],
    };
    for (i, t) in mesh.t.iter().enumerate() {
        for &v in t {
            s.vf[v].push(i);
        }
        if let Some(k) = plane_quadric(s.pos[t[0]], s.pos[t[1]], s.pos[t[2]]) {
            for &v in t {
                for (a, b) in s.q[v].iter_mut().zip(&k) {
                    *a += b;
                }
            }
        }
    }
    let locked: Vec<bool> = s
        .pos
        .iter()
        .map(|p| p[2] < z_range.0 - 1e-6 || p[2] > z_range.1 + 1e-6)
        .collect();
    let limit = tolerance * tolerance;
    let mut heap: BinaryHeap<Entry> = BinaryHeap::new();
    let push = |s: &State, heap: &mut BinaryHeap<Entry>, v: usize, u: usize| {
        if locked[v] {
            return;
        }
        let c = s.cost(v, u);
        if c <= limit {
            heap.push((Reverse(c.to_bits()), v, u, s.stamp[v], s.stamp[u]));
        }
    };
    for t in &mesh.t {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            push(&s, &mut heap, a, b);
            push(&s, &mut heap, b, a);
        }
    }
    let mut worst: f64 = 0.0;
    while let Some((Reverse(bits), v, u, sv, su)) = heap.pop() {
        if s.gone[v] || s.gone[u] || s.stamp[v] != sv || s.stamp[u] != su {
            continue;
        }
        if !s.can_collapse(v, u) {
            continue;
        }
        worst = worst.max(f64::from_bits(bits));
        s.collapse(v, u);
        for w in s.neighbours(u) {
            push(&s, &mut heap, u, w);
            push(&s, &mut heap, w, u);
        }
    }

    let mut index = vec![usize::MAX; n];
    let mut out = Mesh::default();
    for (f, t) in s.tri.iter().enumerate() {
        if !s.alive[f] {
            continue;
        }
        out.t.push(t.map(|v| {
            if index[v] == usize::MAX {
                index[v] = out.v.len();
                out.v.push(s.pos[v]);
            }
            index[v]
        }));
    }
    *mesh = out;
    worst.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cube whose top face carries a 6x6 grid of extra vertices.
    fn busy_cube() -> Mesh {
        let mut m = Mesh::default();
        let c = Mesh::cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 2.0]);
        m.append(&c);
        m
    }

    #[test]
    fn a_plain_cube_is_left_alone() {
        let mut m = busy_cube();
        let before = m.t.len();
        decimate(&mut m, 0.05, (f64::MIN, f64::MAX));
        assert_eq!(m.t.len(), before);
        assert!(m.is_watertight());
    }

    #[test]
    fn collinear_vertices_and_flat_interiors_collapse_and_creases_stay() {
        // a wall of a prism with many collinear points: 40 points on a straight line, extruded
        let pts: Vec<[f64; 2]> = (0..40)
            .map(|i| [i as f64, 0.0])
            .chain([[40.0, 10.0], [0.0, 10.0]])
            .collect();
        let mut m = Mesh::default();
        let n = pts.len();
        for z in [0.0, 2.0] {
            for p in &pts {
                m.v.push([p[0], p[1], z]);
            }
        }
        for i in 0..n {
            let j = (i + 1) % n;
            m.t.push([i, j, n + j]);
            m.t.push([i, n + j, n + i]);
        }
        for i in 1..n - 1 {
            m.t.push([0, i + 1, i]);
            m.t.push([n, n + i, n + i + 1]);
        }
        assert!(m.is_watertight());
        let volume = m.volume();
        let before = m.t.len();
        let dev = decimate(&mut m, 0.01, (f64::MIN, f64::MAX));
        assert!(m.is_watertight(), "decimation must keep the mesh closed");
        assert!(m.t.len() < before / 2, "{} -> {}", before, m.t.len());
        assert!((m.volume() - volume).abs() < 0.05 * volume);
        assert!(dev <= 0.01 + 1e-9);
        let (lo, hi) = m.bounds();
        assert_eq!((lo, hi), ([0.0, 0.0, 0.0], [40.0, 10.0, 2.0]));
    }

    #[test]
    fn vertices_outside_the_z_range_are_frozen() {
        let mut m = Mesh::default();
        let n = 20;
        for z in [0.0, 2.0] {
            for i in 0..n {
                m.v.push([i as f64, 0.0, z]);
            }
            m.v.push([n as f64, 5.0, z]);
        }
        let k = n + 1;
        for i in 0..k {
            let j = (i + 1) % k;
            m.t.push([i, j, k + j]);
            m.t.push([i, k + j, k + i]);
        }
        for i in 1..k - 1 {
            m.t.push([0, i + 1, i]);
            m.t.push([k, k + i, k + i + 1]);
        }
        let before = m.t.len();
        decimate(&mut m, 0.01, (5.0, 6.0));
        assert_eq!(m.t.len(), before);
    }
}
