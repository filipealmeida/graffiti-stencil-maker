"""Node implementations. Mask convention (same as the stencil library): True = paint = hole in the stencil."""
from __future__ import annotations

import io
import zipfile

import numpy as np
from PIL import Image
from scipy import ndimage as ndi

from stencil.core import Params, load_image, make_stencil, otsu

from .graph import choice, flag, inp, node, num, out

MAX_COLOURS = 8


def _gray(img: Image.Image) -> np.ndarray:
    return np.asarray(img.convert("L"))


def _fit(m: np.ndarray, shape) -> np.ndarray:
    if m.shape == tuple(shape):
        return m
    im = Image.fromarray((m * 255).astype(np.uint8)).resize((shape[1], shape[0]), Image.NEAREST)
    return np.asarray(im) > 127


def _size_of(v) -> tuple[int, int]:
    return (v.shape[0], v.shape[1]) if isinstance(v, np.ndarray) else (v.height, v.width)


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


@node("channels", "Channel split", "trace", [inp("image")],
      [out("c1", "image"), out("c2", "image"), out("c3", "image"), out("c4", "image")],
      [choice("space", "Colour space", "rgb", ["rgb", "hsv", "ycbcr", "cmyk"])],
      "Splits an image into grey images, one per channel. CMYK has a fourth channel.")
def n_channels(i, p, ctx):
    img = i["image"].convert("CMYK" if p["space"] == "cmyk" else p["space"].upper() if p["space"] != "rgb" else "RGB")
    ch = img.split()
    res = {f"c{k + 1}": c for k, c in enumerate(ch)}
    return {f"c{k}": res.get(f"c{k}") for k in range(1, 5)}


@node("reduce", "Colour reduction", "trace", [inp("image")],
      [out("preview", "image")] + [out(f"m{k}", "mask") for k in range(1, MAX_COLOURS + 1)],
      [num("k", "Colours", 4, 2, MAX_COLOURS, integer=True), num("median", "Clean-up (median px)", 3, 0, 15, 2, integer=True),
       choice("order", "Order", "dark to light", ["dark to light", "light to dark"])],
      "k-means colour reduction. Each colour becomes a mask (m1 = darkest), the preview shows the palette applied.")
def n_reduce(i, p, ctx):
    from sklearn.cluster import KMeans
    img = i["image"].convert("RGB")
    a = np.asarray(img).reshape(-1, 3).astype(np.float32)
    rng = np.random.default_rng(0)
    sample = a[rng.choice(len(a), min(len(a), 20000), replace=False)]
    k = min(p["k"], max(1, len(np.unique(sample, axis=0))))
    km = KMeans(k, n_init=3, random_state=0).fit(sample)
    lab = km.predict(a).reshape(img.height, img.width)
    lum = km.cluster_centers_ @ np.array([0.299, 0.587, 0.114])
    order = np.argsort(lum)[::(-1 if p["order"] == "light to dark" else 1)]
    rank = np.empty(k, int)
    rank[order] = np.arange(k)
    lab = rank[lab]
    if p["median"] > 1:
        lab = ndi.median_filter(lab, size=p["median"])
    pal = km.cluster_centers_[order].clip(0, 255).astype(np.uint8)
    res = {"preview": Image.fromarray(pal[lab])}
    for c in range(1, MAX_COLOURS + 1):
        res[f"m{c}"] = (lab == c - 1) if c <= k else None
    return res


@node("threshold", "Threshold", "trace", [inp("image")], [out("mask", "mask")],
      [num("threshold", "Threshold (256 = automatic)", 256, 0, 256, 1, integer=True), flag("invert", "Invert")],
      "Dark areas become paint (holes); invert to make light areas the holes.")
def n_threshold(i, p, ctx):
    g = _gray(i["image"])
    t = otsu(g) if p["threshold"] >= 256 else p["threshold"]
    m = g <= t
    return {"mask": ~m if p["invert"] else m}


@node("mask_filter", "Mask filter", "trace", [inp("mask", "mask")], [out("mask", "mask")],
      [choice("op", "Operation", "despeckle", ["despeckle", "open", "close", "dilate", "erode", "smooth", "invert"]),
       num("size", "Size (px)", 2, 1, 40, 1, integer=True)],
      "Clean up or reshape a mask: remove specks smaller than size², open/close/grow/shrink by size px, round corners, or invert.")
