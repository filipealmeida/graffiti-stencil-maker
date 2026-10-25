//! Making the stacked outlines manifold: point-contact repair, shared noding between runs and thin-segment widening.

use super::*;

pub(super) fn repair_point_contacts(
    shapes: &[Vec<Vec<IntPoint<i32>>>],
) -> Vec<Vec<Vec<IntPoint<i32>>>> {
    let mut counts = HashMap::new();
    for contour in shapes.iter().flat_map(|shape| shape.iter()) {
        for p in contour {
            *counts.entry((p.x, p.y)).or_insert(0usize) += 1;
        }
    }
    let radius = (0.01 * UNITS_PER_MM) as i32;
    let patches: Vec<Vec<IntPoint<i32>>> = counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|((x, y), _)| {
            vec![
                IntPoint::new(x - radius, y - radius),
                IntPoint::new(x + radius, y - radius),
                IntPoint::new(x + radius, y + radius),
                IntPoint::new(x - radius, y + radius),
            ]
        })
        .collect();
    if patches.is_empty() {
        shapes.to_vec()
    } else {
        shapes.overlay(patches.as_slice(), OverlayRule::Union, FillRule::NonZero)
    }
}

/// A shape: its outer outline and holes, as integer polygons.
pub(super) type Shape = Vec<Vec<IntPoint<i32>>>;

/// A contour index, a vertex index within it, and the vertex.
pub(super) type Vertex = (usize, usize, IntPoint<i32>);

/// What an earlier noding round already looked at: every vertex, and every edge it left behind.
#[derive(Default)]
pub(super) struct Seen {
    pub(super) vertices: FastSet<(i32, i32)>,
    pub(super) edges: FastSet<[i32; 4]>,
}

/// Every run shares the vertices created where bridges cross the outlines: an edge passing within a unit or two
/// of another run's vertex gets that vertex too, so all runs agree on where the walls break.
///
/// Later rounds are incremental: an edge that survived the last round unchanged only needs checking against the
/// vertices that appeared since then (`seen` remembers the earlier ones); an edge a repair created is checked
/// against everything. Runs that gain a vertex are marked in `dirty`.
pub(super) fn node_runs(runs: &mut [Vec<Shape>], dirty: &mut [bool], seen: &mut Seen) {
    type Contour = Vec<IntPoint<i32>>;
    const CELL: i64 = 4096;
    let key = |x: i64, y: i64| (x.div_euclid(CELL), y.div_euclid(CELL));
    let mut grid: FastMap<(i64, i64), Vec<IntPoint<i32>>> = FastMap::default();
    let mut fresh: FastMap<(i64, i64), Vec<IntPoint<i32>>> = FastMap::default();
    let mut now = FastSet::default();
    for r in runs.iter().flatten().flatten().flatten() {
        if now.insert((r.x, r.y)) {
            let cell = key(r.x as i64, r.y as i64);
            grid.entry(cell).or_default().push(*r);
            if !seen.vertices.contains(&(r.x, r.y)) {
                fresh.entry(cell).or_default().push(*r);
            }
        }
    }
    seen.vertices = now;
    let sizes: Vec<usize> = runs
        .iter()
        .map(|r| r.iter().flatten().map(Vec::len).sum())
        .collect();
    let mut contours: Vec<&mut Contour> = runs
        .iter_mut()
        .flat_map(|run| run.iter_mut().flatten())
        .collect();
    let before = &seen.edges;
    crate::par::each_mut(&mut contours, |contour| {
        let n = contour.len();
        let mut out = Vec::with_capacity(n);
        let mut cells: Vec<(i64, i64)> = Vec::new();
        for i in 0..n {
            let (p, q) = (contour[i], contour[(i + 1) % n]);
            out.push(p);
            let grid = if before.contains(&[p.x, p.y, q.x, q.y]) {
                &fresh
            } else {
                &grid
            };
            if grid.is_empty() {
                continue;
            }
            let (dx, dy) = (
                (q.x as i64 - p.x as i64) as f64,
                (q.y as i64 - p.y as i64) as f64,
            );
            let l2 = dx * dx + dy * dy;
            if l2 == 0.0 {
                continue;
            }
            let (c0, c1) = (key(p.x as i64, p.y as i64), key(q.x as i64, q.y as i64));
            cells.clear();
            if (c0.0 - c1.0).abs() <= 1 && (c0.1 - c1.1).abs() <= 1 {
                // a short edge: the cells around its two ends cover every vertex that can lie near it
                for cx in c0.0.min(c1.0) - 1..=c0.0.max(c1.0) + 1 {
                    for cy in c0.1.min(c1.1) - 1..=c0.1.max(c1.1) + 1 {
                        cells.push((cx, cy));
                    }
                }
            } else {
                let samples = (l2.sqrt() / CELL as f64).ceil() as i64 + 1;
                for s in 0..=samples {
                    let t = s as f64 / samples as f64;
                    let (x, y) = (p.x as f64 + dx * t, p.y as f64 + dy * t);
                    let (cx, cy) = key(x as i64, y as i64);
                    for a in -1..=1 {
                        for b in -1..=1 {
                            cells.push((cx + a, cy + b));
                        }
                    }
                }
                cells.sort_unstable();
                cells.dedup();
            }
            let mut found: Vec<(f64, IntPoint<i32>)> = Vec::new();
            for cell in &cells {
                for &r in grid.get(cell).into_iter().flatten() {
                    if r == p || r == q {
                        continue;
                    }
                    let (rx, ry) = (
                        (r.x as i64 - p.x as i64) as f64,
                        (r.y as i64 - p.y as i64) as f64,
                    );
                    let t = (rx * dx + ry * dy) / l2;
                    if t <= 0.0 || t >= 1.0 {
                        continue;
                    }
                    if (rx * dy - ry * dx).abs() / l2.sqrt() <= 2.0 {
                        found.push((t, r));
                    }
                }
            }
            found.sort_by(|a, b| a.0.total_cmp(&b.0));
            out.extend(found.into_iter().map(|f| f.1));
        }
        **contour = out;
    });
    seen.edges = contours
        .iter()
        .flat_map(|c| (0..c.len()).map(move |i| (c[i], c[(i + 1) % c.len()])))
        .map(|(p, q)| [p.x, p.y, q.x, q.y])
        .collect();
    for (d, (run, before)) in dirty.iter_mut().zip(runs.iter().zip(sizes)) {
        *d |= run.iter().flatten().map(Vec::len).sum::<usize>() != before;
    }
}

