use super::{carve, Grid};

/// Unit-capacity undirected max flow counts bridge-disjoint routes, not just local attachments.
/// The residual source side identifies a bottleneck when fewer than requested routes exist.
pub(crate) fn independent_routes(
    n: usize,
    links: &[[usize; 2]],
    frame: usize,
    source: usize,
    requested: usize,
) -> (usize, Vec<bool>) {
    let mut capacity = vec![vec![0usize; n]; n];
    for &[a, b] in links {
        capacity[a][b] += 1;
        capacity[b][a] += 1;
    }
    let mut flow = 0;
    loop {
        let mut parent = vec![usize::MAX; n];
        parent[source] = source;
        let mut queue = std::collections::VecDeque::from([source]);
        while let Some(a) = queue.pop_front() {
            for (b, from) in parent.iter_mut().enumerate() {
                if *from == usize::MAX && capacity[a][b] > 0 {
                    *from = a;
                    queue.push_back(b);
                }
            }
        }
        if parent[frame] == usize::MAX || flow >= requested {
            return (flow, parent.iter().map(|&p| p != usize::MAX).collect());
        }
        let mut b = frame;
        while b != source {
            let a = parent[b];
            capacity[a][b] -= 1;
            capacity[b][a] += 1;
            b = a;
        }
        flow += 1;
    }
}

#[derive(Default)]
pub(crate) struct BridgePlan {
    pub carved: usize,
    pub frame: u32,
    pub directions: Vec<Vec<[f64; 2]>>,
    pub routes: Vec<usize>,
    pub paths: Vec<BridgePath>,
}

pub(crate) struct BridgePath {
    pub cells: Vec<usize>,
    pub straight: bool,
}

struct Link {
    path: Vec<usize>,
    ends: [u32; 2],
    straight: bool,
}

fn attachment_direction(at: usize, centre: [f64; 2], w: usize) -> [f64; 2] {
    let x = (at % w) as f64 - centre[0];
    let y = (at / w) as f64 - centre[1];
    let length = x.hypot(y).max(1e-9);
    [x / length, y / length]
}

impl Link {
    fn new(path: Vec<usize>, label: &[u32], straight: bool) -> Self {
        let last = path.len() - 1;
        Self {
            ends: [label[path[0]], label[path[last]]],
            path,
            straight,
        }
    }

    fn attachment(&self, end: usize) -> usize {
        if end == 0 {
            self.path[0]
        } else {
            *self.path.last().unwrap()
        }
    }

    fn holes(&self) -> &[usize] {
        &self.path[1..self.path.len() - 1]
    }
}

struct Planner<'a> {
    w: usize,
    h: usize,
    label: &'a [u32],
    radius: i64,
    contact: i64,
    avoid: Option<&'a [bool]>,
    blocked: Vec<bool>,
    attachments: Vec<Vec<usize>>,
    groups: Vec<usize>,
    centres: Vec<[f64; 2]>,
    links: Vec<[usize; 2]>,
    plan: BridgePlan,
    min_angle: f64,
}

