//! Island analysis on the material grid: labelling, distance transform, diagonal repair and bridging.

use std::collections::{HashMap, VecDeque};

/// Material grid, row 0 at the bottom (y up). `true` = plate, `false` = hole (paint).
pub(crate) struct Grid {
    pub(crate) w: usize,
    pub(crate) h: usize,
    pub(crate) m: Vec<bool>,
}

/// 4-connected plate components. Returns (islands, label per pixel); the frame is the component at (0,0).
pub(crate) fn label_islands(g: &Grid) -> (usize, Vec<u32>) {
    let mut label = vec![0u32; g.w * g.h];
    let mut n = 0u32;
    let mut stack = Vec::new();
    for start in 0..g.w * g.h {
        if !g.m[start] || label[start] != 0 {
            continue;
        }
        n += 1;
        label[start] = n;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % g.w, i / g.w);
            let mut push = |j: usize| {
                if g.m[j] && label[j] == 0 {
                    label[j] = n;
                    stack.push(j);
                }
            };
            if x > 0 {
                push(i - 1);
            }
            if x + 1 < g.w {
                push(i + 1);
            }
            if y > 0 {
                push(i - g.w);
            }
            if y + 1 < g.h {
                push(i + g.w);
            }
        }
    }
    (n.saturating_sub(1) as usize, label)
}

/// Squared-distance transform (Felzenszwalb): distance in pixels from every pixel to the nearest `true` pixel.
pub(crate) fn edt_sq(src: &[bool], w: usize, h: usize) -> Vec<f32> {
    const INF: f32 = 1e12;
    let mut g: Vec<f32> = src.iter().map(|&b| if b { 0.0 } else { INF }).collect();
    let pass = |f: &[f32], n: usize| -> Vec<f32> {
        let mut d = vec![0f32; n];
        let mut v = vec![0usize; n];
        let mut z = vec![0f32; n + 1];
        let mut k = 0usize;
        z[0] = -INF;
        z[1] = INF;
        for q in 1..n {
            loop {
                let r = v[k];
                let s = ((f[q] + (q * q) as f32) - (f[r] + (r * r) as f32))
                    / (2.0 * (q as f32 - r as f32));
                if s <= z[k] && k > 0 {
                    k -= 1;
                    continue;
                }
                if s <= z[k] {
                    v[0] = q;
                    z[1] = INF;
                    break;
                }
                k += 1;
                v[k] = q;
                z[k] = s;
                z[k + 1] = INF;
                break;
            }
        }
        k = 0;
        for q in 0..n {
            while z[k + 1] < q as f32 {
                k += 1;
            }
            let r = v[k];
            d[q] = (q as f32 - r as f32).powi(2) + f[r];
        }
        d
    };
    let mut col = vec![0f32; h];
    for x in 0..w {
        for y in 0..h {
            col[y] = g[y * w + x];
        }
        let d = pass(&col, h);
        for y in 0..h {
            g[y * w + x] = d[y];
        }
    }
    for y in 0..h {
        let d = pass(&g[y * w..(y + 1) * w].to_vec(), w);
        g[y * w..(y + 1) * w].copy_from_slice(&d);
    }
    g
}

/// Area (mm2, summed over printed layers) that sticks out more than layer_height*tan(angle) past the layer below.
#[allow(clippy::too_many_arguments)]
pub(crate) fn overhang_area(
    kmin: &[u32],
    w: usize,
    h: usize,
    l: u32,
    s: f64,
    lh: f64,
    angle: f64,
    flip: bool,
) -> f64 {
    let angle = if angle <= 0.0 { 45.0 } else { angle.min(89.0) };
    let reach = lh * angle.to_radians().tan() / s + 1e-3;
    let mut total = 0usize;
    let layer = |k: u32| -> Vec<bool> { kmin.iter().map(|&q| q <= k).collect() };
    for j in 1..l {
        let (cur_k, prev_k) = if flip { (l - 1 - j, l - j) } else { (j, j - 1) };
        if (0..w * h).all(|i| (kmin[i] <= cur_k) == (kmin[i] <= prev_k)) {
            continue;
        }
        let (cur, prev) = (layer(cur_k), layer(prev_k));
        let d = edt_sq(&prev, w, h);
        total += (0..w * h)
            .filter(|&i| cur[i] && (d[i].sqrt() as f64) > reach)
            .count();
    }
    total as f64 * s * s
}

pub(crate) fn delete_small_islands(g: &mut Grid, label: &[u32], n: usize, min_px: f64) -> usize {
    let mut area = vec![0usize; n + 2];
    for &q in label {
        area[q as usize] += 1;
    }
    let frame = label[0];
    let mut deleted = 0;
    let mut gone = vec![false; n + 2];
    for q in 1..=n + 1 {
        if q as u32 != frame && area[q] > 0 && (area[q] as f64) < min_px {
            gone[q] = true;
            deleted += 1;
        }
    }
    for i in 0..label.len() {
        if label[i] != 0 && gone[label[i] as usize] {
            g.m[i] = false;
        }
    }
    deleted
}

