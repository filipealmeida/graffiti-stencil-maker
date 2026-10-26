//! Image -> mask -> island-free layered plate -> watertight binary STL, in pure Rust.
//!
//! The solid is described by `kmin`: for every pixel, the first layer where it is solid (it then runs to the top).
//! Plain stencils have kmin = 0 on material. Raised bridges start higher up, so the first layer holds the real
//! artwork (islands included) and paint passes under the bridges.

use decode::{decode_gray, otsu};
use islands::*;
use layered::*;

pub mod colour;
mod decimate;
mod decode;
mod fasthash;
pub mod graph;
pub mod hardware;
pub mod imageinfo;
mod islands;
mod layered;
pub mod mesh;
pub mod morphology;
mod par;
#[cfg(not(target_arch = "wasm32"))]
pub use par::init_threads;
pub mod progress;
pub mod remesh;
pub mod report;
mod ring;
mod segindex;
mod shape_edit;
pub mod smooth;
pub mod svg;
pub mod text;
mod thin;
mod tile_bridge;
pub mod tile_first;
pub mod tiles;
pub mod trace;
pub mod transform;
mod vector_stencil;
pub mod zip;

#[derive(Clone, Debug)]
pub struct Params {
    pub width_mm: f64,
    pub thickness_mm: f64,
    pub margin_mm: f64,
    pub bridge_mm: f64,
    /// Bridges from every island to the rest of the plate (1-5).
    pub bridges_per_island: u32,
    /// Each further bridge of an island must leave at least this angle (degrees, 0-90) from the earlier ones; 0
    /// takes the shortest links whatever their direction.
    pub bridge_min_angle_deg: f64,
    /// With an angle: the longest a bridge may be, as a multiple of the island's shortest link (1-16).
    pub bridge_max_length: f64,
    /// With an angle: how many passes the islands are bridged in (1-8).
    pub bridge_passes: u32,
    /// Pixels on the longest side of the traced image.
    pub resolution: u32,
    pub threshold: Option<u8>,
    pub invert: bool,
    pub layer_height_mm: f64,
    /// Bridges rise one layer at a time from the island, so the first layer keeps every island.
    pub raised_bridges: bool,
    pub z_bridging: ZBridging,
    /// Copies of the final layer stacked on top for strength.
    pub extra_top_layers: u32,
    /// Print upside down: a 180 degree rotation about the Y axis.
    pub flip: bool,
    /// Islands smaller than this area are deleted instead of bridged.
    pub min_island_mm2: f64,
    /// Stretches of material thinner than this (a rope between two parts) are widened to it; 0 is off.
    pub min_contact_mm: f64,
    /// Minimum width of narrow features at the build plate; 0 is off.
    pub min_first_layer_width_mm: f64,
    /// Height over which the bottom flare tapers back to the stencil outline.
    pub first_layer_taper_mm: f64,
    /// Angle (from vertical) used for the overhang statistic; 0 means 45.
    pub max_overhang_deg: f64,
    /// Bridge islands to the largest piece instead of the one at the grid's corner: a tile cut out of a plate has
    /// no frame to hold on to.
    pub frame_largest: bool,
}

/// The grid a raster stencil is built on: the margin in cells, the grid size and the cell size in mm. The
/// artwork grid is padded by the margin and the whole thing scaled to `width_mm`.
pub(crate) fn plate_grid(
    gw: usize,
    gh: usize,
    width_mm: f64,
    margin_mm: f64,
) -> (usize, usize, usize, f64) {
    let px = width_mm / gw as f64;
    let mpx = (margin_mm / px).ceil() as usize;
    let (w, h) = (gw + 2 * mpx, gh + 2 * mpx);
    (mpx, w, h, width_mm / w as f64)
}

