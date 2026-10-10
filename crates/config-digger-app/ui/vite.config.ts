import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { thirdPartyLicensesPlugin } from "./build/licenses.ts";

// The Tauri window loads the dev server on this fixed port (`devUrl`).
export default defineConfig({
  plugins: [react(), thirdPartyLicensesPlugin()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  // The third-party licenses, about 1.2 MB with their texts, are a chunk of
  // their own that only About's dialog loads.
  build: { target: "es2022", outDir: "dist", emptyOutDir: true, chunkSizeWarningLimit: 1600 },
});
