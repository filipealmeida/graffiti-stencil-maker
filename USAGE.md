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

Build and run the studio as described in [INSTALL.md](INSTALL.md). Drop an image onto the canvas, or choose one of
the starter templates. The graph is evaluated in a Web Worker; node outputs can be inspected in the side panel.

The graph is arranged into three stages:

1. **Trace:** Image, Threshold, Reduce colours, Split colours, Tone patterns, Trace, and Shapes to mask.
2. **Stencil:** Stencil accepts a raster mask; Vector stencil accepts traced outlines and builds polygon geometry
   directly. Both support bridge width and island handling. Raised bridges use stepped Z geometry to retain the
   original artwork on the first layer.
3. **Post-process:** Tiles & mounting splits plates, adds optional hardware features, and exports parts as a ZIP.
   The Hardware node can produce fasteners and connector fit-test parts.

Use the Vector template for the traced-outline workflow. Use the Colour template for multi-colour/tone-pattern
workflows. Double-click a node to open its workbench. Save and reopen the graph and its embedded images with the
project controls.

Edits can be undone with **Undo**/**Redo** in the toolbar or Ctrl+Z / Ctrl+Shift+Z (Ctrl+Y). Edits made within
half a second, such as a slider drag, count as one step; the last 100 steps are kept for the session.

## Island handling and output

Normal bridges connect stencil material islands to the frame so the printed part is connected. With raised bridges,
the first layer keeps the unbridged artwork and upper layers grow bridges into contact; this lets paint pass below.
The bridge strategy is selectable in the stencil node. A successful stencil output is checked for remaining islands
and watertightness before export.
