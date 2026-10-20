use stencil_core::{make_stencil, Params, ZBridging};
use wasm_bindgen::prelude::*;

#[cfg(feature = "threads")]
pub use wasm_bindgen_rayon::init_thread_pool;

#[wasm_bindgen]
pub struct StencilResult {
    stl: Vec<u8>,
    stats: String,
}

#[wasm_bindgen]
impl StencilResult {
    #[wasm_bindgen(getter)]
    pub fn stl(&self) -> Vec<u8> {
        self.stl.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn stats(&self) -> String {
        self.stats.clone()
    }
}

#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn make(
    bytes: &[u8],
    width_mm: f64,
    thickness_mm: f64,
    margin_mm: f64,
    bridge_mm: f64,
    resolution: u32,
    threshold: i32,
    invert: bool,
    layer_height_mm: f64,
    raised_bridges: bool,
    easy_bridging: bool,
    extra_top_layers: u32,
    flip: bool,
    min_island_mm2: f64,
    max_overhang_deg: f64,
) -> Result<StencilResult, JsError> {
    let p = Params {
        width_mm,
        thickness_mm,
        margin_mm,
        bridge_mm,
        bridges_per_island: 1,
        resolution,
        threshold: if (0..=255).contains(&threshold) {
            Some(threshold as u8)
        } else {
            None
        },
        invert,
        layer_height_mm,
        raised_bridges,
        z_bridging: if easy_bridging {
            ZBridging::Easy
        } else {
            ZBridging::Steps
        },
        extra_top_layers,
        flip,
        min_island_mm2,
        min_contact_mm: 0.0,
        max_overhang_deg,
        frame_largest: false,
    };
    let out = make_stencil(bytes, &p).map_err(|e| JsError::new(&e))?;
    Ok(StencilResult {
        stl: out.stl,
        stats: out.stats.to_json(),
    })
}

#[wasm_bindgen]
pub struct Engine {
    inner: stencil_core::graph::Engine,
    progress: Option<js_sys::Function>,
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        Engine {
            inner: stencil_core::graph::Engine::new(),
            progress: None,
        }
    }

    pub fn node_types(&self) -> String {
        stencil_core::graph::node_types().to_string()
    }

    pub fn add_image(&mut self, bytes: Vec<u8>) -> Result<String, JsError> {
        self.inner
            .add_image(bytes)
            .map(|j| j.to_string())
            .map_err(|e| JsError::new(&e))
    }

    pub fn image_info(&self, id: &str, resolution: u32) -> Result<String, JsError> {
        self.inner
            .image_info(id, resolution)
            .map(|j| j.to_string())
            .map_err(|e| JsError::new(&e))
    }

    pub fn has_image(&self, id: &str) -> bool {
        self.inner.has_image(id)
    }

    /// Installs a `(label, fraction)` callback that long nodes call between phases while `run` is working.
    pub fn set_progress(&mut self, f: Option<js_sys::Function>) {
        self.progress = f;
    }

    // targets_json is a JSON array of node ids, or an empty string for the whole graph.
    pub fn run(&mut self, graph_json: &str, targets_json: &str) -> Result<String, JsError> {
        let graph: serde_json::Value =
            serde_json::from_str(graph_json).map_err(|e| JsError::new(&e.to_string()))?;
        let targets: Option<Vec<String>> = if targets_json.is_empty() {
            None
        } else {
            Some(serde_json::from_str(targets_json).map_err(|e| JsError::new(&e.to_string()))?)
        };
        if let Some(f) = self.progress.clone() {
            stencil_core::progress::set_hook(Some(std::rc::Rc::new(move |label, frac| {
                let _ = f.call2(
                    &JsValue::NULL,
                    &JsValue::from_str(label),
                    &JsValue::from_f64(frac),
                );
            })));
        }
        let out = self
            .inner
            .run(&graph, targets.as_deref(), &js_sys::Date::now)
            .to_string();
        stencil_core::progress::set_hook(None);
        Ok(out)
    }

    pub fn mime(&self, key: &str, out: &str) -> String {
        self.inner
            .value(key, out)
            .and_then(|v| v.encode().ok())
            .map(|(m, _)| m.to_string())
            .unwrap_or_default()
    }

    pub fn artifact(&self, key: &str, out: &str) -> Result<Vec<u8>, JsError> {
        let v = self
            .inner
            .value(key, out)
            .ok_or_else(|| JsError::new("artifact not found"))?;
        v.encode().map(|(_, b)| b).map_err(|e| JsError::new(&e))
    }

    pub fn part(&self, key: &str, out: &str, name: &str) -> Result<Vec<u8>, JsError> {
        self.inner
            .value(key, out)
            .and_then(|v| v.part(name))
            .ok_or_else(|| JsError::new("part not found"))
    }

    pub fn info(&self, key: &str, out: &str) -> String {
        self.inner
            .value(key, out)
            .map(|v| v.info().to_string())
            .unwrap_or_default()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}
