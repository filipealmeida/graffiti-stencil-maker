//! Image -> mask -> island-free layered plate -> watertight binary STL, in pure Rust.
//!
//! The solid is described by `kmin`: for every pixel, the first layer where it is solid (it then runs to the top).
//! Plain stencils have kmin = 0 on material. Raised bridges start higher up, so the first layer holds the real
//! artwork (islands included) and paint passes under the bridges.

use i_triangle::i_overlay::i_float::int::point::IntPoint;
use i_triangle::int::unchecked::IntUncheckedTriangulatable;
use std::collections::{HashMap, VecDeque};

pub mod colour;
pub mod graph;
pub mod hardware;
pub mod mesh;
pub mod svg;
pub mod tiles;
pub mod trace;
mod vector_stencil;
pub mod zip;

#[derive(Clone, Debug)]
pub struct Params {
    pub width_mm: f64,
    pub thickness_mm: f64,
    pub margin_mm: f64,
    pub bridge_mm: f64,
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

/// Material grid, row 0 at the bottom (y up). `true` = plate, `false` = hole (paint).
struct Grid {
    w: usize,
    h: usize,
    m: Vec<bool>,
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

    fix_diagonals(&mut grid);
    let (islands, label) = label_islands(&grid);
    stats.islands_found = islands;
    if p.min_island_mm2 > 0.0 {
        stats.islands_deleted =
            delete_small_islands(&mut grid, &label, islands, p.min_island_mm2 / (s * s));
        fix_diagonals(&mut grid);
    }
    let original = grid.m.clone();
    let radius = ((p.bridge_mm / s).round() as i64).max(1);
    let mut rise = vec![f32::NAN; w * h];
    stats.bridge_pixels = bridge_islands(&mut grid, radius, &mut rise);

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
    stats.overhang_area_mm2 = overhang_area(&kmin, w, h, l, s, lh, p.max_overhang_deg, p.flip);

