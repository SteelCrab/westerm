#!/bin/sh
# Install or update westerm from GitHub Releases (macOS and Linux).
#
#   curl -fsSL https://raw.githubusercontent.com/SteelCrab/westerm/main/install.sh | sh
#
# WESTERM_VERSION       release tag to install, e.g. v0.1.0 (default: latest)
# WESTERM_INSTALL_DIR   folder for the binary (default: ~/.local/bin)
# WESTERM_RELEASES_URL  another copy of the releases page (mirrors, tests)
set -eu

repo="SteelCrab/westerm"
version="${WESTERM_VERSION:-latest}"
install_dir="${WESTERM_INSTALL_DIR:-$HOME/.local/bin}"
releases="${WESTERM_RELEASES_URL:-https://github.com/$repo/releases}"

say() { printf 'westerm: %s\n' "$*"; }
fail() { printf 'westerm: %s\n' "$*" >&2; exit 1; }

os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Darwin)
    # A shell under Rosetta reports x86_64 on Apple silicon; use the native build.
    if [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
      arch=arm64
    fi
    case "$arch" in
      arm64) target=aarch64-apple-darwin ;;
      x86_64) target=x86_64-apple-darwin ;;
      *) fail "no build for macOS on $arch" ;;
    esac
    ;;
  Linux)
    case "$arch" in
      x86_64 | amd64) target=x86_64-unknown-linux-musl ;;
      aarch64 | arm64) target=aarch64-unknown-linux-musl ;;
      *) fail "no build for Linux on $arch; build from source: https://github.com/$repo#build-from-source" ;;
    esac
    ;;
  *) fail "this script is for macOS and Linux. On Windows run install.ps1: https://github.com/$repo#install-a-release" ;;
esac

asset="westerm-$target.tar.gz"
if [ "$version" = latest ]; then
  url="$releases/latest/download/$asset"
else
  url="$releases/download/$version/$asset"
fi

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -q -O "$2" "$1"; }
else
  fail "curl or wget is required"
fi
if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d ' ' -f 1; }
else
  sha256() { shasum -a 256 "$1" | cut -d ' ' -f 1; }
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
trap 'exit 1' INT TERM

say "downloading $url"
fetch "$url" "$tmp/$asset" || fail "download failed: $url"
fetch "$url.sha256" "$tmp/$asset.sha256" || fail "download failed: $url.sha256"
expected="$(cut -d ' ' -f 1 < "$tmp/$asset.sha256")"
[ -n "$expected" ] && [ "$expected" = "$(sha256 "$tmp/$asset")" ] || fail "checksum mismatch for $asset"

tar -xzf "$tmp/$asset" -C "$tmp"
binary="$tmp/westerm-$target/westerm"
[ -f "$binary" ] || fail "the archive has no westerm binary"

mkdir -p "$install_dir"
dest="$install_dir/westerm"
updating=false
[ -e "$dest" ] && updating=true
# Replace by rename: a running westerm keeps its old file and is not disturbed.
cp "$binary" "$install_dir/.westerm.$$"
chmod 755 "$install_dir/.westerm.$$"
mv -f "$install_dir/.westerm.$$" "$dest"

say "installed $("$dest" --version)"
say "location: $dest"
if [ "$updating" = true ]; then
  say "if westerm is running, restart it. Open shells may come back once as \"restored\"."
fi
case ":$PATH:" in
  *":$install_dir:"*) say "run: westerm   then open http://127.0.0.1:4317" ;;
  *) say "add the folder to PATH, for example: echo 'export PATH=\"$install_dir:\$PATH\"' >> ~/.zshrc" ;;
esac
