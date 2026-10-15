// oximg's wasip1 reactor behind a small JS API, shared by the Worker
// (index.js) and the node A/B bench (bench/platforms/wasm_simd_ab.mjs).

const ENOSYS = 52, EBADF = 8;

// The handful of WASI calls wasi-libc and Rust std make here: stdio to
// the console, a clock, randomness for HashMap seeds; no files, env,
// or argv. Anything else answers ENOSYS.
function wasiImports(memory) {
  const dv = () => new DataView(memory().buffer);
  const u8 = () => new Uint8Array(memory().buffer);
  const impl = {
    fd_write(fd, iovs, n, nwritten) {
      const v = dv();
      let total = 0, text = "";
      for (let i = 0; i < n; i++) {
        const p = v.getUint32(iovs + i * 8, true), l = v.getUint32(iovs + i * 8 + 4, true);
        text += new TextDecoder().decode(u8().subarray(p, p + l));
        total += l;
      }
      v.setUint32(nwritten, total, true);
      if (fd !== 1 && fd !== 2) return EBADF;
      console.log(`[wasm fd${fd}] ${text.trimEnd()}`);
      return 0;
    },
    environ_sizes_get(c, s) { dv().setUint32(c, 0, true); dv().setUint32(s, 0, true); return 0; },
    environ_get() { return 0; },
    args_sizes_get(c, s) { dv().setUint32(c, 0, true); dv().setUint32(s, 0, true); return 0; },
    args_get() { return 0; },
    clock_time_get(_id, _prec, out) {
      // Workers freezes timers during execution; good enough for logs.
      dv().setBigUint64(out, BigInt(Date.now()) * 1000000n, true);
      return 0;
    },
    random_get(p, l) { crypto.getRandomValues(u8().subarray(p, p + l)); return 0; },
    proc_exit(code) { throw new Error(`wasm called exit(${code})`); },
    fd_prestat_get() { return EBADF; },
    fd_close() { return EBADF; },
    sched_yield() { return 0; },
  };
  return new Proxy(impl, { get: (t, k) => t[k] ?? (() => ENOSYS) });
}

export const FORMATS = { jpg: 1, jpeg: 1, png: 2, webp: 3 };
export const TYPES = { 1: "image/jpeg", 2: "image/png", 3: "image/webp" };

export class Oximg {
  /** @param {WebAssembly.Module} module */
  constructor(module) {
    let inst;
    inst = new WebAssembly.Instance(module, {
      wasi_snapshot_preview1: wasiImports(() => inst.exports.memory),
    });
    inst.exports._initialize?.();
    this.x = inst.exports;
  }

  /** Linear memory never shrinks: this is the high-water mark. */
  get memoryBytes() {
    return this.x.memory.buffer.byteLength;
  }

  /**
   * Throws WebAssembly.RuntimeError on a trap (panic=abort: libjpeg's
   * fatal errors, OOM), after which this instance must be dropped.
   * @returns {{status: number, format: number, bytes: Uint8Array}}
   */
  process(src, w, h, fmt, q) {
    const x = this.x;
    const p = x.oximg_alloc(src.length);
    new Uint8Array(x.memory.buffer, p, src.length).set(src);
    try {
      const r = x.oximg_process(p, src.length, w, h, fmt, q);
      const v = new DataView(x.memory.buffer);
      const [status, format, ptr, len] = [0, 4, 8, 12].map((o) => v.getUint32(r + o, true));
      const bytes = new Uint8Array(x.memory.buffer, ptr, len).slice();
      x.oximg_out_free(r);
      return { status, format, bytes };
    } finally {
      x.oximg_free(p, src.length);
    }
  }
}
