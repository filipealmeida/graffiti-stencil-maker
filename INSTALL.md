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
# Optional threaded engine (nightly; falls back to 1 thread when unavailable)
RUSTFLAGS='-C target-feature=+atomics,+bulk-memory,+mutable-globals -C link-args=--shared-memory -C link-args=--import-memory -C link-args=--max-memory=4294967296 -C link-args=--export=__wasm_init_tls -C link-args=--export=__tls_size -C link-args=--export=__tls_align -C link-args=--export=__tls_base'   rustup run nightly wasm-pack build --release --target web --out-dir ../../web/public/pkg-mt --out-name stencil_wasm   -- --features threads -Z build-std=panic_abort,std || true
cd ../../web
node scripts/patch-mt-worker.mjs
npm ci
npm run build
npm run dev
```

Vite prints the local studio URL. The built static site is in `web/dist/` and can be deployed to a static host,
including GitHub Pages. The CI workflow builds the WASM package before building the web app.

## Host the studio on your own web server (any sub-folder)

The studio is a static site; everything runs in the visitor's browser, nothing is uploaded. The build uses relative
paths, so it works from the site root or from a sub-folder such as `https://example.com/apps/stencil/`.

1. Build it (needs Rust, `wasm-pack`, Node 22; see Requirements). From the repository root:
   ```bash
   cd crates/stencil-wasm
   wasm-pack build --release --target web --out-dir ../../web/pkg
   # optional, faster multi-threaded engine (needs the nightly toolchain and rust-src)
   RUSTFLAGS='-C target-feature=+atomics,+bulk-memory,+mutable-globals -C link-args=--shared-memory -C link-args=--import-memory -C link-args=--max-memory=4294967296 -C link-args=--export=__wasm_init_tls -C link-args=--export=__tls_size -C link-args=--export=__tls_align -C link-args=--export=__tls_base' \
     rustup run nightly wasm-pack build --release --target web --out-dir ../../web/public/pkg-mt --out-name stencil_wasm -- --features threads -Z build-std=panic_abort,std
   cd ../../web && npm ci && node scripts/patch-mt-worker.mjs && npm run build
   ```
   The site is now in `web/dist/` (`index.html`, `assets/`, `pkg-mt/`). Skip the threaded step to get a smaller site that
   runs on one thread.
2. Copy the **contents** of `web/dist/` into the folder you serve, for example:
   ```bash
   rsync -av --delete web/dist/ user@server:/var/www/html/apps/stencil/
   ```
3. Make sure the server sends `.wasm` files as `application/wasm` and `.js` as `text/javascript` (most do).
4. Optional, for the multi-threaded engine: send these two headers on **every** response of the studio folder
   (the page, scripts and `.wasm`), and serve over HTTPS (or `localhost`):
   ```
   Cross-Origin-Opener-Policy: same-origin
   Cross-Origin-Embedder-Policy: require-corp
   ```
   - nginx: inside `location /apps/stencil/ { ... }` add `add_header Cross-Origin-Opener-Policy same-origin always;`
     and `add_header Cross-Origin-Embedder-Policy require-corp always;`
   - Apache (`mod_headers`), in the folder's `.htaccess`: `Header set Cross-Origin-Opener-Policy "same-origin"` and
     `Header set Cross-Origin-Embedder-Policy "require-corp"`
   - Static hosts that cannot set headers (GitHub Pages) run the single-threaded engine; everything still works, only
     slower on big images.
   The studio's Perf panel shows whether the threaded engine started.
5. Open `https://your-server/apps/stencil/` (keep the trailing slash). Drop an image to check it works. In the browser
   dev tools, no request should return 404, and `crossOriginIsolated` should be `true` if you set the headers.

Notes: a page embedded in an `<iframe>` from another origin, or loading third-party scripts, needs the headers'
counterparts (CORP/CORS) and may fall back to one thread. Rebuild and re-copy after every update.

## Release downloads and the launcher

Each release zip (`stencil-linux-x64`, `stencil-macos-arm64`, `stencil-windows-x64`) holds `stencil` (the CLI with the
studio built in) and `stencil-studio`, a launcher. Double-click `stencil-studio` (or run it, optionally with a port such
as `stencil-studio 8080`): it starts the studio next to it on a free port and opens your browser. Close its window to
stop it. Build it yourself with `cargo build --release -p stencil-cli --features studio -p stencil-launcher`.
The binaries are unsigned, so macOS Gatekeeper or Windows SmartScreen may ask you to confirm the first run.

## Build the single-binary studio

`stencil studio` serves the web studio from inside the CLI executable, so one file is all you need to run it.
The studio must be built first (the steps above, ending with `npm run build`), because `web/dist/` is embedded at
compile time. The server gzips text and WASM (the 3.7 MB engine travels as about 1.6 MB), serves the hashed build
files with a one-year cache and the rest with ETag revalidation, so reloads reuse the engine from the browser cache:

```sh
cargo build --release -p stencil-cli --features studio
./target/release/stencil studio            # opens http://127.0.0.1:5173/
./target/release/stencil studio --port 8080 --no-open
```

The server only listens on `127.0.0.1`. It serves COOP/COEP headers so the threaded engine can run when `web/public/pkg-mt/`
is present. Without `--features studio` the command prints an error explaining how to rebuild. The executable can be copied
anywhere; it has no runtime dependencies. Prebuilt binaries for Linux, macOS and Windows are attached to GitHub releases
when a `v*` tag is pushed.

## Tests

Run the Rust workspace tests from the repository root:

```sh
cargo test --release --workspace
```
