import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import { fileURLToPath, URL } from "node:url";

export default defineConfig({
  plugins: [solid({ ssr: true })],
  resolve: {
    alias: {
      "~": fileURLToPath(new URL("./src", import.meta.url)),
      "solid-js/web": fileURLToPath(new URL("./src/server/solid-ssr.ts", import.meta.url)),
      "solid-js": fileURLToPath(new URL("./src/server/solid-core.ts", import.meta.url))
    }
  },
  build: {
    outDir: "dist/renderer",
    emptyOutDir: true,
    ssr: "src/renderer.tsx",
    rollupOptions: {
      output: {
        entryFileNames: "renderer.js",
        inlineDynamicImports: true
      }
    }
  }
});
