# Pixel app benchmarks

Measures terminal-browser in westerm: what crosses the PTY, what the host
does, and what the page's main thread pays. See "Pixel updates" in
`docs/architecture.md` for the design.

## Pages

`pages/` holds three test pages. Serve them over HTTP, since a browser pane
only opens http(s) URLs:

```bash
python3 -m http.server 8791 --bind 127.0.0.1 --directory bench/pixel/pages
```

- `blink.html`: a 2 × 20 px bar blinks twice a second (a caret).
- `scroll.html`: text scrolls 4 px every animation frame.
- `video.html`: every pixel changes every frame.

## PTY output (`capture.py`)

Runs `terminal-browser open` in a PTY the way westerm does and records its
output. `responder.mjs` answers terminal queries with the same Ghostty WASM
core as the pane and reports how long it spent parsing. `--shm` also accepts
shared-memory frames, as the host does.

```bash
python3 bench/pixel/capture.py http://127.0.0.1:8791/video.html out/video-inline
python3 bench/pixel/capture.py http://127.0.0.1:8791/video.html out/video-shm --shm
```

`replay-host.sh out/*.bin` runs a recording through the host's pixel code
(`replay_a_recording` in `native/src/pixels/mod.rs`) and prints its time.

## End to end (`e2e-layout.py`, `cpu.py`, `pages/measure.js`)

1. Run a dashboard with its own state folder. The path must be short: a Unix
   socket path has at most 104 bytes on macOS.

   ```bash
   WESTERM_STATE_DIR=native/target/bs-b native/target/release/westerm serve --port 4400
   ```

2. Leave the dashboard page (it saves its layout when it closes), then write a
   layout with one browser pane: `e2e-layout.py native/target/bs-b localhost:8791/scroll.html s1`.
3. Open `http://127.0.0.1:4400/`, and at once start
   `cpu.py 10 <westerm binary> out.json 9` and load `measure.js` into the page
   (as a `<script>`). Both wait out the warm-up, then measure 10 seconds.
4. Read `window.__benchResult`. `pages/count-paints.js` instead counts wterm's
   `putImageData` calls.

`localStorage["westerm.pixels"] = "off"` in the dashboard page measures the
Kitty image path in the same build.

## Results (2026-11-07)

MacBook (Apple silicon), Chrome 156, device pixel ratio 1.25, one browser pane
of 1701 × 1003 CSS px, terminal-browser 0.13.4. Ten seconds after eight of
warm-up. A and C ran twice for scroll and video; the table shows both runs.

- **A**: `origin/main` (989cb1d) with only the counters of `pane-stats.ts` added.
- **B**: this branch with `westerm.pixels=off`: Kitty images through the PTY,
  plus the image repaint patch.
- **C**: this branch: shared memory, pixel updates, Worker drawing.

### Frames that reach the page, per second

| Page | A | B | C |
|---|---|---|---|
| blink | 2 | 2 | 2 |
| scroll | 8.4 / 8.4 | 8.5 | 107.3 / 107.7 |
| video | 5.0 / 5.0 | 5.0 | 107.9 / 107.2 |

### Page main thread (video)

| | A | B | C |
|---|---|---|---|
| Long tasks (ms per second) | 217 / 159 | 148 | 0 / 0 |
| Page frames per second | 98.7 / 99.9 | 99.5 | 120.1 / 120.1 |
| Slowest frame (ms) | 51.2 / 51.6 | 51.5 | 10.4 / 10.4 |
| 10 ms timer, worst delay (ms) | 55 / 52.6 | 51.5 | 1.6 / 5.2 |

Scroll had no long tasks in any variant: there the PTY was the limit, not
the page.

### CPU (% of one core, averaged over the runs)

| Page | Process | A | C |
|---|---|---|---|
| scroll | terminal-browser | 38 | 34 |
| | westerm host | 5 | 18 |
| | Chrome renderers | 34 | 53 |
| | Chrome GPU | 8 | 20 |
| | **CPU ms per frame shown** | **102** | **13** |
| video | terminal-browser | 50 | 56 |
| | westerm host | 20 | 29 |
| | Chrome renderers | 57 | 52 |
| | Chrome GPU | 4 | 19 |
| | **CPU ms per frame shown** | **263** | **15** |

"Frame shown" is a display frame with a new picture (C: 100 to 106 a
second; A: the same as above). C shows 12 to 21 times as many frames for 1.2
to 1.5 times the CPU.

Before the socket buffer fix (`ipc::tune`), C reached 38 (scroll) and 21
(video) frames a second, and the host used 112 % and 116 %: tungstenite's
quadratic copying of 7 MB messages through an 8 KiB Unix socket.

### PTY and decoding (`capture.py`, `replay-host.sh`)

| | Inline (A, B) | Shared memory (C) |
|---|---|---|
| scroll: PTY bytes per second | 2.96 MB | 29 KB |
| scroll: frames per second at the PTY | 11.2 | 106.8 |
| video: PTY bytes per second | 6.79 MB | 90 KB |
| video: frames per second at the PTY | 5.0 | 106.8 |

The same recorded inline stream (scroll, 31.7 MB): the pane's Ghostty WASM
core parsed it in 1137 ms on the main thread; the host's Rust code parses,
decodes, and composes it in 307 ms on its own thread (video, 72.8 MB:
1694 ms → 641 ms).

### wterm image repaints (`count-paints.js`, blink, old path)

| | A | B |
|---|---|---|
| putImageData calls per second | 60 | 2 |
| Pixels painted per blink | 1,771,000 | 40 |

### Remote windows

Deflate (zlib level 1) on the same recordings: scroll updates shrink from
451 MB to 24 MB (19 times), video from 220 MB to 54 MB (4 times). Remote
windows were not measured end to end.
