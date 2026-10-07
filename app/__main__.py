import argparse

import uvicorn

ap = argparse.ArgumentParser(description="Stencil studio (node graph UI)")
ap.add_argument("--host", default="127.0.0.1")
ap.add_argument("--port", type=int, default=5056)
a = ap.parse_args()
uvicorn.run("app.server:app", host=a.host, port=a.port)
