# Usage

The project provides a Rust CLI and a browser-based node-graph studio. Both use the same Rust stencil engine.

## Command line

Build the CLI with `cargo build --release -p stencil-cli`, then run:

```sh
./target/release/stencil INPUT [OPTIONS]
```

Windows: `.\target\release\stencil.exe`. Use `stencil --help` for the complete, current flag list.

Examples:

```sh
stencil logo.png -o logo.stl
stencil artwork.svg --width 120 --thickness 3 --margin 10 --bridge 2 -o plate.stl
stencil artwork.svg --vector --bridges-per-island 3 -o plate.stl   # three bridges for every island
stencil portrait.jpg --threshold 110 --invert
stencil logo.png --vector --detail 4
stencil tiled.png --tile-width 60 --tile-height 60 --connector hex --pads 4 --pad-thread M6
stencil --project saved-studio-graph.json -o output/
```

Images can be PNG, JPG, BMP, or SVG (at most 100 MB and 100 megapixels). The CLI prints a `verified:` line with the
island and watertight check results and exits unsuccessfully if stencil validation fails. A single output part is
written as an STL; multi-part exports also produce a ZIP archive. Projects saved from the studio can be run with
`--project`; native builds cut tiles on all CPU cores (set `RAYON_NUM_THREADS` to limit it; output is identical);
picture ids are remapped automatically, including for projects saved by older versions.

## Studio

With a binary built with the `studio` feature, `stencil studio [--port N] [--no-open]` serves the studio locally and
opens it in your browser; everything still runs in the browser tab. Stop it with Ctrl+C.
The **Crop & rotate** node (Trace lane, image in, image out) crops each side by a percentage, flips, turns by 90° steps and rotates freely by up to ±45° (the canvas grows to fit and the corners are white, i.e. no paint). Place it right after the Image node.
The **Text** node (Trace lane, no input, mask out) renders typed text (multi-line, sans or serif bold, size, line spacing, padding) as a mask that can go straight into a Stencil node. Only DejaVu Bold fonts are built in, so there is no stencil-style lettering; the letters' inner islands (O, A, B…) are bridged like any other.
Editing: select nodes and press Ctrl+C / Ctrl+V to copy and paste them (links between copied nodes are kept), Ctrl+D or the ⧉ button on a node to duplicate. The clipboard is internal to the tab. Every parameter has a number box beside its slider, a ↺ button when it differs from the default, and the panel has a "Reset changed settings" button. The view fits the graph after a load, a drop or a template. The **Templates** menu rebuilds the graph (Simple, Colour tones, Vector) around the current picture. If the app ever fails to draw, a recovery page offers Reload, Download autosave, or Discard autosave.
The 3D viewers (Stencil and Post-process outputs) show the model size (width × depth × height in mm) in the corner, and per-file sizes in the file list; for tiled output the corner shows the assembled plate. The Trace node's output has an **Export SVG** button (workbench footer and inspector) that saves the vector outlines.

Build and run the studio as described in [INSTALL.md](INSTALL.md). Drop an image onto the canvas, or choose one of
the starter templates. The graph is evaluated in a Web Worker; node outputs can be inspected in the side panel.
New nodes are placed in the first free spot of their lane (Trace / Stencil / Post-process). Drag the grip on a lane's right edge to resize it; widths are remembered between visits. Existing nodes do not move when a lane is resized.

The graph is arranged into three stages:

1. **Trace:** Image, Threshold, Reduce colours, Split colours, Tone patterns, Morphology, Smooth mask (blur + supersample for curved edges), Trace, **Round corners**, **Remove islands**, and Shapes to mask.
   Round corners and Remove islands take traced shapes and give shapes, so they sit between Trace and Vector stencil.
   *Round corners* replaces corners sharper than the chosen angle with a tangent arc of the chosen radius (px); it rounds paint tips
   (losing material) and, optionally, empty spikes (gaining it), and its `rounded` output previews the cut-off tips.
   *Remove islands* deletes paint specks and/or fills holes smaller than an area (px²), together with anything nested inside,
   and can keep only the N largest shapes; its `removed` output shows what was dropped. Filling a hole removes a stencil island
   at the source, so fewer bridges are needed.