impl Planner<'_> {
    fn group(&self, mut i: usize) -> usize {
        while self.groups[i] != i {
            i = self.groups[i];
        }
        i
    }

    fn supported(&self, i: usize) -> bool {
        self.group(i) == self.group(self.plan.frame as usize)
    }

    fn direction(&self, link: &Link, end: usize) -> [f64; 2] {
        attachment_direction(
            link.attachment(end),
            self.centres[link.ends[end] as usize],
            self.w,
        )
    }

    fn parallel(&self, link: &Link) -> f64 {
        let mut parallel: f64 = -1.0;
        for end in 0..2 {
            let l = link.ends[end] as usize;
            if l == self.plan.frame as usize {
                continue;
            }
            let direction = self.direction(link, end);
            for d in &self.plan.directions[l] {
                parallel = parallel.max(d[0] * direction[0] + d[1] * direction[1]);
            }
        }
        parallel
    }

    fn valid(&self, link: &Link) -> bool {
        if link.holes().is_empty() || link.ends[0] == link.ends[1] {
            return false;
        }
        if self.min_angle > 0.0 && self.parallel(link) > self.min_angle.to_radians().cos() + 1e-9 {
            return false;
        }
        let spacing = (self.radius + 1) as f64;
        for end in 0..2 {
            let l = link.ends[end] as usize;
            let at = link.attachment(end);
            if self.attachments[l].iter().any(|&p| {
                let dx = (at % self.w) as f64 - (p % self.w) as f64;
                let dy = (at / self.w) as f64 - (p / self.w) as f64;
                dx * dx + dy * dy < spacing * spacing
            }) {
                return false;
            }
        }
        // Test the entire flared footprint: carving must not truncate an end at a pocket or merge bridges.
        let holes = link.holes();
        for (j, &q) in holes.iter().enumerate() {
            let width = self
                .radius
                .max(self.contact - j.min(holes.len() - 1 - j) as i64);
            let (x, y) = ((q % self.w) as i64, (q / self.w) as i64);
            for dy in 0..width {
                for dx in 0..width {
                    let (cx, cy) = (x + dx - width / 2, y + dy - width / 2);
                    if cx < 0 || cy < 0 || cx >= self.w as i64 || cy >= self.h as i64 {
                        continue;
                    }
                    let i = cy as usize * self.w + cx as usize;
                    if self.label[i] == 0 && (self.blocked[i] || self.avoid.is_some_and(|a| a[i])) {
                        return false;
                    }
                    if self.label[i] != 0 && !link.ends.contains(&self.label[i]) {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn add(&mut self, g: &mut Grid, original: &[bool], rise: &mut [f32], link: Link) {
        let added = carve(
            g,
            original,
            rise,
            link.holes(),
            self.radius,
            self.contact,
            self.avoid,
        );
        if added.is_empty() {
            return;
        }
        // One empty cell between footprints keeps the attachments physically distinct.
        for &i in &added {
            let (x, y) = (i % self.w, i / self.w);
            for cy in y.saturating_sub(1)..=(y + 1).min(self.h - 1) {
                for cx in x.saturating_sub(1)..=(x + 1).min(self.w - 1) {
                    self.blocked[cy * self.w + cx] = true;
                }
            }
        }
        for end in 0..2 {
            let l = link.ends[end] as usize;
            let direction = self.direction(&link, end);
            self.plan.directions[l].push(direction);
            self.attachments[l].push(link.attachment(end));
        }
        let a = self.group(link.ends[0] as usize);
        let b = self.group(link.ends[1] as usize);
        self.groups[b] = a;
        self.links.push(link.ends.map(|l| l as usize));
        self.plan.paths.push(BridgePath {
            cells: link.path,
            straight: link.straight,
        });
        self.plan.carved += added.len();
    }

    fn ray(&self, start: usize, d: [f64; 2], reach: f64) -> Option<Link> {
        let source = self.label[start];
        let (mut x, mut y) = ((start % self.w) as i64, (start / self.w) as i64);
        let (sx, sy) = (
            if d[0] >= 0.0 { 1 } else { -1 },
            if d[1] >= 0.0 { 1 } else { -1 },
        );
        let (dx, dy) = (1.0 / d[0].abs(), 1.0 / d[1].abs());
        let (mut tx, mut ty) = (0.5 * dx, 0.5 * dy);
        let mut path = vec![start];
        loop {
            let t = tx.min(ty);
            if t > reach {
                return None;
            }
            if tx <= ty {
                x += sx;
                tx += dx;
            } else {
                y += sy;
                ty += dy;
            }
            if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
                return None;
            }
            let i = y as usize * self.w + x as usize;
            if self.label[i] != 0 {
                if self.label[i] == source {
                    return None;
                }
                path.push(i);
                return Some(Link::new(path, self.label, true));
            }
            if self.blocked[i] || self.avoid.is_some_and(|a| a[i]) {
                return None;
            }
            path.push(i);
        }
    }

    fn score(&self, link: &Link, spread: bool, landing: &[u32]) -> (f64, f64) {
        let cost = link.holes().len() as f64
            + landing.get(link.attachment(0)).copied().unwrap_or(0) as f64
            + landing.get(link.attachment(1)).copied().unwrap_or(0) as f64;
        (if spread { self.parallel(link) } else { 0.0 }, cost)
    }

    fn best(
        &self,
        source: usize,
        boundary: &[usize],
        reach: f64,
        spread: bool,
        connecting: bool,
        landing: &[u32],
        cut: Option<&[bool]>,
    ) -> Option<Link> {
        let mut best: Option<(f64, f64, Link)> = None;
        let samples = boundary.len().min(256);
        for k in 0..samples {
            let start = boundary[k * boundary.len() / samples];
            for n in 0..72 {
                let theta = n as f64 * std::f64::consts::TAU / 72.0;
                let Some(link) = self.ray(start, [theta.cos(), theta.sin()], reach) else {
                    continue;
                };
                let target = link.ends[1] as usize;
                if !self.supported(target)
                    || (connecting && self.group(source) == self.group(target))
                    || cut.is_some_and(|side| side[target])
                {
                    continue;
                }
                let (angle, cost) = self.score(&link, spread && !connecting, landing);
                if best
                    .as_ref()
                    .is_none_or(|b| angle.total_cmp(&b.0).then(cost.total_cmp(&b.1)).is_lt())
                    && self.valid(&link)
                {
                    best = Some((angle, cost, link));
                }
            }
        }
        best.map(|b| b.2)
    }

    fn routes(&self, source: usize, requested: usize) -> (usize, Vec<bool>) {
        independent_routes(
            self.groups.len(),
            &self.links,
            self.plan.frame as usize,
            source,
            requested,
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn plan(
    g: &mut Grid,
    original: &[bool],
    label: &[u32],
    frame: u32,
    radius: i64,
    rise: &mut [f32],
    primary: &[Vec<usize>],
    per_island: usize,
    min_angle: f64,
    max_length: f64,
    avoid: Option<&[bool]>,
    contact: i64,
    landing: &[u32],
    spread: bool,
) -> BridgePlan {
    let n = label.iter().copied().max().unwrap_or(frame).max(frame) as usize + 1;
    let mut planner = Planner {
        w: g.w,
        h: g.h,
        label,
        radius,
        contact,
        avoid,
        blocked: vec![false; label.len()],
        attachments: vec![vec![]; n],
        groups: (0..n).collect(),
        centres: vec![[0.0; 2]; n],
        links: Vec::new(),
        plan: BridgePlan {
            frame,
            directions: vec![vec![]; n],
            ..Default::default()
        },
        min_angle,
    };
    let mut boundaries = vec![Vec::new(); n];
    let mut areas = vec![0usize; n];
    for (i, &l) in label.iter().enumerate() {
        if l == 0 {
            continue;
        }
        let (x, y) = (i % g.w, i / g.w);
        areas[l as usize] += 1;
        planner.centres[l as usize][0] += x as f64;
        planner.centres[l as usize][1] += y as f64;
        if (x > 0 && label[i - 1] == 0)
            || (x + 1 < g.w && label[i + 1] == 0)
            || (y > 0 && label[i - g.w] == 0)
            || (y + 1 < g.h && label[i + g.w] == 0)
        {
            boundaries[l as usize].push(i);
        }
    }
    for (centre, area) in planner.centres.iter_mut().zip(areas) {
        if area > 0 {
            centre[0] /= area as f64;
            centre[1] /= area as f64;
        }
    }
    let mut seeds = Vec::new();
    let mut shortest = vec![f64::INFINITY; n];
    for path in primary {
        let mut start = 0;
        for j in 1..path.len() {
            if label[path[j]] != 0 {
                if j > start + 1 {
                    let link = Link::new(path[start..=j].to_vec(), label, false);
                    for &l in &link.ends {
                        shortest[l as usize] = shortest[l as usize].min(link.holes().len() as f64);
                    }
                    seeds.push(link);
                }
                start = j;
            }
        }
    }
    seeds.sort_by_key(|l| l.holes().len());
    for link in seeds {
        if planner.group(link.ends[0] as usize) != planner.group(link.ends[1] as usize)
            && planner.valid(&link)
        {
            planner.add(g, original, rise, link);
        }
    }
    let reach = |i: usize| {
        let shortest = if shortest[i].is_finite() {
            shortest[i]
        } else {
            radius as f64
        };
        let factor = if spread { max_length } else { 4.0 };
        factor * shortest.max(radius as f64) + 4.0 * radius as f64
    };
    loop {
        let mut progress = false;
        let mut order: Vec<_> = (1..n).collect();
        order.sort_by_key(|&i| boundaries[i].len());
        for i in order {
            let boundary = &boundaries[i];
            if boundary.is_empty() || planner.supported(i) {
                continue;
            }
            if let Some(link) = planner.best(i, boundary, reach(i), false, true, landing, None) {
                planner.add(g, original, rise, link);
                progress = true;
            }
        }
        if !progress {
            break;
        }
    }
    let mut order: Vec<_> = (1..n).collect();
    order.sort_by_key(|&i| boundaries[i].len());
    for i in order {
        if i == frame as usize || !planner.supported(i) {
            continue;
        }
        loop {
            let (routes, cut) = planner.routes(i, per_island);
            if routes >= per_island {
                break;
            }
            let mut best: Option<(f64, f64, Link)> = None;
            for (source, boundary) in boundaries.iter().enumerate().skip(1) {
                if !cut[source] {
                    continue;
                }
                let Some(link) = planner.best(
                    source,
                    boundary,
                    reach(source),
                    spread,
                    false,
                    landing,
                    Some(&cut),
                ) else {
                    continue;
                };
                let (angle, cost) = planner.score(&link, spread, landing);
                if best
                    .as_ref()
                    .is_none_or(|b| angle.total_cmp(&b.0).then(cost.total_cmp(&b.1)).is_lt())
                {
                    best = Some((angle, cost, link));
                }
            }
            let Some((_, _, link)) = best else { break };
            planner.add(g, original, rise, link);
        }
    }
    planner.plan.routes = (0..n)
        .map(|i| {
            if i == frame as usize || boundaries[i].is_empty() {
                0
            } else {
                planner.routes(i, per_island).0
            }
        })
        .collect();
    planner.plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_strips_at_different_heights_have_distinct_attachment_angles() {
        let w = 100;
        let centre = [9.5, 79.5];
        let a = attachment_direction(30 * w + 19, centre, w);
        let b = attachment_direction(130 * w + 19, centre, w);
        assert!(a[0] * b[0] + a[1] * b[1] < 45f64.to_radians().cos());
        let nearby = attachment_direction(31 * w + 19, centre, w);
        assert!(a[0] * nearby[0] + a[1] * nearby[1] > 45f64.to_radians().cos());
    }

    #[test]
    fn local_attachments_do_not_hide_a_shared_bottleneck() {
        let links = [[1, 2], [1, 2], [1, 2], [2, 0]];
        let (routes, cut) = independent_routes(3, &links, 0, 1, 4);
        assert_eq!(routes, 1);
        assert_eq!(cut, vec![false, true, true]);
        let reinforced = [
            [1, 2],
            [1, 2],
            [1, 2],
            [1, 2],
            [2, 0],
            [2, 0],
            [2, 0],
            [2, 0],
        ];
        assert_eq!(independent_routes(3, &reinforced, 0, 1, 4).0, 4);
    }

    #[test]
    fn tall_parallel_parts_get_four_separated_bridges_across_every_gap() {
        for spread in [false, true] {
            for angle in [30.0, 45.0] {
                let (w, h) = (100, 160);
                let label: Vec<u32> = (0..w * h)
                    .map(|i| match i % w {
                        0..=19 => 1,
                        30..=49 => 2,
                        60..=99 => 3,
                        _ => 0,
                    })
                    .collect();
                let mut grid = Grid {
                    w,
                    h,
                    m: label.iter().map(|&l| l > 0).collect(),
                };
                let mut rise = vec![f32::NAN; w * h];
                let plan = super::super::bridge_labelled(
                    &mut grid, &label, 3, 2, &mut rise, 4, angle, 4.0, None, 0, spread,
                );
                let minimum = if angle == 30.0 { 4 } else { 2 };
                assert!(
                    plan.routes[1] >= minimum,
                    "spread={spread}, angle={angle}: {:?}",
                    plan.directions
                );
                assert!(
                    plan.routes[2] >= minimum,
                    "spread={spread}, angle={angle}: {:?}",
                    plan.directions
                );
                assert!(
                    plan.directions[3].len() >= minimum,
                    "the only connection to the main part must not remain a single bridge"
                );
                for directions in &plan.directions[1..3] {
                    for (i, a) in directions.iter().enumerate() {
                        for b in &directions[i + 1..] {
                            assert!(a[0] * b[0] + a[1] * b[1] <= angle.to_radians().cos() + 1e-9);
                        }
                    }
                }
                assert_eq!(super::super::label_islands(&grid).0, 0);
            }
        }
    }

    fn nested(spread: bool, angle: f64, requested: usize) -> (Grid, Vec<u32>, BridgePlan) {
        let w = 81;
        let mut label = vec![0; w * w];
        for y in 0..w {
            for x in 0..w {
                label[y * w + x] = if !(6..75).contains(&x) || !(6..75).contains(&y) {
                    1
                } else if (20..61).contains(&x)
                    && (20..61).contains(&y)
                    && (!(26..55).contains(&x) || !(26..55).contains(&y))
                {
                    2
                } else if (35..46).contains(&x) && (35..46).contains(&y) {
                    3
                } else {
                    0
                };
            }
        }
        let mut grid = Grid {
            w,
            h: w,
            m: label.iter().map(|&l| l > 0).collect(),
        };
        let mut rise = vec![f32::NAN; w * w];
        let plan = super::super::bridge_labelled(
            &mut grid, &label, 1, 2, &mut rise, requested, angle, 4.0, None, 0, spread,
        );
        (grid, label, plan)
    }

    #[test]
    fn nested_islands_get_four_bridges_in_both_modes() {
        for spread in [false, true] {
            let (grid, _, plan) = nested(spread, 30.0, 4);
            assert_eq!(super::super::label_islands(&grid).0, 0);
            for island in [2, 3] {
                assert!(
                    plan.directions[island].len() >= 4,
                    "spread={spread}, island={island}: {:?}",
                    plan.directions[island]
                );
                for (i, a) in plan.directions[island].iter().enumerate() {
                    for b in &plan.directions[island][i + 1..] {
                        assert!(a[0] * b[0] + a[1] * b[1] <= 30f64.to_radians().cos() + 1e-9);
                    }
                }
            }
        }
    }

    #[test]
    fn opposite_bridges_are_allowed_and_impossible_angles_are_not_relaxed() {
        for spread in [false, true] {
            let (_, _, plan) = nested(spread, 90.0, 5);
            for island in [2, 3] {
                assert!(plan.directions[island].len() >= 2);
                assert!(plan.directions[island].len() <= 4);
                for (i, a) in plan.directions[island].iter().enumerate() {
                    for b in &plan.directions[island][i + 1..] {
                        assert!(a[0] * b[0] + a[1] * b[1] <= 1e-9);
                    }
                }
            }
        }
    }
}
