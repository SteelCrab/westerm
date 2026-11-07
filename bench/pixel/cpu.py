#!/usr/bin/env python3
"""CPU seconds used over a window by terminal-browser and by a westerm host.

Usage: cpu.py <seconds> <host binary path> <out.json> [warm-up seconds]
terminal-browser is one shared daemon with Electron helpers: every process
whose command line names terminal-browser counts.
"""

import json
import subprocess
import sys
import time


def cpu_seconds(pids):
    if not pids:
        return 0.0
    out = subprocess.run(["ps", "-o", "time=", "-p", ",".join(map(str, pids))], capture_output=True, text=True).stdout
    total = 0.0
    for line in out.split():
        seconds = 0.0
        for part in line.replace("-", ":").split(":"):
            seconds = seconds * 60 + float(part)
        total += seconds
    return total


def pids(pattern):
    out = subprocess.run(["pgrep", "-f", pattern], capture_output=True, text=True).stdout
    return [int(pid) for pid in out.split()]


seconds, host_binary, out_path = float(sys.argv[1]), sys.argv[2], sys.argv[3]
# Optional warm-up, to line up with measure.js.
time.sleep(float(sys.argv[4]) if len(sys.argv) > 4 else 0)
groups = {
    "terminalBrowser": pids("terminal-browser"),
    "westermHost": pids(f"{host_binary} host"),
    # Every Chrome renderer: the bench tab is the busy one (main thread and
    # the pixel worker both count here).
    "chromeRenderers": pids("Google Chrome Helper.*--type=renderer"),
    "chromeGpu": pids("Google Chrome Helper.*--type=gpu-process"),
}
before = {name: cpu_seconds(group) for name, group in groups.items()}
time.sleep(seconds)
after = {name: cpu_seconds(group) for name, group in groups.items()}
result = {f"{name}CpuPercent": round((after[name] - before[name]) / seconds * 100, 1) for name in groups}
with open(out_path, "w") as out:
    json.dump(result, out)
print(json.dumps(result))
