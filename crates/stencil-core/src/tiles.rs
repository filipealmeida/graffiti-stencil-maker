//! Splits a plate into printable tiles joined by glued rods in blind seam holes.
use crate::mesh::{Mesh, P3};

pub const DIAMETERS: [f64; 5] = [2.5, 3.0, 4.0, 5.0, 6.0];
pub const MIN_HEIGHT_MM: f64 = 4.0;
pub const WALL_MM: f64 = 1.0;
pub const THIN_WALL_MM: f64 = 0.6;
pub const ROW_GAP_MM: f64 = 1.5;
pub const MAX_ROWS: usize = 4;
pub const SPACING_TARGET_MM: f64 = 30.0;
pub const POCKET_DEPTH_MM: f64 = 10.0;

/// A rod sits this much short of the full pocket depth, so the glue has room.
pub fn rod_length(depth: f64) -> f64 {
    2.0 * depth - 1.0
}

/// Adds the seam gap to the rod while retaining the same total glue clearance.
pub fn rod_length_with_gap(depth: f64, gap: f64) -> f64 {
    rod_length(depth) + gap
}
/// Tiles narrower than this cannot hold pockets of the given depth.
pub fn min_tile(depth: f64) -> f64 {
    depth / 0.4
}

#[derive(Clone, Copy, PartialEq)]
pub enum Shape {
    Hex,
    Round,
}

pub struct TileParams {
    pub max_x: f64,
    pub max_y: f64,
    pub shape: Shape,
    pub diameter: f64,
    /// How deep a connector sinks into each of the two tiles (mm).
    pub depth: f64,
    pub clearance: f64,
    /// Total material removed at each internal seam, split evenly between adjacent tiles (mm).
    pub seam_gap: f64,
    /// Tilt of every seam face from vertical (degrees); 0 cuts straight down, a larger angle cuts a bevel.
    pub seam_angle_deg: f64,
    pub segments: usize,
    /// Plate thickness without raised pads; 0 uses the mesh height.
    pub thickness: f64,
    /// Bridge rules of the source stencil; pieces cut off from their tile are bridged with them.
    pub bridge: Option<crate::tile_bridge::BridgeParams>,
    /// How far (mm) a seam may move to cross more material; 0 keeps the even grid.
    pub shift_mm: f64,
    /// Circles (x, y, radius) in the solid's frame that seams and connector holes keep clear of: pads and holes.
    pub avoid: Vec<(f64, f64, f64)>,
    /// Where no connector fits in the artwork, add a block of material at the seam to hold one.
    pub reinforce: bool,
}

pub struct Tile {
    pub name: String,
    pub mesh: Mesh,
    /// Lower-left corner of the tile in the input solid's frame (the mesh itself is moved to start at 0, 0).
    pub origin: [f64; 2],
    pub size: [f64; 2],
}

pub struct Tiled {
    pub tiles: Vec<Tile>,
    pub rod: Option<Mesh>,
    pub info: serde_json::Value,
    /// Seam segments `[axis, position, from, to]`; axis 0 is a line of constant x.
    pub seams: Vec<[f64; 4]>,
    /// Plan footprints `[x0, y0, x1, y1]` of the connector holes.
    pub connectors: Vec<[f64; 4]>,
}

/// Unbridgeable pieces up to this size (mm) are dropped from a tile.
const SPECK_MM: f64 = 4.0;

/// One strip of material from every piece larger than a speck to the main piece, along the shortest link between
/// their faces on the bridge plane. The strip lies against that plane and is half the plate thick.
fn weld_pieces(
    m: &Mesh,
    count: usize,
    of: &[usize],
    bp: &crate::tile_bridge::BridgeParams,
    width: f64,
) -> Mesh {
    let plane = if bp.flip { 0.0 } else { bp.plate_top };
    let mut tris = vec![0usize; count];
    for &o in of {
        tris[o] += 1;
    }
    let main = (0..count).max_by_key(|&c| tris[c]).unwrap_or(0);
    let mut on_plane: Vec<Vec<[f64; 2]>> = vec![Vec::new(); count];
    for (t, &o) in m.t.iter().zip(of) {
        for &vi in t {
            let v = m.v[vi];
            if (v[2] - plane).abs() < 1e-3 {
                on_plane[o].push([v[0], v[1]]);
            }
        }
    }
    // A piece cut by a leaning seam can start above the plane (a wedge); it is then linked from the face nearest it.
    for (c, pts) in on_plane.iter_mut().enumerate() {
        if !pts.is_empty() {
            continue;
        }
        let near = |z: f64| if bp.flip { z } else { -z };
        let best =
            m.t.iter()
                .zip(of)
                .filter(|(_, &o)| o == c)
                .flat_map(|(t, _)| t.iter().map(|&vi| m.v[vi]))
                .map(|v| near(v[2]))
                .fold(f64::MAX, f64::min);
        for (t, &o) in m.t.iter().zip(of) {
            if o == c {
                pts.extend(
                    t.iter()
                        .map(|&vi| m.v[vi])
                        .filter(|v| near(v[2]) < best + 0.5)
                        .map(|v| [v[0], v[1]]),
                );
            }
        }
    }
    let half = bp.plate_top / 2.0;
    let (z0, z1) = if bp.flip {
        (0.0, half)
    } else {
        (bp.plate_top - half, bp.plate_top)
    };
    let mut out = Mesh::default();
    for c in (0..count).filter(|&c| c != main) {
        let size = |pts: &[[f64; 2]]| {
            let ext = |k: usize| {
                let lo = pts.iter().map(|p| p[k]).fold(f64::MAX, f64::min);
                let hi = pts.iter().map(|p| p[k]).fold(f64::MIN, f64::max);
                hi - lo
            };
            ext(0).max(ext(1))
        };
        if on_plane[c].is_empty() || size(&on_plane[c]) <= SPECK_MM {
            continue;
        }
        let mut best: Option<(f64, [f64; 2], [f64; 2])> = None;
        for a in &on_plane[c] {
            for b in &on_plane[main] {
                let d = (a[0] - b[0]).hypot(a[1] - b[1]);
                if best.is_none_or(|x| d < x.0) {
                    best = Some((d, *a, *b));
                }
            }
        }
        let Some((d, a, b)) = best else { continue };
        let (ux, uy) = if d > 1e-9 {
            ((b[0] - a[0]) / d, (b[1] - a[1]) / d)
        } else {
            (1.0, 0.0)
        };
        let reach = width * 0.6;
        let (a, b) = (
            [a[0] - ux * reach, a[1] - uy * reach],
            [b[0] + ux * reach, b[1] + uy * reach],
        );
        let (nx, ny) = (-uy * width / 2.0, ux * width / 2.0);
        out.append(&Mesh::extrude_convex(
            &[
                [a[0] - nx, a[1] - ny],
                [b[0] - nx, b[1] - ny],
                [b[0] + nx, b[1] + ny],
                [a[0] + nx, a[1] + ny],
            ],
            z0,
            z1,
        ));
    }
    out
}

