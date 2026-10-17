//! Image -> mask -> island-free layered plate -> watertight binary STL, in pure Rust.
//!
//! The solid is described by `kmin`: for every pixel, the first layer where it is solid (it then runs to the top).
//! Plain stencils have kmin = 0 on material. Raised bridges start higher up, so the first layer holds the real
//! artwork (islands included) and paint passes under the bridges.

use decode::{decode_gray, otsu};
use islands::*;
use layered::*;

pub mod colour;
pub mod complexity;
mod decode;
pub mod graph;
pub mod hardware;
mod islands;
mod layered;
pub mod mesh;
pub mod morphology;
pub mod progress;
mod shape_edit;
pub mod smooth;
pub mod svg;
pub mod text;
mod tile_bridge;
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
    /// Angle (from vertical) used for the overhang statistic; 0 means 45.
    pub max_overhang_deg: f64,
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
            resolution: 600,
            threshold: None,
            invert: false,
            layer_height_mm: 0.2,
            raised_bridges: false,
            z_bridging: ZBridging::Steps,
            extra_top_layers: 1,
            flip: false,
            min_island_mm2: 0.0,
            max_overhang_deg: 0.0,
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

    let px = p.width_mm / gw as f64;
    let mpx = (p.margin_mm / px).ceil() as usize;
    let (w, h) = (gw + 2 * mpx, gh + 2 * mpx);
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
    let s = p.width_mm / w as f64;
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
    let first = Grid {
        w,
        h,
        m: kmin.iter().map(|&k| k == 0).collect(),
    };
    stats.first_layer_islands = label_islands(&first).0;
    progress::step("layer map", 0.5);
    stats.overhang_area_mm2 = overhang_area(&kmin, w, h, l, s, lh, p.max_overhang_deg, p.flip);

    let mut mesh = layered_mesh(&kmin, w, h, l, s, lh)?;
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
    Ok(Output { stl, stats })
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
