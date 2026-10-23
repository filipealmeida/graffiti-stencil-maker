//! Tiling before bridging. The plate is divided on a fixed grid while it is still a flat drawing; where two tiles
//! meet, blocks of material are reserved for the connector pockets. Every tile is then a small stencil of its own,
//! bridged on its own, so a cut never creates an island: a piece that a seam separates from its neighbours is
//! just another island of its tile. The pockets are drilled into the finished tiles.
use crate::mesh::Mesh;
use crate::tiles::{self, Shape, ROW_GAP_MM, WALL_MM};
use crate::trace::Shapes;
use crate::vector_stencil::{self, Window};
use crate::{Output, Params};
use serde_json::{json, Value as Json};

#[derive(Clone)]
pub struct Connectors {
    pub shape: Shape,
    pub diameter: f64,
    pub clearance: f64,
    /// How deep a pocket sinks into each of the two tiles (mm).
    pub depth: f64,
    pub segments: usize,
    /// About this far apart along a seam (mm).
    pub spacing: f64,
}

impl Connectors {
    fn hole_radius(&self) -> f64 {
        (self.diameter + self.clearance) / 2.0
    }
    /// Half the width of the material around a pocket, along the seam.
    fn half_width(&self) -> f64 {
        self.hole_radius() + WALL_MM
    }
}

/// A connector pocket: a rod-shaped hole across the seam `axis` (0 = a line of constant x), at `s` across and `u`
/// along the seam.
#[derive(Clone, Copy, Debug)]
pub struct Pocket {
    pub axis: usize,
    pub s: f64,
    pub u: f64,
}

impl Pocket {
    fn rect(&self, across: f64, along: f64) -> [f64; 4] {
        if self.axis == 0 {
            [
                self.s - across,
                self.u - along,
                self.s + across,
                self.u + along,
            ]
        } else {
            [
                self.u - along,
                self.s - across,
                self.u + along,
                self.s + across,
            ]
        }
    }
    /// The plan footprint of the pocket and the wall around it.
    pub fn footprint(&self, c: &Connectors) -> [f64; 4] {
        self.rect(c.depth, c.half_width())
    }
    /// The material kept solid: the footprint with a wall at the closed ends.
    fn block(&self, c: &Connectors) -> [f64; 4] {
        self.rect(c.depth + WALL_MM, c.half_width())
    }
}

/// The paint of the artwork on a grid, for measuring how much a block would remove.
pub struct Cover {
    paint: Vec<bool>,
    gw: usize,
    gh: usize,
    /// Plate x of the grid's left edge and plate y of its top edge.
    x0: f64,
    y_top: f64,
    cw: f64,
    ch: f64,
}

impl Cover {
    fn range(&self, r: [f64; 4]) -> (usize, usize, usize, usize) {
        let cl = |v: f64, n: usize| v.clamp(0.0, n as f64) as usize;
        (
            cl(((r[0] - self.x0) / self.cw).floor(), self.gw),
            cl(((r[2] - self.x0) / self.cw).ceil(), self.gw),
            cl(((self.y_top - r[3]) / self.ch).floor(), self.gh),
            cl(((self.y_top - r[1]) / self.ch).ceil(), self.gh),
        )
    }
    fn paint_mm2(&self, r: [f64; 4]) -> f64 {
        let (c0, c1, r0, r1) = self.range(r);
        let mut n = 0usize;
        for row in r0..r1 {
            n += self.paint[row * self.gw + c0..row * self.gw + c1.max(c0)]
                .iter()
                .filter(|&&p| p)
                .count();
        }
        n as f64 * self.cw * self.ch
    }
    fn at(&self, x: f64, y: f64) -> bool {
        let c = ((x - self.x0) / self.cw).floor();
        let r = ((self.y_top - y) / self.ch).floor();
        c >= 0.0
            && r >= 0.0
            && (c as usize) < self.gw
            && (r as usize) < self.gh
            && self.paint[r as usize * self.gw + c as usize]
    }
}

pub enum Source {
    /// The whole plate as cells (row 0 at the top, `true` = paint) with the frame and the reserved blocks solid.
    Raster {
        w: usize,
        h: usize,
        cell: f64,
        paint: Vec<bool>,
    },
    Vector(Shapes),
}

pub struct Layout {
    pub width: f64,
    pub height: f64,
    /// The solid frame around the artwork (a raster plate rounds the margin up to whole cells).
    pub margin: f64,
    /// Grid lines in plate millimetres, the outer edges included.
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
    pub spec: Connectors,
    pub pockets: Vec<Pocket>,
    pub source: Source,
    cover: Cover,
}

pub struct LayoutParams {
    pub width: f64,
    pub margin: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub spec: Connectors,
    /// How far a seam may move off the even grid to cross less paint (never past the largest tile size).
    pub seam_shift: f64,
}

fn check_plate(lp: &LayoutParams) -> Result<(), String> {
    if lp.width < 20.0 || lp.width > 2000.0 {
        return Err(format!("width must be 20-2000 mm, got {}", lp.width));
    }
    if lp.margin < 0.0 || lp.width <= 2.0 * lp.margin {
        return Err(format!(
            "margin too large for the plate width: width {} mm, margin {} mm (needs width > 2 x margin)",
            lp.width, lp.margin
        ));
    }
    if lp.max_x < 0.0 || lp.max_y < 0.0 {
        return Err("the largest tile size cannot be negative".into());
    }
    Ok(())
}

