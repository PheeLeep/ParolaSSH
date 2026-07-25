#!/usr/bin/env bash
# Give the development build a real icon in the taskbar / app switcher.
#
# Wayland has no protocol for a window to hand the compositor an icon: the
# compositor matches the window's app_id to an installed desktop entry and uses
# that entry's Icon=. `tauri dev` installs nothing, so the window falls back to
# a generic placeholder. This installs a user-level entry so the icon resolves.
#
# The app_id is `parolassh` (GTK falls back to the binary name because
# `enableGTKAppId` is off), so the entry must be named parolassh.desktop.
#
# Packaged builds do not need this — the deb/rpm/AppImage bundler installs its
# own entry with StartupWMClass=parolassh, which matches the same app_id.
#
# Undo with: scripts/install-dev-desktop-entry.sh --uninstall

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
apps_dir="$data_home/applications"
icons_dir="$data_home/icons/hicolor"
entry="$apps_dir/parolassh.desktop"

refresh_caches() {
  command -v update-desktop-database >/dev/null && update-desktop-database "$apps_dir" || true
  command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -qtf "$icons_dir" || true
}

if [[ "${1:-}" == "--uninstall" ]]; then
  rm -f "$entry"
  for size in 32 64 128 256 512; do
    rm -f "$icons_dir/${size}x${size}/apps/parolassh.png"
  done
  refresh_caches
  echo "Removed the development desktop entry and icons."
  exit 0
fi

# 32px uses the simplified artwork; the tower's stripes fragment below ~48px.
declare -A sources=(
  [32]="src-tauri/icons/32x32.png"
  [64]="src-tauri/icons/64x64.png"
  [128]="src-tauri/icons/128x128.png"
  [256]="src-tauri/icons/128x128@2x.png"
  [512]="src-tauri/icons/icon.png"
)

for size in "${!sources[@]}"; do
  src="$repo_root/${sources[$size]}"
  if [[ ! -f "$src" ]]; then
    echo "missing $src — run 'npm run tauri icon' first" >&2
    exit 1
  fi
  install -Dm644 "$src" "$icons_dir/${size}x${size}/apps/parolassh.png"
done

# Point Exec at whichever binary is actually built, so the entry launches.
binary="$repo_root/src-tauri/target/release/parolassh"
[[ -x "$binary" ]] || binary="$repo_root/src-tauri/target/debug/parolassh"

mkdir -p "$apps_dir"
sed "s|^Exec=.*|Exec=$binary|" "$repo_root/packaging/parolassh.desktop" > "$entry"
chmod 644 "$entry"

refresh_caches

echo "Installed $entry"
echo "Icons under $icons_dir/<size>/apps/parolassh.png"
echo "Restart the app so the compositor re-matches the window."
