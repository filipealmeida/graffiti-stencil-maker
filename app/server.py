"""FastAPI backend: node catalogue, uploads, graph runs as background jobs, artifacts and project files."""
from __future__ import annotations

import hashlib
import io
import json
import re
import threading
import uuid
import zipfile
from pathlib import Path

import numpy as np
from fastapi import FastAPI, HTTPException, Request, UploadFile
from fastapi.responses import FileResponse, JSONResponse, Response
from PIL import Image

from . import nodes  # noqa: F401  (registers the node types)
from .graph import REGISTRY, GraphError, Store, run_graph

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "projects"
WEB = Path(__file__).resolve().parent / "web" / "dist"
SAFE = re.compile(r"^[\w\- .]{1,80}$")


class Uploads:
    def __init__(self, folder: Path):
        self.folder = folder
        folder.mkdir(parents=True, exist_ok=True)

    def save(self, data: bytes, name: str) -> str:
        key = hashlib.sha1(data).hexdigest()[:16]
        (self.folder / key).write_bytes(data)
        (self.folder / f"{key}.name").write_text(name)
        return key

    def read(self, key: str):
        if not re.fullmatch(r"[0-9a-f]{16}", key) or not (self.folder / key).exists():
            raise ValueError("image not found: upload it again")
        return (self.folder / key).read_bytes(), (self.folder / f"{key}.name").read_text()


def create_app(data_dir: Path = DATA) -> FastAPI:
    app = FastAPI(title="Stencil studio")
    uploads = Uploads(data_dir / "uploads")
    store = Store()
    jobs: dict[str, dict] = {}
    lock = threading.Lock()

    @app.get("/api/node-types")
    def node_types():
        return [t.schema() for t in REGISTRY.values()]

    @app.post("/api/images")
    async def upload(file: UploadFile):
        data = await file.read()
        if len(data) > 50 * 1024 * 1024:
            raise HTTPException(413, "file too large")
        from stencil.core import load_image
        try:
            img = load_image(data, file.filename or "")
        except Exception:
            raise HTTPException(400, "not a readable png/jpg/bmp/svg image")
        return {"id": uploads.save(data, file.filename or "image"), "width": img.width, "height": img.height,
                "name": file.filename}

    @app.get("/api/images/{key}")
    def image(key: str):
        from stencil.core import load_image
        try:
            data, name = uploads.read(key)
        except ValueError:
            raise HTTPException(404)
        buf = io.BytesIO()
        load_image(data, name).save(buf, "PNG")
        return Response(buf.getvalue(), media_type="image/png")

    @app.post("/api/run")
    async def run(request: Request):
        body = await request.json()
        graph, targets = body.get("graph", {}), body.get("targets")
        try:
            from .graph import validate
            validate(graph)
        except (GraphError, KeyError) as e:
            raise HTTPException(400, str(e))
        jid = uuid.uuid4().hex[:12]
        job = {"status": "running", "nodes": {n["id"]: {"state": "pending"} for n in graph["nodes"]}}
        jobs[jid] = job

        def on_event(nid, st):
            with lock:
                cur = job["nodes"].setdefault(nid, {})
                if st["state"] != "running":
                    cur.clear()
                cur.update(st)

        def work():
            try:
                run_graph(graph, store, uploads, on_event, targets)
            except Exception as e:
                job["error"] = str(e)
            job["status"] = "done"

        threading.Thread(target=work, daemon=True).start()
        while len(jobs) > 40:
            jobs.pop(next(iter(jobs)))
        return {"job": jid}

    @app.get("/api/jobs/{jid}")
    def job_status(jid: str):
        if jid not in jobs:
            raise HTTPException(404)
        with lock:
            return json.loads(json.dumps(jobs[jid]))

    @app.get("/api/artifact/{key}/{output}")
    def artifact(key: str, output: str, size: int = 0):
        hit = store.get(key)
        if hit is None or hit["values"].get(output) is None:
            raise HTTPException(404, "not computed (run the graph again)")
        v = hit["values"][output]
        if isinstance(v, np.ndarray):
            v = Image.fromarray(np.where(v, 0, 255).astype(np.uint8))
        if isinstance(v, Image.Image):
            if size and max(v.size) > size:
                v = v.copy()
                v.thumbnail((size, size), Image.NEAREST if v.mode == "L" else Image.LANCZOS)
            buf = io.BytesIO()
            v.save(buf, "PNG")
            return Response(buf.getvalue(), media_type="image/png")
        if "stl" in v:
            return Response(v["stl"], media_type="model/stl", headers={"Content-Disposition": 'attachment; filename="stencil.stl"'})
        files = v["files"]
        if len(files) == 1:
            n, b = next(iter(files.items()))
            return Response(b, media_type="model/stl", headers={"Content-Disposition": f'attachment; filename="{n}"'})
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
            for n, b in files.items():
                z.writestr(n, b)
        return Response(buf.getvalue(), media_type="application/zip", headers={"Content-Disposition": 'attachment; filename="stencil-parts.zip"'})

    @app.get("/api/info/{key}/{output}")
    def info(key: str, output: str):
        hit = store.get(key)
        v = hit["values"].get(output) if hit else None
        if v is None:
            raise HTTPException(404)
        if isinstance(v, np.ndarray):
            return {"kind": "mask", "width": v.shape[1], "height": v.shape[0], "coverage": round(float(v.mean()), 4)}
        if isinstance(v, Image.Image):
            return {"kind": "image", "width": v.width, "height": v.height}
        if "stl" in v:
            return {"kind": "solid", "stats": v["stats"], "bytes": len(v["stl"])}
        return {"kind": "parts", "files": {n: len(b) for n, b in v["files"].items()}}

    projects = data_dir / "graphs"

    @app.get("/api/projects")
    def list_projects():
        return sorted(p.stem for p in projects.glob("*.json")) if projects.exists() else []

    @app.get("/api/projects/{name}")
    def get_project(name: str):
        f = projects / f"{name}.json"
        if not SAFE.match(name) or not f.exists():
            raise HTTPException(404)
        return JSONResponse(json.loads(f.read_text()))

    @app.put("/api/projects/{name}")
    async def put_project(name: str, request: Request):
        if not SAFE.match(name):
            raise HTTPException(400, "bad project name")
        projects.mkdir(parents=True, exist_ok=True)
        (projects / f"{name}.json").write_text(json.dumps(await request.json()))
        return {"ok": True}

    if WEB.exists():
        @app.get("/{path:path}")
        def spa(path: str):
            f = (WEB / path).resolve()
            return FileResponse(f if path and f.is_file() and WEB in f.parents else WEB / "index.html")

    return app


app = create_app()
