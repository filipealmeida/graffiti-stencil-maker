//! Island analysis on the material grid: labelling, distance transform, diagonal repair and bridging.

use std::collections::HashMap;

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
        #[allow(clippy::needless_range_loop)]
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
        let d = pass(&g[y * w..(y + 1) * w], w);
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
pub(crate) fn bridge_islands(
    g: &mut Grid,
    radius: i64,
    rise: &mut [f32],
    per_island: usize,
    min_angle: f64,
    max_length: f64,
    largest_frame: bool,
) -> usize {
    let (islands, label) = label_islands(g);
    if islands == 0 {
        return 0;
    }
    let frame = if largest_frame {
        let mut size = vec![0usize; islands + 2];
        for &l in &label {
            size[l as usize] += 1;
        }
        (1..size.len()).max_by_key(|&l| size[l]).unwrap_or(1) as u32
    } else {
        label[0]
    };
    bridge_labelled(
        g, &label, frame, radius, rise, per_island, min_angle, max_length, None,
    )
}

#[allow(clippy::too_many_arguments)]
/// Like `bridge_islands`, for pieces already labelled by the caller (0 = empty) and a chosen frame piece. Pixels in
/// `avoid` are crossed only when nothing else connects a piece.
pub(crate) fn bridge_labelled(
    g: &mut Grid,
    label: &[u32],
    frame: u32,
    radius: i64,
    rise: &mut [f32],
    per_island: usize,
    min_angle: f64,
    max_length: f64,
    avoid: Option<&[bool]>,
) -> usize {
    let (w, h) = (g.w, g.h);
    let mut cost = vec![u32::MAX; w * h];
    let mut parent = vec![u32::MAX; w * h];
    let mut dq = std::collections::BinaryHeap::new();
    for i in 0..w * h {
        if label[i] == frame {
            cost[i] = 0;
            dq.push(std::cmp::Reverse((0u32, i)));
        }
    }
    while let Some(std::cmp::Reverse((c, i))) = dq.pop() {
        if c > cost[i] {
            continue;
        }
        let (x, y) = ((i % w) as i64, (i / w) as i64);
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                continue;
            }
            let j = ny as usize * w + nx as usize;
            let step = if g.m[j] {
                0
            } else if avoid.is_some_and(|a| a[j]) {
                40
            } else {
                1
            };
            if cost[i] + step < cost[j] {
                cost[j] = cost[i] + step;
                parent[j] = i as u32;
                dq.push(std::cmp::Reverse((cost[j], j)));
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
    let mut primary: Vec<(u32, Vec<usize>)> = Vec::new();
    for start in starts {
        let mut path = Vec::new();
        let mut i = start;
        while parent[i] != u32::MAX {
            path.push(i);
            i = parent[i] as usize;
        }
        let holes: Vec<usize> = path.into_iter().filter(|&q| !original[q]).collect();
        carved += carve(g, &original, rise, &holes, radius).len();
        primary.push((label[start], holes));
    }
    if per_island > 1 {
        carved += extra_bridges(
            g, &original, label, frame, radius, rise, &primary, per_island, min_angle, max_length,
            avoid,
        );
    }
    carved
}

/// Paint one bridge along `holes` (ordered from the island to the frame); returns the pixels that became plate.
fn carve(
    g: &mut Grid,
    original: &[bool],
    rise: &mut [f32],
    holes: &[usize],
    radius: i64,
) -> Vec<usize> {
    let (w, h) = (g.w, g.h);
    let mut added = Vec::new();
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
                            added.push(t);
                        }
                        if rise[t].is_nan() || r < rise[t] {
                            rise[t] = r;
                        }
                    }
                }
            }
        }
    }
    added
}

/// Square neighbourhood of half-width `r` around every pixel in `px`, inserted into `set`.
fn mark_near(set: &mut [bool], px: &[usize], r: i64, w: usize, h: usize) {
    for &q in px {
        let (x, y) = ((q % w) as i64, (q / w) as i64);
        for cy in (y - r).max(0)..=(y + r).min(h as i64 - 1) {
            for cx in (x - r).max(0)..=(x + r).min(w as i64 - 1) {
                set[cy as usize * w + cx as usize] = true;
            }
        }
    }
}

