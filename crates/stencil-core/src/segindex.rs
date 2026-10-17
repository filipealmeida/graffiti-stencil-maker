//! A uniform grid over line segments, to find the nearest point on a large set of outlines without scanning all of them.

use crate::trace::Pt;
use std::collections::HashMap;

pub(crate) struct SegIndex {
    segs: Vec<(Pt, Pt)>,
    cell: f64,
    cells: HashMap<(i32, i32), Vec<u32>>,
    /// Cell range that holds any segment: rings past it cannot find anything new.
    span: (i32, i32, i32, i32),
}

fn closest_on_segment(p: Pt, a: Pt, b: Pt) -> Pt {
    let d = [b[0] - a[0], b[1] - a[1]];
    let den = d[0] * d[0] + d[1] * d[1];
    let t = if den > 0.0 {
        (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / den).clamp(0.0, 1.0)
    } else {
        0.0
    };
    [a[0] + t * d[0], a[1] + t * d[1]]
}

fn dist_sq(a: Pt, b: Pt) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

impl SegIndex {
    /// Indexes every closed polygon edge. `cell` is the grid pitch in the same units as the points.
    pub(crate) fn new(polys: &[Vec<Pt>], cell: f64) -> SegIndex {
        let cell = cell.max(1e-6);
        let mut segs = Vec::new();
        for poly in polys {
            for i in 0..poly.len() {
                segs.push((poly[i], poly[(i + 1) % poly.len()]));
            }
        }
        let at = |v: f64| (v / cell).floor() as i32;
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        let mut span = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for (k, (a, b)) in segs.iter().enumerate() {
            let (x0, x1) = (at(a[0].min(b[0])), at(a[0].max(b[0])));
            let (y0, y1) = (at(a[1].min(b[1])), at(a[1].max(b[1])));
            span = (
                span.0.min(x0),
                span.1.min(y0),
                span.2.max(x1),
                span.3.max(y1),
            );
            for x in x0..=x1 {
                for y in y0..=y1 {
                    cells.entry((x, y)).or_default().push(k as u32);
                }
            }
        }
        SegIndex {
            segs,
            cell,
            cells,
            span,
        }
    }

    /// The closest point of any indexed segment to `q`, with its squared distance.
    pub(crate) fn nearest(&self, q: Pt) -> Option<(f64, Pt)> {
        if self.segs.is_empty() {
            return None;
        }
        let (cx, cy) = (
            (q[0] / self.cell).floor() as i32,
            (q[1] / self.cell).floor() as i32,
        );
        let reach = (cx - self.span.0)
            .abs()
            .max((cx - self.span.2).abs())
            .max((cy - self.span.1).abs())
            .max((cy - self.span.3).abs());
        let mut best: Option<(f64, Pt)> = None;
        for r in 0..=reach {
            let mut visit = |x: i32, y: i32| {
                for &k in self.cells.get(&(x, y)).into_iter().flatten() {
                    let (a, b) = self.segs[k as usize];
                    let c = closest_on_segment(q, a, b);
                    let d = dist_sq(q, c);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, c));
                    }
                }
            };
            if r == 0 {
                visit(cx, cy);
            } else {
                for x in cx - r..=cx + r {
                    visit(x, cy - r);
                    visit(x, cy + r);
                }
                for y in cy - r + 1..cy + r {
                    visit(cx - r, y);
                    visit(cx + r, y);
                }
            }
            // anything in a farther ring is at least r cells away
            if let Some((bd, _)) = best {
                if bd.sqrt() <= r as f64 * self.cell {
                    break;
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_matches_a_scan() {
        let polys = vec![
            vec![[0.0, 0.0], [50.0, 0.0], [50.0, 40.0], [0.0, 40.0]],
            vec![[10.0, 10.0], [14.0, 10.0], [12.0, 16.0]],
        ];
        let idx = SegIndex::new(&polys, 3.0);
        for (i, q) in [
            [12.0, 12.0],
            [-20.0, 7.0],
            [70.0, 60.0],
            [25.0, 20.0],
            [11.0, 9.0],
        ]
        .iter()
        .enumerate()
        {
            let brute = polys
                .iter()
                .flat_map(|p| (0..p.len()).map(move |k| (p[k], p[(k + 1) % p.len()])))
                .map(|(a, b)| dist_sq(*q, closest_on_segment(*q, a, b)))
                .fold(f64::INFINITY, f64::min);
            let (d, c) = idx.nearest(*q).unwrap();
            assert!((d - brute).abs() < 1e-9, "query {i}: {d} vs {brute}");
            assert!((dist_sq(*q, c) - d).abs() < 1e-9);
        }
    }
}
