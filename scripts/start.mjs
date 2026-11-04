// `npm start`: runs the release build on any OS. Arguments pass through,
// for example `npm start -- --port 4318`.
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const name = process.platform === "win32" ? "westerm.exe" : "westerm";
const binary = fileURLToPath(new URL(`../native/target/release/${name}`, import.meta.url));
const child = spawn(binary, process.argv.slice(2), { stdio: "inherit" });

child.on("error", (error) => {
  console.error(`westerm: ${error.message}. Run "npm run build" first.`);
  process.exit(1);
});
child.on("exit", (code, signal) => process.exit(code ?? (signal ? 1 : 0)));

// Ctrl+C reaches westerm directly; wait for it to exit. Forward a plain stop.
process.on("SIGINT", () => {});
process.on("SIGTERM", () => child.kill("SIGTERM"));
