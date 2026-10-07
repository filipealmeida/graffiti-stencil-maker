"""Node implementations. Mask convention (same as the stencil library): True = paint = hole in the stencil."""
from __future__ import annotations

import io
import zipfile

import numpy as np
from PIL import Image

from stencil.core import Params, load_image, make_stencil, otsu

from .graph import choice, flag, inp, node, num, out

def _gray(img: Image.Image) -> np.ndarray:
    return np.asarray(img.convert("L"))


# ---- trace stage ------------------------------------------------------------------------------

@node("source", "Image", "trace", [], [out("image", "image")],
      [{"name": "image_id", "label": "Image", "kind": "text", "default": "", "group": None},
       num("resolution", "Working resolution (px, longest side)", 500, 20, 2000, 10, integer=True)],
      "The input picture, resized to the working resolution that every mask downstream is traced at.")
def n_source(i, p, ctx):
    if not p["image_id"]:
        raise ValueError("no image loaded: drop a picture on the canvas")
    data, name = ctx.uploads.read(p["image_id"])
    img = load_image(data, name)
    s = p["resolution"] / max(img.size)
    return {"image": img.resize((max(1, round(img.width * s)), max(1, round(img.height * s))), Image.LANCZOS)}


@node("threshold", "Threshold", "trace", [inp("image")], [out("mask", "mask")],
      [num("threshold", "Threshold (256 = automatic)", 256, 0, 256, 1, integer=True), flag("invert", "Invert")],
      "Dark areas become paint (holes); invert to make light areas the holes.")
def n_threshold(i, p, ctx):
    g = _gray(i["image"])
    t = otsu(g) if p["threshold"] >= 256 else p["threshold"]
    m = g <= t
    return {"mask": ~m if p["invert"] else m}


# ---- stencil stage ----------------------------------------------------------------------------

@node("stencil", "Stencil", "stencil", [inp("mask", "mask")], [out("solid", "solid"), out("bridges", "image")],
      [num("width_mm", "Plate width (mm)", 100, 20, 2000, 1, "Plate"), num("thickness_mm", "Thickness (mm)", 2, 0.2, 100, 0.2, "Plate"),
       num("margin_mm", "Margin (mm)", 8, 1, 500, 1, "Plate"), flag("flip", "Print upside down", False, "Plate"),
       num("bridge_mm", "Bridge width (mm)", 1.6, 0.6, 50, 0.1, "Bridges"),
       num("bridges_per_island", "Bridges per island", 1, 1, 6, 1, "Bridges", integer=True),
       flag("raised_bridges", "Raised bridges", False, "Bridges"),
       choice("z_bridging", "Z bridging", "steps", ["steps", "ramp", "stepramp", "island", "grow", "easy"], "Bridges"),
       num("layer_height_mm", "Layer height (mm)", 0.2, 0.05, 5, 0.05, "Bridges"),
       num("extra_top_layers", "Extra top layers", 1, 0, 50, 1, "Bridges", integer=True),
       num("max_overhang_deg", "Max overhang (deg, 0 = off)", 0, 0, 89, 1, "Bridges"),
       num("min_island_mm2", "Delete islands under (mm²)", 0, 0, 100000, 1, "Bridges"),
       num("smooth_mm", "Smooth outline (mm)", 0, 0, 10, 0.1, "Shape"),
       num("min_feature_mm", "Drop specks under (mm)", 0.8, 0, 20, 0.1, "Shape"),
       num("pad_count", "Mounting pads", 0, 0, 4, 1, "Pads", integer=True),
       choice("pad_thread", "Pad thread", "M8", ["M4", "M6", "M8", "M10"], "Pads"),
       num("pad_height_mm", "Pad height (mm)", 6, 1, 50, 1, "Pads"),
       num("tile_max_x_mm", "Tile width (mm, 0 = none)", 0, 0, 5000, 10, "Tiles"),
       num("tile_max_y_mm", "Tile height (mm, 0 = none)", 0, 0, 5000, 10, "Tiles"),
       num("connector_diameter_mm", "Connector rod (mm, 0 = none)", 0, 0, 6, 0.5, "Tiles"),
       choice("connector_shape", "Connector shape", "hex", ["hex", "round"], "Tiles")],
      "Turns a mask into a watertight, island-free 3D plate with bridges. Tiling stays here because bridges are recomputed per tile.")
def n_stencil(i, p, ctx):
    m = i["mask"]
    png = io.BytesIO()
    Image.fromarray(np.where(m, 0, 255).astype(np.uint8)).save(png, "PNG")
    q = Params(resolution=max(m.shape), threshold=128, invert=False, **p)
    stl, stats = make_stencil(png.getvalue(), "mask.png", q, ctx.progress)
    overlay = stats.pop("overlay_png", None)
    tiles = stats.pop("tiles_zip", None)
    stats = {k: v for k, v in stats.items() if isinstance(v, (int, float, str, bool, list, dict, type(None)))}
    return {"solid": {"stl": stl, "tiles_zip": tiles, "stats": stats},
            "bridges": Image.open(io.BytesIO(overlay)).convert("RGB") if overlay else None}


# ---- post stage -------------------------------------------------------------------------------

@node("export", "Export", "post", [inp("solid", "solid")], [out("parts", "parts")],
      [choice("what", "Export", "whole plate", ["whole plate", "tiles + connectors"])],
      "Packages the stencil for download: the single STL, or the tile STLs with connector rods.")
def n_export(i, p, ctx):
    s = i["solid"]
    if p["what"] == "tiles + connectors":
        if not s["tiles_zip"]:
            raise ValueError("no tiles: set a tile size on the Stencil node")
        z = zipfile.ZipFile(io.BytesIO(s["tiles_zip"]))
        return {"parts": {"files": {n: z.read(n) for n in z.namelist()}}}
    return {"parts": {"files": {"stencil.stl": s["stl"]}}}
