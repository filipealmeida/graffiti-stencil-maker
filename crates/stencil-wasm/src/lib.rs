use stencil_core::{make_stencil, Params, ZBridging};
use wasm_bindgen::prelude::*;

#[cfg(feature = "threads")]
pub use wasm_bindgen_rayon::init_thread_pool;

/// A per-thread cache of small blocks in front of the system allocator. The threaded build shares one dlmalloc
/// behind a global lock, which serialises the allocation-heavy geometry work across the worker pool; most of that
/// churn is alloc/free of small buffers, which this layer serves from thread-local free lists without the lock.
#[cfg(feature = "threads")]
mod magazine {
    use core::cell::RefCell;
    use std::alloc::{GlobalAlloc, Layout, System};

    /// Size granularity and the alignment every cached block has.
    const CLASS: usize = 16;
    /// Blocks up to CLASS * CLASSES bytes are cached.
    const CLASSES: usize = 256;
    /// Kept blocks per class and thread; beyond that they go back to the system allocator.
    const PER: usize = 16;

    struct Bins {
        len: [u8; CLASSES],
        slot: [[*mut u8; PER]; CLASSES],
    }

    thread_local! {
        static BINS: RefCell<Bins> = const {
            RefCell::new(Bins { len: [0; CLASSES], slot: [[core::ptr::null_mut(); PER]; CLASSES] })
        };
    }

    pub struct Magazine;

    /// The rounded layout a cached class is really allocated with.
    #[inline]
    fn rounded(class: usize) -> Layout {
        unsafe { Layout::from_size_align_unchecked((class + 1) * CLASS, CLASS) }
    }

    #[inline]
    fn class_of(l: Layout) -> Option<usize> {
        (l.align() <= CLASS && l.size() > 0 && l.size() <= CLASS * CLASSES)
            .then(|| (l.size() - 1) / CLASS)
    }

    unsafe impl GlobalAlloc for Magazine {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            if let Some(c) = class_of(l) {
                let hit = BINS
                    .try_with(|b| {
                        let mut b = b.borrow_mut();
                        let n = b.len[c] as usize;
                        if n > 0 {
                            b.len[c] = (n - 1) as u8;
                            Some(b.slot[c][n - 1])
                        } else {
                            None
                        }
                    })
                    .unwrap_or(None);
                return hit.unwrap_or_else(|| System.alloc(rounded(c)));
            }
            System.alloc(l)
        }

        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            if let Some(c) = class_of(l) {
                let kept = BINS
                    .try_with(|b| {
                        let mut b = b.borrow_mut();
                        let n = b.len[c] as usize;
                        if n < PER {
                            b.slot[c][n] = p;
                            b.len[c] = (n + 1) as u8;
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);
                if !kept {
                    System.dealloc(p, rounded(c));
                }
                return;
            }
            System.dealloc(p, l);
        }
    }
}

#[cfg(feature = "threads")]
#[global_allocator]
static ALLOC: magazine::Magazine = magazine::Magazine;

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

/// Creates a stencil from a JSON object using `stencil_core::Params` field names.
/// Omitted fields use the core parameter defaults.
#[wasm_bindgen]
pub fn make_with_options(bytes: &[u8], options_json: &str) -> Result<StencilResult, JsError> {
    let options: serde_json::Value =
        serde_json::from_str(options_json).map_err(|e| JsError::new(&e.to_string()))?;
    let params = params_from_options(&options)?;
    make_with_params(bytes, &params)
}

/// Compatibility wrapper for callers of the original positional API.
#[allow(deprecated)]
#[wasm_bindgen]
#[deprecated(
    note = "use make_with_options with a JSON options object, or Engine::run with a graph"
)]
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
        bridge_min_angle_deg: 0.0,
        bridge_max_length: 4.0,
        bridge_passes: 4,
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
        min_first_layer_width_mm: 0.0,
        first_layer_taper_mm: 1.0,
        max_overhang_deg,
        frame_largest: false,
        bridging: true,
    };
    let options = serde_json::json!({
        "width_mm": p.width_mm,
        "thickness_mm": p.thickness_mm,
        "margin_mm": p.margin_mm,
        "bridge_mm": p.bridge_mm,
        "bridges_per_island": p.bridges_per_island,
        "bridge_min_angle_deg": p.bridge_min_angle_deg,
        "bridge_max_length": p.bridge_max_length,
        "bridge_passes": p.bridge_passes,
        "resolution": p.resolution,
        "threshold": p.threshold,
        "invert": p.invert,
        "layer_height_mm": p.layer_height_mm,
        "raised_bridges": p.raised_bridges,
        "z_bridging": match p.z_bridging {
            ZBridging::Easy => "easy",
            ZBridging::Steps => "steps",
        },
        "extra_top_layers": p.extra_top_layers,
        "flip": p.flip,
        "min_island_mm2": p.min_island_mm2,
        "min_contact_mm": p.min_contact_mm,
        "min_first_layer_width_mm": p.min_first_layer_width_mm,
        "first_layer_taper_mm": p.first_layer_taper_mm,
        "max_overhang_deg": p.max_overhang_deg,
        "frame_largest": p.frame_largest,
        "bridging": p.bridging,
    });
    make_with_options(bytes, &options.to_string())
}

fn make_with_params(bytes: &[u8], params: &Params) -> Result<StencilResult, JsError> {
    let out = make_stencil(bytes, params).map_err(|e| JsError::new(&e))?;
    Ok(StencilResult {
        stl: out.stl,
        stats: out.stats.to_json(),
    })
}

