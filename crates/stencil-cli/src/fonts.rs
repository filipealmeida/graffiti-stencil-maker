//! System fonts for the command line: finds the fonts a project's Text nodes ask for (by the PostScript name the
//! studio saves) in --font files, --font-dir folders and the operating system's font folders.
use serde_json::Value as Json;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use stencil_core::graph::Engine;

pub struct Found {
    pub postscript: String,
    pub full: String,
    pub label: String,
    pub path: PathBuf,
}

fn system_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut d: Vec<PathBuf> = Vec::new();
    for p in [
        "/usr/share/fonts",
        "/usr/local/share/fonts",
        "/Library/Fonts",
        "/System/Library/Fonts",
    ] {
        d.push(p.into());
    }
    if let Some(h) = home {
        d.push(h.join(".fonts"));
        d.push(h.join(".local/share/fonts"));
        d.push(h.join("Library/Fonts"));
    }
    if let Some(w) = std::env::var_os("WINDIR") {
        d.push(PathBuf::from(w).join("Fonts"));
    }
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        d.push(PathBuf::from(l).join("Microsoft/Windows/Fonts"));
    }
    d
}

fn is_font(p: &Path) -> bool {
    matches!(
        p.extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .as_deref(),
        Some("ttf" | "otf" | "ttc" | "otc")
    )
}

fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            if depth < 6 {
                walk(&p, depth + 1, out);
            }
        } else if is_font(&p) {
            out.push(p);
        }
    }
}

fn name_of(face: &ttf_parser::Face, id: u16) -> String {
    face.names()
        .into_iter()
        .filter(|n| n.name_id == id)
        .find_map(|n| n.to_string())
        .unwrap_or_default()
}

/// The faces of one font file (a collection holds several).
pub fn faces(path: &Path) -> Vec<Found> {
    let Ok(bytes) = fs::read(path) else {
        return vec![];
    };
    let n = ttf_parser::fonts_in_collection(&bytes).unwrap_or(1);
    (0..n)
        .filter_map(|index| {
            let f = ttf_parser::Face::parse(&bytes, index).ok()?;
            let postscript = name_of(&f, 6);
            let full = name_of(&f, 4);
            if postscript.is_empty() && full.is_empty() {
                return None;
            }
            let label = format!("{} {}", name_of(&f, 1), name_of(&f, 2))
                .trim()
                .to_string();
            Some(Found {
                postscript,
                full,
                label,
                path: path.to_path_buf(),
            })
        })
        .collect()
}

/// `--font NAME=PATH` entries as (name, path).
pub fn parse_font_args(args: &[String]) -> Vec<(String, PathBuf)> {
    args.iter()
        .map(|a| match a.split_once('=') {
            Some((n, p)) if !n.is_empty() && !p.is_empty() => (n.to_string(), PathBuf::from(p)),
            _ => crate::fail(&format!("--font {a}: write NAME=PATH")),
        })
        .collect()
}

fn search_dirs(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut d = extra.to_vec();
    d.extend(system_dirs());
    d
}

/// Every font in the given folders followed by the system's.
pub fn scan(extra: &[PathBuf]) -> Vec<Found> {
    let mut files = Vec::new();
    for d in search_dirs(extra) {
        walk(&d, 0, &mut files);
    }
    files.iter().flat_map(|f| faces(f)).collect()
}

/// The PostScript names the project's Text nodes ask for, without repeats.
pub fn needed(graph: &Json) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for n in graph["nodes"].as_array().into_iter().flatten() {
        if n["type"] == "text" {
            if let Some(f) = n["params"]["system_font"]
                .as_str()
                .filter(|f| !f.is_empty())
            {
                if !v.iter().any(|x| x == f) {
                    v.push(f.to_string());
                }
            }
        }
    }
    v
}

/// Registers the fonts the project needs. Fonts that cannot be found are reported and the Text nodes that use
/// them fall back to their built-in font.
pub fn prepare(engine: &mut Engine, graph: &mut Json, explicit: &[String], dirs: &[PathBuf]) {
    let wanted = needed(graph);
    if wanted.is_empty() {
        return;
    }
    let mut paths: HashMap<String, PathBuf> = parse_font_args(explicit).into_iter().collect();
    let mut missing: Vec<String> = wanted
        .iter()
        .filter(|w| !paths.contains_key(*w))
        .cloned()
        .collect();
    if !missing.is_empty() {
        let mut files = Vec::new();
        for d in search_dirs(dirs) {
            walk(&d, 0, &mut files);
        }
        for f in files {
            if missing.is_empty() {
                break;
            }
            for face in faces(&f) {
                if let Some(at) = missing
                    .iter()
                    .position(|m| *m == face.postscript || m.eq_ignore_ascii_case(&face.full))
                {
                    paths.insert(missing.remove(at), face.path.clone());
                }
            }
        }
    }
    let mut unavailable: Vec<String> = Vec::new();
    for w in &wanted {
        let loaded = paths
            .get(w)
            .map(|p| {
                fs::read(p)
                    .map_err(|e| e.to_string())
                    .and_then(|b| engine.add_font(w, b))
            })
            .unwrap_or_else(|| Err("not found in the font folders".into()));
        if let Err(e) = loaded {
            eprintln!("warning: font '{w}' unavailable ({e}); using the built-in font instead. Use --font {w}=FILE or --font-dir");
            unavailable.push(w.clone());
        }
    }
    for n in graph["nodes"].as_array_mut().into_iter().flatten() {
        let name = n["params"]["system_font"].as_str().unwrap_or("");
        if unavailable.iter().any(|u| u == name) {
            n["params"]["system_font"] = Json::String(String::new());
        }
    }
}

/// --list-fonts: one line per font (`name`, `family style`, `path`); with a project, only its fonts, marked found or missing.
pub fn list(graph: Option<&Json>, explicit: &[String], dirs: &[PathBuf]) {
    let all = scan(dirs);
    let Some(g) = graph else {
        for f in &all {
            println!("{}\t{}\t{}", f.postscript, f.label, f.path.display());
        }
        eprintln!("{} fonts", all.len());
        return;
    };
    let given = parse_font_args(explicit);
    let wanted = needed(g);
    if wanted.is_empty() {
        println!("the project uses no system fonts");
    }
    for w in &wanted {
        if let Some((_, p)) = given.iter().find(|(n, _)| n == w) {
            println!("found\t{w}\t{} (--font)", p.display());
        } else if let Some(f) = all
            .iter()
            .find(|f| f.postscript == *w || w.eq_ignore_ascii_case(&f.full))
        {
            println!("found\t{w}\t{}", f.path.display());
        } else {
            println!("missing\t{w}");
        }
    }
}
