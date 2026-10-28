//! Typed node graph: node catalogue, validation and cached evaluation. The web studio and the CLI both run this.

use crate::colour::{self, Pattern, Tone, ToneParams};
use crate::{otsu, stencil_from_paint, Params, ZBridging};
use serde_json::{json, Map, Value as Json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

mod catalogue;
mod colour_set;
mod fragments;
mod nodes;
use catalogue::{catalogue, split_outputs, NodeDef, Outputs};
use nodes::*;
#[cfg(test)]
mod tests;

const CACHE_LIMIT: usize = 64;

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
}

impl Value {
    fn kind(&self) -> &'static str {
        match self {
            Value::Image(_) => "image",
            Value::Mask(_) => "mask",
            Value::Shapes(_) => "shapes",
            Value::Solid(_) => "solid",
            Value::Parts(_) => "parts",
            Value::Tiles(_) => "tiles",
        }
    }

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
            Value::Tiles(l) => {
                let (w, h, rgb) = l.preview();
                Ok(("image/png", png(w, h, image::ColorType::Rgb8, &rgb)?))
            }
            Value::Parts(p) => match p.files.as_slice() {
                [(_, b)] => Ok(("model/stl", b.clone())),
                files => Ok(("application/zip", crate::zip::zip(files))),
            },
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

// ---- node catalogue -----------------------------------------------------------------------------------------------

/// Parameters after defaults and clamping.
pub struct P(Map<String, Json>);

impl P {
    fn f(&self, k: &str) -> f64 {
        self.0.get(k).and_then(Json::as_f64).unwrap_or(0.0)
    }
    fn b(&self, k: &str) -> bool {
        self.0.get(k).and_then(Json::as_bool).unwrap_or(false)
    }
    fn s(&self, k: &str) -> &str {
        self.0.get(k).and_then(Json::as_str).unwrap_or("")
    }
}

fn clean_params(def: &NodeDef, given: Option<&Json>) -> P {
    let mut out = Map::new();
    for d in &def.params {
        let name = d["name"].as_str().unwrap_or_default();
        let raw = given.and_then(|g| g.get(name));
        let v = match d["kind"].as_str().unwrap_or("") {
            "int" | "float" => {
                let x = raw
                    .and_then(Json::as_f64)
                    .unwrap_or_else(|| d["default"].as_f64().unwrap_or(0.0));
                let x = x.clamp(
                    d["min"].as_f64().unwrap_or(f64::MIN),
                    d["max"].as_f64().unwrap_or(f64::MAX),
                );
                json!(if d["kind"] == "int" { x.round() } else { x })
            }
            "bool" => json!(raw
                .and_then(Json::as_bool)
                .unwrap_or_else(|| d["default"].as_bool().unwrap_or(false))),
            "choice" => {
                let ok = raw.and_then(Json::as_str).filter(|r| {
                    d["choices"]
                        .as_array()
                        .is_some_and(|c| c.iter().any(|x| x == r))
                });
                json!(ok.unwrap_or_else(|| d["default"].as_str().unwrap_or("")))
            }
            _ => json!(raw
                .and_then(Json::as_str)
                .unwrap_or_else(|| d["default"].as_str().unwrap_or(""))),
        };
        out.insert(name.to_string(), v);
    }
    P(out)
}

// ---- engine -------------------------------------------------------------------------------------------------------

const MAX_IMAGE_BYTES: usize = 100 << 20;
const MAX_IMAGE_PIXELS: u64 = 100_000_000;

pub fn node_types() -> Json {
    Json::Array(
        catalogue()
            .iter()
            .map(|d| {
                json!({
                    "id": d.id, "label": d.label, "stage": d.stage,
                    "inputs": d.inputs.iter().map(|(n, t, r)| json!({"name": n, "type": t, "required": r})).collect::<Vec<_>>(),
                    "outputs": d.outputs.iter().map(|(n, t)| json!({"name": n, "type": t})).collect::<Vec<_>>(),
                    "params": d.params, "doc": d.doc, "legacy": d.id == "export",
                })
            })
            .collect(),
    )
}

