//! Picture decoding: transparency on white, resize to the working resolution, Otsu threshold.

use crate::{graph, svg};

pub(crate) fn decode_gray(
    bytes: &[u8],
    resolution: u32,
) -> Result<(Vec<u8>, usize, usize), String> {
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
    for (t, &count) in hist.iter().enumerate() {
        wb += count as f64;
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
