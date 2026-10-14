import { defineConfig } from "cf/config";

// Static origin for the platform comparison: every candidate fetches
// the same source JPEGs from here (OXIMG_SOURCE_BASE_URL).
export default defineConfig({
	worker: {
		name: "oximg-bench-origin",
		compatibilityDate: "2026-06-01",
	},
});
