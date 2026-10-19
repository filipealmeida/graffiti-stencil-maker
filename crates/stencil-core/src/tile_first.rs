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
    let xs = snap(tiles::grid(width, lp.max_x), w);
    let ys = snap(tiles::grid(height, lp.max_y), h);
    let cover = Cover {
        paint: full.clone(),
        gw: w,
        gh: h,
        x0: 0.0,
        y_top: height,
        cw: cell,
        ch: cell,
    };
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
        return Err("vector input contains no closed outlines".into());
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
    let xs = tiles::grid(lp.width, lp.max_x);
    let ys = tiles::grid(height, lp.max_y);
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
    let mut pockets: Vec<Pocket> = Vec::new();
    let mut blocks: Vec<[f64; 4]> = Vec::new();
    for (axis, s, a, b) in seams {
        let len = b - a;
        if len < 2.0 * hw + 1.0 {
            return Err(format!(
                "a seam segment is only {len:.1} mm long: it cannot hold a connector pocket"
            ));
        }
        // keep clear of the ends, where another seam or the plate edge is
        let inset = (reach + hw + 0.5).min(len / 2.0 - 1e-6).max(hw);
        let n = ((len / c.spacing.max(5.0) - 1e-9).ceil() as usize).max(1);
        let cell = len / n as f64;
        for k in 0..n {
            let mid = a + cell * (k as f64 + 0.5);
            let lo = (a + inset).max(a + cell * k as f64 + hw);
            let hi = (b - inset).min(a + cell * (k + 1) as f64 - hw);
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

/// How the tiles are built: the stencil parameters of the plate, with the mesh detail for outlines.
pub enum Method<'a> {
    Raster(&'a Params),
    Vector(&'a Params, f64),
}

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
}

fn build_tile(
    l: &Layout,
    m: &Method,
    p: &Params,
    (i, j): (usize, usize),
    zs: &[f64],
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
        (Method::Raster(_), _) => {
            return Err("this layout was made from shapes: connect it to a Vector stencil".into())
        }
        (Method::Vector(..), _) => {
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
    if !mesh.is_watertight() {
        return Err(fail("not watertight after drilling the pockets".into()));
    }
    let pieces = crate::tile_bridge::components(&mesh).0;
    if pieces != 1 {
        return Err(fail(format!(
            "{pieces} separate pieces after drilling the pockets: use fewer or smaller connectors"
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
    })
}

/// Builds every tile (in parallel where threads exist), drills the pockets and describes the plate.
pub fn build(l: &Layout, m: &Method) -> Result<Built, String> {
    let p = match m {
        Method::Raster(p) | Method::Vector(p, _) => *p,
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
    let results = crate::par::map(&cells, |&cell| build_tile(l, m, p, cell, &zs));
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
        "hole_depth_mm": c.depth, "rod_length_mm": tiles::rod_length(c.depth),
        "paint_under_blocks_mm2": (l.pockets.iter().map(|pk| l.cover.paint_mm2(pk.block(c))).sum::<f64>() * 10.0).round() / 10.0,
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
    /// A picture of the plan: paint dark, the blocks blue, the seams red.
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
                    [70, 120, 230]
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
