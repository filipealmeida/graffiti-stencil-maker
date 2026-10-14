//! Binary morphology on masks: kernels, erosion/dilation and their compounds, thinning, skeletons and hit-or-miss.
//!
//! Every kernel is symmetric about its centre, so erosion and dilation need no reflection. Kernels are stored as
//! horizontal runs per row, which lets a pass cost one prefix-sum lookup per run and pixel instead of one per kernel cell.

/// A run of kernel cells `x0..=x1` on row `dy`.
#[derive(Clone, Copy, Debug)]
pub struct Run {
    pub dy: i32,
    pub x0: i32,
    pub x1: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Square,
    Disc,
    Diamond,
    Cross,
    Line,
    Ring,
}

impl Shape {
    pub fn parse(s: &str) -> Option<Shape> {
        Some(match s {
            "square" => Shape::Square,
            "disc" => Shape::Disc,
            "diamond" => Shape::Diamond,
            "cross" => Shape::Cross,
            "line" => Shape::Line,
            "ring" => Shape::Ring,
            _ => return None,
        })
    }
}

/// Builds the kernel of the given shape. `radius` is the distance from the centre to the edge, in pixels;
/// `angle_deg` only matters for lines.
pub fn kernel(shape: Shape, radius: u32, angle_deg: f64) -> Vec<Run> {
    let r = radius as i32;
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    let inside = |dx: i32, dy: i32| -> bool {
        let (x, y) = (dx as f64, dy as f64);
        let d2 = x * x + y * y;
        let rr = radius as f64;
        match shape {
            Shape::Square => true,
            Shape::Disc => d2 <= rr * rr + rr,
            Shape::Diamond => dx.abs() + dy.abs() <= r,
            Shape::Cross => dx == 0 || dy == 0,
            Shape::Ring => {
                d2 <= (rr + 0.5) * (rr + 0.5)
                    && d2 > (rr - 1.0).max(0.0).powi(2) + (rr - 1.0).max(0.0)
            }
            // within half a pixel of the line through the centre
            Shape::Line => {
                let along = x * cos - y * sin;
                let across = x * sin + y * cos;
                across.abs() <= 0.5 && along.abs() <= rr + 0.5
            }
        }
    };
    let mut runs = Vec::new();
    for dy in -r..=r {
        let mut start: Option<i32> = None;
        for dx in -r..=r + 1 {
            let on = dx <= r && inside(dx, dy);
            match (on, start) {
                (true, None) => start = Some(dx),
                (false, Some(s)) => {
                    runs.push(Run {
                        dy,
                        x0: s,
                        x1: dx - 1,
                    });
                    start = None;
                }
                _ => {}
            }
        }
    }
    if runs.is_empty() {
        runs.push(Run {
            dy: 0,
            x0: 0,
            x1: 0,
        });
    }
    runs
}

/// Number of cells in the kernel.
pub fn cells(k: &[Run]) -> usize {
    k.iter().map(|r| (r.x1 - r.x0 + 1) as usize).sum()
}

fn prefix(w: usize, h: usize, m: &[bool]) -> Vec<u32> {
    let mut p = vec![0u32; (w + 1) * h];
    for y in 0..h {
        let mut s = 0;
        for x in 0..w {
            s += m[y * w + x] as u32;
            p[y * (w + 1) + x + 1] = s;
        }
    }
    p
}

/// Dilation (`erode == false`) or erosion. With `edge_free`, cells outside the picture are ignored, so paint touching the
/// border is not eaten from outside; otherwise they count as background.
fn pass(w: usize, h: usize, m: &[bool], k: &[Run], erode: bool, edge_free: bool) -> Vec<bool> {
    let p = prefix(w, h, m);
    let mut out = vec![erode; w * h];
    let stride = w + 1;
    for y in 0..h {
        let row = &mut out[y * w..(y + 1) * w];
        for run in k {
            let yy = y as i32 + run.dy;
            let in_rows = yy >= 0 && (yy as usize) < h;
            if !in_rows {
                // a missing row is background: it kills an erosion unless the edge is free
                if erode && !edge_free {
                    row.fill(false);
                }
                continue;
            }
            let base = yy as usize * stride;
            for (x, slot) in row.iter_mut().enumerate() {
                if erode && !*slot {
                    continue;
                }
                if !erode && *slot {
                    continue;
                }
                let lo = (x as i32 + run.x0).max(0);
                let hi = (x as i32 + run.x1).min(w as i32 - 1);
                let want = (run.x1 - run.x0 + 1) as u32;
                let (cnt, len) = if hi < lo {
                    (0, 0)
                } else {
                    (
                        p[base + hi as usize + 1] - p[base + lo as usize],
                        (hi - lo + 1) as u32,
                    )
                };
                if erode {
                    let full = if edge_free { cnt == len } else { cnt == want };
                    if !full {
                        *slot = false;
                    }
                } else if cnt > 0 {
                    *slot = true;
                }
            }
        }
    }
    out
}