/// Like `repair_point_contacts`, but also for vertices a few units from another vertex of the outlines: rounding
/// turns such near-touches and tiny jogs into pinch points that no manifold mesh can have.
pub(super) fn repair_near_contacts(
    shapes: &[Vec<Vec<IntPoint<i32>>>],
    radius: i32,
) -> (Vec<Shape>, usize) {
    const NEAR: i32 = 3;
    let mut grid: FastMap<(i32, i32), Vec<Vertex>> = FastMap::default();
    let mut lens: Vec<usize> = Vec::new();
    for (id, contour) in shapes.iter().flatten().enumerate() {
        lens.push(contour.len());
        for (i, p) in contour.iter().enumerate() {
            grid.entry((p.x.div_euclid(8), p.y.div_euclid(8)))
                .or_default()
                .push((id, i, *p));
        }
    }
    let mut at: FastSet<(i32, i32)> = Default::default();
    for cell in grid.values() {
        for &(ca, ia, pa) in cell {
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let key = (pa.x.div_euclid(8) + dx, pa.y.div_euclid(8) + dy);
                    for &(cb, ib, pb) in grid.get(&key).into_iter().flatten() {
                        // neighbours along one outline are an edge, not a contact
                        let n = lens[ca];
                        let next = ca == cb && ((ia + 1) % n == ib || (ib + 1) % n == ia);
                        if (cb, ib) == (ca, ia)
                            || next
                            || (pa.x - pb.x).abs() > NEAR
                            || (pa.y - pb.y).abs() > NEAR
                        {
                            continue;
                        }
                        at.insert((pa.x, pa.y));
                    }
                }
            }
        }
    }
    let patches: Vec<Vec<IntPoint<i32>>> = at
        .into_iter()
        .map(|(x, y)| {
            vec![
                IntPoint::new(x - radius, y - radius),
                IntPoint::new(x + radius, y - radius),
                IntPoint::new(x + radius, y + radius),
                IntPoint::new(x - radius, y + radius),
            ]
        })
        .collect();
    let count = patches.len();
    if patches.is_empty() {
        (shapes.to_vec(), 0)
    } else {
        (
            shapes.overlay(patches.as_slice(), OverlayRule::Union, FillRule::NonZero),
            count,
        )
    }
}

