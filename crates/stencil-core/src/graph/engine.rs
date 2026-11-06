//! Engine state and registered image, model and font assets.

use super::{cache::hash_hex, mesh_import, mesh_repair, types::Outputs};
use serde_json::{json, Value as Json};
use std::{
    collections::{HashMap, VecDeque},
    rc::Rc,
};

const MAX_IMAGE_BYTES: usize = 100 << 20;
const MAX_IMAGE_PIXELS: u64 = 100_000_000;
const MAX_FONT_BYTES: usize = 50_000_000;

#[derive(Default)]
pub struct Engine {
    pub(super) uploads: HashMap<String, Vec<u8>>,
    pub(super) fonts: HashMap<String, Rc<Vec<u8>>>,
    pub(super) cache: HashMap<String, Rc<Outputs>>,
    pub(super) order: VecDeque<String>,
    pub(super) fresh: Vec<(String, String)>,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a picture and returns `{id, width, height}`; the id is a hash of the bytes.
    pub fn add_image(&mut self, bytes: Vec<u8>) -> Result<Json, String> {
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err("the picture file is larger than 100 MB".into());
        }
        let (width, height) = crate::svg::size(&bytes)?;
        if (width as u64) * (height as u64) > MAX_IMAGE_PIXELS {
            return Err("the picture is larger than 100 megapixels".into());
        }
        let id = hash_hex(&[&bytes]);
        let out = json!({"id": id, "width": width, "height": height});
        self.uploads.insert(id, bytes);
        Ok(out)
    }

    /// Registers a parseable STL or triangulated OBJ, including meshes awaiting repair.
    pub fn add_model(&mut self, bytes: Vec<u8>, format: &str) -> Result<Json, String> {
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err("the model file is larger than 100 MB".into());
        }
        mesh_import::parse(&bytes, format)?;
        let id = hash_hex(&[&bytes, format.as_bytes()]);
        let out = json!({"id": id});
        self.uploads.insert(id, bytes);
        Ok(out)
    }

    pub fn model_info(&self, id: &str, format: &str) -> Result<Json, String> {
        let bytes = self
            .uploads
            .get(id)
            .ok_or("the model is no longer loaded: open it again")?;
        let mut info = mesh_repair::inspect(&mesh_import::parse(bytes, format)?);
        info["bytes"] = json!(bytes.len());
        info["format"] = json!(format);
        Ok(info)
    }

    pub fn model_preview(&self, id: &str, format: &str) -> Result<Vec<u8>, String> {
        let bytes = self
            .uploads
            .get(id)
            .ok_or("the model is no longer loaded: open it again")?;
        Ok(mesh_import::parse(bytes, format)?.to_stl())
    }

    pub fn repair_model(
        &self,
        id: &str,
        format: &str,
        action: &str,
        tolerance: f64,
    ) -> Result<Vec<u8>, String> {
        let bytes = self
            .uploads
            .get(id)
            .ok_or("the model is no longer loaded: open it again")?;
        Ok(mesh_repair::repair(mesh_import::parse(bytes, format)?, action, tolerance)?.to_stl())
    }

    /// Registers a font file under a name the Text node can ask for (the studio offers the system's fonts this way).
    pub fn add_font(&mut self, name: &str, bytes: Vec<u8>) -> Result<(), String> {
        if bytes.len() > MAX_FONT_BYTES {
            return Err("the font file is larger than 50 MB".into());
        }
        crate::text::check_font(&bytes)?;
        self.fonts.insert(name.to_string(), Rc::new(bytes));
        Ok(())
    }

    pub fn has_font(&self, name: &str) -> bool {
        self.fonts.contains_key(name)
    }

    /// Properties of a stored picture, and how it reads at the working resolution.
    pub fn image_info(&self, id: &str, resolution: u32) -> Result<Json, String> {
        let bytes = self
            .uploads
            .get(id)
            .ok_or("the image is no longer loaded")?;
        crate::imageinfo::describe(bytes, resolution.clamp(20, 4000))
    }

    pub fn has_image(&self, id: &str) -> bool {
        self.uploads.contains_key(id)
    }
}
