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
| `--smooth MM` | 0 (off) | Smooth the outline vertices into curves instead of pixel steps; sigma in mm along the contour (try 0.3–1; up to 10) |
| `--raised-bridges` | off | Bridges only occupy the top layers; see below |
| `--layer-height MM` (alias `--bridge-height`) | 0.2 | Layer height, which is also the bridge height: with raised bridges the island grows one step per layer. The web layer slider uses it too |
| `--bridges N` | 1 | Number of bridges (1–6) from every island to the rest of the stencil. The extra bridges take the shortest free path that stays clear of the other bridges, so they leave in different directions. An island with no room (very small, or enclosed) gets as many as fit; the result card's *Bridges added* shows the total. Works with every Z bridging strategy and with tiles |
| `--z-bridging steps\|ramp\|stepramp\|grow\|island` | steps | With raised bridges: `steps` grows the island layer by layer; `ramp` is one continuous diagonal slope; `stepramp` is steps joined by short slopes; `island` grows each island sideways at the overhang angle until its layer area reaches 2 mm², then only its bridges keep growing (also sideways, up to 3× their width) so they reach the frame by the top layer; `easy` runs the islands the full height and the bridges only the half away from the wall, so paint fills under the bridges (print it `--flip` to avoid overhangs); `grow` widens islands and bridges in X and Y (see below) |
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

- `island` (Island-bridge): the first layer is exactly the thresholded image, with no bridges. Bridges only start from layer 2, rising from the island towards the rest of the stencil in steps joined by short slopes (like `stepramp`). Islands only grow vertically, keeping their outline; each bridge widens sideways, in X and Y, as it rises, at the overhang angle (45° or `--max-overhang`), up to three times its base width (one bridge width added on each side), which gives it a broad, strong attachment. Bridges still get the rounded root fillet, and flaring never crosses a tile seam. Needs at least 3 layers.
- `grow`: islands and bridges also widen sideways, in both X and Y, as they rise: every flank is a chamfer that never overhangs more than 45° (or `--max-overhang`). It is built as one smooth sloped surface (not a stack of layers), so it is fast. The first layer is exactly the thresholded image; each island flares out from layer 2, and a bridge sits as low as the 45° limit allows (starting at the island/frame wall and climbing at the limit angle), which makes it thick and strongly attached. Bridges also get a rounded, flared root (fillet): near each end, up to half a bridge width is added on both sides, tapering to nothing over two bridge widths. A bridge longer than twice the height it can climb in the plate thickness still needs a flat span in the middle (reported as the flat span); a thicker plate or smaller layer height gives more reach. Needs at least 3 layers.

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

*Live threshold preview:* moving the Threshold slider or toggling Invert instantly re-thresholds the input in the browser, over the artwork area of the *First layer* panel. It is only an approximation (no bridges, smoothing or islands handling); the real first layer replaces it when the generation finishes. It appears once a first generation exists for the loaded image.

```sh
.venv/bin/python -m stencil logo.png --smooth 0.5 --raised-bridges --layer-height 0.2
```

## Tiles and connectors

For plates larger than the printer bed: `--tile-max 200x150` cuts the plate into equal tiles no larger than that and writes `<output>_tiles.zip` (tiles `r<row>c<col>`, row 1 on top). Mounting pads are slid along the frame so none is cut by a seam. Island bridging is done per tile (after the seams are placed), so every tile is a single connected piece that holds together on its own.

