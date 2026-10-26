//! The "spread" bridging strategy: islands are bridged in passes, a few well-spaced ones first, so that later
//! bridges can leave at a different angle from the ones around them and may land on an island that is already
//! held. Every bridge ends on a full-height piece, so the raised ramps need nothing special.

use super::{closest_pair, contains, distance_sq, slide, unit_direction, Params, Pt, SegIndex};

pub(super) struct Island {
    pub outline: Vec<Pt>,
    /// Every contour of the island (a ring's inside can be bridged to the ring itself).
    pub contours: Vec<Vec<Pt>>,
}

/// The shortest distance between the segments ab and cd.
fn segment_distance(a: Pt, b: Pt, c: Pt, d: Pt) -> f64 {
    let side = |p: Pt, q: Pt, r: Pt| (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
    if side(a, b, c) * side(a, b, d) < 0.0 && side(c, d, a) * side(c, d, b) < 0.0 {
        return 0.0;
    }
    let near = |p: Pt, s: Pt, e: Pt| {
        let v = [e[0] - s[0], e[1] - s[1]];
        let den = v[0] * v[0] + v[1] * v[1];
        let t = if den > 0.0 {
            (((p[0] - s[0]) * v[0] + (p[1] - s[1]) * v[1]) / den).clamp(0.0, 1.0)
        } else {
            0.0
        };
        distance_sq(p, [s[0] + t * v[0], s[1] + t * v[1]])
    };
    [near(a, c, d), near(b, c, d), near(c, a, b), near(d, a, b)]
        .into_iter()
        .fold(f64::INFINITY, f64::min)
        .sqrt()
}

/// Links from the island outline to the target in many directions: from sampled outline points, a ray that leaves
/// the island and meets the target within `reach` before anything else of the island.
fn ray_options(outline: &[Pt], target: &SegIndex, reach: f64, width: f64) -> Vec<(f64, Pt, Pt)> {
    const DIRECTIONS: usize = 24;
    let own = SegIndex::new(std::slice::from_ref(&outline.to_vec()), width.max(2.0));
    let step = (outline.len() / 96).max(1);
    let mut found = Vec::new();
    for k in (0..outline.len()).step_by(step) {
        let from = outline[k];
        for n in 0..DIRECTIONS {
            let t = n as f64 * std::f64::consts::TAU / DIRECTIONS as f64;
            let d = [t.cos(), t.sin()];
            let Some(len) = target.first_hit(from, d, 1e-6, reach) else {
                continue;
            };
            let probe = [from[0] + d[0] * 1e-3, from[1] + d[1] * 1e-3];
            if contains(outline, probe) || own.first_hit(from, d, 1e-3, len).is_some() {
                continue;
            }
            found.push((
                len * len,
                from,
                [from[0] + d[0] * len, from[1] + d[1] * len],
            ));
        }
    }
    found
}

struct Candidate {
    from: Pt,
    to: Pt,
    length: f64,
    /// The largest |cos| of the angle to a bridge nearby: 1 is parallel, 0 perpendicular.
    parallel: f64,
    overlaps: usize,
}

struct Choice<'a> {
    p: &'a Params,
    /// Bridges already placed by anyone.
    links: &'a [(Pt, Pt)],
}

impl Choice<'_> {
    /// The best link from `outline` to `target`: the one that overlaps other bridges least among those that
    /// keep the wanted angle to the bridges around, relaxing the angle in steps. `own` are this island's bridges,
    /// which always count as nearby.
    fn best(
        &self,
        outline: &[Pt],
        target: &SegIndex,
        own: &[(Pt, Pt)],
        shortest: f64,
    ) -> Option<(Pt, Pt)> {
        let p = self.p;
        let reach = p.bridge_max_length * shortest.max(p.bridge_mm) + 4.0 * p.bridge_mm;
        let radius = 2.0 * shortest.max(p.bridge_mm) + 4.0 * p.bridge_mm;
        let touching = 1.5 * p.bridge_mm;
        let spacing = 3.0 * p.bridge_mm.max(1.0);
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for q in outline {
            for k in 0..2 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
        let margin = reach + radius;
        let nearby: Vec<(Pt, Pt)> = self
            .links
            .iter()
            .copied()
            .filter(|l| {
                (0..2).all(|k| {
                    l.0[k].max(l.1[k]) >= lo[k] - margin && l.0[k].min(l.1[k]) <= hi[k] + margin
                })
            })
            .collect();

        let step = (outline.len() / 256).max(1);
        let mut options: Vec<(f64, Pt, Pt)> = (0..outline.len())
            .step_by(step)
            .filter_map(|k| target.nearest(outline[k]).map(|(d, c)| (d, outline[k], c)))
            .filter(|o| o.0.sqrt() <= reach)
            .collect();
        options.extend(ray_options(outline, target, reach, p.bridge_mm));
        let mut candidates: Vec<Candidate> = options
            .into_iter()
            .filter(|o| {
                !own.iter()
                    .any(|l| distance_sq(l.0, o.1) < spacing * spacing)
            })
            .map(|(d2, from, to)| {
                let dir = unit_direction(from, to);
                let mut parallel: f64 = 0.0;
                let mut overlaps = 0;
                let mut consider = |l: &(Pt, Pt), always: bool| {
                    let gap = segment_distance(from, to, l.0, l.1);
                    if always || gap < radius {
                        let e = unit_direction(l.0, l.1);
                        parallel = parallel.max((dir[0] * e[0] + dir[1] * e[1]).abs());
                    }
                    if gap < touching {
                        overlaps += 1;
                    }
                };
                for l in &nearby {
                    consider(l, false);
                }
                for l in own {
                    consider(l, true);
                }
                Candidate {
                    from,
                    to,
                    length: d2.sqrt(),
                    parallel,
                    overlaps,
                }
            })
            .collect();
        candidates.sort_by(|a, b| a.length.total_cmp(&b.length));
        let wanted = p.bridge_min_angle_deg;
        // the wanted angle is relaxed in steps, so an island never gets fewer bridges than without it
        for tier in [wanted, wanted * 2.0 / 3.0, wanted / 3.0, 0.0] {
            let widest = tier.to_radians().cos();
            let best = candidates
                .iter()
                .filter(|c| tier <= 0.0 || c.parallel <= widest)
                .min_by(|a, b| {
                    a.overlaps
                        .cmp(&b.overlaps)
                        .then(a.length.total_cmp(&b.length))
                });
            if let Some(c) = best {
                return Some((c.from, c.to));
            }
        }
        None
    }
}

