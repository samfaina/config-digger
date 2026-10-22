import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import { thirdPartyLicensesPlugin } from "./build/licenses.ts";

export default defineConfig(({ mode }) => ({
  plugins: [react(), thirdPartyLicensesPlugin()],
  clearScreen: false,
  // The Tauri window loads the dev server on a fixed port (`devUrl`). The
  // fake mode, `npm run dev:fake`, serves the UI in a browser on the fake
  // backend, which `bindings.ts` reaches in place of Tauri's `invoke`, on
  // another port or the next free one, beside the app's dev server.
  server: mode === "fake" ? { port: 1430 } : { port: 1420, strictPort: true },
  resolve:
    mode === "fake"
      ? { alias: [{ find: /^@tauri-apps\/api\/core$/, replacement: fileURLToPath(new URL("./src/fake/core.ts", import.meta.url)) }] }
      : {},
  // The third-party licenses, about 1.2 MB with their texts, are a chunk of
  // their own that only About's dialog loads.
  build: { target: "es2022", outDir: "dist", emptyOutDir: true, chunkSizeWarningLimit: 1600 },
}));
