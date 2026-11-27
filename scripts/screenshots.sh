#!/usr/bin/env bash
# Takes pictures of the window for the manual and for checking the layout
# at several sizes. Needs a built pnxr-gui (cargo build -p pnxr-gui) and Qt.
#
#   scripts/screenshots.sh [OUT_DIR]    (default: docs/manual/images)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$root/docs/manual/images}"
gui="${PNXR_GUI:-$root/target/debug/pnxr-gui}"
mkdir -p "$out"

# A demo home with realistic content, the same every time.
demo="$(mktemp -d)"
trap 'rm -rf "$demo"' EXIT
"$root/scripts/demo-tree.sh" "$demo"
export PNXR_CONFIG_DIR="$demo/.config" PNXR_DATA_DIR="$demo/.data" QT_QPA_PLATFORM=offscreen
export HOME="$demo/home"
# Saved connections, so the drive list shows every category.
mkdir -p "$PNXR_CONFIG_DIR"
cat > "$PNXR_CONFIG_DIR/config.toml" <<TOML
[[connections]]
name = "nas"
uri = "sftp://me@nas.lan/srv/media"

[[connections]]
name = "photos"
uri = "s3://family-photos/"

[[connections]]
name = "office"
uri = "smb://fileserver/projects"

[[connections]]
name = "site backup"
uri = "zip://$demo/home/Downloads/site-backup.zip!/"
TOML

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
shot commands 1280x800 "F9" "$left" "$right"
shot commands-category 1280x800 "F9 Right Down Down" "$left" "$right"
shot commands-find 1280x800 "F9 s e t" "$left" "$right"
# F1 explains the highlighted command.
shot commands-help 1280x800 "F9 s w a p F1" "$left" "$right"
shot copy 1280x800 "Down Insert Down Insert F5" "$left" "$right"
shot mkdir 1280x800 "F7" "$left" "$right"
shot delete 1280x800 "Down F8" "$left" "$right"
shot search 1280x800 "Ctrl+F2" "$left" "$right"
shot quick-view 1280x800 "Down Down Down Ctrl+Q" "$left" "$right"
shot viewer-text 1280x800 "End F3" "$demo/home/Projects/website" "$right"
# These show the real disks and user name of the machine: only with
# REAL_DISKS=1, never for pictures that are published.
if [ "${REAL_DISKS:-}" = 1 ]; then
  shot drives 1280x800 "Shift+F1" "$left" "$right"
  shot drives-new 1280x800 "Shift+F1 $(printf 'Down %.0s' {1..20})Enter" "$left" "$right"
fi
shot pack-advanced 1280x800 "Down Shift+F5 Ctrl+O Enter Enter s e c r e t" "$left" "$right"
shot bookmarks 1280x800 "Ctrl+Shift+D Ctrl+D" "$left" "$right"
shot jobs 1280x800 "Ctrl+J" "$left" "$right"
shot properties 1280x800 "Down Ctrl+E" "$left" "$right"
shot archive 1280x800 "" "zip://$demo/home/Downloads/site-backup.zip!/" "$right"
docs="$demo/home/Documents"
models="$demo/home/Projects/models"
photos="$demo/home/Pictures/2026"
shot viewer-markdown 1280x800 "Down F3" "$demo/home/Projects/notes" "$right"
# Documents/: .., budget.xlsx, invoice…, kickoff.pptx, offer.docx, report.pdf
shot viewer-xlsx 1280x800 "Down F3" "$docs" "$right"
shot viewer-pptx 1280x800 "Down Down Down F3" "$docs" "$right"
shot viewer-docx 1280x800 "Down Down Down Down F3" "$docs" "$right"
shot viewer-pdf 1280x800 "Down Down Down Down Down F3" "$docs" "$right"
shot viewer-pdf-page2 1280x800 "Down Down Down Down Down F3 PageDown" "$docs" "$right"
shot viewer-model 1280x800 "Down F3" "$models" "$right"
shot viewer-model-turned 1280x800 "Down F3 Right Right Down Down +" "$models" "$right"
# Pictures/2026/: .., lake.png, sunset.png
shot viewer-image 1280x800 "Down Down F3" "$photos" "$right"
shot viewer-image-zoom 1280x800 "Down Down F3 + + Left Left" "$photos" "$right"
shot quick-view-pdf 1280x800 "Down Down Down Down Down Ctrl+Q" "$docs" "$right"
shot quick-view-model 1280x800 "Down Ctrl+Q" "$models" "$right"
shot quick-view-markdown 1280x800 "Down Ctrl+Q" "$demo/home/Projects/notes" "$right"
shot settings 1280x800 "Ctrl+," "$left" "$right"
# Typing finds a setting.
shot settings-find 1280x800 "Ctrl+, f o n t" "$left" "$right"
# Synchronize: the plan of a mirror, before anything is run.
shot sync 1280x800 "F9 s y n c Enter Enter" "$left" "$right"
# The basket with two items from the left panel.
shot basket 1280x800 "Down Insert Down Insert Ctrl+G Ctrl+B" "$left" "$right"
# Copy path: the forms offered.
shot copy-path 1280x800 "Down Ctrl+P" "$left" "$right"
# Folder sizes counted in the panel, then the disk usage list.
shot folder-sizes 1280x800 "Alt+S" "$demo/home" "$right"
shot disk-usage 1280x800 "Ctrl+F7" "$demo/home" "$right"
# Disks and partitions of the machine the pictures are taken on.
[ "${REAL_DISKS:-}" = 1 ] && shot disks 1280x800 "Ctrl+F8" "$left" "$right"
# Bookmarks with the recent folders under them, filtered by typing.
shot bookmarks-filter 1280x800 "Ctrl+Shift+D Down Enter Backspace Ctrl+D w e b" "$left" "$right"