/// A raster layout: the plate is the stencil's own grid (the mask padded by the margin), cut at cell boundaries.
pub fn from_mask(
    paint: &[bool],
    gw: usize,
    gh: usize,
    lp: &LayoutParams,
) -> Result<Layout, String> {
    check_plate(lp)?;
    if gw == 0 || gh == 0 || paint.len() != gw * gh {
        return Err("mask size does not match its dimensions".into());
    }
    let (mpx, w, h, cell) = crate::plate_grid(gw, gh, lp.width, lp.margin);
    let mut full = vec![false; w * h];
    for r in 0..gh {
        full[(r + mpx) * w + mpx..(r + mpx) * w + mpx + gw]
            .copy_from_slice(&paint[r * gw..(r + 1) * gw]);
    }
    let (width, height) = (w as f64 * cell, h as f64 * cell);
    let snap = |v: Vec<f64>, cells: usize| -> Vec<f64> {
        let last = v.len() - 1;
        v.iter()
            .enumerate()
            .map(|(i, &x)| {
                if i == 0 {
                    0.0
                } else if i == last {
                    cells as f64 * cell
                } else {
                    (x / cell).round() * cell
                }
            })
            .collect()
    };
    let mut xs = snap(tiles::grid(width, lp.max_x), w);
    let mut ys = snap(tiles::grid(height, lp.max_y), h);
    let cover = Cover {
        paint: full.clone(),
        gw: w,
        gh: h,
        x0: 0.0,
        y_top: height,
        cw: cell,
        ch: cell,
    };
    shift_seams(lp, &mut xs, &mut ys, &cover, cell);
    let pockets = plan(&lp.spec, &xs, &ys, &cover)?;
    for pk in &pockets {
        let b = pk.block(&lp.spec);
        let (c0, c1, r0, r1) = {
            let cl = |v: f64, n: usize| v.clamp(0.0, n as f64) as usize;
            (
                cl((b[0] / cell + 1e-6).floor(), w),
                cl((b[2] / cell - 1e-6).ceil(), w),
                cl(((height - b[3]) / cell + 1e-6).floor(), h),
                cl(((height - b[1]) / cell - 1e-6).ceil(), h),
            )
        };
        for r in r0..r1 {
            full[r * w + c0..r * w + c1].fill(false);
        }
    }
    Ok(Layout {
        width,
        height,
        margin: mpx as f64 * cell,
        xs,
        ys,
        spec: lp.spec.clone(),
        pockets,
        source: Source::Raster {
            w,
            h,
            cell,
            paint: full,
        },
        cover,
    })
}

/// A vector layout: the tiles are windows onto the outlines, cut exactly at the seams.
pub fn from_shapes(shapes: &Shapes, lp: &LayoutParams) -> Result<Layout, String> {
    check_plate(lp)?;
    if shapes.w == 0 || shapes.h == 0 || shapes.loops.is_empty() {
        return Err(
            "the shapes contain no closed outlines: trace a picture that has paint in it".into(),
        );
    }
    let art_w = lp.width - 2.0 * lp.margin;
    let mm_per_px = art_w / shapes.w as f64;
    let art_h = shapes.h as f64 * mm_per_px;
    let height = art_h + 2.0 * lp.margin;
    // a rough raster of the artwork is all the pocket search needs
    let gw = ((art_w / 0.4).ceil() as usize).clamp(16, 2500);
    let gh = ((art_h / (art_w / gw as f64)).round() as usize).clamp(1, 2500);
    let cover = Cover {
        paint: crate::trace::rasterise(shapes, gw, gh),
        gw,
        gh,
        x0: lp.margin,
        y_top: lp.margin + art_h,
        cw: art_w / gw as f64,
        ch: art_h / gh as f64,
    };
    let mut xs = tiles::grid(lp.width, lp.max_x);
    let mut ys = tiles::grid(height, lp.max_y);
    shift_seams(lp, &mut xs, &mut ys, &cover, 0.0);
    let pockets = plan(&lp.spec, &xs, &ys, &cover)?;
    Ok(Layout {
        width: lp.width,
        height,
        margin: lp.margin,
        xs,
        ys,
        spec: lp.spec.clone(),
        pockets,
        source: Source::Vector(shapes.clone()),
        cover,
    })
}

fn overlap(a: [f64; 4], b: [f64; 4], gap: f64) -> bool {
    a[0] < b[2] + gap && b[0] < a[2] + gap && a[1] < b[3] + gap && b[1] < a[3] + gap
}

