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
        {"id": "thr", "type": "threshold", "params": {}},
        {"id": "st", "type": "stencil", "params": {"width_mm": 80, "margin_mm": 6, "raised_bridges": True, "z_bridging": "easy", "thickness_mm": 2}},
        {"id": "ex", "type": "export"}],
        "edges": [{"from": ["src", "image"], "to": ["thr", "image"]},
                  {"from": ["thr", "mask"], "to": ["st", "mask"]}, {"from": ["st", "solid"], "to": ["ex", "solid"]}]}
    st = run(c, g)
    assert all(n["state"] == "done" for n in st["nodes"].values()), st
    key = st["nodes"]["st"]["key"]
    info = c.get(f"/api/info/{key}/solid").json()
    assert info["stats"]["watertight"] and info["stats"]["islands_remaining"] == 0
    # an edit re-runs only what changed
    g["nodes"][1]["params"]["invert"] = True
    st2 = run(c, g)
    assert st2["nodes"]["src"]["cached"] and not st2["nodes"]["thr"]["cached"]


def test_errors_and_types(tmp_path):
    c = TestClient(create_app(tmp_path))
    bad = {"nodes": [{"id": "a", "type": "threshold"}, {"id": "b", "type": "stencil"}],
           "edges": [{"from": ["a", "mask"], "to": ["b", "mask"]}]}
    st = run(c, bad)
    assert st["nodes"]["a"]["state"] == "error" and st["nodes"]["b"]["state"] == "error"
    wrong = {"nodes": [{"id": "a", "type": "threshold"}, {"id": "b", "type": "export"}],
             "edges": [{"from": ["a", "mask"], "to": ["b", "solid"]}]}
    assert c.post("/api/run", json={"graph": wrong}).status_code == 400


def test_logs_tiles_and_parts(tmp_path):
    c = TestClient(create_app(tmp_path))
    img = c.post("/api/images", files={"file": ("a.png", picture(), "image/png")}).json()["id"]
    g = {"nodes": [
        {"id": "src", "type": "source", "params": {"image_id": img, "resolution": 200}},
        {"id": "thr", "type": "threshold", "params": {}},
        {"id": "st", "type": "stencil", "params": {"width_mm": 120, "margin_mm": 8}},
        {"id": "ex", "type": "export", "params": {"tile_max_x_mm": 70, "tile_max_y_mm": 200, "mount_screw": "M4"}},
    ], "edges": [
        {"from": ["src", "image"], "to": ["thr", "image"]},
        {"from": ["thr", "mask"], "to": ["st", "mask"]},
        {"from": ["st", "solid"], "to": ["ex", "solid"]},
    ]}
    st = run(c, g, targets=["thr"])
    assert st["nodes"]["st"]["state"] == "pending"          # only the trace part ran
    assert any(l["node"] == "thr" and "done" in l["msg"] for l in st["log"])
    st = run(c, g)
    assert not c.get(f"/api/info/{st['nodes']['st']['key']}/solid").json()["tiles"]      # the stencil node never tiles
    key = st["nodes"]["ex"]["key"]
    info = c.get(f"/api/info/{key}/solid").json()
    assert len(info["tiles"]) >= 2 and info["stats"]["islands_remaining"] == 0
    mount = info["stats"]["mount"]
    assert mount["holes"] >= 4 and mount["pairs"] >= 1
    r = c.get(f"/api/part/{key}/solid/{info['tiles'][0]}")
    assert r.status_code == 200 and len(r.content) > 84
    assert c.get(f"/api/part/{key}/solid/nope.stl").status_code == 404
    files = c.get(f"/api/info/{key}/parts").json()["files"]
    assert "joiner_2hole_M4.stl" in files and "extender_4hole_M4.stl" in files


def test_reduce_and_tone_patterns(tmp_path):
    c = TestClient(create_app(tmp_path))
    img = c.post("/api/images", files={"file": ("a.png", picture(), "image/png")}).json()["id"]
    g = {"nodes": [
        {"id": "src", "type": "source", "params": {"image_id": img, "resolution": 200}},
        {"id": "red", "type": "reduce", "params": {"colours": 3}},
        {"id": "tp", "type": "tone_patterns", "params": {"plate_width_mm": 100}},
        {"id": "st", "type": "stencil", "params": {"width_mm": 100, "margin_mm": 6, "raised_bridges": True, "z_bridging": "easy"}},
    ], "edges": [
        {"from": ["src", "image"], "to": ["red", "image"]},
        {"from": ["red", "image"], "to": ["tp", "image"]},
        {"from": ["tp", "mask"], "to": ["st", "mask"]},
    ]}
    st = run(c, g)
    assert all(n["state"] == "done" for n in st["nodes"].values()), st
    key = st["nodes"]["tp"]["key"]
    cov = c.get(f"/api/info/{key}/mask").json()["coverage"]
    g["nodes"][2]["params"]["invert"] = True
    st2 = run(c, g)
    cov2 = c.get(f"/api/info/{st2['nodes']['tp']['key']}/mask").json()["coverage"]
    assert 0.05 < cov < 0.95 and cov != cov2
    info = c.get(f"/api/info/{st['nodes']['st']['key']}/solid").json()
    assert info["stats"]["watertight"] and info["stats"]["islands_remaining"] == 0
