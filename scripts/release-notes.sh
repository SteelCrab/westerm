#!/bin/sh
# Prints the CHANGELOG.md section of one version, for the GitHub release
# notes. Fails when the section is missing or empty.
#
#   sh scripts/release-notes.sh 0.1.0
set -eu
version="${1:?usage: release-notes.sh <version>}"
changelog="$(dirname "$0")/../CHANGELOG.md"

notes="$(awk -v head="## [$version]" '
  index($0, head) == 1 { on = 1; next }
  on && /^## \[/ { exit }
  on && /^\[[^]]*\]: / { next }
  on { print }
' "$changelog")"

# Trim blank lines around the section.
notes="$(printf '%s\n' "$notes" | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}')"
if [ -z "$notes" ]; then
  echo "CHANGELOG.md has no notes for $version" >&2
  exit 1
fi
printf '%s\n' "$notes"