/// Pixels in the kmin map that touch only diagonally in some layer would give a non-manifold edge: fill one.
pub(crate) fn fix_diagonals_k(k: &mut [u32], w: usize, h: usize, l: u32) {
    loop {
        let mut changed = false;
        for y in 0..h.saturating_sub(1) {
            for x in 0..w.saturating_sub(1) {
                let (ia, ib, ic, id) = (
                    y * w + x,
                    y * w + x + 1,
                    (y + 1) * w + x,
                    (y + 1) * w + x + 1,
                );
                let (a, b, c, d) = (k[ia], k[ib], k[ic], k[id]);
                let (hi_ad, lo_bc) = (a.max(d), b.min(c));
                if hi_ad < l && hi_ad < lo_bc {
                    k[ib] = hi_ad;
                    changed = true;
                }
                let (hi_bc, lo_ad) = (b.max(c), a.min(d));
                if hi_bc < l && hi_bc < lo_ad {
                    k[ia] = hi_bc;
                    changed = true;
                }
            }
        }
        if !changed {
            return;
        }
    }
}

/// Two plate pixels touching only at a corner would give a non-manifold edge. Fill one hole pixel.
pub(crate) fn fix_diagonals(g: &mut Grid) {
    loop {
        let mut changed = false;
        for y in 0..g.h.saturating_sub(1) {
            for x in 0..g.w.saturating_sub(1) {
                let (a, b, c, d) = (
                    g.m[y * g.w + x],
                    g.m[y * g.w + x + 1],
                    g.m[(y + 1) * g.w + x],
                    g.m[(y + 1) * g.w + x + 1],
                );
                if a && d && !b && !c {
                    g.m[y * g.w + x + 1] = true;
                    changed = true;
                } else if b && c && !a && !d {
                    g.m[y * g.w + x] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            return;
        }
    }
}

/// Connect every island to the frame by the cheapest path (hole pixels cost 1, plate pixels 0).
/// `rise` gets, on every new bridge pixel, a value in (0,1): 0 at the island, 1 at the frame.
pub(crate) fn bridge_islands(g: &mut Grid, radius: i64, rise: &mut [f32]) -> usize {
    let (w, h) = (g.w, g.h);
    let (islands, label) = label_islands(g);
    if islands == 0 {
        return 0;
    }
    let frame = label[0];
    let mut cost = vec![u32::MAX; w * h];
    let mut parent = vec![u32::MAX; w * h];
    let mut dq = VecDeque::new();
    for i in 0..w * h {
        if label[i] == frame {
            cost[i] = 0;
            dq.push_back(i);
        }
    }
    while let Some(i) = dq.pop_front() {
        let (x, y) = ((i % w) as i64, (i / w) as i64);
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                continue;
            }
            let j = ny as usize * w + nx as usize;
            let step = if g.m[j] { 0 } else { 1 };
            if cost[i] + step < cost[j] {
                cost[j] = cost[i] + step;
                parent[j] = i as u32;
                if step == 0 {
                    dq.push_front(j)
                } else {
                    dq.push_back(j)
                }
            }
        }
    }
    let mut best: HashMap<u32, usize> = HashMap::new();
    for i in 0..w * h {
        if g.m[i] && label[i] != frame && cost[i] != u32::MAX {
            let e = best.entry(label[i]).or_insert(i);
            if cost[i] < cost[*e] {
                *e = i;
            }
        }
    }
    let mut starts: Vec<usize> = best.values().copied().collect();
    starts.sort_unstable();
    let original = g.m.clone();
    let mut carved = 0;
    for start in starts {
        let mut path = Vec::new();
        let mut i = start;
        while parent[i] != u32::MAX {
            path.push(i);
            i = parent[i] as usize;
        }
        let holes: Vec<usize> = path.into_iter().filter(|&q| !original[q]).collect();
        let n = holes.len() as f32;
        for (j, &q) in holes.iter().enumerate() {
            let r = (j as f32 + 1.0) / (n + 1.0);
            let (x, y) = ((q % w) as i64, (q / w) as i64);
            for dy in 0..radius {
                for dx in 0..radius {
                    let (cx, cy) = (x + dx - radius / 2, y + dy - radius / 2);
                    if cx >= 0 && cy >= 0 && cx < w as i64 && cy < h as i64 {
                        let t = cy as usize * w + cx as usize;
                        if !original[t] {
                            if !g.m[t] {
                                g.m[t] = true;
                                carved += 1;
                            }
                            if rise[t].is_nan() || r < rise[t] {
                                rise[t] = r;
                            }
                        }
                    }
                }
            }
        }
    }
    carved
}