2. **Stencil:** Stencil accepts a raster mask; Vector stencil accepts traced outlines and builds polygon geometry
   directly. Both support bridge width, bridges per island (1–5) and island handling. Raised bridges use stepped Z geometry to retain the
   original artwork on the first layer (Vector stencil builds them from vectors, so curves stay smooth; mesh detail goes up to 20).
   **Minimum surface contact** (mm, default 0 = off; CLI `--min-contact`): a strip of material narrower than this that
   joins two different parts of the stencil (a thin "rope" the printer could not lay down on the build plate) is
   widened to the value, over the full height. Dead-end tips are left alone, and the widening grows into the paint
   side only, so the paint area shrinks slightly there.
   **Complexity reducer** (solid in, solid out) goes right after a stencil node, before post-processing. It re-slices the
   STL into layers (layer 1 is on the plate; the layer height and the layer count come from the input stencil, and the
   interval `layer_from`–`layer_to` defaults to the real maximum, stored as `0`), then smooths and simplifies the outlines
   and rebuilds a watertight mesh. Outline chains shared between layers are simplified once, so layers stay consistent.
   `tolerance_mm` is the allowed outline deviation and `smoothing` (0–1) rounds the outline before it is simplified;
   smoothing helps little on already very dense outlines. With `keep_material` the outline may grow by up to the tolerance
   but loses at most a quarter of it ("never thinner"). `max_overhang_deg` (90 = no limit) keeps each layer's growth over the
   one below within that angle. Layers outside the interval are unchanged, so a partial interval saves few vertices.
   If the rebuild is not watertight the tolerance is halved (twice) before giving up. If nothing works, or nothing is saved,
   the input passes through unchanged and the stats show `skipped` with the reason. `max_deviation_mm` is an upper bound.
   The tolerance must be well above the outline point spacing to remove anything. Only vertical-walled plates are supported.
3. **Post-process:** Tiles & mounting splits plates (the `depth` slider sets how far each connector sinks into each tile, 4–20 mm, default 10; the rod is `2 × depth − 1` mm long and tiles must be at least `depth / 0.4` mm wide), adds optional hardware features, and exports parts as a ZIP.
   Pieces that a seam cuts off from the rest of their tile are bridged again per tile (same bridge width, count and
   raised-bridge settings as the stencil); `bridged_tiles` in the tiling info counts them. If a tile still has loose
   pieces the export fails with a clear error instead of emitting islands.
   The Hardware node can produce fasteners and connector fit-test parts.

Use the Vector template for the traced-outline workflow. Use the Colour template for multi-colour/tone-pattern
workflows. Double-click a node to open its workbench. Save and reopen the graph and its embedded images with the
project controls.

The **+ trace / + stencil / + post** menus add nodes by stage; long menus flow into several columns and scroll
inside themselves when they reach the bottom of the page. **+ All** lists every node, most used first; the badge
(and the tooltip) shows how many times you added that node in this browser. An Image node has **Open new…** (adds
another Image node for a different picture) and **Download** (saves the original picture). The **Perf** button toggles an
overlay with the engine worker count and state, the share of the last 10 s the engine was computing, how late the page's
timer fires (UI lag) and the CPU core count. Browsers do not expose real CPU use, so those figures are what the page
can measure, and the engine runs in a single worker.

Edits can be undone with **Undo**/**Redo** in the toolbar or Ctrl+Z / Ctrl+Shift+Z (Ctrl+Y). Edits made within
half a second, such as a slider drag, count as one step; the last 100 steps are kept for the session.

Each settled edit starts one run, which evaluates the graph node by node; results appear as nodes finish and older
results stay visible (marked stale) until replaced. A new edit stops the run in progress at the next node boundary. If a
node has been computing for more than about 300 ms, the worker is restarted instead, which stops it at once (upstream nodes are
then recomputed). Long nodes (Stencil, Vector stencil, Smooth mask, Tiles & mounting) report their phases: the console shows
`↳ phase` lines with the time each took, the node's bar shows the current phase, and the top progress bar advances by the
fraction of the running node. The tile cutting phase is one step, because it runs on several threads. To run the browser tests: `cd web && npm run build && npx playwright install chromium-headless-shell && npm run e2e`.

## Island handling and output

Normal bridges connect stencil material islands to the frame so the printed part is connected. With raised bridges,
the first layer keeps the unbridged artwork and upper layers grow bridges into contact; this lets paint pass below.
The bridge strategy is selectable in the stencil node. A successful stencil output is checked for remaining islands
and watertightness before export.

## Colour sets, live preview and feedback

- **Colour set node** (post stage, or Templates → *Colour set*): takes up to six solids (one per colour) of the *same* plate size and drills matching registration holes (0–4, diagonal / L / corners) through every plate, so layers line up. With *pins* on it also outputs a pin STL sized to the holes. Holes must fit inside the margin, or the node reports why. Output: one STL per colour plus pins, exportable as a ZIP.
- **Live preview**: while a slider is dragged a coarse draft (lower resolution, easy bridging, low vector detail) runs; releasing the slider runs the full quality pass. The status line says "draft preview" until then.
- **Progress and ETA**: each running node shows its message, elapsed time and an estimated time left; the top status line is announced to screen readers.
- **Badges** on nodes flag problems (islands remaining, not watertight, bridges under 1 mm, empty/full masks, heavy meshes).
- **Tooltips and units**: hover or focus a slider for a short explanation; ↺ resets it.
- **Performance**: the vector stencil uses a spatial index for bridge selection and near-contact repair and stops repairing once the mesh is watertight (a 287-island project: 19 s → 2.8 s easy, ~73 s → 13 s steps). The CLI also runs contours in parallel; the browser engine is single-threaded.
- **Fix**: *Delete islands under (mm²)* in the vector stencil now really uses mm².