/// How raised bridges climb: `Steps` grows the island layer by layer; `Easy` runs islands the full height and
/// starts all bridges at half height.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ZBridging {
    #[default]
    Steps,
    Easy,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            width_mm: 100.0,
            thickness_mm: 2.0,
            margin_mm: 8.0,
            bridge_mm: 1.6,
            bridges_per_island: 1,
            bridge_min_angle_deg: 0.0,
            bridge_max_length: 4.0,
            bridge_passes: 4,
            resolution: 600,
            threshold: None,
            invert: false,
            layer_height_mm: 0.2,
            raised_bridges: false,
            z_bridging: ZBridging::Easy,
            extra_top_layers: 1,
            flip: false,
            min_island_mm2: 0.0,
            min_contact_mm: 0.0,
            min_first_layer_width_mm: 0.0,
            first_layer_taper_mm: 1.0,
            max_overhang_deg: 0.0,
            frame_largest: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub width_mm: f64,
    pub height_mm: f64,
    pub thickness_mm: f64,
    pub layers: usize,
    pub grid_w: usize,
    pub grid_h: usize,
    pub threshold: u8,
    pub islands_found: usize,
    pub islands_deleted: usize,
    pub bridge_pixels: usize,
    pub islands_remaining: usize,
    pub first_layer_islands: usize,
    pub overhang_area_mm2: f64,
    pub watertight: bool,
    pub triangles: usize,
    pub stl_bytes: usize,
}

impl Stats {
    pub fn to_json(&self) -> String {
        format!(
            "{{\"width_mm\":{:.2},\"height_mm\":{:.2},\"thickness_mm\":{:.2},\"layers\":{},\"grid_w\":{},\"grid_h\":{},\"threshold\":{},\"islands_found\":{},\"islands_deleted\":{},\"bridge_pixels\":{},\"islands_remaining\":{},\"first_layer_islands\":{},\"overhang_area_mm2\":{:.2},\"watertight\":{},\"triangles\":{},\"stl_bytes\":{}}}",
            self.width_mm, self.height_mm, self.thickness_mm, self.layers, self.grid_w, self.grid_h,
            self.threshold, self.islands_found, self.islands_deleted, self.bridge_pixels, self.islands_remaining,
            self.first_layer_islands, self.overhang_area_mm2, self.watertight, self.triangles, self.stl_bytes
        )
    }
}

pub struct Output {
    pub stl: Vec<u8>,
    pub stats: Stats,
    /// Set when the plate only verified after the bridges were changed (vector mode).
    pub fallback: Option<String>,
}

pub fn make_stencil(bytes: &[u8], p: &Params) -> Result<Output, String> {
    validate(p)?;
    let (gray, gw, gh) = decode_gray(bytes, p.resolution)?;
    let thr = p.threshold.unwrap_or_else(|| otsu(&gray));
    // Dark pixels are paint (holes) unless inverted.
    let paint: Vec<bool> = gray.iter().map(|&g| (g < thr) != p.invert).collect();
    stencil_from_paint(&paint, gw, gh, thr, p)
}

