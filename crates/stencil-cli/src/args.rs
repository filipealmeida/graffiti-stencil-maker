use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(version)]
pub(crate) struct Args {
    /// Picture to turn into a stencil
    pub(crate) input: Option<PathBuf>,
    /// A project saved from the studio (.json) to run instead of a picture
    #[arg(long, conflicts_with = "input")]
    pub(crate) project: Option<PathBuf>,
    /// Output STL for a single part, or the folder for several (default: next to the input)
    #[arg(short, long)]
    pub(crate) output: Option<PathBuf>,
    /// Trace the picture to curves and build the plate from them
    #[arg(long)]
    pub(crate) vector: bool,
    /// Vector mode: samples per mm of the plate
    #[arg(long)]
    pub(crate) detail: Option<f64>,
    /// Pixels on the longest side
    #[arg(long)]
    pub(crate) resolution: Option<f64>,
    #[arg(long)]
    pub(crate) threshold: Option<f64>,
    #[arg(long)]
    pub(crate) invert: bool,

    #[arg(long)]
    pub(crate) width: Option<f64>,
    #[arg(long)]
    pub(crate) thickness: Option<f64>,
    #[arg(long)]
    pub(crate) margin: Option<f64>,
    #[arg(long)]
    pub(crate) bridge: Option<f64>,
    /// Bridges from every island to the plate (1-5)
    #[arg(long)]
    pub(crate) bridges_per_island: Option<f64>,
    /// Place the further bridges of an island at an angle to the earlier ones, not just on the shortest links
    #[arg(long)]
    pub(crate) bridge_spread: bool,
    /// Smallest angle between two bridges of an island in degrees (default 45); implies --bridge-spread
    #[arg(long)]
    pub(crate) bridge_min_angle: Option<f64>,
    /// With --bridge-spread: longest bridge as a multiple of the shortest link (1-16, default 4)
    #[arg(long)]
    pub(crate) bridge_max_length: Option<f64>,
    /// Widen any strip of material thinner than this (mm); 0 is off
    #[arg(long)]
    pub(crate) min_contact: Option<f64>,
    /// Minimum width of narrow features at the build plate (mm); 0 is off
    #[arg(long)]
    pub(crate) min_first_layer_width: Option<f64>,
    /// Height over which the bottom flare tapers back to the stencil outline (mm)
    #[arg(long)]
    pub(crate) first_layer_taper: Option<f64>,
    /// Worker threads (default: half of the cores, never all of them)
    #[arg(long)]
    pub(crate) threads: Option<usize>,
    #[arg(long)]
    pub(crate) layer_height: Option<f64>,
    /// Bridges rise layer by layer so the first layer keeps every island
    #[arg(long)]
    pub(crate) raised_bridges: bool,
    /// With --raised-bridges: 'steps' or 'easy'
    #[arg(long, value_parser = ["steps", "easy"])]
    pub(crate) z_bridging: Option<String>,
    #[arg(long)]
    pub(crate) extra_top_layers: Option<f64>,
    /// Print upside down (180 degrees about Y)
    #[arg(long)]
    pub(crate) flip: bool,
    /// Delete islands smaller than this area (mm2) instead of bridging
    #[arg(long)]
    pub(crate) min_island: Option<f64>,
    #[arg(long)]
    pub(crate) max_overhang: Option<f64>,

    /// Largest tile width in mm (0 = no tiling)
    #[arg(long)]
    pub(crate) tile_width: Option<f64>,
    /// Largest tile height in mm (0 = no tiling)
    #[arg(long)]
    pub(crate) tile_height: Option<f64>,
    /// Connector shape; none cuts the seams and drills nothing
    #[arg(long, value_parser = ["hex", "round", "none"])]
    pub(crate) connector: Option<String>,
    /// Connector diameter in mm: 2.5, 3, 4, 5 or 6
    #[arg(long)]
    pub(crate) connector_d: Option<String>,
    #[arg(long)]
    pub(crate) connector_clearance: Option<f64>,
    /// Total gap removed at each tile seam (mm; split evenly between both tiles)
    #[arg(long)]
    pub(crate) seam_clearance: Option<f64>,
    /// Turn the tile seams clockwise by this angle (0-60 degrees) so they cut diagonally; 0 is the straight grid
    #[arg(long)]
    pub(crate) seam_angle: Option<f64>,
    /// Round segments for connectors, holes and threads
    #[arg(long)]
    pub(crate) segments: Option<f64>,
    /// Raised threaded pads in the frame (0-4)
    #[arg(long)]
    pub(crate) pads: Option<f64>,
    /// M3, M4, M5, M6, M8 or M10
    #[arg(long)]
    pub(crate) pad_thread: Option<String>,
    #[arg(long)]
    pub(crate) pad_height: Option<f64>,
    /// Mounting holes at the seams: none, M3, M4, M5, M6, M8 or M10
    #[arg(long)]
    pub(crate) mount_screw: Option<String>,
    /// Plain holes instead of cut threads
    #[arg(long)]
    pub(crate) mount_plain: bool,
    #[arg(long)]
    pub(crate) mount_offset: Option<f64>,
    #[arg(long)]
    pub(crate) mount_extend: Option<f64>,
    /// Also write a bolt, nut and connector fit test of this screw size
    #[arg(long)]
    pub(crate) hardware: Option<String>,

