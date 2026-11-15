#!/usr/bin/env bash
# Packages SuperCommander as an AppImage.
#
#   packaging/linux/build-appimage.sh <directory with the sc binary> <version>
#
# Produces SuperCommander-<version>-<arch>.AppImage in the current directory.
# linuxdeploy is downloaded on first use (or taken from $LINUXDEPLOY).
# The desktop GUI will be added to the same AppImage once it exists; today
# the AppImage starts the terminal interface.
set -euo pipefail

bin_dir=${1:?usage: build-appimage.sh <bin dir> <version>}
version=${2:?usage: build-appimage.sh <bin dir> <version>}
here=$(cd "$(dirname "$0")" && pwd)
arch=$(uname -m)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

appdir="$work/AppDir"
install -Dm755 "$bin_dir/sc" "$appdir/usr/bin/sc"
install -Dm644 "$here/supercommander.desktop" "$appdir/usr/share/applications/supercommander.desktop"
install -Dm644 "$here/supercommander.svg" "$appdir/usr/share/icons/hicolor/scalable/apps/supercommander.svg"

linuxdeploy=${LINUXDEPLOY:-}
if [[ -z "$linuxdeploy" ]]; then
    linuxdeploy="$work/linuxdeploy-$arch.AppImage"
    curl -fsSL -o "$linuxdeploy" \
        "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$arch.AppImage"
    chmod +x "$linuxdeploy"
fi

# CI machines have no FUSE: let AppImages run by extracting themselves.
export APPIMAGE_EXTRACT_AND_RUN=1
export LINUXDEPLOY_OUTPUT_VERSION="$version"
"$linuxdeploy" --appdir "$appdir" \
    --desktop-file "$appdir/usr/share/applications/supercommander.desktop" \
    --icon-file "$appdir/usr/share/icons/hicolor/scalable/apps/supercommander.svg" \
    --output appimage

echo "built SuperCommander-$version-$arch.AppImage"
