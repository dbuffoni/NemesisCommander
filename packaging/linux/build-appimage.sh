#!/usr/bin/env bash
# Packages NemesisCommander as an AppImage.
#
#   packaging/linux/build-appimage.sh <directory with nmc and nmc-gui> <version>
#
# Produces NemesisCommander-<version>-<arch>.AppImage in the current directory.
# The AppImage opens the window; `NemesisCommander.AppImage nmc ...` runs the
# `nmc` program (terminal interface and commands), see `nemesiscommander`.
#
# Qt is bundled by linuxdeploy's Qt plugin, which uses the qmake given in
# $QMAKE (the Qt nmc-gui was built with). linuxdeploy and the plugin are
# downloaded on first use, or taken from $LINUXDEPLOY and $LINUXDEPLOY_QT.
set -euo pipefail

bin_dir=${1:?usage: build-appimage.sh <bin dir> <version>}
version=${2:?usage: build-appimage.sh <bin dir> <version>}
here=$(cd "$(dirname "$0")" && pwd)
arch=$(uname -m)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

appdir="$work/AppDir"
install -Dm755 "$bin_dir/nmc" "$appdir/usr/bin/nmc"
install -Dm755 "$bin_dir/nmc-gui" "$appdir/usr/bin/nmc-gui"
install -Dm755 "$here/nemesiscommander" "$appdir/usr/bin/nemesiscommander"
install -Dm644 "$here/nemesiscommander.desktop" "$appdir/usr/share/applications/nemesiscommander.desktop"
# The icon, from assets/branding: the SVG and every PNG size.
brand="$here/../../assets/branding"
install -Dm644 "$brand/nemesiscommander.svg" "$appdir/usr/share/icons/hicolor/scalable/apps/nemesiscommander.svg"
for size in 16 24 32 48 64 128 256 512; do
    install -Dm644 "$brand/nemesiscommander-$size.png" \
        "$appdir/usr/share/icons/hicolor/${size}x${size}/apps/nemesiscommander.png"
done

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
    --executable "$appdir/usr/bin/nmc" \
    --executable "$appdir/usr/bin/nmc-gui" \
    --plugin qt \
    --desktop-file "$appdir/usr/share/applications/nemesiscommander.desktop" \
    --icon-file "$appdir/usr/share/icons/hicolor/512x512/apps/nemesiscommander.png" \
    --output appimage

echo "built NemesisCommander-$version-$arch.AppImage"
