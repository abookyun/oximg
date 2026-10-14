# Options-prefix / Cloudflare-style route

Mounted only when `OXIMG_OPTIONS_PREFIX` is set (e.g. `/image`,
`/cdn-cgi/image`). Speaks `key=value,key=value` so Cloudflare Images
URLs survive without a rewrite layer. Catalog:
[docs/features/routes.md](../../../../docs/features/routes.md) (Options),
[knobs.md](../../../../docs/features/knobs.md).

## Sub-features

- `GET {prefix}/{options}/{*file}` with keys `width`, `height`
  (1–8192, at least one required), `quality` (1–100), `format`
  (`jpeg|png|webp|avif|auto`).
- Filename is literal: **no** `@{fmt}` on this route. `format=` owns
  the choice; absent/`auto` = same negotiation as a bare positional URL.
- Unknown or duplicate keys → 400 naming the key (fail-closed;
  Cloudflare would ignore `fit=cover` and change pixels).
- Colliding with `/health`, `/metrics`, `/resize` is fatal at boot.
- Signed form: `GET /{sig}{prefix}/{options}/{*file}`; raw option
  order is part of the signed material.

## How to get to it (user POV)

```
OXIMG_OPTIONS_PREFIX=/image
GET /image/width=100,quality=80/photo.jpg
GET /image/width=100,format=webp/photo.jpg
```

Without the env, `/image/…` is not mounted (positional 404).

## Driving it with oximg-ctl

```sh
CTL=./target/release/oximg-ctl

$CTL --env OXIMG_OPTIONS_PREFIX=/image \
  get /image/width=100,quality=80/photo.jpg --expect 200
# expect: status 200, image/jpeg, probe.width 100, probe.height 75

$CTL --env OXIMG_OPTIONS_PREFIX=/image \
  get /image/width=100,format=webp/photo.jpg --expect 200
# expect: image/webp

$CTL --env OXIMG_OPTIONS_PREFIX=/image \
  get /image/width=100,fit=cover/photo.jpg --expect 400
# body_text names the unknown key

# unmounted by default
$CTL get /image/width=100/photo.jpg --expect 404
```

`--env` applies to the auto-spawned server. Combine with signing the
same way as positional: both `OXIMG_KEY` and `OXIMG_SALT` plus the
options path (vector in `tests/server.rs` around
`/image/width=100,quality=80/photo.jpg`).

## Gotchas

- Prefix must start with `/` and must not collide; invalid values
  refuse to boot (`stderr` names `OXIMG_OPTIONS_PREFIX`).
- `quality=` is per-request and overrides process-wide `QUALITY`.
- Do not append `@webp` to the filename here; use `format=webp`.
- `--env OXIMG_OPTIONS_PREFIX=/image` on `resize`/`probe` is
  meaningless (no HTTP). Use it on `get` / `matrix` / `serve`.
