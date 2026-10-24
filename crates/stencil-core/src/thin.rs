//! Raster side of "minimum surface contact": strips of material thinner than the value are widened to it.

use crate::islands::{label_islands, Grid};

const FAR: f64 = 1e20;

/// Squared distance from every pixel to the nearest `true` pixel (exact; Felzenszwalb-Huttenlocher).
fn distance_sq(sites: &[bool], w: usize, h: usize) -> Vec<f64> {
    if !sites.iter().any(|&site| site) {
        return vec![FAR; sites.len()];
    }
    let mut d: Vec<f64> = sites.iter().map(|&s| if s { 0.0 } else { FAR }).collect();
    let n = w.max(h);
    let (mut f, mut out) = (vec![0f64; n], vec![0f64; n]);
    let (mut v, mut z) = (vec![0usize; n], vec![0f64; n + 1]);
    let mut pass = |d: &mut Vec<f64>, len: usize, at: &dyn Fn(usize) -> usize| {
        for q in 0..len {
            f[q] = d[at(q)];
        }
        let cut = |f: &[f64], q: usize, p: usize| {
            ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * (q as f64 - p as f64))
        };
        let mut k = 0;
        v[0] = 0;
        z[0] = -FAR;
        z[1] = FAR;
        for q in 1..len {
            let mut s = cut(&f, q, v[k]);
            while s <= z[k] {
                k -= 1;
                s = cut(&f, q, v[k]);
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = FAR;
        }
        k = 0;
        #[allow(clippy::needless_range_loop)]
        for q in 0..len {
            while z[k + 1] < q as f64 {
                k += 1;
            }
            let dq = q as f64 - v[k] as f64;
            out[q] = dq * dq + f[v[k]];
        }
        for q in 0..len {
            d[at(q)] = out[q];
        }
    };
    for x in 0..w {
        pass(&mut d, h, &|y| y * w + x);
    }
    for y in 0..h {
        pass(&mut d, w, &|x| y * w + x);
    }
    d
}

/// For each extra footprint pixel, the exclusive layer at which its tapered flare ends.
pub(crate) fn first_layer_flare_ends(
    g: &Grid,
    s: f64,
    min_width_mm: f64,
    taper_layers: u32,
) -> Vec<u32> {
    let mut ends = vec![0; g.w * g.h];
    let target_radius = min_width_mm * 0.5 / s;
    if target_radius < 0.5 || taper_layers == 0 {
        return ends;
    }
    let hollow: Vec<_> = g.m.iter().map(|&m| !m).collect();
    if !hollow.iter().any(|&site| site) {
        return ends;
    }
    let away = distance_sq(&hollow, g.w, g.h);
    let eroded: Vec<_> = (0..g.w * g.h)
        .map(|i| g.m[i] && away[i] > target_radius * target_radius)
        .collect();
    let near = distance_sq(&eroded, g.w, g.h);
    let candidate = Grid {
        w: g.w,
        h: g.h,
        m: (0..g.w * g.h)
            .map(|i| g.m[i] && near[i] > target_radius * target_radius)
            .collect(),
    };
    if !candidate.m.iter().any(|&m| m) {
        return ends;
    }
    let (_, labels) = label_islands(&candidate);
    let count = labels.iter().copied().max().unwrap_or(0) as usize + 1;
    let mut component_radius = vec![0.0f64; count];
    for (i, &label) in labels.iter().enumerate() {
        if label != 0 {
            component_radius[label as usize] = component_radius[label as usize].max(away[i].sqrt());
        }
    }
    let base_radius = component_radius
        .into_iter()
        .skip(1)
        .filter(|&r| r < target_radius)
        .min_by(f64::total_cmp);
    let Some(base_radius) = base_radius else {
        return ends;
    };
    let grow_by = target_radius - base_radius;
    if grow_by < 0.5 {
        return ends;
    }
    let distance = distance_sq(&candidate.m, g.w, g.h);
    let max_d2 = grow_by * grow_by;
    for i in 0..ends.len() {
        if g.m[i] || distance[i] > max_d2 {
            continue;
        }
        let expanded_radius = base_radius + distance[i].sqrt();
        if expanded_radius >= target_radius {
            continue;
        }
        ends[i] = ((taper_layers as f64 * (1.0 - expanded_radius / target_radius)).ceil() as u32)
            .clamp(1, taper_layers);
    }
    let (_, openings) = label_islands(&Grid {
        w: g.w,
        h: g.h,
        m: hollow,
    });
    let opening_count = openings.iter().copied().max().unwrap_or(0) as usize + 1;
    let mut remaining = vec![false; opening_count];
    let mut farthest = vec![(0.0, None); opening_count];
    for (i, &label) in openings.iter().enumerate() {
        if label == 0 {
            continue;
        }
        let id = label as usize;
        if ends[i] == 0 {
            remaining[id] = true;
        }
        if distance[i] > farthest[id].0 {
            farthest[id] = (distance[i], Some(i));
        }
    }
    for id in 1..opening_count {
        if !remaining[id] {
            if let Some(i) = farthest[id].1 {
                ends[i] = 0;
            }
        }
    }
    ends
}