fn z_extent(shape: Shape, d: f64) -> f64 {
    if shape == Shape::Hex {
        3f64.sqrt() / 2.0 * d
    } else {
        d
    }
}

pub fn grid(len: f64, max: f64) -> Vec<f64> {
    let n = if max > 0.0 {
        ((len / max - 1e-9).ceil() as usize).max(1)
    } else {
        1
    };
    (0..=n).map(|i| len * i as f64 / n as f64).collect()
}

/// Cross-section area of the solid at each of the (ascending) planes `coordinate[axis] = s`.
fn section_areas(solid: &Mesh, axis: usize, planes: &[f64]) -> Vec<f64> {
    let u = 1 - axis;
    let mut area = vec![0.0; planes.len()];
    for t in &solid.t {
        let v = [solid.v[t[0]], solid.v[t[1]], solid.v[t[2]]];
        let lo = v.iter().map(|p| p[axis]).fold(f64::MAX, f64::min);
        let hi = v.iter().map(|p| p[axis]).fold(f64::MIN, f64::max);
        let from = planes.partition_point(|&s| s <= lo);
        for (k, &s) in planes.iter().enumerate().skip(from) {
            if s >= hi {
                break;
            }
            let d = [v[0][axis] - s, v[1][axis] - s, v[2][axis] - s];
            let (mut start, mut end) = (None, None);
            for i in 0..3 {
                let j = (i + 1) % 3;
                if (d[i] < 0.0) == (d[j] < 0.0) {
                    continue;
                }
                let f = d[i] / (d[i] - d[j]);
                let pt = (
                    v[i][u] + f * (v[j][u] - v[i][u]),
                    v[i][2] + f * (v[j][2] - v[i][2]),
                );
                if d[i] < 0.0 {
                    start = Some(pt);
                } else {
                    end = Some(pt);
                }
            }
            if let (Some(a), Some(b)) = (start, end) {
                area[k] += 0.5 * (a.0 * b.1 - b.0 * a.1);
            }
        }
    }
    area.iter().map(|a| a.abs()).collect()
}

/// The seam lines of one axis, each moved by up to `p.shift_mm` to where the plate has the most material across it
/// (more room for connectors), as long as the tiles stay within the maximum and clear of the `avoid` circles.
fn shift_seams(
    solid: &Mesh,
    axis: usize,
    lo: f64,
    even: &[f64],
    max: f64,
    p: &TileParams,
) -> Result<Vec<f64>, String> {
    let mut out: Vec<f64> = even.iter().map(|x| lo + x).collect();
    let n = even.len() - 1;
    if n < 2 {
        return Ok(out);
    }
    let len = even[n];
    let slack = if max > 0.0 {
        (max - len / n as f64) / 2.0
    } else {
        0.0
    };
    let reach = p.shift_mm.min(slack).max(0.0);
    let cuts = |s: f64| {
        p.avoid
            .iter()
            .any(|&(x, y, r)| ((if axis == 0 { x } else { y }) - s).abs() < r + 1.0)
    };
    let steps = (reach / 0.5).floor() as i64;
    #[allow(clippy::needless_range_loop)]
    for k in 1..n {
        let nominal = out[k];
        let cand: Vec<f64> = (-steps..=steps).map(|i| nominal + i as f64 * 0.5).collect();
        // a hair off the grid keeps vertices of axis-aligned art from lying exactly on a plane
        let probe: Vec<f64> = cand.iter().map(|c| c + 1.37e-4).collect();
        let area = if steps == 0 {
            vec![0.0]
        } else {
            section_areas(solid, axis, &probe)
        };
        let base = area[area.len() / 2];
        let mut best: Option<(usize, f64)> = None;
        for (i, &c) in cand.iter().enumerate() {
            if cuts(c) {
                continue;
            }
            let better = best.is_none_or(|(_, a)| area[i] > a * 1.0000001);
            if better {
                best = Some((i, area[i]));
            }
        }
        let Some((i, a)) = best else {
            return Err(format!("a seam at {nominal:.0} mm would cut through a pad or hole: choose a different tile size"));
        };
        // a move has to be worth it: more than 5 % more material, unless the even line cuts a pad
        if cuts(nominal) || a > base * 1.05 {
            out[k] = cand[i];
        }
    }
    Ok(out)
}

