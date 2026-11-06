//! Cut sheet: the tiles of a plan as flat 2D parts for a laser or a CNC router. Every tile is the material left
//! round the paint; islands of material are tied to the rest with bridges, and the cut line runs half a kerf (or bit)
//! off the material, in the paint. The parts are laid out on sheets and exported as SVG and DXF in millimetres.
use crate::dxf;
use crate::tile_export::{clip_polygon, rect_poly};
use crate::tile_first::Layout;
use crate::trace::{Cubic, Loop, Pt, Shapes};
use i_triangle::i_overlay::core::fill_rule::FillRule;
use i_triangle::i_overlay::core::overlay_rule::OverlayRule;
use i_triangle::i_overlay::float::single::SingleFloatOverlay;
use i_triangle::i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_triangle::i_overlay::mesh::float::style::{LineJoin, OutlineStyle};
use serde_json::{json, Value as Json};

pub struct Params {
    pub laser: bool,
    /// Kerf of the laser or diameter of the router bit.
    pub tool: f64,
    pub bridge: f64,
    pub bridges_per_island: usize,
    /// Sheet size; 0 for no limit (the parts keep the plan's rows).
    pub sheet_w: f64,
    pub sheet_h: f64,
    pub gap: f64,
}

type Shape = Vec<Vec<Pt>>;

/// One tile as cut: shapes (outer contour first, then holes) in millimetres, y up, lower left corner at the origin.
pub struct Piece {
    pub name: String,
    pub w: f64,
    pub h: f64,
    pub shapes: Vec<Shape>,
    pub islands: usize,
    pub bridges: usize,
    pub lost_mm2: f64,
}

pub struct Sheet {
    pub name: String,
    pub w: f64,
    pub h: f64,
    /// Index of the piece and where its corner goes.
    pub items: Vec<(usize, f64, f64)>,
}

pub struct Cuts {
    pub pieces: Vec<Piece>,
    pub sheets: Vec<Sheet>,
    pub tool: f64,
}

fn area(c: &[Pt]) -> f64 {
    let mut a = 0.0;
    for i in 0..c.len() {
        let (p, q) = (c[i], c[(i + 1) % c.len()]);
        a += p[0] * q[1] - q[0] * p[1];
    }
    a / 2.0
}

fn shape_area(s: &Shape) -> f64 {
    s.iter()
        .map(|c| area(c).abs())
        .enumerate()
        .map(|(i, a)| if i == 0 { a } else { -a })
        .sum()
}

fn ccw(mut c: Vec<Pt>) -> Vec<Pt> {
    if area(&c) < 0.0 {
        c.reverse();
    }
    c
}

fn dist2(a: Pt, b: Pt) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

/// Thins a closed contour: a run of points becomes its chord while every dropped point stays within `eps` of it.
/// For measuring areas only - the real cut path keeps every point.
fn decimate(c: &[Pt], eps: f64) -> Vec<Pt> {
    if c.len() < 8 {
        return c.to_vec();
    }
    let e2 = eps * eps;
    let fits = |a: Pt, b: Pt, q: Pt| {
        let d = [b[0] - a[0], b[1] - a[1]];
        let l2 = (d[0] * d[0] + d[1] * d[1]).max(1e-18);
        let v = [q[0] - a[0], q[1] - a[1]];
        let t = ((v[0] * d[0] + v[1] * d[1]) / l2).clamp(0.0, 1.0);
        dist2([a[0] + d[0] * t, a[1] + d[1] * t], q) <= e2
    };
    let mut out = vec![c[0]];
    let mut i = 0;
    while i + 1 < c.len() {
        // the longest chord from c[i] whose skipped points all stay within eps
        let mut j = i + 1;
        while j + 1 < c.len() && (i + 1..=j).all(|k| fits(c[i], c[j + 1], c[k])) {
            j += 1;
        }
        out.push(c[j]);
        i = j;
    }
    out
}

/// `decimate` over every contour of every shape.
fn decimate_all(shapes: &[Shape], eps: f64) -> Vec<Shape> {
    shapes
        .iter()
        .map(|s| s.iter().map(|c| decimate(c, eps)).collect())
        .collect()
}

