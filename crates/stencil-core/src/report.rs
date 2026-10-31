//! Print-readiness findings: what a finished node tells you about whether its output can be printed.
//! The studio's node badges (`web/src/badges.ts`) report the same findings from the same infos.

use serde_json::{json, Value as Json};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

pub struct Finding {
    pub level: Level,
    pub text: String,
}

fn n(v: &Json) -> f64 {
    v.as_f64().filter(|x| x.is_finite()).unwrap_or(0.0)
}

fn plural(count: f64, one: &str, many: &str) -> String {
    format!("{} {}", count, if count > 1.0 { many } else { one })
}

/// The findings for one node: its type, its cleaned parameters and the info of each output it produced.
pub fn findings(ty: &str, params: &Json, infos: &[Json]) -> Vec<Finding> {
    let mut out: Vec<Finding> = Vec::new();
    let mut add = |level: Level, text: String| out.push(Finding { level, text });
    for info in infos {
        let s = &info["stats"];
        if s.is_object() {
            if n(&s["islands_remaining"]) > 0.0 {
                add(
                    Level::Error,
                    format!(
                        "{} remain",
                        plural(n(&s["islands_remaining"]), "island", "islands")
                    ),
                );
            }
            if s["watertight"] == json!(false) {
                add(Level::Error, "mesh is not watertight".into());
            }
            if n(&s["overhang_area_mm2"]) > 0.5 {
                add(
                    Level::Warn,
                    format!("overhang {:.0} mm²", n(&s["overhang_area_mm2"])),
                );
            }
            if n(&s["islands_deleted"]) > 0.0 {
                add(
                    Level::Warn,
                    format!(
                        "{} deleted",
                        plural(n(&s["islands_deleted"]), "island", "islands")
                    ),
                );
            }
            if n(&s["triangles"]) > 1_500_000.0 {
                add(
                    Level::Warn,
                    format!(
                        "{:.1} M triangles: heavy to slice",
                        n(&s["triangles"]) / 1e6
                    ),
                );
            }
            if let Some(f) = s["fallback"].as_str() {
                add(Level::Warn, f.to_string());
            }
            if n(&s["islands_found"]) > 0.0 && n(&s["islands_remaining"]) == 0.0 {
                add(
                    Level::Info,
                    format!("{} islands bridged", n(&s["islands_found"])),
                );
            }
        }
        if info["kind"] == "parts" {
            for w in info["meta"]["pads"]["warnings"]
                .as_array()
                .into_iter()
                .flatten()
            {
                add(Level::Warn, w.as_str().unwrap_or("").to_string());
            }
            let t = &info["meta"]["tiling"];
            let bf = &info["meta"]["bridged_fragments"];
            if n(&bf["islands_bridged"]) > 0.0 {
                add(
                    Level::Info,
                    format!(
                        "{} bridged in {}",
                        plural(n(&bf["islands_bridged"]), "island", "islands"),
                        plural(n(&bf["tiles_bridged"]), "tile", "tiles")
                    ),
                );
            }
            for w in bf["warnings"].as_array().into_iter().flatten() {
                add(Level::Warn, w.as_str().unwrap_or("").to_string());
            }
            let fr = &info["meta"]["fragments"];
            if n(&fr["pieces_removed"]) > 0.0 {
                add(
                    Level::Info,
                    format!(
                        "{} removed from {} ({:.0} mm³)",
                        plural(n(&fr["pieces_removed"]), "fragment", "fragments"),
                        plural(n(&fr["tiles_cleaned"]), "tile", "tiles"),
                        n(&fr["volume_removed_mm3"])
                    ),
                );
            }
            if matches!(ty, "tiles" | "stencil" | "vector_stencil") && n(&t["loose_pieces"]) > 0.0 {
                add(
                    Level::Warn,
                    format!(
                        "{} loose in {}: join them with Bridges or drop them with Remove fragments",
                        plural(n(&t["loose_pieces"]), "piece", "pieces"),
                        plural(n(&t["loose_tiles"]), "tile", "tiles")
                    ),
                );
            }
            if n(&t["reinforced_seams"]) > 0.0 {
                add(
                    Level::Info,
                    format!("{} seams reinforced", n(&t["reinforced_seams"])),
                );
            }
            for f in t["fallback_tiles"].as_array().into_iter().flatten() {
                add(
                    Level::Info,
                    format!(
                        "{}: {}",
                        f["tile"].as_str().unwrap_or(""),
                        f["note"].as_str().unwrap_or("")
                    ),
                );
            }
            if n(&t["dropped_specks"]) > 0.0 {
                add(
                    Level::Warn,
                    format!("{} tiny pieces dropped", n(&t["dropped_specks"])),
                );
            }
            if n(&t["dropped_fragments"]) > 0.0 {
                add(
                    Level::Warn,
                    format!(
                        "{} loose pieces dropped where tiles were cut: paint may reach through there",
                        n(&t["dropped_fragments"])
                    ),
                );
            }
            if n(&t["seam_step_mm"]) > 0.0 {
                add(
                    Level::Info,
                    format!(
                        "{:.0} mm of seam where neighbouring tiles differ by more than a layer in height",
                        n(&t["seam_step_mm"])
                    ),
                );
            }
            if n(&t["paint_under_blocks_mm2"]) > 0.0 && t["pre_tiled"] != json!(true) {
                add(
                    Level::Info,
                    format!(
                        "{:.0} mm² of paint under connector blocks",
                        n(&t["paint_under_blocks_mm2"])
                    ),
                );
            }
        }
        if info["kind"] == "tiles" && n(&info["paint_under_blocks_mm2"]) > 0.0 {
            let lost = n(&info["paint_under_blocks_mm2"]);
            let share = if n(&info["paint_mm2"]) > 0.0 {
                format!(
                    " ({:.1}% of the artwork)",
                    100.0 * lost / n(&info["paint_mm2"])
                )
            } else {
                String::new()
            };
            let level = if n(&info["paint_mm2"]) > 0.0 && lost / n(&info["paint_mm2"]) > 0.05 {
                Level::Warn
            } else {
                Level::Info
            };
            add(
                level,
                format!("{lost:.0} mm² of paint under connector blocks{share}"),
            );
        }
        if info["kind"] == "mask" {
            let c = n(&info["coverage"]);
            if c < 0.005 {
                add(Level::Warn, "mask is almost empty".into());
            } else if c > 0.995 {
                add(Level::Warn, "mask is almost all paint".into());
            }
        }
        if info["kind"] == "shapes" && n(&info["loops"]) == 0.0 {
            add(Level::Warn, "no outlines found".into());
        }
    }
    let bridge = n(&params["bridge_mm"]);
    if ty == "bridges" && bridge > 0.0 && bridge < 1.0 {
        add(
            Level::Warn,
            format!("bridge {bridge} mm is thinner than a nozzle"),
        );
    }
    out
}

