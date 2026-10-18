//! `stencil studio`: serves the embedded web studio on localhost.

#[cfg(feature = "studio")]
pub fn run(args: &[String]) {
    #[derive(rust_embed::RustEmbed)]
    #[folder = "../../web/dist/"]
    struct Site;

    let mut port: u16 = 5173;
    let mut open_browser = true;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--port" | "-p" => {
                port = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| super::fail("--port needs a number"))
            }
            "--no-open" => open_browser = false,
            _ => {
                println!("usage: stencil studio [--port N] [--no-open]");
                return;
            }
        }
    }
    let server = tiny_http::Server::http(("127.0.0.1", port))
        .unwrap_or_else(|e| super::fail(&format!("cannot listen on port {port}: {e}")));
    let url = format!("http://127.0.0.1:{port}/");
    println!("Stencil studio running at {url}  (Ctrl+C to stop)");
    if open_browser {
        let _ = open::that(&url);
    }
    // a few workers, so a large download (the engine) never holds up the other requests
    let server = std::sync::Arc::new(server);
    let gz: std::sync::Arc<
        std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<Vec<u8>>>>,
    > = Default::default();
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let (server, gz) = (server.clone(), gz.clone());
            std::thread::spawn(move || {
                for req in server.incoming_requests() {
                    serve::<Site>(req, &gz);
                }
            })
        })
        .collect();
    for w in workers {
        let _ = w.join();
    }
}

#[cfg(feature = "studio")]
type GzCache = std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<Vec<u8>>>>;

#[cfg(feature = "studio")]
fn header(name: &str, value: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(name, value).unwrap()
}

/// Serves one embedded file: ETag revalidation, long caching for the hashed build assets, and gzip for text and WASM.
#[cfg(feature = "studio")]
fn serve<S: rust_embed::RustEmbed>(req: tiny_http::Request, gz: &GzCache) {
    let path = req.url().split(['?', '#']).next().unwrap_or("/");
    let path = path.trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let Some(f) = S::get(path) else {
        let _ = req.respond(tiny_http::Response::from_string("not found").with_status_code(404));
        return;
    };
    let etag = format!(
        "\"{}\"",
        f.metadata
            .sha256_hash()
            .iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let find = |name: &'static str| {
        req.headers()
            .iter()
            .find(|h| h.field.equiv(name))
            .map(|h| h.value.as_str().to_string())
    };
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    if find("If-None-Match").as_deref() == Some(etag.as_str()) {
        let _ = req.respond(
            tiny_http::Response::empty(304)
                .with_header(header("ETag", &etag))
                .with_header(header("Cache-Control", cache)),
        );
        return;
    }
    let mime = f.metadata.mimetype().to_string();
    let compressible = mime.starts_with("text/")
        || mime.contains("javascript")
        || mime.contains("json")
        || mime.contains("wasm")
        || mime.contains("svg");
    let wants_gzip = find("Accept-Encoding").is_some_and(|v| v.contains("gzip"));
    let (body, gzipped) = if compressible && wants_gzip {
        let cached = gz.lock().unwrap().get(path).cloned();
        let packed = cached.unwrap_or_else(|| {
            use std::io::Write;
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
            let _ = enc.write_all(&f.data);
            let packed = std::sync::Arc::new(enc.finish().unwrap_or_default());
            gz.lock().unwrap().insert(path.to_string(), packed.clone());
            packed
        });
        ((*packed).clone(), true)
    } else {
        (f.data.into_owned(), false)
    };
    let mut resp = tiny_http::Response::from_data(body)
        .with_header(header("Content-Type", &mime))
        .with_header(header("ETag", &etag))
        .with_header(header("Cache-Control", cache))
        .with_header(header("Vary", "Accept-Encoding"))
        .with_header(header("Cross-Origin-Opener-Policy", "same-origin"))
        .with_header(header("Cross-Origin-Embedder-Policy", "require-corp"));
    if gzipped {
        resp = resp.with_header(header("Content-Encoding", "gzip"));
    }
    let _ = req.respond(resp);
}

#[cfg(not(feature = "studio"))]
pub fn run(_args: &[String]) {
    super::fail(
        "this binary was built without the studio; rebuild with --features studio (see INSTALL.md)",
    )
}
