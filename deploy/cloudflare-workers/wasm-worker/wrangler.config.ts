import { defineWranglerConfig } from "wrangler/experimental-config";

export default defineWranglerConfig({
	build: {
		command: "./build.sh",
		watchDir: "src",
	},
});
