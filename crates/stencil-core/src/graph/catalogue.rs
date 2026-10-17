//! The node catalogue: ids, ports, parameter definitions and docs. Execution lives in the parent module.

use super::*;

pub(super) type Outputs = Vec<(&'static str, Value)>;
pub(super) type RunFn = fn(&Engine, &HashMap<String, Value>, &P) -> Result<Outputs, String>;

pub(super) struct NodeDef {
    pub(super) id: &'static str,
    pub(super) label: &'static str,
    pub(super) stage: &'static str,
    pub(super) inputs: Vec<(&'static str, &'static str, bool)>,
    pub(super) outputs: Vec<(&'static str, &'static str)>,
    pub(super) params: Vec<Json>,
    pub(super) doc: &'static str,
    pub(super) run: RunFn,
}

pub(super) fn num(
    name: &str,
    label: &str,
    default: f64,
    lo: f64,
    hi: f64,
    step: f64,
    group: Option<&str>,
    int: bool,
) -> Json {
    json!({"name": name, "label": label, "kind": if int {"int"} else {"float"}, "default": default, "min": lo, "max": hi, "step": step, "group": group})
}
pub(super) fn flag(name: &str, label: &str, default: bool, group: Option<&str>) -> Json {
    json!({"name": name, "label": label, "kind": "bool", "default": default, "group": group})
}
pub(super) fn choice(
    name: &str,
    label: &str,
    default: &str,
    choices: &[&str],
    group: Option<&str>,
) -> Json {
    json!({"name": name, "label": label, "kind": "choice", "default": default, "choices": choices, "group": group})
}

pub(super) const THREAD_NAMES: [&str; 6] = ["M3", "M4", "M5", "M6", "M8", "M10"];

pub(super) fn catalogue() -> Vec<NodeDef> {
    vec![
        NodeDef {
            id: "source",
            label: "Image",
            stage: "trace",
            inputs: vec![],
            outputs: vec![("image", "image")],
            params: vec![
                json!({"name": "image_id", "label": "Image", "kind": "text", "default": "", "group": null}),
                num("resolution", "Working resolution (px, longest side)", 500.0, 20.0, 2000.0, 10.0, None, true),
            ],
            doc: "The input picture, resized to the working resolution that every mask downstream is traced at.",
            run: run_source,
        },
        NodeDef {
            id: "text",
            label: "Text",
            stage: "trace",
            inputs: vec![],
            outputs: vec![("mask", "mask")],
            params: vec![
                json!({"name": "text", "label": "Text (Enter starts a new line)", "kind": "string", "default": "STENCIL", "group": null}),
                choice("font", "Font", "sans bold", &crate::text::FONTS, None),
                num("size", "Letter height (px)", 160.0, 20.0, 600.0, 5.0, None, true),
                num("line_spacing", "Line spacing (x)", 1.0, 0.7, 2.0, 0.05, None, false),
                num("padding", "Padding (px)", 20.0, 0.0, 200.0, 1.0, None, true),
            ],
            doc: "Letters as a mask, ready for a Stencil node (letters become cut-outs, and the Stencil bridges the islands inside letters like O, A and B). Several lines are centred. To put text on a picture, there is no overlay yet: use it as its own stencil or tile it next to the picture's plate.",
            run: run_text,
        },
        NodeDef {
            id: "transform",
            label: "Crop & rotate",
            stage: "trace",
            inputs: vec![("image", "image", true)],
            outputs: vec![("image", "image")],
            params: vec![
                num("crop_left", "Crop left (%)", 0.0, 0.0, 90.0, 0.5, Some("Crop"), false),
                num("crop_top", "Crop top (%)", 0.0, 0.0, 90.0, 0.5, Some("Crop"), false),
                num("crop_right", "Crop right (%)", 0.0, 0.0, 90.0, 0.5, Some("Crop"), false),
                num("crop_bottom", "Crop bottom (%)", 0.0, 0.0, 90.0, 0.5, Some("Crop"), false),
                choice("turn", "Turn (clockwise, degrees)", "0", &["0", "90", "180", "270"], Some("Rotate")),
                num("angle", "Fine rotation (degrees, clockwise)", 0.0, -45.0, 45.0, 0.5, Some("Rotate"), false),
                flag("flip_h", "Flip horizontally", false, Some("Rotate")),
                flag("flip_v", "Flip vertically", false, Some("Rotate")),
            ],
            doc: "Crops, flips and rotates the picture before tracing. The order is crop, flip, turn, then the fine rotation, which grows the canvas to fit and fills the corners white (no paint). Put it right after the Image node so every later node sees the corrected picture.",
            run: run_transform,
        },
        NodeDef {
            id: "threshold",
            label: "Threshold",
            stage: "trace",
            inputs: vec![("image", "image", true)],
            outputs: vec![("mask", "mask")],
            params: vec![
                num("threshold", "Threshold (256 = automatic)", 256.0, 0.0, 256.0, 1.0, None, true),
                flag("invert", "Invert", false, None),
            ],
            doc: "Dark areas become paint (holes); invert to make light areas the holes.",
            run: run_threshold,
        },
        NodeDef {
            id: "reduce",
            label: "Reduce colours",
            stage: "trace",
            inputs: vec![("image", "image", true)],
            outputs: vec![("image", "image")],
            params: vec![
                num("colours", "Colours", 3.0, 2.0, 10.0, 1.0, None, true),
                num("smooth", "Pre-blur (px)", 0.0, 0.0, 6.0, 0.5, None, false),
            ],
            doc: "Quantizes the picture to N colours (k-means in Lab space) so every region has exactly one flat colour.",
            run: run_reduce,
        },
        NodeDef {
            id: "split_colours",
            label: "Split colours",
            stage: "trace",
            inputs: vec![("image", "image", true)],
            outputs: split_outputs(),
            params: vec![
                flag("invert", "Invert order (lightest first)", false, None),
                num("colours", "Colours if the input has more than this many (2-10)", 10.0, 2.0, 10.0, 1.0, None, true),
                num("min_area", "Drop specks smaller than (% of image, 0 = keep)", 0.0, 0.0, 5.0, 0.05, None, false),
            ],
            doc: "Splits an image into one mask per flat colour, darkest first (colour 1), so each colour can feed its own Stencil. An image with more colours than the setting is reduced first. Every mask is true where that colour is (paint). Outputs beyond the number of colours are hidden.",
            run: run_split,
        },
        NodeDef {
            id: "tone_patterns",
            label: "Tone patterns",
            stage: "trace",
            inputs: vec![("image", "image", true)],
            outputs: vec![("mask", "mask"), ("tones", "image")],
            params: tone_params(),
            doc: "Ranks the colours of a reduced image by lightness. The darkest becomes solid paint, the lightest stays empty, and the tones between get patterns that thin out step by step. Dots leave islands that the stencil bridges. One tone per colour of the input (up to 10).",
            run: run_tones,
        },
        NodeDef {
            id: "smooth",
            label: "Smooth mask",
            stage: "trace",
            inputs: vec![("mask", "mask", true)],
            outputs: vec![("mask", "mask")],
            params: vec![
                num("radius", "Smoothing (px)", 1.5, 0.0, 10.0, 0.25, None, false),
                num("supersample", "Supersample (x)", 2.0, 1.0, 4.0, 1.0, None, true),
            ],
            doc: "Rounds the pixel staircase of a mask. The mask is enlarged by the supersample factor, blurred by the smoothing radius (in source pixels) and cut at half strength, so edges become curves and sharp corners round off. Very thin strokes can vanish at large radii. Feed the result to a Stencil node: bridging still runs on it, so it stays island-free. The output is supersample-times larger in each direction, and the stencil grid grows with it.",
            run: run_smooth,
        },
        NodeDef {
            id: "morphology",
            label: "Morphology",
            stage: "trace",
            inputs: vec![("mask", "mask", true)],
            outputs: vec![("mask", "mask")],
            params: vec![
                choice(
                    "op",
                    "Operation",
                    "open",
                    &["erode", "dilate", "open", "close", "gradient", "white_hat", "black_hat", "thin", "skeleton", "hit_or_miss"],
                    None,
                ),
                choice("kernel", "Kernel shape", "disc", &["square", "disc", "diamond", "cross", "line", "ring"], Some("Kernel")),
                num("radius", "Kernel radius (px)", 2.0, 1.0, 30.0, 1.0, Some("Kernel"), true),
                num("angle", "Line angle (deg)", 0.0, 0.0, 179.0, 1.0, Some("Kernel"), false),
                num("iterations", "Iterations (thin: max passes, 0 = until stable)", 1.0, 0.0, 10.0, 1.0, Some("Kernel"), true),
                choice("pattern", "Hit-or-miss pattern", "endpoints", &["isolated", "endpoints", "corners", "junctions", "custom"], Some("Hit-or-miss")),
                json!({"name": "custom", "label": "Custom 3x3 (1 paint, 0 empty, x any)", "kind": "text", "default": "x00 011 x1x", "group": "Hit-or-miss"}),
                flag("rotate", "Also match 90° rotations (custom)", true, Some("Hit-or-miss")),
            ],
            doc: "Binary morphology on a mask (paint = true). Erode shrinks paint and dilate grows it; open drops specks and thin necks, close fills pinholes and gaps; gradient is the outline band; white-hat is what open removed, black-hat is what close added. Thin (Zhang-Suen) reduces strokes to one-pixel lines that stay connected, Skeleton is the morphological skeleton (3x3 square). Hit-or-miss marks pixels matching a 3x3 pattern: isolated pixels, endpoints, convex corners, junctions, or your own. The kernel (square, disc, diamond, cross, line at an angle, ring) applies to the erode-to-black-hat operations; paint at the picture edge is not eroded. The output is the mask of the affected pixels, so gradient, hats, skeleton and hit-or-miss give only those pixels.",
            run: run_morphology,
        },
        NodeDef {
            id: "trace",
            label: "Trace",
            stage: "trace",
            inputs: vec![("mask", "mask", true)],
            outputs: vec![("shapes", "shapes")],
            params: vec![
                num("smooth", "Smoothing (px)", 1.5, 0.0, 8.0, 0.25, None, false),
                num("corner_deg", "Sharp corner angle (deg, 180 = round everything)", 60.0, 20.0, 180.0, 5.0, None, false),
                num("tolerance", "Simplify tolerance (px)", 0.4, 0.05, 3.0, 0.05, None, false),
                num("min_area", "Drop specks smaller than (px²)", 4.0, 0.0, 1000.0, 1.0, None, false),
            ],
            doc: "Turns a mask into vector shapes: closed Bézier outlines with holes, smooth curves and sharp corners kept. Feed it to a Vector stencil, so the outlines need no pixel smoothing.",
            run: run_trace,
        },
        NodeDef {
            id: "round_corners",
            label: "Round corners",
            stage: "trace",
            inputs: vec![("shapes", "shapes", true)],
            outputs: vec![("shapes", "shapes"), ("rounded", "shapes")],
            params: vec![
                num("max_angle", "Corners sharper than (deg)", 60.0, 10.0, 170.0, 5.0, None, false),
                num("radius", "Arc radius (px)", 3.0, 0.5, 50.0, 0.5, None, false),
                flag("round_concave", "Also round empty spikes poking into the paint", false, None),
            ],
            doc: "Replaces sharp pointy corners of traced outlines with a small tangent arc, so tips are not too fragile to cut or print. Only paint tips are rounded by default (they lose material); empty spikes that poke into the paint are rounded too when asked (they gain material). The arc is shortened where the neighbouring edges are short, so a very sharp tip may keep a smaller radius. The second output shows the cut-off tips.",
            run: run_round_corners,
        },
        NodeDef {
            id: "remove_islands",
            label: "Remove islands",
            stage: "trace",
            inputs: vec![("shapes", "shapes", true)],
            outputs: vec![("shapes", "shapes"), ("removed", "shapes")],
            params: vec![
                flag("delete_specks", "Delete small paint specks", true, None),
                flag("fill_holes", "Fill small holes (the unpainted islands a stencil would need bridges for)", true, None),
                num("max_area", "Smaller than (px²)", 50.0, 1.0, 100000.0, 1.0, None, false),
                num("keep_largest", "Keep only the N largest shapes (0 = off)", 0.0, 0.0, 1000.0, 1.0, None, true),
            ],
            doc: "Cleans traced shapes by size. Specks are separate paint pieces and holes are unpainted regions enclosed by paint; each smaller than the area limit is removed, together with anything nested inside it. Filling a hole means that part is cut out too, so the stencil needs no bridge there. 'Keep only the N largest' drops the other separate paint shapes. The second output shows what was removed.",
            run: run_remove_islands,
        },
        NodeDef {
            id: "shapes_to_mask",
            label: "Shapes to mask",
            stage: "trace",
            inputs: vec![("shapes", "shapes", true)],
            outputs: vec![("mask", "mask")],
            params: vec![num("resolution", "Resolution (px, longest side)", 1000.0, 20.0, 4000.0, 10.0, None, true)],
            doc: "Rasterises vector shapes back to a mask at any resolution, so curves can feed the raster nodes.",
            run: run_shapes_to_mask,
        },
        NodeDef {
            id: "stencil",
            label: "Stencil",
            stage: "stencil",
            inputs: vec![("mask", "mask", true)],
            outputs: vec![("solid", "solid")],
            params: stencil_params(false),
            doc: "Turns a mask into a watertight, island-free 3D plate with bridges. Raised bridges keep the first layer true to the artwork: islands and all, with the paint filling underneath.",
            run: run_stencil,
        },
        NodeDef {
            id: "vector_stencil",
            label: "Vector stencil",
            stage: "stencil",
            inputs: vec![("shapes", "shapes", true)],
            outputs: vec![("solid", "solid")],
            params: stencil_params(true),
            doc: "Builds the plate directly from vector outlines with polygon booleans, bridges enclosed material islands, and extrudes the result. Mesh detail controls curve flattening accuracy. Raised bridges are built from vectors too, as stepped strips, so curves stay smooth.",
            run: run_vector_stencil,
        },
        NodeDef {
            id: "complexity",
            label: "Complexity reducer",
            stage: "stencil",
            inputs: vec![("solid", "solid", true)],
            outputs: vec![("solid", "solid")],
            params: vec![
                num("layer_from", "First layer (1 = on the plate)", 1.0, 1.0, 1000.0, 1.0, Some("Layers"), true),
                num("layer_to", "Last layer (0 = the top one)", 0.0, 0.0, 1000.0, 1.0, Some("Layers"), true),
                num("tolerance_mm", "Largest shift of an outline (mm)", 0.1, 0.01, 1.0, 0.01, Some("Simplify"), false),
                num("smoothing", "Smoothing (0 = none, 1 = strongest)", 0.5, 0.0, 1.0, 0.05, Some("Simplify"), false),
                flag("keep_material", "Only round towards material (never thinner)", true, Some("Simplify")),
                num("max_overhang_deg", "Largest overhang (deg, 90 = no limit)", 45.0, 10.0, 90.0, 1.0, Some("Simplify"), false),
            ],
            doc: "Cuts the mesh density. Outline edges that several layers share are smoothed and simplified once, so layers keep sharing vertices; the shift stays within the tolerance and, with the material option, only adds material. No layer overhangs the one below by more than the angle. The layer height is the stencil's. Layers outside the interval keep their shape (an interval short of the whole plate saves few vertices). Run it right after a stencil, before post-processing.",
            run: run_complexity,
        },
        NodeDef {
            id: "export",
            label: "Tiles & mounting",
            stage: "post",
            inputs: vec![("solid", "solid", true)],
            outputs: vec![("parts", "parts"), ("solid", "solid")],
            params: vec![
                num("max_x", "Largest tile width (mm, 0 = no tiling)", 0.0, 0.0, 2000.0, 5.0, Some("Tiles"), false),
                num("max_y", "Largest tile height (mm, 0 = no tiling)", 0.0, 0.0, 2000.0, 5.0, Some("Tiles"), false),
                choice("connector", "Connector shape", "hex", &["hex", "round"], Some("Tiles")),
                choice("connector_d", "Connector diameter (mm)", "3", &["2.5", "3", "4", "5", "6"], Some("Tiles")),
                num("depth", "Connector depth into each tile (mm)", 10.0, 4.0, 20.0, 0.5, Some("Tiles"), false),
                num("clearance", "Connector hole clearance (mm)", 0.2, 0.0, 0.6, 0.05, Some("Tiles"), false),
                num("segments", "Round segments (connectors, holes, threads)", 48.0, 24.0, 128.0, 4.0, Some("Tiles"), true),
                num("pad_count", "Raised pads (0-4)", 0.0, 0.0, 4.0, 1.0, Some("Pads"), true),
                choice("pad_thread", "Pad thread", "M8", &THREAD_NAMES, Some("Pads")),
                num("pad_height", "Pad height (mm)", 6.0, 1.0, 30.0, 0.5, Some("Pads"), false),
                num("thread_clearance", "Thread clearance (mm, radial)", 0.2, 0.0, 0.6, 0.05, Some("Pads"), false),
                choice("mount_screw", "Mounting screw", "none", &["none", "M3", "M4", "M5", "M6", "M8", "M10"], Some("Mounting holes")),
                num("mount_clearance", "Hole clearance (mm, diametral)", 0.3, 0.0, 1.0, 0.05, Some("Mounting holes"), false),
                flag("mount_threaded", "Cut a real thread", true, Some("Mounting holes")),
                num("mount_offset", "Distance from a seam to the hole (mm)", 10.0, 4.0, 60.0, 1.0, Some("Mounting holes"), false),
                num("mount_extend", "Extender plate: outer row distance (mm)", 20.0, 8.0, 100.0, 1.0, Some("Mounting holes"), false),
            ],
            doc: "Splits the plate into printable tiles on a seam grid, with glued connector rods in blind seam holes. Adds raised threaded pads and threaded or plain mounting holes in the frame, and joiner and extender plates that bridge the seams. Download everything as a ZIP.",
            run: run_export,
        },
        NodeDef {
            id: "hardware",
            label: "Screws, nuts & fit test",
            stage: "post",
            inputs: vec![],
            outputs: vec![("parts", "parts")],
            params: vec![
                choice("screw", "Screw size", "M4", &THREAD_NAMES, Some("Screw and nut")),
                num("length", "Screw length (mm)", 20.0, 4.0, 120.0, 1.0, Some("Screw and nut"), false),
                num("nut_clearance", "Nut thread clearance (mm, radial)", 0.2, 0.0, 0.6, 0.05, Some("Screw and nut"), false),
                flag("fit_test", "Connector fit-test strip", true, Some("Fit test")),
                choice("connector", "Connector shape", "hex", &["hex", "round"], Some("Fit test")),
                choice("connector_d", "Connector diameter (mm)", "3", &["2.5", "3", "4", "5", "6"], Some("Fit test")),
                num("segments", "Round segments (threads, round holes)", 48.0, 24.0, 128.0, 4.0, None, true),
            ],
            doc: "Printable hex-head bolt and hex nut with real threads (M3 to M10), plus a calibration strip of connector holes with growing clearance: try the rod in each and keep the snuggest.",
            run: run_hardware,
        },
    ]
}

pub(super) fn stencil_params(vector: bool) -> Vec<Json> {
    let mut v = vec![
        num(
            "width_mm",
            "Plate width (mm)",
            100.0,
            20.0,
            2000.0,
            1.0,
            Some("Plate"),
            false,
        ),
        num(
            "thickness_mm",
            "Thickness (mm)",
            2.0,
            0.2,
            100.0,
            0.2,
            Some("Plate"),
            false,
        ),
        num(
            "margin_mm",
            "Margin (mm)",
            8.0,
            1.0,
            500.0,
            1.0,
            Some("Plate"),
            false,
        ),
        flag("flip", "Print upside down", false, Some("Plate")),
        num(
            "bridge_mm",
            "Bridge width (mm)",
            1.6,
            0.6,
            50.0,
            0.1,
            Some("Bridges"),
            false,
        ),
        num(
            "bridges_per_island",
            "Bridges per island",
            1.0,
            1.0,
            5.0,
            1.0,
            Some("Bridges"),
            true,
        ),
        flag("raised_bridges", "Raised bridges", false, Some("Bridges")),
        choice(
            "z_bridging",
            "Z bridging",
            "steps",
            &["steps", "easy"],
            Some("Bridges"),
        ),
        num(
            "layer_height_mm",
            "Layer height (mm)",
            0.2,
            0.05,
            5.0,
            0.05,
            Some("Bridges"),
            false,
        ),
        num(
            "extra_top_layers",
            "Extra top layers",
            1.0,
            0.0,
            50.0,
            1.0,
            Some("Bridges"),
            true,
        ),
        num(
            "max_overhang_deg",
            "Overhang check angle (deg, 0 = 45)",
            0.0,
            0.0,
            89.0,
            1.0,
            Some("Bridges"),
            false,
        ),
        num(
            "min_island_mm2",
            "Delete islands under (mm²)",
            0.0,
            0.0,
            100000.0,
            1.0,
            Some("Bridges"),
            false,
        ),
    ];
    if vector {
        v.insert(
            0,
            num(
                "detail",
                "Mesh detail (samples per mm)",
                3.0,
                0.5,
                20.0,
                0.5,
                Some("Plate"),
                false,
            ),
        );
    }
    v
}

pub(super) fn split_outputs() -> Vec<(&'static str, &'static str)> {
    const NAMES: [&str; 10] = [
        "colour_1",
        "colour_2",
        "colour_3",
        "colour_4",
        "colour_5",
        "colour_6",
        "colour_7",
        "colour_8",
        "colour_9",
        "colour_10",
    ];
    NAMES
        .iter()
        .map(|n| (*n, "mask"))
        .chain([("palette", "image")])
        .collect()
}

pub(super) fn tone_params() -> Vec<Json> {
    let kinds = [
        "auto",
        "lines",
        "dots",
        "crosshatch",
        "checker",
        "solid",
        "empty",
    ];
    let mut v = vec![
        num(
            "plate_width_mm",
            "Plate width (mm, match the Stencil node)",
            100.0,
            20.0,
            2000.0,
            1.0,
            None,
            true,
        ),
        flag(
            "invert",
            "Invert (lightest colour solid, darkest empty)",
            false,
            None,
        ),
        choice("kind", "Default pattern type", "auto", &kinds[..5], None),
        num(
            "spacing_mm",
            "Default pattern spacing (mm, 0 = solid)",
            2.5,
            0.0,
            20.0,
            0.1,
            None,
            false,
        ),
        num(
            "angle",
            "Default pattern angle (deg)",
            45.0,
            0.0,
            90.0,
            5.0,
            None,
            true,
        ),
    ];
    for k in 1..=colour::MAX_RUNGS {
        let g = format!("Tone {k}");
        v.push(choice(
            &format!("kind_{k}"),
            "Type",
            "auto",
            &kinds,
            Some(&g),
        ));
        v.push(num(
            &format!("spacing_{k}"),
            "Spacing (mm, 0 = default)",
            0.0,
            0.0,
            20.0,
            0.1,
            Some(&g),
            false,
        ));
        v.push(num(
            &format!("cover_{k}"),
            "Coverage (%, 0 = automatic)",
            0.0,
            0.0,
            95.0,
            1.0,
            Some(&g),
            false,
        ));
    }
    v
}
