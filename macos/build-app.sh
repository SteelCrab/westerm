#!/bin/sh
# Builds westerm.app for macOS: the Swift window (macos/app), the westerm
# binary, and the icon. Universal (Apple silicon and Intel) by default.
#
#   sh macos/build-app.sh                           # dist-app/westerm.app, westerm-macos-app.zip and .dmg
#   WESTERM_APP_ARCHS=arm64 sh macos/build-app.sh   # one architecture, faster
#
# Needs Xcode or its command line tools, Node.js, and the Rust targets
# (rustup target add aarch64-apple-darwin x86_64-apple-darwin).
set -eu
cd "$(dirname "$0")/.."

archs="${WESTERM_APP_ARCHS:-arm64 x86_64}"
version="$(node -p "require('./package.json').version")"
out="dist-app"
app="$out/westerm.app"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

say() { printf '> %s\n' "$*"; }

say "web UI"
npx vite build >/dev/null

binaries=""
launchers=""
for arch in $archs; do
  case "$arch" in
    arm64) target=aarch64-apple-darwin ;;
    x86_64) target=x86_64-apple-darwin ;;
    *) echo "unknown architecture: $arch" >&2; exit 2 ;;
  esac
  say "westerm for $target"
  cargo build --release --locked --target "$target" --manifest-path native/Cargo.toml
  binaries="$binaries native/target/$target/release/westerm"
  say "app window for $arch"
  swiftc -O -swift-version 5 -target "$arch-apple-macos12.0" macos/app/*.swift -o "$work/westerm-app-$arch"
  launchers="$launchers $work/westerm-app-$arch"
done

say "bundle"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
# shellcheck disable=SC2086 # word splitting is intended: one path per architecture
lipo -create $binaries -output "$app/Contents/MacOS/westerm"
# shellcheck disable=SC2086
lipo -create $launchers -output "$app/Contents/MacOS/westerm-app"
sed "s/@VERSION@/$version/g" macos/Info.plist > "$app/Contents/Info.plist"
cp LICENSE "$app/Contents/Resources/LICENSE"

say "icon"
iconset="$work/AppIcon.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" macos/AppIcon.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" macos/AppIcon.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"

# Ad hoc signatures: Apple silicon runs only signed code. The app is not
# notarized, so a downloaded copy needs one approval (see README).
say "sign"
codesign --force --sign - "$app/Contents/MacOS/westerm"
codesign --force --sign - "$app"

# The zip is for the app's own updater; the disk image is for people, who
# install by dragging the app to Applications.
say "zip"
rm -f "$out/westerm-macos-app.zip"
ditto -c -k --keepParent "$app" "$out/westerm-macos-app.zip"
sh macos/build-dmg.sh "$app" "$out/westerm-macos-app.dmg"
say "done: $app ($(lipo -archs "$app/Contents/MacOS/westerm-app"))"
