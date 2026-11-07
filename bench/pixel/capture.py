#!/usr/bin/env python3
"""Run terminal-browser in a PTY the way westerm does and record its output.

Queries are answered by responder.mjs, which runs the same ghostty-vt core as
the westerm browser pane. With --shm, Kitty shared-memory frames are also
accepted: the probe gets "OK", and every frame's segment is read and
unlinked, as a terminal must.

Writes <out>.bin (raw PTY output) and <out>.json (summary).
"""

import argparse
import base64
import fcntl
import json
import os
import re
import selectors
import signal
import struct
import subprocess
import sys
import termios
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
FOREIGN = re.compile(r"^(TERM_PROGRAM.*|TERM_SESSION_ID|TMUX.*|STY|ZELLIJ.*|PIXEL_.*|TERMINAL_BROWSER_.*|"
                     r"GHOSTTY_.*|KITTY_.*|ITERM_.*|WEZTERM_.*|VSCODE_.*|TERM|COLORTERM)$")
APC = re.compile(rb"\x1b_G([^;\x1b]*)(?:;([^\x1b]*))?\x1b\\")


def cpu_seconds(pid):
    """User + system CPU time of one process, from ps."""
    out = subprocess.run(["ps", "-o", "time=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
    if not out:
        return 0.0
    parts = out.replace("-", ":").split(":")
    seconds = 0.0
    for part in parts:
        seconds = seconds * 60 + float(part)
    return seconds


def browser_pids():
    """The shared terminal-browser daemon and its Electron helpers."""
    out = subprocess.run(["pgrep", "-f", "terminal-browser"], capture_output=True, text=True).stdout
    return [int(pid) for pid in out.split()]


class ShmFrames:
    """Accepts t=s frames: replies OK to the probe, reads and unlinks segments."""

    def __init__(self):
        import _posixshmem  # noqa: PLC0415 (CPython's POSIX shm binding)
        import mmap  # noqa: PLC0415

        self.posix = _posixshmem
        self.mmap = mmap
        self.buffer = b""
        self.read_bytes = 0
        self.segments = 0

    def feed(self, data):
        """Returns replies for the PTY input."""
        self.buffer = (self.buffer + data)[-(1 << 20):]
        replies = b""
        last = 0
        for match in APC.finditer(self.buffer):
            last = match.end()
            keys = dict(kv.split(b"=", 1) for kv in match.group(1).split(b",") if b"=" in kv)
            if keys.get(b"t") != b"s":
                continue
            name = base64.b64decode(match.group(2) or b"").decode()
            if keys.get(b"a") == b"q":
                replies += b"\x1b_Gi=" + keys.get(b"i", b"0") + b";OK\x1b\\"
            try:
                fd = self.posix.shm_open(name, os.O_RDONLY, 0)
                size = os.fstat(fd).st_size
                if size:
                    with self.mmap.mmap(fd, size, prot=self.mmap.PROT_READ) as view:
                        self.read_bytes += len(view[:])
                os.close(fd)
                self.posix.shm_unlink(name)
                self.segments += 1
            except OSError:
                pass
        self.buffer = self.buffer[last:]
        return replies


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("url")
    parser.add_argument("out")
    parser.add_argument("--seconds", type=float, default=10)
    parser.add_argument("--warmup", type=float, default=4)
    parser.add_argument("--cols", type=int, default=150)
    parser.add_argument("--rows", type=int, default=43)
    parser.add_argument("--width", type=int, default=1350)
    parser.add_argument("--height", type=int, default=780)
    parser.add_argument("--env", action="append", default=[], help="KEY=VALUE for terminal-browser")
    parser.add_argument("--shm", action="store_true", help="accept Kitty shared-memory frames")
    args = parser.parse_args()

    env = {k: v for k, v in os.environ.items() if not FOREIGN.match(k)}
    env.update(TERM="xterm-256color", COLORTERM="truecolor", TERM_PROGRAM="westerm",
               TERMINAL_BROWSER_PRESENT="patched")
    for pair in args.env:
        key, value = pair.split("=", 1)
        env[key] = value

    master, slave = os.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", args.rows, args.cols, args.width, args.height))

    def controlling_tty():
        fcntl.ioctl(0, termios.TIOCSCTTY, 0)

    browser = subprocess.Popen(["terminal-browser", "open", "--no-merge", args.url], stdin=slave, stdout=slave,
                               stderr=slave, env=env, start_new_session=True, preexec_fn=controlling_tty)
    os.close(slave)
    responder = subprocess.Popen(["node", str(HERE / "responder.mjs"), str(args.cols), str(args.rows),
                                  str(96 * 1024 * 1024)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE)
    shm = ShmFrames() if args.shm else None

    selector = selectors.DefaultSelector()
    selector.register(master, selectors.EVENT_READ, "pty")
    selector.register(responder.stdout, selectors.EVENT_READ, "reply")

    out = open(args.out + ".bin", "wb")
    start = time.monotonic()
    window_start = start + args.warmup
    window_end = window_start + args.seconds
    window_bytes = 0
    window_offset = None
    cpu_before = None
    pids = []
    while time.monotonic() < window_end:
        now = time.monotonic()
        if cpu_before is None and now >= window_start:
            pids = browser_pids()
            cpu_before = sum(cpu_seconds(pid) for pid in pids)
            window_offset = out.tell()
        for key, _ in selector.select(timeout=0.05):
            if key.data == "pty":
                try:
                    data = os.read(master, 1 << 16)
                except OSError:
                    data = b""
                if not data:
                    window_end = 0
                    break
                out.write(data)
                if cpu_before is not None:
                    window_bytes += len(data)
                responder.stdin.write(data)
                responder.stdin.flush()
                if shm:
                    reply = shm.feed(data)
                    if reply:
                        os.write(master, reply)
            else:
                reply = os.read(responder.stdout.fileno(), 1 << 16)
                if reply:
                    os.write(master, reply)
    cpu_after = sum(cpu_seconds(pid) for pid in pids)
    window_stop = out.tell()
    # Closing the PTY hangs it up; a writer blocked on a full PTY wakes too.
    selector.close()
    os.close(master)
    browser.send_signal(signal.SIGTERM)
    try:
        browser.wait(timeout=5)
    except subprocess.TimeoutExpired:
        browser.kill()
        browser.wait()
    responder.stdin.close()
    parse = json.loads(responder.stderr.readline() or "{}")
    responder.wait()
    out.close()

    data = Path(args.out + ".bin").read_bytes()
    window = data[window_offset or 0:window_stop]
    commands = [dict(kv.split(b"=", 1) for kv in m.group(1).split(b",") if b"=" in kv) for m in APC.finditer(window)]
    # The first chunk of a transmission carries a=; later chunks only m=.
    images = [c for c in commands if c.get(b"a") in (b"T", b"t")]
    frames = window.count(b"\x1b[?2026l")
    summary = {
        "url": args.url,
        "env": args.env,
        "seconds": args.seconds,
        "ptyBytes": window_bytes,
        "ptyBytesPerSecond": round(window_bytes / args.seconds),
        "framesPerSecond": round(frames / args.seconds, 1),
        "apcCommands": len(commands),
        "imagesSent": len(images),
        "imagesPerSecond": round(len(images) / args.seconds, 1),
        "browserCpuSeconds": round(cpu_after - (cpu_before or 0), 2),
        "windowOffset": window_offset,
        "windowStop": window_stop,
        "ghosttyParseMs": parse.get("parseMs"),
        "shmSegments": shm.segments if shm else 0,
        "shmBytes": shm.read_bytes if shm else 0,
    }
    Path(args.out + ".json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary))


if __name__ == "__main__":
    sys.exit(main())