/// Builds the stencil from a paint mask (`true` = paint, row 0 at the top, like an image).
pub fn stencil_from_paint(
    paint: &[bool],
    gw: usize,
    gh: usize,
    thr: u8,
    p: &Params,
) -> Result<Output, String> {
    validate(p)?;
    if paint.len() != gw * gh || gw == 0 || gh == 0 {
        return Err("mask size does not match its dimensions".into());
    }
    let mut stats = Stats {
        threshold: thr,
        ..Stats::default()
    };
    let mut art = vec![false; gw * gh];
    for r in 0..gh {
        for c in 0..gw {
            art[(gh - 1 - r) * gw + c] = !paint[r * gw + c];
        }
    }

    let (mpx, w, h, s) = plate_grid(gw, gh, p.width_mm, p.margin_mm);
    let mut grid = Grid {
        w,
        h,
        m: vec![true; w * h],
    };
    for y in 0..gh {
        for x in 0..gw {
            grid.m[(y + mpx) * w + x + mpx] = art[y * gw + x];
        }
    }
    stats.grid_w = w;
    stats.grid_h = h;
    stats.width_mm = w as f64 * s;
    stats.height_mm = h as f64 * s;

    let lh = p.layer_height_mm;
    let base_layers = ((p.thickness_mm / lh).round() as usize).max(1);
    let layers = base_layers + p.extra_top_layers as usize;
    stats.layers = layers;
    stats.thickness_mm = layers as f64 * lh;

    progress::step("prepare grid", 0.05);
    fix_diagonals(&mut grid);
    if p.min_contact_mm > 0.0 {
        thin::widen_thin(&mut grid, s, p.min_contact_mm);
        fix_diagonals(&mut grid);
    }
    let (islands, label) = label_islands(&grid);
    progress::step("find islands", 0.15);
    stats.islands_found = islands;
    if p.min_island_mm2 > 0.0 {
        stats.islands_deleted =
            delete_small_islands(&mut grid, &label, islands, p.min_island_mm2 / (s * s));
        fix_diagonals(&mut grid);
    }
    let original = grid.m.clone();
    let radius = ((p.bridge_mm / s).round() as i64).max(1);
    let mut rise = vec![f32::NAN; w * h];
    stats.bridge_pixels = bridge_islands(
        &mut grid,
        radius,
        &mut rise,
        p.bridges_per_island.clamp(1, 5) as usize,
        p.bridge_min_angle_deg,
        p.bridge_max_length,
        p.frame_largest,
    );

    progress::step("bridge islands", 0.4);
    let l = layers as u32;
    let mut kmin = vec![l; w * h];
    for i in 0..w * h {
        if original[i] {
            kmin[i] = 0;
        } else if grid.m[i] {
            kmin[i] = if p.raised_bridges && base_layers > 1 && !rise[i].is_nan() {
                match p.z_bridging {
                    ZBridging::Steps => {
                        ((rise[i] as f64 * (base_layers - 1) as f64 - 1e-6).ceil() as i64)
                            .clamp(1, base_layers as i64 - 1) as u32
                    }
                    ZBridging::Easy => (base_layers / 2).max(1) as u32,
                }
            } else {
                0
            };
        }
    }
    fix_diagonals_k(&mut kmin, w, h, l);

    let top = Grid {
        w,
        h,
        m: kmin.iter().map(|&k| k < l).collect(),
    };
    stats.islands_remaining = label_islands(&top).0;
    let flare_end = if p.min_first_layer_width_mm > 0.0 {
        let taper_layers = (p.first_layer_taper_mm / lh).ceil().max(1.0) as u32;
        thin::first_layer_flare_ends(
            &Grid {
                w,
                h,
                m: if p.flip {
                    top.m.clone()
                } else {
                    original.clone()
                },
            },
            s,
            p.min_first_layer_width_mm,
            taper_layers,
        )
    } else {
        vec![0; w * h]
    };

    let first = Grid {
        w,
        h,
        m: (0..w * h)
            .map(|i| {
                let core = if p.flip { top.m[i] } else { kmin[i] == 0 };
                core || flare_end[i] > 0
            })
            .collect(),
    };
    stats.first_layer_islands = label_islands(&first).0;
    progress::step("layer map", 0.5);
    stats.overhang_area_mm2 = overhang_area(&kmin, w, h, l, s, lh, p.max_overhang_deg, p.flip);

    let mut mesh = if flare_end.iter().any(|&end| end > 0) {
        layered_mesh_with_flare(&kmin, &flare_end, w, h, l, s, lh, p.flip)?
    } else {
        layered_mesh(&kmin, w, h, l, s, lh)?
    };
    if p.flip {
        let (ww, tt) = (w as f32 * s as f32, l as f32 * lh as f32);
        for v in mesh.verts.iter_mut() {
            v[0] = ww - v[0];
            v[2] = tt - v[2];
        }
    }
    progress::step("build mesh", 0.9);
    stats.watertight = mesh.is_watertight();
    stats.triangles = mesh.tris.len();
    let stl = mesh.to_stl();
    stats.stl_bytes = stl.len();
    progress::step("write STL", 1.0);
    Ok(Output {
        stl,
        stats,
        fallback: None,
    })
}

