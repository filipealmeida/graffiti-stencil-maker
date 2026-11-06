//! Connection validation and target-aware topological planning.

use super::types::NodeDef;
use serde_json::Value as Json;
use std::collections::{HashMap, HashSet};

/// Validates the graph and returns the nodes to evaluate, parents first.
pub(super) fn plan(
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
                if it == ot
                    || (*it == "any" && (*ot == "image" || *ot == "mask"))
                    || (*it == "item" && matches!(*ot, "image" | "mask" | "shapes" | "layer"))
                    || (*it == "body" && (*ot == "solid" || *ot == "parts")) => {}
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