pub fn dilate(w: usize, h: usize, m: &[bool], k: &[Run], n: u32) -> Vec<bool> {
    (0..n).fold(m.to_vec(), |a, _| pass(w, h, &a, k, false, true))
}

pub fn erode(w: usize, h: usize, m: &[bool], k: &[Run], n: u32) -> Vec<bool> {
    (0..n).fold(m.to_vec(), |a, _| pass(w, h, &a, k, true, true))
}

pub fn open(w: usize, h: usize, m: &[bool], k: &[Run], n: u32) -> Vec<bool> {
    dilate(w, h, &erode(w, h, m, k, n), k, n)
}

pub fn close(w: usize, h: usize, m: &[bool], k: &[Run], n: u32) -> Vec<bool> {
    erode(w, h, &dilate(w, h, m, k, n), k, n)
}

fn diff(a: &[bool], b: &[bool]) -> Vec<bool> {
    a.iter().zip(b).map(|(&x, &y)| x && !y).collect()
}

/// Dilation minus erosion: the outline band around every shape.
pub fn gradient(w: usize, h: usize, m: &[bool], k: &[Run], n: u32) -> Vec<bool> {
    diff(&dilate(w, h, m, k, n), &erode(w, h, m, k, n))
}

/// The input minus its opening: bright details narrower than the kernel.
pub fn white_hat(w: usize, h: usize, m: &[bool], k: &[Run], n: u32) -> Vec<bool> {
    diff(m, &open(w, h, m, k, n))
}

/// The closing minus the input: gaps and holes narrower than the kernel.
pub fn black_hat(w: usize, h: usize, m: &[bool], k: &[Run], n: u32) -> Vec<bool> {
    diff(&close(w, h, m, k, n), m)
}

/// Zhang-Suen thinning down to one-pixel-wide, 8-connected lines. `max_passes` bounds the work (0 = until stable).
pub fn thin(w: usize, h: usize, m: &[bool], max_passes: u32) -> Vec<bool> {
    let mut a = m.to_vec();
    let at = |a: &[bool], x: i64, y: i64| -> bool {
        x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && a[y as usize * w + x as usize]
    };
    let mut pass_no = 0;
    loop {
        let mut changed = false;
        for step in 0..2 {
            let mut kill = Vec::new();
            for y in 0..h as i64 {
                for x in 0..w as i64 {
                    if !a[y as usize * w + x as usize] {
                        continue;
                    }
                    // neighbours clockwise from north
                    let n = [
                        at(&a, x, y - 1),
                        at(&a, x + 1, y - 1),
                        at(&a, x + 1, y),
                        at(&a, x + 1, y + 1),
                        at(&a, x, y + 1),
                        at(&a, x - 1, y + 1),
                        at(&a, x - 1, y),
                        at(&a, x - 1, y - 1),
                    ];
                    let b = n.iter().filter(|&&v| v).count();
                    let t = (0..8).filter(|&i| !n[i] && n[(i + 1) % 8]).count();
                    let (p2, p4, p6, p8) = (n[0], n[2], n[4], n[6]);
                    let ok = if step == 0 {
                        !(p2 && p4 && p6) && !(p4 && p6 && p8)
                    } else {
                        !(p2 && p4 && p8) && !(p2 && p6 && p8)
                    };
                    if (2..=6).contains(&b) && t == 1 && ok {
                        kill.push(y as usize * w + x as usize);
                    }
                }
            }
            changed |= !kill.is_empty();
            for i in kill {
                a[i] = false;
            }
        }
        pass_no += 1;
        if !changed || (max_passes > 0 && pass_no >= max_passes) {
            return a;
        }
    }
}

/// Morphological (Lantuéjoul) skeleton with a 3x3 square: the union of what each successive erosion loses to an opening.
/// Thinner than the shape, but not guaranteed to stay connected (use `thin` for that).
pub fn skeleton(w: usize, h: usize, m: &[bool]) -> Vec<bool> {
    let b = kernel(Shape::Square, 1, 0.0);
    let mut e = m.to_vec();
    let mut s = vec![false; w * h];
    while e.iter().any(|&v| v) {
        // outside the picture counts as background here, so shapes touching the border still shrink to nothing
        let eroded = pass(w, h, &e, &b, true, false);
        let opened = pass(w, h, &eroded, &b, false, true);
        for i in 0..s.len() {
            if e[i] && !opened[i] {
                s[i] = true;
            }
        }
        e = eroded;
    }
    s
}

/// A 3x3 hit-or-miss pattern: `Some(true)` must be paint, `Some(false)` must be empty, `None` is ignored.
/// Cells are row-major, so index 4 is the centre.
pub type Pattern = [Option<bool>; 9];

