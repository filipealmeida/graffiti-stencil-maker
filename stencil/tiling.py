"""Split a finished stencil into printable tiles joined by glued rods in blind seam holes."""
from __future__ import annotations

import io
import math
import zipfile
from pathlib import Path

import numpy as np

DIAMETERS = (2.5, 3.0, 4.0, 5.0, 6.0)     # connector diameter in mm (hexagon: across the corners)
SHAPES = ("hex", "round")
MIN_HEIGHT_MM = 4.0                       # connectors need a plate thicker than this
WALL_MM = 1.0                             # material kept around every hole (also above and below)
THIN_WALL_MM = 0.6                        # last resort where a seam has little material
ROW_GAP_MM = 1.5                          # material between two stacked holes
MAX_ROWS = 4
MAX_SPACING_MM = 40.0                     # connectors along a seam are never planned further apart than this
SPACING_TARGET_MM = 30.0                  # aimed-for spacing (leaves room to move a blocked hole and stay under 40 mm)
EXPLODE_GAP_MM = 10.0                     # tile spacing in the STL preview
POCKET_DEPTH_MM = 10.0                    # hole depth in each of the two tiles
ROD_LENGTH_MM = 2 * POCKET_DEPTH_MM - 1.0   # leaves 0.5 mm per side for glue
MIN_TILE_MM = POCKET_DEPTH_MM / 0.4       # holes may use at most 40% of a tile's width


def z_extent(shape: str, d: float) -> float:
    """Height of a connector lying in a seam hole (hexagon flats face up and down)."""
    return math.sqrt(3) / 2 * d if shape == "hex" else d


def max_diameter(shape: str, height: float) -> float:
    return (height - 2 * WALL_MM) / (math.sqrt(3) / 2 if shape == "hex" else 1)


def check_connector(shape: str, d: float, height: float) -> None:
    if shape not in SHAPES:
        raise ValueError(f"unknown connector shape {shape!r}; use one of {', '.join(SHAPES)}")
    if d not in DIAMETERS:
        raise ValueError(f"connector diameter must be one of {', '.join(f'{x:g}' for x in DIAMETERS)} mm")
    if height <= MIN_HEIGHT_MM:
        raise ValueError(f"connectors need a plate thicker than {MIN_HEIGHT_MM:g} mm (it is {height:g} mm)")
    if z_extent(shape, d) + 2 * WALL_MM > height + 1e-9:
        raise ValueError(f"a {shape} connector of diameter {d:g} mm needs a plate at least {z_extent(shape, d) + 2 * WALL_MM:.1f} mm thick "
                         f"(it is {height:g} mm; the largest diameter that fits is {max(0.0, max_diameter(shape, height)):.1f} mm)")


def grid(W: float, H: float, max_x: float, max_y: float):
    """Seam coordinates of equal tiles no larger than max_x * max_y (0 = unlimited)."""
    nx = max(1, math.ceil(W / max_x - 1e-9)) if max_x > 0 else 1
    ny = max(1, math.ceil(H / max_y - 1e-9)) if max_y > 0 else 1
    return [W * i / nx for i in range(nx + 1)], [H * j / ny for j in range(ny + 1)]


def avoid_seams(centers, r: float, W: float, xs, ys, clear: float = 1.0):
    """Slide pads along the frame so no pad is cut by a seam; pads always stay inside one tile."""
    sx, sy = xs[1:-1], ys[1:-1]
    out = []
    for x, y in centers:
        if any(abs(y - s) < r + clear for s in sy):
            raise ValueError("a horizontal seam would cut through a mounting pad: use a taller tile size or fewer/smaller pads")
        for _ in range(len(sx) + 1):
            hit = next((s for s in sx if abs(x - s) < r + clear), None)
            if hit is None:
                break
            opts = [c for c in (hit - r - clear, hit + r + clear) if r <= c <= W - r]
            if not opts:
                raise ValueError("a vertical seam would cut through a mounting pad: use a wider tile size or fewer/smaller pads")
            x = min(opts, key=lambda c: abs(c - x))
        else:
            raise ValueError("mounting pads cannot be kept inside a single tile: use a wider tile size or fewer/smaller pads")
        out.append((x, y))
    return out


