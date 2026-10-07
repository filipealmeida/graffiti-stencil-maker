"""Typed node graph: registry, validation and cached evaluation."""
from __future__ import annotations

import hashlib
import json
from collections import OrderedDict
from dataclasses import dataclass, field
from typing import Any, Callable

TYPES = ("image", "mask", "solid", "parts")
STAGES = ("trace", "stencil", "post")


@dataclass
class NodeType:
    id: str
    label: str
    stage: str
    inputs: list[dict]      # {name, type, required}; type "any" accepts image or mask (used only for its size)
    outputs: list[dict]     # {name, type}
    params: list[dict]      # {name, label, kind: int|float|bool|choice|text, default, min, max, step, choices, group}
    fn: Callable
    doc: str = ""

    def schema(self) -> dict:
        return {"id": self.id, "label": self.label, "stage": self.stage, "inputs": self.inputs,
                "outputs": self.outputs, "params": self.params, "doc": self.doc}

    def defaults(self) -> dict:
        return {p["name"]: p["default"] for p in self.params}


REGISTRY: dict[str, NodeType] = {}


def inp(name, type="image", required=True):
    return {"name": name, "type": type, "required": required}


def out(name, type):
    return {"name": name, "type": type}


def num(name, label, default, lo, hi, step=None, group=None, integer=False):
    return {"name": name, "label": label, "kind": "int" if integer else "float", "default": default, "min": lo, "max": hi,
            "step": step or (1 if integer else 0.1), "group": group}


def flag(name, label, default=False, group=None):
    return {"name": name, "label": label, "kind": "bool", "default": default, "group": group}


def choice(name, label, default, choices, group=None):
    return {"name": name, "label": label, "kind": "choice", "default": default, "choices": choices, "group": group}


def node(id, label, stage, inputs, outputs, params=(), doc=""):
    def deco(fn):
        REGISTRY[id] = NodeType(id, label, stage, list(inputs), list(outputs), list(params), fn, doc)
        return fn
    return deco


def compatible(src_type: str, dst_type: str) -> bool:
    return dst_type == src_type or (dst_type == "any" and src_type in ("image", "mask"))


class GraphError(ValueError):
    pass


def clean_params(nt: NodeType, given: dict) -> dict:
    """Fill defaults and coerce/clamp every parameter to its declared kind."""
    res = {}
    for p in nt.params:
        v = given.get(p["name"], p["default"])
        k = p["kind"]
        if k == "bool":
            v = bool(v)
        elif k in ("int", "float"):
            v = float(v)
            v = min(p["max"], max(p["min"], v))
            v = int(round(v)) if k == "int" else v
        elif k == "choice":
            v = v if v in p["choices"] else p["default"]
        else:
            v = str(v)
        res[p["name"]] = v
    return res


def validate(graph: dict) -> None:
    ids = set()
    for n in graph.get("nodes", []):
        if n["id"] in ids:
            raise GraphError(f"duplicate node id {n['id']}")
        ids.add(n["id"])
        if n["type"] not in REGISTRY:
            raise GraphError(f"unknown node type {n['type']}")
    taken = set()
    by_id = {n["id"]: n for n in graph.get("nodes", [])}
    for e in graph.get("edges", []):
        (a, ao), (b, bi) = e["from"], e["to"]
        if a not in by_id or b not in by_id:
            raise GraphError("edge refers to a missing node")
        src = next((o for o in REGISTRY[by_id[a]["type"]].outputs if o["name"] == ao), None)
        dst = next((i for i in REGISTRY[by_id[b]["type"]].inputs if i["name"] == bi), None)
        if not src or not dst:
            raise GraphError(f"edge {a}.{ao} -> {b}.{bi}: no such socket")
        if not compatible(src["type"], dst["type"]):
            raise GraphError(f"cannot connect {src['type']} to {dst['type']} ({a}.{ao} -> {b}.{bi})")
        if (b, bi) in taken:
            raise GraphError(f"input {b}.{bi} has more than one connection")
        taken.add((b, bi))
    # cycle check
    deps = {i: [] for i in ids}
    for e in graph.get("edges", []):
        deps[e["to"][0]].append(e["from"][0])
    state: dict[str, int] = {}

    def visit(i):
        if state.get(i) == 1:
            raise GraphError("the graph has a cycle")
        if state.get(i) == 2:
            return
        state[i] = 1
        for d in deps[i]:
            visit(d)
        state[i] = 2
    for i in ids:
        visit(i)


