//! Tile-layout and cut-sheet adapters.

use super::super::types::Outputs;
use super::super::{Engine, Value, P};
use std::{collections::HashMap, rc::Rc};

pub(in crate::graph) fn run_tile_layout(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    use crate::tile_first as tf;
    let diameter: f64 = p.s("connector_d").parse().unwrap_or(3.0);
    let lp = tf::LayoutParams {
        edits: tf::Edits::parse(p.s("edits"))?,
        width: p.f("width_mm"),
        margin: p.f("margin_mm"),
        max_x: p.f("max_x"),
        max_y: p.f("max_y"),
        seam_shift: p.f("seam_shift"),
        spec: tf::Connectors {
            shape: connector_shape(p),
            diameter,
            clearance: p.f("clearance"),
            depth: p.f("depth"),
            segments: p.f("segments") as usize,
            spacing: p.f("spacing"),
            seam_gap: p.f("seam_gap"),
        },
    };
    let layout = match (i.get("mask"), i.get("shapes")) {
        (Some(Value::Mask(m)), None) => tf::from_mask(&m.paint, m.w, m.h, &lp)?,
        (None, Some(Value::Shapes(s))) => tf::from_shapes(s, &lp)?,
        (None, None) => {
            return Err("connect a mask (for a Stencil) or shapes (for a Vector stencil)".into())
        }
        _ => return Err("connect either a mask or shapes, not both".into()),
    };
    Ok(vec![("tiles", Value::Tiles(Rc::new(layout)))])
}

/// The connector choice of a node: a hex or round rod, or none at all.
pub(super) fn connector_shape(p: &P) -> crate::tiles::Shape {
    match p.s("connector") {
        "round" => crate::tiles::Shape::Round,
        "none" => crate::tiles::Shape::None,
        _ => crate::tiles::Shape::Hex,
    }
}

pub(in crate::graph) fn run_cut_sheet(
    _: &Engine,
    i: &HashMap<String, Value>,
    p: &P,
) -> Result<Outputs, String> {
    use crate::tile_first as tf;
    let owned;
    let layout: &tf::Layout = match (i.get("tiles"), i.get("shapes")) {
        (Some(Value::Tiles(l)), None) => l,
        (None, Some(Value::Shapes(s))) => {
            owned = tf::from_shapes(
                s,
                &tf::LayoutParams {
                    edits: tf::Edits::default(),
                    width: p.f("width_mm"),
                    margin: p.f("margin_mm"),
                    max_x: 1e6,
                    max_y: 1e6,
                    seam_shift: 0.0,
                    spec: tf::Connectors {
                        shape: crate::tiles::Shape::None,
                        diameter: 3.0,
                        clearance: 0.2,
                        depth: 10.0,
                        segments: 48,
                        spacing: 30.0,
                        seam_gap: 0.0,
                    },
                },
            )?;
            &owned
        }
        (None, None) => return Err("connect a tile plan or outlines".into()),
        _ => return Err("connect either a tile plan or outlines, not both".into()),
    };
    let laser = p.s("machine") == "laser";
    let cuts = crate::cut_sheet::make(
        layout,
        &crate::cut_sheet::Params {
            laser,
            tool: p.f("tool_mm"),
            bridge: p.f("bridge_mm"),
            bridges_per_island: p.f("bridges_per_island") as usize,
            sheet_w: p.f("sheet_w"),
            sheet_h: p.f("sheet_h"),
            gap: p.f("gap_mm"),
        },
    )?;
    let sheet = Value::Shapes(Rc::new(cuts.shapes()));
    Ok(vec![("sheet", sheet), ("cuts", Value::Cuts(Rc::new(cuts)))])
}
