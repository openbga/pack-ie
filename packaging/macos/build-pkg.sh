#!/bin/bash
# Builds the macOS installer package: target/packaging/packie-<version>-macos-<arch>.pkg
#
#   packaging/macos/build-pkg.sh [path/to/packie]
#
# Environment:
#   PACKIE_BIN          the packie binary (default: target/release/packie, or the only target/<triple>/release/packie)
#   PACKIE_CONSOLE_APP  the console "PACK-IE Console.app" (default: the Tauri bundle output under
#                    console/src-tauri/target[/<triple>]/release/bundle/macos); skipped when absent
# Not signed or notarized.
set -euo pipefail

# Keep extended attributes out of the archives as AppleDouble "._*" files. macOS re-attaches the
# unremovable com.apple.provenance attribute to files this script creates, so the payload lists
# "._*" entries for it; the installer restores them as attributes, not as files.
export COPYFILE_DISABLE=1

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=../common.sh
source "$here/../common.sh"

command -v pkgbuild >/dev/null || die "pkgbuild not found (macOS only)"
command -v productbuild >/dev/null || die "productbuild not found (macOS only)"

resolve_packie_bin "${1:-}"

arch=$(lipo -archs "$PACKIE_BIN_PATH" 2>/dev/null | tr ' ' '+' || true)
[ -n "$arch" ] || arch=$(uname -m)

# Console bundle: explicit path, else the Tauri output.
console_app="${PACKIE_CONSOLE_APP:-}"
if [ -z "$console_app" ]; then
    for candidate in \
        "$ROOT"/console/src-tauri/target/release/bundle/macos/*.app \
        "$ROOT"/console/src-tauri/target/*/release/bundle/macos/*.app; do
        if [ -d "$candidate" ]; then
            console_app="$candidate"
            break
        fi
    done
elif [ ! -d "$console_app" ]; then
    die "PACKIE_CONSOLE_APP is not a directory: $console_app"
fi
if [ -z "$console_app" ]; then
    echo "console: no Tauri bundle found under console/src-tauri/target/*/bundle/macos; the package will not include the console"
fi

work="$OUT_DIR/macos-work"
rm -rf "$work"
mkdir -p "$work/engine-root/usr/local/bin" "$work/scripts" "$OUT_DIR"

install -m 0755 "$PACKIE_BIN_PATH" "$work/engine-root/usr/local/bin/packie"
install -m 0755 "$here/postinstall" "$work/scripts/postinstall"
install -m 0644 "$here/bootstrap.json" "$work/scripts/bootstrap.json"
xattr -cr "$work/engine-root" "$work/scripts"

pkgbuild --quiet \
    --root "$work/engine-root" \
    --identifier org.openbga.positiveack.engine \
    --version "$VERSION" \
    --install-location / \
    --ownership recommended \
    --scripts "$work/scripts" \
    "$work/packie-engine.pkg"

products=(--package "$work/packie-engine.pkg")

if [ -n "$console_app" ]; then
    app_name=$(basename "$console_app")
    mkdir -p "$work/console-root"
    ditto --noextattr --norsrc "$console_app" "$work/console-root/$app_name"
    xattr -cr "$work/console-root"
    # The Tauri bundler leaves the app linker-signed with an unsealed Info.plist and resources,
    # which fails `codesign --verify`. Seal such a bundle ad hoc; a properly signed app is kept.
    if ! codesign --verify --deep "$work/console-root/$app_name" 2>/dev/null; then
        codesign --force --deep --sign - "$work/console-root/$app_name"
    fi
    # Not relocatable: always install to /Applications, never update a copy found elsewhere.
    cat > "$work/console-components.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<array>
  <dict>
    <key>RootRelativeBundlePath</key>
    <string>$app_name</string>
    <key>BundleIsRelocatable</key>
    <false/>
    <key>BundleIsVersionChecked</key>
    <false/>
    <key>BundleOverwriteAction</key>
    <string>upgrade</string>
  </dict>
</array>
</plist>
PLIST
    pkgbuild --quiet \
        --root "$work/console-root" \
        --component-plist "$work/console-components.plist" \
        --identifier org.openbga.positiveack.console \
        --version "$VERSION" \
        --install-location /Applications \
        --ownership recommended \
        "$work/packie-console.pkg"
    products+=(--package "$work/packie-console.pkg")
    echo "console: including $console_app"
fi

# The installer may only run on the architectures the binary was built for.
host_archs=${arch//+/,}
productbuild --synthesize "${products[@]}" "$work/distribution.xml" >/dev/null
sed -i '' -E "s/hostArchitectures=\"[^\"]*\"/hostArchitectures=\"$host_archs\"/" "$work/distribution.xml"

out="$OUT_DIR/packie-$VERSION-macos-$arch.pkg"
rm -f "$out"
productbuild --distribution "$work/distribution.xml" --package-path "$work" --version "$VERSION" "$out" >/dev/null
rm -rf "$work"
echo "built $out"
