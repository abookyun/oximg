#!/usr/bin/env python3
"""Quality as a function of the JPEG DCT decode scale.

`OXIMG_DCT_MARGIN` decides how much of a downscale libjpeg's scaled
decode does (numerator k of k/8) and how much is left to the
resampler. This measures every reachable k at several ratios, against
a ground truth that no decoder has touched:

- the truth is a lossless PNG (e.g. DIV2K HR);
- the served source is that PNG encoded as a JPEG (`--src-quality`,
  4:2:0), the shape real traffic arrives in;
- oximg decodes at k/8, resizes to the target and encodes at
  `--quality`;
- the reference is the truth PNG resized to the output's exact size
  with Lanczos, in linear light (primary) and in sRGB (secondary).
  The two disagree about some decode choices, so both are reported.

Scores are paired per image against the full decode (k=8): mean
difference, worst image, and how many images lose more than 2 points.

Two things an earlier version of this script got wrong, and why the
numbers it published moved:

- It chose the margin from the width alone. `dct_scale_num` needs both
  dimensions, so when the target height rounded up, a cell labelled k
  decoded at k+1 (2000x1334 -> 500: "k=4" was a 5/8 decode). The
  margin now comes from both dimensions, and every run is checked: the
  decoded size printed under OXIMG_TIMING must be the one k predicts.
- Its reference was a resize of the served JPEG itself, which bakes
  one decoder's choices into the yardstick.

  python3 bench/quality/dct_sweep.py TRUTH_DIR [--ratios 2,3,4,5.3,6,8,14]
      [--quality 80] [--src-quality 92] [--bin target/release/oximg]
      [--jobs N] [--out bench/quality/dct-sweep.json]

Needs `magick` and `ssimulacra2` on PATH.
"""

import argparse
import hashlib
import json
import math
import os
import pathlib
import re
import statistics
import subprocess
import sys
from concurrent.futures import ProcessPoolExecutor

ROOT = pathlib.Path(__file__).resolve().parents[2]


def sh(*cmd, env=None):
    r = subprocess.run([str(c) for c in cmd], capture_output=True, text=True, env=env)
    if r.returncode != 0:
        raise RuntimeError(f"{cmd[0]} failed: {r.stderr.strip()[:300]}")
    return r


def dims(path):
    return tuple(int(v) for v in sh("magick", "identify", "-format", "%w %h", path).stdout.split())


def fit_dims(src_w, src_h, max_w):
    """src/pipeline/mod.rs `fit_dims` for `oximg resize <in> max_w 0`
    (a 0 bound is u32::MAX). Rust rounds half away from zero."""
    scale = min(max_w / src_w, min(4294967295 / src_h, 1.0))
    return (max(1, math.floor(src_w * scale + 0.5)),
            max(1, math.floor(src_h * scale + 0.5)))