/// At most `n` of the points, evenly spread.
fn sample(v: &[Pt], n: usize) -> Vec<Pt> {
    let step = v.len().div_ceil(n).max(1);
    v.iter().step_by(step).copied().collect()
}

fn nearest(a: &[Pt], b: &[Pt]) -> Option<(f64, Pt, Pt)> {
    let mut best: Option<(f64, Pt, Pt)> = None;
    for &p in a {
        for &q in b {
            let d = dist2(p, q);
            if best.is_none_or(|x| d < x.0) {
                best = Some((d, p, q));
            }
        }
    }
    best
}

/// A strip of the given width from `a` to `b`, reaching `ext` past both ends.
fn strip(a: Pt, b: Pt, width: f64, ext: f64) -> Vec<Pt> {
    let len = dist2(a, b).sqrt();
    let d = if len < 1e-9 {
        [1.0, 0.0]
    } else {
        [(b[0] - a[0]) / len, (b[1] - a[1]) / len]
    };
    let n = [-d[1] * width / 2.0, d[0] * width / 2.0];
    let (a, b) = (
        [a[0] - d[0] * ext, a[1] - d[1] * ext],
        [b[0] + d[0] * ext, b[1] + d[1] * ext],
    );
    vec![
        [a[0] - n[0], a[1] - n[1]],
        [b[0] - n[0], b[1] - n[1]],
        [b[0] + n[0], b[1] + n[1]],
        [a[0] + n[0], a[1] + n[1]],
    ]
}

fn grow(shapes: &[Shape], by: f64, laser: bool) -> Vec<Shape> {
    let mut st = OutlineStyle::new(by);
    st = st.line_join(if laser {
        LineJoin::Miter(0.3)
    } else {
        LineJoin::Round(0.2)
    });
    shapes.to_vec().outline(&st)
}

struct Cut {
    shapes: Vec<Shape>,
    islands: usize,
    bridges: usize,
    lost_mm2: f64,
}

