//! Threads, pads, mounting holes, screws, nuts, joiner plates and the connector fit test.
use crate::mesh::Mesh;
use std::f64::consts::{PI, TAU};

/// (name, major diameter, pitch) in mm
pub const THREADS: [(&str, f64, f64); 6] = [
    ("M3", 3.0, 0.5),
    ("M4", 4.0, 0.7),
    ("M5", 5.0, 0.8),
    ("M6", 6.0, 1.0),
    ("M8", 8.0, 1.25),
    ("M10", 10.0, 1.5),
];
pub const PAD_WALL_MM: f64 = 2.0;
pub const JOIN_WALL_MM: f64 = 3.0;

pub fn thread(name: &str) -> Result<(f64, f64), String> {
    THREADS
        .iter()
        .find(|t| t.0 == name)
        .map(|t| (t.1, t.2))
        .ok_or_else(|| format!("unknown thread {name}; use M3, M4, M5, M6, M8 or M10"))
}

pub fn pad_diameter(name: &str) -> Result<f64, String> {
    Ok(thread(name)?.0 + 2.0 * PAD_WALL_MM)
}

/// Right-handed thread solid (the shape an internal thread removes, with `clearance` added radially), from z0 to z1.
pub fn thread_void(
    name: &str,
    clearance: f64,
    z0: f64,
    z1: f64,
    n: usize,
    per_turn: usize,
) -> Result<Mesh, String> {
    let (d, pitch) = thread(name)?;
    let r_maj = d / 2.0 + clearance;
    let depth = 0.54 * pitch;
    let n = n.max(24);
    let prof: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let f = if !(0.0625..0.9375).contains(&t) {
                1.0
            } else if t < 0.375 {
                1.0 - (t - 0.0625) / 0.3125
            } else if t <= 0.625 {
                0.0
            } else {
                (t - 0.625) / 0.3125
            };
            (r_maj - depth * (1.0 - f), TAU * t)
        })
        .collect();
    let h = z1 - z0;
    let slices = ((h / pitch * per_turn as f64).ceil() as usize).max(1);
    let mut m = Mesh::default();
    for k in 0..=slices {
        let z = z0 + h * k as f64 / slices as f64;
        let twist = TAU * (z - z0) / pitch;
        for &(r, a) in &prof {
            m.v.push([r * (a + twist).cos(), r * (a + twist).sin(), z]);
        }
    }
    let (c0, c1) = (m.v.len(), m.v.len() + 1);
    m.v.push([0.0, 0.0, z0]);
    m.v.push([0.0, 0.0, z1]);
    for k in 0..slices {
        for i in 0..n {
            let j = (i + 1) % n;
            let (a, b, c, e) = (k * n + i, k * n + j, (k + 1) * n + j, (k + 1) * n + i);
            m.t.push([a, b, c]);
            m.t.push([a, c, e]);
        }
    }
    for i in 0..n {
        let j = (i + 1) % n;
        m.t.push([c0, j, i]);
        m.t.push([c1, slices * n + i, slices * n + j]);
    }
    Ok(m)
}

pub fn cylinder(r: f64, z0: f64, z1: f64, n: usize) -> Mesh {
    Mesh::prism_z(n.max(12), r, z0, z1)
}

pub fn hexagon(across_corners: f64, z0: f64, z1: f64) -> Mesh {
    Mesh::prism_z(6, across_corners / 2.0, z0, z1)
}

pub fn union_all(parts: Vec<Mesh>) -> Mesh {
    let mut out = Mesh::default();
    for p in &parts {
        out.append(p);
    }
    out
}

pub struct PadParams {
    pub count: usize,
    pub thread: String,
    pub height: f64,
    pub clearance: f64,
    pub flip: bool,
    pub segments: usize,
}

pub fn pad_centres(count: usize, w: f64, h: f64, margin: f64) -> Vec<(f64, f64)> {
    let c = margin / 2.0;
    match count {
        0 => vec![],
        1 => vec![(w / 2.0, h - c)],
        2 => vec![(w / 2.0, h - c), (w / 2.0, c)],
        3 => vec![(c, h - c), (w - c, h - c), (w / 2.0, c)],
        _ => vec![(c, h - c), (w - c, h - c), (c, c), (w - c, c)],
    }
}

