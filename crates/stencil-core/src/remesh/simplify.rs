//! Chain simplification: the corner-keeping, deviation-bounded thinning behind Remesh.

use super::*;

/// How far the points strictly between `a` and `b` lie outside and inside the chord a-b.
pub(super) fn chord_cost(
    pts: &[[f64; 2]],
    a: usize,
    b: usize,
    outward_is_right: bool,
) -> (f64, f64) {
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
pub(super) fn simplify_open(
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
pub(super) struct Knobs {
    pub(super) smooth_limit: f64,
    pub(super) passes: usize,
    pub(super) chord: f64,
    pub(super) loss: f64,
    pub(super) outward_is_right: bool,
}

pub(super) fn simplify_chain(pts: &[IntPoint], k: &Knobs, dev: &mut f64) -> Vec<IntPoint> {
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

pub(super) type Pt = (i32, i32);

pub(super) fn pt(p: IntPoint) -> Pt {
    (p.x, p.y)
}

pub(super) fn edge_key(a: IntPoint, b: IntPoint) -> (Pt, Pt) {
    if pt(a) < pt(b) {
        (pt(a), pt(b))
    } else {
        (pt(b), pt(a))
    }
}

/// Inserts into every outline edge the vertices of any layer that lie exactly on it, so layers that
/// overlap along a straight line share identical edges.
pub(super) fn node_units(units: &[Region]) -> Vec<Region> {
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
pub(super) fn simplify_units(
    units: &[Region],
    touched: &[bool],
    k: &Knobs,
    dev: &mut f64,
) -> Vec<Region> {
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
