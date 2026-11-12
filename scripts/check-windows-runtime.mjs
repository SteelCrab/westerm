// Checks that a Windows executable starts on a clean Windows.
//
//   node scripts/check-windows-runtime.mjs <file.exe> [more files]
//
// A clean Windows has no Visual C++ runtime (VCRUNTIME140.dll, MSVCP140.dll).
// An executable that imports one stops with a "DLL was not found" dialog
// before its first line runs. The check reads the import tables of each file
// and fails when one of those DLLs is there. The Universal CRT
// (api-ms-win-crt-*.dll) is part of Windows 10 and later, so it may stay.
import { readFileSync } from "node:fs";

/** The DLLs that come with the Visual C++ Redistributable, not with Windows. */
const NOT_IN_WINDOWS = /^(vcruntime|msvcp|concrt|vcomp|vccorlib)\d/i;

/** The names of the DLLs a PE file imports (normal and delay-loaded). */
function importedDlls(bytes) {
  const pe = bytes.readUInt32LE(0x3c);
  if (bytes.toString("latin1", 0, 2) !== "MZ" || bytes.toString("latin1", pe, pe + 4) !== "PE\0\0") {
    throw new Error("not a Windows executable");
  }
  const sectionCount = bytes.readUInt16LE(pe + 6);
  const optional = pe + 24;
  const magic = bytes.readUInt16LE(optional);
  if (magic !== 0x10b && magic !== 0x20b) throw new Error(`unknown optional header 0x${magic.toString(16)}`);
  // The data directories follow the fixed part: 96 bytes in PE32, 112 in PE32+.
  const directories = optional + (magic === 0x20b ? 112 : 96);
  const directoryCount = bytes.readUInt32LE(directories - 4);
  const sections = optional + bytes.readUInt16LE(pe + 20);

  /** Where a relative virtual address is in the file. */
  const offsetOf = (rva) => {
    for (let at = 0; at < sectionCount; at++) {
      const section = sections + at * 40;
      const start = bytes.readUInt32LE(section + 12);
      const size = Math.max(bytes.readUInt32LE(section + 8), bytes.readUInt32LE(section + 16));
      if (rva >= start && rva < start + size) return rva - start + bytes.readUInt32LE(section + 20);
    }
    throw new Error(`address 0x${rva.toString(16)} is in no section`);
  };
  const textAt = (rva) => {
    const start = offsetOf(rva);
    return bytes.toString("latin1", start, bytes.indexOf(0, start));
  };

  const names = [];
  // [directory index, entry size, where the DLL name's address is in an entry]
  for (const [index, entrySize, nameAt] of [[1, 20, 12], [13, 32, 4]]) {
    if (index >= directoryCount) continue;
    const table = bytes.readUInt32LE(directories + index * 8);
    if (table === 0) continue;
    // The table ends with an entry that has no name.
    for (let entry = offsetOf(table); bytes.readUInt32LE(entry + nameAt) !== 0; entry += entrySize) {
      names.push(textAt(bytes.readUInt32LE(entry + nameAt)));
    }
  }
  return names;
}

const files = process.argv.slice(2);
if (files.length === 0) {
  console.error("usage: node scripts/check-windows-runtime.mjs <file.exe> [more files]");
  process.exit(2);
}
let failed = false;
for (const file of files) {
  const missing = importedDlls(readFileSync(file)).filter((name) => NOT_IN_WINDOWS.test(name));
  if (missing.length === 0) {
    console.log(`ok: ${file} needs no Visual C++ runtime`);
    continue;
  }
  failed = true;
  console.error(`error: ${file} imports ${missing.join(", ")}, which a clean Windows does not have`);
}
if (failed) {
  console.error("Link the Visual C++ runtime into the executable (native/build.rs: static_vcruntime).");
  process.exit(1);
}