pub(crate) fn row_heights(shape: Shape, d: f64, t: f64) -> Vec<f64> {
    let ext = z_extent(shape, d);
    let n = (((t - 2.0 * WALL_MM + ROW_GAP_MM) / (ext + ROW_GAP_MM)).floor() as usize)
        .clamp(1, MAX_ROWS);
    if n == 1 {
        return vec![t / 2.0];
    }
    let (lo, hi) = (WALL_MM + ext / 2.0, t - WALL_MM - ext / 2.0);
    (0..n)
        .map(|i| lo + i as f64 * (hi - lo) / (n - 1) as f64)
        .collect()
}

pub(crate) fn rod(shape: Shape, r: f64, len: f64, segments: usize, axis: usize) -> Mesh {
    let n = if shape == Shape::Hex {
        6
    } else {
        segments.max(12)
    };
    Mesh::prism_z(n, r, -len / 2.0, len / 2.0).along(axis)
}

pub fn connector_rod(shape: Shape, d: f64, depth: f64, segments: usize) -> Mesh {
    connector_rod_with_gap(shape, d, depth, 0.0, segments)
}

pub fn connector_rod_with_gap(shape: Shape, d: f64, depth: f64, gap: f64, segments: usize) -> Mesh {
    let len = rod_length_with_gap(depth, gap);
    let mut m = rod(shape, d / 2.0, len, segments, 2);
    m.translate([0.0, 0.0, len / 2.0]);
    m
}

pub fn check(p: &TileParams, height: f64) -> Result<(), String> {
    check_connector(p.shape, p.diameter, height)
}

pub fn check_connector(shape: Shape, diameter: f64, height: f64) -> Result<(), String> {
    if !DIAMETERS.contains(&diameter) {
        return Err("connector diameter must be 2.5, 3, 4, 5 or 6 mm".into());
    }
    if height <= MIN_HEIGHT_MM {
        return Err(format!(
            "connectors need a plate thicker than {MIN_HEIGHT_MM} mm (it is {height:.1} mm)"
        ));
    }
    let need = z_extent(shape, diameter) + 2.0 * WALL_MM;
    if need > height + 1e-9 {
        return Err(format!("a connector of diameter {diameter} mm needs a plate at least {need:.1} mm thick (it is {height:.1} mm)"));
    }
    Ok(())
}

struct Hole {
    lo: P3,
    hi: P3,
}

fn overlaps(a: &Hole, b: &Hole) -> bool {
    (0..3).all(|k| a.lo[k] < b.hi[k] && b.lo[k] < a.hi[k])
}

/// The numbers every stage of the cut shares.
#[derive(Clone, Copy)]
struct Geometry {
    bevel: Bevel,
    r_hole: f64,
    /// Pocket depth on each side of the seam, lengthened to cover the bevel.
    depth: f64,
    pocket_len: f64,
    top: f64,
}

/// A seam face leans by `lean` mm per mm of height, about the mid-thickness. A horizontal rod meets it at an
/// angle, so each pocket must reach `r_hole * lean` further, and the gap measured across the face is longer
/// along the rod.
#[derive(Clone, Copy)]
struct Bevel {
    lean: f64,
    secant: f64,
    z_mid: f64,
}

impl Bevel {
    fn new(angle_deg: f64, thickness: f64) -> Self {
        Bevel {
            lean: angle_deg.to_radians().tan(),
            secant: 1.0 / angle_deg.to_radians().cos(),
            z_mid: thickness / 2.0,
        }
    }

    /// Where the seam `s` of `axis` passes at height z, as a point on the plate.
    fn on_seam(&self, axis: usize, s: f64, u: f64, z: f64) -> P3 {
        let s = s + self.lean * (z - self.z_mid);
        if axis == 0 {
            [s, u, z]
        } else {
            [u, s, z]
        }
    }
}

/// The connector pockets, reinforcing blocks and the counts of how they went.
struct Connectors {
    holes: Vec<Hole>,
    cutters: Mesh,
    blocks: Mesh,
    thin: usize,
    missed: usize,
    reinforced: usize,
    added_volume: f64,
}