`--connector-diameter 2.5|3|4|5|6` (mm; hexagons are measured across the corners; with `--connector-shape hex|round`, default `hex`) drills blind holes into both sides of each seam, aiming for one every 30 mm (never planned further apart than 40 mm) and stacked vertically when the plate is thick enough. The default hole diameter is 0.2 mm larger than the rod diameter; adjust this with `--connector-clearance` (diametral difference). Every internal seam gets at least one connector through the outer frame margin; if that cannot fit safely, generation reports an error instead of silently leaving the frame unconnected. With connectors, seams may shift by up to 8 mm (tiles stay within the size limit) to where the plate has most material; a hole that does not fit is moved along the seam and, as a last resort, drilled with a 0.6 mm wall instead of 1 mm. The holes are inside the plate, so the main STL (and the 3D preview) shows the tiles 10 mm apart to make the seam holes visible; the real, unspaced tiles are in the zip. The result card reports how many seams have a margin connector and the largest gap between connectors along a seam. Holes are only drilled where at least 1 mm of material surrounds them, the plate must be thicker than 4 mm, and each diameter needs a plate of at least `diameter + 2` mm (`0.87 * diameter + 2` for hexagons). Glue a rod into each hole pair; the zip contains the matching rod, and all rods (19 mm long, standing on the bed) are in `toolbox/stls/connectors/` (rebuild with `python toolbox/build_connectors.py`) and downloadable from the web interface.

## Web interface

```sh
.venv/bin/python -m stencil --serve [--host 127.0.0.1] [--port 5000]
```

Open <http://127.0.0.1:5000>.

1. Drag and drop an image onto the page (or click the drop area).
2. Adjust threshold, invert, smoothing, raised bridges, width, thickness, margin, bridge width and resolution;
   the stencil regenerates automatically.
3. Explore the 3D model: drag to rotate, scroll to zoom, right-drag to pan.
   (Requires a browser with WebGL.) The **Layers shown** range slider has two handles: the left one is the first layer shown (default 1) and the right one the last (default: top), so you can show any range, e.g. only the mid layers
   (one layer = one layer height, wall side first) and watch the islands grow into the bridges. ◀ ▶ ▶| and play move the right handle.
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

## Stencil studio (node-graph interface)

The studio is a newer, node-based front end (FastAPI backend, React frontend). The legacy single-page
interface above and the command line keep working.

### Run it

```sh
.venv/bin/pip install -r requirements.txt
(cd app/web && npm install && npm run build)     # once, and after frontend changes
.venv/bin/python -m app --port 5056              # open http://127.0.0.1:5056
```

For frontend development run `python -m app` plus `npm run dev` in `app/web` (port 5173, proxies `/api`).
The 3D views need a browser with WebGL.

### The graph

The canvas has three lanes, left to right: **1 · Trace** (image to masks), **2 · Stencil** (mask to a 3D plate with
bridges) and **3 · Post-process** (export). Data flows along the edges; ports are typed (image, mask, solid, parts)
and only compatible ports connect. A mask means *painted = hole in the stencil*. Branches give multi-stencil sets.

- **Start:** drop a picture on the canvas (or **Open image…**) for a starter graph, or press **Colour template**
  to rebuild the graph as Image → Reduce colours → Tone patterns → Stencil → Export from the current image.
- **Add nodes:** the **+ trace / + stencil / + post** header menus; a menu closes after you pick a node
  (also on outside click or Esc).
- **Delete:** the × on a node header, or select an edge and press its delete button (Delete/Backspace also work).
- **Edit parameters:** select a node and use the Inspector on the right, or open its workbench.
- **Save / load:** the name box plus **Save** writes to `projects/`; **Open project…** loads one; **New** clears
  the graph. The current graph is also kept in the browser between reloads.
- **Running:** trace nodes recompute about 100 ms after each change (almost live); the Stencil and Export nodes
  follow about 0.7 s after edits settle, and stale runs are cancelled. Each node shows a status dot, a progress bar
  and, in the header, a global progress bar. **Console** shows the run log: every line starts with an ISO 8601 timestamp (local time with offset, ms precision) and activity lines end with the time they took in ms. **Export** saves the shown lines as a `.log` text file; **Clear** empties it.

### Workbench (double-click a node)

A full-size preview with the node's parameters and its own log beside it:
- **Image/mask outputs:** zoom/pan raster; **over source** overlays the mask on the source image (the setting is remembered).
- **3D outputs (solid):** orbit view (drag, scroll, right-drag), wireframe, a layer-clip slider, exploded tiles,
  Iso/Top camera buttons, stats (islands, bridges, watertight) and STL / tiles downloads.
- Press Esc to close.

### Nodes