pub(crate) fn validate(p: &Params) -> Result<(), String> {
    if p.width_mm < 20.0 || p.width_mm > 2000.0 {
        return Err(format!("width must be 20-2000 mm, got {}", p.width_mm));
    }
    if p.thickness_mm <= 0.0 {
        return Err("thickness must be positive".into());
    }
    if p.layer_height_mm < 0.05 || p.layer_height_mm > 5.0 {
        return Err(format!(
            "layer height must be 0.05-5 mm, got {}",
            p.layer_height_mm
        ));
    }
    if !p.bridge_min_angle_deg.is_finite() || !(0.0..=90.0).contains(&p.bridge_min_angle_deg) {
        return Err("minimum angle between bridges must be 0-90 degrees".into());
    }
    if !p.bridge_max_length.is_finite() || !(1.0..=16.0).contains(&p.bridge_max_length) {
        return Err("longest bridge must be 1-16 times the shortest link".into());
    }
    if !(1..=8).contains(&p.bridge_passes) {
        return Err("bridging passes must be 1-8".into());
    }
    if !p.min_contact_mm.is_finite() || p.min_contact_mm < 0.0 {
        return Err("minimum surface contact must be 0 or more".into());
    }
    if !p.min_first_layer_width_mm.is_finite() || p.min_first_layer_width_mm < 0.0 {
        return Err("minimum first-layer width must be 0 or more".into());
    }
    if !p.first_layer_taper_mm.is_finite() || p.first_layer_taper_mm < 0.0 {
        return Err("first-layer taper height must be 0 or more".into());
    }
    if p.min_first_layer_width_mm > 0.0
        && (p.first_layer_taper_mm <= 0.0 || p.first_layer_taper_mm > p.thickness_mm)
    {
        return Err(format!(
            "first-layer taper height must be above 0 and no greater than stencil thickness ({} mm)",
            p.thickness_mm
        ));
    }
    if p.margin_mm < 0.0 || p.width_mm <= 2.0 * p.margin_mm {
        return Err(format!(
            "margin too large for the stencil width: width {} mm, margin {} mm (needs width > 2 x margin)",
            p.width_mm, p.margin_mm
        ));
    }
    if p.resolution < 16 || p.resolution > 20000 {
        return Err("resolution must be 16-20000 px".into());
    }
    Ok(())
}