    let mut mesh = layered_mesh(&kmin, w, h, l, s, lh)?;
    if p.flip {
        let (ww, tt) = (w as f32 * s as f32, l as f32 * lh as f32);
        for v in mesh.verts.iter_mut() {
            v[0] = ww - v[0];
            v[2] = tt - v[2];
        }
    }
    stats.watertight = mesh.is_watertight();
    stats.triangles = mesh.tris.len();
    let stl = mesh.to_stl();
    stats.stl_bytes = stl.len();
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

fn decode_gray(bytes: &[u8], resolution: u32) -> Result<(Vec<u8>, usize, usize), String> {
    let img = svg::open(bytes, Some(resolution))?;
    let (iw, ih) = (img.width(), img.height());
    let k = resolution as f64 / iw.max(ih) as f64;
    let (tw, th) = (
        ((iw as f64 * k).round() as u32).max(1),
        ((ih as f64 * k).round() as u32).max(1),
    );
    let rgba = img.to_rgba8();
    let rgba = if (tw, th) != (iw, ih) {
        image::imageops::resize(&rgba, tw, th, image::imageops::FilterType::Triangle)
    } else {
        rgba
    };
    let mut gray = Vec::with_capacity((tw * th) as usize);
    for px in rgba.pixels() {
        let a = px[3] as u32;
        let blend = |v: u8| (v as u32 * a + 255 * (255 - a)) / 255;
        let (r, g, b) = (blend(px[0]), blend(px[1]), blend(px[2]));
        gray.push(((299 * r + 587 * g + 114 * b) / 1000) as u8);
    }
    Ok((gray, tw as usize, th as usize))
}

/// Decodes a picture, composites transparency on white and resizes it so the longest side is `resolution`.
pub(crate) fn decode_rgb(bytes: &[u8], resolution: u32) -> Result<graph::RgbImage, String> {
    let img = svg::open(bytes, Some(resolution))?;
    let (iw, ih) = (img.width(), img.height());
    let k = resolution as f64 / iw.max(ih) as f64;
    let (tw, th) = (
        ((iw as f64 * k).round() as u32).max(1),
        ((ih as f64 * k).round() as u32).max(1),
    );
    let rgba = img.to_rgba8();
    let rgba = if (tw, th) != (iw, ih) {
        image::imageops::resize(&rgba, tw, th, image::imageops::FilterType::Lanczos3)
    } else {
        rgba
    };
    let mut rgb = Vec::with_capacity((tw * th * 3) as usize);
    for px in rgba.pixels() {
        let a = px[3] as u32;
        for c in 0..3 {
            rgb.push(((px[c] as u32 * a + 255 * (255 - a)) / 255) as u8);
        }
    }
    Ok(graph::RgbImage {
        w: tw as usize,
        h: th as usize,
        rgb,
    })
}

pub(crate) fn otsu(gray: &[u8]) -> u8 {
    let mut hist = [0u64; 256];
    for &g in gray {
        hist[g as usize] += 1;
    }
    let total = gray.len() as f64;
    let sum: f64 = hist
        .iter()
        .enumerate()
        .map(|(i, &c)| i as f64 * c as f64)
        .sum();
    let (mut wb, mut sb, mut best, mut thr) = (0.0, 0.0, -1.0, 127u8);
    for t in 0..256 {
        wb += hist[t] as f64;
        if wb == 0.0 {
            continue;
        }
        let wf = total - wb;
        if wf == 0.0 {
            break;
        }
        sb += t as f64 * hist[t] as f64;
        let (mb, mf) = (sb / wb, (sum - sb) / wf);
        let between = wb * wf * (mb - mf) * (mb - mf);
        if between > best {
            best = between;
            thr = t as u8;
        }
    }
    thr.saturating_add(1)
}

/// 4-connected plate components. Returns (islands, label per pixel); the frame is the component at (0,0).
fn label_islands(g: &Grid) -> (usize, Vec<u32>) {
    let mut label = vec![0u32; g.w * g.h];
    let mut n = 0u32;
    let mut stack = Vec::new();
    for start in 0..g.w * g.h {
        if !g.m[start] || label[start] != 0 {
            continue;
        }
        n += 1;
        label[start] = n;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % g.w, i / g.w);
            let mut push = |j: usize| {
                if g.m[j] && label[j] == 0 {
                    label[j] = n;
                    stack.push(j);
                }
            };
            if x > 0 {
                push(i - 1);
            }
            if x + 1 < g.w {
                push(i + 1);
            }
            if y > 0 {
                push(i - g.w);
            }
            if y + 1 < g.h {
                push(i + g.w);
            }
        }
    }
    (n.saturating_sub(1) as usize, label)
}

/// Squared-distance transform (Felzenszwalb): distance in pixels from every pixel to the nearest `true` pixel.
fn edt_sq(src: &[bool], w: usize, h: usize) -> Vec<f32> {
    const INF: f32 = 1e12;
    let mut g: Vec<f32> = src.iter().map(|&b| if b { 0.0 } else { INF }).collect();
    let pass = |f: &[f32], n: usize| -> Vec<f32> {
        let mut d = vec![0f32; n];
        let mut v = vec![0usize; n];
        let mut z = vec![0f32; n + 1];
        let mut k = 0usize;
        z[0] = -INF;
        z[1] = INF;
        for q in 1..n {
            loop {
                let r = v[k];
                let s = ((f[q] + (q * q) as f32) - (f[r] + (r * r) as f32))
                    / (2.0 * (q as f32 - r as f32));
                if s <= z[k] && k > 0 {
                    k -= 1;
                    continue;
                }
                if s <= z[k] {
                    v[0] = q;
                    z[1] = INF;
                    break;
                }
                k += 1;
                v[k] = q;
                z[k] = s;
                z[k + 1] = INF;
                break;
            }
        }
        k = 0;
        for q in 0..n {
            while z[k + 1] < q as f32 {
                k += 1;
            }
            let r = v[k];
            d[q] = (q as f32 - r as f32).powi(2) + f[r];
        }
        d
    };
    let mut col = vec![0f32; h];
    for x in 0..w {
        for y in 0..h {
            col[y] = g[y * w + x];
        }
        let d = pass(&col, h);
        for y in 0..h {
            g[y * w + x] = d[y];
        }
    }
    for y in 0..h {
        let d = pass(&g[y * w..(y + 1) * w].to_vec(), w);
        g[y * w..(y + 1) * w].copy_from_slice(&d);
    }
    g
}