/// The cut of one tile: `r` is its rectangle and `art` the paint, in plate millimetres.
fn cut_tile(art: &[Vec<Pt>], r: [f64; 4], prm: &Params) -> Cut {
    let paint: Vec<Vec<Pt>> = art
        .iter()
        .flat_map(|p| clip_polygon(p, r))
        .map(ccw)
        .collect();
    let rect = vec![rect_poly(r)];
    let material: Vec<Shape> = if paint.is_empty() {
        vec![rect.clone()]
    } else {
        rect.overlay(&paint, OverlayRule::Difference, FillRule::EvenOdd)
    };
    let mut keep = material.clone();
    let (mut islands, mut bridges) = (0, 0);
    if material.len() > 1 {
        let main = (0..material.len())
            .max_by(|&a, &b| shape_area(&material[a]).total_cmp(&shape_area(&material[b])))
            .unwrap_or(0);
        let mut linked: Vec<Pt> = material[main].iter().flatten().copied().collect();
        let mut rest: Vec<Vec<Pt>> = (0..material.len())
            .filter(|&i| i != main)
            .map(|i| material[i][0].clone())
            .collect();
        islands = rest.len();
        let mut strips: Vec<Vec<Pt>> = Vec::new();
        // Linking only ever adds points, so each island's distance to the linked material is cached and refreshed
        // against the newly linked outline alone, instead of rescanning everything every round.
        let conn = sample(&linked, 1500);
        let mut best: Vec<f64> = rest
            .iter()
            .map(|c| nearest(&sample(c, 1500), &conn).map_or(f64::MAX, |n| n.0))
            .collect();
        while !rest.is_empty() {
            let pick = best
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.total_cmp(b.1))
                .map(|x| x.0)
                .unwrap_or(0);
            let isl = rest.swap_remove(pick);
            best.swap_remove(pick);
            let conn = sample(&linked, 1500);
            let k = prm.bridges_per_island.max(1).min(isl.len() / 3 + 1);
            for chunk in 0..k {
                let part: Vec<Pt> = isl
                    .iter()
                    .skip(chunk * isl.len() / k)
                    .take(isl.len() / k + 1)
                    .copied()
                    .collect();
                if let Some((_, a, b)) = nearest(&sample(&part, 600), &conn) {
                    strips.push(strip(a, b, prm.bridge, prm.bridge * 0.5));
                    bridges += 1;
                }
            }
            let newly = sample(&isl, 1500);
            for (i, c) in rest.iter().enumerate() {
                if let Some((d, _, _)) = nearest(&sample(c, 1500), &newly) {
                    best[i] = best[i].min(d);
                }
            }
            linked.extend(isl);
        }
        keep = material.overlay(&strips, OverlayRule::Union, FillRule::NonZero);
    }
    let path = grow(&keep, prm.tool / 2.0, prm.laser);
    // What the tool can reach: wherever its centre fits outside the grown material, swept by its radius. This is a
    // measure, not a cut path, so the contours are thinned to 0.02 mm and the joins mitred: both keep the point
    // count down and the corner error is far below the 0.1 mm² the figure reports.
    let t = prm.tool;
    let big = vec![rect_poly([
        r[0] - 2.0 * t,
        r[1] - 2.0 * t,
        r[2] + 2.0 * t,
        r[3] + 2.0 * t,
    ])];
    let centre: Vec<Shape> = big.overlay(
        &grow(&decimate_all(&keep, 0.02), t / 2.0, true),
        OverlayRule::Difference,
        FillRule::NonZero,
    );
    let swept = grow(&decimate_all(&centre, 0.02), t / 2.0, true);
    let opened: f64 = rect
        .overlay(&swept, OverlayRule::Intersect, FillRule::NonZero)
        .iter()
        .map(shape_area)
        .sum();
    let kept: f64 = keep.iter().map(shape_area).sum();
    let free = area(&rect[0]).abs() - kept;
    Cut {
        shapes: path,
        islands,
        bridges,
        lost_mm2: (free - opened).max(0.0),
    }
}

fn bounds(shapes: &[Shape]) -> [f64; 4] {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for p in shapes.iter().flatten().flatten() {
        b = [
            b[0].min(p[0]),
            b[1].min(p[1]),
            b[2].max(p[0]),
            b[3].max(p[1]),
        ];
    }
    if b[0] > b[2] {
        [0.0; 4]
    } else {
        b
    }
}

pub fn make(layout: &Layout, prm: &Params) -> Result<Cuts, String> {
    if prm.tool <= 0.0 {
        return Err("the tool size must be above 0".into());
    }
    let art = layout.art();
    // The tiles are independent, so they are cut in parallel where threads exist. Sub-step progress comes from the
    // thread running the node, so the lot reports as one step.
    let rects = layout.tile_rects();
    let pieces: Vec<Piece> = crate::par::map(&rects, |t| {
        let c = cut_tile(art, [t.x0, t.y0, t.x1, t.y1], prm);
        let b = bounds(&c.shapes);
        let shapes = c
            .shapes
            .into_iter()
            .map(|s| {
                s.into_iter()
                    .map(|poly| {
                        poly.into_iter()
                            .map(|p| [p[0] - b[0], p[1] - b[1]])
                            .collect()
                    })
                    .collect()
            })
            .collect();
        Piece {
            name: t.name.clone(),
            w: b[2] - b[0],
            h: b[3] - b[1],
            shapes,
            islands: c.islands,
            bridges: c.bridges,
            lost_mm2: (c.lost_mm2 * 10.0).round() / 10.0,
        }
    });
    crate::progress::step("cut tiles", 0.9);
    let sheets = lay_out(&pieces, layout, prm)?;
    Ok(Cuts {
        pieces,
        sheets,
        tool: prm.tool,
    })
}

