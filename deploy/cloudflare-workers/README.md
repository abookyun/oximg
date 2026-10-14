# oximg on Cloudflare Workers (experimental)

Workers cannot run native code, so this is `oximg::pipeline` compiled
to wasm. Two packagings are here. Route B is the one deployed and
measured; for whether to use Workers at all, see
[docs/deploy-targets.md](../../docs/deploy-targets.md).

| | A: `rust-worker/` | B: `wasm-worker/` |
|---|---|---|
| Shape | workers-rs `#[event(fetch)]`, worker-build + wasm-bindgen | JS Worker + our `.wasm` with a C ABI |
| Target | `wasm32-unknown-unknown` | `wasm32-wasip1` (reactor) |
| C/C++ libc | wasi-libc linked by hand; `src/wasi_stubs.rs` defines the WASI calls | wasi-libc natively; `src/oximg.js` shims WASI in JS |
| Crate patches | mozjpeg, jpegli-sys (`patches/`) | none |
| wasm, gzip -9 | 1.09 MB | 0.91 MB |
| Corrupt JPEG | trap → 500; workers-rs reinitializes | trap → JS drops the instance → 422 |
| Deployed | local `wrangler dev` only | yes, with `cf deploy` |

Verified 2026-10-14 with wasi-sdk 34, Rust 1.96, worker-build 0.8.7,
wrangler 4.95.0 (local dev) and 4.145.0 (`cf deploy`, via cf
1.0.0-beta.14). wrangler 4.95 exits silently under node 24, so use
node 22.

## Results

`smoke.sh` POSTs every fixture to a worker and compares the bytes with
the native `transcode` example. Locally and deployed, every fixture is
byte-identical on both routes except two. `cmyk_icc.jpg` has 8 of 3072
pixels off by 1/255 (moxcms SIMD vs scalar rounding), and
`bogus_huffman.jpg` traps (see Gaps). The fixtures are ≤ 400 px and
never resize. A 3 MP JPEG resized to 800 px differs from x86 native in
~6 of 480 000 pixels, by 1/255. Routes A and B match each other byte
for byte.

Deployed CPU per transform is 129 / 256 / 685 ms for 1 / 3 / 12 MP
sources, 3–6× native; the method is in docs/deploy-targets.md. A
12 MP source peaks at 97 MB of linear memory, under the isolate's
128 MB. `WASM_SIMD=1` (simd128 for Rust and C) moved 3 MP from
177 to 173 ms, median of an interleaved 7-round A/B in node 22
(`bench/platforms/wasm_simd_ab.mjs`). That is noise: no hot loop has a
wasm SIMD path.

## What it took

1. **A C sysroot.** `wasm32-unknown-unknown` has no libc, so mozjpeg
   stops at `stdlib.h`. `env.sh` points cc and cmake at wasi-sdk for
   both targets.
2. **`<setjmp.h>` in jpegli's `error.cc`.** It is included but never
   called, and wasi-libc refuses the header unless wasm exception
   handling is on. `shim-include/setjmp.h` is empty.
3. **libc++, libc++abi and compiler-rt link flags** (`env.sh`).
4. **Route A only:** the `libc` crate has no `free`, `fdopen` or `FILE`
   on unknown-unknown. mozjpeg imports the first two unused, and
   jpegli-sys aliases `FILE`. `vendor.sh` copies both crates out of the
   registry and applies `patches/`.
5. **In oximg:** `std::time::Instant::now()` panics on unknown-unknown;
   `src/clock.rs` gives the stage timers a zero-duration stand-in
   there.

## Gaps

- **Errors trap.** Both targets are panic=abort, and libjpeg's fatal
  errors reach Rust by unwinding (`src/panic_guard.rs`), so a corrupt
  JPEG kills the instance instead of returning `Undecodable`. The fix
  is wasm exception handling (`-C panic=unwind` with
  `+exception-handling` through `-Zbuild-std`, nightly), which V8
  supports. Not tried yet.
- **Memory.** The streaming decode→resize path (`jpeg.rs`, `fuse.rs`)
  is gated to x86_64/aarch64 because `FuseKernel` is AVX2 or NEON, so
  wasm decodes the whole frame to u16 first. That fallback, not the
  codecs, is why 12 MP reaches 97 MB. A scalar or simd128 `RowKernel`
  would put wasm on the streaming path.
- **CPU.** The free plan's 10 ms CPU limit rules out all but tiny
  images. Paid plans (30 s default) fit, and CPU-ms is what they bill.
- **Missing:** URL signing, AVIF (SVT-AV1 and dav1d are not ported),
  the `server` feature's remote fetch. Route B's GET path fetches
  sources over a service binding instead.

## Run it

```sh
# once: an unpacked wasi-sdk, worker-build (route A), the patched crates
export WASI_SDK_PATH=~/wasi-sdk-34.0-x86_64-macos
cargo install worker-build@0.8.7 --locked
deploy/cloudflare-workers/vendor.sh

# local (workerd): route A or B, then POST fixtures at it
cd deploy/cloudflare-workers/rust-worker   # or wasm-worker
npx wrangler@4.95.0 dev --port 8791
cargo build --release --example transcode --no-default-features
deploy/cloudflare-workers/smoke.sh http://127.0.0.1:8791
```

Deploy route B (`cf auth whoami` must show the account):

```sh
cd deploy/cloudflare-workers/wasm-worker
npm install --ignore-scripts   # sharp, a miniflare dependency, does not build on Intel macOS
cf deploy                      # runs ./build.sh, which needs WASI_SDK_PATH
```

`GET /resize/{w}/{h}/{file}[@fmt]` reads sources from the Worker named
by `ORIGIN_WORKER` at deploy time (default `oximg-bench-origin`,
the static origin in `bench/platforms/origin/`). `POST /?w=&h=&fmt=&q=`
takes the image in the body.
