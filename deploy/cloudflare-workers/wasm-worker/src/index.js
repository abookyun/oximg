// Path B: a JS Worker driving oximg compiled to a wasm32-wasip1 reactor.
// POST an image body to /?w=400&h=400&fmt=webp&q=80.
import wasmModule from "../build/oximg.wasm";
import { FORMATS, TYPES, Oximg } from "./oximg.js";

let oximg = null;

// Same origin and path shape as the native server's positional route
// (/resize/{w}/{h}/{file}[@fmt]), so the platform comparison fetches
// identical sources everywhere.
const RESIZE = /^\/resize\/(\d+)\/(\d+)\/([^@]+)(?:@(\w+))?$/;

async function request(req, env) {
  const u = new URL(req.url);
  if (req.method === "GET") {
    const m = RESIZE.exec(u.pathname);
    if (!m) return { error: new Response("GET /resize/{w}/{h}/{file}[@fmt]", { status: 404 }) };
    const fmt = m[4] ? FORMATS[m[4]] : 0;
    if (fmt === undefined) return { error: new Response(`bad @${m[4]}`, { status: 400 }) };
    const r = await env.ORIGIN.fetch(`https://origin/${m[3]}`);
    if (!r.ok) return { error: new Response(`origin ${r.status}`, { status: 502 }) };
    return { src: new Uint8Array(await r.arrayBuffer()), w: +m[1], h: +m[2], fmt, q: 0 };
  }
  if (req.method !== "POST") return { error: new Response("POST an image body to /?w=&h=&fmt=&q=", { status: 405 }) };
  const fmt = u.searchParams.has("fmt") ? FORMATS[u.searchParams.get("fmt")] : 0;
  if (fmt === undefined) return { error: new Response(`bad fmt=${u.searchParams.get("fmt")}`, { status: 400 }) };
  const num = (k) => Number(u.searchParams.get(k) ?? 0) || 0;
  return { src: new Uint8Array(await req.arrayBuffer()), w: num("w"), h: num("h"), fmt, q: num("q") };
}

export default {
  async fetch(req, env) {
    const { error, src, w, h, fmt, q } = await request(req, env);
    if (error) return error;
    const t0 = performance.now();
    let r;
    try {
      oximg ??= new Oximg(wasmModule);
      r = oximg.process(src, w, h, fmt, q);
    } catch (e) {
      // A trap leaves the instance unusable; start fresh next request.
      oximg = null;
      return new Response(`wasm trap: ${e.message}`, { status: 422 });
    }
    if (r.status !== 0) return new Response(new TextDecoder().decode(r.bytes), { status: 422 });
    return new Response(r.bytes, {
      headers: {
        "content-type": TYPES[r.format],
        "x-oximg-pipeline-ms": String(Math.round(performance.now() - t0)),
        "x-oximg-path": "js+wasip1",
        // The number that meets the 128 MB isolate limit.
        "x-oximg-wasm-memory-mb": String(oximg.memoryBytes / 1048576),
      },
    });
  },
};
