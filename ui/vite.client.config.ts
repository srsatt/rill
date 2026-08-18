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
    emptyOutDir: true,
    manifest: true,
    rollupOptions: {
      input: {
        modern: "src/modern-client.tsx"
      }
    }
  }
});
