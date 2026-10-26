use super::*;
use crate::trace::{trace, TraceParams};
use crate::ZBridging;

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as f64 / (1u64 << 31) as f64
    }
}

/// Random discs, rings and rectangles: nested shapes and loose pieces both create islands.
fn blobs(rng: &mut Lcg, n: usize) -> Vec<bool> {
    let mut m = vec![false; n * n];
    for _ in 0..(3 + (rng.next() * 6.0) as usize) {
        let (cx, cy) = (rng.next() * n as f64, rng.next() * n as f64);
        let r = 4.0 + rng.next() * n as f64 * 0.25;
        let hole = if rng.next() < 0.5 {
            rng.next() * 0.7
        } else {
            0.0
        };
        let rect = rng.next() < 0.3;
        for y in 0..n {
            for x in 0..n {
                let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
                let d = if rect {
                    dx.abs().max(dy.abs())
                } else {
                    dx.hypot(dy)
                };
                if d < r && d >= r * hole {
                    m[y * n + x] = true;
                }
            }
        }
    }
    m
}

#[test]
fn random_shapes_always_build_island_free_watertight_plates() {
    let tp = TraceParams {
        smooth_px: 1.0,
        corner_deg: 60.0,
        tolerance_px: 0.4,
        min_area_px: 4.0,
    };
    let mut built = 0;
    for seed in 0..24u64 {
        let mut rng = Lcg(seed * 7919 + 1);
        let n = 90;
        let mask = blobs(&mut rng, n);
        let shapes = trace(&mask, n, n, &tp);
        for (raised, z) in [
            (false, ZBridging::Steps),
            (true, ZBridging::Steps),
            (true, ZBridging::Easy),
        ] {
            let p = Params {
                width_mm: 80.0,
                margin_mm: 4.0,
                bridges_per_island: 1 + (seed as u32 % 5),
                bridge_min_angle_deg: if seed % 2 == 0 { 45.0 } else { 0.0 },
                raised_bridges: raised,
                z_bridging: z,
                ..Params::default()
            };
            match build(&shapes, 2.0, &p) {
                Ok(o) => {
                    assert_eq!(o.stats.islands_remaining, 0, "seed {seed} raised {raised}");
                    assert!(o.stats.watertight, "seed {seed} raised {raised}");
                    built += 1;
                }
                Err(e) => assert!(
                    shapes.loops.is_empty(),
                    "seed {seed} raised {raised} {z:?}: {e}"
                ),
            }
        }
    }
    assert!(built >= 40, "only {built} plates built");
}

#[test]
fn extra_bridges_leave_an_island_in_different_directions() {
    let tp = TraceParams {
        smooth_px: 1.0,
        corner_deg: 60.0,
        tolerance_px: 0.4,
        min_area_px: 4.0,
    };
    let n = 120;
    let mask: Vec<bool> = (0..n * n)
        .map(|i| {
            let (x, y) = ((i % n) as f64 - 60.0, (i / n) as f64 - 60.0);
            (30.0..40.0).contains(&x.hypot(y))
        })
        .collect();
    let shapes = trace(&mask, n, n, &tp);
    let count = |k| {
        let p = Params {
            width_mm: 80.0,
            margin_mm: 4.0,
            bridges_per_island: k,
            ..Params::default()
        };
        build(&shapes, 2.0, &p).unwrap().stats.bridge_pixels
    };
    assert_eq!((count(1), count(3), count(5)), (1, 3, 5));
}

#[test]
fn spread_passes_bridge_every_island_for_any_pass_count() {
    let tp = TraceParams {
        smooth_px: 1.0,
        corner_deg: 60.0,
        tolerance_px: 0.4,
        min_area_px: 4.0,
    };
    let n = 120;
    // many small blobs on a grid, each an island once the frame is cut around them
    let mask: Vec<bool> = (0..n * n)
        .map(|i| {
            let (x, y) = (i % n, i / n);
            (x % 12 < 5 && y % 12 < 5 && x > 6 && y > 6 && x < 110 && y < 110)
                || (x as f64 - 60.0).hypot(y as f64 - 60.0) > 52.0
        })
        .collect();
    let shapes = trace(&mask, n, n, &tp);
    for passes in [1, 2, 4, 8] {
        for per in [1, 3] {
            let p = Params {
                width_mm: 100.0,
                margin_mm: 4.0,
                bridges_per_island: per,
                bridge_min_angle_deg: 60.0,
                bridge_passes: passes,
                ..Params::default()
            };
            let o = build(&shapes, 2.0, &p).unwrap();
            assert_eq!(o.stats.islands_remaining, 0, "{passes} passes, {per} each");
            assert!(o.stats.watertight, "{passes} passes, {per} each");
        }
    }
}

#[test]
fn spread_bridges_never_cost_an_island_a_bridge() {
    let tp = TraceParams {
        smooth_px: 1.0,
        corner_deg: 60.0,
        tolerance_px: 0.4,
        min_area_px: 4.0,
    };
    let n = 120;
    let mask: Vec<bool> = (0..n * n)
        .map(|i| {
            let (x, y) = ((i % n) as f64 - 60.0, (i / n) as f64 - 60.0);
            (30.0..40.0).contains(&x.hypot(y)) || (8.0..14.0).contains(&x.hypot(y))
        })
        .collect();
    let shapes = trace(&mask, n, n, &tp);
    for k in 1..=4 {
        let run = |angle| {
            let p = Params {
                width_mm: 80.0,
                margin_mm: 4.0,
                bridges_per_island: k,
                bridge_min_angle_deg: angle,
                ..Params::default()
            };
            build(&shapes, 2.0, &p).unwrap()
        };
        let (closest, spread) = (run(0.0), run(60.0));
        assert_eq!(
            closest.stats.bridge_pixels, spread.stats.bridge_pixels,
            "{k}"
        );
        assert_eq!(spread.stats.islands_remaining, 0);
        assert!(spread.stats.watertight);
    }
}

