"""Regenerate the connector rod STLs in toolbox/stls/connectors (run from the repo root: python toolbox/build_connectors.py)."""
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from stencil.tiling import write_connector_stls  # noqa: E402

if __name__ == "__main__":
    for f in write_connector_stls(ROOT / "toolbox" / "stls" / "connectors"):
        print(f.relative_to(ROOT))
