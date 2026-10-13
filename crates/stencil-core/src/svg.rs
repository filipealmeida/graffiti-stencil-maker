//! Picture loading: png/jpg/bmp through `image`, SVG rendered by resvg at the working resolution.

use image::{DynamicImage, RgbaImage};

pub fn is_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(2048)];
    let text = String::from_utf8_lossy(head);
    let t = text.trim_start_matches('\u{feff}').trim_start();
    t.starts_with("<svg")
        || t.starts_with("<?xml")
        || t.starts_with("<!DOCTYPE svg")
        || text.contains("<svg")
}

fn tree(bytes: &[u8]) -> Result<resvg::usvg::Tree, String> {
    resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default())
        .map_err(|e| format!("cannot read the SVG: {e}"))
}

/// Size of the picture in its own units (pixels for bitmaps).
pub fn size(bytes: &[u8]) -> Result<(u32, u32), String> {
    if is_svg(bytes) {
        let s = tree(bytes)?.size();
        Ok((
            (s.width().round() as u32).max(1),
            (s.height().round() as u32).max(1),
        ))
    } else {
        let i = image::load_from_memory(bytes)
            .map_err(|_| "not a readable png/jpg/bmp/svg image".to_string())?;
        Ok((i.width(), i.height()))
    }
}

/// Decodes a picture; an SVG is drawn directly at `longest` pixels on its longest side so edges stay sharp.
pub fn open(bytes: &[u8], longest: Option<u32>) -> Result<DynamicImage, String> {
    if !is_svg(bytes) {
        return image::load_from_memory(bytes).map_err(|e| format!("cannot decode image: {e}"));
    }
    let t = tree(bytes)?;
    let s = t.size();
    let k = longest
        .map(|l| l as f32 / s.width().max(s.height()))
        .unwrap_or(1.0);
    let (w, h) = (
        ((s.width() * k).round() as u32).max(1),
        ((s.height() * k).round() as u32).max(1),
    );
    let mut pix = resvg::tiny_skia::Pixmap::new(w, h).ok_or("the SVG is too large to draw")?;
    resvg::render(
        &t,
        resvg::tiny_skia::Transform::from_scale(k, k),
        &mut pix.as_mut(),
    );
    // tiny-skia stores premultiplied alpha
    let mut out = RgbaImage::new(w, h);
    for (o, p) in out.pixels_mut().zip(pix.pixels()) {
        let c = p.demultiply();
        *o = image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Ok(DynamicImage::ImageRgba8(out))
}
