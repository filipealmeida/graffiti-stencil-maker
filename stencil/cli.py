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
    ap.add_argument("--z-bridging", choices=["steps", "ramp", "stepramp"], default="steps",
                    help="with --raised-bridges: 'steps' grows the island layer by layer; 'ramp' makes one continuous diagonal slope from the island-bearing first layer to the island-free top layer; 'stepramp' uses steps joined by short slopes")
    ap.add_argument("--min-island", type=float, default=d.min_island_mm2, help="delete islands (pieces not touching the frame) smaller than this area in mm2 (default %(default)s = keep all)")
    ap.add_argument("--max-overhang", type=float, default=d.max_overhang_deg, help="with --raised-bridges: limit overhang per layer to layer_height*tan(angle), angle from vertical in degrees (e.g. 45); 0 = unlimited")
    ap.add_argument("--flip", action="store_true", help="export upside down (island side up) so the stencil prints with no overhangs")
    ap.add_argument("--extra-top-layers", type=int, default=d.extra_top_layers, help="extra copies of the last layer on top for strength (default %(default)s)")
    ap.add_argument("--threshold", type=int, default=None, help="0-255 (default: automatic)")
    ap.add_argument("--invert", action="store_true", help="light areas become holes instead of dark")
    a = ap.parse_args(argv)

    if a.serve:
        from .server import app
        app.run(host=a.host, port=a.port)
        return 0
    if not a.input:
        ap.error("input image required (or use --serve)")
    src = Path(a.input)
    p = Params(width_mm=a.width, resolution=a.resolution, thickness_mm=a.thickness, margin_mm=a.margin, bridge_mm=a.bridge,
               threshold=a.threshold, invert=a.invert, min_feature_mm=a.min_feature, smooth_mm=a.smooth,
               raised_bridges=a.raised_bridges, layer_height_mm=a.layer_height, z_bridging=a.z_bridging,
               extra_top_layers=max(0, a.extra_top_layers), min_island_mm2=max(0.0, a.min_island),
               max_overhang_deg=max(0.0, a.max_overhang), flip=a.flip)
    stl, stats = make_stencil(src.read_bytes(), src.name, p)
    out = Path(a.output) if a.output else src.with_suffix(".stl")
    out.write_bytes(stl)
    print(f"wrote {out} ({stats['triangles']} triangles)", file=sys.stderr)
    print(json.dumps({k: v for k, v in stats.items() if k not in ("params", "first_layer_svg", "overlay_png")}))
    return 0 if stats["islands_remaining"] == 0 and stats["watertight"] else 1