/// Moves every inner grid line (within the shift, the largest tile size and the smallest workable tile) to where
/// the pockets that then fit cover the least paint. `quantum` keeps raster seams on cell boundaries.
fn shift_seams(lp: &LayoutParams, xs: &mut [f64], ys: &mut [f64], cover: &Cover, quantum: f64) {
    if lp.seam_shift <= 0.0 {
        return;
    }
    let low = tiles::min_tile(lp.spec.depth);
    let cost = |xs: &[f64], ys: &[f64]| -> f64 {
        plan(&lp.spec, xs, ys, cover).map_or(f64::MAX, |ps| {
            ps.iter().map(|p| cover.paint_mm2(p.block(&lp.spec))).sum()
        })
    };
    let mut best = cost(xs, ys);
    for axis in 0..2 {
        let (count, max) = if axis == 0 {
            (xs.len(), lp.max_x)
        } else {
            (ys.len(), lp.max_y)
        };
        for i in 1..count.saturating_sub(1) {
            let g: &mut [f64] = if axis == 0 { &mut *xs } else { &mut *ys };
            let old = g[i];
            let lo = (old - lp.seam_shift).max(g[i - 1] + low).max(if max > 0.0 {
                g[i + 1] - max
            } else {
                f64::MIN
            });
            let hi = (old + lp.seam_shift).min(g[i + 1] - low).min(if max > 0.0 {
                g[i - 1] + max
            } else {
                f64::MAX
            });
            let step = if quantum > 0.0 { quantum } else { 1.0 };
            let mut v = if quantum > 0.0 {
                (lo / step).ceil() * step
            } else {
                lo
            };
            let mut pick = old;
            while v <= hi + 1e-9 {
                let g: &mut [f64] = if axis == 0 { &mut *xs } else { &mut *ys };
                g[i] = v;
                let c = cost(xs, ys) + 0.001 * (v - old).abs();
                if c < best - 1e-9 {
                    best = c;
                    pick = v;
                }
                v += step;
            }
            let g: &mut [f64] = if axis == 0 { &mut *xs } else { &mut *ys };
            g[i] = pick;
        }
    }
}

/// Pockets along every seam segment, each where the fewest paint is under its block.
fn plan(c: &Connectors, xs: &[f64], ys: &[f64], cover: &Cover) -> Result<Vec<Pocket>, String> {
    let (nx, ny) = (xs.len() - 1, ys.len() - 1);
    if nx * ny == 1 {
        return Ok(Vec::new());
    }
    for (g, n) in [(xs, nx), (ys, ny)] {
        if n > 1
            && g.windows(2)
                .any(|v| v[1] - v[0] < tiles::min_tile(c.depth) - 1e-6)
        {
            return Err(format!(
                "tiles must be at least {} mm wide to hold connector pockets",
                tiles::min_tile(c.depth)
            ));
        }
    }
    let mut seams: Vec<(usize, f64, f64, f64)> = Vec::new();
    for &s in &xs[1..nx] {
        for j in 0..ny {
            seams.push((0, s, ys[j], ys[j + 1]));
        }
    }
    for &s in &ys[1..ny] {
        for i in 0..nx {
            seams.push((1, s, xs[i], xs[i + 1]));
        }
    }
    let hw = c.half_width();
    let reach = c.depth + WALL_MM;
    // seams of one axis run along the other axis' extent
    let edge = [(ys[0], ys[ny]), (xs[0], xs[nx])];
    let mut pockets: Vec<Pocket> = Vec::new();
    let mut blocks: Vec<[f64; 4]> = Vec::new();
    for (axis, s, a, b) in seams {
        let len = b - a;
        if len < 2.0 * hw + 1.0 {
            return Err(format!(
                "a seam segment is only {len:.1} mm long: it cannot hold a connector pocket"
            ));
        }
        // keep clear of the ends where another seam crosses; at the plate edge only the end wall is needed
        let (ea, eb) = (a <= edge[axis].0 + 1e-6, b >= edge[axis].1 - 1e-6);
        let near = hw + WALL_MM + 0.5;
        let far = (reach + hw + 0.5).min(len / 2.0 - 1e-6).max(hw);
        let (ina, inb) = (if ea { near } else { far }, if eb { near } else { far });
        let n = ((len / c.spacing.max(5.0) - 1e-9).ceil() as usize).max(1);
        let cell = len / n as f64;
        for k in 0..n {
            let mid = a + cell * (k as f64 + 0.5);
            let lo = (a + ina).max(a + cell * k as f64 + hw);
            let hi = (b - inb).min(a + cell * (k + 1) as f64 - hw);
            let (lo, hi) = if hi < lo { (mid, mid) } else { (lo, hi) };
            let mut options: Vec<f64> = Vec::new();
            let mut u = lo;
            while u < hi {
                options.push(u);
                u += 1.0;
            }
            options.push(hi);
            let mut best: Option<(f64, Pocket)> = None;
            for u in options {
                let pk = Pocket { axis, s, u };
                let r = pk.block(c);
                if blocks.iter().any(|&o| overlap(r, o, ROW_GAP_MM)) {
                    continue;
                }
                let score = cover.paint_mm2(r) + 0.01 * (u - mid).abs();
                if best.is_none_or(|x| score < x.0) {
                    best = Some((score, pk));
                }
            }
            let Some((_, pk)) = best else {
                return Err(format!(
                    "no room for a connector pocket on the seam at {s:.0} mm"
                ));
            };
            blocks.push(pk.block(c));
            pockets.push(pk);
        }
    }
    Ok(pockets)
}

/// A boolean can leave closed fragments no thicker than a rounding error in two directions where a cutter just touches a corner;
/// they are no part of the tile, so they go.
fn drop_slivers(mesh: &mut Mesh) {
    let (n, of) = crate::tile_bridge::components(mesh);
    if n < 2 {
        return;
    }
    let mut lo = vec![[f64::MAX; 3]; n];
    let mut hi = vec![[f64::MIN; 3]; n];
    for (t, &k) in mesh.t.iter().zip(&of) {
        for &i in t {
            for a in 0..3 {
                lo[k][a] = lo[k][a].min(mesh.v[i][a]);
                hi[k][a] = hi[k][a].max(mesh.v[i][a]);
            }
        }
    }
    let tiny = |k: usize| (0..3).filter(|&a| hi[k][a] - lo[k][a] < 0.05).count() >= 2;
    if (0..n).all(tiny) {
        return;
    }
    let mut k = 0;
    mesh.t.retain(|_| {
        k += 1;
        !tiny(of[k - 1])
    });
}