/// Area (mm2, summed over printed layers) that sticks out more than layer_height*tan(angle) past the layer below.
#[allow(clippy::too_many_arguments)]
fn overhang_area(
    kmin: &[u32],
    w: usize,
    h: usize,
    l: u32,
    s: f64,
    lh: f64,
    angle: f64,
    flip: bool,
) -> f64 {
    let angle = if angle <= 0.0 { 45.0 } else { angle.min(89.0) };
    let reach = lh * angle.to_radians().tan() / s + 1e-3;
    let mut total = 0usize;
    let layer = |k: u32| -> Vec<bool> { kmin.iter().map(|&q| q <= k).collect() };
    for j in 1..l {
        let (cur_k, prev_k) = if flip { (l - 1 - j, l - j) } else { (j, j - 1) };
        if (0..w * h).all(|i| (kmin[i] <= cur_k) == (kmin[i] <= prev_k)) {
            continue;
        }
        let (cur, prev) = (layer(cur_k), layer(prev_k));
        let d = edt_sq(&prev, w, h);
        total += (0..w * h)
            .filter(|&i| cur[i] && (d[i].sqrt() as f64) > reach)
            .count();
    }
    total as f64 * s * s
}

fn delete_small_islands(g: &mut Grid, label: &[u32], n: usize, min_px: f64) -> usize {
    let mut area = vec![0usize; n + 2];
    for &q in label {
        area[q as usize] += 1;
    }
    let frame = label[0];
    let mut deleted = 0;
    let mut gone = vec![false; n + 2];
    for q in 1..=n + 1 {
        if q as u32 != frame && area[q] > 0 && (area[q] as f64) < min_px {
            gone[q] = true;
            deleted += 1;
        }
    }
    for i in 0..label.len() {
        if label[i] != 0 && gone[label[i] as usize] {
            g.m[i] = false;
        }
    }
    deleted
}

/// Pixels in the kmin map that touch only diagonally in some layer would give a non-manifold edge: fill one.
fn fix_diagonals_k(k: &mut [u32], w: usize, h: usize, l: u32) {
    loop {
        let mut changed = false;
        for y in 0..h.saturating_sub(1) {
            for x in 0..w.saturating_sub(1) {
                let (ia, ib, ic, id) = (
                    y * w + x,
                    y * w + x + 1,
                    (y + 1) * w + x,
                    (y + 1) * w + x + 1,
                );
                let (a, b, c, d) = (k[ia], k[ib], k[ic], k[id]);
                let (hi_ad, lo_bc) = (a.max(d), b.min(c));
                if hi_ad < l && hi_ad < lo_bc {
                    k[ib] = hi_ad;
                    changed = true;
                }
                let (hi_bc, lo_ad) = (b.max(c), a.min(d));
                if hi_bc < l && hi_bc < lo_ad {
                    k[ia] = hi_bc;
                    changed = true;
                }
            }
        }
        if !changed {
            return;
        }
    }
}