/// Parses nine of `1`, `0` and `x` (also `X`, `-`, `?`, `.`); whitespace, `/` and `,` are ignored.
pub fn parse_pattern(s: &str) -> Result<Pattern, String> {
    let mut out = [None; 9];
    let mut n = 0;
    for c in s
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '/' && *c != ',')
    {
        let v = match c {
            '1' => Some(true),
            '0' => Some(false),
            'x' | 'X' | '-' | '?' | '.' => None,
            _ => return Err(format!("pattern: unexpected '{c}', use 1, 0 or x")),
        };
        if n < 9 {
            out[n] = v;
        }
        n += 1;
    }
    if n != 9 {
        return Err(format!("pattern needs 9 cells (3 rows of 3), got {n}"));
    }
    Ok(out)
}

fn rotate90(p: &Pattern) -> Pattern {
    let mut o = [None; 9];
    for y in 0..3 {
        for x in 0..3 {
            o[x * 3 + (2 - y)] = p[y * 3 + x];
        }
    }
    o
}

fn rotations(p: Pattern) -> Vec<Pattern> {
    let mut v = vec![p];
    for _ in 0..3 {
        let r = rotate90(v.last().unwrap());
        if !v.contains(&r) {
            v.push(r);
        }
    }
    v
}

/// Named presets, each already covering every orientation.
pub fn preset(name: &str) -> Option<Vec<Pattern>> {
    let pat = |s: &str| parse_pattern(s).expect("built-in pattern");
    Some(match name {
        "isolated" => vec![pat("000 010 000")],
        // one painted neighbour in any of the eight directions
        "endpoints" => (0..9)
            .filter(|&i| i != 4)
            .map(|i| {
                let mut p = [Some(false); 9];
                p[4] = Some(true);
                p[i] = Some(true);
                p
            })
            .collect(),
        "corners" => rotations(pat("00x 011 x1x")),
        "junctions" => rotations(pat("x1x 111 x0x"))
            .into_iter()
            .chain(rotations(pat("101 010 x1x")))
            .collect(),
        _ => return None,
    })
}

pub fn hit_or_miss(w: usize, h: usize, m: &[bool], patterns: &[Pattern]) -> Vec<bool> {
    let at = |x: i64, y: i64| {
        x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && m[y as usize * w + x as usize]
    };
    let mut out = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            out[y * w + x] = patterns.iter().any(|p| {
                (0..9).all(|i| match p[i] {
                    None => true,
                    Some(want) => {
                        at(x as i64 + (i % 3) as i64 - 1, y as i64 + (i / 3) as i64 - 1) == want
                    }
                })
            });
        }
    }
    out
}