fn params_from_options(options: &serde_json::Value) -> Result<Params, JsError> {
    if !options.is_object() {
        return Err(JsError::new("options must be a JSON object"));
    }

    let mut params = Params::default();
    macro_rules! number {
        ($field:ident) => {
            if let Some(value) = options.get(stringify!($field)) {
                params.$field = value.as_f64().ok_or_else(|| {
                    JsError::new(concat!(stringify!($field), " must be a number"))
                })?;
            }
        };
    }
    macro_rules! boolean {
        ($field:ident) => {
            if let Some(value) = options.get(stringify!($field)) {
                params.$field = value.as_bool().ok_or_else(|| {
                    JsError::new(concat!(stringify!($field), " must be a boolean"))
                })?;
            }
        };
    }
    macro_rules! integer {
        ($field:ident) => {
            if let Some(value) = options.get(stringify!($field)) {
                params.$field =
                    value
                        .as_u64()
                        .and_then(|n| n.try_into().ok())
                        .ok_or_else(|| {
                            JsError::new(concat!(stringify!($field), " must be an integer"))
                        })?;
            }
        };
    }

    number!(width_mm);
    number!(thickness_mm);
    number!(margin_mm);
    number!(bridge_mm);
    integer!(bridges_per_island);
    number!(bridge_min_angle_deg);
    number!(bridge_max_length);
    integer!(bridge_passes);
    integer!(resolution);
    if let Some(value) = options.get("threshold") {
        params.threshold = if value.is_null() {
            None
        } else {
            Some(
                value
                    .as_u64()
                    .and_then(|n| n.try_into().ok())
                    .ok_or_else(|| {
                        JsError::new("threshold must be an integer from 0 to 255 or null")
                    })?,
            )
        };
    }
    boolean!(invert);
    number!(layer_height_mm);
    boolean!(raised_bridges);
    if let Some(value) = options.get("z_bridging") {
        params.z_bridging = match value.as_str() {
            Some("easy") => ZBridging::Easy,
            Some("steps") => ZBridging::Steps,
            _ => return Err(JsError::new("z_bridging must be 'easy' or 'steps'")),
        };
    }
    integer!(extra_top_layers);
    boolean!(flip);
    number!(min_island_mm2);
    number!(min_contact_mm);
    number!(min_first_layer_width_mm);
    number!(first_layer_taper_mm);
    number!(max_overhang_deg);
    boolean!(frame_largest);
    boolean!(bridging);

    Ok(params)
}

thread_local! {
    static LAST_PANIC: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn console_error(msg: &str);
}

/// Remembers why the engine panicked, so the page can show the reason instead of a bare "unreachable".
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = info.to_string();
        console_error(&msg);
        LAST_PANIC.with(|p| *p.borrow_mut() = msg);
    }));
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
        install_panic_hook();
        Engine {
            inner: stencil_core::graph::Engine::new(),
            progress: None,
        }
    }

    /// The message of the last panic, or an empty string. After a panic the module must be discarded.
    pub fn last_panic(&self) -> String {
        LAST_PANIC.with(|p| p.borrow().clone())
    }

    /// The version of the engine this module was built from.
    pub fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
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

    pub fn add_font(&mut self, name: &str, bytes: Vec<u8>) -> Result<(), JsError> {
        self.inner
            .add_font(name, bytes)
            .map_err(|e| JsError::new(&e))
    }

    pub fn add_model(&mut self, bytes: Vec<u8>, format: &str) -> Result<String, JsError> {
        self.inner
            .add_model(bytes, format)
            .map(|j| j.to_string())
            .map_err(|e| JsError::new(&e))
    }

    pub fn has_font(&self, name: &str) -> bool {
        self.inner.has_font(name)
    }

    pub fn model_info(&self, id: &str, format: &str) -> Result<String, JsError> {
        self.inner
            .model_info(id, format)
            .map(|j| j.to_string())
            .map_err(|e| JsError::new(&e))
    }

    pub fn model_preview(&self, id: &str, format: &str) -> Result<Vec<u8>, JsError> {
        self.inner
            .model_preview(id, format)
            .map_err(|e| JsError::new(&e))
    }

    pub fn repair_model(
        &self,
        id: &str,
        format: &str,
        action: &str,
        tolerance: f64,
    ) -> Result<Vec<u8>, JsError> {
        self.inner
            .repair_model(id, format, action, tolerance)
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

    /// The browser's clock and time zone, for the timestamps inside downloaded ZIPs.
    fn stamp() {
        let d = js_sys::Date::new_0();
        stencil_core::zip::set_clock(
            d.get_time() as u64,
            (-d.get_timezone_offset() * 60.0) as i64,
        );
    }

    pub fn mime(&self, key: &str, out: &str) -> String {
        self.inner
            .value(key, out)
            .and_then(|v| {
                Self::stamp();
                v.encode().ok()
            })
            .map(|(m, _)| m.to_string())
            .unwrap_or_default()
    }

    pub fn artifact(&self, key: &str, out: &str) -> Result<Vec<u8>, JsError> {
        let v = self
            .inner
            .value(key, out)
            .ok_or_else(|| JsError::new("artifact not found"))?;
        Self::stamp();
        v.encode().map(|(_, b)| b).map_err(|e| JsError::new(&e))
    }

    /// The outlines of a result as DXF.
    pub fn dxf(&self, key: &str, out: &str) -> Result<Vec<u8>, JsError> {
        self.inner
            .value(key, out)
            .and_then(|v| v.dxf())
            .ok_or_else(|| JsError::new("no outlines to export"))
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
