# Installation

The current version is written in Rust, with a React/Vite web studio and a WebAssembly engine.

## Requirements

| Tool | Required for | Notes |
|---|---|---|
| Rust stable and Cargo | CLI and engine | Install with [rustup](https://rustup.rs/) |
| `wasm32-unknown-unknown` target | Building the studio engine | Installed through `rustup target add wasm32-unknown-unknown` |
| `wasm-pack` | Building the WebAssembly package | Install from the [wasm-pack guide](https://rustwasm.github.io/wasm-pack/installer/) |
| Node.js 22 and npm | Building or developing the studio | The lockfile is tested with Node 22 |

## Build the CLI

From the repository root:

```sh
cargo build --release -p stencil-cli
```

The executable is `target/release/stencil` on Linux/macOS and `target/release/stencil.exe` on Windows.

## Build and run the web studio

```sh
rustup target add wasm32-unknown-unknown
cd crates/stencil-wasm
wasm-pack build --release --target web --out-dir ../../web/pkg
cd ../../web
npm ci
npm run build
npm run dev
```

Vite prints the local studio URL. The built static site is in `web/dist/` and can be deployed to a static host,
including GitHub Pages. The CI workflow builds the WASM package before building the web app.

## Build the single-binary studio

`stencil studio` serves the web studio from inside the CLI executable, so one file is all you need to run it.
The studio must be built first (the steps above, ending with `npm run build`), because `web/dist/` is embedded at
compile time:

```sh
cargo build --release -p stencil-cli --features studio
./target/release/stencil studio            # opens http://127.0.0.1:5173/
./target/release/stencil studio --port 8080 --no-open
```

The server only listens on `127.0.0.1`. Without `--features studio` the command prints an error explaining how to
rebuild. The executable can be copied anywhere; it has no runtime dependencies. Prebuilt binaries for Linux, macOS
and Windows are attached to GitHub releases when a `v*` tag is pushed.

## Tests

Run the Rust workspace tests from the repository root:

```sh
cargo test --release --workspace
```