/// Raised pads with a threaded hole on the print-top face. Printed upside down: plain threaded holes in the frame.
pub fn add_pads(
    solid: &Mesh,
    p: &PadParams,
    margin: f64,
    layer: f64,
) -> Result<(Mesh, serde_json::Value), String> {
    if p.count == 0 {
        return Ok((solid.clone(), serde_json::json!({"pads": 0})));
    }
    let (lo, hi) = solid.bounds();
    let (w, h, t) = (hi[0] - lo[0], hi[1] - lo[1], hi[2]);
    let (d, pitch) = thread(&p.thread)?;
    let dia = pad_diameter(&p.thread)?;
    if margin < dia - 1e-6 {
        return Err(format!("margin too small for {} pads: the margin is {margin:.1} mm and a {} pad is {dia:.0} mm wide", p.thread, p.thread));
    }
    let rise = if p.flip { 0.0 } else { p.height };
    if !p.flip && (rise < layer || t + rise < 4.0 * pitch) {
        return Err(format!(
            "pad height too small for an {} thread: plate + pad must be at least {} mm",
            p.thread,
            4.0 * pitch
        ));
    }
    let r = (dia / 2.0).min(margin / 2.0);
    let cs: Vec<(f64, f64)> = pad_centres(p.count, w, h, margin)
        .into_iter()
        .map(|(x, y)| (x + lo[0], y + lo[1]))
        .collect();
    for i in 0..cs.len() {
        for j in 0..i {
            if (cs[i].0 - cs[j].0).hypot(cs[i].1 - cs[j].1) < 2.0 * r - 1e-6 {
                return Err(format!(
                    "the plate is too small for {} {} pads: they would overlap",
                    p.count, p.thread
                ));
            }
        }
    }
    let out = add_pads_at(solid, &cs, p, r, layer)?;
    Ok((
        out,
        serde_json::json!({"pads": cs.len(), "pad_diameter_mm": dia, "pad_thread": p.thread, "thread_major_mm": d}),
    ))
}

/// Raised pads (or, printed flipped, plain threaded holes) at `cs`, in the solid's own coordinates.
pub fn add_pads_at(
    solid: &Mesh,
    cs: &[(f64, f64)],
    p: &PadParams,
    r: f64,
    layer: f64,
) -> Result<Mesh, String> {
    let (_, hi) = solid.bounds();
    let t = hi[2];
    let rise = if p.flip { 0.0 } else { p.height };
    let mut out = solid.clone();
    if !p.flip {
        let pads = union_all(
            cs.iter()
                .map(|&(x, y)| {
                    let mut m = cylinder(r, t - layer, t + rise, 96);
                    m.translate([x, y, 0.0]);
                    m
                })
                .collect(),
        );
        out = out.union(&pads)?;
    }
    let per_turn = (p.segments / 4).clamp(8, 32);
    let voids = union_all(
        cs.iter()
            .map(|&(x, y)| {
                let mut m = thread_void(
                    &p.thread,
                    p.clearance,
                    -0.5,
                    t + rise + 0.5,
                    p.segments,
                    per_turn,
                )
                .unwrap();
                m.translate([x, y, 0.0]);
                m
            })
            .collect(),
    );
    out.subtract(&voids)
}

pub struct MountParams {
    pub screw: String,
    pub clearance: f64,
    pub threaded: bool,
    pub offset: f64,
    pub segments: usize,
}

/// Two hole centres either side of a seam, and the seam index.
pub type MountPair = ((f64, f64), (f64, f64), usize);

pub struct Mounts {
    pub points: Vec<(f64, f64)>,
    pub pairs: Vec<MountPair>,
}