/// The numbers behind the findings, for comparing runs: the largest value of each across the infos.
pub fn metrics(infos: &[Json]) -> serde_json::Map<String, Json> {
    let mut out = serde_json::Map::new();
    let mut put = |key: &str, v: &Json| {
        if let Some(x) = v.as_f64().filter(|x| x.is_finite()) {
            let slot = out.entry(key.to_string()).or_insert(json!(x));
            if slot.as_f64().is_some_and(|old| x > old) {
                *slot = json!(x);
            }
        }
    };
    for info in infos {
        put("seam_step_mm", &info["meta"]["tiling"]["seam_step_mm"]);
        put(
            "paint_under_blocks_mm2",
            &info["meta"]["tiling"]["paint_under_blocks_mm2"],
        );
        put("paint_under_blocks_mm2", &info["paint_under_blocks_mm2"]);
        put("triangles", &info["stats"]["triangles"]);
    }
    out
}

/// "ready" when nothing needs a look, "check" when something warns, "blocked" when something is wrong.
pub fn verdict(all: &[Level]) -> &'static str {
    match all.iter().max() {
        Some(Level::Error) => "blocked",
        Some(Level::Warn) => "check",
        _ => "ready",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn islands_left_block_and_a_thin_bridge_warns() {
        let info = json!({"kind": "solid", "stats": {"islands_remaining": 2, "watertight": false, "triangles": 3_000_000}});
        let f = findings("bridges", &json!({"bridge_mm": 0.6}), &[info]);
        let levels: Vec<Level> = f.iter().map(|x| x.level).collect();
        assert_eq!(verdict(&levels), "blocked");
        assert!(f.iter().any(|x| x.text == "2 islands remain"));
        assert!(f.iter().any(|x| x.text.contains("not watertight")));
        assert!(f.iter().any(|x| x.text.contains("heavy to slice")));
        assert!(f.iter().any(|x| x.text.contains("thinner than a nozzle")));
    }

    #[test]
    fn a_clean_plate_is_ready_and_a_costly_layout_warns() {
        let ok = json!({"kind": "solid", "stats": {"islands_remaining": 0, "islands_found": 5, "watertight": true, "triangles": 1000}});
        let f = findings("stencil", &json!({"bridge_mm": 2.0}), &[ok]);
        assert_eq!(
            verdict(&f.iter().map(|x| x.level).collect::<Vec<_>>()),
            "ready"
        );
        let lay = json!({"kind": "tiles", "paint_under_blocks_mm2": 300.0, "paint_mm2": 1000.0});
        let f = findings("tile_layout", &json!({}), &[lay]);
        assert_eq!(f[0].level, Level::Warn);
        assert!(f[0].text.contains("30.0% of the artwork"));
    }
}