#[derive(Default)]
pub struct Engine {
    uploads: HashMap<String, Vec<u8>>,
    cache: HashMap<String, Rc<Outputs>>,
    order: VecDeque<String>,
    fresh: Vec<(String, String)>,
}

/// FNV-1a over length-prefixed parts. Unlike `DefaultHasher` it gives the same id on 32-bit wasm and 64-bit native,
/// so a project saved in the browser finds its pictures again in the CLI.
fn hash_hex(parts: &[&[u8]]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for &b in bytes {
            h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for p in parts {
        eat(&(p.len() as u64).to_le_bytes());
        eat(p);
    }
    format!("{h:016x}")
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

    pub fn value(&self, key: &str, out: &str) -> Option<&Value> {
        self.cache
            .get(key)?
            .iter()
            .find(|(n, _)| *n == out)
            .map(|(_, v)| v)
    }

    /// Outputs computed by the last run, as `(key, output)`.
    pub fn take_fresh(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.fresh)
    }

    /// Print-readiness of a finished run: the findings of every node (see `report`) and how long each node took.
    pub fn report(&self, graph: &Json, result: &Json) -> Json {
        let graph = &migrate(graph);
        let defs: HashMap<&str, NodeDef> = catalogue().into_iter().map(|d| (d.id, d)).collect();
        let mut items: Vec<Json> = Vec::new();
        let mut levels = Vec::new();
        let mut timing: Vec<Json> = Vec::new();
        let mut metrics = Map::new();
        for node in graph["nodes"].as_array().into_iter().flatten() {
            let id = node["id"].as_str().unwrap_or("");
            let ty = node["type"].as_str().unwrap_or("");
            let (Some(def), r) = (defs.get(ty), &result["nodes"][id]) else {
                continue;
            };
            if r["state"] == "error" {
                let text = r["error"].as_str().unwrap_or("failed").to_string();
                levels.push(crate::report::Level::Error);
                items.push(json!({"level": "error", "node": id, "type": ty, "label": def.label, "text": text}));
            }
            let Some(key) = r["key"].as_str() else {
                continue;
            };
            timing.push(json!({"node": id, "type": ty, "label": def.label, "ms": r["ms"], "cached": r["cached"]}));
            let infos: Vec<Json> = def
                .outputs
                .iter()
                .filter_map(|(o, _)| self.value(key, o).map(Value::info))
                .collect();
            let params = Json::Object(clean_params(def, node.get("params")).0);
            for (k, x) in crate::report::metrics(&infos) {
                let (new, old) = (
                    x.as_f64().unwrap_or(0.0),
                    metrics.get(&k).and_then(Json::as_f64),
                );
                if old.is_none_or(|o| new > o) {
                    metrics.insert(k, x);
                }
            }
            for f in crate::report::findings(ty, &params, &infos) {
                // parts carry the tiling description on through Pads and Screws: say it once, at the node that made it
                if items.iter().any(|i| i["text"] == f.text.as_str()) {
                    continue;
                }
                levels.push(f.level);
                items.push(json!({"level": f.level.name(), "node": id, "type": ty, "label": def.label, "text": f.text}));
            }
        }
        timing.sort_by(|a, b| {
            b["ms"]
                .as_f64()
                .unwrap_or(0.0)
                .total_cmp(&a["ms"].as_f64().unwrap_or(0.0))
        });
        json!({"verdict": crate::report::verdict(&levels), "findings": items, "metrics": metrics, "timing": timing})
    }

    /// Evaluates the graph, or only the ancestors of `targets`. `now` returns epoch milliseconds.
    pub fn run(&mut self, graph: &Json, targets: Option<&[String]>, now: &dyn Fn() -> f64) -> Json {
        let graph = &migrate(graph);
        let defs: HashMap<&str, NodeDef> = catalogue().into_iter().map(|d| (d.id, d)).collect();
        // `fresh` reports only what this call computed; otherwise every run would re-export all earlier results
        self.fresh.clear();
        let mut log: Vec<Json> = Vec::new();
        let t0 = now();
        let mut say = |level: &str, node: &str, ty: &str, msg: String, ms: Option<f64>| {
            log.push(json!({"at": now(), "t": ((now() - t0) / 10.0).round() / 100.0, "level": level, "node": node, "type": ty, "msg": msg, "ms": ms}));
        };

        let order = match plan(graph, &defs, targets) {
            Ok(o) => o,
            Err(e) => {
                return json!({"status": "error", "error": e, "nodes": {}, "log": log, "fresh": []})
            }
        };
        let mut nodes_json = Map::new();
        for n in graph["nodes"].as_array().into_iter().flatten() {
            nodes_json.insert(
                n["id"].as_str().unwrap_or("").to_string(),
                json!({"state": "pending"}),
            );
        }
        let feeds: HashMap<(String, String), (String, String)> = graph["edges"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| {
                let s = |v: &Json, i: usize| v.get(i).and_then(Json::as_str).map(str::to_string);
                Some((
                    (s(&e["to"], 0)?, s(&e["to"], 1)?),
                    (s(&e["from"], 0)?, s(&e["from"], 1)?),
                ))
            })
            .collect();
        let by_id: HashMap<&str, &Json> = graph["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| Some((n["id"].as_str()?, n)))
            .collect();

        let mut results: HashMap<String, Option<(String, Rc<Outputs>)>> = HashMap::new();
        for id in order {
            let node = by_id[id.as_str()];
            let ty = node["type"].as_str().unwrap_or("");
            let def = &defs[ty];
            let params = clean_params(def, node.get("params"));
            let fail = |nodes: &mut Map<String, Json>,
                        results: &mut HashMap<String, Option<(String, Rc<Outputs>)>>,
                        msg: String| {
                nodes.insert(id.clone(), json!({"state": "error", "error": msg}));
                results.insert(id.clone(), None);
            };

            let mut inputs = HashMap::new();
            let mut keys = Vec::new();
            let mut problem = None;
            for (port, _, required) in &def.inputs {
                match feeds.get(&(id.clone(), port.to_string())) {
                    None if *required => {
                        problem = Some(format!("input '{port}' is not connected"));
                        break;
                    }
                    None => {}
                    Some((src, out)) => match results.get(src) {
                        Some(Some((key, vals))) => match vals.iter().find(|(n, _)| n == out) {
                            Some((_, v)) => {
                                inputs.insert(port.to_string(), v.clone());
                                keys.push(format!("{key}:{out}"));
                            }
                            None => {
                                problem = Some(format!("'{out}' is not available from {src}"));
                                break;
                            }
                        },
                        _ => {
                            problem =
                                Some("an earlier node failed: fix that one first".to_string());
                            break;
                        }
                    },
                }
            }
            if let Some(m) = problem {
                say("error", &id, ty, m.clone(), None);
                fail(&mut nodes_json, &mut results, m);
                continue;
            }

            let pj = Json::Object(params.0.clone()).to_string();
            let key = hash_hex(&[ty.as_bytes(), pj.as_bytes(), keys.join("|").as_bytes()]);
            let cached = self.cache.get(&key).cloned();
            let t1 = now();
            let vals = match cached {
                Some(v) => Ok((v, true)),
                None => (def.run)(self, &inputs, &params).map(|v| (Rc::new(v), false)),
            };
            match vals {
                Err(msg) => {
                    say("error", &id, ty, msg.clone(), None);
                    fail(&mut nodes_json, &mut results, msg);
                }
                Ok((vals, was_cached)) => {
                    let ms = (now() - t1).round();
                    if !was_cached {
                        for (n, _) in vals.iter() {
                            self.fresh.push((key.clone(), n.to_string()));
                        }
                        self.cache.insert(key.clone(), vals.clone());
                        self.order.push_back(key.clone());
                        while self.order.len() > CACHE_LIMIT {
                            if let Some(old) = self.order.pop_front() {
                                self.cache.remove(&old);
                            }
                        }
                    }
                    say(
                        "info",
                        &id,
                        ty,
                        if was_cached {
                            "cached".into()
                        } else {
                            "done".into()
                        },
                        if was_cached { None } else { Some(ms) },
                    );
                    let outputs: Map<String, Json> = def
                        .outputs
                        .iter()
                        .map(|(n, t)| {
                            (
                                n.to_string(),
                                json!({"type": t, "available": vals.iter().any(|(k, _)| k == n)}),
                            )
                        })
                        .collect();
                    nodes_json.insert(id.clone(), json!({"state": "done", "key": key, "cached": was_cached, "ms": ms, "outputs": outputs}));
                    results.insert(id.clone(), Some((key, vals)));
                }
            }
        }
        let fresh: Vec<Json> = self
            .fresh
            .iter()
            .filter_map(|(k, o)| {
                self.value(k, o)
                    .map(|v| json!({"key": k, "out": o, "kind": v.kind()}))
            })
            .collect();
        json!({"status": "done", "nodes": nodes_json, "log": log, "fresh": fresh})
    }
}

/// Validates the graph and returns the nodes to evaluate, parents first.
/// Projects saved before a node was renamed keep working: "complexity" is now "remesh".
fn migrate(graph: &Json) -> Json {
    let mut g = graph.clone();
    if let Some(nodes) = g["nodes"].as_array_mut() {
        for n in nodes {
            if n["type"] == "complexity" {
                n["type"] = json!("remesh");
            }
        }
    }
    g
}

fn plan(
    graph: &Json,
    defs: &HashMap<&str, NodeDef>,
    targets: Option<&[String]>,
) -> Result<Vec<String>, String> {
    let nodes = graph["nodes"].as_array().ok_or("graph has no nodes list")?;
    let mut types: HashMap<&str, &str> = HashMap::new();
    for n in nodes {
        let id = n["id"].as_str().ok_or("node without id")?;
        let ty = n["type"].as_str().unwrap_or("");
        if !defs.contains_key(ty) {
            return Err(format!("unknown node type '{ty}'"));
        }
        if types.insert(id, ty).is_some() {
            return Err(format!("duplicate node id '{id}'"));
        }
    }
    let mut parents: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in graph["edges"].as_array().into_iter().flatten() {
        let (a, ao, b, bi) = (
            e["from"][0].as_str().unwrap_or(""),
            e["from"][1].as_str().unwrap_or(""),
            e["to"][0].as_str().unwrap_or(""),
            e["to"][1].as_str().unwrap_or(""),
        );
        let out = types
            .get(a)
            .and_then(|t| defs[t].outputs.iter().find(|(n, _)| *n == ao));
        let inp = types
            .get(b)
            .and_then(|t| defs[t].inputs.iter().find(|(n, _, _)| *n == bi));
        match (out, inp) {
            (Some((_, ot)), Some((_, it, _)))
                if it == ot || (*it == "any" && (*ot == "image" || *ot == "mask")) => {}
            _ => return Err(format!("invalid connection {a}.{ao} -> {b}.{bi}")),
        }
        parents.entry(b).or_default().push(a);
    }
    let mut needed: HashSet<&str> = HashSet::new();
    match targets {
        Some(t) if !t.is_empty() => {
            let mut todo: Vec<&str> = t.iter().map(String::as_str).collect();
            while let Some(i) = todo.pop() {
                if types.contains_key(i) && needed.insert(i) {
                    todo.extend(parents.get(i).into_iter().flatten().copied());
                }
            }
        }
        _ => needed.extend(types.keys().copied()),
    }
    let mut ids: Vec<&str> = needed.iter().copied().collect();
    ids.sort_unstable();
    let (mut order, mut done, mut open): (Vec<String>, HashSet<&str>, HashSet<&str>) =
        (vec![], HashSet::new(), HashSet::new());
    fn visit<'a>(
        i: &'a str,
        parents: &HashMap<&'a str, Vec<&'a str>>,
        done: &mut HashSet<&'a str>,
        open: &mut HashSet<&'a str>,
        order: &mut Vec<String>,
    ) -> Result<(), String> {
        if done.contains(i) {
            return Ok(());
        }
        if !open.insert(i) {
            return Err("the graph has a cycle".into());
        }
        for p in parents.get(i).into_iter().flatten() {
            visit(p, parents, done, open, order)?;
        }
        open.remove(i);
        done.insert(i);
        order.push(i.to_string());
        Ok(())
    }
    for i in ids {
        visit(i, &parents, &mut done, &mut open, &mut order)?;
    }
    Ok(order)
}
