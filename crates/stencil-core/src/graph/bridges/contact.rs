use crate::{mesh::Mesh, tile_bridge::components};
use std::collections::{BTreeMap, HashMap};

type Point = [f64; 2];
type Segment = [Point; 2];
const EPS: f64 = 1e-5;

#[derive(Default)]
pub(super) struct Contacts {
    pub widths: Vec<f64>,
}

impl Contacts {
    pub fn weak(&self, target: f64) -> usize {
        self.widths.iter().filter(|&&w| w + EPS < target).count()
    }

    pub fn minimum(&self) -> Option<f64> {
        self.widths.iter().copied().reduce(f64::min)
    }
}

fn cross(a: Point, b: Point) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}

fn length(a: Point) -> f64 {
    a[0].hypot(a[1])
}

fn slice(m: &Mesh, labels: &[usize], z: f64) -> BTreeMap<usize, Vec<Segment>> {
    let mut out: BTreeMap<usize, Vec<Segment>> = BTreeMap::new();
    for (t, &label) in m.t.iter().zip(labels) {
        let p = t.map(|i| m.v[i]);
        let mut hits = Vec::new();
        for i in 0..3 {
            let (a, b) = (p[i], p[(i + 1) % 3]);
            if (a[2] > z) != (b[2] > z) {
                let f = (z - a[2]) / (b[2] - a[2]);
                hits.push([a[0] + f * (b[0] - a[0]), a[1] + f * (b[1] - a[1])]);
            }
        }
        if hits.len() == 2 && length(sub(hits[0], hits[1])) > EPS {
            out.entry(label).or_default().push([hits[0], hits[1]]);
        }
    }
    out
}

fn inside(p: Point, edges: &[Segment]) -> bool {
    let mut odd = false;
    for &[a, b] in edges {
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
        {
            odd = !odd;
        }
    }
    odd
}

fn covered([a, b]: Segment, edges: &[Segment]) -> Vec<Segment> {
    let d = sub(b, a);
    let mut cuts = vec![0.0, 1.0];
    for &[c, e] in edges {
        let v = sub(e, c);
        let den = cross(d, v);
        if den.abs() < 1e-12 {
            continue;
        }
        let t = cross(sub(c, a), v) / den;
        let u = cross(sub(c, a), d) / den;
        if (0.0..=1.0).contains(&t) && (-EPS..=1.0 + EPS).contains(&u) {
            cuts.push(t);
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
    let at = |t: f64| [a[0] + t * d[0], a[1] + t * d[1]];
    cuts.windows(2)
        .filter(|t| inside(at((t[0] + t[1]) * 0.5), edges))
        .map(|t| [at(t[0]), at(t[1])])
        .filter(|s| length(sub(s[0], s[1])) > EPS)
        .collect()
}

fn narrowest_patch(segments: Vec<Segment>) -> Option<f64> {
    let key = |p: Point| ((p[0] / EPS).round() as i64, (p[1] / EPS).round() as i64);
    let mut ends: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, segment) in segments.iter().enumerate() {
        for &p in segment {
            ends.entry(key(p)).or_default().push(i);
        }
    }
    let mut seen = vec![false; segments.len()];
    let mut minimum: Option<f64> = None;
    for start in 0..segments.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![start];
        let mut points = Vec::new();
        while let Some(i) = stack.pop() {
            for &p in &segments[i] {
                points.push(p);
                let (x, y) = key(p);
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for &j in ends.get(&(x + dx, y + dy)).into_iter().flatten() {
                            if !seen[j] && segments[j].iter().any(|&q| length(sub(p, q)) < EPS) {
                                seen[j] = true;
                                stack.push(j);
                            }
                        }
                    }
                }
            }
        }
        // Projected span avoids counting a jagged staircase as a wide attachment.
        points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
        points.dedup();
        let mut hull: Vec<Point> = Vec::new();
        for &p in &points {
            while hull.len() >= 2
                && cross(
                    sub(hull[hull.len() - 1], hull[hull.len() - 2]),
                    sub(p, hull[hull.len() - 1]),
                ) <= 0.0
            {
                hull.pop();
            }
            hull.push(p);
        }
        let lower = hull.len();
        for &p in points.iter().rev().skip(1) {
            while hull.len() > lower
                && cross(
                    sub(hull[hull.len() - 1], hull[hull.len() - 2]),
                    sub(p, hull[hull.len() - 1]),
                ) <= 0.0
            {
                hull.pop();
            }
            hull.push(p);
        }
        let mut span: f64 = 0.0;
        for &a in &hull {
            for &b in &hull {
                span = span.max(length(sub(a, b)));
            }
        }
        minimum = Some(minimum.map_or(span, |v| v.min(span)));
    }
    minimum
}