/// Places the parts in the plan's reading order (row 1 at the top, left to right), one shelf per plan row; a row that
/// does not fit the sheet's width wraps to a new shelf and a shelf that does not fit its height starts a new sheet.
fn lay_out(pieces: &[Piece], layout: &Layout, prm: &Params) -> Result<Vec<Sheet>, String> {
    let g = prm.gap;
    let rows: Vec<usize> = layout.tile_rects().iter().map(|t| t.row).collect();
    let sized = prm.sheet_w > 0.0 && prm.sheet_h > 0.0;
    let (sw, sh) = if sized {
        (prm.sheet_w, prm.sheet_h)
    } else {
        (f64::INFINITY, f64::INFINITY)
    };
    // Per sheet: the parts with their top left corner measured down from the sheet's top.
    let mut sheets: Vec<Vec<(usize, f64, f64)>> = vec![vec![]];
    let (mut x, mut top, mut shelf_h) = (g, g, 0.0f64);
    let mut row = rows.first().copied().unwrap_or(1);
    for (i, p) in pieces.iter().enumerate() {
        if sized && (p.w + 2.0 * g > sw + 1e-9 || p.h + 2.0 * g > sh + 1e-9) {
            return Err(format!(
                "{} ({:.1} x {:.1} mm with the cut) does not fit the {sw} x {sh} mm sheet",
                p.name, p.w, p.h
            ));
        }
        let new_row = rows.get(i).is_some_and(|&r| r != row);
        if new_row || (x > g && x + p.w + g > sw + 1e-9) {
            top += shelf_h + g;
            x = g;
            shelf_h = 0.0;
        }
        row = rows.get(i).copied().unwrap_or(row);
        if top + p.h.max(shelf_h) + g > sh + 1e-9 {
            sheets.push(vec![]);
            top = g;
            x = g;
            shelf_h = 0.0;
        }
        sheets.last_mut().unwrap().push((i, x, top));
        x += p.w + g;
        shelf_h = shelf_h.max(p.h);
    }
    let out = sheets
        .into_iter()
        .enumerate()
        .map(|(n, items)| {
            let (w, h) = if sized {
                (sw, sh)
            } else {
                let right = items
                    .iter()
                    .map(|&(i, x, _)| x + pieces[i].w)
                    .fold(0.0, f64::max);
                let bottom = items
                    .iter()
                    .map(|&(i, _, y)| y + pieces[i].h)
                    .fold(0.0, f64::max);
                (right + g, bottom + g)
            };
            Sheet {
                name: format!("sheet_{}", n + 1),
                w,
                h,
                // DXF and the stored positions have y up from the sheet's lower edge.
                items: items
                    .into_iter()
                    .map(|(i, x, t)| (i, x, h - t - pieces[i].h))
                    .collect(),
            }
        })
        .collect();
    Ok(out)
}

fn layers_of(shapes: &[Shape], dx: f64, dy: f64) -> (Vec<Vec<Pt>>, Vec<Vec<Pt>>) {
    let (mut outer, mut inner) = (vec![], vec![]);
    for s in shapes {
        for (i, c) in s.iter().enumerate() {
            let c = c.iter().map(|p| [p[0] + dx, p[1] + dy]).collect();
            if i == 0 { &mut outer } else { &mut inner }.push(c);
        }
    }
    (outer, inner)
}

fn dxf_of(items: &[(&Piece, f64, f64)]) -> String {
    let (mut outer, mut inner) = (vec![], vec![]);
    for (p, x, y) in items {
        let (o, i) = layers_of(&p.shapes, *x, *y);
        outer.extend(o);
        inner.extend(i);
    }
    dxf::write(&[
        dxf::Layer {
            name: "CUT_1_INNER",
            colour: 1,
            polys: &inner,
        },
        dxf::Layer {
            name: "CUT_2_OUTER",
            colour: 5,
            polys: &outer,
        },
    ])
}

