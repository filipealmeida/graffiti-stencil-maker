# Graffiti Stencil Maker

Converts png/jpg/bmp/svg into a printable stencil STL. Dark areas (or light, with invert) become holes.
Material pieces that would float free ("islands", e.g. the centre of an O) are detected and tied to the
frame with narrow bridges, so the stencil is always one connected, watertight piece.

    python3 -m venv .venv && .venv/bin/pip install -r requirements.txt
    .venv/bin/python -m stencil input.png -o out.stl [--width 100 --thickness 2 --bridge 1.6 --invert --threshold 128 --smooth 0.5 --raised-bridges]
    .venv/bin/python -m stencil --serve      # web UI on http://127.0.0.1:5000 (drag & drop, 3D preview)

CLI exits non-zero if islands remain or the mesh isn't watertight.

## License

[The Unlicense](LICENSE) (public domain), except `stencil/static/vendor/`, which is three.js under the MIT license (see `stencil/static/vendor/LICENSE`).
