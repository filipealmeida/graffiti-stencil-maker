//! Per-tile bridging: a tile cut from a bridged stencil can hold pieces that were only joined through another
//! tile. They are found as 3D components, rasterised on the plate's top layer and bridged to the tile's largest
//! piece with the same bridge rules as the stencil itself.
use crate::islands::{bridge_labelled, Grid};
use crate::mesh::{Mesh, P3};

mod geometry;
mod pieces;
pub use pieces::{Pieces, SPECK_MM};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BridgeSpacing {
    #[default]
    Closest,
    Spread,
    Aligned,
}

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
    /// Minimum attachment-position angle, or undirected line-axis angle for Aligned.
    pub min_angle_deg: f64,
    pub spacing: BridgeSpacing,
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
            spacing: BridgeSpacing::Closest,
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

/// The narrowest strip that holds two parts together: the contact target, with a floor for projects without one.
/// A bridge is only widened to the target at its ends, so the join never exceeds most of the bridge's own width.
pub fn join_mm(bp: &BridgeParams) -> f64 {
    bp.min_contact_mm
        .min(bp.bridge_mm * 0.8)
        .max(pieces::SPECK_MM)
}

/// A mesh that, unioned into `m`, joins its separate pieces; `None` when `m` is a single piece.
/// `avoid` boxes (connector pockets) remain open.
pub fn bridge_mesh(
    m: &Mesh,
    avoid: &[(P3, P3)],
    bp: &BridgeParams,
) -> Result<Option<Mesh>, String> {
    let plane = if bp.flip { 0.0 } else { bp.plate_top };
    let pieces = Pieces::new(m, plane, bp.flip, join_mm(bp));
    let (lo, hi) = m.bounds();
    bridge_pieces(m, &pieces, avoid, [[lo[0], lo[1]], [hi[0], hi[1]]], bp)
}

/// `bridge_mesh` for pieces already found in `m`. `outline` is the tile's rectangle on the bridging plane: its
/// sides are cut edges, not material, so no bridge may run along one.
pub fn bridge_pieces(
    m: &Mesh,
    pieces: &Pieces,
    avoid: &[(P3, P3)],
    outline: [[f64; 2]; 2],
    bp: &BridgeParams,
) -> Result<Option<Mesh>, String> {
    if pieces.count() < 2 {
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
    let label = pieces.raster([lo[0], lo[1]], cell, w, h);
    let mut count = vec![0usize; pieces.count()];
    for &l in &label {
        if l > 0 {
            count[l as usize - 1] += 1;
        }
    }
    // pieces smaller than a cell cannot be bridged here: the caller's final check reports them
    let Some(frame) = pieces.frame().filter(|&f| count[f] > 0) else {
        return Ok(None);
    };
    let to_cell = |x: f64, y: f64| -> (i64, i64) {
        (
            ((x - lo[0]) / cell).floor() as i64,
            ((y - lo[1]) / cell).floor() as i64,
        )
    };
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
    let [fl, fh] = outline;
    let edge: Vec<[f32; 2]> = (0..w * h)
        .map(|i| {
            let x = lo[0] + ((i % w) as f64 + 0.5) * cell;
            let y = lo[1] + ((i / w) as f64 + 0.5) * cell;
            [
                ((x - fl[0]).min(fh[0] - x) / cell) as f32,
                ((y - fl[1]).min(fh[1] - y) / cell) as f32,
            ]
        })
        .collect();
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
        Some(&edge),
        if bp.min_contact_mm > 0.0 {
            (bp.min_contact_mm / cell).ceil() as i64 + 4
        } else {
            0
        },
        bp.spacing,
    );
    if carved.carved == 0 {
        return Ok(None);
    }

    let mut out = Mesh::default();
    for path in &carved.paths {
        let mut bridge = if let Some(axis) = path.axis {
            geometry::build_axis(axis, [lo[0], lo[1]], cell, bp)?
        } else {
            geometry::build(&path.cells, w, [lo[0], lo[1]], cell, bp, path.straight)?
        };
        // Bridges planned in adjacent grid corridors can touch exactly edge to edge; such a union is joined
        // along a single edge, which the boolean engine rejects on the next operation. A nudge far below
        // print resolution (but above f32 rounding) turns the exact contact into an overlap or a clearance.
        let mut merged = out.union(&bridge)?;
        let mut nudge = 1e-3;
        while !merged.is_edge_manifold() {
            if nudge > 0.1 {
                return Err("bridges could not be joined into a manifold solid".into());
            }
            bridge.translate([nudge, nudge, 0.0]);
            merged = out.union(&bridge)?;
            nudge *= 2.0;
        }
        out = merged;
    }
    for &(lo, hi) in avoid {
        out = out.subtract(&Mesh::cuboid(lo, hi))?;
    }
    Ok(Some(out))
}
