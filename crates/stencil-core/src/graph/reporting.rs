//! Findings, metrics and timing for completed graph runs.

use super::{
    catalogue::catalogue, migrate::migrate, params::clean_params, types::NodeDef, Engine, Value,
};
use serde_json::{json, Map, Value as Json};
use std::collections::HashMap;

impl Engine {
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
}
