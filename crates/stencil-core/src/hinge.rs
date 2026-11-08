//! Experimental raised-back tambour joints with a stepped paint-side seam.

use crate::mesh::Mesh;
use std::f64::consts::PI;

const WALL: f64 = 0.8;
const PIN: f64 = 1.5;
const NECK: f64 = 0.6;
const SEGMENTS: usize = 48;
pub const FOLD_DEG: f64 = 60.0;

#[derive(Clone, Debug)]
pub struct Params {
    /// 0: constant x (hinge runs along y); 1: constant y.
    pub axis: usize,
    pub thickness: f64,
    pub clearance: f64,
    pub paint_out: bool,
}

impl Params {
    pub fn check(&self) -> Result<(), String> {
        if self.axis > 1 {
            return Err("hinge axis must be x or y".into());
        }
        if !self.thickness.is_finite() || self.thickness < 3.0 {
            return Err("experimental hinges need a plate at least 3 mm thick".into());
        }
        if !self.clearance.is_finite() || !(0.15..=0.5).contains(&self.clearance) {
            return Err("hinge radial clearance must be between 0.15 and 0.5 mm".into());
        }
        Ok(())
    }

    pub fn axis_height(&self) -> f64 {
        self.thickness + PIN + self.clearance
    }

    pub fn back_height(&self) -> f64 {
        self.axis_height() + PIN + self.clearance + WALL
    }

    fn radius(&self) -> f64 {
        PIN + self.clearance + WALL
    }

    pub fn reach(&self) -> f64 {
        self.radius() + self.clearance + WALL + 1.0
    }
}

fn box_at(axis: usize, x0: f64, x1: f64, u0: f64, u1: f64, z0: f64, z1: f64) -> Mesh {
    if axis == 0 {
        Mesh::cuboid([x0, u0, z0], [x1, u1, z1])
    } else {
        Mesh::cuboid([u0, x0, z0], [u1, x1, z1])
    }
}

fn cylinder(p: &Params, s: f64, u0: f64, u1: f64, r: f64) -> Mesh {
    let mut m = Mesh::prism_z(SEGMENTS, r, u0, u1).along(1 - p.axis);
    let d = if p.axis == 0 {
        [s, 0.0, p.axis_height()]
    } else {
        [0.0, s, p.axis_height()]
    };
    m.translate(d);
    m
}

/// Conservative full-material intervals. Never add hinge material over an aperture.
pub fn runs(mesh: &Mesh, position: f64, p: &Params) -> Result<Vec<[f64; 2]>, String> {
    p.check()?;
    let (lo, hi) = mesh.bounds();
    if mesh.t.is_empty() || lo[2].abs() > 1e-4 || (hi[2] - p.thickness).abs() > 1e-4 {
        return Err("hinges require a flat plate at Z=0: remove pads/raised bridges, or hinge before adding them".into());
    }
    if !position.is_finite()
        || position - p.reach() <= lo[p.axis]
        || position + p.reach() >= hi[p.axis]
    {
        return Err("hinge seam is too close to an outer edge or another seam".into());
    }
    let along = 1 - p.axis;
    let n = ((hi[along] - lo[along]) / 0.5).ceil() as usize;
    if n > 10_000 {
        return Err("hinge seam exceeds the experimental 5000 mm length limit".into());
    }
    let step = (hi[along] - lo[along]) / n as f64;
    let mut intervals = Vec::new();
    let mut start = None;
    for k in 0..n {
        let a = lo[along] + k as f64 * step;
        let b = a + step;
        let bounds = if p.axis == 0 {
            (
                [position - p.reach(), a + 1e-4, 1e-4],
                [position + p.reach(), b - 1e-4, p.thickness - 1e-4],
            )
        } else {
            (
                [a + 1e-4, position - p.reach(), 1e-4],
                [b - 1e-4, position + p.reach(), p.thickness - 1e-4],
            )
        };
        if mesh.box_inside(bounds.0, bounds.1) {
            start.get_or_insert(a);
        } else if let Some(a0) = start.take() {
            if a - a0 >= 4.0 {
                intervals.push([a0, a]);
            }
        }
    }
    if let Some(a0) = start {
        if hi[along] - a0 >= 4.0 {
            intervals.push([a0, hi[along]]);
        }
    }
    if intervals.is_empty() {
        return Err("no continuous material run of at least 4 mm can carry this hinge; move the seam or increase the blank border".into());
    }
    Ok(intervals)
}

