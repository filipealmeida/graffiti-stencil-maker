"""Node implementations. Mask convention (same as the stencil library): True = paint = hole in the stencil."""
from __future__ import annotations

import io
import zipfile

import numpy as np
from PIL import Image, ImageFilter

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


def _rgb_to_lab(c):
    c = np.where(c > 0.04045, ((c + 0.055) / 1.055) ** 2.4, c / 12.92)
    m = np.array([[0.4124, 0.3576, 0.1805], [0.2126, 0.7152, 0.0722], [0.0193, 0.1192, 0.9505]], dtype=np.float32)
    xyz = (c @ m.T) / np.array([0.95047, 1.0, 1.08883], dtype=np.float32)
    f = np.where(xyz > 0.008856, np.cbrt(xyz), 7.787 * xyz + 16 / 116)
    return np.stack([116 * f[:, 1] - 16, 500 * (f[:, 0] - f[:, 1]), 200 * (f[:, 1] - f[:, 2])], axis=1)


@node("reduce", "Reduce colours", "trace", [inp("image")], [out("image", "image")],
      [num("colours", "Colours", 3, 2, 10, 1, integer=True),
       num("smooth", "Pre-blur (px)", 0, 0, 6, 0.5)],
      "Quantizes the picture to N colours (k-means in Lab space) so every region has exactly one flat colour.")
def n_reduce(i, p, ctx):
    from sklearn.cluster import KMeans
    img = i["image"].convert("RGB")
    if p["smooth"] > 0:
        img = img.filter(ImageFilter.GaussianBlur(p["smooth"]))
    lab = _rgb_to_lab(np.asarray(img, dtype=np.float32).reshape(-1, 3) / 255.0)
    rng = np.random.default_rng(0)
    sample = lab[rng.choice(len(lab), min(len(lab), 20000), replace=False)]
    ctx.progress(0.2, f"clustering into {p['colours']} colours")
    km = KMeans(n_clusters=p["colours"], n_init=4, random_state=0).fit(sample)
    idx = km.predict(lab)
    rgb = np.asarray(img, dtype=np.float32).reshape(-1, 3)
    palette = np.stack([rgb[idx == k].mean(axis=0) if (idx == k).any() else np.zeros(3) for k in range(p["colours"])])
    ctx.progress(0.9, "painting")
    return {"image": Image.fromarray(palette[idx].reshape(img.height, img.width, 3).round().astype(np.uint8))}


# ---- patterns: each returns a bool array, True = paint (hole in the stencil) ----------------------

def _pat_lines(x, y, spacing, cov, angle):
    a = np.radians(angle)
    u = x * np.cos(a) + y * np.sin(a)
    return np.mod(u, spacing) < cov * spacing


def _pat_dots(x, y, spacing, cov, angle):
    """Hexagonal lattice of round dots; the radius is chosen so the dots cover `cov` of the area."""
    a = np.radians(angle)
    u, v = x * np.cos(a) + y * np.sin(a), -x * np.sin(a) + y * np.cos(a)
    row = spacing * np.sqrt(3) / 2
    r = min(np.sqrt(cov * spacing * row / np.pi), spacing * 0.5)
    best = np.full(u.shape, np.inf)
    for off_u, off_v in ((0, 0), (spacing / 2, row)):
        du = np.mod(u - off_u + spacing / 2, spacing) - spacing / 2
        dv = np.mod(v - off_v + row, 2 * row) - row
        best = np.minimum(best, du * du + dv * dv)
    return best <= r * r


def _pat_cross(x, y, spacing, cov, angle):
    c = 1 - np.sqrt(1 - cov)
    return _pat_lines(x, y, spacing, c, angle) | _pat_lines(x, y, spacing, c, angle + 90)


def _pat_checker(x, y, spacing, cov, angle):
    a = np.radians(angle)
    u, v = x * np.cos(a) + y * np.sin(a), -x * np.sin(a) + y * np.cos(a)
    return (np.floor(u / spacing) + np.floor(v / spacing)) % 2 == 0


PATTERNS = {"lines": _pat_lines, "dots": _pat_dots, "crosshatch": _pat_cross, "checker": _pat_checker}
MAX_RUNGS = 8          # patterned tones between "solid" and "empty" (10 colours)