/// Two plate pixels touching only at a corner would give a non-manifold edge. Fill one hole pixel.
fn fix_diagonals(g: &mut Grid) {
    loop {
        let mut changed = false;
        for y in 0..g.h.saturating_sub(1) {
            for x in 0..g.w.saturating_sub(1) {
                let (a, b, c, d) = (
                    g.m[y * g.w + x],
                    g.m[y * g.w + x + 1],
                    g.m[(y + 1) * g.w + x],
                    g.m[(y + 1) * g.w + x + 1],
                );
                if a && d && !b && !c {
                    g.m[y * g.w + x + 1] = true;
                    changed = true;
                } else if b && c && !a && !d {
                    g.m[y * g.w + x] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            return;
        }
    }
}

/// Connect every island to the frame by the cheapest path (hole pixels cost 1, plate pixels 0).
/// `rise` gets, on every new bridge pixel, a value in (0,1): 0 at the island, 1 at the frame.
fn bridge_islands(g: &mut Grid, radius: i64, rise: &mut [f32]) -> usize {
    let (w, h) = (g.w, g.h);
    let (islands, label) = label_islands(g);
    if islands == 0 {
        return 0;
    }
    let frame = label[0];
    let mut cost = vec![u32::MAX; w * h];
    let mut parent = vec![u32::MAX; w * h];
    let mut dq = VecDeque::new();
    for i in 0..w * h {
        if label[i] == frame {
            cost[i] = 0;
            dq.push_back(i);
        }
    }
    while let Some(i) = dq.pop_front() {
        let (x, y) = ((i % w) as i64, (i / w) as i64);
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                continue;
            }
            let j = ny as usize * w + nx as usize;
            let step = if g.m[j] { 0 } else { 1 };
            if cost[i] + step < cost[j] {
                cost[j] = cost[i] + step;
                parent[j] = i as u32;
                if step == 0 {
                    dq.push_front(j)
                } else {
                    dq.push_back(j)
                }
            }
        }
    }
    let mut best: HashMap<u32, usize> = HashMap::new();
    for i in 0..w * h {
        if g.m[i] && label[i] != frame && cost[i] != u32::MAX {
            let e = best.entry(label[i]).or_insert(i);
            if cost[i] < cost[*e] {
                *e = i;
            }
        }
    }
    let mut starts: Vec<usize> = best.values().copied().collect();
    starts.sort_unstable();
    let original = g.m.clone();
    let mut carved = 0;
    for start in starts {
        let mut path = Vec::new();
        let mut i = start;
        while parent[i] != u32::MAX {
            path.push(i);
            i = parent[i] as usize;
        }
        let holes: Vec<usize> = path.into_iter().filter(|&q| !original[q]).collect();
        let n = holes.len() as f32;
        for (j, &q) in holes.iter().enumerate() {
            let r = (j as f32 + 1.0) / (n + 1.0);
            let (x, y) = ((q % w) as i64, (q / w) as i64);
            for dy in 0..radius {
                for dx in 0..radius {
                    let (cx, cy) = (x + dx - radius / 2, y + dy - radius / 2);
                    if cx >= 0 && cy >= 0 && cx < w as i64 && cy < h as i64 {
                        let t = cy as usize * w + cx as usize;
                        if !original[t] {
                            if !g.m[t] {
                                g.m[t] = true;
                                carved += 1;
                            }
                            if rise[t].is_nan() || r < rise[t] {
                                rise[t] = r;
                            }
                        }
                    }
                }
            }
        }
    }
    carved
}

type Vid = u32;

struct Edge {
    from: Vid,
    to: Vid,
    tag: u32,
}

struct Loop {
    v: Vec<Vid>,
    tag: Vec<u32>, // tag of the edge leaving vertex i
}

fn trace(edges: &[Edge], vw: usize) -> Vec<Loop> {
    let xy = |v: Vid| ((v as usize % vw) as i64, (v as usize / vw) as i64);
    let mut by_from: HashMap<Vid, Vec<usize>> = HashMap::new();
    for (i, e) in edges.iter().enumerate() {
        by_from.entry(e.from).or_default().push(i);
    }
    let mut used = vec![false; edges.len()];
    let mut loops = Vec::new();
    for s in 0..edges.len() {
        if used[s] {
            continue;
        }
        let (mut v, mut tag) = (Vec::new(), Vec::new());
        let mut cur = s;
        loop {
            used[cur] = true;
            v.push(edges[cur].from);
            tag.push(edges[cur].tag);
            let (a, b) = (xy(edges[cur].from), xy(edges[cur].to));
            let u = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
            // Leftmost turn keeps pinched corners as separate loops.
            let next = by_from[&edges[cur].to]
                .iter()
                .copied()
                .filter(|&j| !used[j] || j == s)
                .max_by(|&p, &q| {
                    let ang = |j: usize| {
                        let (c, d) = (xy(edges[j].from), xy(edges[j].to));
                        let wv = ((d.0 - c.0) as f64, (d.1 - c.1) as f64);
                        (u.0 * wv.1 - u.1 * wv.0).atan2(u.0 * wv.0 + u.1 * wv.1)
                    };
                    ang(p).partial_cmp(&ang(q)).unwrap()
                });
            match next {
                Some(j) if j != s => cur = j,
                _ => break,
            }
        }
        loops.push(Loop { v, tag });
    }
    loops
}

