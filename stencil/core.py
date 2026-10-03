"""Image -> graffiti stencil STL, guaranteeing a single connected piece (no islands)."""
from __future__ import annotations

import io
import struct
from dataclasses import dataclass, asdict

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


def _bridge_islands(mat: np.ndarray, bridge_px: int):
    """Connect every material component to the frame component through hole pixels.

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
        main = lab == lab[0, 0]
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
                    hits.setdefault(int(flat_lab[o]), int(s_))
            if nxt:
                nxt = np.unique(np.concatenate(nxt))
                flat_dist[nxt] = layer
            frontier = nxt if len(nxt) else np.empty(0, np.int64)
        if not hits:
            mat &= main        # unreachable (cannot happen with a frame)
            return mat, bridges, rise, blen
        before = mat.copy()
        for s0 in hits.values():
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
            f = (dk - d) / (dk - 1) if dk > 1 else np.ones_like(d)
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
            blen[r0:r1, c0:c1][upd] = dk
        mat |= ~np.isnan(rise)


def build_mask(img: Image.Image, p: Params):
    """Return (pre-bridge mask, bridged mask, rise map, stats, pixel size in mm, bridge width px)."""
    scale = p.resolution / max(img.size)
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
    pad = max(1, round(p.margin_mm / px_mm))
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
    islands = label4(mat0)[1] - 1
    matb, bridges, rise, blen = _bridge_islands(mat0, bridge_px)
    top_span = 0.0
    if p.raised_bridges and p.max_overhang_deg > 0 and not p.flip and bridges:
        lh = p.layer_height_mm if p.layer_height_mm and p.layer_height_mm > 0 else 0.2
        base = max(1, round(p.thickness_mm / lh))
        n_steps = max(1, base - 1 if p.z_bridging == "steps" else base - 2)
        d_mm = lh * np.tan(np.radians(min(p.max_overhang_deg, 89.0)))
        length = blen * px_mm
        # bridges longer than d*n_steps climb at the steepest allowed slope and finish with a flat span
        rise = np.minimum(1.0, rise * np.maximum(1.0, length / (d_mm * n_steps))).astype(np.float32)
        top_span = float(np.nanmax(np.maximum(length - d_mm * n_steps, 0))) if np.isfinite(length).any() else 0.0
    stats = {
        "islands_deleted": deleted,
        "pad_px": int(pad),
        "_deleted": deleted_mask,
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


def make_stencil(data: bytes, filename: str = "", params: Params | None = None, progress=None):
    """Return (stl_bytes, stats). `progress(fraction, stage)` is called as work advances."""
    p = params or Params()
    report = progress or (lambda f, s: None)
    report(0.02, "Reading image")
    img = load_image(data, filename)
    report(0.08, "Thresholding & bridging islands")
    mat0, matb, rise, stats, px, _ = build_mask(img, p)
    deleted_mask = stats.pop("_deleted")
    report(0.45, "Building layers")
    T = p.thickness_mm
    step = p.layer_height_mm if p.layer_height_mm and p.layer_height_mm > 0 else 0.2
    base_layers = max(1, round(T / step))
    lh = step                                   # layer height is exact; thickness snaps to whole layers
    extra = max(0, int(p.extra_top_layers))
    ramp = p.raised_bridges and p.z_bridging in ("ramp", "stepramp")
    if p.raised_bridges and not ramp:
        masks = layer_masks(mat0, matb, rise, base_layers)
    else:
        masks = [_fix_diagonals(matb)] * base_layers
    n_layers = base_layers + extra
    T = n_layers * lh
    sigma = p.smooth_mm / px
    factors = (1.0, 0.5, 0.25, 0.0) if sigma > 0 else (0.0,)
    solid = None
    for a, factor in enumerate(factors):
        lo, span = 0.5 + 0.4 * a / len(factors), 0.4 / len(factors)
        tick = lambda f, lo=lo, span=span: report(lo + span * f, "Extruding & merging layers")
        if ramp:
            solid = build_solid_ramp(layer_masks(mat0, matb, rise, max(base_layers, 2))[0], _fix_diagonals(matb), rise, px, base_layers, extra,
                                     sigma * factor, tick, p.z_bridging == "stepramp").scale((1.0, 1.0, lh))
        else:
            solid = build_solid(masks + [masks[-1]] * extra, px, lh, sigma * factor, tick)
        if len(solid.decompose()) == 1:       # smoothing must never split the part
            break
    trace = trace_svg(solid.slice(lh / 2), mat0.shape[1] * px, mat0.shape[0] * px)
    if p.flip:
        # turn the part over (180 deg about the x axis, not a mirror): the printed object is the real stencil seen from behind
        solid = solid.rotate((0.0, 180.0, 0.0)).translate((float(mat0.shape[1] * px), 0.0, float(T)))
    report(0.92, "Checking mesh")
    pieces = len(solid.decompose())
    mesh = solid.to_mesh()
    tris = mesh.vert_properties[:, :3][mesh.tri_verts].astype(np.float32)
    stats.update(
        overhang_area_mm2=round(overhang_area(solid, lh, n_layers, p.max_overhang_deg or 45.0), 2),
        flipped=bool(p.flip),
        islands_remaining=int(pieces - 1),
        triangles=int(len(tris)),
        watertight=bool(solid.status().name == "NoError" and check_watertight(mesh.tri_verts)),
        layers=int(n_layers),
        extra_top_layers=extra,
        z_bridging=p.z_bridging if p.raised_bridges else None,
        layer_height_mm=round(lh, 4),
        smoothing_mm=round(float(sigma * factor * px), 3),
        size_mm=[round(mat0.shape[1] * px, 2), round(mat0.shape[0] * px, 2), T],
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
    report(0.97, "Writing STL")
    out = triangles_to_stl(tris)
    report(1.0, "Done")
    return out, stats