/// Places connector holes along every seam (`(axis the hole runs along, seam coordinate, u range along the seam)`),
/// reinforcing a seam with a block where none fits.
fn place_connectors(
    solid: &Mesh,
    seams: &[(usize, f64, f64, f64)],
    zs: &[f64],
    p: &TileParams,
    g: &Geometry,
) -> Result<Connectors, String> {
    let Geometry {
        bevel,
        r_hole,
        depth,
        pocket_len,
        top,
    } = *g;
    let lean = bevel.lean;
    let on_seam = |axis: usize, s: f64, u: f64, z: f64| bevel.on_seam(axis, s, u, z);
    let mut holes: Vec<Hole> = Vec::new();
    let mut cutters = Mesh::default();
    let (mut thin, mut missed) = (0usize, 0usize);
    let mut blocks = Mesh::default();
    let (mut reinforced, mut added_volume) = (0usize, 0.0f64);
    for &(axis, s, a, b) in seams {
        // work on the strip around the seam only: cheaper booleans
        let reach = depth + 1.0 + lean * (top + 2.0) / 2.0;
        let (cl, ch): (P3, P3) = if axis == 0 {
            ([s - reach, a - 1.0, -1.0], [s + reach, b + 1.0, top + 1.0])
        } else {
            ([a - 1.0, s - reach, -1.0], [b + 1.0, s + reach, top + 1.0])
        };
        let strip = solid.intersect(&Mesh::cuboid(cl, ch))?;
        let n = (((b - a) / SPACING_TARGET_MM - 1e-9).ceil() as usize).max(1);
        let cell = (b - a) / n as f64;
        let mut placed_here = 0;
        for k in 0..n {
            let nominal = a + cell * (k as f64 + 0.5);
            for &z in zs {
                let reach = (cell / 2.0 - r_hole - WALL_MM).max(0.0);
                let mut offs: Vec<f64> = vec![0.0];
                let mut o = 1.5;
                while o <= reach {
                    offs.extend([o, -o]);
                    o += 1.5;
                }
                let mut done = false;
                'cand: for &wall in &[WALL_MM, THIN_WALL_MM] {
                    for &off in &offs {
                        let u = nominal + off;
                        let at = |r: f64, len: f64| -> Mesh {
                            let mut m = rod(p.shape, r, len, p.segments, axis);
                            m.translate(on_seam(axis, s, u, z));
                            m
                        };
                        let guard = at(r_hole + wall, pocket_len);
                        let (gl, gh) = guard.bounds();
                        let near = Hole {
                            lo: [
                                gl[0] - ROW_GAP_MM / 2.0,
                                gl[1] - ROW_GAP_MM / 2.0,
                                gl[2] - ROW_GAP_MM / 2.0,
                            ],
                            hi: [
                                gh[0] + ROW_GAP_MM / 2.0,
                                gh[1] + ROW_GAP_MM / 2.0,
                                gh[2] + ROW_GAP_MM / 2.0,
                            ],
                        };
                        let body = Hole { lo: gl, hi: gh };
                        if holes.iter().any(|h| overlaps(h, &near)) {
                            continue;
                        }
                        if p.avoid.iter().any(|&(x, y, r)| {
                            let (dx, dy) =
                                ((x.clamp(gl[0], gh[0])) - x, (y.clamp(gl[1], gh[1])) - y);
                            dx.hypot(dy) < r + 0.5
                        }) {
                            continue;
                        }
                        if !strip.box_inside(gl, gh) {
                            continue;
                        }
                        cutters.append(&at(r_hole, pocket_len));
                        holes.push(body);
                        thin += (wall < WALL_MM) as usize;
                        placed_here += 1;
                        done = true;
                        break 'cand;
                    }
                }
                if !done {
                    missed += 1;
                }
            }
        }
        if placed_here == 0 && p.reinforce {
            let reach = r_hole + WALL_MM;
            let halves: Vec<(f64, f64)> = if b - a >= 2.0 * SPACING_TARGET_MM {
                vec![(a, (a + b) / 2.0), ((a + b) / 2.0, b)]
            } else {
                vec![(a, b)]
            };
            for (ha, hb) in halves {
                let z = zs[zs.len() / 2];
                // the spot where the block replaces the least void, i.e. covers the most existing material
                let mut best: Option<(f64, f64)> = None;
                let mut u = ha + reach;
                while u <= hb - reach + 1e-9 {
                    let mut m = rod(p.shape, reach, pocket_len, p.segments, axis);
                    m.translate(on_seam(axis, s, u, z));
                    let (gl, gh) = m.bounds();
                    let near = Hole {
                        lo: gl.map(|v| v - ROW_GAP_MM / 2.0),
                        hi: gh.map(|v| v + ROW_GAP_MM / 2.0),
                    };
                    let clear = !holes.iter().any(|h| overlaps(h, &near))
                        && !p.avoid.iter().any(|&(x, y, r)| {
                            (x.clamp(gl[0], gh[0]) - x).hypot(y.clamp(gl[1], gh[1]) - y) < r + 0.5
                        });
                    if clear {
                        let kept = strip
                            .intersect(&Mesh::cuboid(gl, gh))
                            .map_or(0.0, |m| m.volume());
                        if best.is_none_or(|(_, v)| kept > v) {
                            best = Some((u, kept));
                        }
                    }
                    u += 2.0;
                }
                let Some((u, _)) = best else {
                    continue;
                };
                let at = |r: f64| -> Mesh {
                    let mut m = rod(p.shape, r, pocket_len, p.segments, axis);
                    m.translate(on_seam(axis, s, u, z));
                    m
                };
                let (gl, gh) = at(reach).bounds();
                let block = Mesh::cuboid(gl, gh);
                added_volume +=
                    block.volume() - strip.intersect(&block).map_or(0.0, |m| m.volume());
                blocks.append(&block);
                cutters.append(&at(r_hole));
                holes.push(Hole { lo: gl, hi: gh });
                reinforced += 1;
                placed_here += 1;
            }
        }
        if placed_here == 0 {
            return Err(format!("no room for a connector hole on the {} seam at {:.1} mm: use a smaller connector, larger tiles or a thicker plate", if axis == 0 { "vertical" } else { "horizontal" }, s));
        }
    }
    Ok(Connectors {
        holes,
        cutters,
        blocks,
        thin,
        missed,
        reinforced,
        added_volume,
    })
}

/// How many tiles needed each kind of repair, counted while the tiles are cut in parallel.
#[derive(Default)]
struct Counts {
    bridged: std::sync::atomic::AtomicUsize,
    dropped: std::sync::atomic::AtomicUsize,
    welded: std::sync::atomic::AtomicUsize,
    loose: std::sync::atomic::AtomicUsize,
    loose_tiles: std::sync::atomic::AtomicUsize,
}

/// Cuts one tile out of the drilled plate: the sides (leaning for a bevel), bridges, welds and specks.
struct Cutter<'a> {
    p: &'a TileParams,
    xs: &'a [f64],
    ys: &'a [f64],
    bevel: Bevel,
    top: f64,
    drilled: &'a Mesh,
    holes: &'a [Hole],
    cutters: &'a Mesh,
    count: Counts,
}