/// Volume of a binary STL in mm³.
#[cfg(test)]
pub(crate) fn stl_volume(stl: &[u8]) -> f64 {
    let f = |o: usize| f32::from_le_bytes(stl[o..o + 4].try_into().unwrap()) as f64;
    (0..u32::from_le_bytes(stl[80..84].try_into().unwrap()) as usize)
        .map(|i| {
            let b = 84 + i * 50 + 12;
            let v = |k: usize| [f(b + k * 12), f(b + k * 12 + 4), f(b + k * 12 + 8)];
            let (a, c, d) = (v(0), v(1), v(2));
            (a[0] * (c[1] * d[2] - c[2] * d[1]) - a[1] * (c[0] * d[2] - c[2] * d[0])
                + a[2] * (c[0] * d[1] - c[1] * d[0]))
                / 6.0
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> Vec<u8> {
        let img = image::GrayImage::from_fn(w, h, |x, y| image::Luma([f(x, y)]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn ring_with_island_is_bridged_and_watertight() {
        // White plate, black ring (paint) with a white centre island.
        let data = png(120, 120, |x, y| {
            let d = (((x as f64 - 60.0).powi(2) + (y as f64 - 60.0).powi(2)).sqrt()) as u32;
            if (30..40).contains(&d) {
                0
            } else {
                255
            }
        });
        let o = make_stencil(
            &data,
            &Params {
                resolution: 120,
                ..Params::default()
            },
        )
        .unwrap();
        assert!(o.stats.islands_found >= 1);
        assert_eq!(o.stats.islands_remaining, 0);
        assert!(o.stats.watertight);
    }

    /// Paint everywhere except two blobs of plate joined by a thin rope.
    fn rope_mask() -> Vec<bool> {
        let (w, h) = (200usize, 100usize);
        (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let blob = (20..60).contains(&x) || (140..180).contains(&x);
                let blob = blob && (30..70).contains(&y);
                let rope = (60..140).contains(&x) && (49..51).contains(&y);
                !(blob || rope)
            })
            .collect()
    }

    #[test]
    fn thin_rope_is_widened_by_minimum_contact() {
        let mask = rope_mask();
        let run = |min_contact_mm: f64| {
            let p = Params {
                width_mm: 100.0,
                margin_mm: 4.0,
                min_contact_mm,
                ..Params::default()
            };
            stencil_from_paint(&mask, 200, 100, 0, &p).unwrap()
        };
        let (off, on) = (run(0.0), run(3.0));
        assert!(on.stats.watertight && on.stats.islands_remaining == 0);
        let (v0, v1) = (stl_volume(&off.stl), stl_volume(&on.stl));
        // a 1 mm rope over 40 mm grows to about 3 mm wide: roughly 80 mm² of plate, 2 mm thick
        assert!(v1 - v0 > 100.0, "volume grew by only {}", v1 - v0);
        let tiny = run(0.2);
        assert!((stl_volume(&tiny.stl) - v0).abs() < 1.0);
    }

    #[test]
    fn first_layer_flare_tapers_without_changing_bridge_selection() {
        let mask = rope_mask();
        let run = |width: f64, taper: f64, raised_bridges: bool, flip: bool| {
            stencil_from_paint(
                &mask,
                200,
                100,
                0,
                &Params {
                    width_mm: 100.0,
                    margin_mm: 4.0,
                    raised_bridges,
                    flip,
                    min_first_layer_width_mm: width,
                    first_layer_taper_mm: taper,
                    ..Params::default()
                },
            )
            .unwrap()
        };
        for raised in [false, true] {
            for flip in [false, true] {
                let (plain, short, long) = (
                    run(0.0, 1.0, raised, flip),
                    run(3.0, 0.4, raised, flip),
                    run(3.0, 1.6, raised, flip),
                );
                assert!(short.stats.watertight && long.stats.watertight);
                assert_eq!(short.stats.islands_remaining, 0);
                assert_eq!(short.stats.bridge_pixels, plain.stats.bridge_pixels);
                let base = stl_volume(&plain.stl);
                let short_gain = stl_volume(&short.stl) - base;
                let long_gain = stl_volume(&long.stl) - base;
                assert!(
                    short_gain > 0.0,
                    "raised {raised}, flip {flip}: no bottom flare was added"
                );
                assert!(
                    long_gain > short_gain,
                    "raised {raised}, flip {flip}: a taller taper should add more flare volume: short {short_gain}, long {long_gain}"
                );
            }
        }
    }

    #[test]
    fn first_layer_taper_must_fit_in_the_stencil_thickness() {
        let p = Params {
            min_first_layer_width_mm: 1.0,
            first_layer_taper_mm: 2.1,
            thickness_mm: 2.0,
            ..Params::default()
        };
        assert!(validate(&p)
            .unwrap_err()
            .contains("no greater than stencil thickness"));
    }

    #[test]
    fn dead_end_tips_are_left_alone() {
        let (w, h) = (200usize, 100usize);
        let mask: Vec<bool> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let blob = (20..60).contains(&x) && (30..70).contains(&y);
                let finger = (60..140).contains(&x) && (49..51).contains(&y);
                !(blob || finger)
            })
            .collect();
        let run = |min_contact_mm: f64| {
            let p = Params {
                width_mm: 100.0,
                margin_mm: 4.0,
                min_contact_mm,
                ..Params::default()
            };
            stl_volume(&stencil_from_paint(&mask, w, h, 0, &p).unwrap().stl)
        };
        assert!((run(3.0) - run(0.0)).abs() < 1.0);
    }

    #[test]
    fn diagonal_pixels_stay_manifold() {
        let data = png(16, 16, |x, y| if (x + y) % 2 == 0 { 0 } else { 255 });
        let o = make_stencil(
            &data,
            &Params {
                resolution: 16,
                margin_mm: 4.0,
                ..Params::default()
            },
        )
        .unwrap();
        assert_eq!(o.stats.islands_remaining, 0);
        assert!(o.stats.watertight);
    }

    fn ring() -> Vec<u8> {
        png(120, 120, |x, y| {
            let d = (((x as f64 - 60.0).powi(2) + (y as f64 - 60.0).powi(2)).sqrt()) as u32;
            if (30..40).contains(&d) {
                0
            } else {
                255
            }
        })
    }

    #[test]
    fn raised_bridges_keep_first_layer_islands() {
        let p = Params {
            resolution: 120,
            raised_bridges: true,
            ..Params::default()
        };
        let o = make_stencil(&ring(), &p).unwrap();
        assert_eq!(o.stats.first_layer_islands, 1);
        assert_eq!(o.stats.islands_remaining, 0);
        assert!(o.stats.watertight);
    }

    #[test]
    fn more_bridges_per_island_add_bridges_and_stay_island_free() {
        let mut last = 0;
        for n in [1, 3, 5] {
            let p = Params {
                resolution: 120,
                bridges_per_island: n,
                ..Params::default()
            };
            let o = make_stencil(&ring(), &p).unwrap();
            assert_eq!(o.stats.islands_remaining, 0);
            assert!(o.stats.watertight);
            assert!(
                o.stats.bridge_pixels > last,
                "{n} bridges: {}",
                o.stats.bridge_pixels
            );
            last = o.stats.bridge_pixels;
        }
    }

    #[test]
    fn spread_bridges_keep_the_count_and_stay_island_free() {
        for n in [2, 3, 5] {
            let run = |angle| {
                let p = Params {
                    resolution: 120,
                    bridges_per_island: n,
                    bridge_min_angle_deg: angle,
                    ..Params::default()
                };
                make_stencil(&ring(), &p).unwrap()
            };
            let (closest, spread) = (run(0.0), run(45.0));
            assert_eq!(spread.stats.islands_remaining, 0);
            assert!(spread.stats.watertight);
            assert!(spread.stats.bridge_pixels > 0);
            assert!(closest.stats.bridge_pixels > 0);
        }
    }

    #[test]
    fn flip_has_no_overhang_and_same_height() {
        let p = Params {
            resolution: 120,
            raised_bridges: true,
            flip: true,
            ..Params::default()
        };
        let o = make_stencil(&ring(), &p).unwrap();
        assert_eq!(o.stats.overhang_area_mm2, 0.0);
        assert!(o.stats.watertight);
    }

    #[test]
    fn min_island_deletes_small_islands() {
        let p = Params {
            resolution: 120,
            min_island_mm2: 1.0e6,
            ..Params::default()
        };
        let o = make_stencil(&ring(), &p).unwrap();
        assert!(o.stats.islands_deleted >= 1);
        assert!(o.stats.watertight);
    }

    #[test]
    fn margin_error_quotes_values() {
        let e = make_stencil(
            &[],
            &Params {
                margin_mm: 60.0,
                ..Params::default()
            },
        )
        .err()
        .unwrap();
        assert!(e.contains("width 100") && e.contains("margin 60"));
    }
}