def n_filter(i, p, ctx):
    m, s, op = i["mask"], p["size"], p["op"]
    ball = ndi.generate_binary_structure(2, 1)
    if op == "invert":
        return {"mask": ~m}
    if op == "despeckle":
        lab, n = ndi.label(m)
        keep = np.bincount(lab.ravel(), minlength=n + 1) >= s * s
        keep[0] = False
        m2 = keep[lab]
        lab, n = ndi.label(~m2)
        fill = np.bincount(lab.ravel(), minlength=n + 1) < s * s
        fill[0] = False
        return {"mask": m2 | fill[lab]}
    if op == "smooth":
        return {"mask": ndi.gaussian_filter(m.astype(np.float32), s / 2) > 0.5}
    f = {"open": ndi.binary_opening, "close": ndi.binary_closing, "dilate": ndi.binary_dilation, "erode": ndi.binary_erosion}[op]
    return {"mask": f(m, ball, iterations=s)}


@node("mask_boolean", "Mask combine", "trace", [inp("a", "mask"), inp("b", "mask")], [out("mask", "mask")],
      [choice("op", "Operation", "and", ["and", "or", "xor", "a minus b"])],
      "Combine two masks, e.g. a colour region AND a pattern to fill that colour with dots.")
def n_boolean(i, p, ctx):
    a, b = i["a"], _fit(i["b"], i["a"].shape)
    return {"mask": {"and": a & b, "or": a | b, "xor": a ^ b, "a minus b": a & ~b}[p["op"]]}


@node("pattern", "Pattern", "trace", [inp("size", "any")], [out("mask", "mask")],
      [choice("kind", "Pattern", "dots", ["dots", "staggered dots", "lines", "crosshatch", "checker"]),
       num("spacing", "Spacing (px)", 12, 3, 200, 1, integer=True), num("fill", "Fill (0-1)", 0.5, 0.05, 0.95, 0.05),
       num("angle", "Angle (deg)", 45, 0, 180, 1), flag("invert", "Invert")],
      "A Ben-Day style pattern (True = hole) the size of the connected image or mask. Combine it with a colour mask to substitute a colour by a pattern.")
def n_pattern(i, p, ctx):
    h, w = _size_of(i["size"])
    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    t = np.radians(p["angle"])
    u, v = x * np.cos(t) + y * np.sin(t), -x * np.sin(t) + y * np.cos(t)
    s, f, k = float(p["spacing"]), p["fill"], p["kind"]
    if k in ("dots", "staggered dots"):
        row = np.floor(v / s)
        uu = u + (s / 2 * (row % 2) if k == "staggered dots" else 0)
        du, dv = (uu % s) - s / 2, (v % s) - s / 2
        m = du * du + dv * dv < (s * np.sqrt(f / np.pi)) ** 2
    elif k == "lines":
        m = (v % s) < s * f
    elif k == "crosshatch":
        m = ((u % s) < s * (1 - np.sqrt(1 - f))) | ((v % s) < s * (1 - np.sqrt(1 - f)))
    else:
        m = ((np.floor(u / s) + np.floor(v / s)) % 2) == 0
    return {"mask": ~m if p["invert"] else m}


@node("halftone", "Halftone", "trace", [inp("image")], [out("mask", "mask")],
      [choice("kind", "Shape", "dots", ["dots", "lines"]), num("spacing", "Cell size (px)", 10, 3, 100, 1, integer=True),
       num("angle", "Angle (deg)", 45, 0, 180, 1), num("contrast", "Contrast", 1.0, 0.3, 3.0, 0.1), flag("invert", "Dots on light areas")],
      "Tone-driven halftone: the darker the image, the bigger the dot (or the thicker the line).")
def n_halftone(i, p, ctx):
    g = ndi.gaussian_filter(_gray(i["image"]).astype(np.float32), p["spacing"] / 3)
    h, w = g.shape
    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    t = np.radians(p["angle"])
    c, s_ = np.cos(t), np.sin(t)
    u, v = x * c + y * s_, -x * s_ + y * c
    s = float(p["spacing"])
    cu, cv = (np.floor(u / s) + .5) * s, (np.floor(v / s) + .5) * s
    cx, cy = cu * c - cv * s_, cu * s_ + cv * c
    tone = g[np.clip(np.rint(cy), 0, h - 1).astype(int), np.clip(np.rint(cx), 0, w - 1).astype(int)] / 255
    d = np.clip(((1 - tone) if not p["invert"] else tone) * p["contrast"], 0, 1)
    if p["kind"] == "dots":
        m = (u - cu) ** 2 + (v - cv) ** 2 < s * s * d / np.pi
    else:
        m = np.abs(v - cv) < s * d / 2
    return {"mask": m}


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