/// Drop vertices on straight runs, but only where the wall height on both sides of the vertex is the same.
fn simplify(lp: Loop, vw: usize) -> Loop {
    let xy = |v: Vid| ((v as usize % vw) as i64, (v as usize / vw) as i64);
    let n = lp.v.len();
    let (mut v, mut tag) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for i in 0..n {
        let (a, b, c) = (
            xy(lp.v[(i + n - 1) % n]),
            xy(lp.v[i]),
            xy(lp.v[(i + 1) % n]),
        );
        let straight = (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0) == 0
            && (b.0 - a.0) * (c.0 - b.0) + (b.1 - a.1) * (c.1 - b.1) > 0;
        if !(straight && lp.tag[(i + n - 1) % n] == lp.tag[i]) {
            v.push(lp.v[i]);
            tag.push(lp.tag[i]);
        }
    }
    Loop { v, tag }
}

struct Mesh {
    verts: Vec<[f32; 3]>,
    tris: Vec<[u32; 3]>,
}

impl Mesh {
    fn is_watertight(&self) -> bool {
        let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
        for t in &self.tris {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                if a == b {
                    return false;
                }
                *edges.entry((a, b)).or_insert(0) += 1;
            }
        }
        let bad = edges
            .iter()
            .filter(|(&(a, b), &n)| !(n == 1 && edges.get(&(b, a)) == Some(&1)))
            .count();
        if std::env::var("DBG").is_ok() {
            eprintln!("bad edges {bad} of {}", edges.len());
            for (&(a, b), &n) in edges
                .iter()
                .filter(|(&(a, b), &n)| !(n == 1 && edges.get(&(b, a)) == Some(&1)))
                .take(40)
            {
                eprintln!(
                    "{:?}->{:?} n={} rev={:?}",
                    self.verts[a as usize],
                    self.verts[b as usize],
                    n,
                    edges.get(&(b, a))
                );
            }
        }
        bad == 0
    }

    fn to_stl(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(84 + self.tris.len() * 50);
        out.extend_from_slice(&[0u8; 80]);
        out.extend_from_slice(&(self.tris.len() as u32).to_le_bytes());
        for t in &self.tris {
            let (a, b, c) = (
                self.verts[t[0] as usize],
                self.verts[t[1] as usize],
                self.verts[t[2] as usize],
            );
            let (u, v) = (
                [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
            );
            let mut n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len > 0.0 {
                n = [n[0] / len, n[1] / len, n[2] / len];
            }
            for f in n.iter().chain(a.iter()).chain(b.iter()).chain(c.iter()) {
                out.extend_from_slice(&f.to_le_bytes());
            }
            out.extend_from_slice(&[0, 0]);
        }
        out
    }
}

fn signed_area(lp: &[Vid], vw: usize) -> f64 {
    let xy = |v: Vid| ((v as usize % vw) as f64, (v as usize / vw) as f64);
    let mut a = 0.0;
    for i in 0..lp.len() {
        let (p, q) = (xy(lp[i]), xy(lp[(i + 1) % lp.len()]));
        a += p.0 * q.1 - q.0 * p.1;
    }
    a / 2.0
}

