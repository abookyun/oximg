# URL signing

imgproxy-style HMAC over the percent-decoded path. When `OXIMG_KEY`
and `OXIMG_SALT` are set (both or neither), unsigned image URLs are
403. Catalog: [docs/features/routes.md](../../../../docs/features/routes.md)
(Signing), vectors in `tests/ctl.rs` `sign_matches_the_server_vector`
and `tests/server.rs` `signing_gate`.

## Sub-features

- `base64url(HMAC-SHA256(key, salt || path))` over the
  percent-decoded path, unpadded.
- One signature covers every percent-encoding of that same decoded
  path (`%2F` vs `/`). It does **not** cover a different `@{fmt}`.
- Signed positional form: `GET /{sig}/resize/{w}/{h}/{*file}`.
- Signed options form: `GET /{sig}{prefix}/{options}/{*file}` (raw
  option order included).
- `OPTIONS` preflight and `/metrics` are outside the scheme.
- Auto-spawn **strips inherited keys** unless you pass `--env`.

## How to get to it (user POV)

Operators set `OXIMG_KEY` and `OXIMG_SALT` (hex) on the server.
Clients (including `rubygem/oximg-rails`) put the signature in the
first path segment. A URL built for `/resize/100/100/photo.jpg` will
not unlock `/resize/100/100/photo.jpg@webp`.

## Driving it with oximg-ctl

Committed vector (key = `deadbeef` × 8, salt = `cafebabe` × 8):

```sh
CTL=./target/release/oximg-ctl
KEY=deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef
SALT=cafebabecafebabecafebabecafebabecafebabecafebabecafebabecafebabe

$CTL sign /resize/100/100/photo.jpg --key "$KEY" --salt "$SALT"
# expect: signature t-jKRoyvzhs4dEBnGGBUS_t6Uh_HE6WysfGYvs8UaTo
# url /t-jKRoyvzhs4dEBnGGBUS_t6Uh_HE6WysfGYvs8UaTo/resize/100/100/photo.jpg

$CTL sign /resize/100/100/photo.jpg@webp --key "$KEY" --salt "$SALT"
# expect: signature XQ8C3eYRVAkFAnUczGBsuXMOu-J6vMoYi3W8_4-sT6Q

# Live gate (keys on the spawn, not inherited from the parent shell)
$CTL --env OXIMG_KEY="$KEY" --env OXIMG_SALT="$SALT" \
  get /resize/100/100/photo.jpg --expect 403

$CTL --env OXIMG_KEY="$KEY" --env OXIMG_SALT="$SALT" \
  get /t-jKRoyvzhs4dEBnGGBUS_t6Uh_HE6WysfGYvs8UaTo/resize/100/100/photo.jpg \
  --expect 200
# expect: status 200, content_type image/jpeg, probe 100×75
```

Pass `--env` **both** keys, non-empty. Empty `OXIMG_KEY=` is unset
(signing off). Half-signing (only one `--env`) must not boot a spawn
that silently 403s unsigned URLs.

## Gotchas

- `oximg-ctl sign` is library HMAC, not HTTP. A scheme or route change
  also needs a signed `get` against a keyed spawn.
- Dry-run `serve` redacts `OXIMG_KEY` / `OXIMG_SALT` as `<redacted>`.
- Nested `%2F`: sign the decoded path; the server verifies every
  encoding of it. Vectors for `albums%2F2026%2Fphoto.jpg` are in
  `tests/ctl.rs`.
- Do not copy production secrets into evidence JSON; use the committed
  test vector.
