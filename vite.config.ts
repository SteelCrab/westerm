import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// `npm run dev` serves the UI with HMR on :5173 and forwards the API and
// terminal sockets to the Rust server from `npm run dev:server` on :4317.
const server = "http://127.0.0.1:4317";

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
    proxy: {
      // ws: a remote window's status stream is /api/remote/link.
      "/api": { target: server, ws: true },
      "/ws": { target: server, ws: true },
      "/remote": { target: server, ws: true },
    },
  },
  build: { target: "es2022" },
});
