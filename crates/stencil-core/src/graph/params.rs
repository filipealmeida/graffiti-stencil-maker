//! Parameter defaults, coercion and clamping.

use super::types::NodeDef;
use serde_json::{json, Map, Value as Json};

/// Parameters after defaults and clamping.
pub struct P(pub(super) Map<String, Json>);

impl P {
    pub(super) fn f(&self, k: &str) -> f64 {
        self.0.get(k).and_then(Json::as_f64).unwrap_or(0.0)
    }
    pub(super) fn b(&self, k: &str) -> bool {
        self.0.get(k).and_then(Json::as_bool).unwrap_or(false)
    }
    pub(super) fn s(&self, k: &str) -> &str {
        self.0.get(k).and_then(Json::as_str).unwrap_or("")
    }
}

pub(super) fn clean_params(def: &NodeDef, given: Option<&Json>) -> P {
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