impl Cutter<'_> {
    fn cut(&self, &(i, j): &(usize, usize)) -> Result<Option<Tile>, String> {
        let Cutter {
            p,
            xs,
            ys,
            bevel,
            top,
            drilled,
            holes,
            cutters,
            count,
        } = self;
        let (nx, ny, top) = (xs.len() - 1, ys.len() - 1, *top);
        let (lean, secant, z_mid) = (bevel.lean, bevel.secant, bevel.z_mid);
        // each internal side is a plane leaning by `lean`, set back by half the gap measured across the face
        let inset = p.seam_gap / 2.0 * secant;
        let rect = |z: f64| -> [[f64; 2]; 4] {
            let d = lean * (z - z_mid);
            let xa = if i > 0 {
                xs[i] + inset + d
            } else {
                xs[i] - 1.0
            };
            let xb = if i < nx - 1 {
                xs[i + 1] - inset + d
            } else {
                xs[i + 1] + 1.0
            };
            let ya = if j > 0 {
                ys[j] + inset + d
            } else {
                ys[j] - 1.0
            };
            let yb = if j < ny - 1 {
                ys[j + 1] - inset + d
            } else {
                ys[j + 1] + 1.0
            };
            [[xa, ya], [xb, ya], [xb, yb], [xa, yb]]
        };
        let bbox = Mesh::loft_quads(rect(-1.0), -1.0, rect(top + 1.0), top + 1.0);
        let mut m = drilled.intersect(&bbox)?;
        if m.t.is_empty() {
            return Ok(None);
        }
        if let Some(bp) = &p.bridge {
            if crate::tile_bridge::components(&m).0 > 1 {
                let (ml, mh) = m.bounds();
                // bridges stay within the tile: its bounding box and its (leaning) sides
                let clip = Mesh::cuboid([ml[0], ml[1], -1.0], [mh[0], mh[1], top + 1.0])
                    .intersect(&bbox)?;
                let avoid: Vec<(P3, P3)> = holes
                    .iter()
                    .filter(|h| {
                        h.lo[0] < mh[0] && h.hi[0] > ml[0] && h.lo[1] < mh[1] && h.hi[1] > ml[1]
                    })
                    .map(|h| (h.lo, h.hi))
                    .collect();
                if let Some(b) = crate::tile_bridge::bridge_mesh(&m, &avoid, bp)? {
                    m = m.union(&b)?.intersect(&clip)?;
                    if !cutters.t.is_empty() {
                        m = m.subtract(cutters)?;
                    }
                    count
                        .bridged
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                for attempt in 0..3 {
                    if crate::tile_bridge::components(&m).0 < 2 {
                        break;
                    }
                    // pieces the first pass could not reach: bridge again, then with the pockets allowed
                    let allowed = if attempt == 0 { &avoid[..] } else { &[] };
                    let Some(b) = crate::tile_bridge::bridge_mesh(&m, allowed, bp)? else {
                        break;
                    };
                    m = m.union(&b)?.intersect(&clip)?;
                    if !cutters.t.is_empty() {
                        m = m.subtract(cutters)?;
                    }
                }
                // Pieces that touch the rest only across a gap finer than the raster never get a bridge of
                // their own; they are welded to the main piece with a strip along their shortest link.
                for _ in 0..3 {
                    let (left, of) = crate::tile_bridge::components(&m);
                    if left < 2 {
                        break;
                    }
                    let welds = weld_pieces(&m, left, &of, bp, bp.bridge_mm);
                    if welds.t.is_empty() {
                        break;
                    }
                    m = m.union(&welds)?.intersect(&clip)?;
                    if !cutters.t.is_empty() {
                        m = m.subtract(cutters)?;
                    }
                    count
                        .welded
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                let (left, of) = crate::tile_bridge::components(&m);
                if left > 1 {
                    // Specks cut off at a seam cannot be held by anything; drop them, keep the main piece.
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
                    let speck =
                        |c: usize| (hi[c][0] - lo[c][0]).max(hi[c][1] - lo[c][1]) <= SPECK_MM;
                    if (0..left).all(|c| c == main || speck(c)) {
                        m.t =
                            m.t.iter()
                                .zip(&of)
                                .filter(|(_, &o)| o == main)
                                .map(|(t, _)| *t)
                                .collect();
                        count
                            .dropped
                            .fetch_add(left - 1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                let left = crate::tile_bridge::components(&m).0;
                if left > 1 {
                    return Err(format!(
                        "tile r{}c{} still has {left} separate pieces after bridging: use a larger bridge width or different tile sizes",
                        ny - j,
                        i + 1
                    ));
                }
            }
        }
        m.translate([-xs[i], -ys[j], 0.0]);
        m.quantize_f32();
        let pieces = crate::tile_bridge::components(&m).0;
        if pieces > 1 {
            count
                .loose
                .fetch_add(pieces - 1, std::sync::atomic::Ordering::Relaxed);
            count
                .loose_tiles
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(Some(Tile {
            name: format!("stencil_r{}c{}", ny - j, i + 1),
            mesh: m,
            origin: [xs[i], ys[j]],
            size: [xs[i + 1] - xs[i], ys[j + 1] - ys[j]],
        }))
    }
}

/// Tiles the solid. With no seams the plate comes back whole and nothing is drilled.
pub fn tile(solid: &Mesh, p: &TileParams) -> Result<Tiled, String> {
    if !p.seam_gap.is_finite() || !(0.0..=1.0).contains(&p.seam_gap) {
        return Err("seam clearance must be between 0 and 1 mm".into());
    }
    if !p.seam_angle_deg.is_finite() || !(0.0..=60.0).contains(&p.seam_angle_deg) {
        return Err("seam angle must be between 0 and 60 degrees".into());
    }
    let (lo, hi) = solid.bounds();
    let (w, h, top) = (hi[0] - lo[0], hi[1] - lo[1], hi[2]);
    let t = if p.thickness > 0.0 { p.thickness } else { top };
    let (gx, gy) = (grid(w, p.max_x), grid(h, p.max_y));
    let (nx, ny) = (gx.len() - 1, gy.len() - 1);
    if nx * ny == 1 {
        return Ok(Tiled {
            tiles: vec![Tile {
                name: "stencil".into(),
                mesh: solid.clone(),
                origin: [lo[0], lo[1]],
                size: [w, h],
            }],
            rod: None,
            info: serde_json::json!({"tiles": 1, "connectors": 0}),
            seams: Vec::new(),
            connectors: Vec::new(),
        });
    }
    let xs = shift_seams(solid, 0, lo[0], &gx, p.max_x, p)?;
    let ys = shift_seams(solid, 1, lo[1], &gy, p.max_y, p)?;
    let moved = [(&xs, &gx, lo[0]), (&ys, &gy, lo[1])]
        .iter()
        .map(|(a, g, l)| {
            a.iter()
                .zip(g.iter())
                .filter(|(a, g)| (**a - l - **g).abs() > 1e-9)
                .count()
        })
        .sum::<usize>();
    check(p, t)?;
    let zs = row_heights(p.shape, p.diameter + p.clearance, t);
    let r_hole = (p.diameter + p.clearance) / 2.0;
    let bevel = Bevel::new(p.seam_angle_deg, t);
    let depth = p.depth + r_hole * bevel.lean;
    let gap_along = p.seam_gap * bevel.secant;
    let pocket_len = 2.0 * depth + gap_along;
    for c in [&xs, &ys] {
        if c.len() > 2 && c.windows(2).any(|v| v[1] - v[0] < min_tile(depth) - 1e-6) {
            return Err(format!(
                "tiles must be at least {} mm wide to hold connector holes",
                min_tile(depth)
            ));
        }
    }

    // (axis the hole runs along, seam coordinate, u range along the seam)
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

    let geom = Geometry {
        bevel,
        r_hole,
        depth,
        pocket_len,
        top,
    };
    let Connectors {
        holes,
        cutters,
        blocks,
        thin,
        missed,
        reinforced,
        added_volume,
    } = place_connectors(solid, &seams, &zs, p, &geom)?;

    crate::progress::step("place connector holes", 0.3);
    let grown;
    let solid = if blocks.t.is_empty() {
        solid
    } else {
        grown = solid.union(&blocks)?;
        &grown
    };
    let drilled = if cutters.t.is_empty() {
        solid.clone()
    } else {
        solid.subtract(&cutters)?
    };
    crate::progress::step("drill holes", 0.5);
    let cells: Vec<(usize, usize)> = (0..ny)
        .rev()
        .flat_map(|j| (0..nx).map(move |i| (i, j)))
        .collect();
    let cutter = Cutter {
        p,
        xs: &xs,
        ys: &ys,
        bevel,
        top,
        drilled: &drilled,
        holes: &holes,
        cutters: &cutters,
        count: Counts::default(),
    };
    #[cfg(not(target_arch = "wasm32"))]
    let cut_all: Vec<Result<Option<Tile>, String>> = {
        use rayon::prelude::*;
        cells.par_iter().map(|c| cutter.cut(c)).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let cut_all: Vec<Result<Option<Tile>, String>> = cells.iter().map(|c| cutter.cut(c)).collect();
    let mut tiles = Vec::new();
    for t in cut_all {
        if let Some(t) = t? {
            tiles.push(t);
        }
    }
    let info = serde_json::json!({
        "tiles": tiles.len(), "grid": [nx, ny], "connectors": holes.len(), "rows": zs.len(),
        "bridged_tiles": cutter.count.bridged.load(std::sync::atomic::Ordering::Relaxed), "welded_tiles": cutter.count.welded.load(std::sync::atomic::Ordering::Relaxed), "dropped_specks": cutter.count.dropped.load(std::sync::atomic::Ordering::Relaxed), "thin_wall_holes": thin, "positions_without_room": missed,
        "hole_depth_mm": p.depth, "seam_gap_mm": p.seam_gap, "rod_length_mm": rod_length_with_gap(depth, gap_along), "seams_shifted": moved, "seam_angle_deg": p.seam_angle_deg,
        "loose_pieces": cutter.count.loose.load(std::sync::atomic::Ordering::Relaxed), "loose_tiles": cutter.count.loose_tiles.load(std::sync::atomic::Ordering::Relaxed),
        "reinforced_seams": reinforced, "added_material_mm3": added_volume,
    });
    let seam_list = seams
        .iter()
        .map(|&(a, s, f, t)| [a as f64, s, f, t])
        .collect();
    let connectors = holes
        .iter()
        .map(|h| [h.lo[0], h.lo[1], h.hi[0], h.hi[1]])
        .collect();
    Ok(Tiled {
        tiles,
        rod: Some(connector_rod_with_gap(
            p.shape, p.diameter, depth, gap_along, p.segments,
        )),
        info,
        seams: seam_list,
        connectors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_plate_and_drills_watertight_tiles() {
        // 100 x 60 plate, 6 mm thick with a rectangular window
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [100.0, 60.0, 6.0])
            .subtract(&Mesh::cuboid([20.0, 20.0, -1.0], [40.0, 40.0, 7.0]))
            .unwrap();
        let p = TileParams {
            max_x: 60.0,
            max_y: 0.0,
            shape: Shape::Hex,
            diameter: 3.0,
            depth: POCKET_DEPTH_MM,
            clearance: 0.2,
            seam_gap: 0.0,
            seam_angle_deg: 0.0,
            segments: 32,
            thickness: 0.0,
            bridge: None,
            shift_mm: 0.0,
            avoid: Vec::new(),
            reinforce: false,
        };
        let t = tile(&plate, &p).unwrap();
        assert_eq!(t.tiles.len(), 2);
        assert!(t.tiles.iter().all(|x| x.mesh.is_watertight()));
        let whole: f64 = t.tiles.iter().map(|x| x.mesh.volume()).sum();
        assert!(whole < plate.volume() && whole > plate.volume() - 20.0 * 6.0 * 3.0 * 0.5 * 4.0);
        assert!(t.info["connectors"].as_u64().unwrap() >= 2);
        assert!(t.rod.unwrap().is_watertight());
    }

    #[test]
    fn the_connector_depth_sets_the_pockets_and_the_rod() {
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [100.0, 60.0, 6.0]);
        let mk = |depth| TileParams {
            max_x: 60.0,
            max_y: 0.0,
            shape: Shape::Hex,
            diameter: 3.0,
            depth,
            clearance: 0.2,
            seam_gap: 0.0,
            seam_angle_deg: 0.0,
            segments: 32,
            thickness: 0.0,
            bridge: None,
            shift_mm: 0.0,
            avoid: Vec::new(),
            reinforce: false,
        };
        let (a, b) = (
            tile(&plate, &mk(6.0)).unwrap(),
            tile(&plate, &mk(10.0)).unwrap(),
        );
        assert_eq!(a.info["hole_depth_mm"], 6.0);
        assert_eq!(a.info["rod_length_mm"], 11.0);
        let (la, ha) = a.rod.as_ref().unwrap().bounds();
        assert!((ha[2] - la[2] - 11.0).abs() < 1e-6);
        let v = |t: &Tiled| t.tiles.iter().map(|x| x.mesh.volume()).sum::<f64>();
        assert!(v(&a) > v(&b));
        assert!(a.tiles.iter().all(|x| x.mesh.is_watertight()));
    }

    #[test]
    fn seam_clearance_is_split_evenly_and_connector_rods_span_it() {
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [100.0, 60.0, 6.0]);
        let mut p = plain(60.0, 0.0, false);
        p.seam_gap = 0.3;
        let tiled = tile(&plate, &p).unwrap();
        assert_eq!(tiled.info["seam_gap_mm"], 0.3);
        assert_eq!(tiled.info["hole_depth_mm"], p.depth);
        assert_eq!(tiled.info["rod_length_mm"], rod_length(p.depth) + 0.3);
        let (_, left_hi) = tiled.tiles[0].mesh.bounds();
        let (right_lo, _) = tiled.tiles[1].mesh.bounds();
        let gap = tiled.tiles[1].origin[0] + right_lo[0] - tiled.tiles[0].origin[0] - left_hi[0];
        assert!((gap - 0.3).abs() < 1e-5, "{gap}");
        assert!(tiled.tiles.iter().all(|t| t.mesh.is_watertight()));
        let (rod_lo, rod_hi) = tiled.rod.unwrap().bounds();
        assert!((rod_hi[2] - rod_lo[2] - (rod_length(p.depth) + 0.3)).abs() < 1e-6);
    }

    #[test]
    fn seam_clearance_is_bounded() {
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [100.0, 60.0, 6.0]);
        for seam_gap in [-0.1, 1.1, f64::NAN] {
            let mut p = plain(60.0, 0.0, false);
            p.seam_gap = seam_gap;
            assert!(tile(&plate, &p).is_err());
        }
    }

    fn split_plate() -> Mesh {
        let cb = |a: [f64; 2], b: [f64; 2]| Mesh::cuboid([a[0], a[1], 0.0], [b[0], b[1], 6.0]);
        let mut m = cb([0.0, 0.0], [60.0, 60.0]);
        for part in [
            cb([60.0, 15.0], [80.0, 45.0]),
            cb([72.0, 10.0], [90.0, 50.0]),
            cb([100.0, 0.0], [140.0, 60.0]),
            cb([0.0, 0.0], [140.0, 4.0]),
        ] {
            m = m.union(&part).unwrap();
        }
        m
    }

    fn params(bridge: bool, flip: bool) -> TileParams {
        TileParams {
            max_x: 70.0,
            max_y: 0.0,
            shape: Shape::Hex,
            diameter: 3.0,
            depth: POCKET_DEPTH_MM,
            clearance: 0.2,
            seam_gap: 0.0,
            seam_angle_deg: 0.0,
            segments: 32,
            thickness: 6.0,
            shift_mm: 0.0,
            avoid: Vec::new(),
            reinforce: false,
            bridge: bridge.then_some(crate::tile_bridge::BridgeParams {
                bridge_mm: 2.0,
                per_island: 1,
                raised: false,
                easy: false,
                base_layers: 30,
                layer_height: 0.2,
                plate_top: 6.0,
                flip,
            }),
        }
    }

    #[test]
    fn tiles_get_their_own_bridges() {
        let plate = split_plate();
        let off = tile(&plate, &params(false, false)).unwrap();
        assert!(off
            .tiles
            .iter()
            .any(|x| crate::tile_bridge::components(&x.mesh).0 > 1));
        for flip in [false, true] {
            let on = tile(&plate, &params(true, flip)).unwrap();
            assert!(on.info["bridged_tiles"].as_u64().unwrap() >= 1);
            for x in &on.tiles {
                assert!(x.mesh.is_watertight(), "{}", x.name);
                assert_eq!(crate::tile_bridge::components(&x.mesh).0, 1, "{}", x.name);
            }
        }
    }

    #[test]
    fn rejects_thin_plates() {
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [100.0, 60.0, 3.0]);
        let p = TileParams {
            max_x: 60.0,
            max_y: 0.0,
            shape: Shape::Round,
            diameter: 3.0,
            depth: POCKET_DEPTH_MM,
            clearance: 0.2,
            seam_gap: 0.0,
            seam_angle_deg: 0.0,
            segments: 32,
            thickness: 0.0,
            bridge: None,
            shift_mm: 0.0,
            avoid: Vec::new(),
            reinforce: false,
        };
        assert!(tile(&plate, &p).is_err());
    }

    fn plain(max_x: f64, shift: f64, reinforce: bool) -> TileParams {
        TileParams {
            max_x,
            max_y: 0.0,
            shape: Shape::Hex,
            diameter: 3.0,
            depth: POCKET_DEPTH_MM,
            clearance: 0.2,
            seam_gap: 0.0,
            seam_angle_deg: 0.0,
            segments: 32,
            thickness: 0.0,
            bridge: None,
            shift_mm: shift,
            avoid: Vec::new(),
            reinforce,
        }
    }

    #[test]
    fn a_bevelled_seam_leans_and_the_tiles_still_fit_together() {
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [120.0, 60.0, 6.0]);
        let mut p = plain(70.0, 0.0, false);
        p.seam_angle_deg = 30.0;
        let t = tile(&plate, &p).unwrap();
        assert_eq!(t.tiles.len(), 2);
        assert!(t.tiles.iter().all(|x| x.mesh.is_watertight()));
        // with no clearance the two bevelled tiles add up to the plate less the rod pockets
        let vol: f64 = t.tiles.iter().map(|x| x.mesh.volume()).sum();
        assert!(
            vol < plate.volume() && vol > plate.volume() - 1500.0,
            "{vol}"
        );
        // the cut face leans: the left tile is wider at the top than at the bottom
        let left = &t.tiles[0].mesh;
        let width_at = |z: f64| {
            let s = Mesh::cuboid([-1.0, -1.0, z - 0.01], [200.0, 100.0, z + 0.01]);
            let b = left.intersect(&s).unwrap().bounds();
            b.1[0] - b.0[0]
        };
        let rise = width_at(5.5) - width_at(0.5);
        assert!(
            (rise - 5.0 * 30f64.to_radians().tan()).abs() < 0.2,
            "{rise}"
        );
    }

    #[test]
    fn a_bevelled_seam_clearance_is_measured_across_the_face() {
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [120.0, 60.0, 6.0]);
        let mut p = plain(70.0, 0.0, false);
        p.seam_angle_deg = 45.0;
        let closed = tile(&plate, &p).unwrap();
        p.seam_gap = 0.4;
        let bevel = tile(&plate, &p).unwrap();
        let sum = |t: &Tiled| t.tiles.iter().map(|x| x.mesh.volume()).sum::<f64>();
        // 0.4 mm across a face that is 6 mm / cos 45 deg tall and 60 mm long
        let expected = 0.4 * 60.0 * 6.0 / 45f64.to_radians().cos();
        let lost = sum(&closed) - sum(&bevel);
        assert!(
            (lost - expected).abs() < 0.1 * expected,
            "{lost} {expected}"
        );
        assert!(bevel.tiles.iter().all(|x| x.mesh.is_watertight()));
    }

    #[test]
    fn seam_angle_must_be_within_range() {
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [120.0, 60.0, 6.0]);
        let mut p = plain(70.0, 0.0, false);
        p.seam_angle_deg = 75.0;
        assert!(tile(&plate, &p).is_err());
    }

    #[test]
    fn a_seam_moves_off_a_slot_to_cross_more_material() {
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [100.0, 60.0, 6.0])
            .subtract(&Mesh::cuboid([48.0, -1.0, -1.0], [52.0, 50.0, 7.0]))
            .unwrap();
        let even = tile(&plate, &plain(60.0, 0.0, false));
        assert!(even.is_err() || even.unwrap().info["seams_shifted"] == 0);
        let moved = tile(&plate, &plain(60.0, 5.0, false)).unwrap();
        assert_eq!(moved.info["seams_shifted"], 1);
        let x = moved.seams[0][1];
        assert!(
            (x - 50.0).abs() > 4.0 && (x - 50.0).abs() <= 5.0 + 1e-9,
            "{x}"
        );
        assert!(moved.tiles.iter().all(|t| t.mesh.is_watertight()));
        let sum: f64 = moved.tiles.iter().map(|t| t.size[0]).sum();
        assert!((sum - 100.0).abs() < 1e-6);
        assert!(!moved.connectors.is_empty());
    }

    #[test]
    fn material_is_added_where_no_connector_fits() {
        let cb = |a: [f64; 2], b: [f64; 2]| Mesh::cuboid([a[0], a[1], 0.0], [b[0], b[1], 6.0]);
        let plate = cb([0.0, 0.0], [30.0, 60.0])
            .union(&cb([30.0, 28.5], [70.0, 31.5]))
            .unwrap()
            .union(&cb([70.0, 0.0], [100.0, 60.0]))
            .unwrap();
        let err = tile(&plate, &plain(60.0, 0.0, false))
            .err()
            .expect("no room");
        assert!(err.contains("no room"), "{err}");
        let t = tile(&plate, &plain(60.0, 0.0, true)).unwrap();
        assert!(t.info["reinforced_seams"].as_u64().unwrap() >= 1);
        assert!(t.info["added_material_mm3"].as_f64().unwrap() > 0.0);
        assert!(t.tiles.iter().all(|x| x.mesh.is_watertight()));
    }

    #[test]
    fn tiles_are_checked_as_stored_in_the_file() {
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [100.0, 60.0, 6.0]);
        let t = tile(&plate, &plain(60.0, 0.0, false)).unwrap();
        for x in &t.tiles {
            let back = Mesh::from_stl(&x.mesh.to_stl()).unwrap();
            assert!(back.is_watertight(), "{}", x.name);
            assert_eq!(back.t.len(), x.mesh.t.len());
        }
    }
}
