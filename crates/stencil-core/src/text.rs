//! Text to mask: DejaVu Bold glyphs rasterised with fontdue, one or more centred lines.

use crate::graph::Mask;

const SANS: &[u8] = include_bytes!("../fonts/DejaVuSans-Bold.ttf");
const SERIF: &[u8] = include_bytes!("../fonts/DejaVuSerif-Bold.ttf");
pub const FONTS: [&str; 2] = ["sans bold", "serif bold"];
const MAX_PIXELS: usize = 25_000_000;

pub fn render(
    text: &str,
    font: &str,
    size: f32,
    pad: usize,
    line_gap: f32,
) -> Result<Mask, String> {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    if lines.iter().all(|l| l.trim().is_empty()) {
        return Err("the text is empty: type something in the Text parameter".into());
    }
    let bytes = if font == "serif bold" { SERIF } else { SANS };
    let f = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .map_err(|e| format!("cannot load the font: {e}"))?;
    let lm = f
        .horizontal_line_metrics(size)
        .ok_or("the font has no line metrics")?;
    let pitch = lm.new_line_size * line_gap;

    let width_of = |l: &str| -> f32 {
        let mut w = 0.0;
        let mut prev = None;
        for c in l.chars() {
            if let Some(p) = prev {
                w += f.horizontal_kern(p, c, size).unwrap_or(0.0);
            }
            w += f.metrics(c, size).advance_width;
            prev = Some(c);
        }
        w
    };
    let widths: Vec<f32> = lines.iter().map(|l| width_of(l)).collect();
    let inner_w = widths.iter().cloned().fold(0.0, f32::max).ceil() as usize + 1;
    let inner_h = (lm.ascent - lm.descent + pitch * (lines.len() - 1) as f32).ceil() as usize + 1;
    let (w, h) = (inner_w + 2 * pad, inner_h + 2 * pad);
    if w * h > MAX_PIXELS {
        return Err(
            "the text image would be too large: lower the text height or shorten the text".into(),
        );
    }

    let mut paint = vec![false; w * h];
    for (li, l) in lines.iter().enumerate() {
        let baseline = pad as f32 + lm.ascent + pitch * li as f32;
        let mut x = pad as f32 + (inner_w as f32 - widths[li]) / 2.0;
        let mut prev = None;
        for c in l.chars() {
            if let Some(p) = prev {
                x += f.horizontal_kern(p, c, size).unwrap_or(0.0);
            }
            let (m, cov) = f.rasterize(c, size);
            let gx = (x + m.xmin as f32).round() as i64;
            let gy = (baseline - m.ymin as f32 - m.height as f32).round() as i64;
            for row in 0..m.height {
                for col in 0..m.width {
                    if cov[row * m.width + col] >= 128 {
                        let (px, py) = (gx + col as i64, gy + row as i64);
                        if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                            paint[py as usize * w + px as usize] = true;
                        }
                    }
                }
            }
            x += m.advance_width;
            prev = Some(c);
        }
    }
    Ok(Mask { w, h, paint })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_paint_inside_a_padded_canvas() {
        let m = render("Hi", "sans bold", 60.0, 10, 1.0).unwrap();
        let n = m.paint.iter().filter(|&&p| p).count();
        assert!(n > 500 && n < m.w * m.h / 2);
        assert!(
            m.paint[..m.w * 10].iter().all(|&p| !p),
            "padding stays empty"
        );
    }

    #[test]
    fn two_lines_are_taller_than_one_and_fonts_differ() {
        let one = render("AB", "sans bold", 50.0, 5, 1.0).unwrap();
        let two = render("AB\nAB", "sans bold", 50.0, 5, 1.0).unwrap();
        assert!(two.h > one.h * 3 / 2 && two.w == one.w);
        let serif = render("AB", "serif bold", 50.0, 5, 1.0).unwrap();
        assert!(serif.paint != one.paint || serif.w != one.w);
    }

    #[test]
    fn empty_text_is_an_error() {
        assert!(render("  \n ", "sans bold", 50.0, 5, 1.0).is_err());
    }
}
