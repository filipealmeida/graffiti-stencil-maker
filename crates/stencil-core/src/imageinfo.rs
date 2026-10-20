//! What a loaded picture is: its file format, size, resolution metadata and how it will be read by the pipeline.

use crate::{decode, svg};
use serde_json::{json, Map, Value as Json};

const MM_PER_INCH: f64 = 25.4;

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn format_of(b: &[u8]) -> &'static str {
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        "PNG"
    } else if b.starts_with(&[0xFF, 0xD8]) {
        "JPEG"
    } else if b.starts_with(b"BM") {
        "BMP"
    } else if svg::is_svg(b) {
        "SVG"
    } else {
        "unknown"
    }
}

/// Pixels per inch stored in the file, if any.
fn dpi(b: &[u8], format: &str) -> Option<(f64, f64)> {
    match format {
        "PNG" => {
            let mut at = 8;
            while at + 8 <= b.len() {
                let len = be32(b, at)? as usize;
                match &b[at + 4..at + 8] {
                    b"pHYs" if len >= 9 && b.get(at + 16) == Some(&1) => {
                        let (x, y) = (be32(b, at + 8)? as f64, be32(b, at + 12)? as f64);
                        return (x > 0.0 && y > 0.0)
                            .then(|| (x * MM_PER_INCH / 1000.0, y * MM_PER_INCH / 1000.0));
                    }
                    b"IDAT" | b"IEND" => return None,
                    _ => {}
                }
                at += 12 + len;
            }
            None
        }
        "JPEG" => {
            let mut at = 2;
            while at + 4 <= b.len() && b[at] == 0xFF {
                let marker = b[at + 1];
                let len = u16::from_be_bytes([b[at + 2], b[at + 3]]) as usize;
                if marker == 0xE0 && b.get(at + 4..at + 9) == Some(b"JFIF\0") && len >= 14 {
                    let unit = *b.get(at + 11)?;
                    let x = u16::from_be_bytes([*b.get(at + 12)?, *b.get(at + 13)?]) as f64;
                    let y = u16::from_be_bytes([*b.get(at + 14)?, *b.get(at + 15)?]) as f64;
                    if x <= 0.0 || y <= 0.0 {
                        return None;
                    }
                    return match unit {
                        1 => Some((x, y)),
                        2 => Some((x * MM_PER_INCH / 10.0, y * MM_PER_INCH / 10.0)),
                        _ => None,
                    };
                }
                if marker == 0xDA {
                    return None;
                }
                at += 2 + len;
            }
            None
        }
        "BMP" => {
            let (x, y) = (le32(b, 38)? as f64, le32(b, 42)? as f64);
            (x > 0.0 && y > 0.0).then(|| (x * MM_PER_INCH / 1000.0, y * MM_PER_INCH / 1000.0))
        }
        _ => None,
    }
}

/// Bit depth and colour model, read from the header.
fn colour_model(b: &[u8], format: &str) -> Option<String> {
    match format {
        "PNG" => {
            let depth = *b.get(24)?;
            let kind = match *b.get(25)? {
                0 => "greyscale",
                2 => "RGB",
                3 => "palette",
                4 => "greyscale + alpha",
                6 => "RGBA",
                _ => return None,
            };
            Some(format!("{kind}, {depth}-bit"))
        }
        "BMP" => Some(format!(
            "{}-bit",
            u16::from_le_bytes([*b.get(28)?, *b.get(29)?])
        )),
        "JPEG" => {
            let mut at = 2;
            while at + 4 <= b.len() && b[at] == 0xFF {
                let marker = b[at + 1];
                let len = u16::from_be_bytes([b[at + 2], b[at + 3]]) as usize;
                if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
                    let comps = *b.get(at + 9)?;
                    let kind = match comps {
                        1 => "greyscale",
                        3 => "YCbCr",
                        4 => "CMYK",
                        _ => "unknown",
                    };
                    let kind = if marker == 0xC2 {
                        format!("{kind}, progressive")
                    } else {
                        kind.to_string()
                    };
                    return Some(format!("{kind}, {}-bit", b.get(at + 4)?));
                }
                at += 2 + len;
            }
            None
        }
        _ => None,
    }
}

/// The root `<svg>` attributes worth showing: declared size, viewBox and title.
fn svg_details(b: &[u8], out: &mut Map<String, Json>) {
    let text = String::from_utf8_lossy(&b[..b.len().min(64 * 1024)]);
    let Some(start) = text.find("<svg") else {
        return;
    };
    let Some(end) = text[start..].find('>') else {
        return;
    };
    let tag = &text[start..start + end];
    let attr = |name: &str| {
        let pat = format!(" {name}=");
        let i = tag.find(&pat)? + pat.len();
        let q = tag[i..].chars().next()?;
        let rest = &tag[i + 1..];
        Some(rest[..rest.find(q)?].to_string())
    };
    for (key, name) in [("svg_width", "width"), ("svg_height", "height")] {
        if let Some(v) = attr(name) {
            out.insert(key.into(), json!(v));
        }
    }
    if let Some(v) = attr("viewBox") {
        out.insert("svg_viewbox".into(), json!(v));
    }
    if let (Some(a), Some(z)) = (text.find("<title>"), text.find("</title>")) {
        if a + 7 <= z {
            out.insert("svg_title".into(), json!(text[a + 7..z].trim()));
        }
    }
}

