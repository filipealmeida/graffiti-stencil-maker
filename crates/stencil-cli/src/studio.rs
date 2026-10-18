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
    for req in server.incoming_requests() {
        let path = req.url().split(['?', '#']).next().unwrap_or("/");
        let path = path.trim_start_matches('/');
        let path = if path.is_empty() { "index.html" } else { path };
        let resp = match Site::get(path) {
            Some(f) => {
                let mime = f.metadata.mimetype().to_string();
                tiny_http::Response::from_data(f.data.into_owned())
                    .with_header(tiny_http::Header::from_bytes("Content-Type", mime).unwrap())
                    .with_header(
                        tiny_http::Header::from_bytes("Cross-Origin-Opener-Policy", "same-origin")
                            .unwrap(),
                    )
                    .with_header(
                        tiny_http::Header::from_bytes(
                            "Cross-Origin-Embedder-Policy",
                            "require-corp",
                        )
                        .unwrap(),
                    )
            }
            None => {
                let _ = req
                    .respond(tiny_http::Response::from_string("not found").with_status_code(404));
                continue;
            }
        };
        let _ = req.respond(resp);
    }
}

#[cfg(not(feature = "studio"))]
pub fn run(_args: &[String]) {
    super::fail(
        "this binary was built without the studio; rebuild with --features studio (see INSTALL.md)",
    )
}
