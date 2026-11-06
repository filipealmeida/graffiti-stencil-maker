//! Graph values and the internal node execution contract.

use super::{Engine, P};
use serde_json::Value as Json;
use std::{collections::HashMap, rc::Rc};

pub struct RgbImage {
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<u8>,
}

/// `true` = paint (a hole in the stencil). Row 0 is the top, like an image.
pub struct Mask {
    pub w: usize,
    pub h: usize,
    pub paint: Vec<bool>,
}

pub struct Solid {
    pub stl: Vec<u8>,
    pub stats: Json,
}

pub struct Parts {
    pub files: Vec<(String, Vec<u8>)>,
    pub meta: Json,
}

#[derive(Clone)]
pub enum Value {
    Image(Rc<RgbImage>),
    Mask(Rc<Mask>),
    Shapes(Rc<crate::trace::Shapes>),
    Solid(Rc<Solid>),
    Parts(Rc<Parts>),
    /// A plate divided into tiles, before it is a solid.
    Tiles(Rc<crate::tile_first::Layout>),
    /// A picture, paint mask or outlines with a position and size on a canvas.
    Layer(Rc<Layer>),
    /// Where a Collage or Repeat put each layer, for the layers panel. Never a port.
    Layout(Rc<Json>),
    /// The parts of a Cut sheet, for its panel. Never a port.
    Cuts(Rc<crate::cut_sheet::Cuts>),
}

pub enum Art {
    Image(Rc<RgbImage>),
    Mask(Rc<Mask>),
    Shapes(Rc<crate::trace::Shapes>),
}

impl Art {
    /// Native size in pixels.
    pub(super) fn size(&self) -> (f64, f64) {
        match self {
            Art::Image(i) => (i.w as f64, i.h as f64),
            Art::Mask(m) => (m.w as f64, m.h as f64),
            Art::Shapes(s) => (s.w as f64, s.h as f64),
        }
    }
    pub(super) fn value(&self) -> Value {
        match self {
            Art::Image(i) => Value::Image(i.clone()),
            Art::Mask(m) => Value::Mask(m.clone()),
            Art::Shapes(s) => Value::Shapes(s.clone()),
        }
    }
}

/// Where a piece of art sits on a canvas: its top-left corner (x right, y down) and its scale, in canvas pixels.
pub struct Layer {
    pub art: Art,
    pub x: f64,
    pub y: f64,
    pub sx: f64,
    pub sy: f64,
}

impl Value {
    pub(super) fn kind(&self) -> &'static str {
        match self {
            Value::Image(_) => "image",
            Value::Mask(_) => "mask",
            Value::Shapes(_) => "shapes",
            Value::Solid(_) => "solid",
            Value::Parts(_) => "parts",
            Value::Tiles(_) => "tiles",
            Value::Layer(_) => "layer",
            Value::Layout(_) => "layout",
            Value::Cuts(_) => "cuts",
        }
    }
}

pub(super) type Outputs = Vec<(&'static str, Value)>;
pub(super) type RunFn = fn(&Engine, &HashMap<String, Value>, &P) -> Result<Outputs, String>;

pub(super) struct NodeDef {
    pub(super) id: &'static str,
    pub(super) label: &'static str,
    pub(super) stage: &'static str,
    pub(super) inputs: Vec<(&'static str, &'static str, bool)>,
    pub(super) outputs: Vec<(&'static str, &'static str)>,
    pub(super) params: Vec<Json>,
    pub(super) doc: &'static str,
    pub(super) run: RunFn,
}
