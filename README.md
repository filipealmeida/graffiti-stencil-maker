# Graffiti Stencil Maker

Converts png/jpg/bmp/svg into a printable stencil STL. Dark areas (or light, with invert) become holes.
Material pieces that would float free ("islands", e.g. the centre of an O) are detected and tied to the
frame with narrow bridges, so the stencil is always one connected, watertight piece.

    python3 -m venv .venv && .venv/bin/pip install -r requirements.txt
    .venv/bin/python -m stencil input.png -o out.stl [--width 100 --thickness 2 --bridge 1.6 --invert --threshold 128 --smooth 0.5 --raised-bridges]
    .venv/bin/python -m stencil --serve      # web UI on http://127.0.0.1:5000 (drag & drop, 3D preview)

CLI exits non-zero if islands remain or the mesh isn't watertight.

## Stencil studio (v2, node graph)

A Blender-style node editor: image → trace nodes (Threshold, Reduce colours, Tone patterns) → stencil node (the engine above) → export. Multi-stencil sets are just several branches.

    .venv/bin/pip install -r requirements.txt
    (cd app/web && npm install && npm run build)
    .venv/bin/python -m app --port 5056      # http://127.0.0.1:5056
    # dev: python -m app, and `npm run dev` in app/web (port 5173, proxies /api)

Layout: `stencil/` geometry library + CLI, `webui/` legacy single-page UI (`python -m stencil --serve`),
`app/` studio backend (FastAPI), `app/web/` studio frontend (React + Vite + @xyflow/react + three.js),
`tests/` pytest. Drop an image on the canvas to get a starter graph; mask convention: painted = hole.
Projects are saved under `projects/`.

Colour workflow (header button **Colour template**): *Reduce colours* (k-means in Lab to N flat colours) →
*Tone patterns*: colours are ranked by lightness (**Invert** flips it); the darkest is solid paint, the lightest
stays empty, and the tones between get lines/dots/crosshatch/checker that thin out step by step. Type, spacing
(mm) and coverage can be overridden per tone; set the node's plate width to the Stencil node's width so mm are
true. Dot patterns leave islands, which the stencil step bridges.

Double-click any node for its workbench: a full-size live preview (zoom/pan raster with mask-over-source overlay;
orbit 3D view with wireframe, layer clipping and exploded tiles) beside the node's parameters and its own log.
Trace nodes recompute almost live as you drag; the stencil/post stages follow when edits settle, and stale
runs are cancelled. The Console button shows the run log.

## License

[The Unlicense](LICENSE) (public domain), except `webui/static/vendor/`, which is three.js under the MIT license (see `webui/static/vendor/LICENSE`).
