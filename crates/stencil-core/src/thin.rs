//! Raster side of "minimum surface contact": strips of material thinner than the value are widened to it.

use crate::islands::{label_islands, Grid};

const FAR: f64 = 1e20;

/// Squared distance from every pixel to the nearest `true` pixel (exact; Felzenszwalb-Huttenlocher).
fn distance_sq(sites: &[bool], w: usize, h: usize) -> Vec<f64> {
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
    for i in 0..w * h {
        if reach[i] <= r2 {
            g.m[i] = true;
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
}
