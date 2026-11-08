//! Typed node graph shared by the studio and CLI.
//!
//! Engine state, validation, parameters, execution, caching and serialization are
//! independent of the node adapters registered by `catalogue`.

use serde_json::{json, Value as Json};
use std::{collections::HashMap, rc::Rc};
use types::Outputs;

mod cache;
mod engine;
mod execution;
mod params;
mod reporting;
mod serialization;
mod types;
mod validation;

mod bridges;
mod canvas;
mod catalogue;
mod colour_set;
mod fragments;
mod mesh_import;
mod mesh_repair;
mod migrate;
mod nodes;
mod roll_up;

pub use catalogue::node_types;
pub use engine::Engine;
pub use params::P;
pub use types::{Art, Layer, Mask, Parts, RgbImage, Solid, Value};

/// Brings a graph saved by an older version up to the current node set.
pub fn migrate_graph(graph: &Json) -> Json {
    migrate::migrate(graph)
}

#[cfg(test)]
mod tests;
