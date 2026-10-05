"""Image -> graffiti stencil STL, guaranteeing a single connected piece (no islands)."""
from __future__ import annotations

import io
import struct
from dataclasses import dataclass, asdict
from pathlib import Path

import numpy as np
from PIL import Image
from scipy import ndimage as ndi
from scipy.ndimage import gaussian_filter1d

FOUR = ndi.generate_binary_structure(2, 1)


@dataclass
class Params:
    width_mm: float = 100.0       # stencil plate width (frame included)
    resolution: int = 250         # pixels along the longest side
    thickness_mm: float = 2.0
    margin_mm: float = 8.0        # solid frame around the artwork
    bridge_mm: float = 1.6        # width of the bridges that tie islands in
    bridges_per_island: int = 1   # 1-6 bridges from every island to the rest of the stencil
    threshold: int | None = None  # 0-255; None = automatic (Otsu)
    invert: bool = False          # False: dark areas become holes (paint)
    min_feature_mm: float = 0.8   # specks smaller than this are dropped
    smooth_mm: float = 0.0        # >0: smooth the outline polygons (gaussian sigma in mm along the contour)
    raised_bridges: bool = False  # islands grow toward the frame one layer at a time; free on the wall side
    extra_top_layers: int = 1     # copies of the final layer stacked on top for strength
    z_bridging: str = "steps"    # "steps": island grows layer by layer; "ramp": one continuous diagonal slope
    layer_height_mm: float = 0.2  # layer = bridge height: with raised bridges the island grows one step per layer
    min_island_mm2: float = 0.0   # islands (material not touching the frame) smaller than this area are deleted
    max_overhang_deg: float = 0.0 # >0: raised bridges grow at most layer_height*tan(angle) per layer (angle from vertical)
    flip: bool = False            # export upside down (island side up) so that nothing overhangs when printed
    pad_count: int = 0            # 0-4 raised pads with a threaded hole, placed on the margin only
    pad_thread: str = "M8"        # M4, M6, M8 or M10
    pad_height_mm: float = 6.0    # how far the pad rises above the plate
    thread_clearance_mm: float = 0.2  # radial clearance added to the thread void so printed threads mate
    tile_max_x_mm: float = 0.0    # >0: split into equal tiles at most this wide (x) ...
    tile_max_y_mm: float = 0.0    # ... and this tall (y); 0 = no limit on that axis
    connector_diameter_mm: float = 0.0   # 0 = plain cut; else 2.5, 3, 4, 5 or 6: blind holes in the seams for glued rods (plate > 4 mm)
    connector_shape: str = "hex"  # "hex" or "round"
    connector_clearance_mm: float = 0.2  # diametral clearance between the rod and its hole


def load_image(data: bytes, filename: str = "") -> Image.Image:
    """Decode png/jpg/bmp/svg bytes to an RGB image on a white background."""
    head = data[:512].lstrip().lower()
    is_svg = filename.lower().endswith((".svg", ".svgz")) or head.startswith(b"<svg") or b"<svg" in head
    if is_svg:
        import cairosvg
        data = cairosvg.svg2png(bytestring=data, output_width=1600, background_color="white")
    img = Image.open(io.BytesIO(data))
    img.load()
    img = img.convert("RGBA")
    bg = Image.new("RGBA", img.size, (255, 255, 255, 255))
    return Image.alpha_composite(bg, img).convert("RGB")


def otsu(gray: np.ndarray) -> int:
    hist = np.bincount(gray.ravel(), minlength=256).astype(float)
    total = hist.sum()
    sum_all = (np.arange(256) * hist).sum()
    w0 = np.cumsum(hist)
    s0 = np.cumsum(hist * np.arange(256))
    w1 = total - w0
    with np.errstate(divide="ignore", invalid="ignore"):
        var = (w0 * w1) * (s0 / w0 - (sum_all - s0) / w1) ** 2
    var = np.nan_to_num(var)
    return int(np.argmax(var))


def label4(mask: np.ndarray):
    return ndi.label(mask, structure=FOUR)


def _remove_small(mask: np.ndarray, min_area: int) -> np.ndarray:
    if min_area <= 1:
        return mask
    lab, n = label4(mask)
    if n == 0:
        return mask
    sizes = np.bincount(lab.ravel())
    keep = sizes >= min_area
    keep[0] = False
    return keep[lab]


def _fix_diagonals(m: np.ndarray) -> np.ndarray:
    """Remove checkerboard 2x2 contacts (non-manifold corners) by filling one hole pixel."""
    m = m.copy()
    while True:
        a, b = m[:-1, :-1], m[:-1, 1:]
        c, d = m[1:, :-1], m[1:, 1:]
        p1 = a & d & ~b & ~c
        p2 = b & c & ~a & ~d
        if not (p1.any() or p2.any()):
            return m
        m[:-1, 1:] |= p1
        m[:-1, :-1] |= p2


def _island_center(mask: np.ndarray) -> tuple[int, int]:
    """Centre of an island: its centroid, or the deepest pixel when the centroid falls outside (C or ring shapes)."""
    rr, cc = np.nonzero(mask)
    r0, r1, c0, c1 = rr.min(), rr.max() + 1, cc.min(), cc.max() + 1
    sub = mask[r0:r1, c0:c1]
    cr, ccn = int(round(rr.mean())) - r0, int(round(cc.mean())) - c0
    if not sub[cr, ccn]:
        cr, ccn = np.unravel_index(int(np.argmax(ndi.distance_transform_edt(np.pad(sub, 1))[1:-1, 1:-1])), sub.shape)
    return int(cr + r0), int(ccn + c0)