#[test]
fn delete_islands_threshold_is_in_square_millimetres() {
    let tp = TraceParams {
        smooth_px: 1.0,
        corner_deg: 60.0,
        tolerance_px: 0.4,
        min_area_px: 4.0,
    };
    let n = 120;
    // a ring: its inside is one island of about 25 mm² at 100 mm wide
    let mask: Vec<bool> = (0..n * n)
        .map(|i| {
            let (x, y) = ((i % n) as f64 - 60.0, (i / n) as f64 - 60.0);
            (20.0..30.0).contains(&x.hypot(y))
        })
        .collect();
    let shapes = trace(&mask, n, n, &tp);
    let run = |min: f64| {
        let p = Params {
            width_mm: 100.0,
            margin_mm: 4.0,
            min_island_mm2: min,
            ..Params::default()
        };
        build(&shapes, 2.0, &p).unwrap().stats
    };
    assert_eq!(run(0.0).islands_deleted, 0);
    assert_eq!(
        run(1.0).islands_deleted,
        0,
        "an island above the limit must stay"
    );
    assert_eq!(run(1.0e6).islands_deleted, 1);
}

#[test]
fn thin_rope_is_widened_by_minimum_contact() {
    let (w, h) = (200usize, 100usize);
    let mask: Vec<bool> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let blob = ((20..60).contains(&x) || (140..180).contains(&x)) && (30..70).contains(&y);
            !(blob || ((60..140).contains(&x) && (49..51).contains(&y)))
        })
        .collect();
    let tp = TraceParams {
        smooth_px: 0.0,
        corner_deg: 60.0,
        tolerance_px: 0.3,
        min_area_px: 4.0,
    };
    let shapes = trace(&mask, w, h, &tp);
    let run = |min_contact_mm: f64, raised: bool| {
        let p = Params {
            width_mm: 100.0,
            margin_mm: 4.0,
            min_contact_mm,
            raised_bridges: raised,
            ..Params::default()
        };
        build(&shapes, 4.0, &p).unwrap()
    };
    for raised in [false, true] {
        let (off, on) = (run(0.0, raised), run(3.0, raised));
        assert!(on.stats.watertight && on.stats.islands_remaining == 0);
        let (v0, v1) = (crate::stl_volume(&off.stl), crate::stl_volume(&on.stl));
        assert!(v1 - v0 > 100.0, "raised {raised}: grew by only {}", v1 - v0);
    }
}

#[test]
fn first_layer_flare_tapers_back_without_changing_bridge_selection() {
    let (w, h) = (200usize, 100usize);
    let mask: Vec<bool> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let blob = ((20..60).contains(&x) || (140..180).contains(&x)) && (30..70).contains(&y);
            !(blob || ((60..140).contains(&x) && (49..51).contains(&y)))
        })
        .collect();
    let tp = TraceParams {
        smooth_px: 0.0,
        corner_deg: 60.0,
        tolerance_px: 0.3,
        min_area_px: 4.0,
    };
    let shapes = trace(&mask, w, h, &tp);
    let run = |width: f64, taper: f64, raised_bridges: bool, flip: bool| {
        let p = Params {
            width_mm: 100.0,
            margin_mm: 4.0,
            raised_bridges,
            flip,
            min_first_layer_width_mm: width,
            first_layer_taper_mm: taper,
            ..Params::default()
        };
        build(&shapes, 4.0, &p).unwrap()
    };
    for raised in [false, true] {
        for flip in [false, true] {
            let (plain, short, long) = (
                run(0.0, 1.0, raised, flip),
                run(3.0, 0.4, raised, flip),
                run(3.0, 1.6, raised, flip),
            );
            assert!(short.stats.watertight && long.stats.watertight);
            assert_eq!(short.stats.islands_remaining, 0);
            assert_eq!(short.stats.bridge_pixels, plain.stats.bridge_pixels);
            let base = crate::stl_volume(&plain.stl);
            let short_gain = crate::stl_volume(&short.stl) - base;
            let long_gain = crate::stl_volume(&long.stl) - base;
            assert!(
                short_gain > 0.0,
                "raised {raised}, flip {flip}: no bottom flare was added"
            );
            assert!(
                long_gain > short_gain,
                "raised {raised}, flip {flip}: a taller taper should add more flare volume: short {short_gain}, long {long_gain}"
            );
        }
    }
}

#[test]
fn rejects_degenerate_input() {
    let empty = Shapes {
        w: 10,
        h: 10,
        loops: vec![],
    };
    assert!(build(&empty, 2.0, &Params::default()).is_err());
    let tp = TraceParams {
        smooth_px: 1.0,
        corner_deg: 60.0,
        tolerance_px: 0.4,
        min_area_px: 1.0,
    };
    let mut m = vec![false; 400];
    for y in 5..15 {
        for x in 5..15 {
            m[y * 20 + x] = true;
        }
    }
    let s = trace(&m, 20, 20, &tp);
    assert!(build(&s, 0.0, &Params::default()).is_err());
    assert!(build(&s, f64::NAN, &Params::default()).is_err());
    assert!(build(
        &s,
        2.0,
        &Params {
            width_mm: 5.0,
            margin_mm: 4.0,
            ..Params::default()
        }
    )
    .is_err());
}
