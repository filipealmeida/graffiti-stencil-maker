use super::{Grid, Link, Planner};
use std::{
    cmp::Ordering,
    collections::{BTreeSet, BinaryHeap},
};

struct Gap {
    ends: [usize; 2],
    axis: [[f64; 2]; 2],
    cost: f64,
}

impl Gap {
    fn link(&self, planner: &Planner<'_>) -> Link {
        let [a, b] = self.axis;
        let mut path = Vec::new();
        walk(a, b, planner.w, planner.h, |i, _| {
            if let Some(&previous) = path.last() {
                if previous % planner.w != i % planner.w && previous / planner.w != i / planner.w {
                    path.push(previous / planner.w * planner.w + i % planner.w);
                }
            }
            path.push(i);
        });
        let mut link = Link::new(path, planner.label, true);
        link.axis = Some(self.axis);
        link
    }
}

struct Alignment {
    gaps: Vec<Gap>,
}

// Visit grid cells along the exact axis, retaining a point inside each cell for meshing.
fn walk(a: [f64; 2], b: [f64; 2], w: usize, h: usize, mut visit: impl FnMut(usize, [f64; 2])) {
    let d = [b[0] - a[0], b[1] - a[1]];
    let mut at = [(a[0] + 0.5).floor() as i64, (a[1] + 0.5).floor() as i64];
    let end = [(b[0] + 0.5).floor() as i64, (b[1] + 0.5).floor() as i64];
    let step = d.map(|v| if v >= 0.0 { 1 } else { -1 });
    let delta = d.map(|v| 1.0 / v.abs());
    let mut next = [0.0; 2];
    for k in 0..2 {
        next[k] = if d[k].abs() < 1e-12 {
            f64::INFINITY
        } else {
            ((at[k] as f64 + step[k] as f64 * 0.5) - a[k]) / d[k]
        };
    }
    let mut t = 0.0;
    loop {
        if at[0] < 0 || at[1] < 0 || at[0] >= w as i64 || at[1] >= h as i64 {
            break;
        }
        let exit = next[0].min(next[1]).min(1.0);
        let middle = (t + exit) * 0.5;
        visit(
            at[1] as usize * w + at[0] as usize,
            [a[0] + d[0] * middle, a[1] + d[1] * middle],
        );
        if at == end || exit >= 1.0 {
            break;
        }
        // At a corner, step both axes: zero-length corner contacts cannot carry a bridge.
        for k in 0..2 {
            if next[k] <= exit + 1e-12 {
                at[k] += step[k];
                next[k] += delta[k];
            }
        }
        t = exit;
    }
}

fn clipped_axis(w: usize, h: usize, d: [f64; 2], offset: f64) -> Option<[[f64; 2]; 2]> {
    let anchor = [-d[1] * offset, d[0] * offset];
    let mut low = f64::NEG_INFINITY;
    let mut high = f64::INFINITY;
    for k in 0..2 {
        let max = [w, h][k] as f64 - 0.5 - 1e-7;
        let min = -0.5 + 1e-7;
        if d[k].abs() < 1e-12 {
            if anchor[k] < min || anchor[k] > max {
                return None;
            }
        } else {
            let a = (min - anchor[k]) / d[k];
            let b = (max - anchor[k]) / d[k];
            low = low.max(a.min(b));
            high = high.min(a.max(b));
        }
    }
    (low < high).then(|| [low, high].map(|t| [anchor[0] + t * d[0], anchor[1] + t * d[1]]))
}

