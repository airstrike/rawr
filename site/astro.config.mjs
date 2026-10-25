// The site: the landing page, the docs and the playground, pre-rendered to
// static HTML in dist/. React loads only for the playground's island.
// play/deploy/cloudflare/build.mjs runs this build and merges dist/ with the
// bot's files into the Worker's assets.
import react from "@astrojs/react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "astro/config";

export default defineConfig({
  site: "https://rawr.airstrike.fyi",
  // A page is a file as the Worker's assets serve it today: docs/run.html for
  // /docs/run, and docs/iced/index.html for /docs/iced.
  build: { format: "preserve" },
  trailingSlash: "never",
  compressHTML: false,
  devToolbar: { enabled: false },
  integrations: [react()],
  vite: { plugins: [tailwindcss()] },
});
