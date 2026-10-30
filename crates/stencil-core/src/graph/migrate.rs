//! Projects saved by older versions keep working.
//!
//! "complexity" is now "remesh" and "bridge_fragments" is now "bridges". Stencil nodes used to bridge their own
//! islands, and Split volume the islands a cut created; now a Bridges node does it. A node that still carries the
//! old bridge parameters gets a Bridges node after it, with those parameters.

use serde_json::{json, Value as Json};

/// Bridge parameters the stencil and Split volume nodes used to have.
const LEGACY: [&str; 9] = [
    "bridge_mm",
    "bridges_per_island",
    "bridge_spacing",
    "bridge_min_angle_deg",
    "bridge_max_length",
    "bridge_passes",
    "raised_bridges",
    "z_bridging",
    "bridge_islands",
];

/// The ones the Bridges node has too.
const KEPT: [&str; 7] = [
    "bridge_mm",
    "bridges_per_island",
    "bridge_spacing",
    "bridge_min_angle_deg",
    "bridge_max_length",
    "raised_bridges",
    "z_bridging",
];

pub(super) fn migrate(graph: &Json) -> Json {
    let mut g = graph.clone();
    let renamed: Vec<String> = g["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| n["type"] == "bridge_fragments")
        .filter_map(|n| n["id"].as_str().map(String::from))
        .collect();
    for e in g["edges"].as_array_mut().into_iter().flatten() {
        if e["to"][1] == "parts" && renamed.iter().any(|i| e["to"][0] == i.as_str()) {
            e["to"][1] = json!("body");
        }
    }
    for n in g["nodes"].as_array_mut().into_iter().flatten() {
        if n["type"] == "complexity" {
            n["type"] = json!("remesh");
        }
        if n["type"] == "bridge_fragments" {
            n["type"] = json!("bridges");
        }
    }
    let legacy: Vec<(String, String)> = g["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| {
            matches!(
                n["type"].as_str(),
                Some("stencil" | "vector_stencil" | "tiles")
            ) && n["params"]
                .as_object()
                .is_some_and(|p| LEGACY.iter().any(|k| p.contains_key(*k)))
        })
        .filter_map(|n| {
            Some((
                n["id"].as_str()?.to_string(),
                n["type"].as_str()?.to_string(),
            ))
        })
        .collect();
    for (id, ty) in legacy {
        insert_bridges(&mut g, &id, &ty);
    }
    g
}

fn insert_bridges(g: &mut Json, id: &str, ty: &str) {
    let mut params = serde_json::Map::new();
    let mut wanted = true;
    if let Some(node) = g["nodes"]
        .as_array_mut()
        .and_then(|a| a.iter_mut().find(|n| n["id"] == id))
    {
        if let Some(p) = node["params"].as_object_mut() {
            for k in LEGACY {
                if let Some(v) = p.remove(k) {
                    if KEPT.contains(&k) {
                        params.insert(k.into(), v);
                    } else if k == "bridge_islands" {
                        wanted = v != json!(false);
                    }
                }
            }
        }
    }
    // Split volume bridged only what a cut created, and only if asked to; the stencils bridged always
    if wanted {
        add_bridges(g, id, ty, params);
    }
}

/// Puts a Bridges node after the node `id` (of type `ty`) and moves what read its solid and parts onto the new node.
pub(super) fn add_bridges(g: &mut Json, id: &str, ty: &str, params: serde_json::Map<String, Json>) {
    let ids: Vec<String> = g["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| n["id"].as_str().map(String::from))
        .collect();
    let mut new = format!("{id}-bridges");
    while ids.contains(&new) {
        new.push('_');
    }
    let edges = g["edges"].as_array().cloned().unwrap_or_default();
    // a stencil fed by a tile layout makes parts, otherwise a solid; Split volume makes parts
    let tiled = edges
        .iter()
        .any(|e| e["to"][0] == id && e["to"][1] == "tiles");
    let port = if ty == "tiles" || tiled {
        "parts"
    } else {
        "solid"
    };
    let mut out = Vec::new();
    for mut e in edges {
        if e["from"][0] == id && (e["from"][1] == "solid" || e["from"][1] == "parts") {
            if ty == "tiles" && e["from"][1] == "solid" {
                out.push(e);
                continue;
            }
            e["from"][0] = json!(new);
        }
        out.push(e);
    }
    out.push(json!({"from": [id, port], "to": [new, "body"]}));
    g["edges"] = json!(out);
    if let Some(nodes) = g["nodes"].as_array_mut() {
        nodes.push(json!({"id": new, "type": "bridges", "params": params}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stencil_with_bridge_parameters_gets_a_bridges_node_after_it() {
        let g = json!({
            "nodes": [
                {"id": "s", "type": "stencil", "params": {"width_mm": 100, "bridge_mm": 2.5, "bridge_passes": 4}},
                {"id": "t", "type": "tiles", "params": {"bridge_islands": true, "bridge_mm": 1.6}},
                {"id": "o", "type": "tiles", "params": {"bridge_islands": false}},
            ],
            "edges": [
                {"from": ["s", "solid"], "to": ["t", "solid"]},
                {"from": ["t", "parts"], "to": ["x", "parts"]},
                {"from": ["t", "solid"], "to": ["y", "solid"]},
            ],
        });
        let m = migrate(&g);
        let nodes = m["nodes"].as_array().unwrap();
        let b = nodes.iter().find(|n| n["id"] == "s-bridges").unwrap();
        assert_eq!(b["type"], "bridges");
        assert_eq!(b["params"]["bridge_mm"], 2.5);
        assert!(nodes[0]["params"].get("bridge_mm").is_none());
        assert_eq!(nodes[0]["params"]["width_mm"], 100);
        let e = m["edges"].as_array().unwrap();
        assert!(e.contains(&json!({"from": ["s", "solid"], "to": ["s-bridges", "body"]})));
        assert!(e.contains(&json!({"from": ["s-bridges", "solid"], "to": ["t", "solid"]})));
        assert!(e.contains(&json!({"from": ["t-bridges", "parts"], "to": ["x", "parts"]})));
        // the solid that Split volume passes on stays as it was
        assert!(e.contains(&json!({"from": ["t", "solid"], "to": ["y", "solid"]})));
        // Split volume without the bridging flag gets none
        assert!(!nodes.iter().any(|n| n["id"] == "o-bridges"));
        // migrating twice changes nothing
        assert_eq!(migrate(&m), m);
    }

    #[test]
    fn a_tiled_stencil_bridges_its_parts() {
        let g = json!({
            "nodes": [{"id": "s", "type": "vector_stencil", "params": {"bridge_mm": 1.6}}],
            "edges": [{"from": ["l", "tiles"], "to": ["s", "tiles"]}, {"from": ["s", "parts"], "to": ["p", "parts"]}],
        });
        let e = migrate(&g)["edges"].clone();
        assert!(e
            .as_array()
            .unwrap()
            .contains(&json!({"from": ["s", "parts"], "to": ["s-bridges", "body"]})));
        assert!(e
            .as_array()
            .unwrap()
            .contains(&json!({"from": ["s-bridges", "parts"], "to": ["p", "parts"]})));
    }
}