fn svg_of(items: &[(&Piece, f64, f64)], w: f64, h: f64) -> String {
    let path = |polys: &[Vec<Pt>]| {
        let mut d = String::new();
        for poly in polys {
            for (i, p) in poly.iter().enumerate() {
                d += &format!(
                    "{}{:.3} {:.3}",
                    if i == 0 { "M" } else { "L" },
                    p[0],
                    h - p[1]
                );
            }
            d += "Z";
        }
        d
    };
    let (mut outer, mut inner) = (vec![], vec![]);
    for (p, x, y) in items {
        let (o, i) = layers_of(&p.shapes, *x, *y);
        outer.extend(o);
        inner.extend(i);
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.3}mm\" height=\"{h:.3}mm\" viewBox=\"0 0 {w:.3} {h:.3}\"><path id=\"CUT_1_INNER\" d=\"{}\" fill=\"none\" stroke=\"#f00\" stroke-width=\"0.1\"/><path id=\"CUT_2_OUTER\" d=\"{}\" fill=\"none\" stroke=\"#00f\" stroke-width=\"0.1\"/></svg>",
        path(&inner),
        path(&outer)
    )
}

impl Cuts {
    fn placed(&self, s: &Sheet) -> Vec<(&Piece, f64, f64)> {
        s.items
            .iter()
            .map(|&(i, x, y)| (&self.pieces[i], x, y))
            .collect()
    }

    /// All sheets side by side as one picture, 1 unit = 1 mm.
    pub fn shapes(&self) -> Shapes {
        let total_h = self.sheets.iter().map(|s| s.h).fold(0.0, f64::max);
        let mut x0 = 0.0;
        let mut loops = vec![];
        for s in &self.sheets {
            for (p, x, y) in self.placed(s) {
                let (o, i) = layers_of(&p.shapes, x0 + x, y);
                for c in o.into_iter().chain(i) {
                    let pts: Vec<Pt> = c.iter().map(|q| [q[0], total_h - q[1]]).collect();
                    let n = pts.len();
                    let segs = (0..n)
                        .map(|k| {
                            let (a, b) = (pts[k], pts[(k + 1) % n]);
                            let l = |t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
                            Cubic([a, l(1.0 / 3.0), l(2.0 / 3.0), b])
                        })
                        .collect();
                    loops.push(Loop { segs });
                }
            }
            x0 += s.w + 10.0;
        }
        Shapes {
            w: (x0 - 10.0).ceil().max(1.0) as usize,
            h: total_h.ceil().max(1.0) as usize,
            loops,
        }
    }

    pub fn file_names(&self) -> Vec<String> {
        self.pieces
            .iter()
            .map(|p| &p.name)
            .chain(self.sheets.iter().map(|s| &s.name))
            .flat_map(|n| [format!("{n}.svg"), format!("{n}.dxf")])
            .collect()
    }