@node("tone_patterns", "Tone patterns", "trace", [inp("image")], [out("mask", "mask"), out("tones", "image")],
      [num("plate_width_mm", "Plate width (mm, match the Stencil node)", 100, 20, 2000, 1, integer=True),
       flag("invert", "Invert (lightest colour solid, darkest empty)"),
       choice("kind", "Default pattern type", "auto", ["auto", "lines", "dots", "crosshatch", "checker"]),
       num("spacing_mm", "Default pattern spacing (mm, 0 = solid)", 2.5, 0, 20, 0.1),
       num("angle", "Default pattern angle (deg)", 45, 0, 90, 5, integer=True)]
      + [q for k in range(1, MAX_RUNGS + 1) for q in (
          choice(f"kind_{k}", f"Type", "auto", ["auto", "lines", "dots", "crosshatch", "checker", "solid", "empty"], f"Tone {k}"),
          num(f"spacing_{k}", "Spacing (mm, 0 = default)", 0, 0, 20, 0.1, f"Tone {k}"),
          num(f"cover_{k}", "Coverage (%, 0 = automatic)", 0, 0, 95, 1, f"Tone {k}"))],
      "Ranks the colours of a reduced image by lightness. The darkest becomes solid paint, the lightest stays empty, and the tones "
      "between get patterns that thin out step by step. Tone k is the k-th step away from solid. Connected patterns (lines, dots) "
      "are chosen automatically; dots leave islands that the stencil bridges. One tone per colour of the input (up to 10); unused tone rows are ignored.")
def n_tone_patterns(i, p, ctx):
    img = i["image"].convert("RGB")
    arr = np.asarray(img)
    flat = arr.reshape(-1, 3)
    colours, inverse = np.unique(flat, axis=0, return_inverse=True)
    n = len(colours)
    if n > MAX_RUNGS + 2:
        raise ValueError(f"{n} colours found, at most {MAX_RUNGS + 2} are supported: reduce the colours first")
    lum = colours @ np.array([0.299, 0.587, 0.114])
    order = np.argsort(lum if not p["invert"] else -lum)        # order[0] is the solid tone
    rank = np.empty(n, int)
    rank[order] = np.arange(n)
    h, w = arr.shape[:2]
    mm = p["plate_width_mm"] / w                                  # mm per pixel
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32) * mm
    result = np.zeros((h, w), bool)
    tone_idx = rank[inverse].reshape(h, w)
    levels = []
    for r in range(n):
        if r == 0:
            kind, cov = "solid", 1.0
        elif r == n - 1:
            kind, cov = "empty", 0.0
        else:
            kind, cov = p[f"kind_{r}"], p[f"cover_{r}"] / 100
            if kind == "auto":
                kind = p["kind"]
            if cov == 0:
                cov = 1 - r / (n - 1)
            if kind == "auto":
                kind = "lines" if cov >= 0.5 else "dots"
        spacing = (p[f"spacing_{r}"] if 0 < r < n - 1 else 0) or p["spacing_mm"]
        sel = tone_idx == r
        if spacing <= 0 and kind not in ("empty", "solid"):
            kind = "solid"
        if kind == "solid":
            result |= sel
        elif kind != "empty":
            ctx.progress(r / n, f"tone {r}: {kind} {spacing:g} mm, {cov * 100:.0f}%")
            result |= sel & PATTERNS[kind](xx, yy, spacing, cov, p["angle"] + (r % 2) * 90 if kind != "dots" else p["angle"])
        levels.append((kind, cov))
    grey = np.array([255 * (1 - c) for _, c in levels])[tone_idx].astype(np.uint8)
    return {"mask": result, "tones": Image.fromarray(grey).convert("RGB")}


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
       num("min_feature_mm", "Drop specks under (mm)", 0.8, 0, 20, 0.1, "Shape")],
      "Turns a mask into a watertight, island-free 3D plate with bridges. Tiling, pads and mounting holes are done in the Post stage.")
def n_stencil(i, p, ctx):
    m = i["mask"]
    png = io.BytesIO()
    Image.fromarray(np.where(m, 0, 255).astype(np.uint8)).save(png, "PNG")
    q = Params(resolution=max(m.shape), threshold=128, invert=False, **p)
    stl, stats = make_stencil(png.getvalue(), "mask.png", q, ctx.progress)
    overlay = stats.pop("overlay_png", None)
    tiles = stats.pop("tiles_zip", None)
    stats = {k: v for k, v in stats.items() if isinstance(v, (int, float, str, bool, list, dict, type(None)))}
    return {"solid": {"stl": stl, "tiles_zip": tiles, "stats": stats, "recipe": {"mask": m, "params": dict(p)}},
            "bridges": Image.open(io.BytesIO(overlay)).convert("RGB") if overlay else None}


# ---- post stage -------------------------------------------------------------------------------