fn point_in(lp: &[Vid], vw: usize, pt: (f64, f64)) -> bool {
    let xy = |v: Vid| ((v as usize % vw) as f64, (v as usize / vw) as f64);
    let mut inside = false;
    let mut j = lp.len() - 1;
    for i in 0..lp.len() {
        let (a, b) = (xy(lp[i]), xy(lp[j]));
        if (a.1 > pt.1) != (b.1 > pt.1) && pt.0 < (b.0 - a.0) * (pt.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Boundary edges (solid on the left) of the pixel set `sel`; `tag(inside, outside)` labels each edge.
fn boundary_edges(
    pixels: &[usize],
    inside: impl Fn(usize) -> bool,
    tag: impl Fn(usize, i64, i64) -> u32,
    w: usize,
    h: usize,
) -> Vec<Edge> {
    let vw = w + 1;
    let vid = |x: usize, y: usize| (y * vw + x) as Vid;
    let mut edges = Vec::new();
    for &i in pixels {
        let (x, y) = (i % w, i / w);
        let side = |dx: i64, dy: i64| -> bool {
            let (nx, ny) = (x as i64 + dx, y as i64 + dy);
            nx >= 0
                && ny >= 0
                && (nx as usize) < w
                && (ny as usize) < h
                && inside(ny as usize * w + nx as usize)
        };
        if !side(0, -1) {
            edges.push(Edge {
                from: vid(x, y),
                to: vid(x + 1, y),
                tag: tag(i, 0, -1),
            });
        }
        if !side(1, 0) {
            edges.push(Edge {
                from: vid(x + 1, y),
                to: vid(x + 1, y + 1),
                tag: tag(i, 1, 0),
            });
        }
        if !side(0, 1) {
            edges.push(Edge {
                from: vid(x + 1, y + 1),
                to: vid(x, y + 1),
                tag: tag(i, 0, 1),
            });
        }
        if !side(-1, 0) {
            edges.push(Edge {
                from: vid(x, y + 1),
                to: vid(x, y),
                tag: tag(i, -1, 0),
            });
        }
    }
    edges
}

fn layered_mesh(kmin: &[u32], w: usize, h: usize, l: u32, s: f64, lh: f64) -> Result<Mesh, String> {
    let vw = w + 1;
    let xy = |v: Vid| ((v as usize % vw) as i64, (v as usize / vw) as i64);
    let mut mesh = Mesh {
        verts: Vec::new(),
        tris: Vec::new(),
    };
    let mut ids: HashMap<(Vid, u32), u32> = HashMap::new();
    let mut vertex = |v: Vid, k: u32, mesh: &mut Mesh| -> u32 {
        *ids.entry((v, k)).or_insert_with(|| {
            let (x, y) = xy(v);
            mesh.verts.push([
                (x as f64 * s) as f32,
                (y as f64 * s) as f32,
                (k as f64 * lh) as f32,
            ]);
            (mesh.verts.len() - 1) as u32
        })
    };
    let kn = |i: usize, dx: i64, dy: i64| -> u32 {
        let (x, y) = ((i % w) as i64 + dx, (i / w) as i64 + dy);
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            l
        } else {
            kmin[y as usize * w + x as usize]
        }
    };

    let column_levels = |v: Vid, lo: u32, hi: u32| -> Vec<u32> {
        let (x, y) = xy(v);
        let mut out = Vec::new();
        for (dx, dy) in [(-1, -1), (0, -1), (-1, 0), (0, 0)] {
            let (px, py) = (x + dx, y + dy);
            if px >= 0 && py >= 0 && px < w as i64 && py < h as i64 {
                let k = kmin[py as usize * w + px as usize];
                if k > lo && k < hi {
                    out.push(k);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    };

    // Planes: one cap per distinct kmin value (facing down) and the top cap at plane l (facing up).
    let mut by_k: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, &k) in kmin.iter().enumerate() {
        if k < l {
            by_k.entry(k).or_default().push(i);
        }
    }
    let mut planes: Vec<u32> = by_k.keys().copied().collect();
    planes.sort_unstable();
    let all: Vec<usize> = by_k.values().flatten().copied().collect();

    let mut jobs: Vec<(u32, bool, Vec<Loop>)> = Vec::new();
    for &k in &planes {
        let edges = boundary_edges(&by_k[&k], |j| kmin[j] == k, |i, dx, dy| kn(i, dx, dy), w, h);
        jobs.push((
            k,
            false,
            trace(&edges, vw)
                .into_iter()
                .map(|q| simplify(q, vw))
                .collect(),
        ));
    }
    let edges = boundary_edges(&all, |j| kmin[j] < l, |i, _, _| kmin[i], w, h);
    jobs.push((
        l,
        true,
        trace(&edges, vw)
            .into_iter()
            .map(|q| simplify(q, vw))
            .collect(),
    ));

    for (plane, up, loops) in &jobs {
        let areas: Vec<f64> = loops.iter().map(|q| signed_area(&q.v, vw)).collect();
        let outers: Vec<usize> = (0..loops.len()).filter(|&i| areas[i] > 0.0).collect();
        let bbox = |q: &Loop| {
            let (mut a, mut b, mut c, mut d) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
            for &v in &q.v {
                let (x, y) = xy(v);
                a = a.min(x);
                b = b.min(y);
                c = c.max(x);
                d = d.max(y);
            }
            (a, b, c, d)
        };
        let boxes: Vec<_> = loops.iter().map(bbox).collect();
        let mut holes_of: HashMap<usize, Vec<usize>> = HashMap::new();
        for hi in (0..loops.len()).filter(|&i| areas[i] < 0.0) {
            let pt = xy(loops[hi].v[0]);
            let ptf = (pt.0 as f64, pt.1 as f64);
            let hb = boxes[hi];
            let owner = outers
                .iter()
                .copied()
                .filter(|&o| {
                    let ob = boxes[o];
                    ob.0 <= hb.0
                        && ob.1 <= hb.1
                        && ob.2 >= hb.2
                        && ob.3 >= hb.3
                        && (point_in(&loops[o].v, vw, (ptf.0 + 1e-3, ptf.1 + 1e-3))
                            || point_in(&loops[o].v, vw, (ptf.0 - 1e-3, ptf.1 - 1e-3))
                            || point_in(&loops[o].v, vw, (ptf.0 + 1e-3, ptf.1 - 1e-3))
                            || point_in(&loops[o].v, vw, (ptf.0 - 1e-3, ptf.1 + 1e-3)))
                })
                .min_by(|&a, &b| areas[a].partial_cmp(&areas[b]).unwrap());
            holes_of
                .entry(owner.ok_or("hole without outer loop")?)
                .or_default()
                .push(hi);
        }
        for &o in &outers {
            let group: Vec<usize> = std::iter::once(o)
                .chain(holes_of.get(&o).into_iter().flatten().copied())
                .collect();
            let shape: Vec<Vec<IntPoint>> = group
                .iter()
                .map(|&li| {
                    loops[li]
                        .v
                        .iter()
                        .map(|&v| {
                            let (x, y) = xy(v);
                            IntPoint::new(x as i32, y as i32)
                        })
                        .collect()
                })
                .collect();
            let tri = shape.uncheck_triangulate().into_triangulation::<u32>();
            for c in tri.indices.chunks(3) {
                let (pa, pb, pc) = (
                    tri.points[c[0] as usize],
                    tri.points[c[1] as usize],
                    tri.points[c[2] as usize],
                );
                let cross = (pb.x as i64 - pa.x as i64) * (pc.y as i64 - pa.y as i64)
                    - (pb.y as i64 - pa.y as i64) * (pc.x as i64 - pa.x as i64);
                if cross == 0 {
                    continue;
                }
                let vid_of = |q: &IntPoint| (q.y as usize * vw + q.x as usize) as Vid;
                let (a, b, d) = (
                    vertex(vid_of(&pa), *plane, &mut mesh),
                    vertex(vid_of(&pb), *plane, &mut mesh),
                    vertex(vid_of(&pc), *plane, &mut mesh),
                );
                let ccw = (cross > 0) == *up;
                mesh.tris.push(if ccw { [a, b, d] } else { [a, d, b] });
            }
        }
        if !*up {
            for lp in loops {
                let n = lp.v.len();
                for i in 0..n {
                    let (v0, v1) = (lp.v[i], lp.v[(i + 1) % n]);
                    let j = lp.tag[i];
                    if j > *plane {
                        // Vertical edges carry every plane that touches the column, so neighbouring walls share vertices.
                        let mut poly =
                            vec![vertex(v0, *plane, &mut mesh), vertex(v1, *plane, &mut mesh)];
                        for m in column_levels(v1, *plane, j) {
                            poly.push(vertex(v1, m, &mut mesh));
                        }
                        poly.push(vertex(v1, j, &mut mesh));
                        poly.push(vertex(v0, j, &mut mesh));
                        for m in column_levels(v0, *plane, j).into_iter().rev() {
                            poly.push(vertex(v0, m, &mut mesh));
                        }
                        for t in 1..poly.len() - 1 {
                            mesh.tris.push([poly[0], poly[t], poly[t + 1]]);
                        }
                    }
                }
            }
        }
    }
    Ok(mesh)
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
