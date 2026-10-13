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
pub const ROD_LENGTH_MM: f64 = 2.0 * POCKET_DEPTH_MM - 1.0;
pub const MIN_TILE_MM: f64 = POCKET_DEPTH_MM / 0.4;

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
    pub clearance: f64,
    pub segments: usize,
    /// Plate thickness without raised pads; 0 uses the mesh height.
    pub thickness: f64,
}

pub struct Tile {
    pub name: String,
    pub mesh: Mesh,
}

pub struct Tiled {
    pub tiles: Vec<Tile>,
    pub rod: Option<Mesh>,
    pub info: serde_json::Value,
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

fn row_heights(shape: Shape, d: f64, t: f64) -> Vec<f64> {
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

fn rod(shape: Shape, r: f64, len: f64, segments: usize, axis: usize) -> Mesh {
    let n = if shape == Shape::Hex {
        6
    } else {
        segments.max(12)
    };
    Mesh::prism_z(n, r, -len / 2.0, len / 2.0).along(axis)
}

pub fn connector_rod(shape: Shape, d: f64, segments: usize) -> Mesh {
    let mut m = rod(shape, d / 2.0, ROD_LENGTH_MM, segments, 2);
    m.translate([0.0, 0.0, ROD_LENGTH_MM / 2.0]);
    m
}

pub fn check(p: &TileParams, height: f64) -> Result<(), String> {
    if !DIAMETERS.contains(&p.diameter) {
        return Err("connector diameter must be 2.5, 3, 4, 5 or 6 mm".into());
    }
    if height <= MIN_HEIGHT_MM {
        return Err(format!(
            "connectors need a plate thicker than {MIN_HEIGHT_MM} mm (it is {height:.1} mm)"
        ));
    }
    let need = z_extent(p.shape, p.diameter) + 2.0 * WALL_MM;
    if need > height + 1e-9 {
        return Err(format!("a connector of diameter {} mm needs a plate at least {:.1} mm thick (it is {height:.1} mm)", p.diameter, need));
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

/// Tiles the solid. With no seams the plate comes back whole and nothing is drilled.
pub fn tile(solid: &Mesh, p: &TileParams) -> Result<Tiled, String> {
    let (lo, hi) = solid.bounds();
    let (w, h, top) = (hi[0] - lo[0], hi[1] - lo[1], hi[2]);
    let t = if p.thickness > 0.0 { p.thickness } else { top };
    let xs: Vec<f64> = grid(w, p.max_x).iter().map(|x| lo[0] + x).collect();
    let ys: Vec<f64> = grid(h, p.max_y).iter().map(|y| lo[1] + y).collect();
    let (nx, ny) = (xs.len() - 1, ys.len() - 1);
    if nx * ny == 1 {
        return Ok(Tiled {
            tiles: vec![Tile {
                name: "stencil".into(),
                mesh: solid.clone(),
            }],
            rod: None,
            info: serde_json::json!({"tiles": 1, "connectors": 0}),
        });
    }
    check(p, t)?;
    for c in [&xs, &ys] {
        if c.len() > 2 && c.windows(2).any(|v| v[1] - v[0] < MIN_TILE_MM - 1e-6) {
            return Err(format!(
                "tiles must be at least {MIN_TILE_MM} mm wide to hold connector holes"
            ));
        }
    }
    let zs = row_heights(p.shape, p.diameter + p.clearance, t);
    let r_hole = (p.diameter + p.clearance) / 2.0;

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

    let mut holes: Vec<Hole> = Vec::new();
    let mut cutters = Mesh::default();
    let (mut thin, mut missed) = (0usize, 0usize);
    for &(axis, s, a, b) in &seams {
        // work on the strip around the seam only: cheaper booleans
        let (cl, ch): (P3, P3) = if axis == 0 {
            (
                [s - POCKET_DEPTH_MM - 1.0, a - 1.0, -1.0],
                [s + POCKET_DEPTH_MM + 1.0, b + 1.0, top + 1.0],
            )
        } else {
            (
                [a - 1.0, s - POCKET_DEPTH_MM - 1.0, -1.0],
                [b + 1.0, s + POCKET_DEPTH_MM + 1.0, top + 1.0],
            )
        };
        let strip = solid.intersect(&Mesh::cuboid(cl, ch))?;
        let n = (((b - a) / SPACING_TARGET_MM - 1e-9).ceil() as usize).max(1);
        let cell = (b - a) / n as f64;
        let mut placed_here = 0;
        for k in 0..n {
            let nominal = a + cell * (k as f64 + 0.5);
            for &z in &zs {
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
                            m.translate(if axis == 0 { [s, u, z] } else { [u, s, z] });
                            m
                        };
                        let guard = at(r_hole + wall, 2.0 * POCKET_DEPTH_MM);
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
                        if !strip.box_inside(gl, gh) {
                            continue;
                        }
                        cutters.append(&at(r_hole, 2.0 * POCKET_DEPTH_MM));
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
        if placed_here == 0 {
            return Err(format!("no room for a connector hole on the {} seam at {:.1} mm: use a smaller connector, larger tiles or a thicker plate", if axis == 0 { "vertical" } else { "horizontal" }, s));
        }
    }

    let drilled = if cutters.t.is_empty() {
        solid.clone()
    } else {
        solid.subtract(&cutters)?
    };
    let mut tiles = Vec::new();
    for j in (0..ny).rev() {
        for i in 0..nx {
            let xa = if i > 0 { xs[i] } else { xs[i] - 1.0 };
            let xb = if i < nx - 1 {
                xs[i + 1]
            } else {
                xs[i + 1] + 1.0
            };
            let ya = if j > 0 { ys[j] } else { ys[j] - 1.0 };
            let yb = if j < ny - 1 {
                ys[j + 1]
            } else {
                ys[j + 1] + 1.0
            };
            let mut m = drilled.intersect(&Mesh::cuboid([xa, ya, -1.0], [xb, yb, top + 1.0]))?;
            if m.t.is_empty() {
                continue;
            }
            m.translate([-xs[i], -ys[j], 0.0]);
            tiles.push(Tile {
                name: format!("stencil_r{}c{}", ny - j, i + 1),
                mesh: m,
            });
        }
    }
    let info = serde_json::json!({
        "tiles": tiles.len(), "grid": [nx, ny], "connectors": holes.len(), "rows": zs.len(),
        "thin_wall_holes": thin, "positions_without_room": missed,
        "hole_depth_mm": POCKET_DEPTH_MM, "rod_length_mm": ROD_LENGTH_MM,
    });
    Ok(Tiled {
        tiles,
        rod: Some(connector_rod(p.shape, p.diameter, p.segments)),
        info,
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
            clearance: 0.2,
            segments: 32,
            thickness: 0.0,
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
    fn rejects_thin_plates() {
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [100.0, 60.0, 3.0]);
        let p = TileParams {
            max_x: 60.0,
            max_y: 0.0,
            shape: Shape::Round,
            diameter: 3.0,
            clearance: 0.2,
            segments: 32,
            thickness: 0.0,
        };
        assert!(tile(&plate, &p).is_err());
    }
}
