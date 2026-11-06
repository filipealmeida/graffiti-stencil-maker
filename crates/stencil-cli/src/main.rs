mod args;
mod execution;
mod fonts;
mod graph;
mod output;
mod studio;

use args::Args;
use clap::Parser;
use std::time::Instant;
use stencil_core::graph::Engine;

/// Local time zone offset in seconds, from `date +%z` (+0100); 0 (UTC) if it cannot be read.
fn utc_offset() -> i64 {
    let out = std::process::Command::new("date").arg("+%z").output().ok();
    let text = out
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let b = text.as_bytes();
    if b.len() != 5 || !text[1..].bytes().all(|c| c.is_ascii_digit()) {
        return 0;
    }
    let secs =
        text[1..3].parse::<i64>().unwrap_or(0) * 3600 + text[3..5].parse::<i64>().unwrap_or(0) * 60;
    if b[0] == b'-' {
        -secs
    } else {
        secs
    }
}

fn main() {
    stencil_core::zip::set_utc_offset(utc_offset());
    let argv: Vec<String> = std::env::args().collect();
    if argv.get(1).map(String::as_str) == Some("studio") {
        studio::run(&argv[2..]);
        return;
    }

    let a = Args::parse();
    if let Some(node) = &a.list_params {
        output::list_params(node, &a.params_format);
        return;
    }
    if a.list_fonts {
        let graph = a
            .project
            .as_ref()
            .map(|p| graph::load_project(&mut Engine::new(), p).0);
        fonts::list(graph.as_ref(), &a.font, &a.font_dir);
        return;
    }
    if a.batch.is_some() || !a.sweep.is_empty() {
        execution::run_many(&a, &argv);
        return;
    }

    stencil_core::init_threads(a.threads);
    let t = Instant::now();
    let mut engine = Engine::new();
    let (mut graph, stem, dir) = graph::load_input(&a, &mut engine);
    let result = execution::run(&a, &mut engine, &mut graph, t);
    output::write_outputs(&a, &engine, &graph, &result, &stem, &dir, t);
}

pub(crate) fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}
