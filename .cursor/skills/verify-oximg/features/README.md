# oximg verification feature map (Cursor)

Agent drive book for [verify-oximg](../SKILL.md). Invariants, status
tables, and the full knob inventory live in
[`docs/features/`](../../../docs/features/) — link out, do not copy.

Drive with `oximg-ctl` (JSON on stdout, real `oximg` underneath,
fixtures under `tests/fixtures/`). `cargo test --release` is necessary
and not sufficient; a cell here is proved only when ctl JSON matches
the linked invariant.

After a route, knob, format, or status is added or removed, run
`/maintain-verification-skill` and update the matching file (or add
one). Complementary Grok skill:
[`.grok/skills/verify/SKILL.md`](../../../.grok/skills/verify/SKILL.md).

| File | User-facing surface |
|---|---|
| [http-positional-resize.md](http-positional-resize.md) | `GET /resize/{w}/{h}/{*file}` |
| [cli-resize-probe.md](cli-resize-probe.md) | `oximg resize` / `oximg probe` one-shots |
| [format-at-token.md](format-at-token.md) | `@{fmt}`, GIF→WebP, encode matrix |
| [url-signing.md](url-signing.md) | imgproxy-style HMAC on HTTP paths |
| [options-prefix.md](options-prefix.md) | Cloudflare-style `OXIMG_OPTIONS_PREFIX` route |

Default proof (first cell, JPEG/PNG/WebP fixtures only — skip AVIF
unless the toolchain is already present): positional HTTP resize of
`tests/fixtures/photo.jpg`.
