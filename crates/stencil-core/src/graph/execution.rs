//! Dependency-ordered evaluation and bounded output caching.

use super::{
    cache::hash_hex,
    catalogue::catalogue,
    migrate::migrate,
    params::clean_params,
    types::{NodeDef, Outputs},
    validation::plan,
    Engine,
};
use serde_json::{json, Map, Value as Json};
use std::{collections::HashMap, rc::Rc};

impl Engine {
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
            if matches!(
                ty,
                "bridges"
                    | "remesh"
                    | "pads"
                    | "ring"
                    | "fragments"
                    | "tiles"
                    | "export"
                    | "roll_up_seams"
            ) && inputs.values().any(|v| {
                let meta = match v {
                    super::Value::Solid(s) => &s.stats,
                    super::Value::Parts(p) => &p.meta,
                    _ => return false,
                };
                !meta["roll_up"].is_null() || !meta["hinge"].is_null()
            }) {
                let m = "geometry-changing nodes cannot follow experimental hinges: bridge, split, simplify and add hardware before hinging".to_string();
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
                        self.cache_outputs(key.clone(), vals.clone());
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
