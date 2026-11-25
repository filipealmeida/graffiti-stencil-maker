//! Which parts of a mesh hold together on the bridging plane. Shared vertices are not enough: a strip thinner
//! than the join width, often a sliver left along a tile side or the plate edge, joins two parts that print as
//! separate pieces. A piece is a part of the plane outline that a disk of the join width can sweep without leaving
//! the material. Outside the outline is empty, so a tile side never holds a piece.
use crate::mesh::Mesh;
use crate::remesh::{grow, normalize, Region};
use i_triangle::i_overlay::core::fill_rule::FillRule;
use i_triangle::i_overlay::core::overlay_rule::OverlayRule;
use i_triangle::i_overlay::core::single::SingleIntOverlay;
use i_triangle::i_overlay::i_float::int::point::IntPoint;

const U: f64 = 10_000.0;
/// A component too thin for a disk of this diameter anywhere on the plane is a speck, not a part.
pub const SPECK_MM: f64 = 0.2;

pub struct Pieces {
    /// The 3D component of every triangle.
    pub comp: Vec<usize>,
    /// Components to delete: nothing of them is as wide as `SPECK_MM` on the plane.
    pub specks: Vec<bool>,
    /// The component of every piece, and the outline that defines it.
    of: Vec<usize>,
    shapes: Vec<Region>,
    areas: Vec<f64>,
}

/// The material left by an opening with a disk of radius `d` units.
fn opening(r: &Region, d: f64) -> Region {
    let Some(b) = bounds(r) else {
        return vec![];
    };
    let pad = (2.0 * d) as i32 + 10;
    let frame = vec![vec![
        IntPoint::new(b[0] - pad, b[1] - pad),
        IntPoint::new(b[2] + pad, b[1] - pad),
        IntPoint::new(b[2] + pad, b[3] + pad),
        IntPoint::new(b[0] - pad, b[3] + pad),
    ]];
    let outside =
        frame
            .as_slice()
            .overlay(r.as_slice(), OverlayRule::Difference, FillRule::EvenOdd);
    let eroded = frame.as_slice().overlay(
        grow(&outside, d).as_slice(),
        OverlayRule::Difference,
        FillRule::EvenOdd,
    );
    grow(&eroded, d)
}

fn bounds(r: &Region) -> Option<[i32; 4]> {
    let mut b = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    for p in r.iter().flatten().flatten() {
        b = [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)];
    }
    (b[0] <= b[2]).then_some(b)
}

fn area(r: &Region) -> f64 {
    r.iter()
        .flatten()
        .map(|c| {
            let n = c.len();
            (0..n)
                .map(|i| {
                    let (a, b) = (c[i], c[(i + 1) % n]);
                    a.x as f64 * b.y as f64 - b.x as f64 * a.y as f64
                })
                .sum::<f64>()
                / 2.0
        })
        .sum::<f64>()
        .abs()
        / (U * U)
}

impl Pieces {
    /// `join_mm`: the narrowest strip that still holds two parts together. A piece without a face on the plane
    /// has an empty outline: it counts, but nothing can be bridged to it.
    pub fn new(m: &Mesh, plane: f64, flip: bool, join_mm: f64) -> Pieces {
        let (n, comp) = super::components(m);
        let mut faces: Vec<Vec<Vec<IntPoint>>> = vec![vec![]; n];
        let mut tops: Vec<Vec<Vec<IntPoint>>> = vec![vec![]; n];
        for (t, &c) in m.t.iter().zip(&comp) {
            let [a, b, d] = t.map(|i| m.v[i]);
            let nz = (b[0] - a[0]) * (d[1] - a[1]) - (b[1] - a[1]) * (d[0] - a[0]);
            let mut tri: Vec<IntPoint> = [a, b, d]
                .iter()
                .map(|p| IntPoint::new((p[0] * U).round() as i32, (p[1] * U).round() as i32))
                .collect();
            // the faces looking up cover the solid's shadow on the plate
            if nz > 0.0 {
                tops[c].push(tri.clone());
            }
            if ![a, b, d].iter().all(|p| (p[2] - plane).abs() < 1e-3) || (nz > 0.0) == flip {
                continue;
            }
            if flip {
                tri.reverse();
            }
            faces[c].push(tri);
        }
        let mut pieces = Pieces {
            comp,
            specks: vec![false; n],
            of: vec![],
            shapes: vec![],
            areas: vec![],
        };
        let join = join_mm.max(SPECK_MM) * 0.5 * U;
        for (c, (faces, tops)) in faces.into_iter().zip(tops).enumerate() {
            let region = normalize(faces);
            let speck = SPECK_MM * 0.5 * U;
            if opening(&region, speck).is_empty() {
                // a solid that misses the plane is a piece no bridge can reach, not a speck, unless it is thin
                if opening(&normalize(tops), speck).is_empty() {
                    pieces.specks[c] = true;
                } else {
                    pieces.add(c, vec![]);
                }
                continue;
            }
            let cores = opening(&region, join);
            if cores.is_empty() {
                // narrower than the join width everywhere, yet a part of its own: it stays one piece
                pieces.add(c, region);
            }
            for core in cores {
                pieces.add(c, vec![core]);
            }
        }
        pieces
    }

    fn add(&mut self, c: usize, shape: Region) {
        self.areas.push(area(&shape));
        self.of.push(c);
        self.shapes.push(shape);
    }

    pub fn count(&self) -> usize {
        self.of.len()
    }