/// `s` is the pixel size in mm. Material is what is `true`; the outside of the grid counts as material.
pub(crate) fn widen_thin(g: &mut Grid, s: f64, min_mm: f64) {
    let r = min_mm * 0.5 / s;
    if r < 0.5 {
        return;
    }
    let r2 = r * r;
    let (w, h) = (g.w, g.h);
    let hollow: Vec<bool> = g.m.iter().map(|&m| !m).collect();
    let away = distance_sq(&hollow, w, h);
    let eroded: Vec<bool> = (0..w * h).map(|i| g.m[i] && away[i] > r2).collect();
    let near = distance_sq(&eroded, w, h);
    let min_px = (0.5 * (min_mm / s) * (min_mm / s)) as usize;
    let thin = Grid {
        w,
        h,
        m: (0..w * h).map(|i| g.m[i] && near[i] > r2).collect(),
    };
    let (_, label) = label_islands(&thin);
    let thick = Grid {
        w,
        h,
        m: (0..w * h).map(|i| near[i] <= r2).collect(),
    };
    let (_, parts) = label_islands(&thick);
    let top = label.iter().copied().max().unwrap_or(0) as usize + 1;
    let mut area = vec![0usize; top];
    // A rope joins two different thick parts: remember up to two distinct ones each thin piece touches.
    let mut touches = vec![[0u32; 2]; top];
    #[allow(clippy::needless_range_loop)]
    for i in 0..w * h {
        let l = label[i] as usize;
        if l == 0 {
            continue;
        }
        area[l] += 1;
        let (x, y) = (i % w, i / w);
        let around = [
            (x > 0).then(|| i - 1),
            (x + 1 < w).then(|| i + 1),
            (y > 0).then(|| i - w),
            (y + 1 < h).then(|| i + w),
        ];
        for j in around.into_iter().flatten() {
            let part = parts[j];
            if part != 0 && touches[l][0] != part && touches[l][1] != part {
                if touches[l][0] == 0 {
                    touches[l][0] = part;
                } else if touches[l][1] == 0 {
                    touches[l][1] = part;
                }
            }
        }
    }
    let big: Vec<bool> = label
        .iter()
        .map(|&l| l != 0 && area[l as usize] >= min_px && touches[l as usize][1] != 0)
        .collect();
    if !big.iter().any(|&b| b) {
        return;
    }
    let reach = distance_sq(&big, w, h);
    for (m, &r) in g.m.iter_mut().zip(&reach) {
        if r <= r2 {
            *m = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances_are_exact() {
        let mut sites = vec![false; 7 * 5];
        sites[2 * 7 + 3] = true;
        let d = distance_sq(&sites, 7, 5);
        assert_eq!(d[2 * 7 + 3], 0.0);
        assert_eq!(d[2 * 7 + 6], 9.0);
        assert_eq!(d[4 * 7], 13.0);
    }

    #[test]
    fn a_first_layer_flare_does_not_close_an_existing_opening() {
        let (w, h) = (25, 25);
        let mut material = vec![false; w * h];
        for y in 8..17 {
            for x in 8..17 {
                if x == 8 || x == 16 || y == 8 || y == 16 {
                    material[y * w + x] = true;
                }
            }
        }
        let ends = first_layer_flare_ends(
            &Grid {
                w,
                h,
                m: material.clone(),
            },
            1.0,
            5.0,
            4,
        );
        assert!(ends.iter().any(|&end| end > 0));
        assert!((9..16).any(|y| (9..16).any(|x| !material[y * w + x] && ends[y * w + x] == 0)));
    }
}
