import { bindings, defineConfig } from "cf/config";

// `cf deploy` reads this file; `wrangler dev` reads wrangler.toml (keep
// the two in step). ORIGIN_WORKER names the Worker that serves
// source images for GET /resize/… (default: bench/platforms/origin).
export default defineConfig({
	worker: {
		name: "oximg-workers",
		compatibilityDate: "2026-06-01",
		entrypoint: "src/index.js",
		// Workers Logs: each invocation's CPU time, which the frozen
		// in-request timers cannot measure.
		observability: { enabled: true },
		// A Worker cannot fetch another workers.dev host on the same
		// account (404), so the origin is reached by service binding.
		env: {
			ORIGIN: bindings.worker({ worker: process.env.ORIGIN_WORKER ?? "oximg-bench-origin" }),
		},
	},
});
