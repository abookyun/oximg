---
name: verify
description: >
  Close the loop on oximg changes by driving the compiled binary with
  oximg-ctl and matching JSON to docs/features/. cargo test green is
  not done. Use when changing HTTP routes, pixels, knobs, formats,
  CLI, signing, SIMD, or when asked to verify, prove, /verify,
  oximg-ctl, or whether a change actually works.
---

# Verify oximg

`cargo test --release` is necessary and not sufficient. A change that
touches HTTP, pixels, or a knob is verified only when `oximg-ctl` has
run the **compiled** artifact and the JSON matches
[docs/features/](../../../docs/features/).

Invariants, routes, formats, knobs, and error classes live in that
map — do not copy them here. `oximg-ctl --help` is the CLI contract.

## Build

```sh
cargo build --release                 # both oximg and oximg-ctl
# cargo run --bin oximg-ctl does not build oximg
```

Then `./target/release/oximg-ctl …`. stdout is one JSON object
(`--help` / `--version` are plain text). Auto-spawn isolates inherited
signing and `OXIMG_SOURCE_BASE_URL`; `--base` is a foreign tree (no
local fixture oracle).

## Pick a layer (do not run every layer every time)

| Touch | Proof |
|---|---|
| Any PR | `cargo test --release` (add `--features avif` if AVIF toolchain is present) |
| Routes, signing, `@{fmt}`, box, errors | `oximg-ctl get` and/or `matrix` covering the change |
| CLI / one-shot | `oximg-ctl resize` and `probe` |
| Decode / resize / encoder / default knobs | [bench/quality/QUALITY.md](../../../bench/quality/QUALITY.md) |
| Hot path latency/throughput | [bench/METHODOLOGY.md](../../../bench/METHODOLOGY.md) — interleaved A/B, never sequential |
| SIMD | the same proof on **amd64 and arm64** |

A new behavior also needs a `tests/ctl.rs` (or sibling suite) cell so
CI keeps the proof. Running ctl once locally is not a substitute.

## JSON that counts as proof

- `ok: true` and process exit 0
- HTTP: `status` (and `--expect` when you care), `content_type`,
  `probe.width` / `probe.height` for 200 image bodies
- Failures: non-2xx is a successful **observation** unless `--expect`
  disagrees; `probe.error` on a 200 image is a failed proof

Show the JSON (or the failing object) in the PR / reply. "tests passed"
is not proof.