def _connect_islands(mat: np.ndarray, bridge_px: int, seed: tuple[int, int] = (0, 0), center: bool = False):
    """Connect every material component to the main component (the one containing `seed`, the frame by default) through hole pixels.

    Returns (mat, bridges, rise, blen); blen is the bridge path length in pixels on bridge pixels. `rise` is NaN except on bridge pixels, where it goes from 0 (next to
    the island) to 1 (next to the part it connects to): the fraction of the plate height at which the
    ramp reaches that pixel."""
    mat = mat.copy()
    h, w = mat.shape
    bridges = 0
    rise = np.full(mat.shape, np.nan, np.float32)
    blen = np.full(mat.shape, np.nan, np.float32)
    se = np.ones((bridge_px, bridge_px), bool)
    while True:
        lab, n = label4(mat)
        if n <= 1:
            return mat, bridges, rise, blen
        main = lab == lab[seed]
        dist = np.full(mat.shape, -1, np.int32)
        dist[main] = 0
        frontier = np.flatnonzero(main.ravel())
        flat_lab = lab.ravel()
        flat_hole = (~mat).ravel()
        flat_dist = dist.ravel()
        layer = 0
        hits: dict[int, int] = {}
        while frontier.size and not hits:
            layer += 1
            r, c = np.divmod(frontier, w)
            nxt = []
            for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                rr, cc = r + dr, c + dc
                ok = (rr >= 0) & (rr < h) & (cc >= 0) & (cc < w)
                idx = rr[ok] * w + cc[ok]
                src = frontier[ok]
                fresh = flat_dist[idx] < 0
                idx, src = idx[fresh], src[fresh]
                is_hole = flat_hole[idx]
                nxt.append(idx[is_hole])
                for o, s_ in zip(idx[~is_hole], src[~is_hole]):
                    hits.setdefault(int(flat_lab[o]), (int(s_), int(o)))
            if nxt:
                nxt = np.unique(np.concatenate(nxt))
                flat_dist[nxt] = layer
            frontier = nxt if len(nxt) else np.empty(0, np.int64)
        if not hits:
            mat &= main        # unreachable (cannot happen with a frame)
            return mat, bridges, rise, blen
        before = mat.copy()
        for s0, o0 in hits.values():
            off = 0.0
            if center:      # the bridge starts at the island's centre: its rise counts from there
                cr, cc_ = _island_center(lab == flat_lab[o0])
                off = float(np.hypot(cr - o0 // w, cc_ - o0 % w))
            cur, path = s0, []
            while flat_dist[cur] > 0:
                path.append(cur)
                r0, c0 = divmod(cur, w)
                for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    rr, cc = r0 + dr, c0 + dc
                    if 0 <= rr < h and 0 <= cc < w and flat_dist[rr * w + cc] == flat_dist[cur] - 1:
                        cur = rr * w + cc
                        break
            bridges += 1
            pr, pc = np.divmod(np.array(path), w)
            d = flat_dist[np.array(path)].astype(np.float32)
            dk = d.max()
            f = (off + dk - d) / (off + dk - 1) if dk > 1 else np.ones_like(d)
            b = bridge_px + 1
            r0, r1 = max(0, pr.min() - b), min(h, pr.max() + b + 1)
            c0, c1 = max(0, pc.min() - b), min(w, pc.max() + b + 1)
            pm = np.zeros((r1 - r0, c1 - c0), bool)
            pm[pr - r0, pc - c0] = True
            fv = np.zeros(pm.shape, np.float32)
            fv[pr - r0, pc - c0] = f
            _, (ir, ic) = ndi.distance_transform_edt(~pm, return_indices=True)
            fn = fv[ir, ic]
            region = ndi.binary_dilation(pm, structure=se) & ~before[r0:r1, c0:c1]
            cur_r = rise[r0:r1, c0:c1]
            upd = region & (np.isnan(cur_r) | (fn < cur_r))
            cur_r[upd] = fn[upd]
            blen[r0:r1, c0:c1][upd] = dk + off
        mat |= ~np.isnan(rise)


def _paint_extra(mat, rise, blen, path, fdist, w, bridge_px, off=0.0):
    """Write one bridge along `path` (pixel indices from the island outwards; `fdist` counts steps from the island, 1 = first hole)."""
    h = mat.shape[0]
    pr, pc = np.divmod(np.array(path), w)
    d = fdist[np.array(path)].astype(np.float32)
    dk = d.max()
    f = (off + d - 1) / (off + dk - 1) if dk > 1 else np.zeros_like(d)
    b = bridge_px + 1
    r0, r1 = max(0, pr.min() - b), min(h, pr.max() + b + 1)
    c0, c1 = max(0, pc.min() - b), min(w, pc.max() + b + 1)
    pm = np.zeros((r1 - r0, c1 - c0), bool)
    pm[pr - r0, pc - c0] = True
    fv = np.zeros(pm.shape, np.float32)
    fv[pr - r0, pc - c0] = f
    _, (ir, ic) = ndi.distance_transform_edt(~pm, return_indices=True)
    region = ndi.binary_dilation(pm, structure=np.ones((bridge_px, bridge_px), bool)) & ~mat[r0:r1, c0:c1]
    cur = rise[r0:r1, c0:c1]
    upd = region & np.isnan(cur)
    cur[upd] = fv[ir, ic][upd]
    blen[r0:r1, c0:c1][upd] = dk + off
    return int(dk)


def _bridge_islands(mat: np.ndarray, bridge_px: int, seed: tuple[int, int] = (0, 0), per_island: int = 1, center: bool = False):
    """Connect every island to the main component, with `per_island` (1-6) separate bridges for each island.
    Returns (mat, bridges, rise, blen) like `_connect_islands`."""
    lab0, n0 = label4(mat)
    mat, bridges, rise, blen = _connect_islands(mat, bridge_px, seed, center)
    per_island = max(1, min(6, int(per_island)))
    if per_island == 1 or n0 <= 1:
        return mat, bridges, rise, blen
    h, w = mat.shape
    keep_own = bridge_px + 2
    se_other = np.ones((2 * bridge_px + 1,) * 2, bool)
    # extra bridges: each is the shortest path from the island that stays clear of every other bridge
    # (and well away from this island's own bridges, so they leave in different directions)
    for isl in range(1, n0 + 1):
        if isl == lab0[seed]:
            continue
        src = lab0 == isl
        sizes0 = [int(np.nanmin(blen[ndi.binary_dilation(src, iterations=2)])) if np.isfinite(blen[ndi.binary_dilation(src, iterations=2)]).any() else 0]
        limit = 4 * max(sizes0[0], bridge_px) + 4 * bridge_px
        own = ndi.binary_dilation(src, iterations=bridge_px + 2) & ~np.isnan(rise)
        for _ in range(per_island - 1):
            br = ~np.isnan(rise)
            blocked = ndi.binary_dilation(br, structure=se_other) | ndi.binary_dilation(own, iterations=keep_own)
            dist = np.full(mat.shape, -1, np.int32)
            dist[src] = 0
            fd = dist.ravel()
            hole = (~mat & ~blocked).ravel()
            solid = (mat & ~src & ~br).ravel()
            frontier = np.flatnonzero(src.ravel())
            hit, layer = -1, 0
            while frontier.size and hit < 0 and layer < limit:
                layer += 1
                r, c = np.divmod(frontier, w)
                nxt = []
                for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    rr, cc = r + dr, c + dc
                    ok = (rr >= 0) & (rr < h) & (cc >= 0) & (cc < w)
                    idx = rr[ok] * w + cc[ok]
                    fresh = fd[idx] < 0
                    idx = idx[fresh]
                    if hit < 0 and solid[idx].any():
                        hit = int(frontier[ok][fresh][np.argmax(solid[idx])])
                    nxt.append(idx[hole[idx]])
                frontier = np.unique(np.concatenate(nxt)) if nxt else np.empty(0, np.int64)
                fd[frontier] = layer
            if hit < 0 or fd[hit] < 1:
                break
            path, cur = [], hit
            while fd[cur] > 0:
                path.append(cur)
                r0, c0 = divmod(cur, w)
                for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    rr, cc = r0 + dr, c0 + dc
                    if 0 <= rr < h and 0 <= cc < w and fd[rr * w + cc] == fd[cur] - 1:
                        cur = rr * w + cc
                        break
            before = ~np.isnan(rise)
            off = 0.0
            if center:
                cr, cc_ = _island_center(src)
                off = float(np.hypot(cr - path[-1] // w, cc_ - path[-1] % w))
            _paint_extra(mat, rise, blen, path, fd, w, bridge_px, off)
            own |= ~np.isnan(rise) & ~before
            bridges += 1
    mat = mat | ~np.isnan(rise)
    return mat, bridges, rise, blen


def flare_layers(bridge_px: float, d_px: float, base_layers: int) -> int:
    """Layers an Island-bridge bridge needs at its top end to widen by one bridge width on each side at the overhang angle."""
    return int(min(max(base_layers - 3, 0), np.ceil(bridge_px / (0.75 * d_px)))) if d_px > 0 else 0


def build_mask(img: Image.Image, p: Params):
    """Return (pre-bridge mask, bridged mask, rise map, stats, pixel size in mm, bridge width px)."""
    res = p.resolution
    scale = res / max(img.size)
    size = (max(1, round(img.width * scale)), max(1, round(img.height * scale)))
    gray = np.asarray(img.convert("L").resize(size, Image.LANCZOS))
    t = otsu(gray) if p.threshold is None else int(p.threshold)
    dark = gray <= t
    paint = ~dark if p.invert else dark           # paint == hole in the stencil
    art_w_mm = p.width_mm - 2 * p.margin_mm
    if art_w_mm <= 0:
        raise ValueError(f"margin too large for the stencil width: width {p.width_mm:g} mm, margin {p.margin_mm:g} mm (the margin must be less than half the width)")
    px_mm = art_w_mm / size[0] if size[0] >= size[1] else (art_w_mm / size[1])
    min_area = max(1, round((p.min_feature_mm / px_mm) ** 2))
    paint = _remove_small(paint, min_area)
    mat = _remove_small(~paint, min_area)         # tiny material specks become holes
    pad = max(1, int(np.ceil(p.margin_mm / px_mm - 1e-6)) if p.pad_count > 0 else round(p.margin_mm / px_mm))   # pads need the full margin
    mat0 = np.pad(mat, pad, constant_values=True)
    bridge_px = max(2, round(p.bridge_mm / px_mm))
    deleted = 0
    deleted_mask = np.zeros(mat0.shape, bool)
    if p.min_island_mm2 > 0:
        lab, n = label4(mat0)
        area = np.bincount(lab.ravel(), minlength=n + 1) * px_mm * px_mm
        drop = area < p.min_island_mm2
        drop[0] = False
        drop[lab[0, 0]] = False                   # never drop the frame
        deleted = int(drop.sum())
        deleted_mask = drop[lab]
        mat0 = mat0 & ~deleted_mask
    from . import tiling
    H_mm, W_mm = mat0.shape[0] * px_mm, mat0.shape[1] * px_mm
    xs, ys = tiling.grid(W_mm, H_mm, p.tile_max_x_mm, p.tile_max_y_mm)
    tiled = len(xs) > 2 or len(ys) > 2
    if tiled:
        if p.connector_diameter_mm > 0:
            xs, ys = tiling.optimize_grid(mat0, px_mm, xs, ys, p.tile_max_x_mm, p.tile_max_y_mm)
        # seams sit on pixel boundaries so every tile is exactly a block of the bitmap
        xs = [0.0] + [round(x / px_mm) * px_mm for x in xs[1:-1]] + [W_mm]
        ys = [0.0] + [round(y / px_mm) * px_mm for y in ys[1:-1]] + [H_mm]
        # each tile is its own part: bridges are recomputed inside every tile, towards the tile's largest piece
        matb = np.zeros_like(mat0)
        rise = np.full(mat0.shape, np.nan, np.float32)
        blen = np.full(mat0.shape, np.nan, np.float32)
        islands = bridges = 0
        isl_mask = np.zeros(mat0.shape, bool)
        hh = mat0.shape[0]
        cols = [round(x / px_mm) for x in xs]
        rows = [hh - round(y / px_mm) for y in reversed(ys)]      # image rows run top-down, seam y runs bottom-up
        for r0, r1 in zip(rows, rows[1:]):
            for c0, c1 in zip(cols, cols[1:]):
                sub = mat0[r0:r1, c0:c1]
                lab, n = label4(sub)
                if n == 0:
                    continue
                sizes = np.bincount(lab.ravel())
                sizes[0] = 0
                seed = np.unravel_index(int(np.argmax(lab == int(np.argmax(sizes)))), sub.shape)
                m, b, ri, bl = _bridge_islands(sub, bridge_px, (int(seed[0]), int(seed[1])), p.bridges_per_island, p.z_bridging == "island")
                matb[r0:r1, c0:c1], rise[r0:r1, c0:c1], blen[r0:r1, c0:c1] = m, ri, bl
                isl_mask[r0:r1, c0:c1] = (lab > 0) & (lab != lab[seed])
                islands += n - 1
                bridges += b
    else:
        islands = label4(mat0)[1] - 1
        matb, bridges, rise, blen = _bridge_islands(mat0, bridge_px, (0, 0), p.bridges_per_island, p.z_bridging == "island")
        lab0 = label4(mat0)[0]
        isl_mask = (lab0 > 0) & (lab0 != lab0[0, 0])
    grow = p.z_bridging in ("grow", "island")
    top_span = 0.0
    lh_g = p.layer_height_mm if p.layer_height_mm and p.layer_height_mm > 0 else 0.2
    ang = p.max_overhang_deg or (45.0 if grow else 0.0)
    if p.raised_bridges and ang > 0 and not p.flip and bridges:
        lh = p.layer_height_mm if p.layer_height_mm and p.layer_height_mm > 0 else 0.2
        base = max(1, round(p.thickness_mm / lh))
        n_steps = max(1, base - 1 if p.z_bridging == "steps" else base - 2)
        d_mm = lh * np.tan(np.radians(min(ang, 89.0)))
        length = blen * px_mm
        # bridges longer than d*n_steps climb at the steepest allowed slope and finish with a flat span
        rise = np.minimum(1.0, rise * np.maximum(1.0, length / (d_mm * n_steps))).astype(np.float32)
        top_span = float(np.nanmax(np.maximum(length - d_mm * n_steps, 0))) if np.isfinite(length).any() else 0.0
    stats = {
        "islands_deleted": deleted,
        "pad_px": int(pad),
        "_grid": (xs, ys),
        "_deleted": deleted_mask,
        "_islands": isl_mask,
        "_grow_px": float(lh_g * np.tan(np.radians(min(ang, 89.0))) / px_mm) if grow else 0.0,
        "max_flat_span_mm": round(top_span, 2),
        "islands_found": int(islands),
        "bridges_added": int(bridges),
        "grid": [int(mat0.shape[1]), int(mat0.shape[0])],
        "pixel_mm": round(float(px_mm), 4),
        "threshold": int(t),
    }
    return mat0, matb, rise, stats, px_mm, bridge_px


def trace_loops(mask: np.ndarray) -> list[np.ndarray]:
    """Exact pixel-boundary polygons (x right, y up, pixel units). Outer loops CCW, holes CW."""
    h, w = mask.shape
    m = np.pad(mask, 1)
    core = m[1:-1, 1:-1]
    R, C = np.nonzero(core)
    y0 = h - R - 1
    x0 = C
    vid = lambda x, y: y * (w + 1) + x
    starts, ends = [], []
    for nb, (ax, ay, bx, by) in (
        (m[2:, 1:-1], (x0, y0, x0 + 1, y0)),          # below is empty: bottom edge, going +x
        (m[1:-1, 2:], (x0 + 1, y0, x0 + 1, y0 + 1)),  # right
        (m[:-2, 1:-1], (x0 + 1, y0 + 1, x0, y0 + 1)), # above
        (m[1:-1, :-2], (x0, y0 + 1, x0, y0)),         # left
    ):
        sel = ~nb[R, C]
        starts.append(vid(ax[sel], ay[sel]))
        ends.append(vid(bx[sel], by[sel]))
    nxt = dict(zip(np.concatenate(starts).tolist(), np.concatenate(ends).tolist()))
    loops = []
    while nxt:
        v0, v = nxt.popitem()
        ids = [v0]
        while v != v0:
            ids.append(v)
            v = nxt.pop(v)
        a = np.array(ids)
        P = np.column_stack([a % (w + 1), a // (w + 1)]).astype(float)
        d1 = P - np.roll(P, 1, 0)
        d2 = np.roll(P, -1, 0) - P
        keep = (d1[:, 0] * d2[:, 1] - d1[:, 1] * d2[:, 0] != 0) | ((d1 * d2).sum(1) < 0)
        loops.append(P[keep])
    return loops


def _area(P):
    return 0.5 * float(np.dot(P[:, 0], np.roll(P[:, 1], -1)) - np.dot(P[:, 1], np.roll(P[:, 0], -1)))


def smooth_loop(P: np.ndarray, sigma: float, ds: float):
    """Gaussian-smooth a closed polygon along its length. Returns None if it collapses."""
    Q = np.vstack([P, P[:1]])
    seg = np.linalg.norm(np.diff(Q, axis=0), axis=1)
    s = np.concatenate([[0], np.cumsum(seg)])
    L = s[-1]
    n = max(8, int(L / ds))
    t = np.linspace(0, L, n, endpoint=False)
    x = np.interp(t, s, Q[:, 0])
    y = np.interp(t, s, Q[:, 1])
    g = sigma / (L / n)
    out = np.column_stack([gaussian_filter1d(x, g, mode="wrap"), gaussian_filter1d(y, g, mode="wrap")])
    a0, a1 = _area(P), _area(out)
    if a1 * a0 <= 0 or abs(a1) < 0.3 * abs(a0):
        return None
    return out


def build_solid(masks: list[np.ndarray], px: float, lh: float, smooth_px: float, tick=None):
    """Union of one extruded outline per run of identical layers (manifold, watertight by construction)."""
    import manifold3d as m3d
    parts, k = [], 0
    while k < len(masks):
        k1 = k + 1
        while k1 < len(masks) and np.array_equal(masks[k1], masks[k]):
            k1 += 1
        loops = trace_loops(masks[k])
        if smooth_px > 0:
            loops = [q for q in (smooth_loop(P, smooth_px, 0.5) for P in loops) if q is not None]
        cs = m3d.CrossSection([L * px for L in loops], m3d.FillRule.NonZero)
        parts.append(m3d.Manifold.extrude(cs, float(k1 - k)).translate((0.0, 0.0, float(k))))
        k = k1
        if tick:
            tick(k / len(masks))
    # layers are built on an integer z grid so coplanar faces fuse exactly, then scaled to mm
    return m3d.Manifold.batch_boolean(parts, m3d.OpType.Add).scale((1.0, 1.0, lh))


def build_solid_ramp(mat0, matb, rise, px, base_layers, extra, smooth_px, tick=None, stepped=False):
    """Bottom layer = mat0 (islands), top layer = matb (no islands); each bridge is a prism whose
    underside is one continuous slope from the island (z=0) up to the top layer, with no steps."""
    import manifold3d as m3d

    def extr(mask, z0, n):
        loops = trace_loops(mask)
        if smooth_px > 0:
            loops = [q for q in (smooth_loop(P, smooth_px, 0.5) for P in loops) if q is not None]
        cs = m3d.CrossSection([L * px for L in loops], m3d.FillRule.NonZero)
        return m3d.Manifold.extrude(cs, float(n)).translate((0.0, 0.0, float(z0)))

    h, w = mat0.shape
    parts = [extr(mat0, 0, base_layers), extr(matb, base_layers - 1, 1 + extra)]
    if tick:
        tick(0.1)
    br = ~np.isnan(rise)
    if base_layers > 1 and br.any():
        # ramp height at pixel corners: average of the neighbouring bridge pixels
        val = np.where(br, rise, 0.0)
        S = np.zeros((h + 1, w + 1))
        N = np.zeros((h + 1, w + 1))
        for dr in (0, 1):
            for dc in (0, 1):
                S[dr:dr + h, dc:dc + w] += val
                N[dr:dr + h, dc:dc + w] += br
        # Underside height in layer units, kept within [1, base_layers-1] so the first layer is exactly mat0
        # and the top layer is exactly matb.
        u = S / np.maximum(N, 1) * max(base_layers - 2, 0)
        if stepped:      # plateau for the first half of each layer, slope over the second half
            fl = np.floor(u + 1e-9)
            u = fl + np.clip(2 * (u - fl) - 1, 0, 1)
        Z = 1.0 + u
        top = float(base_layers)
        rows, cols = np.nonzero(br)
        n = len(rows)
        for i, (r, c) in enumerate(zip(rows.tolist(), cols.tolist())):
            x0, x1 = c * px, (c + 1) * px
            ya, yb = (h - r - 1) * px, (h - r) * px
            pts = [(x0, yb, Z[r, c]), (x1, yb, Z[r, c + 1]), (x1, ya, Z[r + 1, c + 1]), (x0, ya, Z[r + 1, c]),
                   (x0, yb, top), (x1, yb, top), (x1, ya, top), (x0, ya, top)]
            parts.append(m3d.Manifold.hull_points(np.array(pts, float)))
            if tick and i % 50 == 0:
                tick(0.1 + 0.6 * i / n)
    return m3d.Manifold.batch_boolean(parts, m3d.OpType.Add)


def overhang_area(solid, lh: float, n_layers: int, angle_deg: float) -> float:
    """Area (mm2, summed over layers) that sticks out more than layer_height*tan(angle) past the layer below."""
    import manifold3d as m3d
    d = lh * float(np.tan(np.radians(min(angle_deg, 89.0))))
    total, prev = 0.0, None
    for k in range(n_layers):
        cur = solid.slice((k + 0.5) * lh)
        if prev is not None:
            sup = prev.offset(d + 1e-4, m3d.JoinType.Miter, 2.0, 0)
            total += (cur - sup).area()
        prev = cur
    return total


def triangles_to_stl(tris: np.ndarray) -> bytes:
    n = len(tris)
    nrm = np.cross(tris[:, 1] - tris[:, 0], tris[:, 2] - tris[:, 0])
    ln = np.linalg.norm(nrm, axis=1, keepdims=True)
    nrm = np.divide(nrm, ln, out=np.zeros_like(nrm), where=ln > 0).astype(np.float32)
    rec = np.zeros(n, dtype=[("n", "<f4", 3), ("v", "<f4", (3, 3)), ("a", "<u2")])
    rec["n"], rec["v"] = nrm, tris
    return b"graffiti stencil".ljust(80, b" ") + struct.pack("<I", n) + rec.tobytes()


def check_watertight(tri_idx: np.ndarray) -> bool:
    """Every directed edge of the indexed mesh must have exactly one opposite twin."""
    e = np.concatenate([tri_idx[:, [0, 1]], tri_idx[:, [1, 2]], tri_idx[:, [2, 0]]]).astype(np.int64)
    base = e.max() + 1
    directed = e[:, 0] * base + e[:, 1]
    rev = e[:, 1] * base + e[:, 0]
    u, cnt = np.unique(directed, return_counts=True)
    return bool(cnt.max() == 1 and np.isin(rev, u).all())


def layer_masks(mat0, matb, rise, n_layers):
    """Mask of every layer, bottom (wall side) first. With one layer the bridges are full height."""
    if n_layers <= 1:
        return [_fix_diagonals(matb)]
    kmin = np.ceil(np.nan_to_num(rise, nan=0.0) * (n_layers - 1) - 1e-6)
    br = ~np.isnan(rise)
    return [_fix_diagonals(mat0 | (br & (kmin <= k))) for k in range(n_layers)]


def fillet_bridges(mat0, rise, bridge_px):
    """Flare each bridge where it meets material: hole pixels near a bridge end are added to the bridge (same rise),
    by up to half a bridge width at the junction, tapering to nothing over two bridge widths. This gives the
    attachment a wide, rounded root instead of a sharp corner."""
    from scipy.ndimage import distance_transform_edt
    br = ~np.isnan(rise)
    if not br.any():
        return rise
    d_br, idx = distance_transform_edt(~br, return_indices=True)
    d_mat = distance_transform_edt(~mat0)
    root = 2.0 * bridge_px
    add = (~mat0) & (~br) & (d_mat < root) & (d_br <= 0.5 * bridge_px * (1.0 - d_mat / root))
    out = rise.copy()
    out[add] = rise[idx[0][add], idx[1][add]]
    return out


ISLAND_GROW_MM2 = 2.0      # island-bridge: layer area at which an island's bridges start growing


def build_solid_grow(mat0, rise, islands, px, base_layers, extra, smooth_px, d_px, tick=None, cuts=None, island=False, bridge_px=2):
    """Islands and bridges widen sideways by `d_px` pixels per layer, so every flank stays within the overhang angle.
    Instead of stacking layers, the flank is one heightfield surface: its height at a pixel corner is the layer at which
    the island (or bridge) reaches that corner, min over the seeds of (seed layer + distance / d_px). The first layer is
    exactly `mat0` and the top layer is the island-free mask. Units: x/y in mm, z in layers."""
    import manifold3d as m3d
    from scipy.ndimage import distance_transform_edt
    mat0 = _fix_diagonals(mat0)
    h, w = mat0.shape
    top1 = float(base_layers - 1)
    br = ~np.isnan(rise)

    # Each pixel is split into n x n cells: a ridge sampled on the pixel grid would be a flat pixel-wide plateau that
    # closes a gap in a single layer, so the flank surface is sampled finely enough for two flanks to meet within one layer.
    n = max(1, int(np.ceil(1.0 / (1.6 * d_px))))
    hf, wf = h * n, w * n
    up = lambda m: np.repeat(np.repeat(m, n, 0), n, 1)

    # Lowest underside that keeps every flank within the overhang angle: it climbs from any vertical wall (island or
    # frame) at `d_px` pixels per layer. Bridges sit at that height; islands start at layer 1 and bridges grow sideways from there.
    d_f = d_px * n * 0.75     # cells per layer; the margin covers diagonal triangles being steeper than their edges
    M, B, I = up(mat0), up(br), up(islands & mat0)
    RV = up(np.nan_to_num(rise))
    Z = np.full((hf + 1, wf + 1), np.inf)
    Zc_all = np.full((hf + 1, wf + 1), np.inf)
    # every tile is its own part: growth never crosses a seam
    for r0, r1, c0, c1 in (cuts or [(0, h, 0, w)]):
        sl = (slice(r0 * n, r1 * n), slice(c0 * n, c1 * n))
        cm, cb, ci = M[sl], B[sl], I[sl]
        def cor(m):
            out = np.zeros((m.shape[0] + 1, m.shape[1] + 1), bool)
            for dr in (0, 1):
                for dc in (0, 1):
                    out[dr:dr + m.shape[0], dc:dc + m.shape[1]] |= m
            return out
        if island:
            # islands grow sideways at the overhang angle until their layer area reaches ISLAND_GROW_MM2; from that layer on only
            # their bridges keep growing (also sideways), so a bridge reaches the frame no later than the top layer
            from scipy.ndimage import label as _label
            lab, nl = _label(ci, structure=[[0, 1, 0], [1, 1, 1], [0, 1, 0]])
            cc = cor(ci)
            dd, (ir, ic) = distance_transform_edt(~cc, return_indices=True)
            Lc = np.zeros(cc.shape, int)
            for dr in (0, 1):
                for dc in (0, 1):
                    np.maximum(Lc[dr:dr + lab.shape[0], dc:dc + lab.shape[1]], lab, out=Lc[dr:dr + lab.shape[0], dc:dc + lab.shape[1]])
            near = Lc[ir, ic]
            cell2 = (px / n) ** 2
            A0 = np.bincount(lab.ravel(), minlength=nl + 1).astype(float)
            per = np.zeros(nl + 1)
            pm = np.pad(lab, 1)
            for sh in ((0, 1), (0, -1), (1, 0), (-1, 0)):
                nb = np.roll(pm, sh, (0, 1))[1:-1, 1:-1]
                per += np.bincount(lab[(lab > 0) & (nb != lab)], minlength=nl + 1)
            tgt = ISLAND_GROW_MM2 / cell2
            with np.errstate(invalid="ignore"):
                rr = (-per + np.sqrt(per ** 2 - 4 * np.pi * (A0 - tgt))) / (2 * np.pi)
            rr = np.where(A0 >= tgt, 0.0, rr)
            cap = np.maximum(1, np.ceil(rr / d_f)) * d_f
            Zi = np.where((dd <= cap[near] + 1e-9) & (near > 0), 1.0 + dd / d_f, np.inf)
            cbc = cor(cb) & ~cc
            # every bridge slope is set per connected island+bridge group, so the underside never jumps along a bridge
            comp, nc = _label(ci | cb, structure=[[0, 1, 0], [1, 1, 1], [0, 1, 0]])
            Cc = np.zeros(cc.shape, int)
            for dr in (0, 1):
                for dc in (0, 1):
                    np.maximum(Cc[dr:dr + comp.shape[0], dc:dc + comp.shape[1]], comp, out=Cc[dr:dr + comp.shape[0], dc:dc + comp.shape[1]])
            # distance along the bridge from its island: the underside rises with the bridge's own length, never with the
            # straight-line distance to some other island that happens to pass nearby
            from scipy.ndimage import binary_dilation
            from scipy.sparse import coo_matrix
            from scipy.sparse.csgraph import dijkstra
            node = cbc | (cc & binary_dilation(cbc, np.ones((3, 3), bool)))
            nid = -np.ones(node.shape, int)
            nid[node] = np.arange(node.sum())
            ra, ca, wa = [], [], []
            for dr, dc in ((0, 1), (1, 0), (1, 1), (1, -1)):
                A_ = node[:node.shape[0] - dr, max(0, -dc):node.shape[1] - max(0, dc)]
                B_ = node[dr:, max(0, dc):node.shape[1] - max(0, -dc)]
                ia = nid[:node.shape[0] - dr, max(0, -dc):node.shape[1] - max(0, dc)]
                ib = nid[dr:, max(0, dc):node.shape[1] - max(0, -dc)]
                ok = A_ & B_ & ~(cc[:node.shape[0] - dr, max(0, -dc):node.shape[1] - max(0, dc)] & cc[dr:, max(0, dc):node.shape[1] - max(0, -dc)])
                ra.append(ia[ok]); ca.append(ib[ok]); wa.append(np.full(ok.sum(), np.hypot(dr, dc)))
            ra, ca, wa = np.concatenate(ra), np.concatenate(ca), np.concatenate(wa)
            G = coo_matrix((wa, (ra, ca)), shape=(node.sum(),) * 2).tocsr()
            sd = dijkstra(G, directed=False, indices=nid[cc & node], min_only=True)
            dd = np.full(node.shape, np.inf)
            dd[node] = sd
            dd[~np.isfinite(dd)] = 0.0
            lmax = np.zeros(nc + 1)
            np.maximum.at(lmax, Cc[cbc], dd[cbc])
            dE = np.maximum(d_f, lmax / max(top1 - 2.0, 0.5))
            Zs = np.where(cbc, 1.0 + dd / dE[Cc], np.inf)
            d2, (jr, jc) = distance_transform_edt(~cbc, return_indices=True)
            Zt = np.minimum(Zi, Zs[jr, jc] + d2 / d_f)
            Zc_all[r0 * n:r1 * n + 1, c0 * n:c1 * n + 1] = Zi
        else:
            Zw = 1.0 + distance_transform_edt(~cor(cm)) / d_f
            lvl = np.where(cor(cb), np.ceil(np.minimum(Zw, top1) * 2) / 2, np.where(cor(ci), 1.0, np.inf))
            Zt = np.full(lvl.shape, np.inf)
            for lv in np.unique(lvl[np.isfinite(lvl)]):
                Zt = np.minimum(Zt, lv + distance_transform_edt(~(lvl == lv)) / d_f)
        win = Z[r0 * n:r1 * n + 1, c0 * n:c1 * n + 1]
        np.minimum(win, Zt, out=win)
    if tick:
        tick(0.2)
    zmax = np.full((h, w), -np.inf)
    for i in range(n + 1):
        for j in range(n + 1):
            zmax = np.maximum(zmax, Z[i::n, j::n][:h, :w])
    reach = True
    if island:      # a bridge flares to at most 3x its width, and never across a tile seam
        reach = np.zeros((h, w), bool)
        for r0, r1, c0, c1 in (cuts or [(0, h, 0, w)]):
            reach[r0:r1, c0:c1] = distance_transform_edt(~br[r0:r1, c0:c1]) <= bridge_px
        grown = np.full((h, w), -np.inf)
        for i in range(n + 1):
            for j in range(n + 1):
                grown = np.maximum(grown, Zc_all[i::n, j::n][:h, :w])
        reach |= np.isfinite(grown)
    P = _fix_diagonals(mat0 | br | ((zmax <= top1 + 1e-6) & reach))
    S = _fix_diagonals(P & ~mat0)       # the flank mesh needs 2-manifold corners on its own
    Z = np.minimum(Z, top1)

    def extr(mask, z0, n, smooth):
        loops = trace_loops(mask)
        if smooth > 0:
            loops = [q for q in (smooth_loop(L, smooth, 0.5) for L in loops) if q is not None]
        cs = m3d.CrossSection([L * px for L in loops], m3d.FillRule.NonZero)
        return m3d.Manifold.extrude(cs, float(n)).translate((0.0, 0.0, float(z0)))

    parts = [extr(mat0, 0, base_layers, smooth_px), extr(P, base_layers - 1, 1 + extra, smooth_px)]
    if S.any():
        top = float(base_layers)
        X = np.arange(wf + 1) * (px / n)
        Y = (hf - np.arange(hf + 1)) * (px / n)
        ids = np.arange((hf + 1) * (wf + 1)).reshape(hf + 1, wf + 1)
        nv = (hf + 1) * (wf + 1)
        V = np.concatenate([
            np.stack([np.broadcast_to(X, Z.shape).ravel(), np.broadcast_to(Y[:, None], Z.shape).ravel(), Z.ravel()], 1),
            np.stack([np.broadcast_to(X, Z.shape).ravel(), np.broadcast_to(Y[:, None], Z.shape).ravel(), np.full(Z.size, top)], 1)])
        r, c = np.nonzero(up(S))
        TL, TR, BL, BR = ids[r, c], ids[r, c + 1], ids[r + 1, c], ids[r + 1, c + 1]
        tris = [np.stack([BL + nv, BR + nv, TR + nv], 1), np.stack([BL + nv, TR + nv, TL + nv], 1),
                np.stack([BL, TR, BR], 1), np.stack([BL, TL, TR], 1)]
        Sp = np.pad(up(S), 1)
        north, south, west, east = ~Sp[r, c + 1], ~Sp[r + 2, c + 1], ~Sp[r + 1, c], ~Sp[r + 1, c + 2]
        tris += [np.stack([TL, TL + nv, TR + nv], 1)[north], np.stack([TL, TR + nv, TR], 1)[north],
                 np.stack([BL, BR, BR + nv], 1)[south], np.stack([BL, BR + nv, BL + nv], 1)[south],
                 np.stack([TL, BL + nv, TL + nv], 1)[west], np.stack([TL, BL, BL + nv], 1)[west],
                 np.stack([BR, TR, TR + nv], 1)[east], np.stack([BR, TR + nv, BR + nv], 1)[east]]
        F = np.concatenate(tris).astype(np.uint32)
        prism = m3d.Manifold(m3d.Mesh(V.astype(np.float32), F))
        if prism.volume() < 0:
            prism = m3d.Manifold(m3d.Mesh(V.astype(np.float32), F[:, ::-1].copy()))
        if prism.status() != m3d.Error.NoError:
            raise RuntimeError(f"could not build the island flanks: {prism.status()}")
        # the flank mesh is pixelated: trim it to the smoothed outline
        parts.append(prism ^ extr(P, 0, base_layers + extra, smooth_px))
    solid = m3d.Manifold.batch_boolean(parts, m3d.OpType.Add)
    pieces = solid.decompose()
    if len(pieces) > 1:     # zero-thickness slivers where a flank meets a wall are not geometry
        solid = m3d.Manifold.compose([q for q in pieces if q.volume() > 1e-6])
    return solid


def layer_svg(section, w: float, h: float) -> str:
    """SVG of a cross-section (mm, y flipped); material is filled, holes are cut out (even-odd)."""
    d = "".join("M" + "L".join(f"{x:.2f} {h - y:.2f}" for x, y in P) + "Z" for P in section.to_polygons())
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w:.2f} {h:.2f}">'
            f'<path fill="#d9dde3" fill-rule="evenodd" d="{d}"/></svg>')


def trace_svg(section, w: float, h: float) -> str:
    """Standalone vector artwork in mm: black material, holes cut out (even-odd), y up as in the source image."""
    d = "".join("M" + "L".join(f"{x:.3f} {h - y:.3f}" for x, y in P) + "Z" for P in section.to_polygons())
    return (f'<?xml version="1.0" encoding="UTF-8"?>\n<svg xmlns="http://www.w3.org/2000/svg" width="{w:.2f}mm" height="{h:.2f}mm" '
            f'viewBox="0 0 {w:.2f} {h:.2f}"><path fill="#000" fill-rule="evenodd" d="{d}"/></svg>\n')


THREADS = {"M4": (4.0, 0.7), "M6": (6.0, 1.0), "M8": (8.0, 1.25), "M10": (10.0, 1.5)}   # (major diameter, pitch) in mm
PAD_WALL_MM = 2.0                                 # solid ring around the thread: pad diameter = major diameter + 2 * wall


def pad_diameter(thread: str) -> float:
    return THREADS[thread][0] + 2 * PAD_WALL_MM


def pad_centers(count: int, W: float, H: float, margin: float):
    """Pad centres in plate mm (y up, image top at y = H). Always in the middle of the frame band."""
    c = margin / 2
    if count == 1:
        return [(W / 2, H - c)]
    if count == 2:
        return [(W / 2, H - c), (W / 2, c)]
    if count == 3:
        return [(c, H - c), (W - c, H - c), (W / 2, c)]
    return [(c, H - c), (W - c, H - c), (c, c), (W - c, c)]


def thread_void(thread: str, clearance: float, z0: float, z1: float):
    """Right-handed internal thread cavity (the external thread shape plus clearance), made by twisting a polar cross-section."""
    import manifold3d as m3d
    d, pitch = THREADS[thread]
    r_maj = d / 2 + clearance
    depth = 0.54 * pitch
    n = 96
    pts = []
    for i in range(n):
        t = i / n                                  # position along one pitch
        if t < 0.0625 or t >= 0.9375:
            f = 1.0                                # crest of the void (thread root of the hole), P/8 wide
        elif t < 0.375:
            f = 1 - (t - 0.0625) / 0.3125          # 60 degree flank
        elif t <= 0.625:
            f = 0.0                                # flat at the minor diameter, P/4 wide
        else:
            f = (t - 0.625) / 0.3125
        r = r_maj - depth * (1 - f)
        a = 2 * np.pi * t
        pts.append((r * np.cos(a), r * np.sin(a)))
    h = z1 - z0
    cs = m3d.CrossSection([np.array(pts)], m3d.FillRule.NonZero)
    body = m3d.Manifold.extrude(cs, h, int(np.ceil(h / pitch * 20)), 360.0 * h / pitch)
    return body.translate((0.0, 0.0, z0))


def add_pads(solid, p: Params, W: float, H: float, margin: float, T: float, lh: float, centers=None):
    """Raised pads with a threaded hole on the print-top face (so they never need support). Returns (solid, centres, pad diameter)."""
    import manifold3d as m3d
    if p.pad_thread not in THREADS:
        raise ValueError(f"unknown thread {p.pad_thread!r}; use one of {', '.join(THREADS)}")
    d, pitch = THREADS[p.pad_thread]
    dia = pad_diameter(p.pad_thread)
    if margin < dia - 1e-6:
        raise ValueError(f"margin too small for {p.pad_thread} pads: the margin is {margin:.1f} mm and a {p.pad_thread} pad is {dia:g} mm wide (the pads must sit inside the margin)")
    h = 0.0 if p.flip else float(p.pad_height_mm)       # printed upside down: plain threaded holes through the margin, no raised pad
    if not p.flip and (h < lh or T + h < 4 * pitch):
        raise ValueError(f"pad height too small for an {p.pad_thread} thread: plate + pad must be at least {4 * pitch:g} mm (pad height at least {max(lh, 4 * pitch - T):.1f} mm)")
    r = min(dia / 2, margin / 2)
    cs = centers if centers is not None else pad_centers(int(p.pad_count), W, H, margin)
    for i in range(len(cs)):
        for j in range(i):
            if np.hypot(cs[i][0] - cs[j][0], cs[i][1] - cs[j][1]) < 2 * r - 1e-6:
                raise ValueError(f"the plate is too small for {p.pad_count} {p.pad_thread} pads: they would overlap")
    z_hi = T + h
    if not p.flip:
        pads = [m3d.Manifold.cylinder(h + lh, r, r, 96).translate((x, y, T - lh)) for x, y in cs]
        solid = m3d.Manifold.batch_boolean([solid] + pads, m3d.OpType.Add)
    voids = [thread_void(p.pad_thread, p.thread_clearance_mm, -0.5, z_hi + 0.5).translate((x, y, 0.0)) for x, y in cs]
    return solid - m3d.Manifold.batch_boolean(voids, m3d.OpType.Add), cs, dia


def make_stencil(data: bytes, filename: str = "", params: Params | None = None, progress=None):
    """Return (stl_bytes, stats). `progress(fraction, stage)` is called as work advances."""
    p = params or Params()
    report = progress or (lambda f, s: None)
    report(0.02, "Reading image")
    img = load_image(data, filename)
    report(0.08, "Thresholding & bridging islands")
    mat0, matb, rise, stats, px, bridge_px = build_mask(img, p)
    deleted_mask = stats.pop("_deleted")
    islands_mask, grow_px = stats.pop("_islands"), stats.pop("_grow_px")
    report(0.45, "Building layers")
    T = p.thickness_mm
    step = p.layer_height_mm if p.layer_height_mm and p.layer_height_mm > 0 else 0.2
    base_layers = max(1, round(T / step))
    lh = step                                   # layer height is exact; thickness snaps to whole layers
    extra = max(0, int(p.extra_top_layers))
    ramp = p.raised_bridges and p.z_bridging in ("ramp", "stepramp")
    grow = p.raised_bridges and p.z_bridging in ("grow", "island") and base_layers > 2
    if grow:
        rise = fillet_bridges(mat0, rise, bridge_px)
        masks = [_fix_diagonals(matb)] * base_layers
    elif p.raised_bridges and not ramp:
        masks = layer_masks(mat0, matb, rise, base_layers)
    else:
        masks = [_fix_diagonals(matb)] * base_layers
    n_layers = base_layers + extra
    T = n_layers * lh
    from . import tiling
    W, H = mat0.shape[1] * px, mat0.shape[0] * px
    xs, ys = stats.pop("_grid")
    tiled = len(xs) > 2 or len(ys) > 2
    tile_cuts = None
    if tiled:
        cc = [round(x / px) for x in xs]
        rr = [mat0.shape[0] - round(y / px) for y in reversed(ys)]
        tile_cuts = [(a, b, c, d) for a, b in zip(rr, rr[1:]) for c, d in zip(cc, cc[1:])]
    if tiled and p.connector_diameter_mm > 0:
        tiling.check_connector(p.connector_shape, p.connector_diameter_mm, T)
    sigma = p.smooth_mm / px
    factors = (1.0, 0.5, 0.25, 0.0) if sigma > 0 else (0.0,)
    solid = None
    for a, factor in enumerate(factors):
        lo, span = 0.5 + 0.4 * a / len(factors), 0.4 / len(factors)
        tick = lambda f, lo=lo, span=span: report(lo + span * f, "Extruding & merging layers")
        if grow:
            solid = build_solid_grow(mat0, rise, islands_mask, px, base_layers, extra, sigma * factor, grow_px, tick, tile_cuts, p.z_bridging == "island", bridge_px).scale((1.0, 1.0, lh))
        elif ramp:
            solid = build_solid_ramp(layer_masks(mat0, matb, rise, max(base_layers, 2))[0], _fix_diagonals(matb), rise, px, base_layers, extra,
                                     sigma * factor, tick, p.z_bridging == "stepramp").scale((1.0, 1.0, lh))
        else:
            solid = build_solid(masks + [masks[-1]] * extra, px, lh, sigma * factor, tick)
        if len(solid.decompose()) == 1:       # smoothing must never split the part
            break
    trace = trace_svg(solid.slice(lh / 2), mat0.shape[1] * px, mat0.shape[0] * px)
    plate = solid                               # overhangs are measured on the plate alone; thread flanks are not counted
    pads_info = []
    if int(p.pad_count) > 0:
        report(0.9, "Adding threaded pads")
        mg = stats["pad_px"] * px
        centers = None
        if tiled:
            centers = tiling.avoid_seams(pad_centers(int(p.pad_count), W, H, mg), min(pad_diameter(p.pad_thread) / 2, mg / 2), W, xs, ys)
        solid, pcs, pdia = add_pads(solid, p, W, H, mg, T, lh, centers)
        pads_info = [[round(x, 2), round(y, 2)] for x, y in pcs]
    if p.flip:
        # turn the part over (180 deg about the y axis, not a mirror): the printed object is the real stencil seen from behind
        def turn(m):
            return m.rotate((0.0, 180.0, 0.0)).translate((float(mat0.shape[1] * px), 0.0, float(T)))
        solid, plate = turn(solid), turn(plate)
        xs = [W - x for x in reversed(xs)]
    overhang = round(overhang_area(plate, lh, n_layers, p.max_overhang_deg or 45.0), 2)
    conn = None
    pieces = 1 if tiled else len(solid.decompose())   # counted before drilling: closed hole cavities would count as extra shells
    if tiled:
        report(0.91, "Drilling connector holes")
        if p.connector_diameter_mm > 0:
            solid, conn = tiling.drill_connectors(solid, p.connector_shape, p.connector_diameter_mm, p.connector_clearance_mm, T, xs, ys, stats["pad_px"] * px)
    report(0.92, "Checking mesh")
    mesh = solid.to_mesh()
    watertight = bool(solid.status().name == "NoError" and check_watertight(mesh.tri_verts))
    tile_parts = None
    if tiled:
        report(0.94, "Cutting tiles")
        tile_parts = tiling.split_tiles(solid, xs, ys)
        pieces = 1 + sum(sum(d.volume() > 1e-3 for d in t.decompose()) - 1 for _, t, _ in tile_parts)    # every tile must be a single piece
        mesh = tiling.exploded(tile_parts, xs, ys).to_mesh()     # the STL preview shows the tiles apart so the holes are visible
    tris = mesh.vert_properties[:, :3][mesh.tri_verts].astype(np.float32)
    stats.update(
        overhang_area_mm2=overhang,
        flipped=bool(p.flip),
        islands_remaining=int(pieces - 1),
        triangles=int(len(tris)),
        watertight=watertight,
        layers=int(n_layers),
        extra_top_layers=extra,
        z_bridging=p.z_bridging if p.raised_bridges else None,
        layer_height_mm=round(lh, 4),
        smoothing_mm=round(float(sigma * factor * px), 3),
        size_mm=[round(mat0.shape[1] * px, 2), round(mat0.shape[0] * px, 2), round(T + (p.pad_height_mm if pads_info and not p.flip else 0.0), 3)],
        pads=pads_info,
        pad_thread_turns=round(T / THREADS[p.pad_thread][1] if p.flip else (T + p.pad_height_mm) / THREADS[p.pad_thread][1], 1) if pads_info else None,
        params=asdict(p),
    )
    report(0.95, "Rendering first layer")
    stats["trace_svg"] = trace
    stats["first_layer_svg"] = layer_svg(solid.slice(lh / 2), mat0.shape[1] * px, mat0.shape[0] * px)
    ov = np.zeros(mat0.shape + (4,), np.uint8)
    ov[deleted_mask] = (80, 160, 255, 255)
    ov[~np.isnan(rise)] = (255, 70, 50, 255)
    buf = io.BytesIO()
    Image.fromarray(ov, "RGBA").save(buf, "PNG")
    stats["overlay_png"] = buf.getvalue()      # bridges (red) and deleted islands (blue), one pixel per grid cell
    if tiled:
        report(0.96, "Writing tiles")
        rod = (tiling.rod_name(p.connector_shape, p.connector_diameter_mm), tiling.rod_stl(p.connector_shape, p.connector_diameter_mm)) if conn else None
        stem = Path(filename).stem or "stencil"
        stats["tiles_zip"], tinfo = tiling.tiles_zip(stem, tile_parts, rod)
        stats["tiles"] = tinfo
        stats["connectors"] = conn
        stats["tile_grid"] = [len(xs) - 1, len(ys) - 1]
    report(0.97, "Writing STL")
    out = triangles_to_stl(tris)
    report(1.0, "Done")
    return out, stats
