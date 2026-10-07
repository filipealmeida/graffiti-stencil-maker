# Graffiti Stencil Maker

Converts png/jpg/bmp/svg into a printable stencil STL. Dark areas (or light, with invert) become holes.
Material pieces that would float free ("islands", e.g. the centre of an O) are detected and tied to the
frame with narrow bridges, so the stencil is always one connected, watertight piece.
    python3 -m venv .venv && .venv/bin/pip install -r requirements.txt
    .venv/bin/python -m stencil input.png -o out.stl [--width 100 --thickness 2 --bridge 1.6 --invert --threshold 128 --smooth 0.5 --raised-bridges]
    .venv/bin/python -m stencil --serve      # web UI on http://127.0.0.1:5000 (drag & drop, 3D preview)

CLI exits non-zero if islands remain or the mesh isn't watertight.

**Installing:** see [INSTALL.md](INSTALL.md) (Windows, Linux, macOS). **Using:** see [USAGE.md](USAGE.md).

## Stencil studio (v2, node graph)

A Blender-style node editor: image → trace nodes (Threshold, Reduce colours, Tone patterns) → stencil node (the engine above) → export. Multi-stencil sets are just several branches. See [USAGE.md](USAGE.md#stencil-studio-node-graph-interface) for the full guide.

    .venv/bin/pip install -r requirements.txt
    (cd app/web && npm install && npm run build)
    .venv/bin/python -m app --port 5056      # http://127.0.0.1:5056
    # dev: python -m app, and `npm run dev` in app/web (port 5173, proxies /api)

Highlights: drop an image to get a starter graph; add/delete nodes and edges; double-click a node for a live
workbench (zoom/pan raster, orbit 3D view with layer clipping and exploded tiles, parameters, per-node log);
trace nodes update almost live; a console and progress bars show runs; the **Colour template** button builds
Image → Reduce colours (2–10) → Tone patterns → Stencil → Tiles & mounting (tiling, pads, frame mounting holes, joiner/extender plates); projects are saved under `projects/`.

Layout: `stencil/` geometry library + CLI, `webui/` legacy single-page UI (`python -m stencil --serve`),
`app/` studio backend (FastAPI), `app/web/` studio frontend (React + Vite + @xyflow/react + three.js),
`tests/` pytest. Mask convention: painted = hole.

## License

[The Unlicense](LICENSE) (public domain), except `webui/static/vendor/`, which is three.js under the MIT license (see `webui/static/vendor/LICENSE`).