pub fn custom_patterns(s: &str, rotate: bool) -> Result<Vec<Pattern>, String> {
    let p = parse_pattern(s)?;
    Ok(if rotate { rotations(p) } else { vec![p] })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: &[&str]) -> (usize, usize, Vec<bool>) {
        let h = rows.len();
        let w = rows[0].len();
        (
            w,
            h,
            rows.iter()
                .flat_map(|r| r.chars().map(|c| c == '#'))
                .collect(),
        )
    }

    fn show(w: usize, m: &[bool]) -> Vec<String> {
        m.chunks(w)
            .map(|r| r.iter().map(|&v| if v { '#' } else { '.' }).collect())
            .collect()
    }

    #[test]
    fn square_dilation_grows_by_the_radius() {
        let (w, h, m) = grid(&[".....", ".....", "..#..", ".....", "....."]);
        let d = dilate(w, h, &m, &kernel(Shape::Square, 1, 0.0), 1);
        assert_eq!(show(w, &d), [".....", ".###.", ".###.", ".###.", "....."]);
    }

    #[test]
    fn erosion_removes_what_the_kernel_does_not_fit_in() {
        let (w, h, m) = grid(&[".....", ".###.", ".###.", ".###.", "....."]);
        let e = erode(w, h, &m, &kernel(Shape::Square, 1, 0.0), 1);
        assert_eq!(show(w, &e), [".....", ".....", "..#..", ".....", "....."]);
    }

    #[test]
    fn erosion_does_not_eat_paint_from_the_picture_border() {
        let (w, h, m) = grid(&["###", "###", "###"]);
        let e = erode(w, h, &m, &kernel(Shape::Square, 1, 0.0), 1);
        assert_eq!(e, m);
    }

    #[test]
    fn opening_drops_specks_and_closing_fills_pinholes() {
        let (w, h, m) = grid(&[
            "#........",
            ".........",
            "..#####..",
            "..#####..",
            "..#####..",
            ".........",
            ".........",
        ]);
        let o = open(w, h, &m, &kernel(Shape::Square, 1, 0.0), 1);
        assert!(!o[0] && o[2 * w + 2]);
        let (w, h, m) = grid(&["#######", "#######", "###.###", "#######", "#######"]);
        let c = close(w, h, &m, &kernel(Shape::Square, 1, 0.0), 1);
        assert!(c.iter().all(|&v| v));
    }

    #[test]
    fn gradient_and_hats_are_set_differences() {
        let (w, h, m) = grid(&[
            ".........",
            ".........",
            "..#####..",
            "..#####..",
            "..#####..",
            "..#####..",
            "..#####..",
            ".........",
            ".........",
        ]);
        let k = kernel(Shape::Square, 1, 0.0);
        let g = gradient(w, h, &m, &k, 1);
        // the ring one pixel either side of the border, nothing in the middle
        assert!(g[w + 1] && !g[4 * w + 4] && !g[0]);
        assert!(white_hat(w, h, &m, &k, 1).iter().all(|&v| !v));
        assert!(black_hat(w, h, &m, &k, 1).iter().all(|&v| !v));
    }

    #[test]
    fn kernels_have_the_expected_size() {
        assert_eq!(cells(&kernel(Shape::Square, 2, 0.0)), 25);
        assert_eq!(cells(&kernel(Shape::Diamond, 2, 0.0)), 13);
        assert_eq!(cells(&kernel(Shape::Cross, 3, 0.0)), 13);
        assert_eq!(cells(&kernel(Shape::Line, 3, 0.0)), 7);
        assert_eq!(cells(&kernel(Shape::Line, 3, 90.0)), 7);
        assert!(cells(&kernel(Shape::Disc, 3, 0.0)) > cells(&kernel(Shape::Diamond, 3, 0.0)));
        let ring = kernel(Shape::Ring, 3, 0.0);
        assert!(ring.iter().all(|r| !(r.dy == 0 && r.x0 <= 0 && r.x1 >= 0)));
    }

    #[test]
    fn thinning_reduces_a_bar_to_a_line() {
        let (w, h, m) = grid(&[
            ".........",
            ".#######.",
            ".#######.",
            ".#######.",
            ".........",
        ]);
        let t = thin(w, h, &m, 0);
        let n = t.iter().filter(|&&v| v).count();
        assert!(n > 0 && n <= 7, "{:?}", show(w, &t));
        // a single row, not a blob
        assert!(
            (0..h)
                .filter(|&y| t[y * w..(y + 1) * w].iter().any(|&v| v))
                .count()
                == 1
        );
    }

    #[test]
    fn skeleton_of_a_bar_lies_inside_it() {
        let (w, h, m) = grid(&[
            ".........",
            ".#######.",
            ".#######.",
            ".#######.",
            ".#######.",
            ".#######.",
            ".........",
        ]);
        let s = skeleton(w, h, &m);
        assert!(s.iter().any(|&v| v));
        assert!(s.iter().zip(&m).all(|(&a, &b)| !a || b));
        assert!(s.iter().filter(|&&v| v).count() < m.iter().filter(|&&v| v).count());
    }

    #[test]
    fn hit_or_miss_finds_endpoints_and_isolated_pixels() {
        let (w, h, m) = grid(&["#.....", "......", "..###.", "......"]);
        let iso = hit_or_miss(w, h, &m, &preset("isolated").unwrap());
        assert_eq!(iso.iter().filter(|&&v| v).count(), 1);
        assert!(iso[0]);
        let ends = hit_or_miss(w, h, &m, &preset("endpoints").unwrap());
        assert_eq!(ends.iter().filter(|&&v| v).count(), 2);
        assert!(ends[2 * w + 2] && ends[2 * w + 4]);
    }

    #[test]
    fn hit_or_miss_corners_and_custom_patterns() {
        let (w, h, m) = grid(&[".....", ".###.", ".###.", ".###.", "....."]);
        let c = hit_or_miss(w, h, &m, &preset("corners").unwrap());
        let found: Vec<_> = (0..w * h).filter(|&i| c[i]).collect();
        assert_eq!(found, vec![w + 1, w + 3, 3 * w + 1, 3 * w + 3]);
        let one = custom_patterns("0 0 0 / 0 1 0 / 0 0 0", false).unwrap();
        assert_eq!(one.len(), 1);
        assert!(custom_patterns("11", false).is_err());
        assert!(custom_patterns("11a 000 000", false).is_err());
        assert_eq!(custom_patterns("x00 011 x1x", true).unwrap().len(), 4);
    }

    #[test]
    fn junction_preset_marks_a_t() {
        let (w, h, m) = grid(&["#####", "..#..", "..#..", "..#.."]);
        let j = hit_or_miss(w, h, &m, &preset("junctions").unwrap());
        assert!(j[2]);
    }
}
