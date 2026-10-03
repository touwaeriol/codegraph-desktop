import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  clearScreen: false,
  optimizeDeps: { entries: ["index.html"] },
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/target/**", "**/src-tauri/**", "**/.tools/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
});
