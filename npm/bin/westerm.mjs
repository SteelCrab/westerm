#!/usr/bin/env node
import { spawn } from "node:child_process";
import { constants } from "node:os";
import { fileURLToPath } from "node:url";
import { filesFor, targetFor } from "./platform.mjs";

try {
  const target = targetFor(process.platform, process.arch);
  const binary = fileURLToPath(new URL(`../vendor/${target}/${filesFor(target)[0]}`, import.meta.url));
  const child = spawn(binary, process.argv.slice(2), { stdio: "inherit" });
  const forward = (signal) => child.kill(signal);
  process.on("SIGINT", forward);
  process.on("SIGTERM", forward);
  child.on("error", (error) => {
    console.error(`westerm: ${error.message}. Reinstall with "npm install -g @pistacrab/westerm".`);
    process.exitCode = 1;
  });
  child.on("exit", (code, signal) => {
    process.exitCode = code ?? (128 + (constants.signals[signal] ?? 1));
  });
} catch (error) {
  console.error(`westerm: ${error.message}`);
  process.exitCode = 1;
}