/// Through-hole centres on the middle of the frame band (plate mm, y up): corners, both sides of every seam that crosses
/// the frame (pairs for joiner plates) and the middle of long frame segments.
#[allow(clippy::too_many_arguments)]
pub fn mount_positions(
    w: f64,
    h: f64,
    margin: f64,
    d: f64,
    off: f64,
    xs: &[f64],
    ys: &[f64],
    avoid: &[(f64, f64, f64)],
) -> Mounts {
    let wall = 1.5;
    let mut m = Mounts {
        points: vec![],
        pairs: vec![],
    };
    if margin < d + 2.0 * wall - 1e-6 {
        return m;
    }
    let c = margin / 2.0;
    let free = |pts: &[(f64, f64)], p: (f64, f64), gap: f64| {
        pts.iter().all(|q| (p.0 - q.0).hypot(p.1 - q.1) >= gap)
            && avoid
                .iter()
                .all(|a| (p.0 - a.0).hypot(p.1 - a.1) >= a.2 + d / 2.0 + wall)
    };
    let sides: [(usize, f64, &[f64], f64); 4] = [
        (0, c, xs, w),
        (0, h - c, xs, w),
        (1, c, ys, h),
        (1, w - c, ys, h),
    ];
    let at = |axis: usize, fixed: f64, t: f64| if axis == 0 { (t, fixed) } else { (fixed, t) };
    for &(axis, fixed, _, len) in &sides {
        for t in [c, len - c] {
            let p = at(axis, fixed, t);
            if free(&m.points, p, 2.0 * d) {
                m.points.push(p);
            }
        }
    }
    for &(axis, fixed, seams, len) in &sides {
        for &s in &seams[1..seams.len() - 1] {
            let ps = [at(axis, fixed, s - off), at(axis, fixed, s + off)];
            if ps.iter().all(|&q| free(&m.points, q, 1.5 * d)) && s - off > 0.0 && s + off < len {
                m.points.extend(ps);
                m.pairs.push((ps[0], ps[1], axis));
            }
        }
    }
    for &(axis, fixed, seams, _) in &sides {
        for v in seams.windows(2) {
            let p = at(axis, fixed, (v[0] + v[1]) / 2.0);
            if v[1] - v[0] > 4.0 * off && free(&m.points, p, 2.0 * d) {
                m.points.push(p);
            }
        }
    }
    m
}

/// Drills through-holes (threaded or plain) at the mounting positions. `xs`/`ys` are seam coordinates in plate mm (origin 0).
pub fn drill_mounts(
    solid: &Mesh,
    p: &MountParams,
    margin: f64,
    xs: &[f64],
    ys: &[f64],
    avoid: &[(f64, f64, f64)],
) -> Result<(Mesh, Mounts, serde_json::Value), String> {
    let (screw_d, _) = thread(&p.screw)?;
    let d = screw_d + p.clearance;
    let (lo, hi) = solid.bounds();
    let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
    let m = mount_positions(w, h, margin, d, p.offset, xs, ys, avoid);
    if m.points.is_empty() {
        return Ok((
            solid.clone(),
            m,
            serde_json::json!({"holes": 0, "note": format!("margin of {margin:.1} mm is too small for {} holes (needs {:.1} mm)", p.screw, d + 3.0)}),
        ));
    }
    let top = hi[2] + 1.0;
    let per_turn = (p.segments / 6).clamp(8, 16);
    let cut = union_all(
        m.points
            .iter()
            .map(|&(x, y)| {
                let mut c = if p.threaded {
                    thread_void(&p.screw, p.clearance / 2.0, -1.0, top, p.segments, per_turn)
                        .unwrap()
                } else {
                    cylinder(d / 2.0, -1.0, top, p.segments)
                };
                c.translate([x + lo[0], y + lo[1], 0.0]);
                c
            })
            .collect(),
    );
    let out = solid.subtract(&cut)?;
    let info = serde_json::json!({"holes": m.points.len(), "pairs": m.pairs.len(), "diameter_mm": d, "threaded": p.threaded});
    Ok((out, m, info))
}

/// Through-holes (threaded or plain) at `pts`, in the solid's own coordinates.
pub fn drill_at(solid: &Mesh, pts: &[(f64, f64)], p: &MountParams) -> Result<Mesh, String> {
    let (screw_d, _) = thread(&p.screw)?;
    let d = screw_d + p.clearance;
    let top = solid.bounds().1[2] + 1.0;
    let per_turn = (p.segments / 6).clamp(8, 16);
    let cut = union_all(
        pts.iter()
            .map(|&(x, y)| {
                let mut c = if p.threaded {
                    thread_void(&p.screw, p.clearance / 2.0, -1.0, top, p.segments, per_turn)
                        .unwrap()
                } else {
                    cylinder(d / 2.0, -1.0, top, p.segments)
                };
                c.translate([x, y, 0.0]);
                c
            })
            .collect(),
    );
    solid.subtract(&cut)
}

/// What the layout keeps clear: seams (`[axis, pos, from, to]`) and connector footprints (`[x0, y0, x1, y1]`), absolute mm.
pub struct Keepout<'a> {
    pub seams: &'a [[f64; 4]],
    pub boxes: &'a [[f64; 4]],
}

