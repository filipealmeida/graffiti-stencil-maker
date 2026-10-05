import argparse
import json
import sys
from pathlib import Path

from .core import Params, make_stencil


def main(argv=None):
    ap = argparse.ArgumentParser(prog="stencil", description="Turn an image (png/jpg/bmp/svg) into an island-free graffiti stencil STL.")
    ap.add_argument("input", nargs="?", help="input image")
    ap.add_argument("-o", "--output", help="output .stl (default: input name + .stl)")
    ap.add_argument("--serve", action="store_true", help="start the web interface instead")
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=5000)
    d = Params()
    ap.add_argument("--width", type=float, default=d.width_mm, help="plate width in mm (default %(default)s)")
    ap.add_argument("--resolution", type=int, default=d.resolution, help="pixels along longest side (default %(default)s)")
    ap.add_argument("--thickness", type=float, default=d.thickness_mm, help="plate thickness in mm")
    ap.add_argument("--margin", type=float, default=d.margin_mm, help="solid frame width in mm")
    ap.add_argument("--bridge", type=float, default=d.bridge_mm, help="bridge width in mm")
    ap.add_argument("--min-feature", type=float, default=d.min_feature_mm, help="drop specks smaller than this (mm)")
    ap.add_argument("--smooth", type=float, default=0.0, help="smooth the outline polygon (sigma in mm along the contour, try 0.4)")
    ap.add_argument("--raised-bridges", action="store_true", help="islands grow toward the frame layer by layer (diagonal ramp); free on the wall side so paint flows under")
    ap.add_argument("--layer-height", "--bridge-height", dest="layer_height", type=float, default=d.layer_height_mm,
                    help="layer height in mm, also the bridge height: with --raised-bridges the island grows one step per layer (default %(default)s)")
    ap.add_argument("--z-bridging", choices=["steps", "ramp", "stepramp", "grow"], default="steps",
                    help="with --raised-bridges: 'steps' grows the island layer by layer; 'ramp' makes one continuous diagonal slope from the island-bearing first layer to the island-free top layer; 'stepramp' uses steps joined by short slopes; 'grow' makes islands and bridges widen in X and Y at 45 degrees (or --max-overhang) layer by layer")
    ap.add_argument("--min-island", type=float, default=d.min_island_mm2, help="delete islands (pieces not touching the frame) smaller than this area in mm2 (default %(default)s = keep all)")
    ap.add_argument("--max-overhang", type=float, default=d.max_overhang_deg, help="with --raised-bridges: limit overhang per layer to layer_height*tan(angle), angle from vertical in degrees (e.g. 45); 0 = unlimited")
    ap.add_argument("--pads", type=int, default=0, choices=range(0, 5), metavar="0-4", help="raised pads with a threaded hole on the margin: 1 top; 2 top+bottom; 3 top corners+bottom; 4 corners (default: 0)")
    ap.add_argument("--pad-thread", default=d.pad_thread, choices=["M4", "M6", "M8", "M10"], help="pad thread (default %(default)s); needs a margin of at least major diameter + 4 mm (M4 8, M6 10, M8 12, M10 14)")
    ap.add_argument("--pad-height", type=float, default=d.pad_height_mm, help="height of the pad above the plate in mm (default %(default)s)")
    ap.add_argument("--thread-clearance", type=float, default=d.thread_clearance_mm, help="radial clearance of the printed thread in mm (default %(default)s)")
    ap.add_argument("--svg", nargs="?", const="", metavar="FILE", help="also write the traced artwork (first layer, mm) as SVG (default name: input name + .svg)")
    ap.add_argument("--flip", action="store_true", help="export upside down (island side up) so the stencil prints with no overhangs")
    ap.add_argument("--extra-top-layers", type=int, default=d.extra_top_layers, help="extra copies of the last layer on top for strength (default %(default)s)")
    ap.add_argument("--threshold", type=int, default=None, help="0-255 (default: automatic)")
    ap.add_argument("--invert", action="store_true", help="light areas become holes instead of dark")
    ap.add_argument("--tile-max", metavar="X[xY]", help="split into equal tiles at most X by Y mm (Y defaults to X); written to a zip next to the output")
    ap.add_argument("--connector-diameter", type=float, default=0.0, choices=[0.0, 2.5, 3.0, 4.0, 5.0, 6.0], help="blind holes in the seams for glued rods, diameter in mm (hexagon: across corners; plate must be thicker than 4 mm; 0 = plain cut)")
    ap.add_argument("--connector-shape", choices=["hex", "round"], default="hex", help="hole and rod shape (default %(default)s)")
    ap.add_argument("--connector-clearance", type=float, default=d.connector_clearance_mm, help="diametral clearance between rod and hole in mm (default %(default)s)")
    a = ap.parse_args(argv)

    if a.serve:
        from .server import app
        app.run(host=a.host, port=a.port)
        return 0
    if not a.input:
        ap.error("input image required (or use --serve)")
    tx = ty = 0.0
    if a.tile_max:
        try:
            parts = [float(v) for v in a.tile_max.lower().split("x")]
            tx, ty = parts[0], parts[-1]
        except ValueError:
            ap.error("--tile-max must look like 200 or 200x150")
    src = Path(a.input)
    p = Params(width_mm=a.width, resolution=a.resolution, thickness_mm=a.thickness, margin_mm=a.margin, bridge_mm=a.bridge,
               threshold=a.threshold, invert=a.invert, min_feature_mm=a.min_feature, smooth_mm=a.smooth,
               raised_bridges=a.raised_bridges, layer_height_mm=a.layer_height, z_bridging=a.z_bridging,
               extra_top_layers=max(0, a.extra_top_layers), min_island_mm2=max(0.0, a.min_island),
               max_overhang_deg=max(0.0, a.max_overhang), flip=a.flip,
               pad_count=a.pads, pad_thread=a.pad_thread, pad_height_mm=a.pad_height, thread_clearance_mm=max(0.0, a.thread_clearance),
               tile_max_x_mm=max(0.0, tx), tile_max_y_mm=max(0.0, ty), connector_diameter_mm=a.connector_diameter,
               connector_shape=a.connector_shape, connector_clearance_mm=max(0.0, a.connector_clearance))
    try:
        stl, stats = make_stencil(src.read_bytes(), src.name, p)
    except ValueError as e:
        ap.error(str(e))
    out = Path(a.output) if a.output else src.with_suffix(".stl")
    out.write_bytes(stl)
    if "tiles_zip" in stats:
        z = out.with_name(out.stem + "_tiles.zip")
        z.write_bytes(stats["tiles_zip"])
        print(f"wrote {z} ({len(stats['tiles'])} tiles)", file=sys.stderr)
    if a.svg is not None:
        svg = Path(a.svg) if a.svg else src.with_suffix(".svg")
        svg.write_text(stats["trace_svg"], encoding="utf-8")
        print(f"wrote {svg}", file=sys.stderr)
    print(f"wrote {out} ({stats['triangles']} triangles)", file=sys.stderr)
    print(json.dumps({k: v for k, v in stats.items() if k not in ("params", "first_layer_svg", "trace_svg", "overlay_png", "tiles_zip")}))
    return 0 if stats["islands_remaining"] == 0 and stats["watertight"] else 1
