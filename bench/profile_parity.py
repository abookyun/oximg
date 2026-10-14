#!/usr/bin/env python3
"""Byte-compare two builds of oximg over the fixture matrix.

The `agent` cargo profile claims release's output byte for byte; this is
the instrument behind that claim. Each build's oximg-ctl drives its own
oximg through `matrix --no-negatives` for every image fixture and box,
and the response bodies' sha256 must agree cell for cell.

    bench/profile_parity.py target/release target/agent

Prints one JSON object; exits 1 if any cell differs or none was compared.
Run from the repo root. Python 3.9 compatible (stock macOS), stdlib only.
"""

import json
import os
import subprocess
import sys

BOXES = ["100x100", "333x0", "64x64"]
NOT_IMAGES = (".md", ".sh", ".txt", ".ppm")


def fixtures():
    return sorted(
        f for f in os.listdir("tests/fixtures") if not f.endswith(NOT_IMAGES)
    )


def cells(build_dir):
    ctl = os.path.join(build_dir, "oximg-ctl")
    # Name the server explicitly: ctl's sibling lookup follows symlinks,
    # so it alone could drive some other build's oximg.
    bin_ = os.path.join(build_dir, "oximg")
    out = {}
    for f in fixtures():
        for box in BOXES:
            res = subprocess.run(
                [ctl, "--bin", bin_, "matrix", "--source", f, "--box", box,
                 "--no-negatives"],
                capture_output=True,
                text=True,
            )
            for c in json.loads(res.stdout)["cells"]:
                out[c["path"]] = (c["status"], c.get("sha256"))
    return out


def main():
    if len(sys.argv) != 3:
        sys.exit("usage: bench/profile_parity.py BUILD_DIR_A BUILD_DIR_B")
    a, b = cells(sys.argv[1]), cells(sys.argv[2])
    differ = sorted(k for k in a.keys() | b.keys() if a.get(k) != b.get(k))
    print(
        json.dumps(
            {
                "a": sys.argv[1],
                "b": sys.argv[2],
                "fixtures": len(fixtures()),
                "cells": len(a),
                "status_200": sum(1 for s, _ in a.values() if s == 200),
                "differ": len(differ),
                "differing_paths": differ[:20],
            }
        )
    )
    sys.exit(1 if differ or not a else 0)


if __name__ == "__main__":
    main()
