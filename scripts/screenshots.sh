#!/usr/bin/env bash
# Takes pictures of the window for the manual and for checking the layout
# at several sizes. Needs a built sc-gui (cargo build -p sc-gui) and Qt.
#
#   scripts/screenshots.sh [OUT_DIR]    (default: docs/manual/images)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$root/docs/manual/images}"
gui="${SC_GUI:-$root/target/debug/sc-gui}"
mkdir -p "$out"

# A demo home with realistic content, the same every time.
demo="$(mktemp -d)"
trap 'rm -rf "$demo"' EXIT
"$root/scripts/demo-tree.sh" "$demo"
export SC_CONFIG_DIR="$demo/.config" SC_DATA_DIR="$demo/.data" QT_QPA_PLATFORM=offscreen
export HOME="$demo/home"

shot() { # NAME SIZE KEYS [LEFT] [RIGHT]
  local name="$1" size="$2" keys="$3"
  shift 3
  "$gui" --self-test --size="$size" --keys="$keys" --screenshot="$out/$name.png" "$@" >/dev/null
  echo "$out/$name.png"
}

left="$demo/home/Projects"
right="$demo/home/Pictures"
for size in 800x500 1024x640 1280x800 1920x1080; do
  shot "panels-$size" "$size" "Down Down" "$left" "$right"
done
shot help 1280x800 "F1" "$left" "$right"
shot menu 1280x800 "F9 Right" "$left" "$right"
shot copy 1280x800 "Down Insert Down Insert F5" "$left" "$right"
shot mkdir 1280x800 "F7" "$left" "$right"
shot delete 1280x800 "Down F8" "$left" "$right"
shot search 1280x800 "Alt+F7" "$left" "$right"
shot quick-view 1280x800 "Down Down Down Ctrl+Q" "$left" "$right"
shot viewer-text 1280x800 "End F3" "$demo/home/Projects/website" "$right"
shot bookmarks 1280x800 "Ctrl+Shift+D Ctrl+D" "$left" "$right"
shot jobs 1280x800 "Ctrl+J" "$left" "$right"
shot properties 1280x800 "Down Alt+Enter" "$left" "$right"
shot archive 1280x800 "" "zip://$demo/home/Downloads/site-backup.zip!/" "$right"
