#!/bin/sh
# Packs westerm.app into a disk image that installs by drag and drop. When
# the image opens, its window shows the app, an arrow, and a link to the
# Applications folder.
#
#   sh macos/build-dmg.sh                    # dist-app/westerm.app → dist-app/westerm-macos-app.dmg
#   sh macos/build-dmg.sh <app> <out.dmg>
#
# macos/build-app.sh runs this. Finder arranges the window, so this needs a
# logged-in session that may control Finder. Without it the image still has
# the app and the Applications link, in an unarranged window.
set -eu
cd "$(dirname "$0")/.."

app="${1:-dist-app/westerm.app}"
dmg="${2:-dist-app/westerm-macos-app.dmg}"
volume="westerm"

if [ ! -d "$app" ]; then
  echo "no app at $app: run macos/build-app.sh first" >&2
  exit 1
fi

work="$(mktemp -d)"
device=""
cleanup() {
  if [ -n "$device" ]; then hdiutil detach "$device" -force -quiet 2>/dev/null || true; fi
  rm -rf "$work"
}
trap cleanup EXIT

say() { printf '> %s\n' "$*"; }

say "disk image: contents"
stage="$work/stage"
mkdir -p "$stage/.background"
ditto "$app" "$stage/westerm.app"
ln -s /Applications "$stage/Applications"
swift macos/make-dmg-background.swift "$work/background.png" "$work/background@2x.png"
# One TIFF with both sizes: Finder picks the sharp one on Retina screens.
tiffutil -cathidpicheck "$work/background.png" "$work/background@2x.png" \
  -out "$stage/.background/background.tiff" 2>/dev/null

# A writable image first, with room for the .DS_Store that Finder writes.
size=$(( $(du -sm "$stage" | cut -f1) + 20 ))
hdiutil create -quiet -ov -volname "$volume" -srcfolder "$stage" -fs HFS+ -format UDRW \
  -size "${size}m" "$work/rw.dmg"

# "hdiutil attach" prints one tab-separated /dev line per partition: the
# whole disk first, then the volume with its mount point.
attached="$(hdiutil attach -readwrite -noverify -noautoopen "$work/rw.dmg" 2>/dev/null | grep '^/dev/')"
device="$(printf '%s\n' "$attached" | awk 'NR == 1 { print $1 }')"
mount="$(printf '%s\n' "$attached" | awk -F '\t' '$2 ~ /^Apple_HFS/ { print $NF }')"
if [ -z "$device" ] || [ ! -d "$mount" ]; then
  echo "cannot mount $work/rw.dmg" >&2
  exit 1
fi

say "disk image: window layout"
# The positions are the icon centers; macos/make-dmg-background.swift draws
# the arrow between them. The bounds are the 640 × 400 background plus the
# title bar.
if osascript - "$mount" >/dev/null <<'EOF'
on run argv
	set mountPath to item 1 of argv
	tell application "Finder"
		set theDisk to disk (POSIX file mountPath as alias)
		open theDisk
		set theWindow to container window of theDisk
		set current view of theWindow to icon view
		set toolbar visible of theWindow to false
		set statusbar visible of theWindow to false
		set pathbar visible of theWindow to false
		set bounds of theWindow to {200, 120, 840, 548}
		set viewOptions to icon view options of theWindow
		set arrangement of viewOptions to not arranged
		set icon size of viewOptions to 128
		set text size of viewOptions to 13
		set background picture of viewOptions to file ".background:background.tiff" of theDisk
		set position of item "westerm.app" of theDisk to {160, 190}
		set position of item "Applications" of theDisk to {480, 190}
		-- Closing and opening again makes Finder write .DS_Store.
		close theWindow
		open theDisk
		update theDisk without registering applications
		delay 1
		close container window of theDisk
	end tell
end run
EOF
then
  # Finder writes .DS_Store a moment later.
  tries=0
  while [ ! -f "$mount/.DS_Store" ] && [ "$tries" -lt 20 ]; do
    sleep 0.5
    tries=$((tries + 1))
  done
  [ -f "$mount/.DS_Store" ] || echo "warning: Finder did not save the window layout" >&2
else
  echo "warning: Finder could not arrange the window; the image works, unarranged" >&2
fi

# The volume shows the westerm icon in Finder and on the desktop. This comes
# after the layout: Finder removes a volume icon it finds while it works.
# (Nothing touches the app's Finder flags: codesign rejects them.)
if [ -f "$app/Contents/Resources/AppIcon.icns" ] && command -v SetFile >/dev/null; then
  cp "$app/Contents/Resources/AppIcon.icns" "$mount/.VolumeIcon.icns"
  SetFile -a C "$mount"
fi

rm -rf "$mount/.fseventsd" "$mount/.Trashes"
chmod -Rf go-w "$mount" || true
sync

# Finder can hold the volume for a moment after closing its window.
tries=0
until hdiutil detach "$device" -quiet 2>/dev/null; do
  tries=$((tries + 1))
  if [ "$tries" -ge 5 ]; then
    echo "cannot detach $device" >&2
    exit 1
  fi
  sleep 2
done
device=""

say "disk image: compress"
mkdir -p "$(dirname "$dmg")"
rm -f "$dmg"
hdiutil convert "$work/rw.dmg" -quiet -format UDZO -imagekey zlib-level=9 -o "$dmg"
say "done: $dmg"