/// Measure contiguous original-solid/bridge contacts after clipping. Sample both ends and the middle
/// of every mesh-height interval and every printed layer; do not infer strength from vertex connectivity.
pub(super) fn measure(original: &Mesh, bridge: &Mesh, layer: f64) -> Contacts {
    if bridge.t.is_empty() {
        return Contacts::default();
    }
    let (_, original_labels) = components(original);
    let (_, bridge_labels) = components(bridge);
    let (lo, hi) = bridge.bounds();
    let mut heights: Vec<f64> = original
        .v
        .iter()
        .chain(&bridge.v)
        .map(|v| v[2])
        .filter(|&z| z >= lo[2] && z <= hi[2])
        .collect();
    for k in (lo[2] / layer).floor() as i64..=(hi[2] / layer).ceil() as i64 {
        let z = k as f64 * layer;
        if z >= lo[2] && z <= hi[2] {
            heights.push(z);
        }
    }
    heights.sort_by(f64::total_cmp);
    heights.dedup_by(|a, b| (*a - *b).abs() < EPS);
    let mut widths: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    for band in heights.windows(2).filter(|b| b[1] - b[0] > EPS) {
        for fraction in [0.001, 0.5, 0.999] {
            let z = band[0] + fraction * (band[1] - band[0]);
            let solids = slice(original, &original_labels, z);
            let bridges = slice(bridge, &bridge_labels, z);
            for (&b, edges) in &bridges {
                let mut low = [f64::MAX; 2];
                let mut high = [f64::MIN; 2];
                for p in edges.iter().flatten() {
                    for axis in 0..2 {
                        low[axis] = low[axis].min(p[axis]);
                        high[axis] = high[axis].max(p[axis]);
                    }
                }
                for (&s, boundary) in &solids {
                    let contacts = boundary
                        .iter()
                        .filter(|e| {
                            (0..2).all(|axis| {
                                e[0][axis].max(e[1][axis]) >= low[axis]
                                    && e[0][axis].min(e[1][axis]) <= high[axis]
                            })
                        })
                        .flat_map(|&e| covered(e, edges))
                        .collect();
                    if let Some(width) = narrowest_patch(contacts) {
                        widths
                            .entry((b, s))
                            .and_modify(|v| *v = v.min(width))
                            .or_insert(width);
                    }
                }
            }
        }
    }
    Contacts {
        widths: widths.into_values().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_the_small_end_not_the_nominal_bridge_width() {
        let mut solid = Mesh::cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 8.0]);
        solid.append(&Mesh::cuboid([20.0, 9.2, 0.0], [30.0, 20.0, 8.0]));
        let bridge = Mesh::cuboid([9.0, 8.0, 4.0], [21.0, 10.0, 8.0]);
        let contacts = measure(&solid, &bridge, 0.2);
        assert_eq!(contacts.widths.len(), 2);
        assert!(
            (contacts.minimum().unwrap() - 1.0f64.hypot(0.8)).abs() < EPS,
            "{:?}",
            contacts.widths
        );
        assert_eq!(contacts.weak(1.5), 1);
    }

    #[test]
    fn disconnected_contacts_do_not_add_up_to_a_strong_contact() {
        let patches = vec![[[0.0, 0.0], [0.5, 0.0]], [[0.0, 2.0], [0.5, 2.0]]];
        assert_eq!(narrowest_patch(patches), Some(0.5));
    }

    #[test]
    fn measures_contact_through_the_height_not_only_at_the_top() {
        let mut solid = Mesh::cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 8.0]);
        solid.append(&Mesh::cuboid([20.0, 0.0, 0.0], [30.0, 10.0, 8.0]));
        let bridge = Mesh::loft_quads(
            [[9.0, 9.5], [21.0, 9.5], [21.0, 10.0], [9.0, 10.0]],
            4.0,
            [[9.0, 8.0], [21.0, 8.0], [21.0, 10.0], [9.0, 10.0]],
            8.0,
        );
        let contacts = measure(&solid, &bridge, 0.2);
        assert_eq!(contacts.widths.len(), 2);
        assert!(
            (0.49..0.51).contains(&contacts.minimum().unwrap()),
            "{:?}",
            contacts.widths
        );
        assert_eq!(contacts.weak(1.0), 2);
    }
}
