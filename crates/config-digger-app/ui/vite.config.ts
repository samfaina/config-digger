import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The Tauri window loads the dev server on this fixed port (`devUrl`).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
});
