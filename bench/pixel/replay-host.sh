#!/bin/sh
# Replays recorded PTY output through the host's pixel code and prints one
# JSON line per recording and codec. Usage: replay-host.sh <capture.bin>...
set -eu
cd "$(dirname "$0")/../.."
for file in "$@"; do
  WESTERM_PIXEL_REPLAY="$file" cargo test --release --quiet --manifest-path native/Cargo.toml --bin westerm \
    replay_a_recording -- --ignored --nocapture 2>/dev/null | grep '^{'
done