/// Widens every stretch of material narrower than `min_mm` that is big enough to matter (a rope between two
/// parts, a thin tip) to that width. Thin parts are what an opening by half the width removes; they grow into
/// the paint side only, never past the plate.
pub(super) fn widen_thin(
    material: &[Vec<Vec<IntPoint<i32>>>],
    plate: &[IntPoint<i32>],
    min_mm: f64,
) -> Vec<Vec<Vec<IntPoint<i32>>>> {
    let r = min_mm * 0.5 * UNITS_PER_MM;
    let plate_region = [vec![plate.to_vec()]];
    let plate_region = &plate_region[0];
    let paint =
        plate_region
            .as_slice()
            .overlay(material, OverlayRule::Difference, FillRule::EvenOdd);
    let eroded = plate_region.as_slice().overlay(
        grow(&paint, r).as_slice(),
        OverlayRule::Difference,
        FillRule::EvenOdd,
    );
    let opening = grow(&eroded, r);
    let min_area = 0.5 * min_mm * min_mm;
    let boxes: Vec<[i32; 4]> = opening.iter().map(|s| bounds(s)).collect();
    // A rope joins two different thick parts; slivers along one part's outline and thin tips touch only one.
    let thin: Vec<_> = material
        .overlay(
            opening.as_slice(),
            OverlayRule::Difference,
            FillRule::EvenOdd,
        )
        .into_iter()
        .filter(|s| shape_area(s) >= min_area)
        .filter(|s| {
            let reach = grow(&vec![s.clone()], 50.0);
            let Some(first) = reach.first() else {
                return false;
            };
            let b = bounds(first);
            let touching = (0..opening.len())
                .filter(|&j| {
                    let o = boxes[j];
                    o[0] <= b[2] && b[0] <= o[2] && o[1] <= b[3] && b[1] <= o[3]
                })
                .filter(|&j| {
                    !reach
                        .as_slice()
                        .overlay(
                            opening[j].as_slice(),
                            OverlayRule::Intersect,
                            FillRule::NonZero,
                        )
                        .is_empty()
                })
                .count();
            touching >= 2
        })
        .collect();
    if thin.is_empty() {
        return material.to_vec();
    }
    let widened = material.overlay(
        grow(&thin, r).as_slice(),
        OverlayRule::Union,
        FillRule::NonZero,
    );
    widened.as_slice().overlay(
        plate_region.as_slice(),
        OverlayRule::Intersect,
        FillRule::NonZero,
    )
}

pub(super) fn first_layer_flare_parts(
    material: &[Shape],
    plate: &[IntPoint<i32>],
    min_width_mm: f64,
) -> Flare {
    let radius = min_width_mm * 0.5 * UNITS_PER_MM;
    let plate_region = [vec![plate.to_vec()]];
    let plate_region = &plate_region[0];
    let paint =
        plate_region
            .as_slice()
            .overlay(material, OverlayRule::Difference, FillRule::EvenOdd);
    let eroded = plate_region.as_slice().overlay(
        grow(&paint, radius).as_slice(),
        OverlayRule::Difference,
        FillRule::EvenOdd,
    );
    let opening = grow(&eroded, radius);
    let min_area = 0.5 * min_width_mm * min_width_mm;
    let candidates: Vec<_> = material
        .overlay(
            opening.as_slice(),
            OverlayRule::Difference,
            FillRule::EvenOdd,
        )
        .into_iter()
        .filter_map(|shape| {
            let area = shape_area(&shape);
            if area < min_area {
                return None;
            }
            let b = bounds(&shape);
            let length = (b[2] - b[0]).max(b[3] - b[1]) as f64 / UNITS_PER_MM;
            if length <= 0.0 {
                return None;
            }
            let estimated_width = area / length;
            let expansion = ((min_width_mm - estimated_width).max(0.0) * 0.5) * UNITS_PER_MM;
            (expansion > 0.0).then_some((shape, expansion))
        })
        .collect();
    // A patch can only take paint away from the voids it overlaps, so each candidate is checked against those voids
    // and the patches already accepted around them instead of rebuilding the whole plate.
    let voids: Vec<([i32; 4], &Shape)> = paint.iter().map(|v| (bounds(v), v)).collect();
    let overlaps =
        |a: &[i32; 4], b: &[i32; 4]| a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3];
    let mut accepted = Vec::new();
    let mut placed: Vec<([i32; 4], Shape)> = Vec::new();
    for (shape, expansion) in candidates {
        let patch = grow(&vec![shape.clone()], expansion);
        let patch_boxes: Vec<[i32; 4]> = patch.iter().map(|s| bounds(s)).collect();
        let hits = |b: &[i32; 4]| patch_boxes.iter().any(|p| overlaps(p, b));
        let preserves_openings = voids.iter().filter(|(b, _)| hits(b)).all(|(vb, void)| {
            let mut cover: Vec<Shape> = patch.clone();
            cover.extend(
                placed
                    .iter()
                    .filter(|(b, _)| overlaps(b, vb))
                    .map(|(_, s)| s.clone()),
            );
            !vec![(*void).clone()]
                .as_slice()
                .overlay(cover.as_slice(), OverlayRule::Difference, FillRule::NonZero)
                .is_empty()
        });
        if preserves_openings {
            placed.extend(patch.into_iter().map(|s| (bounds(&s), s)));
            accepted.push((shape, expansion));
        }
    }
    Flare::new(&accepted)
}