    pub fn dropped(&self) -> usize {
        self.specks.iter().filter(|&&s| s).count()
    }

    /// The biggest piece, which the others are joined to.
    pub fn frame(&self) -> Option<usize> {
        (0..self.count()).max_by(|&a, &b| self.areas[a].total_cmp(&self.areas[b]))
    }

    /// Pieces painted by cell centre, numbered from 1; 0 is empty or too thin to hold anything. Different pieces
    /// never share a cell side, so a path on the grid cannot pass from one to another without crossing a gap.
    pub fn raster(&self, lo: [f64; 2], cell: f64, w: usize, h: usize) -> Vec<u32> {
        let mut label = vec![0u32; w * h];
        for (p, shape) in self.shapes.iter().enumerate() {
            let mut rows: Vec<Vec<f64>> = vec![vec![]; h];
            for c in shape.iter().flatten() {
                for i in 0..c.len() {
                    let (a, b) = (c[i], c[(i + 1) % c.len()]);
                    let (ay, by) = (a.y as f64 / U, b.y as f64 / U);
                    let first = ((ay.min(by) - lo[1]) / cell - 0.5).ceil().max(0.0) as usize;
                    let last = ((ay.max(by) - lo[1]) / cell - 0.5).floor();
                    if last < 0.0 {
                        continue;
                    }
                    for row in first..=(last as usize).min(h - 1) {
                        let y = lo[1] + (row as f64 + 0.5) * cell;
                        if (ay > y) != (by > y) {
                            let (ax, bx) = (a.x as f64 / U, b.x as f64 / U);
                            rows[row].push(ax + (y - ay) / (by - ay) * (bx - ax));
                        }
                    }
                }
            }
            let mut painted = false;
            for (row, xs) in rows.iter_mut().enumerate() {
                xs.sort_by(f64::total_cmp);
                for span in xs.chunks_exact(2) {
                    let from = ((span[0] - lo[0]) / cell - 0.5).ceil().max(0.0) as usize;
                    let to = ((span[1] - lo[0]) / cell - 0.5).floor();
                    if to < 0.0 {
                        continue;
                    }
                    for x in from..=(to as usize).min(w - 1) {
                        label[row * w + x] = p as u32 + 1;
                        painted = true;
                    }
                }
            }
            // a piece narrower than a cell can miss every centre: it keeps the cell under one of its corners
            if !painted {
                if let Some(q) = shape.iter().flatten().flatten().next() {
                    let x = ((q.x as f64 / U - lo[0]) / cell).floor();
                    let y = ((q.y as f64 / U - lo[1]) / cell).floor();
                    if x >= 0.0 && y >= 0.0 && (x as usize) < w && (y as usize) < h {
                        let i = y as usize * w + x as usize;
                        if label[i] == 0 {
                            label[i] = p as u32 + 1;
                        }
                    }
                }
            }
        }
        let kept = label.clone();
        for i in 0..w * h {
            let l = kept[i];
            let (x, y) = (i % w, i / w);
            let next = |j: usize| kept[j] != 0 && kept[j] < l;
            if l != 0
                && ((x > 0 && next(i - 1))
                    || (x + 1 < w && next(i + 1))
                    || (y > 0 && next(i - w))
                    || (y + 1 < h && next(i + w)))
            {
                label[i] = 0;
            }
        }
        label
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_strip_thinner_than_the_join_does_not_hold_two_parts() {
        let joined = |strip: f64| {
            Mesh::cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 2.0])
                .union(&Mesh::cuboid([0.0, 10.0, 0.0], [strip, 20.0, 2.0]))
                .unwrap()
                .union(&Mesh::cuboid([0.0, 20.0, 0.0], [10.0, 30.0, 2.0]))
                .unwrap()
        };
        let mut m = joined(0.15);
        assert_eq!(super::super::components(&m).0, 1);
        assert_eq!(Pieces::new(&m, 2.0, false, 0.4).count(), 2);
        assert_eq!(Pieces::new(&m, 0.0, true, 0.4).count(), 2);
        assert_eq!(Pieces::new(&joined(0.6), 2.0, false, 0.4).count(), 1);
        m.append(&Mesh::cuboid([20.0, 0.0, 0.0], [20.1, 30.0, 2.0]));
        m.append(&Mesh::cuboid([30.0, 0.0, 1.0], [35.0, 5.0, 1.5]));
        let pieces = Pieces::new(&m, 2.0, false, 0.4);
        assert_eq!((pieces.count(), pieces.dropped()), (3, 1));
        let label = pieces.raster([0.0, 0.0], 0.5, 60, 60);
        assert!(label.contains(&1) && label.contains(&2));
    }

    #[test]
    fn touching_pieces_never_share_a_cell_side() {
        let m = Mesh::cuboid([0.0, 0.0, 0.0], [5.0, 5.0, 1.0])
            .union(&Mesh::cuboid([5.0, 0.0, 0.0], [10.0, 0.1, 1.0]))
            .unwrap();
        let mut m = m;
        m.append(&Mesh::cuboid([5.1, 0.0, 0.0], [10.0, 5.0, 1.0]));
        let pieces = Pieces::new(&m, 1.0, false, 0.4);
        assert_eq!(pieces.count(), 2);
        let (w, h) = (20, 10);
        let label = pieces.raster([0.0, 0.0], 0.5, w, h);
        for i in 0..w * h {
            if label[i] != 0 && i % w + 1 < w && label[i + 1] != 0 {
                assert_eq!(label[i], label[i + 1]);
            }
        }
    }
}