    /// Set any node parameter: NODE.PARAM=VALUE, where NODE is a node id or a node type used once in the graph
    /// (for example --set tile_layout.max_x=200 --set bridges.bridge_mm=2). Repeat it for more.
    #[arg(long = "set", value_name = "NODE.PARAM=VALUE")]
    pub(crate) set: Vec<String>,
    /// Write the picture of every tile layout in the graph (paint dark, connector blocks blue, orange where they
    /// cover paint, seams red) to this PNG; with several layouts the node id is added to the name
    #[arg(long, value_name = "FILE.png")]
    pub(crate) layout: Option<PathBuf>,
    /// Write every tile of every tile layout in the graph as SVG and DXF files (millimetres: the paint to cut, the
    /// tile border and the connector blocks) into this folder
    #[arg(long, value_name = "DIR")]
    pub(crate) tile_drawings: Option<PathBuf>,
    /// Write every part and sheet of every Cut sheet node in the graph as SVG and DXF files (millimetres) into this
    /// folder
    #[arg(long, value_name = "DIR")]
    pub(crate) cut_sheets: Option<PathBuf>,
    /// Print the print-readiness report (findings and the slowest nodes)
    #[arg(long)]
    pub(crate) report: bool,
    /// Write the print-readiness report as JSON
    #[arg(long, value_name = "FILE.json")]
    pub(crate) report_file: Option<PathBuf>,
    /// Exit with an error when the report has warnings, not only errors
    #[arg(long)]
    pub(crate) strict: bool,
    /// List the parameters --set accepts (all nodes, or only this node type) with their ranges, then exit
    #[arg(long, value_name = "NODE", num_args = 0..=1, default_missing_value = "")]
    pub(crate) list_params: Option<String>,
    /// With --list-params: `text` (default) or `markdown`, a reference table per node that can be pasted into docs
    #[arg(
        long,
        value_name = "FORMAT",
        default_value = "text",
        requires = "list_params"
    )]
    pub(crate) params_format: String,
    /// Write the graph's outlines (the 'shapes' of Trace and the nodes after it) as an SVG file. With several outline
    /// outputs the node and port names are added to the file name
    #[arg(long, value_name = "FILE.svg")]
    pub(crate) svg: Option<PathBuf>,
    /// Like --svg, as a DXF file for CAD and cutters (R12, picture pixels, y up, layer OUTLINE)
    #[arg(long, value_name = "FILE.dxf")]
    pub(crate) dxf: Option<PathBuf>,
    /// Use this font file for a Text node's system font: NAME=PATH, NAME being the PostScript name the studio saved
    /// (see --list-fonts). Repeatable. Fonts the project needs are otherwise looked up in the system's font folders
    #[arg(long, value_name = "NAME=PATH")]
    pub(crate) font: Vec<String>,
    /// Also look for system fonts in this folder (repeatable)
    #[arg(long, value_name = "DIR")]
    pub(crate) font_dir: Vec<PathBuf>,
    /// List the fonts found (name, family and style, path), then exit. With --project: only the fonts it uses, found or missing
    #[arg(long)]
    pub(crate) list_fonts: bool,
    /// With --project: use this picture in the project's Image node instead of the saved one
    #[arg(long, value_name = "FILE")]
    pub(crate) source: Option<PathBuf>,
    /// With --project: run the project once for every picture in this folder (png, jpg, bmp, svg). -o is then the
    /// folder that gets one subfolder per picture, plus summary.csv
    #[arg(long, value_name = "DIR", requires = "project")]
    pub(crate) batch: Option<PathBuf>,
    /// Run once for every value: NODE.PARAM=a,b,c (repeatable; several are combined). -o is then the folder that
    /// gets one subfolder per run, plus summary.csv
    #[arg(long, value_name = "NODE.PARAM=a,b,c")]
    pub(crate) sweep: Vec<String>,
}