/// The widening of the thin parts as distances on one lattice shared by every layer of the taper. Each layer takes
/// the cells within its own radius, so layers nest exactly and their edges coincide instead of being offset curves
/// that nearly touch (which the meshing could not settle).
pub(super) struct Flare {
    cell: i64,
    fields: Vec<FlareField>,
}

struct FlareField {
    i0: i64,
    j0: i64,
    nx: usize,
    ny: usize,
    /// Squared distance in cells to the part, `u16::MAX` beyond the widening.
    d2: Vec<u16>,
    reach: f64,
}

impl Flare {
    pub(super) fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub(super) fn new(parts: &[(Shape, f64)]) -> Flare {
        let widest = parts.iter().map(|p| p.1).fold(0.0, f64::max);
        let extent = parts
            .iter()
            .map(|(s, e)| {
                let b = bounds(s);
                (b[2] as f64 - b[0] as f64).max(b[3] as f64 - b[1] as f64) + 2.0 * e
            })
            .fold(0.0, f64::max);
        // fine enough to follow the width, coarse enough that the largest part stays a few million cells
        let cell = ((widest / 6.0).max(extent / 2000.0).max(0.05 * UNITS_PER_MM) / 100.0).ceil()
            as i64
            * 100;
        let fields = parts
            .iter()
            .map(|(shape, expansion)| FlareField::new(shape, *expansion, cell))
            .collect();
        Flare { cell, fields }
    }
}

impl FlareField {
    fn new(shape: &Shape, expansion: f64, cell: i64) -> FlareField {
        let c = cell as f64;
        let reach = expansion / c;
        let pad = reach.ceil() as i64 + 1;
        let b = bounds(shape);
        let i0 = (b[0] as i64).div_euclid(cell) - pad;
        let j0 = (b[1] as i64).div_euclid(cell) - pad;
        let nx = ((b[2] as i64).div_euclid(cell) + pad - i0 + 1) as usize;
        let ny = ((b[3] as i64).div_euclid(cell) + pad - j0 + 1) as usize;
        let mut rows: Vec<Vec<f64>> = vec![Vec::new(); ny];
        for contour in shape {
            let n = contour.len();
            for k in 0..n {
                let (p, q) = (contour[k], contour[(k + 1) % n]);
                if p.y == q.y {
                    continue;
                }
                let (lo, hi) = (p.y.min(q.y) as f64, p.y.max(q.y) as f64);
                let first = (lo / c - 0.5).ceil() as i64;
                let last = (hi / c - 0.5).ceil() as i64 - 1;
                for j in first.max(j0)..=last.min(j0 + ny as i64 - 1) {
                    let y = (j as f64 + 0.5) * c;
                    let t = (y - p.y as f64) / (q.y as f64 - p.y as f64);
                    rows[(j - j0) as usize].push(p.x as f64 + t * (q.x as f64 - p.x as f64));
                }
            }
        }
        let mut inside = vec![false; nx * ny];
        for (r, xs) in rows.iter_mut().enumerate() {
            xs.sort_by(f64::total_cmp);
            for pair in xs.chunks_exact(2) {
                let a = ((pair[0] / c - 0.5).ceil() as i64 - i0).clamp(0, nx as i64) as usize;
                let z = ((pair[1] / c - 0.5).ceil() as i64 - i0).clamp(0, nx as i64) as usize;
                for cell in &mut inside[r * nx + a..r * nx + z.max(a)] {
                    *cell = true;
                }
            }
        }
        let mut d2 = vec![u16::MAX; nx * ny];
        let radius = pad;
        let limit = (reach * reach).ceil().min(f64::from(u16::MAX - 1)) as i64;
        for y in 0..ny {
            for x in 0..nx {
                if !inside[y * nx + x] {
                    continue;
                }
                d2[y * nx + x] = 0;
                let edge = x == 0
                    || y == 0
                    || x + 1 == nx
                    || y + 1 == ny
                    || !inside[y * nx + x - 1]
                    || !inside[y * nx + x + 1]
                    || !inside[(y - 1) * nx + x]
                    || !inside[(y + 1) * nx + x];
                if !edge {
                    continue;
                }
                for dy in -radius..=radius {
                    let yy = y as i64 + dy;
                    if yy < 0 || yy >= ny as i64 {
                        continue;
                    }
                    for dx in -radius..=radius {
                        let xx = x as i64 + dx;
                        let dist = dx * dx + dy * dy;
                        if xx < 0 || xx >= nx as i64 || dist > limit {
                            continue;
                        }
                        let slot = &mut d2[yy as usize * nx + xx as usize];
                        *slot = (*slot).min(dist as u16);
                    }
                }
            }
        }
        FlareField {
            i0,
            j0,
            nx,
            ny,
            d2,
            reach,
        }
    }

