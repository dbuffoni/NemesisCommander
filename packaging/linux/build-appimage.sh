#!/usr/bin/env bash
# Packages SuperCommander as an AppImage.
#
#   packaging/linux/build-appimage.sh <directory with sc and sc-gui> <version>
#
# Produces SuperCommander-<version>-<arch>.AppImage in the current directory.
# The AppImage opens the window; `SuperCommander.AppImage sc ...` runs the
# `sc` program (terminal interface and commands), see `supercommander`.
#
# Qt is bundled by linuxdeploy's Qt plugin, which uses the qmake given in
# $QMAKE (the Qt sc-gui was built with). linuxdeploy and the plugin are
# downloaded on first use, or taken from $LINUXDEPLOY and $LINUXDEPLOY_QT.
set -euo pipefail

bin_dir=${1:?usage: build-appimage.sh <bin dir> <version>}
version=${2:?usage: build-appimage.sh <bin dir> <version>}
here=$(cd "$(dirname "$0")" && pwd)
arch=$(uname -m)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

appdir="$work/AppDir"
install -Dm755 "$bin_dir/sc" "$appdir/usr/bin/sc"
install -Dm755 "$bin_dir/sc-gui" "$appdir/usr/bin/sc-gui"
install -Dm755 "$here/supercommander" "$appdir/usr/bin/supercommander"
install -Dm644 "$here/supercommander.desktop" "$appdir/usr/share/applications/supercommander.desktop"
install -Dm644 "$here/supercommander.svg" "$appdir/usr/share/icons/hicolor/scalable/apps/supercommander.svg"

# Downloads a linuxdeploy tool unless its variable names one already.
fetch() {
    local file="$work/$1-$arch.AppImage"
    curl -fsSL -o "$file" "https://github.com/linuxdeploy/$1/releases/download/continuous/$1-$arch.AppImage"
    chmod +x "$file"
    echo "$file"
}
linuxdeploy=${LINUXDEPLOY:-$(fetch linuxdeploy)}
# The Qt plugin is found by name next to linuxdeploy or on the PATH.
plugins="$work/plugins"
mkdir -p "$plugins"
ln -s "${LINUXDEPLOY_QT:-$(fetch linuxdeploy-plugin-qt)}" "$plugins/linuxdeploy-plugin-qt"
export PATH="$plugins:$PATH"
# The QML files tell the plugin which QML modules to bundle.
export QML_SOURCES_PATHS="$here/../../gui/qml"

# CI machines have no FUSE: let AppImages run by extracting themselves.
export APPIMAGE_EXTRACT_AND_RUN=1
export LINUXDEPLOY_OUTPUT_VERSION="$version"
"$linuxdeploy" --appdir "$appdir" \
    --executable "$appdir/usr/bin/sc" \
    --executable "$appdir/usr/bin/sc-gui" \
    --plugin qt \
    --desktop-file "$appdir/usr/share/applications/supercommander.desktop" \
    --icon-file "$appdir/usr/share/icons/hicolor/scalable/apps/supercommander.svg" \
    --output appimage

echo "built SuperCommander-$version-$arch.AppImage"
