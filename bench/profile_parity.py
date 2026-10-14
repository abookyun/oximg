#!/usr/bin/env python3
"""Byte-compare two builds of oximg over the fixture matrix.

The `agent` cargo profile claims release's output byte for byte; this is
the instrument behind that claim. Each build's oximg-ctl drives its own
oximg through `matrix --no-negatives` for every image fixture and box,
and the response bodies' sha256 must agree cell for cell. Only cells
both builds answer with 200 count as compared; the same error on both
sides is reported as skipped.

    bench/profile_parity.py target/release target/agent

Prints one JSON object; exits 1 if any cell differs, none was compared,
or ctl failed, and 2 on a usage error.
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
            try:
                res = subprocess.run(
                    [ctl, "--bin", bin_, "matrix", "--source", f, "--box",
                     box, "--no-negatives"],
                    capture_output=True,
                    text=True,
                )
            except OSError as e:
                res = subprocess.CompletedProcess(ctl, 127, "", str(e))
            try:
                report = json.loads(res.stdout)
            except ValueError:
                report = {}
            if "cells" not in report:
                # ctl failed before any cell ran: missing binary, spawn
                # failure. Say which build and why, as the one JSON object.
                fail(build_dir, f, box, res, report)
            for c in report["cells"]:
                # A transport failure has no status; keep its error so it
                # can only match the same failure on the other side.
                out[c["path"]] = (c.get("status", c.get("error")), c.get("sha256"))
    return out


def fail(build_dir, source, box, res, report):
    print(
        json.dumps(
            {
                "ok": False,
                "build": build_dir,
                "source": source,
                "box": box,
                "ctl_exit": res.returncode,
                "ctl_report": report or None,
                "ctl_stderr": res.stderr[-2000:],
            }
        )
    )
    sys.exit(1)


def main():
    if len(sys.argv) != 3:
        # Exit 2 for usage, as oximg-ctl does; still one JSON object.
        print(
            json.dumps(
                {
                    "ok": False,
                    "hint": "usage: bench/profile_parity.py BUILD_DIR_A BUILD_DIR_B",
                }
            )
        )
        sys.exit(2)
    a, b = cells(sys.argv[1]), cells(sys.argv[2])
    # Only a 200 on both sides compares encoded output. The same error on
    # both (an AVIF fixture on a build without the feature answers 422)
    # agrees without proving anything, so it is skipped, not counted.
    compared, skipped, differ = [], [], []
    for k in sorted(a.keys() | b.keys()):
        sa, sb = a.get(k), b.get(k)
        if sa != sb:
            differ.append(k)
        elif sa[0] == 200:
            compared.append(k)
        else:
            skipped.append(k)
    print(
        json.dumps(
            {
                "a": sys.argv[1],
                "b": sys.argv[2],
                "fixtures": len(fixtures()),
                "cells": len(a.keys() | b.keys()),
                "compared": len(compared),
                "skipped_same_error": len(skipped),
                "differ": len(differ),
                "differing_paths": differ[:20],
                "skipped_paths": skipped[:20],
            }
        )
    )
    sys.exit(1 if differ or not compared else 0)


if __name__ == "__main__":
    main()
