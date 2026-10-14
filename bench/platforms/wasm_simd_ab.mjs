// Interleaved A/B of two oximg wasip1 builds on V8 (node), the engine
// Workers runs. Each round runs A then B (order alternating) on a fresh
// instance per module, same input, same params.
//
//   node bench/platforms/wasm_simd_ab.mjs a.wasm b.wasm image.jpg [rounds] [w] [fmt]
import { readFileSync } from "node:fs";
import { Oximg, FORMATS } from "../../deploy/cloudflare-workers/wasm-worker/src/oximg.js";

const [a, b, img, rounds = "7", w = "800", fmt = "webp"] = process.argv.slice(2);
const mods = [a, b].map((p) => new WebAssembly.Module(readFileSync(p)));
const src = new Uint8Array(readFileSync(img));
const inst = mods.map((m) => new Oximg(m));
// Warm-up: JIT tiers and memory growth.
for (const o of inst) o.process(src, +w, +w, FORMATS[fmt], 0);
const times = [[], []];
let bytes = [null, null];
for (let r = 0; r < +rounds; r++) {
  for (const i of r % 2 ? [1, 0] : [0, 1]) {
    const t0 = performance.now();
    const out = inst[i].process(src, +w, +w, FORMATS[fmt], 0);
    times[i].push(performance.now() - t0);
    bytes[i] = out.bytes;
  }
}
const med = (xs) => [...xs].sort((x, y) => x - y)[xs.length >> 1];
for (const i of [0, 1]) {
  console.log(
    `${["A", "B"][i]} ${[a, b][i]}: median ${med(times[i]).toFixed(1)} ms ` +
      `[${times[i].map((t) => t.toFixed(0)).join(" ")}] mem ${inst[i].memoryBytes / 1048576} MB`,
  );
}
const same = bytes[0].length === bytes[1].length && bytes[0].every((v, k) => v === bytes[1][k]);
console.log(`output ${same ? "byte-identical" : "DIFFERS"} (${bytes[0].length} / ${bytes[1].length} bytes)`);
