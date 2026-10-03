import json
import threading
import time
import uuid
from pathlib import Path

from flask import Flask, Response, request

from .core import Params, make_stencil
from .tiling import DIAMETERS, SHAPES, rod_name, rod_stl

STATIC = Path(__file__).parent / "static"
CONNECTORS = Path(__file__).parent.parent / "toolbox" / "stls" / "connectors"
app = Flask(__name__, static_folder=str(STATIC), static_url_path="/static")
app.config["MAX_CONTENT_LENGTH"] = 50 * 1024 * 1024
JOBS: dict[str, dict] = {}
LOCK = threading.Lock()


@app.get("/")
def index():
    return Response((STATIC / "index.html").read_bytes(), mimetype="text/html")


def parse_params(form) -> Params:
    thr = form.get("threshold", "")
    return Params(
        width_mm=float(form.get("width", 100)),
        resolution=min(600, max(20, int(form.get("resolution", 250)))),
        thickness_mm=float(form.get("thickness", 2)),
        margin_mm=float(form.get("margin", 8)),
        bridge_mm=float(form.get("bridge", 1.6)),
        threshold=int(thr) if thr not in ("", "auto") else None,
        invert=form.get("invert") in ("1", "true", "on"),
        smooth_mm=min(3.0, max(0.0, float(form.get("smooth", 0)))),
        raised_bridges=form.get("raised") in ("1", "true", "on"),
        z_bridging=form.get("z_bridging") if form.get("z_bridging") in ("ramp", "stepramp") else "steps",
        min_island_mm2=max(0.0, float(form.get("min_island", 0) or 0)),
        max_overhang_deg=min(89.0, max(0.0, float(form.get("max_overhang", 0) or 0))),
        flip=form.get("flip") in ("1", "true", "on"),
        pad_count=min(4, max(0, int(form.get("pads", 0) or 0))),
        pad_thread=form.get("pad_thread") if form.get("pad_thread") in ("M4", "M6", "M8", "M10") else "M8",
        pad_height_mm=min(50.0, max(0.2, float(form.get("pad_height", 6) or 6))),
        thread_clearance_mm=min(1.0, max(0.0, float(form.get("thread_clearance", 0.2) or 0.2))),
        tile_max_x_mm=max(0.0, float(form.get("tile_x", 0) or 0)),
        tile_max_y_mm=max(0.0, float(form.get("tile_y", 0) or 0)),
        connector_diameter_mm=float(form.get("connector_diameter", 0) or 0),
        connector_shape=form.get("connector_shape") if form.get("connector_shape") in SHAPES else "hex",
        connector_clearance_mm=min(1.0, max(0.0, float(form.get("connector_clearance", 0.2) or 0.2))),
        extra_top_layers=min(50, max(0, int(form.get("extra_top", 1)))),
        layer_height_mm=max(0.05, float(form.get("layer_height") or form.get("bridge_height") or 0.2)),
    )


@app.post("/api/stencil")
def stencil():
    """Synchronous API: returns the STL, statistics in the X-Stencil-Stats header."""
    f = request.files.get("image")
    if f is None:
        return {"error": "missing 'image' file"}, 400
    try:
        stl, stats = make_stencil(f.read(), f.filename or "", parse_params(request.form))
        stats.pop("first_layer_svg", None)
        stats.pop("trace_svg", None)
        stats.pop("overlay_png", None)
        stats.pop("tiles_zip", None)
    except Exception as e:  # bad image / params
        return {"error": str(e)}, 400
    return Response(stl, mimetype="model/stl", headers={"X-Stencil-Stats": json.dumps(stats)})


def _run(job, data, name, p):
    t0 = time.perf_counter()
    events = job["events"]          # appended the moment a stage finishes
    cur = [None, t0]

    def finish(now):
        if cur[0] and cur[0] != "Done":
            events.append({"stage": cur[0], "ms": round((now - cur[1]) * 1000, 1)})

    def progress(f, stage):
        if job.get("cancel"):
            raise RuntimeError("cancelled")
        now = time.perf_counter()
        job["progress"], job["stage"] = f, stage
        if stage != cur[0]:
            finish(now)
            cur[0], cur[1] = stage, now

    try:
        job["stl"], job["stats"] = make_stencil(data, name, p, progress)
        job["layer_svg"] = job["stats"].pop("first_layer_svg", "")
        job["trace_svg"] = job["stats"].pop("trace_svg", "")
        job["overlay"] = job["stats"].pop("overlay_png", b"")
        job["tiles_zip"] = job["stats"].pop("tiles_zip", b"")
    except Exception as e:
        job["error"] = str(e)
    end = time.perf_counter()
    finish(end)
    job["total_ms"] = round((end - t0) * 1000, 1)
    job["done"] = True