impl Keepout<'_> {
    fn hits(&self, x: f64, y: f64, r: f64) -> bool {
        self.seams.iter().any(|s| {
            let (across, along) = if s[0] == 0.0 { (x, y) } else { (y, x) };
            (across - s[1]).abs() < r && along > s[2] - r && along < s[3] + r
        }) || self
            .boxes
            .iter()
            .any(|b| (x.clamp(b[0], b[2]) - x).hypot(y.clamp(b[1], b[3]) - y) < r)
    }
}

pub struct FrameLayout {
    pub pads: Vec<(f64, f64)>,
    pub holes: Vec<(f64, f64)>,
}

/// Pads and mounting holes on the middle of the frame band of a plate at `lo` of size `w` x `h` (absolute mm).
/// Pads go on the corners and, on edges longer than `long_edge`, evenly between them; holes go in the gaps every
/// `hole_spacing` mm (0 = none). A spot that would touch a seam or a connector slides along its edge until it is clear.
#[allow(clippy::too_many_arguments)]
pub fn frame_layout(
    lo: [f64; 2],
    w: f64,
    h: f64,
    margin: f64,
    long_edge: f64,
    hole_spacing: f64,
    pad_r: f64,
    hole_r: f64,
    keep: &Keepout,
) -> Result<FrameLayout, String> {
    let c = margin / 2.0;
    let (x0, x1, y0, y1) = (lo[0] + c, lo[0] + w - c, lo[1] + c, lo[1] + h - c);
    let gap = 1.5;
    let mut out = FrameLayout {
        pads: vec![],
        holes: vec![],
    };
    let free = |out: &FrameLayout, p: (f64, f64), r: f64| {
        !keep.hits(p.0, p.1, r + gap)
            && out
                .pads
                .iter()
                .all(|q| (p.0 - q.0).hypot(p.1 - q.1) >= r + pad_r + gap)
            && out
                .holes
                .iter()
                .all(|q| (p.0 - q.0).hypot(p.1 - q.1) >= r + hole_r + gap)
    };
    // candidate positions along an edge: 0, +1, -1, +2, ... mm up to `reach`
    let slide = |out: &FrameLayout, at: &dyn Fn(f64) -> (f64, f64), r: f64, reach: f64| {
        let mut d = 0.0;
        while d <= reach {
            for s in [d, -d] {
                let p = at(s);
                if free(out, p, r) {
                    return Some(p);
                }
            }
            d += 1.0;
        }
        None
    };
    for (cx, cy, sx, sy) in [
        (x0, y0, 1.0, 1.0),
        (x1, y0, -1.0, 1.0),
        (x0, y1, 1.0, -1.0),
        (x1, y1, -1.0, -1.0),
    ] {
        // corners only slide inwards, along either edge
        let p = slide(&out, &|d| (cx + sx * d.abs(), cy), pad_r, (x1 - x0) / 4.0)
            .or_else(|| slide(&out, &|d| (cx, cy + sy * d.abs()), pad_r, (y1 - y0) / 4.0))
            .ok_or_else(|| format!("no clear spot for the corner pad near ({cx:.0}, {cy:.0}) mm: a seam or connector is in the way"))?;
        out.pads.push(p);
    }
    let edges: [(bool, f64, f64, f64); 4] = [
        (true, y0, x0, x1),
        (true, y1, x0, x1),
        (false, x0, y0, y1),
        (false, x1, y0, y1),
    ];
    let pt = |horiz: bool, fixed: f64, t: f64| if horiz { (t, fixed) } else { (fixed, t) };
    for &(horiz, fixed, a, b) in &edges {
        let n = if long_edge > 0.0 {
            (((b - a) / long_edge).ceil() as usize).max(1) - 1
        } else {
            0
        };
        let mut ts = vec![a];
        for k in 1..=n {
            let want = a + (b - a) * k as f64 / (n + 1) as f64;
            let p = slide(
                &out,
                &|d| pt(horiz, fixed, want + d),
                pad_r,
                ((b - a) / (2.0 * (n + 1) as f64) - pad_r).max(0.0),
            )
            .ok_or_else(|| format!("no clear spot for a pad on the edge near {:.0} mm", want))?;
            out.pads.push(p);
            ts.push(if horiz { p.0 } else { p.1 });
        }
        ts.push(b);
        if hole_spacing > 0.0 {
            for v in ts.windows(2) {
                let m = (((v[1] - v[0]) / hole_spacing).ceil() as usize).max(1) - 1;
                for k in 1..=m {
                    let want = v[0] + (v[1] - v[0]) * k as f64 / (m + 1) as f64;
                    let reach = (v[1] - v[0]) / (2.0 * (m + 1) as f64);
                    if let Some(p) = slide(&out, &|d| pt(horiz, fixed, want + d), hole_r, reach) {
                        out.holes.push(p);
                    }
                }
            }
        }
    }
    Ok(out)
}

