//! The Colour set node: one plate per colour, drilled with the same registration holes so the layers line up.

use super::catalogue::Outputs;
use super::*;
use crate::hardware as hw;

pub(super) const MAX_COLOURS: usize = 6;
pub(super) const INPUTS: [&str; MAX_COLOURS] = [
    "colour_1", "colour_2", "colour_3", "colour_4", "colour_5", "colour_6",
];

/// Hole centres in the plate frame (origin at the corner, y up). Two holes sit on a diagonal and three form an
/// L, so a plate cannot be laid on the wall rotated by a half turn.
pub(crate) fn registration_centres(count: usize, w: f64, h: f64, inset: f64) -> Vec<(f64, f64)> {
    let c = inset;
    match count {
        0 => vec![],
        1 => vec![(c, c)],
        2 => vec![(c, c), (w - c, h - c)],
        3 => vec![(c, c), (w - c, c), (w - c, h - c)],
        _ => vec![(c, c), (w - c, c), (w - c, h - c), (c, h - c)],
    }
}

pub(super) fn run_colour_set(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let plates: Vec<(usize, &Rc<Solid>)> = INPUTS
        .iter()
        .enumerate()
        .filter_map(|(k, n)| match i.get(*n) {
            Some(Value::Solid(s)) => Some((k + 1, s)),
            _ => None,
        })
        .collect();
    if plates.is_empty() {
        return Err("connect at least one stencil to colour_1".into());
    }
    let holes = p.f("holes") as usize;
    let d = p.f("hole_d");
    let mut files = Vec::new();
    let mut meta = json!({"colours": plates.len()});
    let mut size: Option<(f64, f64, usize)> = None;
    for (k, s) in &plates {
        let mut mesh = crate::mesh::Mesh::from_stl(&s.stl)?;
        let (lo, hi) = mesh.bounds();
        let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
        match size {
            None => size = Some((w, h, *k)),
            Some((w0, h0, k0)) if (w - w0).abs() > 0.05 || (h - h0).abs() > 0.05 => {
                return Err(format!(
                    "colour_{k} is {w:.1} x {h:.1} mm but colour_{k0} is {w0:.1} x {h0:.1} mm: give every colour the same plate width and margin, from the same picture size"
                ));
            }
            _ => {}
        }
        if holes > 0 {
            let margin = s.stats["margin_mm"].as_f64().unwrap_or(0.0);
            let inset = if p.f("inset") > 0.0 {
                p.f("inset")
            } else {
                margin / 2.0
            };
            if inset - d / 2.0 < 0.5 || inset + d / 2.0 > margin - 0.5 {
                return Err(format!(
                    "a {d:.1} mm registration hole does not fit the {margin:.1} mm margin of colour_{k}: raise the margin to at least {:.1} mm or use a smaller hole",
                    d + 1.0
                ));
            }
            let flip = s.stats["flip"].as_bool().unwrap_or(false);
            let top = hi[2] + 1.0;
            let cut = hw::union_all(
                registration_centres(holes, w, h, inset)
                    .into_iter()
                    .map(|(x, y)| {
                        let mut c = hw::cylinder(d / 2.0, lo[2] - 1.0, top, 48);
                        // a plate printed upside down is mirrored left to right
                        c.translate([lo[0] + if flip { w - x } else { x }, lo[1] + y, 0.0]);
                        c
                    })
                    .collect(),
            );
            mesh = mesh.subtract(&cut)?;
            if !mesh.is_watertight() {
                return Err(format!(
                    "verification failed: colour_{k} is not watertight after drilling"
                ));
            }
        }
        files.push((format!("colour_{k}.stl"), mesh.to_stl()));
    }
    if holes > 0 {
        meta["registration"] = json!({"holes": holes, "diameter_mm": d});
        if p.b("pins") {
            let pin_d = (d - p.f("pin_clearance")).max(0.5);
            let len = p.f("pin_length");
            let pins = hw::union_all(
                (0..holes)
                    .map(|k| {
                        let mut c = hw::cylinder(pin_d / 2.0, 0.0, len, 48);
                        c.translate([pin_d / 2.0 + k as f64 * (pin_d + 3.0), pin_d / 2.0, 0.0]);
                        c
                    })
                    .collect(),
            );
            if !pins.is_watertight() {
                return Err("verification failed: the pins are not watertight".into());
            }
            files.push((format!("pins_{holes}x_d{pin_d:.1}mm.stl"), pins.to_stl()));
        }
    }
    Ok(vec![(
        "parts",
        Value::Parts(Rc::new(Parts { files, meta })),
    )])
}
