// Answers terminal queries the way westerm's browser pane does: with the same
// ghostty-vt WASM core. Reads PTY output on stdin and writes replies on stdout.
// At exit it reports on stderr how long the core spent parsing.
import { readFileSync } from "node:fs";
import { GhosttyCore } from "@wterm/ghostty";

const [cols, rows, imageLimit] = process.argv.slice(2).map(Number);
const wasm = new URL("../../node_modules/@wterm/ghostty/wasm/ghostty-vt.wasm", import.meta.url);
const realFetch = globalThis.fetch;
globalThis.fetch = async (url, init) =>
  String(url).startsWith("file:")
    ? new Response(readFileSync(new URL(String(url))), { headers: { "content-type": "application/wasm" } })
    : realFetch(url, init);

const core = await GhosttyCore.load({ wasmPath: wasm.href, imageStorageLimit: imageLimit });
core.init(cols, rows);
let parseNs = 0n;
let bytes = 0;
process.stdin.on("data", (chunk) => {
  const start = process.hrtime.bigint();
  core.writeRaw(new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength));
  parseNs += process.hrtime.bigint() - start;
  bytes += chunk.byteLength;
  // wterm reads every queued response after each write (wterm.js).
  let reply;
  while ((reply = core.getResponse()) !== null) process.stdout.write(reply);
});
const report = () => {
  process.stderr.write(JSON.stringify({ parseMs: Number(parseNs) / 1e6, bytes }) + "\n");
  process.exit(0);
};
process.stdin.on("end", report);
process.on("SIGTERM", report);