@app.post("/api/jobs")
def start_job():
    f = request.files.get("image")
    if f is None:
        return {"error": "missing 'image' file"}, 400
    try:
        p = parse_params(request.form)
    except Exception as e:
        return {"error": str(e)}, 400
    job = {"progress": 0.0, "stage": "Queued", "done": False, "t": time.time(), "events": []}
    jid = uuid.uuid4().hex
    with LOCK:
        for k in [k for k, v in JOBS.items() if time.time() - v["t"] > 600]:
            del JOBS[k]
        JOBS[jid] = job
    threading.Thread(target=_run, args=(job, f.read(), f.filename or "", p), daemon=True).start()
    return {"id": jid}


@app.get("/api/jobs/<jid>")
def job_status(jid):
    job = JOBS.get(jid)
    if job is None:
        return {"error": "unknown job"}, 404
    return {"progress": job["progress"], "stage": job["stage"], "done": job["done"], "error": job.get("error"),
            "events": job["events"][int(request.args.get("since", 0)):], "total_ms": job.get("total_ms")}


@app.get("/api/jobs/<jid>/stl")
def job_stl(jid):
    job = JOBS.get(jid)
    if job is None or not job.get("done") or "stl" not in job:
        return {"error": "not ready"}, 404
    return Response(job["stl"], mimetype="model/stl", headers={"X-Stencil-Stats": json.dumps(job["stats"])})


@app.get("/api/jobs/<jid>/layer.svg")
def job_layer(jid):
    job = JOBS.get(jid)
    if job is None or not job.get("layer_svg"):
        return {"error": "not ready"}, 404
    return Response(job["layer_svg"], mimetype="image/svg+xml")


@app.get("/api/jobs/<jid>/trace.svg")
def job_trace(jid):
    job = JOBS.get(jid)
    if job is None or not job.get("trace_svg"):
        return {"error": "not ready"}, 404
    return Response(job["trace_svg"], mimetype="image/svg+xml")


@app.get("/api/jobs/<jid>/overlay.png")
def job_overlay(jid):
    job = JOBS.get(jid)
    if job is None or not job.get("overlay"):
        return {"error": "not ready"}, 404
    return Response(job["overlay"], mimetype="image/png")


@app.get("/api/jobs/<jid>/tiles.zip")
def job_tiles(jid):
    job = JOBS.get(jid)
    if job is None or not job.get("tiles_zip"):
        return {"error": "not ready"}, 404
    return Response(job["tiles_zip"], mimetype="application/zip")


def _connector(shape, d):
    f = CONNECTORS / rod_name(shape, d)
    return f.read_bytes() if f.is_file() else rod_stl(shape, d)


@app.get("/api/connectors/<name>")
def connector_stl(name):
    known = {rod_name(s, d): (s, d) for s in SHAPES for d in DIAMETERS}
    if name not in known:
        return {"error": "unknown connector"}, 404
    return Response(_connector(*known[name]), mimetype="model/stl",
                    headers={"Content-Disposition": f"attachment; filename={name}"})


@app.get("/api/connectors.zip")
def connectors_zip():
    import io
    import zipfile
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        for s in SHAPES:
            for d in DIAMETERS:
                z.writestr(rod_name(s, d), _connector(s, d))
    return Response(buf.getvalue(), mimetype="application/zip", headers={"Content-Disposition": "attachment; filename=connectors.zip"})


@app.delete("/api/jobs/<jid>")
def job_cancel(jid):
    """Ask a running job to stop at its next progress report (used when a newer request supersedes it)."""
    job = JOBS.get(jid)
    if job is None:
        return {"error": "unknown job"}, 404
    job["cancel"] = True
    return {"ok": True}