/// How the tiles are built: the stencil parameters of the plate, with the mesh detail for outlines.
pub enum Method<'a> {
    Raster(&'a Params),
    Vector(&'a Params, f64),
    /// Like `Vector`, but the plate is bridged once as a whole and each tile is cut from it, so neighbouring tiles
    /// meet at a seam with the same ramps. Only the pieces a cut leaves loose are bridged again.
    Cut(&'a Params, f64),
}

/// A piece of a cut tile this small that nothing can hold is dropped (and reported) instead of failing the plate.
const FRAGMENT_MM: f64 = 10.0;

pub struct Built {
    pub tiles: Vec<tiles::Tile>,
    /// `[axis, position, from, to]`, in the frame of the printed tiles.
    pub seams: Vec<[f64; 4]>,
    /// `[x0, y0, x1, y1]` plan footprints of the pockets, in the same frame.
    pub connectors: Vec<[f64; 4]>,
    pub info: Json,
    /// The printed height of a tile, and the layer height.
    pub top: f64,
    pub layer: f64,
}

struct TileOut {
    tile: tiles::Tile,
    islands: usize,
    bridges: usize,
    fallback: Option<String>,
    top: f64,
    dropped: usize,
}

/// The whole plate, bridged as one: the printed mesh and what the build said about it.
type Whole = (Mesh, Output);

fn whole_plate(l: &Layout, shapes: &Shapes, detail: f64, p: &Params) -> Result<Whole, String> {
    let win = Window {
        origin: [0.0, 0.0],
        size: [l.width, l.height],
        solid: l.pockets.iter().map(|pk| pk.block(&l.spec)).collect(),
    };
    let out = vector_stencil::build_with(shapes, detail, p, Some(&win))?;
    Ok((Mesh::from_stl(&out.stl)?, out))
}

/// One tile cut from the bridged plate. Returns the tile as a stencil output and how many loose fragments were dropped.
fn cut_tile(
    whole: &Whole,
    p: &Params,
    l: &Layout,
    (x0, x1, y0, y1): (f64, f64, f64, f64),
) -> Result<(Output, usize), String> {
    let (global, out) = whole;
    let top = out.stats.thickness_mm;
    // the printed plate is mirrored about its width when flipped
    let (px0, px1) = if p.flip {
        (l.width - x1, l.width - x0)
    } else {
        (x0, x1)
    };
    let clip = Mesh::cuboid([px0, y0, -1.0], [px1, y1, top + 1.0]);
    let mut m = global.intersect(&clip)?;
    let loose = crate::tile_bridge::components(&m).0.saturating_sub(1);
    let layer = p.layer_height_mm;
    let bp = crate::tile_bridge::BridgeParams {
        bridge_mm: p.bridge_mm,
        per_island: p.bridges_per_island as usize,
        raised: p.raised_bridges,
        easy: p.z_bridging == crate::ZBridging::Easy,
        base_layers: (p.thickness_mm / layer).round() as u32,
        layer_height: layer,
        plate_top: top,
        flip: p.flip,
    };
    for _ in 0..3 {
        let Some(b) = crate::tile_bridge::bridge_mesh(&m, &[], &bp)? else {
            break;
        };
        m = m.union(&b)?.intersect(&clip)?;
    }
    let mut dropped = 0;
    let (left, of) = crate::tile_bridge::components(&m);
    if left > 1 {
        let mut tris = vec![0usize; left];
        let mut lo = vec![[f64::MAX; 2]; left];
        let mut hi = vec![[f64::MIN; 2]; left];
        for (t, &o) in m.t.iter().zip(&of) {
            tris[o] += 1;
            for &vi in t {
                for k in 0..2 {
                    lo[o][k] = lo[o][k].min(m.v[vi][k]);
                    hi[o][k] = hi[o][k].max(m.v[vi][k]);
                }
            }
        }
        let main = (0..left).max_by_key(|&c| tris[c]).unwrap_or(0);
        let small = |c: usize| (hi[c][0] - lo[c][0]).max(hi[c][1] - lo[c][1]) <= FRAGMENT_MM;
        if !(0..left).all(|c| c == main || small(c)) {
            return Err(format!(
                "the cut leaves {} loose pieces that cannot be bridged: set the Tile method to per_tile",
                left - 1
            ));
        }
        m.t =
            m.t.iter()
                .zip(&of)
                .filter(|(_, &o)| o == main)
                .map(|(t, _)| *t)
                .collect();
        dropped = left - 1;
    }
    m.translate([-px0, -y0, 0.0]);
    let mut stats = out.stats.clone();
    stats.islands_found = loose;
    stats.bridge_pixels = 0;
    stats.islands_remaining = 0;
    stats.watertight = m.is_watertight();
    let o = Output {
        stl: m.to_stl(),
        stats,
        fallback: None,
    };
    Ok((o, dropped))
}

fn build_tile(
    l: &Layout,
    m: &Method,
    p: &Params,
    (i, j): (usize, usize),
    zs: &[f64],
    whole: Option<&Whole>,
) -> Result<TileOut, String> {
    let (nx, ny) = (l.xs.len() - 1, l.ys.len() - 1);
    let (x0, x1, y0, y1) = (l.xs[i], l.xs[i + 1], l.ys[j], l.ys[j + 1]);
    let (tw, th) = (x1 - x0, y1 - y0);
    let name = format!(
        "stencil_r{}c{}",
        ny - j,
        if p.flip { nx - i } else { i + 1 }
    );
    let fail = |e: String| format!("tile {name}: {e}");
    let mut dropped = 0usize;
    let out: Output = match (m, &l.source) {
        (Method::Raster(_), Source::Raster { w, h, cell, paint }) => {
            let cells = |v: f64| (v / cell).round() as usize;
            let (c0, c1, g0, g1) = (cells(x0), cells(x1), cells(y0), cells(y1));
            let mut crop = Vec::with_capacity((c1 - c0) * (g1 - g0));
            for r in (h - g1)..(h - g0) {
                crop.extend_from_slice(&paint[r * w + c0..r * w + c1]);
            }
            let q = Params {
                width_mm: (c1 - c0) as f64 * cell,
                margin_mm: 0.0,
                frame_largest: true,
                ..p.clone()
            };
            crate::stencil_from_paint(&crop, c1 - c0, g1 - g0, 0, &q).map_err(fail)?
        }
        (Method::Vector(_, detail), Source::Vector(shapes)) => {
            let solid = l
                .pockets
                .iter()
                .map(|pk| pk.block(&l.spec))
                .filter_map(|b| {
                    let r = [b[0].max(x0), b[1].max(y0), b[2].min(x1), b[3].min(y1)];
                    (r[0] < r[2] && r[1] < r[3]).then_some(r)
                })
                .collect();
            let win = Window {
                origin: [x0, y0],
                size: [tw, th],
                solid,
            };
            vector_stencil::build_with(shapes, *detail, p, Some(&win)).map_err(fail)?
        }
        (Method::Cut(..), Source::Vector(_)) => {
            let whole = whole.ok_or("the whole plate was not built")?;
            let (o, d) = cut_tile(whole, p, l, (x0, x1, y0, y1)).map_err(fail)?;
            dropped = d;
            o
        }
        (Method::Raster(_), _) => {
            return Err("this layout was made from shapes: connect it to a Vector stencil".into())
        }
        (Method::Vector(..) | Method::Cut(..), _) => {
            return Err("this layout was made from a mask: connect it to a Stencil".into())
        }
    };
    if out.stats.islands_remaining != 0 || !out.stats.watertight {
        return Err(fail(
            "verification failed: the stencil has islands or is not watertight".into(),
        ));
    }
    let mut mesh = Mesh::from_stl(&out.stl)?;
    let c = &l.spec;
    let mut cutters = Mesh::default();
    for pk in &l.pockets {
        let f = pk.footprint(c);
        if !(f[0] < x1 - 1e-9 && f[2] > x0 + 1e-9 && f[1] < y1 - 1e-9 && f[3] > y0 + 1e-9) {
            continue;
        }
        let (cx, cy) = if pk.axis == 0 {
            (pk.s, pk.u)
        } else {
            (pk.u, pk.s)
        };
        let lx = if p.flip { tw - (cx - x0) } else { cx - x0 };
        for &z in zs {
            let mut r = tiles::rod(c.shape, c.hole_radius(), 2.0 * c.depth, c.segments, pk.axis);
            r.translate([lx, cy - y0, z]);
            cutters.append(&r);
        }
    }
    if !cutters.t.is_empty() {
        mesh = mesh.subtract(&cutters).map_err(fail)?;
    }
    mesh.quantize_f32();
    drop_slivers(&mut mesh);
    if !mesh.is_watertight() {
        return Err(fail("not watertight after drilling the pockets".into()));
    }
    let (pieces, of) = crate::tile_bridge::components(&mesh);
    if pieces != 1 {
        let mut boxes = vec![[f64::MAX, f64::MAX, f64::MIN, f64::MIN]; pieces];
        for (t, &k) in mesh.t.iter().zip(&of) {
            for &i in t {
                let v = mesh.v[i];
                boxes[k][0] = boxes[k][0].min(v[0]);
                boxes[k][1] = boxes[k][1].min(v[1]);
                boxes[k][2] = boxes[k][2].max(v[0]);
                boxes[k][3] = boxes[k][3].max(v[1]);
            }
        }
        boxes.sort_by(|a, b| {
            ((a[2] - a[0]) * (a[3] - a[1])).total_cmp(&((b[2] - b[0]) * (b[3] - b[1])))
        });
        let small: Vec<String> = boxes[..pieces - 1]
            .iter()
            .map(|b| format!("x {:.1}-{:.1}, y {:.1}-{:.1}", b[0], b[2], b[1], b[3]))
            .collect();
        return Err(fail(format!(
            "{pieces} separate pieces after drilling the pockets ({}): use fewer or smaller connectors",
            small.join("; ")
        )));
    }
    Ok(TileOut {
        tile: tiles::Tile {
            name,
            mesh,
            origin: [if p.flip { l.width - x1 } else { x0 }, y0],
            size: [tw, th],
        },
        islands: out.stats.islands_found,
        bridges: out.stats.bridge_pixels,
        fallback: out.fallback,
        top: out.stats.thickness_mm,
        dropped,
    })
}

/// The height of the printed top surface at a point (none where there is no material), from the upward faces.
fn top_at(tris: &[[[f64; 3]; 3]], x: f64, y: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    for t in tris {
        let d =
            (t[1][1] - t[2][1]) * (t[0][0] - t[2][0]) + (t[2][0] - t[1][0]) * (t[0][1] - t[2][1]);
        if d.abs() < 1e-12 {
            continue;
        }
        let a = ((t[1][1] - t[2][1]) * (x - t[2][0]) + (t[2][0] - t[1][0]) * (y - t[2][1])) / d;
        let b = ((t[2][1] - t[0][1]) * (x - t[2][0]) + (t[0][0] - t[2][0]) * (y - t[2][1])) / d;
        let c = 1.0 - a - b;
        if a >= -1e-9 && b >= -1e-9 && c >= -1e-9 {
            let z = a * t[0][2] + b * t[1][2] + c * t[2][2];
            if best.is_none_or(|v| z > v) {
                best = Some(z);
            }
        }
    }
    best
}

/// The length of seam (mm) where both neighbouring tiles have material at the cut but more than a layer apart in height, so
/// the assembled stencil has a step there.
fn seam_steps(l: &Layout, outs: &[TileOut], flip: bool, layer: f64) -> f64 {
    let (nx, ny) = (l.xs.len() - 1, l.ys.len() - 1);
    // outs run from the top row down, left to right
    let at = |i: usize, j: usize| &outs[(ny - 1 - j) * nx + i].tile;
    let strip = |t: &tiles::Tile, edge: usize| -> Vec<[[f64; 3]; 3]> {
        let (tw, th) = (t.size[0], t.size[1]);
        t.mesh
            .t
            .iter()
            .map(|f| [t.mesh.v[f[0]], t.mesh.v[f[1]], t.mesh.v[f[2]]])
            .filter(|f| {
                let up = (f[1][0] - f[0][0]) * (f[2][1] - f[0][1])
                    - (f[1][1] - f[0][1]) * (f[2][0] - f[0][0])
                    > 0.0;
                let (lo, hi) = (0..3).fold((f64::MAX, f64::MIN), |(lo, hi), k| {
                    let v = if edge < 2 { f[k][0] } else { f[k][1] };
                    (lo.min(v), hi.max(v))
                });
                let (near_lo, near_hi) = (lo < 0.5, hi > (if edge < 2 { tw } else { th }) - 0.5);
                // printed x is mirrored on a flipped plate
                let want_low = if edge < 2 {
                    (edge == 0) != flip
                } else {
                    edge == 2
                };
                up && if want_low { near_lo } else { near_hi }
            })
            .collect()
    };
    const INSET: f64 = 0.15;
    let mut steps = 0.0;
    let mut sample =
        |a: &[[[f64; 3]; 3]], pa: (f64, f64), b: &[[[f64; 3]; 3]], pb: (f64, f64), du: f64| {
            if let (Some(za), Some(zb)) = (top_at(a, pa.0, pa.1), top_at(b, pb.0, pb.1)) {
                if (za - zb).abs() > 1.5 * layer {
                    steps += du;
                }
            }
        };
    let px = |x: f64, w: f64| if flip { w - x } else { x };
    for i in 0..nx - 1 {
        for j in 0..ny {
            let (left, right) = (at(i, j), at(i + 1, j));
            let (sa, sb) = (strip(left, 1), strip(right, 0));
            let (y0, y1) = (l.ys[j], l.ys[j + 1]);
            let n = ((y1 - y0) / 0.25) as usize;
            for k in 0..n {
                let y = y0 + (k as f64 + 0.5) * (y1 - y0) / n as f64;
                sample(
                    &sa,
                    (px(left.size[0] - INSET, left.size[0]), y - y0),
                    &sb,
                    (px(INSET, right.size[0]), y - y0),
                    (y1 - y0) / n as f64,
                );
            }
        }
    }
    for j in 0..ny - 1 {
        for i in 0..nx {
            let (below, above) = (at(i, j), at(i, j + 1));
            let (sa, sb) = (strip(below, 3), strip(above, 2));
            let (x0, x1) = (l.xs[i], l.xs[i + 1]);
            let n = ((x1 - x0) / 0.25) as usize;
            for k in 0..n {
                let x = x0 + (k as f64 + 0.5) * (x1 - x0) / n as f64;
                let u = px(x - x0, x1 - x0);
                sample(
                    &sa,
                    (u, below.size[1] - INSET),
                    &sb,
                    (u, INSET),
                    (x1 - x0) / n as f64,
                );
            }
        }
    }
    steps
}

/// Builds every tile (in parallel where threads exist), drills the pockets and describes the plate.
pub fn build(l: &Layout, m: &Method) -> Result<Built, String> {
    let p = match m {
        Method::Raster(p) | Method::Vector(p, _) | Method::Cut(p, _) => *p,
    };
    let (nx, ny) = (l.xs.len() - 1, l.ys.len() - 1);
    let c = &l.spec;
    if nx * ny > 1 {
        tiles::check_connector(c.shape, c.diameter, p.thickness_mm)?;
    }
    let zs = tiles::row_heights(c.shape, c.diameter + c.clearance, p.thickness_mm);
    let cells: Vec<(usize, usize)> = (0..ny)
        .rev()
        .flat_map(|j| (0..nx).map(move |i| (i, j)))
        .collect();
    let whole = match (m, &l.source) {
        (Method::Cut(_, detail), Source::Vector(shapes)) => {
            let w = whole_plate(l, shapes, *detail, p)?;
            crate::progress::step("bridge the whole plate", 0.5);
            Some(w)
        }
        _ => None,
    };
    let results = crate::par::map(&cells, |&cell| {
        build_tile(l, m, p, cell, &zs, whole.as_ref())
    });
    let mut outs = Vec::new();
    for r in results {
        outs.push(r?);
    }
    let fallbacks: Vec<Json> = outs
        .iter()
        .filter_map(|o| {
            o.fallback
                .as_ref()
                .map(|f| json!({"tile": o.tile.name, "note": f}))
        })
        .collect();
    let info = json!({
        "tiles": outs.len(), "grid": [nx, ny], "connectors": l.pockets.len(),
        "rows": if l.pockets.is_empty() { 0 } else { zs.len() },
        "pre_tiled": true, "islands_bridged": outs.iter().map(|o| o.islands).sum::<usize>(),
        "bridges": outs.iter().map(|o| o.bridges).sum::<usize>(),
        "fallback_tiles": fallbacks,
        "tile_method": if matches!(m, Method::Cut(..)) { "cut_whole" } else { "per_tile" },
        "dropped_fragments": outs.iter().map(|o| o.dropped).sum::<usize>(),
        "seam_step_mm": (seam_steps(l, &outs, p.flip, p.layer_height_mm) * 10.0).round() / 10.0,
        "hole_depth_mm": c.depth, "rod_length_mm": tiles::rod_length(c.depth),
        "paint_under_blocks_mm2": (l.paint_under_blocks_mm2() * 10.0).round() / 10.0,
    });
    let top = outs.first().map_or(0.0, |o| o.top);
    // plate coordinates of the printed tiles: a flipped plate is mirrored about its width
    let fx = |x: f64| if p.flip { l.width - x } else { x };
    let mut seams = Vec::new();
    for &s in &l.xs[1..nx] {
        for j in 0..ny {
            seams.push([0.0, fx(s), l.ys[j], l.ys[j + 1]]);
        }
    }
    for &s in &l.ys[1..ny] {
        for i in 0..nx {
            let (a, b) = (fx(l.xs[i]), fx(l.xs[i + 1]));
            seams.push([1.0, s, a.min(b), a.max(b)]);
        }
    }
    let connectors = l
        .pockets
        .iter()
        .map(|pk| {
            let f = pk.footprint(c);
            let (a, b) = (fx(f[0]), fx(f[2]));
            [a.min(b), f[1], a.max(b), f[3]]
        })
        .collect();
    Ok(Built {
        tiles: outs.into_iter().map(|o| o.tile).collect(),
        seams,
        connectors,
        info,
        top,
        layer: p.layer_height_mm,
    })
}

impl Layout {
    /// The artwork (mm²) the reserved blocks interrupt.
    pub fn paint_under_blocks_mm2(&self) -> f64 {
        self.pockets
            .iter()
            .map(|pk| self.cover.paint_mm2(pk.block(&self.spec)))
            .sum()
    }

    /// All the artwork (mm²) of the plate.
    pub fn paint_mm2(&self) -> f64 {
        self.cover.paint.iter().filter(|&&p| p).count() as f64 * self.cover.cw * self.cover.ch
    }

    /// A picture of the plan: paint dark, the blocks blue (orange where they cover paint), the seams red.
    pub fn preview(&self) -> (usize, usize, Vec<u8>) {
        let k = 1000.0 / self.width.max(self.height);
        let (w, h) = (
            ((self.width * k).ceil() as usize).max(1),
            ((self.height * k).ceil() as usize).max(1),
        );
        let blocks: Vec<[f64; 4]> = self.pockets.iter().map(|p| p.block(&self.spec)).collect();
        let near = |v: f64, g: &[f64]| g[1..g.len() - 1].iter().any(|&s| (v - s).abs() < 0.75 / k);
        let mut rgb = Vec::with_capacity(w * h * 3);
        for py in 0..h {
            for px in 0..w {
                let (x, y) = ((px as f64 + 0.5) / k, self.height - (py as f64 + 0.5) / k);
                let px_rgb = if near(x, &self.xs) || near(y, &self.ys) {
                    [220, 40, 40]
                } else if blocks
                    .iter()
                    .any(|b| x >= b[0] && x < b[2] && y >= b[1] && y < b[3])
                {
                    if self.cover.at(x, y) {
                        [240, 150, 40]
                    } else {
                        [70, 120, 230]
                    }
                } else if self.cover.at(x, y) {
                    [40, 40, 40]
                } else {
                    [225, 225, 225]
                };
                rgb.extend_from_slice(&px_rgb);
            }
        }
        (w, h, rgb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> Params {
        Params {
            width_mm: 120.0,
            thickness_mm: 6.0,
            margin_mm: 10.0,
            bridge_mm: 2.0,
            ..Params::default()
        }
    }

    fn spec() -> Connectors {
        Connectors {
            shape: Shape::Hex,
            diameter: 3.0,
            clearance: 0.2,
            depth: 10.0,
            segments: 32,
            spacing: 30.0,
        }
    }

    /// A ring with a disc inside (an island) centred on the plate, and a bar across the middle seam.
    fn art(n: usize) -> Vec<bool> {
        let c = n as f64 / 2.0;
        (0..n * n)
            .map(|i| {
                let (x, y) = ((i % n) as f64 - c, (i / n) as f64 - c);
                let d = x.hypot(y);
                (d > c * 0.55 && d < c * 0.8) || d < c * 0.25 || (y.abs() < 3.0 && x > c * 0.1)
            })
            .collect()
    }

    fn lp() -> LayoutParams {
        LayoutParams {
            width: 120.0,
            margin: 10.0,
            max_x: 70.0,
            max_y: 70.0,
            spec: spec(),
            seam_shift: 0.0,
        }
    }

    fn check(b: &Built, tiles: usize) {
        assert_eq!(b.tiles.len(), tiles);
        for t in &b.tiles {
            assert!(t.mesh.is_watertight(), "{}", t.name);
            assert_eq!(crate::tile_bridge::components(&t.mesh).0, 1, "{}", t.name);
        }
    }

    #[test]
    fn a_raster_plate_is_cut_into_tiles_that_each_have_no_islands() {
        let l = from_mask(&art(120), 120, 120, &lp()).unwrap();
        assert_eq!((l.xs.len(), l.ys.len()), (3, 3));
        assert!(l.pockets.len() >= 4);
        for flip in [false, true] {
            let p = Params { flip, ..params() };
            let b = build(&l, &Method::Raster(&p)).unwrap();
            check(&b, 4);
            assert_eq!(b.connectors.len(), l.pockets.len());
        }
    }

    #[test]
    fn a_seam_may_move_to_cover_less_paint_without_breaking_the_tile_limits() {
        let paint = art(120);
        let under = |l: &Layout| {
            l.pockets
                .iter()
                .map(|p| l.cover.paint_mm2(p.block(&l.spec)))
                .sum::<f64>()
        };
        let even = from_mask(&paint, 120, 120, &lp()).unwrap();
        let moved = from_mask(
            &paint,
            120,
            120,
            &LayoutParams {
                seam_shift: 10.0,
                ..lp()
            },
        )
        .unwrap();
        assert!(under(&moved) <= under(&even) + 1e-6);
        for g in [&moved.xs, &moved.ys] {
            assert!(g
                .windows(2)
                .all(|v| v[1] - v[0] <= 70.0 + 1e-6 && v[1] - v[0] >= tiles::min_tile(10.0)));
        }
        let p = params();
        let b = build(&moved, &Method::Raster(&p)).unwrap();
        check(&b, 4);
        assert!(b.info["seam_step_mm"].as_f64().is_some());
    }

    #[test]
    fn fragments_no_thicker_than_rounding_are_dropped_but_real_pieces_stay() {
        let mut m = Mesh::default();
        let mut add = |o: [f64; 3], s: f64| {
            let b = m.v.len();
            m.v.extend([
                o,
                [o[0] + s, o[1], o[2]],
                [o[0], o[1] + s, o[2]],
                [o[0], o[1], o[2] + 3.0],
            ]);
            m.t.extend([
                [b, b + 2, b + 1],
                [b, b + 1, b + 3],
                [b + 1, b + 2, b + 3],
                [b + 2, b, b + 3],
            ]);
        };
        add([0.0; 3], 5.0);
        add([20.0, 0.0, 0.0], 0.001);
        drop_slivers(&mut m);
        assert_eq!(crate::tile_bridge::components(&m).0, 1);
        assert_eq!(m.t.len(), 4);
    }

    #[test]
    fn a_vector_plate_is_cut_into_tiles_that_each_have_no_islands() {
        let paint = art(120);
        let shapes = crate::trace::trace(
            &paint,
            120,
            120,
            &crate::trace::TraceParams {
                smooth_px: 1.0,
                corner_deg: 60.0,
                tolerance_px: 0.4,
                min_area_px: 4.0,
            },
        );
        let l = from_shapes(&shapes, &lp()).unwrap();
        for flip in [false, true] {
            let p = Params { flip, ..params() };
            let b = build(&l, &Method::Vector(&p, 1.0)).unwrap();
            check(&b, 4);
        }
    }

    #[test]
    fn a_plate_cut_from_the_whole_has_watertight_single_piece_tiles() {
        let paint = art(120);
        let shapes = crate::trace::trace(
            &paint,
            120,
            120,
            &crate::trace::TraceParams {
                smooth_px: 1.0,
                corner_deg: 60.0,
                tolerance_px: 0.4,
                min_area_px: 4.0,
            },
        );
        let l = from_shapes(&shapes, &lp()).unwrap();
        for flip in [false, true] {
            let p = Params { flip, ..params() };
            let b = build(&l, &Method::Cut(&p, 1.0)).unwrap();
            check(&b, 4);
            assert_eq!(b.info["tile_method"], "cut_whole");
            assert!(b.info["seam_step_mm"].is_number());
        }
    }

    #[test]
    fn a_small_plate_stays_whole() {
        let mut l = lp();
        l.max_x = 0.0;
        l.max_y = 0.0;
        let l = from_mask(&art(120), 120, 120, &l).unwrap();
        let b = build(&l, &Method::Raster(&params())).unwrap();
        check(&b, 1);
        assert!(b.seams.is_empty() && b.connectors.is_empty());
    }

    #[test]
    fn tiles_too_narrow_for_the_pockets_are_refused() {
        let mut l = lp();
        l.max_x = 20.0;
        assert!(from_mask(&art(120), 120, 120, &l)
            .err()
            .is_some_and(|e| e.contains("at least")));
    }
}