/// Low/high halves in source coordinates. `captive` adds integral axial end caps.
pub fn split(
    mesh: &Mesh,
    position: f64,
    p: &Params,
    captive: bool,
) -> Result<(Mesh, Mesh), String> {
    let intervals = runs(mesh, position, p)?;
    let (lo, hi) = mesh.bounds();
    let along = 1 - p.axis;
    let (u0, u1) = (lo[along] - 1.0, hi[along] + 1.0);
    let c = p.clearance;
    let z = p.axis_height();
    let r = p.radius();
    let lap = 1.5;
    let ledge = p.thickness * 0.45;
    let b = |a, b, from, to, bottom, top| box_at(p.axis, a, b, from, to, bottom, top);
    let low_body = b(
        lo[p.axis] - 1.0,
        position - c / 2.0,
        u0,
        u1,
        -1.0,
        p.thickness + 0.01,
    );
    let low_lap = b(position - c - 0.2, position + lap, u0, u1, -1.0, ledge);
    let mut low = mesh.intersect(&low_body.union(&low_lap)?)?;
    let mut high = mesh.intersect(&b(
        position + c / 2.0,
        hi[p.axis] + 1.0,
        u0,
        u1,
        -1.0,
        p.thickness + 0.01,
    ))?;
    high = high.subtract(&b(
        position - 0.1,
        position + lap + c,
        u0,
        u1,
        -1.0,
        ledge + c,
    ))?;

    for [a, b0] in intervals {
        let cap = if captive { WALL } else { 0.0 };
        let (pa, pb) = (a + cap + c, b0 - cap - c);
        // Clear the socket's outside from the low slat; the neck connects behind it.
        low = low.subtract(&cylinder(p, position, a, b0, r + c))?;
        let boss = b(
            position - p.reach(),
            position - r - c,
            pa,
            pb,
            p.thickness - 0.2,
            z + NECK / 2.0,
        );
        let neck = b(
            position - p.reach() + 0.1,
            position,
            pa,
            pb,
            z - NECK / 2.0,
            z + NECK / 2.0,
        );
        let pin = cylinder(p, position, pa, pb, PIN);
        low = low.union(&boss.union(&neck)?.union(&pin)?)?;

        let mut socket = cylinder(p, position, a, b0, r);
        socket = socket.subtract(&cylinder(p, position, a + cap, b0 - cap, PIN + c))?;
        // The throat must be narrower than the bead even at maximum clearance.
        let mut mouth = vec![[0.0, 0.0]];
        for k in 0..=22 {
            let angle = (100.0 + 93.0 * k as f64 / 22.0) * PI / 180.0;
            mouth.push([3.0 * r * angle.cos(), 3.0 * r * angle.sin()]);
        }
        let mut cut = Mesh::extrude_convex(&mouth, a - 0.01, b0 + 0.01).along(along);
        cut.translate(if p.axis == 0 {
            [position, 0.0, z]
        } else {
            [0.0, position, z]
        });
        socket = socket.subtract(&cut)?;
        high = high.union(&socket)?;
    }
    // The raised hardware and overlapping ledge must not overlap in the deployed state.
    if low.intersect(&high)?.volume().abs() > 1e-5 {
        return Err("hinge verification failed: the two slats overlap".into());
    }
    for m in [&mut low, &mut high] {
        if !p.paint_out {
            for v in &mut m.v {
                v[2] = p.thickness - v[2];
            }
            for t in &mut m.t {
                t.swap(1, 2);
            }
        }
        m.quantize_f32();
        if !m.is_watertight() || m.volume() <= 0.0 {
            return Err(
                "hinge verification failed: a slat is not a watertight positive-volume mesh".into(),
            );
        }
    }
    Ok((low, high))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(axis: usize) -> Params {
        Params {
            axis,
            thickness: 3.0,
            clearance: 0.25,
            paint_out: true,
        }
    }

    #[test]
    fn hinge_clearance_and_rotation() {
        for axis in [0, 1] {
            for captive in [false, true] {
                let p = params(axis);
                let plate = Mesh::cuboid([0.0, 0.0, 0.0], [40.0, 30.0, 3.0]);
                let (a, b) = split(&plate, 15.0, &p, captive).unwrap();
                assert!(a.is_watertight() && b.is_watertight());
                assert_eq!(crate::tile_bridge::components(&a).0, 1);
                assert_eq!(crate::tile_bridge::components(&b).0, 1);
                for degrees in (0..=60).step_by(5) {
                    let mut folded = a.clone();
                    let angle = degrees as f64 * PI / 180.0;
                    for v in &mut folded.v {
                        let x = v[axis] - 15.0;
                        let z = v[2] - p.axis_height();
                        v[axis] = 15.0 + x * angle.cos() + z * angle.sin();
                        v[2] = p.axis_height() - x * angle.sin() + z * angle.cos();
                    }
                    let overlap = folded.intersect(&b).unwrap().volume().abs();
                    assert!(
                        overlap < 1e-5,
                        "axis={axis}, cap={captive}, angle={degrees}: overlap {overlap}"
                    );
                }
            }
        }
    }

    #[test]
    fn no_hardware_over_apertures_and_flat_paint_face() {
        let p = params(0);
        let hole = Mesh::cuboid([10.0, 8.0, -1.0], [20.0, 22.0, 10.0]);
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [40.0, 30.0, 3.0])
            .subtract(&hole)
            .unwrap();
        let intervals = runs(&plate, 15.0, &p).unwrap();
        assert!(intervals.iter().all(|r| r[1] <= 8.0 || r[0] >= 22.0));
        let (a, b) = split(&plate, 15.0, &p, true).unwrap();
        assert!(a.intersect(&hole).unwrap().volume().abs() < 1e-5);
        assert!(b.intersect(&hole).unwrap().volume().abs() < 1e-5);
        assert_eq!(a.bounds().0[2], 0.0);
        assert_eq!(b.bounds().0[2], 0.0);
        // The vertical seam on the paint face is covered by the upper step.
        assert!(b.box_inside([16.55, 2.0, 1.8], [16.65, 3.0, 2.0]));
    }

    #[test]
    fn rejects_invalid_geometry() {
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [40.0, 30.0, 3.0]);
        assert!(split(&plate, 1.0, &params(0), true).is_err());
        assert!(split(
            &plate,
            15.0,
            &Params {
                thickness: 2.0,
                ..params(0)
            },
            true
        )
        .is_err());
        assert!(split(&plate, f64::NAN, &params(0), true).is_err());
        assert!(split(
            &plate,
            15.0,
            &Params {
                clearance: 0.0,
                ..params(0)
            },
            true
        )
        .is_err());
        let (a, b) = split(
            &plate,
            15.0,
            &Params {
                paint_out: false,
                ..params(0)
            },
            true,
        )
        .unwrap();
        assert!(a.bounds().0[2] < 0.0 && b.bounds().0[2] < 0.0);
        assert!(a.is_watertight() && b.is_watertight());
        assert!(a.intersect(&b).unwrap().volume().abs() < 1e-5);
    }

    #[test]
    fn radial_clearance_and_axial_retention_at_parameter_extremes() {
        for clearance in [0.15, 0.5] {
            assert!(2.0 * (PIN + clearance) * (93f64.to_radians() / 2.0).sin() < 2.0 * PIN);
            for thickness in [3.0, 6.0] {
                let p = Params {
                    clearance,
                    thickness,
                    ..params(0)
                };
                let plate = Mesh::cuboid([0.0, 0.0, 0.0], [40.0, 20.0, thickness]);
                let (mut a, b) = split(&plate, 15.0, &p, true).unwrap();
                let (mut slide, socket) = split(&plate, 15.0, &p, false).unwrap();
                a.translate([0.0, 1.0, 0.0]);
                slide.translate([0.0, 1.0, 0.0]);
                assert!(
                    a.intersect(&b).unwrap().volume() > 0.01,
                    "caps must retain the pin"
                );
                assert!(
                    slide.intersect(&socket).unwrap().volume().abs() < 1e-5,
                    "slide-in joint must allow axial insertion"
                );
                let (mut folded, high) = split(&plate, 15.0, &p, true).unwrap();
                let angle = FOLD_DEG.to_radians();
                for v in &mut folded.v {
                    let x = v[0] - 15.0;
                    let z = v[2] - p.axis_height();
                    v[0] = 15.0 + x * angle.cos() + z * angle.sin();
                    v[2] = p.axis_height() - x * angle.sin() + z * angle.cos();
                }
                assert!(folded.intersect(&high).unwrap().volume().abs() < 1e-5);
            }
        }
    }
}