fn hull(mut pts: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    pts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    pts.dedup();
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut lower: Vec<[f64; 2]> = vec![];
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<[f64; 2]> = vec![];
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Joiner (two holes `2*off` apart) and extender (a second row `ext` outward), lying flat on the bed.
pub fn join_plates(
    screw: &str,
    threaded: bool,
    clearance: f64,
    off: f64,
    ext: f64,
    thickness: f64,
    segments: usize,
) -> Result<(Mesh, Mesh), String> {
    let (d, _) = thread(screw)?;
    let r = d / 2.0 + JOIN_WALL_MM;
    let make = |pts: &[(f64, f64)]| -> Result<Mesh, String> {
        let circles: Vec<[f64; 2]> = pts
            .iter()
            .flat_map(|&(x, y)| {
                (0..64).map(move |i| {
                    [
                        x + r * (TAU * i as f64 / 64.0).cos(),
                        y + r * (TAU * i as f64 / 64.0).sin(),
                    ]
                })
            })
            .collect();
        let body = Mesh::extrude_convex(&hull(circles), 0.0, thickness);
        let holes = union_all(
            pts.iter()
                .map(|&(x, y)| {
                    let mut c = if threaded {
                        thread_void(screw, clearance, -1.0, thickness + 1.0, segments, 8).unwrap()
                    } else {
                        cylinder((d + clearance) / 2.0, -1.0, thickness + 1.0, segments)
                    };
                    c.translate([x, y, 0.0]);
                    c
                })
                .collect(),
        );
        body.subtract(&holes)
    };
    let row = [(-off, 0.0), (off, 0.0)];
    Ok((
        make(&row)?,
        make(&[row[0], row[1], (-off, ext), (off, ext)])?,
    ))
}

/// Printable hex-head bolt (fully threaded, head on the bed) and its hex nut.
pub fn screw_parts(
    screw: &str,
    length: f64,
    nut_clearance: f64,
    segments: usize,
) -> Result<(Mesh, Mesh), String> {
    let (d, _) = thread(screw)?;
    let af = (1.6 * d * 10.0).round() / 10.0;
    let (head, nut) = (
        (0.7 * d * 10.0).round() / 10.0,
        (0.8 * d * 10.0).round() / 10.0,
    );
    let ac = af / (PI / 6.0).cos();
    let per_turn = (segments / 6).clamp(8, 16);
    let bolt = hexagon(ac, 0.0, head).union(&thread_void(
        screw,
        0.0,
        head - 0.01,
        head + length,
        segments,
        per_turn,
    )?)?;
    let nut = hexagon(ac, 0.0, nut).subtract(&thread_void(
        screw,
        nut_clearance,
        -0.5,
        nut + 0.5,
        segments,
        per_turn,
    )?)?;
    Ok((bolt, nut))
}

/// Calibration strip: through holes (axis y) with growing clearance and 1..n dimples on top.
pub fn fit_test(hex: bool, d: f64, steps: &[f64], segments: usize) -> Result<Mesh, String> {
    let pitch = d + 2.0 * crate::tiles::WALL_MM + 5.0;
    let maxc = steps.iter().cloned().fold(0.0, f64::max);
    let ext = if hex {
        3f64.sqrt() / 2.0 * (d + maxc)
    } else {
        d + maxc
    };
    let hgt = ext + 2.0 * crate::tiles::WALL_MM + 1.0;
    let len = pitch * steps.len() as f64 + 4.0;
    let block = Mesh::cuboid([0.0, -6.0, 0.0], [len, 6.0, hgt]);
    let mut cuts: Vec<Mesh> = vec![];
    for (i, c) in steps.iter().enumerate() {
        let x = 2.0 + pitch * (i as f64 + 0.5);
        let n = if hex { 6 } else { segments.max(12) };
        let mut hole = Mesh::prism_z(n, (d + c) / 2.0, -7.0, 7.0).along(1);
        hole.translate([x, 0.0, hgt / 2.0]);
        cuts.push(hole);
        for k in 0..=i {
            let mut dim = cylinder(0.6, hgt - 0.7, hgt + 0.7, 16);
            dim.translate([x + (k as f64 - i as f64 / 2.0) * 2.0, -4.0, 0.0]);
            cuts.push(dim);
        }
    }
    block.subtract(&union_all(cuts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_solid_is_closed() {
        for (n, d, _) in THREADS {
            let m = thread_void(n, 0.0, 0.0, 6.0, 48, 8).unwrap();
            assert!(m.is_watertight() && m.volume() > 0.0, "{n}");
            assert!(m.volume() < std::f64::consts::PI * (d / 2.0) * (d / 2.0) * 6.0);
        }
    }

    #[test]
    fn every_thread_size_cuts_a_watertight_hole_in_any_plate() {
        for (name, _, _) in THREADS {
            for t in [2.0, 4.0, 8.0, 12.0] {
                for segs in [32usize, 96] {
                    let plate = Mesh::cuboid([0.0, 0.0, 0.0], [40.0, 30.0, t]);
                    let mut cut = thread_void(name, 0.15, -1.0, t + 1.0, segs, 8).unwrap();
                    cut.translate([20.0, 15.0, 0.0]);
                    let r = plate
                        .subtract(&cut)
                        .unwrap_or_else(|e| panic!("{name} t={t} segs={segs}: {e}"));
                    assert!(r.is_watertight(), "{name} t={t} segs={segs}");
                    assert!(r.volume() < plate.volume());
                }
            }
        }
    }

    #[test]
    fn screws_nuts_plates_and_fit_test_build() {
        for (name, _, _) in THREADS {
            let (b, n) = screw_parts(name, 10.0, 0.2, 48).unwrap();
            assert!(b.is_watertight() && n.is_watertight(), "{name}");
            let (j, e) = join_plates(name, true, 0.15, 10.0, 20.0, 4.0, 48).unwrap();
            assert!(j.is_watertight() && e.is_watertight(), "{name}");
        }
        assert!(fit_test(true, 3.0, &[0.0, 0.1, 0.2, 0.3], 48)
            .unwrap()
            .is_watertight());
    }

    #[test]
    fn pads_and_mounts_on_a_framed_plate() {
        let plate = Mesh::cuboid([0.0, 0.0, 0.0], [120.0, 90.0, 6.0]);
        let (padded, info) = add_pads(
            &plate,
            &PadParams {
                count: 4,
                thread: "M6".into(),
                height: 6.0,
                clearance: 0.2,
                flip: false,
                segments: 48,
            },
            12.0,
            0.2,
        )
        .unwrap();
        assert!(padded.is_watertight() && info["pads"] == 4);
        let xs = [0.0, 60.0, 120.0];
        let ys = [0.0, 90.0];
        let avoid = vec![(6.0, 6.0, 5.0)];
        let (drilled, m, i) = drill_mounts(
            &padded,
            &MountParams {
                screw: "M4".into(),
                clearance: 0.3,
                threaded: true,
                offset: 10.0,
                segments: 48,
            },
            12.0,
            &xs,
            &ys,
            &avoid,
        )
        .unwrap();
        assert!(drilled.is_watertight() && !m.points.is_empty(), "{i}");
    }

    #[test]
    fn frame_layout_slides_off_seams_and_adds_edge_pads() {
        let seams = [[0.0, 50.0, 0.0, 400.0]];
        let keep = Keepout {
            seams: &seams,
            boxes: &[],
        };
        // the nominal midpoint of the 400 mm bottom edge is x = 200; a seam at 50 only matters for the corner pad
        let l = frame_layout(
            [0.0, 0.0],
            410.0,
            100.0,
            10.0,
            300.0,
            100.0,
            5.0,
            2.2,
            &keep,
        )
        .unwrap();
        assert_eq!(
            l.pads.len(),
            4 + 2,
            "corners plus a midpoint on each long edge"
        );
        assert!(!l.holes.is_empty());
        for p in l.pads.iter().chain(&l.holes) {
            assert!((p.0 - 50.0).abs() > 5.0, "{p:?}");
        }
        let seams = [[0.0, 7.0, 0.0, 100.0]];
        let near = Keepout {
            seams: &seams,
            boxes: &[],
        };
        let l = frame_layout([0.0, 0.0], 410.0, 100.0, 10.0, 0.0, 0.0, 5.0, 2.2, &near).unwrap();
        assert!(l.pads.iter().all(|p| (p.0 - 7.0).abs() > 5.0));
    }
}