/// The unit direction from the first to the last pixel of a path; a path that goes nowhere conflicts with nothing.
fn chord(path: &[usize], w: usize) -> [f64; 2] {
    let (Some(&a), Some(&b)) = (path.first(), path.last()) else {
        return [0.0, 0.0];
    };
    let (dx, dy) = (
        (b % w) as f64 - (a % w) as f64,
        (b / w) as f64 - (a / w) as f64,
    );
    let n = dx.hypot(dy);
    if n < 1e-9 {
        [0.0, 0.0]
    } else {
        [dx / n, dy / n]
    }
}

/// Up to `per_island - 1` more bridges for every island: each is the shortest hole path to the frame that stays
/// clear of every other bridge and leaves the island away from its own bridges, so they run in different directions.
#[allow(clippy::too_many_arguments)]
fn extra_bridges(
    g: &mut Grid,
    original: &[bool],
    label: &[u32],
    frame: u32,
    radius: i64,
    rise: &mut [f32],
    primary: &[(u32, Vec<usize>)],
    per_island: usize,
    min_angle: f64,
    max_length: f64,
    avoid: Option<&[bool]>,
) -> usize {
    let (w, h) = (g.w, g.h);
    let mut pixels: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, &l) in label.iter().enumerate() {
        if g.m[i] && original[i] && l != frame && l != 0 {
            pixels.entry(l).or_default().push(i);
        }
    }
    let mut blocked = avoid.map_or_else(|| vec![false; w * h], <[bool]>::to_vec);
    let all: Vec<usize> = (0..w * h).filter(|&i| g.m[i] && !original[i]).collect();
    mark_near(&mut blocked, &all, radius + 1, w, h);
    let mut stamp = vec![0u32; w * h];
    let mut parent = vec![u32::MAX; w * h];
    let mut gen = 0u32;
    let mut carved = 0;
    for (l, holes) in primary {
        let Some(src) = pixels.get(l) else { continue };
        let spread = min_angle > 0.0;
        let factor = if spread { max_length } else { 4.0 };
        let limit =
            (factor * holes.len().max(radius as usize) as f64) as usize + 4 * radius as usize;
        let mut own = vec![false; w * h];
        mark_near(&mut own, holes, 2 * radius + 2, w, h);
        let mut dirs = vec![chord(holes, w)];
        // the wanted angle is relaxed in steps, so an island never gets fewer bridges than without it
        let tiers: Vec<f64> = if spread {
            vec![min_angle, min_angle * 2.0 / 3.0, min_angle / 3.0, 0.0]
        } else {
            vec![0.0]
        };
        'island: for _ in 1..per_island {
            let mut path = Vec::new();
            for &tier in &tiers {
                let widest = tier.to_radians().cos();
                gen += 1;
                let mut frontier: Vec<usize> = src.clone();
                for &i in &frontier {
                    stamp[i] = gen;
                    parent[i] = u32::MAX;
                }
                let mut hit = None;
                let mut layer = 0;
                'bfs: while !frontier.is_empty() && layer < limit {
                    layer += 1;
                    let mut next = Vec::new();
                    for &i in &frontier {
                        let (x, y) = ((i % w) as i64, (i / w) as i64);
                        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                            let (nx, ny) = (x + dx, y + dy);
                            if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                                continue;
                            }
                            let j = ny as usize * w + nx as usize;
                            if stamp[j] == gen {
                                continue;
                            }
                            if original[j] {
                                if label[j] == frame && parent[i] != u32::MAX {
                                    let mut candidate = Vec::new();
                                    let mut k = i;
                                    while parent[k] != u32::MAX {
                                        candidate.push(k);
                                        k = parent[k] as usize;
                                    }
                                    candidate.reverse();
                                    let d = chord(&candidate, w);
                                    if tier <= 0.0
                                        || dirs
                                            .iter()
                                            .all(|e| (d[0] * e[0] + d[1] * e[1]).abs() <= widest)
                                    {
                                        hit = Some(candidate);
                                        break 'bfs;
                                    }
                                }
                            } else if !g.m[j] && !blocked[j] && !own[j] {
                                stamp[j] = gen;
                                parent[j] = i as u32;
                                next.push(j);
                            }
                        }
                    }
                    frontier = next;
                }
                if let Some(found) = hit {
                    path = found;
                    break;
                }
            }
            if path.is_empty() {
                break 'island;
            }
            let added = carve(g, original, rise, &path, radius);
            mark_near(&mut blocked, &added, radius + 1, w, h);
            mark_near(&mut own, &path, 2 * radius + 2, w, h);
            dirs.push(chord(&path, w));
            carved += added.len();
        }
    }
    carved
}
