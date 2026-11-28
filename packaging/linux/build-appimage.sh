#!/usr/bin/env bash
# Packages Pairnox as an AppImage.
#
#   packaging/linux/build-appimage.sh <directory with pnxr and pnxr-gui> <version>
#
# Produces Pairnox-<version>-<arch>.AppImage in the current directory.
# The AppImage opens the window; `Pairnox.AppImage pnxr ...` runs the
# `pnxr` program (terminal interface and commands), see `pairnox`.
#
# Qt is bundled by linuxdeploy's Qt plugin, which uses the qmake given in
# $QMAKE (the Qt pnxr-gui was built with). linuxdeploy and the plugin are
# downloaded on first use, or taken from $LINUXDEPLOY and $LINUXDEPLOY_QT.
set -euo pipefail

bin_dir=${1:?usage: build-appimage.sh <bin dir> <version>}
version=${2:?usage: build-appimage.sh <bin dir> <version>}
here=$(cd "$(dirname "$0")" && pwd)
arch=$(uname -m)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

appdir="$work/AppDir"
install -Dm755 "$bin_dir/pnxr" "$appdir/usr/bin/pnxr"
install -Dm755 "$bin_dir/pnxr-gui" "$appdir/usr/bin/pnxr-gui"
install -Dm755 "$here/pairnox" "$appdir/usr/bin/pairnox"
install -Dm644 "$here/pairnox.desktop" "$appdir/usr/share/applications/pairnox.desktop"
# The icon, from assets/branding: the SVG and every PNG size.
brand="$here/../../assets/branding"
install -Dm644 "$brand/pairnox.svg" "$appdir/usr/share/icons/hicolor/scalable/apps/pairnox.svg"
install -Dm644 "$brand/pairnox-symbolic.svg" \
    "$appdir/usr/share/icons/hicolor/symbolic/apps/pairnox-symbolic.svg"
for size in 16 24 32 48 64 128 256 512; do
    install -Dm644 "$brand/pairnox-$size.png" \
        "$appdir/usr/share/icons/hicolor/${size}x${size}/apps/pairnox.png"
done

# Licence and third-party notices (Qt, libarchive, crates).
install -Dm644 "$here/../../LICENSE" "$appdir/usr/share/doc/pairnox/LICENSE"
python3 "$here/../../scripts/third_party.py" -o "$appdir/usr/share/doc/pairnox/THIRD-PARTY-NOTICES.md"

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

# Native Wayland: the platform plugins, and the window decorations,
# graphics and shell integrations they load (the Qt plugin bundles only the
# X11 platform otherwise). XDG desktop portal theme plugin: dark mode on GNOME.
export EXTRA_PLATFORM_PLUGINS="${EXTRA_PLATFORM_PLUGINS:-libqwayland-egl.so;libqwayland-generic.so}"
export EXTRA_QT_PLUGINS="${EXTRA_QT_PLUGINS:-waylandcompositor;wayland-decoration-client;wayland-graphics-integration-client;wayland-shell-integration;platformthemes;xcbglintegrations}"

# CI machines have no FUSE: let AppImages run by extracting themselves.
export APPIMAGE_EXTRACT_AND_RUN=1
export LINUXDEPLOY_OUTPUT_VERSION="$version"
"$linuxdeploy" --appdir "$appdir" \
    --executable "$appdir/usr/bin/pnxr" \
    --executable "$appdir/usr/bin/pnxr-gui" \
    --plugin qt \
    --desktop-file "$appdir/usr/share/applications/pairnox.desktop" \
    --icon-file "$appdir/usr/share/icons/hicolor/512x512/apps/pairnox.png" \
    --output appimage

echo "built Pairnox-$version-$arch.AppImage"