@node("export", "Tiles & mounting", "post", [inp("solid", "solid")], [out("parts", "parts"), out("solid", "solid")],
      [num("tile_max_x_mm", "Tile width (mm, 0 = none)", 0, 0, 5000, 10, "Tiles"),
       num("tile_max_y_mm", "Tile height (mm, 0 = none)", 0, 0, 5000, 10, "Tiles"),
       num("connector_diameter_mm", "Connector rod (mm, 0 = none)", 0, 0, 6, 0.5, "Tiles"),
       choice("connector_shape", "Connector shape", "hex", ["hex", "round"], "Tiles"),
       num("connector_clearance_mm", "Connector fit (mm, hole larger than rod)", 0.2, 0, 1, 0.1, "Tiles"),
       choice("mount_screw", "Mounting holes (screw)", "none", ["none", "M3", "M4", "M5", "M6", "M8", "M10"], "Mounting holes"),
       num("mount_offset_mm", "Hole distance from seam (mm)", 10, 4, 100, 1, "Mounting holes"),
       num("mount_clearance_mm", "Hole clearance (mm, diametral)", 0.3, 0, 2, 0.1, "Mounting holes"),
       num("mount_plate_mm", "Joiner plate thickness (mm)", 3, 1, 10, 0.5, "Mounting holes"),
       num("mount_extend_mm", "Extender outer holes (mm outward)", 20, 5, 200, 1, "Mounting holes"),
       num("pad_count", "Mounting pads", 0, 0, 4, 1, "Pads", integer=True),
       choice("pad_thread", "Pad thread", "M8", ["M4", "M6", "M8", "M10"], "Pads"),
       num("pad_height_mm", "Pad height (mm)", 6, 1, 50, 1, "Pads")],
      "Cuts the plate into tiles (connector holes get the fit clearance you set; a calibration strip with holes of 0-0.5 mm clearance is exported to test it; bridges are recomputed per tile so each one is a single piece), adds threaded pads and "
      "screw holes in the frame of tiles that have one, and writes joiner (2-hole) and extender (4-hole) plates that screw "
      "over the holes on both sides of a seam. Nothing set: the plate is passed through.")
def n_export(i, p, ctx):
    from stencil import tiling
    s = i["solid"]
    post = {k: p[k] for k in ("tile_max_x_mm", "tile_max_y_mm", "connector_diameter_mm", "connector_shape", "connector_clearance_mm", "mount_screw", "mount_offset_mm",
                              "mount_clearance_mm", "mount_plate_mm", "mount_extend_mm", "pad_count", "pad_thread", "pad_height_mm")}
    if not (p["tile_max_x_mm"] or p["tile_max_y_mm"] or p["mount_screw"] != "none" or p["pad_count"]):
        return {"parts": {"files": {"stencil.stl": s["stl"]}}, "solid": s}
    r = s["recipe"]
    m = r["mask"]
    png = io.BytesIO()
    Image.fromarray(np.where(m, 0, 255).astype(np.uint8)).save(png, "PNG")
    q = Params(resolution=max(m.shape), threshold=128, invert=False, **{**r["params"], **post})
    stl, stats = make_stencil(png.getvalue(), "mask.png", q, ctx.progress)
    tiles = stats.pop("tiles_zip", None)
    for k in ("overlay_png", "trace_svg", "first_layer_svg"):
        stats.pop(k, None)
    stats = {k: v for k, v in stats.items() if isinstance(v, (int, float, str, bool, list, dict, type(None)))}
    files = {}
    if tiles:
        z = zipfile.ZipFile(io.BytesIO(tiles))
        files = {n: z.read(n) for n in z.namelist()}
    else:
        files["stencil.stl"] = stl
    if tiles and p["connector_diameter_mm"] > 0:
        d = p["connector_diameter_mm"]
        files[f"connector_fit_test_{p['connector_shape']}_d{d:g}mm.stl"] = tiling.stl_bytes(tiling.fit_test(p["connector_shape"], d))[0]
        ctx.progress(0.985, f"connector fit test: holes 1-{len(tiling.FIT_STEPS)} (dimples) have {tiling.FIT_STEPS[0]:g}-{tiling.FIT_STEPS[-1]:g} mm clearance; current setting {p['connector_clearance_mm']:g} mm")
    mount = stats.get("mount")
    if mount and mount.get("pairs"):
        d = tiling.hole_diameter(p["mount_screw"], p["mount_clearance_mm"])
        joiner, extender = tiling.join_plates(d, p["mount_offset_mm"], p["mount_extend_mm"], p["mount_plate_mm"])
        files[f"joiner_2hole_{p['mount_screw']}.stl"] = tiling.stl_bytes(joiner)[0]
        files[f"extender_4hole_{p['mount_screw']}.stl"] = tiling.stl_bytes(extender)[0]
        ctx.progress(0.99, f"{mount['pairs']} seam joints: print {mount['pairs']} joiners (or extenders); {mount['holes']} holes in total")
    elif mount and mount.get("note"):
        ctx.progress(0.99, mount["note"])
    return {"parts": {"files": files}, "solid": {"stl": stl, "tiles_zip": tiles, "stats": stats}}
