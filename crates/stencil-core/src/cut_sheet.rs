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
        while !rest.is_empty() {
            let conn = sample(&linked, 1500);
            let pick = rest
                .iter()
                .enumerate()
                .filter_map(|(i, c)| nearest(&sample(c, 1500), &conn).map(|n| (i, n.0)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|x| x.0)
                .unwrap_or(0);
            let isl = rest.swap_remove(pick);
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
            linked.extend(isl);
        }
        keep = material.overlay(&strips, OverlayRule::Union, FillRule::NonZero);
    }
    let path = grow(&keep, prm.tool / 2.0, prm.laser);
    // What the tool leaves of the paint: the holes of the path, grown back by the tool radius.
    let holes: Vec<Vec<Pt>> = path
        .iter()
        .flat_map(|s| s.iter().skip(1).cloned().map(ccw))
        .collect();
    let opened: Vec<Shape> = if holes.is_empty() {
        vec![]
    } else {
        grow(
            &holes.iter().cloned().map(|h| vec![h]).collect::<Vec<_>>(),
            prm.tool / 2.0,
            prm.laser,
        )
    };
    let kept: f64 = keep.iter().map(shape_area).sum();
    let free = area(&rect[0]).abs() - kept;
    let cut: f64 = opened.iter().map(shape_area).sum();
    Cut {
        shapes: path,
        islands,
        bridges,
        lost_mm2: (free - cut).max(0.0),
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
    let mut pieces = Vec::new();
    for t in layout.tile_rects() {
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
        pieces.push(Piece {
            name: t.name,
            w: b[2] - b[0],
            h: b[3] - b[1],
            shapes,
            islands: c.islands,
            bridges: c.bridges,
            lost_mm2: (c.lost_mm2 * 10.0).round() / 10.0,
        });
        crate::progress::step(
            "cut tiles",
            0.9 * pieces.len() as f64 / layout.tile_rects().len().max(1) as f64,
        );
    }
    let sheets = lay_out(&pieces, layout, prm)?;
    Ok(Cuts {
        pieces,
        sheets,
        tool: prm.tool,
    })
}

fn lay_out(pieces: &[Piece], layout: &Layout, prm: &Params) -> Result<Vec<Sheet>, String> {
    let g = prm.gap;
    if prm.sheet_w <= 0.0 || prm.sheet_h <= 0.0 {
        // The plan's rows, left to right.
        let rows = layout
            .tile_rects()
            .iter()
            .map(|t| t.row)
            .collect::<Vec<_>>();
        let (mut items, mut x, mut y, mut row_h, mut width) = (vec![], g, g, 0.0f64, 0.0f64);
        let mut row = rows.first().copied().unwrap_or(1);
        for (i, p) in pieces.iter().enumerate() {
            if rows[i] != row {
                row = rows[i];
                y += row_h + g;
                x = g;
                row_h = 0.0;
            }
            items.push((i, x, y));
            x += p.w + g;
            row_h = row_h.max(p.h);
            width = width.max(x);
        }
        return Ok(vec![Sheet {
            name: "sheet_1".into(),
            w: width,
            h: y + row_h + g,
            items,
        }]);
    }
    let (sw, sh) = (prm.sheet_w, prm.sheet_h);
    let mut order: Vec<usize> = (0..pieces.len()).collect();
    order.sort_by(|&a, &b| pieces[b].h.total_cmp(&pieces[a].h));
    // Per sheet: the shelves as (y, height, next x).
    type Shelves = Vec<(f64, f64, f64)>;
    type Items = Vec<(usize, f64, f64)>;
    let mut sheets: Vec<(Shelves, Items)> = vec![];
    for i in order {
        let p = &pieces[i];
        if p.w + 2.0 * g > sw + 1e-9 || p.h + 2.0 * g > sh + 1e-9 {
            return Err(format!(
                "{} ({:.1} x {:.1} mm with the cut) does not fit the {sw} x {sh} mm sheet",
                p.name, p.w, p.h
            ));
        }
        let mut done = false;
        for (shelves, items) in sheets.iter_mut() {
            for s in shelves.iter_mut() {
                if p.h <= s.1 + 1e-9 && s.2 + p.w + g <= sw + 1e-9 {
                    items.push((i, s.2, s.0));
                    s.2 += p.w + g;
                    done = true;
                    break;
                }
            }
            if done {
                break;
            }
            let top = shelves.last().map_or(g, |s| s.0 + s.1 + g);
            if top + p.h + g <= sh + 1e-9 {
                shelves.push((top, p.h, g + p.w + g));
                items.push((i, g, top));
                done = true;
                break;
            }
        }
        if !done {
            sheets.push((vec![(g, p.h, g + p.w + g)], vec![(i, g, g)]));
        }
    }
    Ok(sheets
        .into_iter()
        .enumerate()
        .map(|(n, (_, items))| Sheet {
            name: format!("sheet_{}", n + 1),
            w: sw,
            h: sh,
            items,
        })
        .collect())
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

    pub fn file(&self, file: &str) -> Option<Vec<u8>> {
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
                    "pieces": s.items.iter().map(|&(i, _, _)| self.pieces[i].name.clone()).collect::<Vec<_>>()})
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
    fn a_router_bit_drops_paint_narrower_than_itself() {
        let thin = vec![vec![[5.0, 5.0], [15.0, 5.0], [15.0, 6.0], [5.0, 6.0]]];
        let c = cut_tile(&thin, [0.0, 0.0, 20.0, 20.0], &prm(false, 3.0));
        assert!(c.lost_mm2 > 9.0, "{}", c.lost_mm2);
        let c = cut_tile(&thin, [0.0, 0.0, 20.0, 20.0], &prm(true, 0.2));
        assert!(c.lost_mm2 < 1.0, "{}", c.lost_mm2);
    }
}