def _profile(shape: str, r: float) -> np.ndarray:
    n = 6 if shape == "hex" else 64
    a = 2 * np.pi * np.arange(n) / n
    return np.column_stack([r * np.cos(a), r * np.sin(a)])


def prism(shape: str, r: float, length: float, axis: str | None = None):
    """Rod centred on the origin. axis None: along z; 'x' / 'y': lying along that axis, hexagon flats up and down."""
    import manifold3d as m3d
    cs = m3d.CrossSection([_profile(shape, r)], m3d.FillRule.NonZero)
    m = m3d.Manifold.extrude(cs, float(length)).translate((0.0, 0.0, -length / 2))
    if axis == "x":
        m = m.transform(np.array([[0, 0, 1, 0], [1, 0, 0, 0], [0, 1, 0, 0]], float))
    elif axis == "y":
        m = m.transform(np.array([[-1, 0, 0, 0], [0, 0, 1, 0], [0, 1, 0, 0]], float))
    return m


def row_heights(shape: str, d: float, T: float) -> list[float]:
    ext = z_extent(shape, d)
    n = int(min(MAX_ROWS, max(1, (T - 2 * WALL_MM + ROW_GAP_MM) // (ext + ROW_GAP_MM))))
    if n == 1:
        return [T / 2]
    lo, hi = WALL_MM + ext / 2, T - WALL_MM - ext / 2
    return [lo + i * (hi - lo) / (n - 1) for i in range(n)]


def optimize_grid(mat, px: float, xs, ys, max_x: float, max_y: float, shift: float = 8.0, step: float = 2.0):
    """Nudge the seams (within the tile size limit) to where the plate has most material around them, so holes find room.
    `mat` is the material bitmap (rows run top-down, `px` mm per pixel)."""
    hh, ww = mat.shape
    H = hh * px
    half = POCKET_DEPTH_MM / px

    def score(axis, s):
        if axis == "x":
            c = s / px
            return int(mat[:, max(0, int(c - half)):max(0, int(c + half))].sum())
        r = (H - s) / px
        return int(mat[max(0, int(r - half)):max(0, int(r + half)), :].sum())

    def tune(coords, mx, axis):
        n = len(coords) - 1
        if n < 2 or mx <= 0:
            return list(coords)
        total = coords[-1]
        out = list(coords)
        for i in range(1, n):
            best = None
            for k in range(-int(shift / step), int(shift / step) + 1):
                s = coords[i] + k * step
                w = s - out[i - 1]
                if w > mx + 1e-9 or w < MIN_TILE_MM or total - s > (n - i) * mx + 1e-9 or total - s < (n - i) * MIN_TILE_MM:
                    continue
                key = (score(axis, s), -abs(k))
                if best is None or key > best[0]:
                    best = (key, s)
            if best:
                out[i] = best[1]
        ok = all(MIN_TILE_MM - 1e-9 <= b - a <= mx + 1e-9 for a, b in zip(out, out[1:]))
        return out if ok else list(coords)

    return tune(xs, max_x, "x"), tune(ys, max_y, "y")


def drill_connectors(solid, shape: str, d: float, clearance: float, T: float, xs, ys, margin: float):
    """Subtract blind holes straddling every seam. Returns (solid, info)."""
    import manifold3d as m3d
    check_connector(shape, d, T)
    nx, ny = len(xs) - 1, len(ys) - 1
    W, H = xs[-1], ys[-1]
    if any(b - a < MIN_TILE_MM - 1e-6 for c in (xs, ys) if len(c) > 2 for a, b in zip(c, c[1:])):
        raise ValueError(f"tiles must be at least {MIN_TILE_MM:g} mm wide to hold connector holes")
    zs = row_heights(shape, d + clearance, T)
    r_hole = (d + clearance) / 2
    wall_r, sep_r = r_hole + WALL_MM, r_hole + ROW_GAP_MM / 2
    seams = [("x", s, ys[j], ys[j + 1]) for s in xs[1:-1] for j in range(ny)]
    seams += [("y", s, xs[i], xs[i + 1]) for s in ys[1:-1] for i in range(nx)]

    def at(axis, s, u, z):
        return (s, u, z) if axis == "x" else (u, s, z)

    def spot(axis, s, u, z, rad):
        return prism(shape, rad, 2 * POCKET_DEPTH_MM, axis).translate(at(axis, s, u, z))

    holes, guards, missed, worst_gap, fewest, thin, margin_holes = [], [], 0, 0.0, None, 0, 0
    margin_done = set()

    def add_hole(axis, s, u, z, wall):
        nonlocal thin
        guard = spot(axis, s, u, z, r_hole + wall)
        if (guard ^ solid).volume() < guard.volume() * (1 - 1e-4):
            return False
        near = spot(axis, s, u, z, sep_r)
        if any((near ^ g).volume() > 1e-6 for g in guards):
            return False
        guards.append(near)
        holes.append(spot(axis, s, u, z, r_hole))
        thin += wall < WALL_MM
        return True

    for axis, s, a, b in seams:
        n = max(1, math.ceil((b - a) / SPACING_TARGET_MM - 1e-9))
        cell = (b - a) / n
        for z in zs:
            placed = []
            for i in range(n):
                base = a + (i + 0.5) * cell
                done = False
                for reach, wall in ((cell / 2, WALL_MM), (cell, WALL_MM), (cell, THIN_WALL_MM)):
                    wall_r = r_hole + wall
                    lo, hi = a + wall_r, b - wall_r          # stay clear of the crossing seam
                    for k in range(int(reach) + 1):
                        for u in ([base] if k == 0 else [base - k, base + k]):
                            if not lo <= u <= hi:
                                continue
                            if any(abs(u - q) < 2 * sep_r for q in placed):
                                continue
                            if not add_hole(axis, s, u, z, wall):
                                continue
                            placed.append(u)
                            on_frame = u <= margin or u >= (H if axis == "x" else W) - margin
                            seam_key = (axis, s)
                            if on_frame and seam_key not in margin_done:
                                margin_done.add(seam_key)
                                margin_holes += 1
                            done = True
                            break
                        if done:
                            break
                    if done:
                        break
                if not done:
                    missed += 1
            ps = sorted(placed)
            gaps = [2 * (ps[0] - a)] + [q - p for p, q in zip(ps, ps[1:])] + [2 * (b - ps[-1])] if ps else [b - a]
            worst_gap = max(worst_gap, max(gaps))
            fewest = len(ps) if fewest is None else min(fewest, len(ps))

    # Reserve a connector in the outside frame for every continuous internal seam.
    seam_lines = {}
    for axis, s, a, b in seams:
        seam_lines.setdefault((axis, s), []).append((a, b))
    for (axis, s), segments in seam_lines.items():
        seam_key = (axis, s)
        if seam_key in margin_done:
            continue
        z = zs[len(zs) // 2]
        placed_margin = False
        has_frame_segment = False
        for a, b in segments:
            if axis == "x":
                bands = [(0.0, margin)] if a <= 1e-6 else []
                if b >= H - 1e-6:
                    bands.append((H - margin, H))
            else:
                bands = [(0.0, margin)] if a <= 1e-6 else []
                if b >= W - 1e-6:
                    bands.append((W - margin, W))
            for band_lo, band_hi in bands:
                has_frame_segment = True
                for wall in (WALL_MM, THIN_WALL_MM):
                    edge = r_hole + wall
                    lo, hi = max(a + edge, band_lo + edge), min(b - edge, band_hi - edge)
                    if lo > hi:
                        continue
                    candidates = np.arange(lo, hi + 1e-6, 1.0).tolist()
                    candidates.append((lo + hi) / 2)
                    candidates.sort(key=lambda q: abs(q - (band_lo + band_hi) / 2))
                    if any(add_hole(axis, s, u, z, wall) for u in candidates):
                        placed_margin = True
                        break
                if placed_margin:
                    break
            if placed_margin:
                break
        if placed_margin:
            margin_done.add(seam_key)
            margin_holes += 1
        elif has_frame_segment:
            raise ValueError(f"could not place a connector hole in the frame margin on the {axis}-axis seam at {s:.1f} mm; increase the frame margin or use a smaller connector diameter")

    if holes:
        solid = solid - m3d.Manifold.batch_boolean(holes, m3d.OpType.Add)
    info = {"shape": shape, "diameter_mm": d, "clearance_mm": clearance, "rows": len(zs), "count": len(holes),
            "positions_without_room": missed, "thin_wall_holes": thin, "max_gap_mm": round(worst_gap, 1), "fewest_per_seam_row": fewest or 0,
            "margin_holes": margin_holes,
            "hole_depth_mm": POCKET_DEPTH_MM, "rod_length_mm": ROD_LENGTH_MM}
    return solid, info


def exploded(tiles, xs, ys, gap: float = EXPLODE_GAP_MM):
    """All tiles in one manifold, moved apart so the seam faces and their holes can be seen."""
    import manifold3d as m3d
    ny = len(ys) - 1
    parts = [t.translate((xs[c - 1] + (c - 1) * gap, ys[ny - r] + (ny - r) * gap, 0.0)) for _, t, (r, c) in tiles]
    return m3d.Manifold.batch_boolean(parts, m3d.OpType.Add)


def split_tiles(solid, xs, ys):
    """Cut the solid on the seam grid. Returns [(name, tile, (row, col))], row 1 = top, col 1 = left, each tile moved to the origin."""
    import manifold3d as m3d
    bb = solid.bounding_box()
    z0, z1 = bb[2] - 1.0, bb[5] + 1.0
    nx, ny = len(xs) - 1, len(ys) - 1
    tiles = []
    for j in range(ny - 1, -1, -1):
        for i in range(nx):
            xa = xs[i] if i > 0 else xs[i] - 1.0
            xb = xs[i + 1] if i < nx - 1 else xs[i + 1] + 1.0
            ya = ys[j] if j > 0 else ys[j] - 1.0
            yb = ys[j + 1] if j < ny - 1 else ys[j + 1] + 1.0
            box = m3d.Manifold.cube((xb - xa, yb - ya, z1 - z0)).translate((xa, ya, z0))
            t = (solid ^ box).translate((-xs[i], -ys[j], 0.0))
            if t.num_vert() == 0:
                continue
            r, c = ny - j, i + 1
            tiles.append((f"r{r}c{c}", t, (r, c)))
    return tiles


def stl_bytes(m):
    """(binary STL, triangle count, watertight) of a Manifold."""
    from .core import check_watertight, triangles_to_stl
    mesh = m.to_mesh()
    tris = mesh.vert_properties[:, :3][mesh.tri_verts].astype(np.float32)
    return triangles_to_stl(tris), int(len(tris)), bool(m.status().name == "NoError" and check_watertight(mesh.tri_verts))


def rod_name(shape: str, d: float) -> str:
    return f"{shape}_d{d:g}mm.stl"


def rod_stl(shape: str, d: float) -> bytes:
    """Connector rod standing on the bed, axis along z."""
    return stl_bytes(prism(shape, d / 2, ROD_LENGTH_MM).translate((0.0, 0.0, ROD_LENGTH_MM / 2)))[0]


def write_connector_stls(folder) -> list[Path]:
    folder = Path(folder)
    folder.mkdir(parents=True, exist_ok=True)
    out = []
    for shape in SHAPES:
        for d in DIAMETERS:
            f = folder / rod_name(shape, d)
            f.write_bytes(rod_stl(shape, d))
            out.append(f)
    return out


def tiles_zip(stem: str, tiles, rod: tuple[str, bytes] | None):
    """Returns (zip bytes, per-tile stats)."""
    buf = io.BytesIO()
    info = []
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        for name, t, (r, c) in tiles:
            data, n, ok = stl_bytes(t)
            z.writestr(f"{stem}_{name}.stl", data)
            bb = t.bounding_box()
            info.append({"name": f"{stem}_{name}.stl", "row": r, "col": c, "triangles": n, "watertight": ok,
                         "pieces": len(t.decompose()), "size_mm": [round(bb[3] - bb[0], 2), round(bb[4] - bb[1], 2), round(bb[5] - bb[2], 2)]})
        if rod:
            z.writestr(f"connector_{rod[0]}", rod[1])
    return buf.getvalue(), info


# ---- mounting holes and the plates that use them -----------------------------------------------

SCREW_HOLES = {"M3": 3.0, "M4": 4.0, "M5": 5.0}     # nominal screw diameter in mm
JOIN_WALL_MM = 3.0                                  # material around a hole in a joiner plate


def hole_diameter(screw: str, clearance: float) -> float:
    return SCREW_HOLES[screw] + clearance


def mount_positions(W: float, H: float, margin: float, d: float, off: float, xs, ys, avoid=(), wall: float = 1.5):
    """Through-hole centres on the middle of the frame band (plate mm, y up).

    Tiles that own a piece of the frame get a hole in each corner they hold, a hole on each side of every seam that crosses
    the frame (these pairs are what the joiner plates bridge) and one in the middle of each long frame segment.
    Tiles without frame get none. Returns (centres, seam pairs [(a, b, axis)])."""
    if margin < d + 2 * wall - 1e-6:
        return [], []
    c = margin / 2
    pts: list[tuple[float, float]] = []

    def free(p, gap):
        return all(math.hypot(p[0] - q[0], p[1] - q[1]) >= gap for q in pts) and all(
            math.hypot(p[0] - a[0], p[1] - a[1]) >= a[2] + d / 2 + wall for a in avoid)

    sides = [("x", c, xs, W), ("x", H - c, xs, W), ("y", c, ys, H), ("y", W - c, ys, H)]
    for axis, fixed, seams, L in sides:
        for t in (c, L - c):
            p = (t, fixed) if axis == "x" else (fixed, t)
            if free(p, 2 * d):
                pts.append(p)
    pairs = []
    for axis, fixed, seams, L in sides:
        for s in seams[1:-1]:
            ps = [(s - off, fixed) if axis == "x" else (fixed, s - off), (s + off, fixed) if axis == "x" else (fixed, s + off)]
            if all(free(q, 1.5 * d) for q in ps) and 0 < s - off and s + off < L:
                pts.extend(ps)
                pairs.append((ps[0], ps[1], axis))
    for axis, fixed, seams, L in sides:
        for a, b in zip(seams, seams[1:]):
            t = (a + b) / 2
            p = (t, fixed) if axis == "x" else (fixed, t)
            if b - a > 4 * off and free(p, 2 * d):
                pts.append(p)
    return pts, pairs


def _slot(pts, r):
    import manifold3d as m3d
    circles = [m3d.CrossSection.circle(r, 48).translate(p) for p in pts]
    return m3d.CrossSection.batch_hull(circles)


def join_plates(d: float, off: float, ext: float, thickness: float):
    """(joiner, extender) as Manifolds lying flat on the bed.

    The joiner has two holes 2*off apart, one per tile across a seam. The extender adds a second row of two holes `ext`
    outward, to screw to extra margin material or to a wall."""
    import manifold3d as m3d
    r = d / 2 + JOIN_WALL_MM
    row = [(-off, 0.0), (off, 0.0)]
    out = []
    for pts in (row, row + [(-off, ext), (off, ext)]):
        body = m3d.Manifold.extrude(_slot(pts, r), thickness)
        holes = m3d.Manifold.batch_boolean([m3d.Manifold.cylinder(thickness + 2, d / 2, d / 2, 48).translate((x, y, -1.0)) for x, y in pts], m3d.OpType.Add)
        out.append(body - holes)
    return out[0], out[1]
