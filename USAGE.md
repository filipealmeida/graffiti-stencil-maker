# Graffiti Stencil Maker – Usage

Turns an image (PNG, JPG, BMP, SVG) into a printable stencil as an STL file.
Dark areas become holes (where paint passes through); use `--invert` for the opposite.
Pieces that would float free ("islands", e.g. the centre of an "O") are tied to the
surrounding frame with narrow bridges, so the result is always one connected,
watertight piece.

## Install

Requires Python 3.10+.

```sh
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
```

SVG support uses `cairosvg`, which needs the Cairo system library (usually present;
on Debian/Ubuntu: `sudo apt install libcairo2`).

## Command line

```sh
.venv/bin/python -m stencil INPUT [-o OUTPUT.stl] [options]
```

If `-o` is omitted, the output is the input name with `.stl`.

| Option | Default | Description |
|---|---|---|
| `--width MM` | 100 | Total plate width in mm (frame included) |
| `--thickness MM` | 2 | Plate thickness before extra top layers; rounded to whole layers |
| `--margin MM` | 8 | Solid frame around the artwork |
| `--bridge MM` | 1.6 | Width of bridges that connect islands |
| `--min-feature MM` | 0.8 | Specks smaller than this are dropped |
| `--threshold 0-255` | auto (Otsu) | Brightness cut-off between paint and stencil |
| `--invert` | off | Light areas become holes instead of dark ones |
| `--smooth MM` | 0 (off) | Smooth the outline vertices into curves instead of pixel steps; sigma in mm along the contour (try 0.3–1) |
| `--raised-bridges` | off | Bridges only occupy the top layers; see below |
| `--layer-height MM` (alias `--bridge-height`) | 0.2 | Layer height, which is also the bridge height: with raised bridges the island grows one step per layer. The web layer slider uses it too |
| `--z-bridging steps\|ramp\|stepramp` | steps | With raised bridges: `steps` grows the island layer by layer; `ramp` is one continuous diagonal slope; `stepramp` is steps joined by short slopes (see below) |
| `--min-island MM2` | 0 (off) | Delete islands (pieces not touching the frame) smaller than this area; they become paint instead of getting a bridge |
| `--max-overhang DEG` | 0 (off) | With raised bridges: each layer may stick out at most layer height × tan(DEG) past the layer below (DEG measured from vertical; 45 = one layer height). Bridges too long for this finish with a flat span in the top layer; the longest is reported as *Longest flat top span* |
| `--flip` | off | Print upside down: the part is rotated 180° about the Y axis (like turning a page), so the layers (and the layer slider, first-layer preview and viewer) follow the printed orientation. Every layer then sits on the one below, so there are no overhangs. The first printed layer appears horizontally mirrored relative to the art |
| `--svg [FILE]` | off | Also write the traced artwork as an SVG in mm (the first layer, in the art's own orientation even with `--flip`; black material, holes cut out). Default name: input name + `.svg` |
| `--pads 0-4` | 0 | Raised pads with a threaded hole, on the frame margin only (to screw a pole into). 1 = top centre, 2 = top + bottom centre, 3 = top corners + bottom centre, 4 = four corners |
| `--pad-thread M4\|M6\|M8\|M10` | M8 | Thread of the pad hole (ISO, right-handed, printed in the part). The pad is thread diameter + 4 mm wide, so the margin must be at least M4 8, M6 10, M8 12, M10 14 mm (the web UI raises the margin for you) |
| `--pad-height MM` | 6 | How far the pad rises above the plate. Plate + pad must be at least 4 thread pitches (M8: 5 mm) |
| `--thread-clearance MM` | 0.2 | Radial clearance added to the hole thread so a printed or metal bolt screws in; raise it if your printer is tight |
| `--extra-top-layers N` | 1 | Extra copies of the final layer stacked on top (adds N × layer height to the thickness) to make the part stronger |
| `--resolution PX` | 250 | Pixels along the longest side (higher = finer, bigger file) |

Examples:

```sh
.venv/bin/python -m stencil logo.png
.venv/bin/python -m stencil tag.svg -o tag.stl --width 150 --thickness 1.5 --bridge 2
.venv/bin/python -m stencil photo.jpg --threshold 110 --invert
```

On success it prints a JSON summary (islands found / bridges added / islands remaining,
watertight, triangle count, size). The exit code is non-zero if any island remains or
the mesh is not watertight.

### Threaded mounting pads
`--pads N --pad-thread M8 --pad-height 6` adds N round pads on the frame margin (never over the artwork), each with a through hole carrying a real internal thread, so a pole or bolt can be screwed in. Pads are on the print-top face, so they print without supports and without breaking the island-free guarantee. The STL gets taller by the pad height, and the layer slider shows the pads only on the last layer. With `--flip` there is no raised pad: you get only the threaded holes through the margin (the plate keeps its thickness and `--pad-height` is ignored), so the thread has only as many turns as the plate is thick divided by the pitch (reported as `pad_thread_turns`; thicken the plate if you need more grip). The hole thread is not counted in the overhang statistic (its flanks are 60° like any ISO thread; print with a 0.2 mm layer height and test-fit first). The SVG export contains the artwork without the pad holes. In the web UI use the *Mounting pads* group.

### Smooth outlines

`--smooth` works on the geometry, not the image: the exact outline polygons of the stencil are traced,
their vertices are Gaussian-smoothed along the contour (sigma in mm), and the smoothed polygons are
extruded. Edges become curves and the STL gets far fewer triangles than a pixel-step model. Larger values
round corners more and erase fine detail. The result is verified to still be one connected piece (if
smoothing would split it, a weaker value is used automatically; the value actually applied is reported
as `smoothing_mm`).

### Raised bridges

Normally bridges are full thickness. With `--raised-bridges` the plate is built in layers of
`--layer-height` mm (default 0.2). On the wall side (z = 0) every island is free-standing; going up,
the island grows one step per layer along its bridge, diagonally toward the part it belongs to, until
on the top layer the bridge touches it. The top layer therefore has no islands, while paint can flow under
the sloped bridge. Works together with `--smooth`.

**Z bridging strategies** (web: *Z bridging* dropdown)

- `steps` (default): the island grows one pixel-step per layer, as described above.
- `ramp`: the first layer keeps the islands and the last layer has none. Each bridge's underside is a single
  continuous diagonal surface (no stair steps) rising from the island at the wall side to the island-free
  top layer. Gives smoother, much lighter meshes.
- `stepramp`: like `ramp`, but the underside rises in layer-high plateaus joined by short diagonal slopes.

In `ramp`/`stepramp` the first layer is identical to the `steps` first layer; bridges only start rising above it.

**Overhangs.** Every run reports *Overhang beyond limit* (area in mm² sticking out more than 45°, or your `--max-overhang`, past the layer below). With raised bridges that area is normally non-zero when printed in the default orientation; `--flip` brings it to 0. The web form fields are `min_island`, `max_overhang` and `flip`.

**Web UI guide**

- *Controls* are grouped (Input, Size, Bridges, Cleanup & printing) and collapsible. Options that need raised bridges (Z bridging, max overhang) are greyed out until it is on. *Presets* (Simple, Printable without overhangs, Smooth curves) and *Reset* set many options at once. Settings are remembered in the browser.
- *Auto-update* regenerates after each change (the delay grows with the last generation time, and a running job is cancelled when a newer one starts). Turn it off to use the *Generate* button, which turns orange when settings changed.
- *SVG button* (next to Download STL): downloads the traced artwork as a vector file in mm.
- *Results card* (bottom left): Island-free / Watertight / Overhang pills. When there are overhangs, **Fix: print upside down** flips the export.
- *Validation*: out-of-range fields (e.g. margin ≥ half the width) turn red with a message and nothing is sent until fixed; invalid stored settings are reset on load.
- *Loading images*: drop, click, paste (Ctrl+V), pick an example, or load from a URL (the remote site must allow cross-origin requests).
- *Viewer*: display mode (Shaded, Overhang heatmap — red = flat roof, orange = under 45° from flat, view it from below — and Bridges), wireframe, camera presets (Top, Iso, Side, Bottom, Fit), a bed grid that shows the printed orientation, and the plate dimensions. The layer bar has previous/next/play buttons. Shortcuts: `←` `→` layer, `Space` play, `T` `I` `S` `B` `F` cameras, `W` wireframe, `?` help.
- *First printed layer* panel: wheel zoom, drag pan, double-click reset. Tick the checkbox to overlay bridges (red) and deleted islands (blue); the *Input / Stencil* slider compares the input image against the first layer.
- *Log*: Copy, Clear, and a *timings only* filter.
- *Panels*: drag the bars to resize, use the arrow buttons (or focus a bar and press Enter / arrow keys) to collapse and reopen; layout is remembered. On small screens the side panel starts collapsed.

**Panels:** drag the thin bars between panels to resize them; click the arrow button on a bar (or double-click the bar) to collapse and reopen that panel. Click the *Input image* / *First layer* headings to fold them.

The *First layer* panel (web UI) supports mouse-wheel zoom, drag to pan and double-click to reset.

```sh
.venv/bin/python -m stencil logo.png --smooth 0.5 --raised-bridges --layer-height 0.2
```

## Web interface

```sh
.venv/bin/python -m stencil --serve [--host 127.0.0.1] [--port 5000]
```

Open <http://127.0.0.1:5000>.

1. Drag and drop an image onto the page (or click the drop area).
2. Adjust threshold, invert, smoothing, raised bridges, width, thickness, margin, bridge width and resolution;
   the stencil regenerates automatically.
3. Explore the 3D model: drag to rotate, scroll to zoom, right-drag to pan.
   (Requires a browser with WebGL.) The **Layers shown** slider hides everything above layer *n*
   (one layer = one layer height, wall side first) so you can watch the islands grow into the bridges.
4. Click **Download STL**.

The page has three extra areas:
- **Log (bottom):** one block per generation with the image name and date/time, the upload time, the
  server time of every stage (reading image, thresholding & bridging islands, building layers, extruding
  & merging layers, checking mesh, writing STL), the server total, the STL download and viewer-parsing
  times, and a final **Total time** line.
- **Right column:** the top half is the input image panel and the bottom half shows the first layer of the stencil (the wall-side layer, z = 0), refreshed after every generation. With raised bridges you can see the free-standing islands there.
- **Input image panel:** a preview of the loaded image with its name, type, file size, last
  modified date, pixel dimensions, aspect ratio and megapixels.

The side panel reports islands found, bridges added, islands remaining, and whether
the mesh is watertight.

## Tips

- Use high-contrast, bold images. Thin lines make fragile stencils; raise `--bridge`
  or `--min-feature` for sturdier results.
- If the wrong part becomes a hole, try `--invert` or adjust `--threshold`.
- Large `--resolution` values (>400) give big STL files and slower generation.
- Transparent PNG/SVG backgrounds are treated as white.

## HTTP API

The web page uses an asynchronous job API so it can show progress: `POST /api/jobs` (same form fields)
returns `{"id": ...}`; poll `GET /api/jobs/<id>` for `{progress (0-1), stage, done, error}`; then fetch
`GET /api/jobs/<id>/stl` (and `/layer.svg` for the first-layer preview, `/trace.svg` for the downloadable traced artwork). When `done`, the status also contains `total_ms`; `events` lists stages as they finish (`since=N` returns only events after the first N). The synchronous endpoint is described below.


`POST /api/stencil` (multipart form) with field `image` plus optional `width`,
`thickness`, `margin`, `bridge`, `resolution`, `threshold`, `invert`, `smooth` (mm), `raised` (1/0), `layer_height` (mm), `z_bridging` (`steps`/`ramp`/`stepramp`), `extra_top` (layers), `min_island` (mm²), `max_overhang` (°), `flip` (1/0). Returns the
binary STL; statistics are in the `X-Stencil-Stats` response header (JSON).
