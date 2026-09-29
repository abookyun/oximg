import { defineConfig } from "astro/config";

export default defineConfig({
  // Placeholder until the domain is decided; only affects canonical URLs.
  site: "https://oximg.dev",
  // Everything is static; islands opt in to JS individually.
  output: "static",
  build: {
    inlineStylesheets: "always",
  },
});
