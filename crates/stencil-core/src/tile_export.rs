//! The tile plan as drawings: every tile's cut outlines as SVG and DXF (in millimetres, at the tile's own origin),
//! and, for a plan made from outlines, the plan picture itself as a vector drawing.
use crate::dxf;
use crate::tile_first::{Layout, Source};
use crate::trace::Pt;
use serde_json::{json, Value as Json};

/// A tile of the plan in plate millimetres (y up).
pub struct TileRect {
    pub name: String,
    pub row: usize,
    pub col: usize,
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// Where the point `p` of the rectangle's border is, counted counter-clockwise from its lower left corner.
fn along(r: [f64; 4], p: Pt) -> f64 {
    let (w, h) = (r[2] - r[0], r[3] - r[1]);
    let eps = 1e-7 * (w + h);
    if (p[1] - r[1]).abs() <= eps {
        p[0] - r[0]
    } else if (p[0] - r[2]).abs() <= eps {
        w + (p[1] - r[1])
    } else if (p[1] - r[3]).abs() <= eps {
        w + h + (r[2] - p[0])
    } else {
        2.0 * w + h + (r[3] - p[1])
    }
}

/// The part of the segment `a`-`b` inside the rectangle as parameters `t0 <= t1`, if any (Liang-Barsky).
fn clip_segment(a: Pt, b: Pt, r: [f64; 4]) -> Option<(f64, f64)> {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let d = [b[0] - a[0], b[1] - a[1]];
    for (p, q) in [
        (-d[0], a[0] - r[0]),
        (d[0], r[2] - a[0]),
        (-d[1], a[1] - r[1]),
        (d[1], r[3] - a[1]),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t0 <= t1).then_some((t0, t1))
}

fn inside(p: Pt, r: [f64; 4]) -> bool {
    p[0] >= r[0] && p[0] <= r[2] && p[1] >= r[1] && p[1] <= r[3]
}

fn signed_area(poly: &[Pt]) -> f64 {
    (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        / 2.0
}

fn contains(poly: &[Pt], p: Pt) -> bool {
    let mut c = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
        {
            c = !c;
        }
    }
    c
}

/// A closed polygon cut to the rectangle `r` (`[x0, y0, x1, y1]`): the pieces of the polygon inside it, joined along
/// the rectangle's border where the polygon leaves and comes back, so every result is a closed outline.
pub fn clip_polygon(poly: &[Pt], r: [f64; 4]) -> Vec<Vec<Pt>> {
    if poly.len() < 3 {
        return vec![];
    }
    let mut poly = poly.to_vec();
    if signed_area(&poly) < 0.0 {
        poly.reverse();
    }
    let Some(start) = poly.iter().position(|&p| !inside(p, r)) else {
        return vec![poly];
    };
    poly.rotate_left(start);
    let n = poly.len();
    let mut runs: Vec<Vec<Pt>> = Vec::new();
    let mut cur: Option<Vec<Pt>> = None;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let Some((t0, t1)) = clip_segment(a, b, r) else {
            continue;
        };
        let at = |t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        if t0 > 0.0 || cur.is_none() {
            if let Some(run) = cur.take() {
                runs.push(run);
            }
            cur = Some(vec![at(t0)]);
        }
        let run = cur.as_mut().unwrap();
        let end = at(t1);
        if run.last() != Some(&end) {
            run.push(end);
        }
        if t1 < 1.0 {
            runs.push(cur.take().unwrap());
        }
    }
    if let Some(run) = cur.take() {
        runs.push(run);
    }
    runs.retain(|run| run.len() >= 2);
    if runs.is_empty() {
        // no edge crosses the rectangle: it lies inside the polygon or apart from it
        let centre = [(r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0];
        return if contains(&poly, centre) {
            vec![vec![[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]]]
        } else {
            vec![]
        };
    }
    let (w, h) = (r[2] - r[0], r[3] - r[1]);
    let perimeter = 2.0 * (w + h);
    let corners = [
        (w, [r[2], r[1]]),
        (w + h, [r[2], r[3]]),
        (2.0 * w + h, [r[0], r[3]]),
        (perimeter, [r[0], r[1]]),
    ];
    let entry: Vec<f64> = runs.iter().map(|run| along(r, run[0])).collect();
    let mut used = vec![false; runs.len()];
    let mut out = Vec::new();
    for first in 0..runs.len() {
        if used[first] {
            continue;
        }
        let mut ring: Vec<Pt> = Vec::new();
        let mut k = first;
        loop {
            used[k] = true;
            ring.extend_from_slice(&runs[k]);
            let exit = along(r, *runs[k].last().unwrap());
            // the next entry counter-clockwise from the exit; the rectangle's corners in between belong to the outline
            let next = (0..runs.len())
                .map(|j| (j, (entry[j] - exit).rem_euclid(perimeter)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|x| x.0)
                .unwrap();
            let span = (entry[next] - exit).rem_euclid(perimeter);
            for (s, c) in corners {
                if (s - exit).rem_euclid(perimeter) < span
                    && (s - exit).rem_euclid(perimeter) > 1e-9
                {
                    ring.push(c);
                }
            }
            if next == first || used[next] {
                break;
            }
            k = next;
        }
        out.push(ring);
    }
    out
}

fn rect_poly(r: [f64; 4]) -> Vec<Pt> {
    vec![[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]]
}

fn svg_path(polys: &[Vec<Pt>], map: impl Fn(Pt) -> Pt) -> String {
    let mut d = String::new();
    for poly in polys {
        for (i, p) in poly.iter().enumerate() {
            let q = map(*p);
            d += &format!("{}{:.2} {:.2}", if i == 0 { "M" } else { "L" }, q[0], q[1]);
        }
        d += "Z";
    }
    d
}

impl Layout {
    /// The artwork as closed polygons in plate millimetres, y up.
    fn art(&self) -> &Vec<Vec<Pt>> {
        self.art.get_or_init(|| match &self.source {
            Source::Vector(shapes) => {
                let s = (self.width - 2.0 * self.margin) / shapes.w as f64;
                let top = self.margin + shapes.h as f64 * s;
                shapes
                    .flatten(0.02 / s)
                    .into_iter()
                    .map(|p| {
                        p.into_iter()
                            .map(|q| [self.margin + q[0] * s, top - q[1] * s])
                            .collect()
                    })
                    .collect()
            }
            Source::Raster { w, h, cell, paint } => {
                let t = crate::trace::trace(
                    paint,
                    *w,
                    *h,
                    &crate::trace::TraceParams {
                        smooth_px: 0.0,
                        corner_deg: 30.0,
                        tolerance_px: 0.05,
                        min_area_px: 0.0,
                    },
                );
                let top = *h as f64 * cell;
                t.flatten(0.05)
                    .into_iter()
                    .map(|p| {
                        p.into_iter()
                            .map(|q| [q[0] * cell, top - q[1] * cell])
                            .collect()
                    })
                    .collect()
            }
        })
    }

    pub fn tile_rects(&self) -> Vec<TileRect> {
        let (nx, ny) = (self.xs.len() - 1, self.ys.len() - 1);
        let mut v = Vec::new();
        for j in (0..ny).rev() {
            for i in 0..nx {
                v.push(TileRect {
                    name: format!("stencil_r{}c{}", ny - j, i + 1),
                    row: ny - j,
                    col: i + 1,
                    x0: self.xs[i],
                    y0: self.ys[j],
                    x1: self.xs[i + 1],
                    y1: self.ys[j + 1],
                });
            }
        }
        v
    }

    /// The tile's cut outlines (the paint inside it) and the connector blocks that reach into it, in plate millimetres.
    fn tile_drawing(&self, t: &TileRect) -> (Vec<Vec<Pt>>, Vec<Vec<Pt>>) {
        let r = [t.x0, t.y0, t.x1, t.y1];
        let cut = self.art().iter().flat_map(|p| clip_polygon(p, r)).collect();
        let blocks = self
            .pockets
            .iter()
            .map(|p| p.block(&self.spec))
            .filter_map(|b| {
                let c = [
                    b[0].max(r[0]),
                    b[1].max(r[1]),
                    b[2].min(r[2]),
                    b[3].min(r[3]),
                ];
                (c[0] < c[2] && c[1] < c[3]).then(|| rect_poly(c))
            })
            .collect();
        (cut, blocks)
    }

    /// `name.svg` or `name.dxf` of one tile: millimetres, origin at the tile's lower left corner for the DXF
    /// (y up) and at its upper left for the SVG. Layers: CUT (the paint), TILE (the border), BLOCKS (connector blocks).
    pub fn tile_file(&self, file: &str) -> Option<Vec<u8>> {
        let (stem, ext) = file.rsplit_once('.')?;
        let t = self.tile_rects().into_iter().find(|t| t.name == stem)?;
        let (w, h) = (t.x1 - t.x0, t.y1 - t.y0);
        let (cut, blocks) = self.tile_drawing(&t);
        let local = |p: Pt| [p[0] - t.x0, p[1] - t.y0];
        match ext {
            "dxf" => {
                let shift = |v: &[Vec<Pt>]| -> Vec<Vec<Pt>> {
                    v.iter()
                        .map(|p| p.iter().map(|&q| local(q)).collect())
                        .collect()
                };
                let border = [rect_poly([0.0, 0.0, w, h])];
                Some(
                    dxf::write(&[
                        dxf::Layer {
                            name: "CUT",
                            colour: 7,
                            polys: &shift(&cut),
                        },
                        dxf::Layer {
                            name: "TILE",
                            colour: 1,
                            polys: &border,
                        },
                        dxf::Layer {
                            name: "BLOCKS",
                            colour: 5,
                            polys: &shift(&blocks),
                        },
                    ])
                    .into_bytes(),
                )
            }
            "svg" => {
                let flip = |p: Pt| [p[0] - t.x0, t.y1 - p[1]];
                Some(
                    format!(
                        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w:.2} {h:.2}\" width=\"{w:.2}mm\" height=\"{h:.2}mm\">\
                         <rect width=\"{w:.2}\" height=\"{h:.2}\" fill=\"#fff\"/>\
                         <path id=\"CUT\" d=\"{cut}\" fill=\"#000\" fill-rule=\"evenodd\"/>\
                         <path id=\"BLOCKS\" d=\"{blocks}\" fill=\"none\" stroke=\"#2a6bd6\" stroke-width=\"0.3\"/>\
                         <rect id=\"TILE\" width=\"{w:.2}\" height=\"{h:.2}\" fill=\"none\" stroke=\"#dc2828\" stroke-width=\"0.3\"/></svg>",
                        cut = svg_path(&cut, flip),
                        blocks = svg_path(&blocks, flip),
                    )
                    .into_bytes(),
                )
            }
            _ => None,
        }
    }

    /// Size and position of every tile, and the names of its SVG and DXF files.
    pub fn tile_list(&self) -> Vec<Json> {
        let r2 = |v: f64| (v * 100.0).round() / 100.0;
        self.tile_rects()
            .iter()
            .map(|t| {
                json!({"name": t.name, "row": t.row, "col": t.col, "x": r2(t.x0), "y": r2(t.y0),
                    "w": r2(t.x1 - t.x0), "h": r2(t.y1 - t.y0)})
            })
            .collect()
    }

    /// The plan picture as a vector drawing: paint dark, blocks blue (orange where they cover paint), seams red.
    /// Only for a plan made from outlines; a plan made from a mask is a raster and has `preview`.
    pub fn preview_svg(&self) -> Option<String> {
        if !matches!(self.source, Source::Vector(_)) {
            return None;
        }
        let k = 1000.0 / self.width.max(self.height);
        let flip = |p: Pt| [p[0], self.height - p[1]];
        let paint = svg_path(self.art(), flip);
        let blocks: Vec<Vec<Pt>> = self
            .pockets
            .iter()
            .map(|p| rect_poly(p.block(&self.spec)))
            .collect();
        let blocks = svg_path(&blocks, flip);
        let sw = 1.5 / k;
        let mut seams = String::new();
        for &x in &self.xs[1..self.xs.len() - 1] {
            seams += &format!("M{x:.2} 0V{:.2}", self.height);
        }
        for &y in &self.ys[1..self.ys.len() - 1] {
            seams += &format!("M0 {:.2}H{:.2}", self.height - y, self.width);
        }
        Some(format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w:.2} {h:.2}\" width=\"{pw}\" height=\"{ph}\">\
             <rect width=\"{w:.2}\" height=\"{h:.2}\" fill=\"#e1e1e1\"/>\
             <path d=\"{paint}\" fill=\"#282828\" fill-rule=\"evenodd\"/>\
             <path d=\"{blocks}\" fill=\"#4678e6\"/>\
             <clipPath id=\"paint\"><path d=\"{paint}\" clip-rule=\"evenodd\"/></clipPath>\
             <path d=\"{blocks}\" fill=\"#f09628\" clip-path=\"url(#paint)\"/>\
             <path d=\"{seams}\" stroke=\"#dc2828\" stroke-width=\"{sw:.3}\" fill=\"none\"/></svg>",
            w = self.width,
            h = self.height,
            pw = (self.width * k).ceil(),
            ph = (self.height * k).ceil(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(v: &[Vec<Pt>]) -> f64 {
        v.iter().map(|p| signed_area(p).abs()).sum()
    }

    #[test]
    fn a_ring_cut_by_a_window_stays_closed_and_keeps_its_area() {
        // a 10x10 square with a 4x4 hole, cut at x = 5
        let outer = rect_poly([0.0, 0.0, 10.0, 10.0]);
        let hole = rect_poly([3.0, 3.0, 7.0, 7.0]);
        let r = [0.0, 0.0, 5.0, 10.0];
        let (a, b) = (clip_polygon(&outer, r), clip_polygon(&hole, r));
        assert_eq!((a.len(), b.len()), (1, 1));
        assert!((area(&a) - 50.0).abs() < 1e-9 && (area(&b) - 8.0).abs() < 1e-9);
    }

    #[test]
    fn a_polygon_leaving_and_returning_is_joined_along_the_border() {
        // a U shape: the window cuts both arms off the base, leaving two pieces
        let u = vec![
            [0.0, 0.0],
            [9.0, 0.0],
            [9.0, 9.0],
            [7.0, 9.0],
            [7.0, 2.0],
            [2.0, 2.0],
            [2.0, 9.0],
            [0.0, 9.0],
        ];
        let whole = signed_area(&u).abs();
        let left = clip_polygon(&u, [0.0, 0.0, 5.0, 9.0]);
        let right = clip_polygon(&u, [5.0, 0.0, 9.0, 9.0]);
        assert!((area(&left) + area(&right) - whole).abs() < 1e-9);
        let top = clip_polygon(&u, [0.0, 5.0, 9.0, 9.0]);
        assert_eq!(top.len(), 2, "two arms");
        assert!((area(&top) - (2.0 * 4.0 + 2.0 * 4.0)).abs() < 1e-9);
    }

    #[test]
    fn a_window_inside_or_apart_from_a_polygon() {
        let big = rect_poly([0.0, 0.0, 10.0, 10.0]);
        let inside_it = clip_polygon(&big, [2.0, 2.0, 4.0, 4.0]);
        assert!((area(&inside_it) - 4.0).abs() < 1e-9);
        assert!(clip_polygon(&big, [20.0, 20.0, 24.0, 24.0]).is_empty());
        let whole = clip_polygon(&big, [-1.0, -1.0, 11.0, 11.0]);
        assert!((area(&whole) - 100.0).abs() < 1e-9);
    }
}
