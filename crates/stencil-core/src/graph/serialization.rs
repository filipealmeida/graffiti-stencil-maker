//! Output encoding, metadata and downloadable parts.

use super::Value;
use serde_json::{json, Map, Value as Json};

impl Value {
    /// The bytes a browser or file can use, with their MIME type.
    pub fn encode(&self) -> Result<(&'static str, Vec<u8>), String> {
        match self {
            Value::Image(i) => Ok(("image/png", png(i.w, i.h, image::ColorType::Rgb8, &i.rgb)?)),
            Value::Mask(m) => {
                let px: Vec<u8> = m.paint.iter().map(|&p| if p { 0 } else { 255 }).collect();
                Ok(("image/png", png(m.w, m.h, image::ColorType::L8, &px)?))
            }
            Value::Shapes(s) => Ok(("image/svg+xml", s.to_svg().into_bytes())),
            Value::Solid(s) => Ok(("model/stl", s.stl.clone())),
            Value::Tiles(l) => match l.preview_svg() {
                Some(svg) => Ok(("image/svg+xml", svg.into_bytes())),
                None => self.encode_png(),
            },
            Value::Layer(l) => l.art.value().encode(),
            Value::Cuts(c) => Ok(("application/json", c.info().to_string().into_bytes())),
            Value::Layout(j) => Ok(("application/json", j.to_string().into_bytes())),
            Value::Parts(p) => match p.files.as_slice() {
                [(_, b)] => Ok(("model/stl", b.clone())),
                files => Ok(("application/zip", crate::zip::zip(files))),
            },
        }
    }

    /// The tile plan as a picture, always a PNG (a plan made from outlines is a drawing in `encode`).
    pub fn encode_png(&self) -> Result<(&'static str, Vec<u8>), String> {
        match self {
            Value::Tiles(l) => {
                let (w, h, rgb) = l.preview();
                Ok(("image/png", png(w, h, image::ColorType::Rgb8, &rgb)?))
            }
            _ => self.encode(),
        }
    }

    /// The outlines as a DXF file; `None` for anything but outlines.
    pub fn dxf(&self) -> Option<Vec<u8>> {
        match self {
            Value::Shapes(s) => Some(s.to_dxf().into_bytes()),
            _ => None,
        }
    }

    pub fn info(&self) -> Json {
        match self {
            Value::Image(i) => json!({"kind": "image", "width": i.w, "height": i.h}),
            Value::Mask(m) => {
                let cov =
                    m.paint.iter().filter(|&&p| p).count() as f64 / m.paint.len().max(1) as f64;
                json!({"kind": "mask", "width": m.w, "height": m.h, "coverage": (cov * 1e4).round() / 1e4})
            }
            Value::Shapes(s) => {
                json!({"kind": "shapes", "width": s.w, "height": s.h, "loops": s.loops.len(), "segments": s.segment_count()})
            }
            Value::Solid(s) => {
                json!({"kind": "solid", "stats": s.stats, "bytes": s.stl.len(), "tiles": []})
            }
            Value::Cuts(c) => c.info(),
            Value::Layout(j) => {
                let mut o = (**j).clone();
                o["kind"] = json!("layout");
                o
            }
            Value::Layer(l) => {
                let (w, h) = l.art.size();
                json!({"kind": "layer", "width": (w * l.sx).round(), "height": (h * l.sy).round(), "x": l.x, "y": l.y, "scale_x": l.sx, "scale_y": l.sy})
            }
            Value::Tiles(l) => {
                let pocket_list: Vec<Json> = l
                    .pockets
                    .iter()
                    .zip(&l.pocket_auto_u)
                    .map(|(p, auto)| {
                        let (lo, hi) = l.pocket_range(p);
                        json!({"key": p.key(), "axis": p.axis, "seam": p.seam, "s": p.s, "u": p.u, "auto_u": auto,
                            "block": p.block(&l.spec), "range": [lo, hi]})
                    })
                    .collect();
                json!({
                    "kind": "tiles", "width": l.width, "height": l.height,
                    "grid": [l.xs.len() - 1, l.ys.len() - 1], "pockets": l.pockets.len(),
                    "paint_under_blocks_mm2": (l.paint_under_blocks_mm2() * 10.0).round() / 10.0,
                    "paint_mm2": (l.paint_mm2() * 10.0).round() / 10.0,
                    "xs": l.xs, "ys": l.ys, "auto_xs": l.auto_xs, "auto_ys": l.auto_ys,
                    "pocket_list": pocket_list,
                    "min_tile": crate::tiles::min_tile(l.spec.depth),
                    "max": [l.max_x, l.max_y], "gap": 1.5, "quantum": l.quantum,
                    "edits_ignored": l.edits_ignored,
                    "vector": matches!(l.source, crate::tile_first::Source::Vector(_)),
                    "tile_list": l.tile_list(),
                    "tile_files": l.tile_rects().iter().flat_map(|t| [format!("{}.svg", t.name), format!("{}.dxf", t.name)]).collect::<Vec<_>>(),
                })
            }
            Value::Parts(p) => {
                let files: Map<String, Json> = p
                    .files
                    .iter()
                    .map(|(n, b)| (n.clone(), json!(b.len())))
                    .collect();
                json!({"kind": "parts", "files": files, "meta": p.meta})
            }
        }
    }

    pub fn part(&self, name: &str) -> Option<Vec<u8>> {
        match self {
            Value::Parts(p) => p
                .files
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, b)| b.clone()),
            Value::Solid(s) if name == "plate" => Some(s.stl.clone()),
            Value::Tiles(l) => l.tile_file(name),
            Value::Cuts(c) => c.file(name),
            _ => None,
        }
    }
}

fn png(w: usize, h: usize, color: image::ColorType, data: &[u8]) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(data, w as u32, h as u32, color.into())
        .map_err(|e| format!("cannot encode PNG: {e}"))?;
    Ok(out)
}
