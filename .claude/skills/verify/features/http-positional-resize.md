# Positional HTTP resize

The default user path: `GET /resize/{w}/{h}/{*file}` fits a source
inside a box (never enlarges, aspect preserved) and re-encodes it.
Catalog: [docs/features/routes.md](../../../../docs/features/routes.md)
(Positional), [invariants.md](../../../../docs/features/invariants.md)
(Geometry), [errors.md](../../../../docs/features/errors.md).

## Sub-features

- `{file}` may span directories; `.` / `..` / empty / control bytes → 400;
  escaping `IMAGES_DIR` → 404.
- `0` on one axis is unconstrained (`/resize/750/0/…` is width-only).
  Both axes zero → 400. Each axis 1–8192.
- Box is a bound: `photo.jpg` (200×150) into 100×100 → 100×75;
  `tiny.jpg` (40×30) into 100×100 stays 40×30.
- Optional `@{fmt}` suffix is a different feature
  ([format-at-token.md](format-at-token.md)).
- Signed form `GET /{sig}/resize/…` when keys are set
  ([url-signing.md](url-signing.md)). Unsigned with signing on → 403.

## How to get to it (user POV)

A client (or CDN) requests:

```
GET /resize/100/100/photo.jpg
```

against a running `oximg` whose `IMAGES_DIR` holds `photo.jpg` (or
whose `OXIMG_SOURCE_BASE_URL` serves that key). No extra env is
required. `GET /health` is always on and returns body `ok`.

## Driving it with oximg-ctl

Auto-spawn; do not hold a server.

```sh
CTL=./target/release/oximg-ctl

$CTL get /resize/100/100/photo.jpg --expect 200
# expect: status 200, content_type image/jpeg, probe.width 100, probe.height 75

$CTL get /resize/100/100/tiny.jpg --expect 200
# expect: probe 40×30 (upscale guard)

$CTL get /resize/750/0/photo.jpg --expect 200
# width-only: source is already 200 wide → probe 200×150

$CTL get /resize/0/0/photo.jpg --expect 400
$CTL get /resize/100/100/missing.jpg --expect 404

$CTL matrix --source photo.jpg --box 100x100
# expect: ok true, failed 0; cells include the 200 and the 400/404 negatives
```

`--write PATH` saves the body when you need bytes on disk. `matrix`
without `--no-negatives` also hits `/resize/0/0/photo.jpg` (400),
`/resize/100/100/missing.jpg` (404 if that file is absent), and
`@gif` (400).

## Gotchas

- Inherited `OXIMG_KEY`/`OXIMG_SALT` are stripped on auto-spawn so this
  unsigned path stays 200. Pass both `--env` values to opt into
  signing ([url-signing.md](url-signing.md)).
- `--base` is a foreign tree: `matrix` will not 404 `missing.jpg`
  against it (no local oracle).
- `probe` reports stored size; geometry proofs on 200 bodies use
  **output** `probe.width` / `probe.height` after the resize.
- Non-2xx without `--expect` is still exit 0 (observation). Use
  `--expect` when the status is the proof.
- `cargo run --bin oximg-ctl` does not build `oximg`; Launch with
  `cargo build --release`.
