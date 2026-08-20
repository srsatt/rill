import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import { fileURLToPath, URL } from "node:url";

export default defineConfig({
  plugins: [solid()],
  resolve: {
    alias: {
      "~": fileURLToPath(new URL("./src", import.meta.url))
    }
  },
  build: {
    outDir: "dist/client",
    // Keep hashed files referenced by pages that were open while dev rebuilds.
    // Clean release workspaces still start empty, while live pages avoid 404s.
    emptyOutDir: false,
    manifest: true,
    rollupOptions: {
      input: {
        modern: "src/modern-client.tsx"
      }
    }
  }
});
