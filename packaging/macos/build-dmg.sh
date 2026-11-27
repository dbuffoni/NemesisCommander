#!/usr/bin/env bash
# Packages Pairnox for macOS as an app bundle in a disk image:
#
#   packaging/macos/build-dmg.sh <directory with pnxr and pnxr-gui> <version>
#
# Produces Pairnox-<version>-macos-<arch>[-unsigned].dmg in the
# current directory. Qt is bundled by macdeployqt (qmake on the PATH is the
# Qt the window was built with). The bundle is signed with $MACOS_SIGN_IDENTITY
# (a "Developer ID Application" certificate in the keychain); without it, it
# is signed ad hoc (it runs here, Gatekeeper asks on other Macs) and the
# image is named `-unsigned`. With $APPLE_ID, $APPLE_TEAM_ID and
# $APPLE_APP_PASSWORD set too, the image is notarized and stapled.
set -euo pipefail

bin_dir=${1:?usage: build-dmg.sh <bin dir> <version>}
version=${2:?usage: build-dmg.sh <bin dir> <version>}
here=$(cd "$(dirname "$0")" && pwd)
root="$here/../.."
arch=$(uname -m)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

app="$work/dmg/Pairnox.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m755 "$bin_dir/pnxr-gui" "$app/Contents/MacOS/pnxr-gui"
install -m755 "$bin_dir/pnxr" "$app/Contents/MacOS/pnxr"
sed "s/@VERSION@/$version/g" "$here/Info.plist" > "$app/Contents/Info.plist"

# The icon, from assets/branding.
if command -v iconutil >/dev/null; then
    icons="$work/Pairnox.iconset"
    mkdir -p "$icons"
    brand="$root/assets/branding"
    for size in 16 32 128 256 512; do
        cp "$brand/pairnox-$size.png" "$icons/icon_${size}x${size}.png"
    done
    cp "$brand/pairnox-32.png" "$icons/icon_16x16@2x.png"
    cp "$brand/pairnox-64.png" "$icons/icon_32x32@2x.png"
    cp "$brand/pairnox-256.png" "$icons/icon_128x128@2x.png"
    cp "$brand/pairnox-512.png" "$icons/icon_256x256@2x.png"
    iconutil -c icns "$icons" -o "$app/Contents/Resources/Pairnox.icns"
fi

# macdeployqt reports what it could not do and still succeeds: its errors
# stop the build here.
macdeployqt "$app" -qmldir="$root/gui/qml" -executable="$app/Contents/MacOS/pnxr-gui" 2>&1 |
    tee "$work/macdeployqt.log"
# The database drivers of Qt are not used, and want libraries that are not
# there: they are left out, and so are the errors about those libraries.
rm -rf "$app/Contents/PlugIns/sqldrivers"
harmless='^ERROR: (""|no file at ".*/(libmimerapi|libiodbc|libpq)[^/]*\.dylib")$'
if grep '^ERROR:' "$work/macdeployqt.log" | grep -Ev "$harmless" >&2; then
    echo "build-dmg.sh: macdeployqt failed: the app would not run elsewhere" >&2
    exit 1
fi

# Licence and third-party notices (Qt, libarchive, crates).
install -m644 "$root/LICENSE" "$app/Contents/Resources/LICENSE.txt"
python3 "$root/scripts/third_party.py" -o "$app/Contents/Resources/THIRD-PARTY-NOTICES.md"

identity=${MACOS_SIGN_IDENTITY:--}
if [ "$identity" != "-" ]; then
    codesign --force --deep --options runtime --timestamp -s "$identity" "$app"
    suffix=""
else
    codesign --force --deep -s - "$app"
    suffix="-unsigned"
fi
codesign --verify --deep --strict "$app"

ln -s /Applications "$work/dmg/Applications"
dmg="Pairnox-$version-macos-$arch$suffix.dmg"
rm -f "$dmg"
hdiutil create -volname "Pairnox" -srcfolder "$work/dmg" -ov -format UDZO "$dmg"

if [ "$identity" != "-" ] && [ -n "${APPLE_ID:-}" ]; then
    xcrun notarytool submit "$dmg" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
        --password "$APPLE_APP_PASSWORD" --wait
    xcrun stapler staple "$dmg"
fi
echo "wrote $dmg"
