# CLI resize / probe

One-shot surface: no HTTP. `oximg resize` fits a file and writes
bytes; `oximg probe` is header-only stored size (plus animation
metadata). Catalog: [docs/features/paths.md](../../../docs/features/paths.md)
(CLI), [formats.md](../../../docs/features/formats.md).

## Sub-features

- `oximg resize <in> <max_w> <max_h> <out> [-q N] [-f fmt] [--preset P]`
- `0` on one axis unconstrained; `0 0` re-encodes at native size
  (**CLI only** — the server refuses `0/0`).
- Output format: `-f` / `--format`, else `<out>` extension, else
  source format (GIF → WebP).
- Usage errors exit 2; processing failures exit 1.
- `oximg probe <file>` prints format, stored dimensions, and
  animation frames/duration/loop when present.

## How to get to it (user POV)

```sh
oximg resize tests/fixtures/photo.jpg 100 100 /tmp/out.jpg
oximg probe tests/fixtures/anim.gif
```

Same pipeline knobs (`OXIMG_*`, `QUALITY`, `PRESET`) as the server,
fail-closed at startup.

## Driving it with oximg-ctl

`resize` shells out to `oximg resize` (the argv a user types).
`oximg-ctl probe` is in-process library — when the **CLI argv** for
probe is what changed, also run `./target/release/oximg probe` or
`tests/cli.rs`.

```sh
CTL=./target/release/oximg-ctl

$CTL probe tests/fixtures/photo.jpg
# expect: ok true, probe.content_type image/jpeg, width 200, height 150

$CTL probe tests/fixtures/anim.gif
# expect: content_type image/gif, animation.frames 3, duration_ms 1500

$CTL resize tests/fixtures/photo.jpg 100 100 --out /tmp/oximg-verify-cli.jpg
# expect: ok true, probe 100×75, image/jpeg, out path set
# side effect: that file exists and probes the same

$CTL resize tests/fixtures/anim.gif 100 100 -f webp --out /tmp/oximg-verify-cli.webp
# expect: image/webp (GIF has no encoder)

$CTL resize tests/fixtures/photo.jpg 80 80 -q 0
# expect: exit 2, ok false (quality 1–100)
```

Omit `--out` to probe an ephemeral file that ctl deletes (JSON will
not contain `out`). `--dry-run` prints `argv` and must not clobber an
existing `--out`.

## Gotchas

- `oximg-ctl probe` does not execute `oximg probe`; do not treat it as
  coverage of CLI help/argv.
- Geometry: 80×80 on 200×150 → 80×60 (same fit rule as HTTP).
- `tests/fixtures/list.txt` is undecodable: `probe` exits 1 with
  `ok: false`.
- Do not leave `/tmp/oximg-verify-cli.*` as evidence; copy into
  `evidence/` first or `--out` directly there. Cleanup may remove
  `/tmp` files.