    /// A light picture of the sheets side by side, with only the named parts drawn (filled, simplified to 0.1 mm), for
    /// the preview. The sheet borders are always drawn.
    pub fn preview(&self, shown: &[&str]) -> String {
        const PX: f64 = 3.0;
        let total_h = self.sheets.iter().map(|s| s.h).fold(0.0, f64::max);
        let total_w: f64 = self.sheets.iter().map(|s| s.w).sum::<f64>()
            + 10.0 * (self.sheets.len().max(1) - 1) as f64;
        let (mut d, mut frames, mut x0) = (String::new(), String::new(), 0.0);
        for s in &self.sheets {
            frames += &format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\"/>",
                x0 * PX,
                (total_h - s.h) * PX,
                s.w * PX,
                s.h * PX
            );
            for (p, x, y) in self.placed(s) {
                if !shown.contains(&p.name.as_str()) {
                    continue;
                }
                for c in p.shapes.iter().flatten() {
                    let mut last: Option<Pt> = None;
                    for (i, q) in c.iter().enumerate() {
                        if last.is_some_and(|l| dist2(l, *q) < 0.01) && i + 1 < c.len() {
                            continue;
                        }
                        last = Some(*q);
                        d += &format!(
                            "{}{:.1} {:.1}",
                            if i == 0 { "M" } else { "L" },
                            (x0 + x + q[0]) * PX,
                            (total_h - y - q[1]) * PX
                        );
                    }
                    d += "Z";
                }
            }
            x0 += s.w + 10.0;
        }
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\" viewBox=\"0 0 {w:.1} {h:.1}\"><g fill=\"#f4f1ea\" stroke=\"#999\" stroke-width=\"2\">{frames}</g><path d=\"{d}\" fill=\"#4a4a4a\" fill-rule=\"evenodd\" stroke=\"#000\" stroke-width=\"1\"/></svg>",
            w = (total_w * PX).max(1.0),
            h = (total_h * PX).max(1.0)
        )
    }

    pub fn file(&self, file: &str) -> Option<Vec<u8>> {
        if let Some(list) = file.strip_prefix("preview:") {
            let shown: Vec<&str> = list.split(',').filter(|n| !n.is_empty()).collect();
            return Some(self.preview(&shown).into_bytes());
        }
        let (stem, ext) = file.rsplit_once('.')?;
        let (items, w, h): (Vec<(&Piece, f64, f64)>, f64, f64) =
            if let Some(p) = self.pieces.iter().find(|p| p.name == stem) {
                (vec![(p, 0.0, 0.0)], p.w, p.h)
            } else {
                let s = self.sheets.iter().find(|s| s.name == stem)?;
                (self.placed(s), s.w, s.h)
            };
        match ext {
            "svg" => Some(svg_of(&items, w, h).into_bytes()),
            "dxf" => Some(dxf_of(&items).into_bytes()),
            _ => None,
        }
    }

    pub fn info(&self) -> Json {
        let pieces: Vec<Json> = self
            .pieces
            .iter()
            .map(|p| {
                json!({"name": p.name, "w": (p.w * 10.0).round() / 10.0, "h": (p.h * 10.0).round() / 10.0,
                    "islands": p.islands, "bridges": p.bridges, "lost_mm2": p.lost_mm2})
            })
            .collect();
        let sheets: Vec<Json> = self
            .sheets
            .iter()
            .map(|s| {
                json!({"name": s.name, "w": (s.w * 10.0).round() / 10.0, "h": (s.h * 10.0).round() / 10.0,
                    "pieces": s.items.iter().map(|&(i, _, _)| self.pieces[i].name.clone()).collect::<Vec<_>>(),
                    "items": s.items.iter().map(|&(i, x, y)| {
                        let p = &self.pieces[i];
                        json!({"name": p.name, "x": (x * 1e3).round() / 1e3, "y": ((s.h - y - p.h) * 1e3).round() / 1e3})
                    }).collect::<Vec<_>>()})
            })
            .collect();
        json!({"kind": "cuts", "tool": self.tool, "piece_list": pieces, "sheet_list": sheets, "cut_files": self.file_names()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prm(laser: bool, tool: f64) -> Params {
        Params {
            laser,
            tool,
            bridge: 2.0,
            bridges_per_island: 1,
            sheet_w: 0.0,
            sheet_h: 0.0,
            gap: 3.0,
        }
    }

    fn circle(c: Pt, r: f64) -> Vec<Pt> {
        (0..64)
            .map(|i| {
                let a = i as f64 / 64.0 * std::f64::consts::TAU;
                [c[0] + r * a.cos(), c[1] + r * a.sin()]
            })
            .collect()
    }

    #[test]
    fn the_offset_grows_the_outside_and_shrinks_the_holes() {
        let r = vec![rect_poly([0.0, 0.0, 20.0, 20.0])];
        let ring: Vec<Shape> = r.overlay(
            &vec![circle([10.0, 10.0], 5.0)],
            OverlayRule::Difference,
            FillRule::EvenOdd,
        );
        let out = grow(&ring, 0.5, true);
        let b = bounds(&out);
        assert!(
            (b[0] + 0.5).abs() < 1e-3 && (b[2] - 20.5).abs() < 1e-3,
            "{b:?}"
        );
        let hole = area(&out[0][1]).abs();
        let was = area(&ring[0][1]).abs();
        assert!(hole < was, "{hole} {was}");
    }

    #[test]
    fn an_island_is_bridged_into_one_piece() {
        // A ring of paint round a disc of material: the disc is an island.
        let ring = vec![circle([10.0, 10.0], 7.0), circle([10.0, 10.0], 4.0)];
        let c = cut_tile(&ring, [0.0, 0.0, 20.0, 20.0], &prm(true, 0.2));
        assert_eq!(c.islands, 1);
        assert_eq!(c.bridges, 1);
        assert_eq!(c.shapes.len(), 1, "one piece");
    }

    #[test]
    fn paint_at_the_tile_edge_is_not_counted_as_lost() {
        let edge = vec![vec![[0.0, 5.0], [10.0, 5.0], [10.0, 12.0], [0.0, 12.0]]];
        let c = cut_tile(&edge, [0.0, 0.0, 20.0, 20.0], &prm(true, 0.2));
        assert!(c.lost_mm2 < 1.0, "{}", c.lost_mm2);
    }

    #[test]
    fn the_parts_keep_the_plan_order_from_the_top() {
        let piece = |n: &str| Piece {
            name: n.into(),
            w: 50.0,
            h: 40.0,
            shapes: vec![],
            islands: 0,
            bridges: 0,
            lost_mm2: 0.0,
        };
        let pieces: Vec<Piece> = ["a", "b", "c", "d"].iter().map(|n| piece(n)).collect();
        let sq = |x: f64, y: f64| {
            let line = |a: Pt, b: Pt| Cubic([a, a, b, b]);
            Loop {
                segs: vec![
                    line([x, y], [x + 5.0, y]),
                    line([x + 5.0, y], [x + 5.0, y + 5.0]),
                    line([x + 5.0, y + 5.0], [x, y + 5.0]),
                    line([x, y + 5.0], [x, y]),
                ],
            }
        };
        let shapes = crate::trace::Shapes {
            w: 200,
            h: 160,
            loops: vec![sq(0.0, 0.0), sq(195.0, 155.0)],
        };
        let lp = crate::tile_first::LayoutParams {
            width: 200.0,
            margin: 0.0,
            max_x: 100.0,
            max_y: 80.0,
            seam_shift: 0.0,
            edits: Default::default(),
            spec: crate::tile_first::Connectors {
                shape: crate::tiles::Shape::None,
                diameter: 3.0,
                clearance: 0.2,
                depth: 10.0,
                segments: 48,
                spacing: 30.0,
                seam_gap: 0.0,
            },
        };
        let l = crate::tile_first::from_shapes(&shapes, &lp).unwrap();
        assert_eq!(l.tile_rects().len(), 4);
        let mut q = prm(true, 0.2);
        for (sw, sh) in [(0.0, 0.0), (400.0, 400.0)] {
            q.sheet_w = sw;
            q.sheet_h = sh;
            let s = lay_out(&pieces, &l, &q).unwrap();
            assert_eq!(s.len(), 1);
            let at: Vec<(f64, f64)> = s[0].items.iter().map(|&(_, x, y)| (x, y)).collect();
            // a and b side by side on top, c and d below them
            assert!(at[0].1 == at[1].1 && at[0].0 < at[1].0, "{at:?}");
            assert!(at[2].1 < at[0].1 && at[2].0 == at[0].0, "{at:?}");
        }
        q.sheet_w = 100.0;
        q.sheet_h = 50.0;
        let s = lay_out(&pieces, &l, &q).unwrap();
        assert_eq!(s.len(), 4, "one shelf of one part per sheet");
        assert_eq!(
            s.iter().map(|s| s.items[0].0).collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
    }

    #[test]
    fn a_router_bit_drops_paint_narrower_than_itself() {
        let thin = vec![vec![[5.0, 5.0], [15.0, 5.0], [15.0, 6.0], [5.0, 6.0]]];
        let c = cut_tile(&thin, [0.0, 0.0, 20.0, 20.0], &prm(false, 3.0));
        assert!(c.lost_mm2 > 9.0, "{}", c.lost_mm2);
        let c = cut_tile(&thin, [0.0, 0.0, 20.0, 20.0], &prm(true, 0.2));
        assert!(c.lost_mm2 < 1.0, "{}", c.lost_mm2);
    }
}
