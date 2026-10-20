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
stencil --project tiled.json --set tile_layout.max_x=60 --set vector_stencil.bridge_mm=2.2 --layout layout.png --report --strict
```

Images can be PNG, JPG, BMP, or SVG (at most 100 MB and 100 megapixels). The CLI prints a `verified:` line with the
island and watertight check results and exits unsuccessfully if stencil validation fails. A single output part is
written as an STL; multi-part exports also produce a ZIP archive. Projects saved from the studio can be run with
`--project`; native builds cut tiles on all CPU cores (set `RAYON_NUM_THREADS` to limit it; output is identical);
picture ids are remapped automatically, including for projects saved by older versions.

Options that only apply to `--project`:

- `--set NODE.PARAM=VALUE` (repeatable) changes a node parameter before the run. NODE is a node id or a node type that
  appears once in the graph (`tile_layout.max_x=60`). Unknown nodes or parameters, out-of-range numbers and bad
  booleans or choices are rejected with a message that lists the valid names.
- `--layout FILE.png` writes the tile layout preview of each *Tile layout* node (paint dark, blocks blue, or orange
  where a block covers paint, seams red). With several layouts the node id is added to the file name. A graph that
  has only a layout and no stencil is accepted when this flag is given.
- `--report` prints the print-readiness verdict (`ready`, `check` or `blocked`), the findings and the slowest nodes.
  `--report-file FILE.json` writes the same report as JSON (`verdict`, `findings`, `timing`). `--strict` exits with an
  error unless the verdict is `ready`, which is useful in scripts. The findings match the badges in the studio.
- `--list-params [NODE]` prints every parameter `--set` accepts (type, default and range or choices) for all node
  types, or for one, and exits. `--params-format markdown` prints it as tables; the result is kept in
  [PARAMETERS.md](PARAMETERS.md).
- `--source FILE` swaps the picture in the project's single Image node.
- `--batch DIR` runs the project once for every picture in DIR (png, jpg, bmp, svg); `--sweep NODE.PARAM=a,b,c`
  (repeatable, combinations are all run) runs it once for every value. With either, `-o` names a folder that gets one
  subfolder per run (STLs, ZIP and `report.json`) and a `summary.csv` (run, status, verdict, findings, seam step, paint under blocks, seconds). The
  report JSON also has a `metrics` object with those numbers, and a sweep names the run with the fewest seam steps. Each
  run is a separate process, so one failing run does not stop the rest; the exit code is non-zero if any failed.
  Other flags (`--set`, `--strict`, `--threads`) apply to every run.

```sh
stencil --list-params tile_layout
stencil --project tiled.json --sweep tile_layout.max_x=40,50,60 -o sweep/
stencil --project tiled.json --sweep tile_layout.seam_shift=0,4,8,12 -o seams/   # which seam position steps least
stencil --project tiled.json --batch pictures/ --strict -o batch/
stencil --project tiled.json --set vector_stencil.tile_method=cut_whole -o cut/   # fewer seam steps, slower
```

**Tile method** (Vector stencil with a Tile layout). `per_tile` (default) bridges each tile on its own. `cut_whole` bridges the
whole plate once, cuts the tiles from it so neighbours meet with the same ramps, and re-bridges only what the cut leaves loose.
On the mounting sandbox it cut seam steps from 78 mm to 12 mm but took about 4× longer (14 s against 3.5 s); on the inverted one
it made no difference (6 mm) and took 8× longer. Loose pieces of 10 mm or less that nothing can hold are dropped and reported as
a warning ("loose pieces dropped where tiles were cut"): paint may reach through there. Larger ones fail the build; use `per_tile`.

## Studio

With a binary built with the `studio` feature, `stencil studio [--port N] [--no-open]` serves the studio locally and
opens it in your browser; everything still runs in the browser tab. Stop it with Ctrl+C.
The **Crop & rotate** node (Trace lane, image in, image out) crops each side by a percentage, flips, turns by 90° steps and rotates freely by up to ±45° (the canvas grows to fit and the corners are white, i.e. no paint). Place it right after the Image node.
The **Text** node (Trace lane, no input, mask out) renders typed text (multi-line, sans or serif bold, size, line spacing, padding) as a mask that can go straight into a Stencil node. Only DejaVu Bold fonts are built in, so there is no stencil-style lettering; the letters' inner islands (O, A, B…) are bridged like any other.
Editing: select nodes and press Ctrl+C / Ctrl+V to copy and paste them (links between copied nodes are kept), Ctrl+D or the ⧉ button on a node to duplicate. The clipboard is internal to the tab. Every parameter has a number box beside its slider, a ↺ button when it differs from the default, and the panel has a "Reset changed settings" button. Dropping a picture on an empty studio builds the Vector template. New stencil nodes start at 200 mm wide, 5 mm thick, mesh detail 5, raised bridges on and easy Z bridging; the Image node works at 1000 px. Selecting an Image node shows its properties (file name, format and size, colour model, dpi and print size where the file stores them, SVG size, viewBox and title, the working size, brightness, the automatic threshold and how many colours it has). The view fits the graph after a load, a drop or a template. The **Templates** menu rebuilds the graph (Simple, Colour tones, Vector) around the current picture. If the app ever fails to draw, a recovery page offers Reload, Download autosave, or Discard autosave.
The 3D viewers (Stencil and Post-process outputs) show the model size (width × depth × height in mm) in the corner, and per-file sizes in the file list; for tiled output the corner shows the assembled plate. The Trace node's output has an **Export SVG** button (workbench footer and inspector) that saves the vector outlines.

Build and run the studio as described in [INSTALL.md](INSTALL.md). Drop an image onto the canvas, or choose one of
the starter templates. The graph is evaluated in a Web Worker; node outputs can be inspected in the side panel.
New nodes are placed in the first free spot, left to right by stage (Trace, Stencil, Post-process).

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
   **Remesh** (formerly *Complexity reducer*; solid in, solid out) goes right after a stencil node, before post-processing. It re-slices the
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
   **Decimation** (on by default, `decimate_mm` = largest surface shift, default 0.02 mm) then collapses vertices on flat faces
   and straight edges by quadric error, inside the layer interval. Creases survive, topology never changes and the mesh stays
   watertight; if a collapse set would break that the step is skipped. On a 307k-triangle plate it gave 225k at 0.02 mm and 142k
   at 0.1 mm. Old projects with the *Complexity reducer* are opened as Remesh.
3. **Post-process:** Tiles & mounting (old) splits plates (the `depth` slider sets how far each connector sinks into each tile, 4–20 mm, default 10; the rod is `2 × depth − 1` mm long and tiles must be at least `depth / 0.4` mm wide), adds optional hardware features, and exports parts as a ZIP.
   Pieces that a seam cuts off from the rest of their tile are bridged again per tile (same bridge width, count and
   raised-bridge settings as the stencil); `bridged_tiles` in the tiling info counts them. If a tile still has loose
   pieces the export fails with a clear error instead of emitting islands.
   The Hardware node can produce fasteners and connector fit-test parts.

Use the Vector template for the traced-outline workflow. Use the Colour template for multi-colour/tone-pattern
workflows. Double-click a node to open its workbench. Save and reopen the graph and its embedded images with the
project controls.

The **+ trace / + stencil / + post** menus add nodes by stage; long menus flow into several columns and scroll
inside themselves when they reach the bottom of the page. **+ All** lists every node, most used first; the badge
(and the tooltip) shows how many times you added that node in this browser. Select an Image node and its side panel has **Open…** (replaces the loaded picture) and **Download** (saves the original picture). The **Perf** button toggles an
overlay with the engine worker count, state and mode (multi-threaded, or single-threaded and why), the share of the last 10 s the engine was computing, how late the page's
timer fires (UI lag) and the CPU core count. Browsers do not expose real CPU use, so those figures are what the page
can measure, and the engine runs in a single worker.

Edits (parameters, nodes, links and swapping an Image node's picture) can be undone with **Undo**/**Redo** in the toolbar or Ctrl+Z / Ctrl+Shift+Z (Ctrl+Y). Edits made within
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

## Tiles, pads and screws (three post nodes)

Set the pad thread to **none** for a plate without pads (mounting holes still work).

Chain **Split volume → Pads & mounting holes → Screws, nuts & fit test** (Templates → *Tiles, pads & screws*). *Split volume* and *Tiles & mounting (old)* are the older post-solid nodes and still work.

- **Tiles:** cuts the stencil into tiles no larger than *max width / height*, as evenly as possible, and joins them with glued connector rods in blind holes at the seams. A seam may move up to *seam may move* mm to cross more material. Where no connector fits, *Add material* puts a block at the seam (it interrupts the artwork there). Pieces a cut leaves hanging get their own bridges (width, count, raised and Z-bridging options), so no tile has islands. Pieces of 4 mm or less that cannot be held are dropped and counted in the node's badge. Every tile is checked watertight and in one piece.
- **Pads & mounting holes:** takes the Tiles output. Pads go on the corners of the margin band and, on edges longer than *Add pads along edges longer than*, evenly between; through-holes follow every *Hole every* mm. Anything that would hit a seam or a connector slides along its edge until it is clear. *Joiner holes* adds a pair of holes either side of each seam that crosses the frame (widened automatically to clear the connectors). The margin must be at least the pad thread plus 4 mm.
- **Margin expansion:** takes the Pads & mounting holes output (needs a margin above zero and a mounting hole screw) and puts a border of blank slabs around the assembled plate: *Border width* wide, as thick as the plate, cut into slabs no longer than *Longest slab*, plus a corner slab at each corner. *Number of rings* (default 1) wraps further borders, each around the one inside it. Slabs are butt-jointed to the plate and to each other with joiner plates (`ring_joiner_<screw>.stl`, one per joint, turned for vertical seams) screwed through holes that mirror the plate's own mounting holes (same screw as the Pads node). The plate's holes along its edge are used by the joints and no longer fix it to a wall; for further rings the matching holes are added to the ring inside (*Hole every* mm). *Mounting holes along the outer edge* adds wall holes to the outermost ring. A slab left without a screw hole is an error.
- **Screws, nuts & fit test:** bolt, nut and the connector fit-test strip. Connect it after Pads & mounting holes and it also prints a joiner and an extender plate sized for the mounting screw, to bridge the seams over the frame.
- **Layout overlay:** open any of these nodes (double-click) and the 3D viewer draws seams (yellow), connector footprints (blue), pads (green), mounting holes (red) and joiner-plate lines (purple) on each tile, and they follow the tiles when you change the spacing. Untick *layout* to hide them.

### Tiling before bridging (Templates → *Tiles first*)

Chain **Tile layout → Stencil (or Vector stencil) → Pads → Screws**. The mask (or traced shapes) goes to both Tile layout and the stencil node; the layout's `tiles` output goes to the stencil's *tiles* input.

- **Tile layout:** fixes the plate width and margin and cuts it on an even grid no larger than *max width / height*. Along each seam it picks connector spots with the least paint under them and reserves a solid block there (end wall included), so connector holes never need extra material afterwards. The preview shows paint (dark), blocks (blue) and seams (red). Tiles narrower than the connector depth allows are refused.
- **Stencil / Vector stencil with *tiles* connected:** runs once per tile, so every tile is bridged on its own and is island-free by construction, and bridges may reach the seam edge. The plate width and margin come from the layout (the stencil's own width and margin are ignored). A layout made from a mask works with Stencil, one made from shapes with Vector stencil. The *parts* output feeds Pads and Screws unchanged. Connector holes are drilled per tile and each tile is checked watertight and in one piece.
- **Seam may move:** each inner seam may shift up to this many mm off the even grid, to wherever the pockets that then fit cover the least paint. Tiles never grow past the largest size, so a plate that divides exactly has no room to move. Pockets at the plate edge sit in the margin band, where they cost no artwork.
- **Cost preview:** blocks that cover paint are drawn orange in the layout preview, and the Tile layout node's badge and the workbench footer show the mm² of paint under blocks and its share of the artwork (a warning above 5%), so you can compare layouts before running the stencil.
- **Trade-offs:** blocks cost some artwork (the badge shows the mm² of paint under blocks), and neighbouring tiles bridge independently, so a bridge may differ across a seam. The Tiles badge also reports how many mm of seam have material on both sides at heights more than a layer apart (a bridge ramp meeting a tile at full height), so you can compare layouts. A tile that needs it falls back to sideways bridges (shown in the badge). It is also faster than tiling afterwards (about 5–7 s versus 17–19 s on the sandbox projects). The older Tiles node remains for comparison.

- **Report panel** (toolbar → *Report*): the print-readiness verdict for the whole graph (`ready`, `check` or `blocked`), every finding with the node it came from (click one to select the node), and the time each node took (or *cached*). The same checks run in the CLI with `--report`; they live in `web/src/badges.ts` and `crates/stencil-core/src/report.rs` and must be changed together.
- **Problem highlight and counts**: a node with an error finding gets a red outline, one with only warnings an amber one, and the *Report* button shows the totals (`✖` errors, `▲` warnings). A node whose result is being recomputed has a dashed outline.
- **History** (toolbar): lists every edit in order ("Add Morphology", "Edit Stencil: bridge_mm", "Connect nodes", ...); pick one to jump straight to that state. Undo and redo still work as before.
- **Presets** (Inspector, under the parameters): *Save as…* stores the node's parameters under a name in this browser; choosing a preset applies every parameter it holds. Tile layout also has a built-in *Mounting challenge* preset (100 mm wide, 10 mm margin, 50 mm tiles). *Export* writes all your presets to `stencil-presets.json` and *Import* adds those from such a file (a name you already have is kept), so presets can be shared or backed up.
- **Rerun** (`Ctrl+Enter`): runs the whole graph again; unchanged nodes come from the cache.
- **Live preview**: while a slider is dragged a coarse draft (lower resolution, easy bridging, low vector detail) runs; releasing the slider runs the full quality pass. The status line says "draft preview" until then.
- **Progress and ETA**: each running node shows its message, elapsed time and an estimated time left; the top status line is announced to screen readers.
- **Badges** on nodes flag problems (islands remaining, not watertight, bridges under 1 mm, empty/full masks, heavy meshes).
- **Tooltips and units**: hover or focus a slider for a short explanation; ↺ resets it.
- **Performance**: the vector stencil uses a spatial index for bridge selection and near-contact repair and stops repairing once the mesh is watertight (a 287-island project: 19 s → 2.8 s easy, ~73 s → 10 s steps). Noding only re-checks edges a repair created, and the watertight check runs once. The CLI runs contours in parallel; in the browser, if a threaded WASM package is available and shared memory is allowed, the engine uses half the cores and leaves at least one free, else it falls back to one worker.
- **Fix**: *Delete islands under (mm²)* in the vector stencil now really uses mm².

- **Command palette**: press `Ctrl+K` (or the `⌘K` button) and type a node or template name to insert it quickly.
- **3D viewer controls**: `shading` switches between smooth and flat normals, and `detail` can force high/low detail (or auto, which shows low detail while orbiting and high detail when still).
- **Threaded WebAssembly**: when `web/public/pkg-mt/` is built and the page is cross-origin isolated, the engine uses a worker pool sized to half the CPU cores and never all cores; otherwise it falls back to one worker automatically.
