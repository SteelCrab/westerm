export const targets = [
  "aarch64-apple-darwin",
  "x86_64-apple-darwin",
  "aarch64-unknown-linux-musl",
  "x86_64-unknown-linux-musl",
  "x86_64-pc-windows-msvc",
];

export function targetFor(platform, arch) {
  if (arch !== "x64" && arch !== "arm64") {
    throw new Error(`unsupported platform: ${platform}/${arch}`);
  }
  const cpu = arch === "arm64" ? "aarch64" : "x86_64";
  if (platform === "darwin") return `${cpu}-apple-darwin`;
  if (platform === "linux") return `${cpu}-unknown-linux-musl`;
  // Windows on Arm runs the release's x64 executable through emulation.
  if (platform === "win32") return "x86_64-pc-windows-msvc";
  throw new Error(`unsupported platform: ${platform}/${arch}`);
}

export function filesFor(target) {
  return target.endsWith("-windows-msvc")
    ? ["westerm.exe", "conpty.dll", "OpenConsole.exe", "conpty-LICENSE.txt"]
    : ["westerm"];
}
