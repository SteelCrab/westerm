import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// `npm run dev` serves the UI with HMR on :5173 and forwards the API and
// terminal sockets to the Rust server from `npm run dev:server` on :4317.
const server = "http://127.0.0.1:4317";

// The dashboard page's policy, as the Rust server sends it (PAGE_CSP in
// native/src/server/mod.rs): frames show only this origin, so a plugin iframe
// that navigates elsewhere is blocked and stopped. Proxied responses keep the
// Rust server's own headers.
const headers = { "Content-Security-Policy": "frame-src 'self'" };

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      // wterm's Kitty key encoder is internal (not in package exports). The
      // dashboard reuses it so forwarded Cmd keys match wterm's own encoding.
      "@wterm-internal/kitty-keys": fileURLToPath(
        new URL("./node_modules/@wterm/dom/dist/kitty-keys.js", import.meta.url),
      ),
    },
  },
  server: {
    headers,
    proxy: {
      // ws: a remote window's status stream is /api/remote/link.
      "/api": { target: server, ws: true },
      "/ws": { target: server, ws: true },
      "/remote": { target: server, ws: true },
      // External plugins' panel pages and the plugin SDK they load.
      "/plugin-assets": { target: server },
      "/plugin-sdk": { target: server },
    },
  },
  preview: { headers },
  build: { target: "es2022" },
});
