import io
import time

import numpy as np
from fastapi.testclient import TestClient
from PIL import Image, ImageDraw

from app.server import create_app


def picture():
    im = Image.new("RGB", (200, 200), "white")
    d = ImageDraw.Draw(im)
    d.ellipse((30, 30, 170, 170), fill=(20, 20, 20))
    d.ellipse((80, 80, 120, 120), fill=(240, 240, 240))
    d.rectangle((0, 0, 40, 40), fill=(200, 40, 40))
    b = io.BytesIO()
    im.save(b, "PNG")
    return b.getvalue()


def run(c, graph, targets=None):
    jid = c.post("/api/run", json={"graph": graph, "targets": targets}).json()["job"]
    for _ in range(600):
        st = c.get(f"/api/jobs/{jid}").json()
        if st["status"] != "running":
            return st
        time.sleep(0.1)
    raise AssertionError("timeout")


def test_pipeline(tmp_path):
    c = TestClient(create_app(tmp_path))
    img = c.post("/api/images", files={"file": ("a.png", picture(), "image/png")}).json()["id"]
    g = {"nodes": [
        {"id": "src", "type": "source", "params": {"image_id": img, "resolution": 200}},
        {"id": "red", "type": "reduce", "params": {"k": 3}},
        {"id": "pat", "type": "pattern", "params": {"kind": "dots", "spacing": 8}},
        {"id": "and", "type": "mask_boolean", "params": {"op": "and"}},
        {"id": "st", "type": "stencil", "params": {"width_mm": 80, "margin_mm": 6, "raised_bridges": True, "z_bridging": "easy", "thickness_mm": 2}},
        {"id": "ex", "type": "export"}],
        "edges": [{"from": ["src", "image"], "to": ["red", "image"]}, {"from": ["src", "image"], "to": ["pat", "size"]},
                  {"from": ["red", "m1"], "to": ["and", "a"]}, {"from": ["pat", "mask"], "to": ["and", "b"]},
                  {"from": ["and", "mask"], "to": ["st", "mask"]}, {"from": ["st", "solid"], "to": ["ex", "solid"]}]}
    st = run(c, g)
    assert all(n["state"] == "done" for n in st["nodes"].values()), st
    key = st["nodes"]["st"]["key"]
    info = c.get(f"/api/info/{key}/solid").json()
    assert info["stats"]["watertight"] and info["stats"]["islands_remaining"] == 0
    assert c.get(f"/api/artifact/{key}/solid").content[:5] == b"solid" or len(c.get(f"/api/artifact/{key}/solid").content) > 84
    # an edit re-runs only what changed
    g["nodes"][3]["params"]["op"] = "or"
    st2 = run(c, g)
    assert st2["nodes"]["src"]["cached"] and st2["nodes"]["red"]["cached"] and not st2["nodes"]["and"]["cached"]


def test_errors_and_types(tmp_path):
    c = TestClient(create_app(tmp_path))
    bad = {"nodes": [{"id": "a", "type": "threshold"}, {"id": "b", "type": "stencil"}],
           "edges": [{"from": ["a", "mask"], "to": ["b", "mask"]}]}
    st = run(c, bad)
    assert st["nodes"]["a"]["state"] == "error" and st["nodes"]["b"]["state"] == "error"
    wrong = {"nodes": [{"id": "a", "type": "threshold"}, {"id": "b", "type": "export"}],
             "edges": [{"from": ["a", "mask"], "to": ["b", "solid"]}]}
    assert c.post("/api/run", json={"graph": wrong}).status_code == 400
    cyc = {"nodes": [{"id": "a", "type": "mask_filter"}, {"id": "b", "type": "mask_filter"}],
           "edges": [{"from": ["a", "mask"], "to": ["b", "mask"]}, {"from": ["b", "mask"], "to": ["a", "mask"]}]}
    assert c.post("/api/run", json={"graph": cyc}).status_code == 400


def test_halftone_and_reduce_masks(tmp_path):
    from app.nodes import n_halftone, n_pattern
    im = Image.fromarray(np.tile(np.linspace(0, 255, 120, dtype=np.uint8), (60, 1))).convert("RGB")
    m = n_halftone({"image": im}, {"kind": "dots", "spacing": 8, "angle": 45, "contrast": 1.0, "invert": False}, None)["mask"]
    assert m[:, :20].mean() > m[:, -20:].mean()      # darker side has bigger dots
    p = n_pattern({"size": m}, {"kind": "crosshatch", "spacing": 10, "fill": 0.5, "angle": 0, "invert": False}, None)["mask"]
    assert p.shape == m.shape and 0.2 < p.mean() < 0.8


def test_logs_tiles_and_parts(tmp_path):
    c = TestClient(create_app(tmp_path))
    img = c.post("/api/images", files={"file": ("a.png", picture(), "image/png")}).json()["id"]
    g = {"nodes": [
        {"id": "src", "type": "source", "params": {"image_id": img, "resolution": 200}},
        {"id": "thr", "type": "threshold", "params": {}},
        {"id": "st", "type": "stencil", "params": {"width_mm": 120, "margin_mm": 6, "tile_max_x_mm": 70, "tile_max_y_mm": 200}},
    ], "edges": [
        {"from": ["src", "image"], "to": ["thr", "image"]},
        {"from": ["thr", "mask"], "to": ["st", "mask"]},
    ]}
    st = run(c, g, targets=["thr"])
    assert st["nodes"]["st"]["state"] == "pending"          # only the trace part ran
    assert any(l["node"] == "thr" and "done" in l["msg"] for l in st["log"])
    st = run(c, g)
    key = st["nodes"]["st"]["key"]
    info = c.get(f"/api/info/{key}/solid").json()
    assert len(info["tiles"]) >= 2
    r = c.get(f"/api/part/{key}/solid/{info['tiles'][0]}")
    assert r.status_code == 200 and len(r.content) > 84
    assert c.get(f"/api/part/{key}/solid/nope.stl").status_code == 404