fn candidates(
    planner: &Planner<'_>,
    shortest: &[f64],
    max_length: f64,
    landing: &[u32],
) -> Vec<Alignment> {
    let reach = |l: usize| {
        if l == planner.plan.frame as usize {
            f64::INFINITY
        } else {
            let shortest = if shortest[l].is_finite() {
                shortest[l]
            } else {
                planner.radius as f64
            };
            max_length * shortest.max(planner.radius as f64) + 4.0 * planner.radius as f64
        }
    };
    let mut alignments = Vec::new();
    for n in 0..36 {
        let theta = n as f64 * std::f64::consts::PI / 36.0;
        let d = [theta.cos(), theta.sin()];
        let project = |p: [f64; 2]| -d[1] * p[0] + d[0] * p[1];
        let corners = [
            [0.0, 0.0],
            [planner.w as f64 - 1.0, 0.0],
            [0.0, planner.h as f64 - 1.0],
            [planner.w as f64 - 1.0, planner.h as f64 - 1.0],
        ]
        .map(project);
        let low = corners.into_iter().fold(f64::INFINITY, f64::min);
        let high = corners.into_iter().fold(f64::NEG_INFINITY, f64::max);
        let step = planner.radius.max(1);
        let mut offsets: BTreeSet<i64> = ((low.floor() as i64)..=(high.ceil() as i64))
            .step_by(step as usize)
            .map(|v| v * 2)
            .collect();
        for &centre in planner.centres.iter().skip(1) {
            offsets.insert((project(centre) * 2.0).round() as i64);
        }
        for offset in offsets {
            let Some([a, b]) = clipped_axis(planner.w, planner.h, d, offset as f64 * 0.5) else {
                continue;
            };
            let mut gaps = Vec::new();
            let mut previous: Option<(usize, [f64; 2])> = None;
            let mut empty = false;
            walk(a, b, planner.w, planner.h, |i, point| {
                let l = planner.label[i] as usize;
                if l == 0 {
                    empty = true;
                    return;
                }
                if let Some((from, start)) = previous {
                    let source = planner.label[from] as usize;
                    let length = (point[0] - start[0]).hypot(point[1] - start[1]);
                    if empty && source != l && length <= reach(source).min(reach(l)) {
                        gaps.push(Gap {
                            ends: [source, l],
                            axis: [start, point],
                            cost: length
                                + landing.get(from).copied().unwrap_or(0) as f64
                                + landing.get(i).copied().unwrap_or(0) as f64,
                        });
                    }
                }
                previous = Some((i, point));
                empty = false;
            });
            if !gaps.is_empty() {
                alignments.push(Alignment { gaps });
            }
        }
    }
    alignments
}

fn root(groups: &[usize], mut i: usize) -> usize {
    while groups[i] != i {
        i = groups[i];
    }
    i
}

struct Score {
    connected: usize,
    reinforced: usize,
    cost: f64,
    gaps: Vec<usize>,
}

struct Ranked {
    index: usize,
    score: Score,
}

impl Ord for Ranked {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .connected
            .cmp(&other.score.connected)
            .then(self.score.reinforced.cmp(&other.score.reinforced))
            .then_with(|| other.score.cost.total_cmp(&self.score.cost))
            .then_with(|| other.index.cmp(&self.index))
    }
}

impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Ranked {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for Ranked {}

fn score(planner: &Planner<'_>, alignment: &Alignment, cuts: &[(usize, Vec<bool>)]) -> Score {
    let mut groups: Vec<_> = (0..planner.groups.len())
        .map(|i| planner.group(i))
        .collect();
    for gap in &alignment.gaps {
        let a = root(&groups, gap.ends[0]);
        let b = root(&groups, gap.ends[1]);
        groups[b] = a;
    }
    let frame = root(&groups, planner.plan.frame as usize);
    let gaps: Vec<_> = alignment
        .gaps
        .iter()
        .enumerate()
        .filter(|(_, gap)| root(&groups, gap.ends[0]) == frame)
        .filter(|(_, gap)| {
            !planner.supported(gap.ends[0])
                || !planner.supported(gap.ends[1])
                || cuts
                    .iter()
                    .any(|(_, cut)| cut[gap.ends[0]] != cut[gap.ends[1]])
        })
        .map(|(i, _)| i)
        .collect();
    let connected = (1..groups.len())
        .filter(|&i| !planner.supported(i) && root(&groups, i) == frame)
        .count();
    let reinforced = cuts
        .iter()
        .map(|(deficit, cut)| {
            gaps.iter()
                .filter(|&&i| {
                    let ends = alignment.gaps[i].ends;
                    cut[ends[0]] != cut[ends[1]]
                })
                .count()
                .min(*deficit)
        })
        .sum();
    Score {
        connected,
        reinforced,
        cost: gaps.iter().map(|&i| alignment.gaps[i].cost).sum(),
        gaps,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn plan(
    planner: &mut Planner<'_>,
    g: &mut Grid,
    original: &[bool],
    rise: &mut [f32],
    shortest: &[f64],
    max_length: f64,
    landing: &[u32],
    requested: usize,
) {
    let mut alignments = candidates(planner, shortest, max_length, landing);
    loop {
        let cuts: Vec<_> = (1..planner.groups.len())
            .filter(|&i| i != planner.plan.frame as usize && shortest[i].is_finite())
            .filter_map(|i| {
                let (routes, cut) = planner.routes(i, requested);
                (routes < requested).then_some((requested - routes, cut))
            })
            .collect();
        if cuts.is_empty() {
            break;
        }
        let mut ranked: BinaryHeap<_> = alignments
            .iter()
            .enumerate()
            .map(|(index, alignment)| Ranked {
                index,
                score: score(planner, alignment, &cuts),
            })
            .filter(|r| {
                !r.score.gaps.is_empty() && (r.score.connected > 0 || r.score.reinforced > 0)
            })
            .collect();
        let mut selected = None;
        while let Some(Ranked {
            index,
            score: current,
        }) = ranked.pop()
        {
            let alignment = &mut alignments[index];
            // Invalidity is monotonic: later bridges only add direction and footprint constraints.
            let mut invalid = Vec::new();
            let mut links = Vec::new();
            for &i in &current.gaps {
                let link = alignment.gaps[i].link(planner);
                if planner.valid(&link) {
                    links.push(link);
                } else {
                    invalid.push(i);
                }
            }
            if invalid.is_empty() {
                selected = Some((index, links));
                break;
            }
            for i in invalid.into_iter().rev() {
                alignment.gaps.remove(i);
            }
            let updated = score(planner, alignment, &cuts);
            if !updated.gaps.is_empty() && (updated.connected > 0 || updated.reinforced > 0) {
                ranked.push(Ranked {
                    index,
                    score: updated,
                });
            }
        }
        let Some((index, links)) = selected else {
            break;
        };
        let before: Vec<_> = planner.plan.directions.iter().map(Vec::len).collect();
        for link in links {
            planner.add(g, original, rise, link);
        }
        // Two sides of the same selected line share one direction, not two parallel alignments.
        for (directions, old) in planner.plan.directions.iter_mut().zip(before) {
            directions.truncate((old + 1).min(directions.len()));
        }
        alignments.remove(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        islands::{bridge_labelled, label_islands, BridgePlan},
        tile_bridge::BridgeSpacing,
    };

    fn run(
        label: &[u32],
        w: usize,
        requested: usize,
        angle: f64,
        avoid: Option<&[bool]>,
    ) -> (Grid, BridgePlan) {
        let mut grid = Grid {
            w,
            h: label.len() / w,
            m: label.iter().map(|&l| l > 0).collect(),
        };
        let mut rise = vec![f32::NAN; label.len()];
        let plan = bridge_labelled(
            &mut grid,
            label,
            1,
            2,
            &mut rise,
            requested,
            angle,
            4.0,
            avoid,
            0,
            BridgeSpacing::Aligned,
        );
        (grid, plan)
    }

    fn row() -> Vec<u32> {
        (0..100 * 60)
            .map(|i| {
                let (x, y) = (i % 100, i / 100);
                if x < 8 {
                    1
                } else if (24..36).contains(&y) {
                    match x {
                        20..=29 => 2,
                        45..=54 => 3,
                        70..=79 => 4,
                        _ => 0,
                    }
                } else {
                    0
                }
            })
            .collect()
    }

    #[test]
    fn one_alignment_supports_three_islands_and_only_fills_gaps() {
        let label = row();
        let (grid, plan) = run(&label, 100, 1, 45.0, None);
        assert_eq!(label_islands(&grid).0, 0);
        assert_eq!(&plan.routes[2..], &[1, 1, 1]);
        assert_eq!(plan.paths.len(), 3);
        for island in 2..=4 {
            assert_eq!(plan.directions[island].len(), 1);
        }
        let axes: Vec<_> = plan.paths.iter().map(|p| p.axis.unwrap()).collect();
        let [first, last] = axes[0];
        let direction = [last[0] - first[0], last[1] - first[1]];
        for [a, b] in &axes {
            for p in [a, b] {
                assert!(
                    ((p[0] - first[0]) * direction[1] - (p[1] - first[1]) * direction[0]).abs()
                        < 1e-8,
                    "{axes:?}"
                );
            }
        }
        assert!(grid
            .m
            .iter()
            .zip(&label)
            .enumerate()
            .all(|(i, (&solid, &l))| { !solid || l > 0 || (8..70).contains(&(i % 100)) }));
        let (_, again) = run(&label, 100, 1, 45.0, None);
        assert_eq!(
            plan.paths.iter().map(|p| p.axis).collect::<Vec<_>>(),
            again.paths.iter().map(|p| p.axis).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn extra_routes_use_distinct_axes_and_do_not_relax_the_angle() {
        let w = 61;
        let label: Vec<_> = (0..w * w)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                if !(5..56).contains(&x) || !(5..56).contains(&y) {
                    1
                } else if (24..37).contains(&x) && (24..37).contains(&y) {
                    2
                } else {
                    0
                }
            })
            .collect();
        let (grid, plan) = run(&label, w, 5, 90.0, None);
        assert_eq!(
            label_islands(&grid).0,
            0,
            "routes={:?}, axes={:?}",
            plan.routes,
            plan.paths.iter().map(|p| p.axis).collect::<Vec<_>>()
        );
        assert_eq!(plan.routes[2], 4);
        assert_eq!(plan.directions[2].len(), 2);
        let a = plan.directions[2][0];
        let b = plan.directions[2][1];
        assert!((a[0] * b[0] + a[1] * b[1]).abs() < 1e-9);
    }

    #[test]
    fn pockets_cannot_be_crossed_even_to_reach_more_islands() {
        let label = row();
        let avoid: Vec<_> = (0..label.len())
            .map(|i| (8..20).contains(&(i % 100)))
            .collect();
        let (grid, plan) = run(&label, 100, 1, 45.0, Some(&avoid));
        assert_eq!(plan.carved, 0);
        assert_eq!(label_islands(&grid).0, 3);
        assert_eq!(&plan.routes[2..], &[0, 0, 0]);
    }

    #[test]
    fn diagonal_gap_axes_remain_exactly_collinear() {
        let w = 81;
        let label: Vec<_> = (0..w * w)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                if x < 8 && y < 8 {
                    1
                } else if (25..35).contains(&x) && (25..35).contains(&y) {
                    2
                } else if (52..62).contains(&x) && (52..62).contains(&y) {
                    3
                } else {
                    0
                }
            })
            .collect();
        let (grid, plan) = run(&label, w, 1, 45.0, None);
        assert_eq!(
            label_islands(&grid).0,
            0,
            "routes={:?}, axes={:?}",
            plan.routes,
            plan.paths.iter().map(|p| p.axis).collect::<Vec<_>>()
        );
        assert_eq!(plan.paths.len(), 2);
        for path in &plan.paths {
            let [a, b] = path.axis.unwrap();
            assert!(((b[1] - a[1]) / (b[0] - a[0]) - 1.0).abs() < 1e-9);
        }
    }
}
