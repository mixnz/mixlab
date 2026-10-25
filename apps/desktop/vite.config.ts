import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  resolve: {
    alias: {
      // bindings/ at the repository root — see the note on `paths` in tsconfig.json.
      "@mixengine/api": fileURLToPath(new URL("../../bindings/index.ts", import.meta.url)),
    },
  },

  // Two pages: the main window, and the tray panel's window (`src-tauri/src/tray.rs`).
  build: {
    rollupOptions: {
      input: {
        main: fileURLToPath(new URL("index.html", import.meta.url)),
        tray: fileURLToPath(new URL("tray.html", import.meta.url)),
      },
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    fs: {
      // Vite refuses to serve anything above the project root by default; the contract sits two
      // levels up. Allowing the repository root, not "/", keeps the refusal for everything else.
      allow: [fileURLToPath(new URL("../../", import.meta.url))],
    },
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },

  /* Vitest stubs every `.css` request out by default — a test run has nothing to paint, so
     processing stylesheets buys nothing. But the stub answers `?raw` too, with an empty string,
     and a test that parses an empty stylesheet finds no rules, filters them to an empty list, and
     asserts that the list is empty: it passes for ever without reading a line.

     A stylesheet test once did exactly that from the day it was written. The stylesheet tests now
     assert they read something before asserting anything about it, but this is the fix for the
     cause. */
  test: {
    css: true,
  },
}));