def scale_num(src_w, src_h, dst_w, dst_h, margin):
    """src/pipeline/mod.rs `dct_scale_num`, kept in step by the
    decoded-size check in `run_cell`."""
    if margin is None:
        return 8
    need_w, need_h = math.ceil(dst_w * margin), math.ceil(dst_h * margin)
    for k in range(1, 9):
        sw, sh_ = -(-src_w * k // 8), -(-src_h * k // 8)
        if (sw >= need_w and sh_ >= need_h) or (sw >= src_w and sh_ >= src_h):
            return k
    return 8


UNREACHABLE = "unreachable"


def margin_for(src_w, src_h, dst_w, dst_h, k):
    """The OXIMG_DCT_MARGIN that makes dct_scale_num pick exactly k:
    None for k=8 (the default full decode needs no margin), UNREACHABLE
    when k's decode would be smaller than the target in either
    dimension or the margin falls outside the knob's 1.0-8.0 range."""
    if k == 8:
        return None
    sw, sh_ = -(-src_w * k // 8), -(-src_h * k // 8)
    # The largest margin k still satisfies in both dimensions: k-1 is
    # smaller in the binding dimension, so it fails and k is the pick.
    m = min(sw / dst_w, sh_ / dst_h)
    if not 1.0 <= m <= 8.0:
        return UNREACHABLE
    m = math.floor(m * 1e6) / 1e6  # what the env string carries
    return m if scale_num(src_w, src_h, dst_w, dst_h, m) == k else UNREACHABLE


TIMING = re.compile(r"^timing [a-z+-]+\((\d+)x(\d+)", re.M)


def run_cell(job):
    """One image x target x k: encode, check the decode size, score."""
    a = job
    env = {k: v for k, v in os.environ.items() if not k.startswith("OXIMG_")}
    env["OXIMG_TIMING"] = "1"
    if a["margin"] is not None:
        env["OXIMG_DCT_MARGIN"] = f"{a['margin']:.6f}"
    out = pathlib.Path(a["out"])
    r = sh(a["bin"], "resize", a["src"], a["target"], 0, out, "-q", a["quality"], env=env)
    m = TIMING.search(r.stderr)
    if not m:
        raise RuntimeError(f"no timing line from {a['src']}: {r.stderr[:200]}")
    decoded = (int(m[1]), int(m[2]))
    if decoded != a["expect_decoded"]:
        raise RuntimeError(
            f"{a['src']} k={a['k']}: decoded {decoded}, expected {a['expect_decoded']}"
        )
    w, h = dims(out)
    scores = {}
    for kind, args in (("lin", ["-colorspace", "RGB"]), ("srgb", [])):
        ref = pathlib.Path(a["refdir"]) / f"{a['tag']}-{w}x{h}-{kind}.png"
        if not ref.exists():
            tmp = ref.with_suffix(f".{os.getpid()}.png")
            back = ["-colorspace", "sRGB"] if kind == "lin" else []
            sh("magick", a["truth"], *args, "-filter", "Lanczos", "-resize", f"{w}x{h}!", *back, tmp)
            os.replace(tmp, ref)
        scores[kind] = float(sh("ssimulacra2", ref, out).stdout.strip())
    size = out.stat().st_size
    out.unlink()
    return {**{k: a[k] for k in ("stem", "ratio", "k")}, **scores, "bytes": size}


def make_source(a):
    truth, served, q = a
    if not pathlib.Path(served).exists():
        sh("magick", truth, "-quality", q, "-sampling-factor", "2x2", served)
    return served


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("truth", help="directory of lossless PNG ground truths")
    ap.add_argument("--ratios", default="2,3,4,5.3,6,8,14")
    ap.add_argument("--quality", type=int, default=80)
    ap.add_argument("--src-quality", type=int, default=92)
    ap.add_argument("--bin", default=str(ROOT / "target/release/oximg"))
    ap.add_argument("--jobs", type=int, default=os.cpu_count())
    ap.add_argument("--work", default="/tmp/dct-sweep")
    ap.add_argument("--out", default=str(ROOT / "bench/quality/dct-sweep.json"))
    args = ap.parse_args()

    if not pathlib.Path(args.bin).is_file():
        sys.exit(f"{args.bin} not found: cargo build --release")
    truths = sorted(pathlib.Path(args.truth).glob("*.png"))
    if not truths:
        sys.exit(f"no PNG ground truths in {args.truth}")
    work = pathlib.Path(args.work)
    for sub in ("src", "out", "ref"):
        (work / sub).mkdir(parents=True, exist_ok=True)
    ratios = [float(r) for r in args.ratios.split(",")]

    # Cached sources and references are keyed by the truth's content,
    # not its name: a rerun over changed truths, or another corpus with
    # the same file names, must not reuse the previous run's pixels.
    tags = [hashlib.sha256(t.read_bytes()).hexdigest()[:16] for t in truths]
    with ProcessPoolExecutor(args.jobs) as pool:
        served = list(pool.map(make_source, [
            (t, work / "src" / f"{tag}-q{args.src_quality}.jpg", args.src_quality)
            for t, tag in zip(truths, tags)
        ]))
        jobs, skipped = [], {}
        for truth, tag, src in zip(truths, tags, served):
            src_w, src_h = dims(src)
            for ratio in ratios:
                target = round(src_w / ratio)
                dst_w, dst_h = fit_dims(src_w, src_h, target)
                for k in range(1, 9):
                    m = margin_for(src_w, src_h, dst_w, dst_h, k)
                    if m == UNREACHABLE:
                        skipped[(ratio, k)] = skipped.get((ratio, k), 0) + 1
                        continue
                    jobs.append({
                        "bin": args.bin, "src": str(src), "truth": str(truth),
                        "stem": truth.stem, "tag": tag, "ratio": ratio, "k": k, "margin": m,
                        "target": target, "quality": args.quality,
                        "expect_decoded": (-(-src_w * k // 8), -(-src_h * k // 8)),
                        "out": str(work / "out" / f"{tag}-{ratio}-{k}.jpg"),
                        "refdir": str(work / "ref"),
                    })
        print(f"{len(jobs)} cells over {len(truths)} images", file=sys.stderr)
        results = list(pool.map(run_cell, jobs, chunksize=4))

    by = {}
    for r in results:
        by.setdefault((r["ratio"], r["k"]), {})[r["stem"]] = r
    summary = []
    print(f"\nSSIMULACRA2 by DCT numerator; served q{args.src_quality} 4:2:0, "
          f"output q{args.quality}; paired against the full decode (k=8)\n")
    for ratio in ratios:
        full = by.get((ratio, 8), {})
        print(f"## {ratio:g}x  (n={len(full)})")
        print("   k   lin mean  d mean  worst  <-2   srgb mean  d mean  worst  <-2      KB")
        for k in range(1, 9):
            cell = by.get((ratio, k))
            if not cell:
                if (ratio, k) in skipped:
                    print(f"   {k}   unreachable for {skipped[(ratio, k)]} images")
                continue
            row = {"ratio": ratio, "k": k, "n": len(cell)}
            line = f"   {k}"
            for kind in ("lin", "srgb"):
                d = [cell[s][kind] - full[s][kind] for s in cell if s in full]
                mean = statistics.mean(c[kind] for c in cell.values())
                row[kind] = {"mean": mean, "delta_mean": statistics.mean(d),
                             "worst": min(d), "below_minus_2": sum(x < -2 for x in d)}
                line += (f"   {mean:7.2f}  {row[kind]['delta_mean']:+6.2f}  "
                         f"{row[kind]['worst']:+6.2f}  {row[kind]['below_minus_2']:3d}")
            row["kb"] = statistics.mean(c["bytes"] for c in cell.values()) / 1024
            print(f"{line}   {row['kb']:7.1f}")
            summary.append(row)
        print()

    def rounded(x):
        if isinstance(x, float):
            return round(x, 3)
        if isinstance(x, dict):
            return {k: rounded(v) for k, v in x.items()}
        return x

    # The summary is what gets committed; per-image scores (for pairing
    # a later run against this one) stay in the work directory.
    pathlib.Path(args.out).write_text(json.dumps({
        "method": {"truth": f"{len(truths)} lossless PNGs ({pathlib.Path(args.truth).name})",
                   "served": f"q{args.src_quality} 4:2:0", "quality": args.quality,
                   "references": ["linear-light Lanczos", "sRGB Lanczos"]},
        "cells": [rounded(c) for c in summary],
    }, indent=1) + "\n")
    (work / "per_image.json").write_text(json.dumps(results))


if __name__ == "__main__":
    main()