    /// The cells within `fraction` of the widening as rectangles (equal runs of consecutive rows merged).
    fn rectangles(&self, fraction: f64, cell: i64, out: &mut Vec<Shape>) {
        let radius = self.reach * fraction;
        let limit = radius * radius;
        let mut open: FastMap<(usize, usize), usize> = FastMap::default();
        let emit = |a: usize, z: usize, from: usize, to: usize, out: &mut Vec<Shape>| {
            let x0 = ((self.i0 + a as i64) * cell) as i32;
            let x1 = ((self.i0 + z as i64) * cell) as i32;
            let y0 = ((self.j0 + from as i64) * cell) as i32;
            let y1 = ((self.j0 + to as i64) * cell) as i32;
            out.push(vec![vec![
                IntPoint::new(x0, y0),
                IntPoint::new(x1, y0),
                IntPoint::new(x1, y1),
                IntPoint::new(x0, y1),
            ]]);
        };
        for y in 0..=self.ny {
            let mut now: Vec<(usize, usize)> = Vec::new();
            if y < self.ny {
                let row = &self.d2[y * self.nx..(y + 1) * self.nx];
                let mut x = 0;
                while x < self.nx {
                    if f64::from(row[x]) <= limit {
                        let a = x;
                        while x < self.nx && f64::from(row[x]) <= limit {
                            x += 1;
                        }
                        now.push((a, x));
                    } else {
                        x += 1;
                    }
                }
            }
            let keep: FastSet<(usize, usize)> = now.iter().copied().collect();
            let closed: Vec<((usize, usize), usize)> = open
                .iter()
                .filter(|(k, _)| !keep.contains(k))
                .map(|(k, v)| (*k, *v))
                .collect();
            for (k, from) in closed {
                open.remove(&k);
                emit(k.0, k.1, from, y, out);
            }
            for k in now {
                open.entry(k).or_insert(y);
            }
        }
    }
}

pub(super) fn first_layer_flare_section(
    material: &[Shape],
    flare: &Flare,
    plate: &[IntPoint<i32>],
    fraction: f64,
) -> Vec<Shape> {
    if fraction <= 0.0 || flare.is_empty() {
        return material.to_vec();
    }
    let mut patches: Vec<Shape> = Vec::new();
    for field in &flare.fields {
        field.rectangles(fraction, flare.cell, &mut patches);
    }
    let mut expanded = material.overlay(patches.as_slice(), OverlayRule::Union, FillRule::NonZero);
    let plate = [vec![plate.to_vec()]];
    expanded =
        expanded
            .as_slice()
            .overlay(plate.as_slice(), OverlayRule::Intersect, FillRule::NonZero);
    repair_point_contacts(&expanded)
}