/// The bridges of every island, placed in passes: a pass takes a growing share of the islands still loose
/// (1/16 of all, then 1/8, 1/4, 1/2 of those left, and everything in the last), the ones farthest from the islands
/// that are held already, so the bridges spread over the plate. A bridge may end on the frame or on an island
/// that was held before the pass. Islands with more bridges wanted get them afterwards, onto the frame.
pub(super) fn plan(
    islands: &[Island],
    frame_contours: &[Vec<Pt>],
    frame: &SegIndex,
    p: &Params,
    nudge: f64,
    per_island: usize,
) -> Result<Vec<(Pt, Pt)>, String> {
    let n = islands.len();
    let mut links: Vec<(Pt, Pt)> = Vec::new();
    let mut own: Vec<Vec<(Pt, Pt)>> = vec![Vec::new(); n];
    let mut held = vec![false; n];
    let centre: Vec<Pt> = islands
        .iter()
        .map(|i| {
            let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
            for q in &i.outline {
                for k in 0..2 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
            }
            [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5]
        })
        .collect();
    let size: Vec<f64> = islands
        .iter()
        .map(|i| {
            let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
            for q in &i.outline {
                for k in 0..2 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
            }
            (hi[0] - lo[0]) * (hi[1] - lo[1])
        })
        .collect();
    let passes = p.bridge_passes.clamp(1, 8) as usize;
    let cell = p.bridge_mm.max(2.0);
    for pass in 1..=passes {
        let loose: Vec<usize> = (0..n).filter(|&i| !held[i]).collect();
        if loose.is_empty() {
            break;
        }
        let take = if pass == passes {
            loose.len()
        } else {
            let share = 16usize >> (pass - 1).min(4);
            loose.len().div_ceil(share).max(1)
        };
        let anchored: Vec<usize> = (0..n).filter(|&i| held[i]).collect();
        let mut away: Vec<f64> = loose
            .iter()
            .map(|&i| {
                anchored
                    .iter()
                    .map(|&a| distance_sq(centre[i], centre[a]))
                    .fold(f64::INFINITY, f64::min)
            })
            .collect();
        let mut taken = vec![false; loose.len()];
        let mut order = Vec::with_capacity(take);
        for _ in 0..take {
            let pick = (0..loose.len()).filter(|&k| !taken[k]).max_by(|&x, &y| {
                let key = |k: usize| {
                    if away[k].is_finite() {
                        away[k]
                    } else {
                        size[loose[k]]
                    }
                };
                key(x).total_cmp(&key(y)).then(y.cmp(&x))
            });
            let Some(k) = pick else { break };
            taken[k] = true;
            order.push(loose[k]);
            for (j, &i) in loose.iter().enumerate() {
                away[j] = away[j].min(distance_sq(centre[i], centre[loose[k]]));
            }
        }

        let mut polys = frame_contours.to_vec();
        for &a in &anchored {
            polys.extend(islands[a].contours.iter().cloned());
        }
        let wider = (!anchored.is_empty()).then(|| SegIndex::new(&polys, cell));
        let target = wider.as_ref().unwrap_or(frame);
        for &i in &order {
            let outline = &islands[i].outline;
            let closest = closest_pair(outline, target).ok_or("frame has no boundary")?;
            let shortest = distance_sq(closest.0, closest.1).sqrt();
            let choice = Choice { p, links: &links };
            let (a, b) = choice
                .best(outline, target, &[], shortest)
                .unwrap_or(closest);
            let link = slide(a, b, nudge, p.bridge_mm);
            links.push(link);
            own[i].push(link);
        }
        for &i in &order {
            held[i] = true;
        }
    }

    for i in 0..n {
        for _ in 1..per_island {
            let outline = &islands[i].outline;
            let first = own[i][0];
            let shortest = distance_sq(first.0, first.1).sqrt();
            let choice = Choice { p, links: &links };
            let Some((a, b)) = choice.best(outline, frame, &own[i], shortest) else {
                break;
            };
            let link = slide(a, b, nudge, p.bridge_mm);
            links.push(link);
            own[i].push(link);
        }
    }
    Ok(links)
}
