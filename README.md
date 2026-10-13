# Graffiti Stencil Maker

Create island-free, watertight stencil STLs from PNG, JPG, BMP, or SVG images. The Rust engine powers both a
command-line tool and a browser-based node-graph studio. The studio runs locally or on a static host; image
processing happens in WebAssembly in your browser.

## Quick start

### CLI

Install the [Rust toolchain](https://www.rust-lang.org/tools/install), then run:

```sh
cargo build --release -p stencil-cli
./target/release/stencil picture.svg --width 120 --thickness 3 -o plate.stl
```

On Windows, use `.\target\release\stencil.exe`. Run `./target/release/stencil --help` for all options.

### Web studio

See [INSTALL.md](INSTALL.md) for prerequisites and the initial build. Once the WASM package is built, start the
studio for local development with:

```sh
cd web
npm run dev
```

Drop an image onto the canvas, connect trace and stencil nodes, and export an STL or tiled ZIP. See [USAGE.md](USAGE.md)
for the CLI flags and studio workflow.

## Features

- Raster and direct polygon-vector stencil generation, with automatic island bridges and watertightness checks.
- Image thresholding, colour reduction and splitting, tone patterns, and editable traced curves.
- Optional raised bridges that preserve islands on the first layer.
- Tiling, connector rods, threaded pads and mounting holes, joiner/extender plates, and ZIP export.
- Real M3, M4, M5, M6, M8 and M10 thread geometry.
- SVG input and saved graph projects in the CLI.

## Workspace

| Path | Purpose |
|---|---|
| `crates/stencil-core/` | Rust geometry engine, graph, raster/vector stencils, tiling, and hardware |
| `crates/stencil-cli/` | `stencil` command-line application |
| `crates/stencil-wasm/` | WebAssembly bindings |
| `web/` | React node-graph studio and Web Worker |
| `.github/workflows/ci.yml` | Tests, builds, and GitHub Pages deployment |

The Python implementation remains available in the historical [`v1.0.0` release](https://github.com/filipealmeida/graffiti-stencil-maker/tree/v1.0.0).

## License

[The Unlicense](LICENSE).