/// Describes a stored picture and how it reads at the working resolution (the longest side, in pixels).
pub fn describe(bytes: &[u8], resolution: u32) -> Result<Json, String> {
    let format = format_of(bytes);
    let (width, height) = svg::size(bytes)?;
    let mut out = Map::new();
    out.insert("format".into(), json!(format));
    out.insert("bytes".into(), json!(bytes.len()));
    out.insert("width".into(), json!(width));
    out.insert("height".into(), json!(height));
    out.insert(
        "megapixels".into(),
        json!((width as f64 * height as f64 / 1e5).round() / 10.0),
    );
    out.insert(
        "aspect".into(),
        json!((width as f64 / height as f64 * 1000.0).round() / 1000.0),
    );
    if let Some(m) = colour_model(bytes, format) {
        out.insert("colour_model".into(), json!(m));
    }
    if let Some((x, y)) = dpi(bytes, format) {
        out.insert("dpi".into(), json!([x.round(), y.round()]));
        out.insert(
            "print_size_mm".into(),
            json!([
                (width as f64 / x * MM_PER_INCH).round(),
                (height as f64 / y * MM_PER_INCH).round()
            ]),
        );
    }
    if format == "SVG" {
        svg_details(bytes, &mut out);
    }

    // how the pipeline will see it: resized to the working resolution, transparency on white
    let img = decode::decode_rgb(bytes, resolution)?;
    let n = img.w * img.h;
    let mut hist = [0usize; 256];
    let mut colours = std::collections::HashSet::new();
    let mut sum = 0u64;
    for px in img.rgb.chunks_exact(3) {
        let g = ((299 * px[0] as u32 + 587 * px[1] as u32 + 114 * px[2] as u32) / 1000) as usize;
        hist[g] += 1;
        sum += g as u64;
        if colours.len() <= 4096 {
            colours.insert([px[0] >> 3, px[1] >> 3, px[2] >> 3]);
        }
    }
    let gray: Vec<u8> = img
        .rgb
        .chunks_exact(3)
        .map(|px| ((299 * px[0] as u32 + 587 * px[1] as u32 + 114 * px[2] as u32) / 1000) as u8)
        .collect();
    let otsu = decode::otsu(&gray);
    let dark = hist[..=otsu as usize].iter().sum::<usize>();
    let (lo, hi) = (
        hist.iter().position(|&c| c > 0).unwrap_or(0),
        hist.iter().rposition(|&c| c > 0).unwrap_or(0),
    );
    out.insert("working".into(), json!([img.w, img.h]));
    out.insert(
        "scale".into(),
        json!((img.w as f64 / width as f64 * 1000.0).round() / 1000.0),
    );
    out.insert("mean_brightness".into(), json!(sum / n.max(1) as u64));
    out.insert("brightness_range".into(), json!([lo, hi]));
    out.insert("otsu_threshold".into(), json!(otsu));
    out.insert(
        "dark_percent".into(),
        json!((dark as f64 * 1000.0 / n.max(1) as f64).round() / 10.0),
    );
    out.insert(
        "colours".into(),
        json!(if colours.len() > 4096 {
            4097
        } else {
            colours.len()
        }),
    );
    Ok(Json::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(phys: bool) -> Vec<u8> {
        let img = image::RgbImage::from_fn(40, 20, |x, _| {
            image::Rgb(if x % 7 < 3 {
                [0, 0, 0]
            } else {
                [255, 255, 255]
            })
        });
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        let mut b = out.into_inner();
        if phys {
            // a pHYs chunk of 11811 pixels per metre (300 dpi) right after IHDR
            let mut body = b"pHYs".to_vec();
            body.extend(11811u32.to_be_bytes());
            body.extend(11811u32.to_be_bytes());
            body.push(1);
            let mut chunk = 9u32.to_be_bytes().to_vec();
            chunk.extend(&body);
            chunk.extend(crate::zip::crc32(&body).to_be_bytes());
            b.splice(33..33, chunk);
        }
        b
    }

    #[test]
    fn describes_a_png_with_its_resolution() {
        let d = describe(&png(true), 100).unwrap();
        assert_eq!(d["format"], "PNG");
        assert_eq!(d["width"], 40);
        assert_eq!(d["colour_model"], "RGB, 8-bit");
        assert_eq!(d["dpi"], json!([300.0, 300.0]));
        assert_eq!(d["working"], json!([100, 50]));
        assert!(d["dark_percent"].as_f64().unwrap() > 20.0);
    }

    #[test]
    fn a_png_without_physical_size_has_no_dpi() {
        let d = describe(&png(false), 40).unwrap();
        assert!(d.get("dpi").is_none());
        assert_eq!(d["scale"], 1.0);
    }

    #[test]
    fn describes_an_svg() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="20mm" viewBox="0 0 40 20"><title>Ring</title><rect width="20" height="20"/></svg>"#;
        let d = describe(svg, 100).unwrap();
        assert_eq!(d["format"], "SVG");
        assert_eq!(d["svg_viewbox"], "0 0 40 20");
        assert_eq!(d["svg_title"], "Ring");
        assert!(d.get("dpi").is_none());
    }
}
