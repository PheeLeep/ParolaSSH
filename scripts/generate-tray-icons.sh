#!/usr/bin/env bash
# Renders the three tray states from design/tray/tray-template.svg.
# The beam is the only thing that changes: absent = offline, white = online
# with no session, violet = connected. "Connecting" blinks connected/offline at
# runtime and needs no icon of its own.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
template="$root/design/tray/tray-template.svg"
out="$root/src-tauri/icons/tray"
size=64

command -v rsvg-convert >/dev/null || { echo "rsvg-convert not found (librsvg)" >&2; exit 1; }
mkdir -p "$out"

beam() { # $1 = stroke colour
  printf '<polyline points="72,16 148,58 72,100" fill="none" stroke="%s" stroke-width="28" stroke-linecap="round" stroke-linejoin="round"/>' "$1"
}

render() { # $1 = state name, $2 = beam markup
  local tmp
  tmp="$(mktemp -t "tray-$1-XXXX.svg")"
  BEAM="$2" perl -pe 's/__BEAM__/$ENV{BEAM}/' "$template" > "$tmp"
  rsvg-convert -w "$size" -h "$size" "$tmp" -o "$out/$1.png"
  rm -f "$tmp"
  echo "  $out/$1.png"
}

echo "Rendering ${size}x${size} tray icons:"
render offline   ''
render online    "$(beam '#EDEBF4')"
render connected "$(beam '#A78BFA')"