| Node | Stage | Purpose |
|---|---|---|
| Image | trace | The picture, resized to a working resolution (longest side, px). |
| Threshold | trace | Dark areas become holes (256 = automatic Otsu threshold); **Invert** swaps them. |
| Reduce colours | trace | k-means in Lab space to 2–10 flat colours (optional pre-blur). |
| Tone patterns | trace | Turns each colour of a reduced image into a pattern (below). |
| Stencil | stencil | The stencil engine: plate width/thickness/margin, flip, bridge width and count, raised bridges, Z-bridging strategy (steps, ramp, stepramp, island, grow, easy), layer height, smoothing. Outputs the whole-plate solid and a bridge map. |
| Tiles & mounting (id `export`) | post | Tiling, seam connector rods, threaded pads, frame mounting holes, joiner and extender plates (below). Outputs the parts (downloadable) and a solid for the 3D preview. With nothing set it passes the plate through. |

#### Tone patterns

Takes a reduced image with 2–10 colours (one tone per colour) and produces a mask.
- Colours are ranked by lightness; the darkest is **solid** paint, the lightest stays **empty**, and the tones
  between get patterns that thin out step by step. **Invert** flips the order.
- **Default pattern type** (auto, lines, dots, crosshatch, checker) and **default spacing (mm)** apply to every
  tone left on "auto"/0. Default spacing 0 turns those tones solid. *auto* uses lines at coverage ≥ 50 %, dots below.
- Each tone has an override for **type**, **spacing** (mm, 0 = default) and **coverage** (%, 0 = automatic).
  Unused tone rows are ignored.
- Set **Plate width** to the Stencil node's width so spacing in mm is true.
- Connected patterns (lines) keep the material in one piece; dots, crosshatch and checker leave islands that the
  Stencil node bridges. For prints made upside down use *raised bridges* with the *easy* Z-bridging strategy.

#### Tiles & mounting

Tiling, pads and holes live in the Post stage, so changing them never recomputes the stencil. The node rebuilds the
plate from the Stencil node's mask and settings, and bridges every tile separately so each tile is one piece.
- **Tiles:** tile width/height (mm) and optional connector rods, as in the command line `--tile-max` / `--connector-*`.
- **Pads:** 0–4 threaded pads (M4–M10) in the frame, as `--pads`.
- **Mounting holes** (M3/M4/M5, diameter plus clearance): through-holes in the middle of the frame band, only on tiles
  that have frame: one in each plate corner, one on each side of every seam that crosses the frame (distance from
  the seam is **Hole distance from seam**), and one in the middle of long frame segments. Interior tiles with no frame
  get none. Holes avoid pads, and the margin must be at least screw + clearance + 3 mm (otherwise the node says so).
- **Joiner and extender plates:** when a seam has a hole pair, the zip also contains `joiner_2hole_<screw>.stl`
  (two holes, one screwed into each tile across the seam) and `extender_4hole_<screw>.stl` (the same two holes plus
  two more **Extender outer holes** mm outward, to attach extra margin, a wall mount or a clamping rod). The node log
  tells how many joints there are; print one plate per joint.
- **Cost:** the Stencil node still builds a whole-plate preview, and this node builds the plate again with tiling, so a
  tiled export costs about one extra build plus a small per-tile overhead (roughly 1.2–2× the build time for a 3×3 split).
  Holes add little time but more triangles.

### HTTP API (studio)

`GET /api/node-types`, `POST /api/images`, `POST /api/run` (returns a job), `GET /api/jobs/{id}`,
`POST /api/jobs/{id}/cancel`, `GET /api/artifact/{key}/{output}`, `GET /api/info/{key}/{output}`,
`GET /api/part/{key}/{output}/{name}`, `GET|PUT /api/projects[/{name}]`.

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
`thickness`, `margin`, `bridge`, `bridges` (1-6), `resolution`, `threshold`, `invert`, `smooth` (mm), `raised` (1/0), `layer_height` (mm), `z_bridging` (`steps`/`ramp`/`stepramp`/`grow`/`island`), `extra_top` (layers), `min_island` (mm²), `max_overhang` (°), `flip` (1/0). Returns the
binary STL; statistics are in the `X-Stencil-Stats` response header (JSON).
