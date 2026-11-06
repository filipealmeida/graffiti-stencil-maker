//! Raster/vector stencil construction and mesh simplification adapters.

use super::super::types::Outputs;
use super::super::{Engine, Parts, Solid, Value, P};
use crate::{stencil_from_paint, Params};
use serde_json::{json, Value as Json};
use std::{collections::HashMap, rc::Rc};

pub(in crate::graph) fn stencil_params_from(p: &P) -> Params {
    Params {
        width_mm: p.f("width_mm"),
        thickness_mm: p.f("thickness_mm"),
        margin_mm: p.f("margin_mm"),
        // islands are joined by a Bridges node
        bridging: false,
        flip: p.b("flip"),
        layer_height_mm: p.f("layer_height_mm"),
        extra_top_layers: p.f("extra_top_layers") as u32,
        max_overhang_deg: p.f("max_overhang_deg"),
        min_island_mm2: p.f("min_island_mm2"),
        min_contact_mm: p.f("min_contact_mm"),
        min_first_layer_width_mm: p.f("min_first_layer_width_mm"),
        first_layer_taper_mm: p.f("first_layer_taper_mm"),
        ..Params::default()
    }
}

pub(in crate::graph) fn finish_solid(o: crate::Output, p: &P) -> Result<Outputs, String> {
    if !o.stats.watertight {
        return Err("verification failed: the stencil is not watertight".into());
    }
    let mut stats: Json = serde_json::from_str(&o.stats.to_json()).map_err(|e| e.to_string())?;
    stats["margin_mm"] = json!(p.f("margin_mm"));
    stats["flip"] = json!(p.b("flip"));
    stats["thickness_mm"] = json!(p.f("thickness_mm"));
    stats["layer_height_mm"] = json!(p.f("layer_height_mm"));
    stats["plate_top_mm"] = json!(o.stats.thickness_mm);
    stats["min_first_layer_width_mm"] = json!(p.f("min_first_layer_width_mm"));
    stats["first_layer_taper_mm"] = json!(p.f("first_layer_taper_mm"));
    if let Some(f) = &o.fallback {
        stats["fallback"] = json!(f);
    }
    Ok(vec![(
        "solid",
        Value::Solid(Rc::new(Solid { stl: o.stl, stats })),
    )])
}

pub(in crate::graph) fn run_vector_stencil(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let q = stencil_params_from(p);
    if let Some(Value::Tiles(l)) = i.get("tiles") {
        let q = crate::Params {
            width_mm: l.width,
            margin_mm: l.margin,
            frame_largest: true,
            ..q
        };
        let detail = p.f("detail");
        let method = if p.s("tile_method") == "cut_whole" {
            crate::tile_first::Method::Cut(&q, detail)
        } else {
            crate::tile_first::Method::Vector(&q, detail)
        };
        return tiled_parts(l, method, p);
    }
    let Some(Value::Shapes(s)) = i.get("shapes") else {
        return Err(
            "nothing is connected to the 'outlines' input: connect outlines, or a tile plan".into(),
        );
    };
    finish_solid(crate::vector_stencil::build(s, p.f("detail"), &q)?, p)
}

pub(in crate::graph) fn run_stencil(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    if let Some(Value::Tiles(l)) = i.get("tiles") {
        return tiled_parts(
            l,
            crate::tile_first::Method::Raster(&stencil_params_from(p)),
            p,
        );
    }
    let Some(Value::Mask(m)) = i.get("mask") else {
        return Err(
            "nothing is connected to the 'paint' input: connect a paint mask, or a tile plan"
                .into(),
        );
    };
    finish_solid(
        stencil_from_paint(&m.paint, m.w, m.h, 0, &stencil_params_from(p))?,
        p,
    )
}

pub(in crate::graph) fn run_remesh(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    let Some(Value::Solid(s)) = i.get("solid") else {
        return Err("nothing is connected to the 'solid' input: connect a solid".into());
    };
    let lh = s.stats["layer_height_mm"]
        .as_f64()
        .filter(|&h| h > 0.0)
        .unwrap_or(0.2);
    let q = crate::remesh::Params {
        layer_height: lh,
        from: p.f("layer_from") as u32,
        to: p.f("layer_to") as u32,
        tolerance_mm: p.f("tolerance_mm"),
        max_overhang_deg: p.f("max_overhang_deg"),
        keep_material: p.b("keep_material"),
        smoothing: p.f("smoothing"),
        decimate_mm: if p.b("decimate") {
            p.f("decimate_mm")
        } else {
            0.0
        },
    };
    let (stl, r) = crate::remesh::reduce(&s.stl, &q)?;
    let mut stats = s.stats.clone();
    stats["triangles"] = json!(r.triangles_after);
    stats["stl_bytes"] = json!(stl.len());
    stats["remesh"] = json!({
        "skipped": r.skipped,
        "layer_height_mm": lh,
        "layers": r.layers,
        "from": r.from,
        "to": r.to,
        "vertices_before": r.vertices_before,
        "vertices_after": r.vertices_after,
        "triangles_before": r.triangles_before,
        "triangles_after": r.triangles_after,
        "max_deviation_mm": r.max_deviation_mm,
        "tolerance_used_mm": r.tolerance_mm,
        "overhang_violation_mm2": r.overhang_violation_mm2,
        "triangles_after_layers": r.triangles_layers,
        "decimate_mm": r.decimate_mm,
    });
    Ok(vec![("solid", Value::Solid(Rc::new(Solid { stl, stats })))])
}

/// Builds every tile of the layout with the stencil `method` and describes the plate the way a Tiles node does, so
/// Pads and Screws can follow.
fn tiled_parts(
    l: &crate::tile_first::Layout,
    method: crate::tile_first::Method,
    p: &P,
) -> Result<Outputs, String> {
    let b = crate::tile_first::build(l, &method)?;
    crate::progress::step("build tiles", 0.9);
    let mut files = Vec::new();
    let mut parts = Vec::new();
    for t in &b.tiles {
        let file = format!("{}.stl", t.name);
        parts.push(json!({"file": file, "role": "tile", "origin": t.origin, "size": t.size}));
        files.push((file, t.mesh.to_stl()));
    }
    if !l.pockets.is_empty() {
        let c = &l.spec;
        files.push((
            format!(
                "connector_{}_d{}mm.stl",
                if c.shape == crate::tiles::Shape::Round {
                    "round"
                } else {
                    "hex"
                },
                c.diameter
            ),
            crate::tiles::connector_rod_with_gap(
                c.shape, c.diameter, c.depth, c.seam_gap, c.segments,
            )
            .to_stl(),
        ));
    }
    let meta = json!({
        "tiling": b.info,
        "plate": {
            "w": l.width, "h": l.height, "thickness": p.f("thickness_mm"), "layer": b.layer,
            "top": b.top, "flip": p.b("flip"), "margin": l.margin,
            "seams": b.seams, "connectors": b.connectors, "parts": parts,
        },
    });
    Ok(vec![(
        "parts",
        Value::Parts(Rc::new(Parts { files, meta })),
    )])
}
