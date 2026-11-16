//! Per-tile bridging: a tile cut from a bridged stencil can hold pieces that were only joined through another
//! tile. They are found as 3D components, rasterised on the plate's top layer and bridged to the tile's largest
//! piece with the same bridge rules as the stencil itself.
use crate::islands::{bridge_labelled, Grid};
use crate::mesh::{Mesh, P3};

mod geometry;

#[derive(Clone, Copy, Debug)]
pub struct BridgeParams {
    pub bridge_mm: f64,
    pub min_contact_mm: f64,
    pub per_island: usize,
    pub raised: bool,
    pub easy: bool,
    pub base_layers: u32,
    pub layer_height: f64,
    /// Total plate height of the stencil (top of the layered solid).
    pub plate_top: f64,
    pub flip: bool,
    /// Minimum angle between attachment positions about an island's area centre, in either spacing mode.
    pub min_angle_deg: f64,
    pub spread: bool,
    pub max_length: f64,
    pub passes: usize,
}

impl Default for BridgeParams {
    fn default() -> Self {
        BridgeParams {
            bridge_mm: 1.6,
            min_contact_mm: 0.0,
            per_island: 1,
            raised: true,
            easy: true,
            base_layers: 0,
            layer_height: 0.2,
            plate_top: 0.0,
            flip: false,
            min_angle_deg: 0.0,
            spread: false,
            max_length: 4.0,
            passes: 3,
        }
    }
}

/// Connected components over shared vertices: (count, component of every triangle).
pub fn components(m: &Mesh) -> (usize, Vec<usize>) {
    let mut parent: Vec<usize> = (0..m.v.len()).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for t in &m.t {
        let (a, b, c) = (
            find(&mut parent, t[0]),
            find(&mut parent, t[1]),
            find(&mut parent, t[2]),
        );
        parent[b] = a;
        let c = find(&mut parent, c);
        let a = find(&mut parent, a);
        parent[c] = a;
    }
    let mut ids = std::collections::HashMap::new();
    let mut of = Vec::with_capacity(m.t.len());
    for t in &m.t {
        let r = find(&mut parent, t[0]);
        let n = ids.len();
        of.push(*ids.entry(r).or_insert(n));
    }
    (ids.len(), of)
}

fn in_tri(p: [f64; 2], a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    let e = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| {
        (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    };
    let (d1, d2, d3) = (e(a, b, p), e(b, c, p), e(c, a, p));
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// A mesh that, unioned into `m`, joins its separate pieces; `None` when `m` is a single piece.
/// `avoid` boxes (connector pockets) remain open.
pub fn bridge_mesh(
    m: &Mesh,
    avoid: &[(P3, P3)],
    bp: &BridgeParams,
) -> Result<Option<Mesh>, String> {
    let (ncomp, comp) = components(m);
    if ncomp < 2 {
        return Ok(None);
    }
    let (lo, hi) = m.bounds();
    let cell = (bp.bridge_mm / 4.0)
        .max(0.2)
        .max((hi[0] - lo[0]).max(hi[1] - lo[1]) / 1500.0);
    let (w, h) = (
        (((hi[0] - lo[0]) / cell).ceil() as usize).max(1),
        (((hi[1] - lo[1]) / cell).ceil() as usize).max(1),
    );
    let plane = if bp.flip { 0.0 } else { bp.plate_top };
    let tol = 1e-3;
    let mut label = vec![0u32; w * h];
    let mut seen = vec![false; ncomp];
    let mut hit = vec![false; ncomp];
    let to_cell = |x: f64, y: f64| -> (i64, i64) {
        (
            ((x - lo[0]) / cell).floor() as i64,
            ((y - lo[1]) / cell).floor() as i64,
        )
    };
    for (ti, t) in m.t.iter().enumerate() {
        let [a, b, c] = [m.v[t[0]], m.v[t[1]], m.v[t[2]]];
        if ![a, b, c].iter().all(|p| (p[2] - plane).abs() < tol) {
            continue;
        }
        let nz = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if (nz > 0.0) == bp.flip {
            continue;
        }
        seen[comp[ti]] = true;
        let (x0, y0) = to_cell(a[0].min(b[0]).min(c[0]), a[1].min(b[1]).min(c[1]));
        let (x1, y1) = to_cell(a[0].max(b[0]).max(c[0]), a[1].max(b[1]).max(c[1]));
        for cy in y0.max(0)..=y1.min(h as i64 - 1) {
            for cx in x0.max(0)..=x1.min(w as i64 - 1) {
                let p = [
                    lo[0] + (cx as f64 + 0.5) * cell,
                    lo[1] + (cy as f64 + 0.5) * cell,
                ];
                if in_tri(p, [a[0], a[1]], [b[0], b[1]], [c[0], c[1]]) {
                    label[cy as usize * w + cx as usize] = comp[ti] as u32 + 1;
                    hit[comp[ti]] = true;
                }
            }
        }
        // thin parts can miss every cell centre: mark the centroid cell instead
        if !hit[comp[ti]] {
            let (cx, cy) = to_cell((a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0);
            if cx >= 0 && cy >= 0 && cx < w as i64 && cy < h as i64 {
                let i = cy as usize * w + cx as usize;
                if label[i] == 0 {
                    label[i] = comp[ti] as u32 + 1;
                }
            }
        }
    }
    let mut count = vec![0usize; ncomp];
    for &l in &label {
        if l > 0 {
            count[l as usize - 1] += 1;
        }
    }
    // pieces without a top face (or smaller than a cell) cannot be bridged here: the caller's final check reports them
    let Some(frame) = (0..ncomp)
        .max_by_key(|&c| count[c])
        .filter(|&c| count[c] > 0)
    else {
        return Ok(None);
    };
    for c in 0..ncomp {
        if !seen[c] || count[c] == 0 {
            label
                .iter_mut()
                .filter(|l| **l == c as u32 + 1)
                .for_each(|l| *l = 0);
        }
    }
    let mut grid = Grid {
        w,
        h,
        m: label.iter().map(|&l| l > 0).collect(),
    };
    let mut avoid_mask = vec![false; w * h];
    for (bl, bh) in avoid {
        let (x0, y0) = to_cell(bl[0], bl[1]);
        let (x1, y1) = to_cell(bh[0], bh[1]);
        for cy in y0.max(0)..=y1.min(h as i64 - 1) {
            for cx in x0.max(0)..=x1.min(w as i64 - 1) {
                avoid_mask[cy as usize * w + cx as usize] = true;
            }
        }
    }
    let radius = ((bp.bridge_mm / cell).round() as i64).max(1);
    let mut rise = vec![f32::NAN; w * h];
    let carved = bridge_labelled(
        &mut grid,
        &label,
        frame as u32 + 1,
        radius,
        &mut rise,
        bp.per_island.clamp(1, 5),
        bp.min_angle_deg,
        bp.max_length,
        Some(&avoid_mask),
        if bp.min_contact_mm > 0.0 {
            (bp.min_contact_mm / cell).ceil() as i64 + 4
        } else {
            0
        },
        bp.spread,
    );
    if carved.carved == 0 {
        return Ok(None);
    }

    let mut out = Mesh::default();
    for path in &carved.paths {
        let bridge = geometry::build(&path.cells, w, [lo[0], lo[1]], cell, bp, path.straight)?;
        out = out.union(&bridge)?;
    }
    for &(lo, hi) in avoid {
        out = out.subtract(&Mesh::cuboid(lo, hi))?;
    }
    Ok(Some(out))
}