class Store:
    """In-memory LRU cache of node results, keyed by a hash of the node's type, parameters and input keys."""

    def __init__(self, limit=96):
        self.limit = limit
        self.items: OrderedDict[str, dict] = OrderedDict()

    def get(self, key):
        v = self.items.get(key)
        if v is not None:
            self.items.move_to_end(key)
        return v

    def put(self, key, value):
        self.items[key] = value
        self.items.move_to_end(key)
        while len(self.items) > self.limit:
            self.items.popitem(last=False)


@dataclass
class Ctx:
    uploads: Any
    progress: Callable[[float, str], None] = lambda f, m: None


def node_key(nt: NodeType, prm: dict, input_keys: dict) -> str:
    h = hashlib.sha1(json.dumps([nt.id, prm, sorted(input_keys.items())], sort_keys=True, default=str).encode())
    return h.hexdigest()[:20]


def run_graph(graph: dict, store: Store, uploads, on_event: Callable[[str, dict], None], targets=None) -> None:
    """Evaluate the graph (or only the ancestors of `targets`). `on_event(node_id, state_dict)` reports every change."""
    validate(graph)
    nodes = {n["id"]: n for n in graph["nodes"]}
    feeds = {(e["to"][0], e["to"][1]): tuple(e["from"]) for e in graph.get("edges", [])}
    needed = set(nodes)
    if targets:
        needed, todo = set(), list(targets)
        while todo:
            i = todo.pop()
            if i in needed or i not in nodes:
                continue
            needed.add(i)
            todo += [src for (dst, _), (src, _o) in feeds.items() if dst == i]
    order, seen = [], set()

    def visit(i):
        if i in seen:
            return
        seen.add(i)
        for (dst, _), (src, _o) in feeds.items():
            if dst == i:
                visit(src)
        order.append(i)
    for i in sorted(needed):
        visit(i)
    results: dict[str, dict | None] = {}
    for i in (x for x in order if x in needed):
        nt = REGISTRY[nodes[i]["type"]]
        prm = clean_params(nt, nodes[i].get("params", {}))
        on_event(i, {"state": "pending"})
        values, keys, problem = {}, {}, None
        for port in nt.inputs:
            src = feeds.get((i, port["name"]))
            if src is None:
                if port["required"]:
                    problem = f"input '{port['name']}' is not connected"
                    break
                continue
            up = results.get(src[0])
            if up is None:
                problem = "waiting on a failed node upstream"
                break
            val = up["values"].get(src[1])
            if val is None:
                problem = f"'{src[1]}' is not available from {src[0]}"
                break
            values[port["name"]], keys[port["name"]] = val, f"{up['key']}:{src[1]}"
        if problem:
            results[i] = None
            on_event(i, {"state": "error", "error": problem})
            continue
        key = node_key(nt, prm, keys)
        hit = store.get(key)
        cached = hit is not None
        if hit is None:
            on_event(i, {"state": "running", "progress": 0.0})
            try:
                ctx = Ctx(uploads, lambda f, m, i=i: on_event(i, {"state": "running", "progress": f, "message": m}))
                vals = nt.fn(values, prm, ctx)
            except Exception as ex:      # a node failing must not stop the other branches
                results[i] = None
                on_event(i, {"state": "error", "error": str(ex) or ex.__class__.__name__})
                continue
            hit = {"key": key, "values": vals}
            store.put(key, hit)
        results[i] = hit
        on_event(i, {"state": "done", "key": key, "cached": cached,
                     "outputs": {o["name"]: {"type": o["type"], "available": hit["values"].get(o["name"]) is not None}
                                 for o in nt.outputs}})
