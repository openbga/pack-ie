#!/bin/bash
# Builds the Debian package: target/packaging/packie_<version>_<arch>.deb
#
#   packaging/linux/build-deb.sh [path/to/packie] [architecture]
#
# Environment:
#   PACKIE_BIN          the Linux packie binary (default: target/release/packie, or the only
#                    target/<triple>/release/packie)
#   PACKIE_DEB_ARCH     Debian architecture (amd64, arm64, ...); default: detected from the binary
#   PACKIE_MAINTAINER   Maintainer field (default: Positive ACK contributors)
#   PACKIE_STAGE_ONLY   set to 1 to stop after staging the package tree (no dpkg-deb needed)
# Not signed.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=../common.sh
source "$here/../common.sh"

resolve_packie_bin "${1:-}"

arch="${2:-${PACKIE_DEB_ARCH:-}}"
if [ -z "$arch" ]; then
    case "$(file -b "$PACKIE_BIN_PATH" 2>/dev/null)" in
        ELF*x86-64*) arch=amd64 ;;
        ELF*aarch64*) arch=arm64 ;;
        ELF*ARM*) arch=armhf ;;
        *) die "cannot detect the architecture of $PACKIE_BIN_PATH (not a Linux ELF binary?); pass it as the second argument or set PACKIE_DEB_ARCH" ;;
    esac
fi

# Debian versions sort a pre-release before the release with "~", not "-".
deb_version="${VERSION/-/~}"
maintainer="${PACKIE_MAINTAINER:-Positive ACK contributors <noreply@invalid>}"

stage="$OUT_DIR/deb-stage"
rm -rf "$stage"
mkdir -p "$stage/DEBIAN" "$stage/usr/bin" "$stage/usr/share/packie" "$stage/usr/share/doc/packie" "$OUT_DIR"

install -m 0755 "$PACKIE_BIN_PATH" "$stage/usr/bin/packie"
install -m 0644 "$here/bootstrap.json" "$stage/usr/share/packie/bootstrap.json"
install -m 0644 "$ROOT/LICENSE" "$stage/usr/share/doc/packie/copyright"
for script in postinst prerm postrm; do
    install -m 0755 "$here/debian/$script" "$stage/DEBIAN/$script"
done
# Directory modes must not depend on the builder's umask (dpkg-deb keeps them as staged).
find "$stage" -type d -exec chmod 0755 {} +

installed_kb=$(du -sk "$stage/usr" | cut -f1)
cat > "$stage/DEBIAN/control" <<CONTROL
Package: packie
Version: $deb_version
Architecture: $arch
Maintainer: $maintainer
Section: science
Priority: optional
Depends: libc6, systemd
Installed-Size: $installed_kb
Description: Positive ACK laboratory interface engine
 Runs as a systemd service (packie) on one laboratory computer and moves instrument
 and LIS messages between endpoints. Configuration lives in /etc/packie/bootstrap.json.
CONTROL
chmod 0644 "$stage/DEBIAN/control"

if [ "${PACKIE_STAGE_ONLY:-}" = 1 ]; then
    echo "staged $stage (PACKIE_STAGE_ONLY=1, no .deb built)"
    exit 0
fi

command -v dpkg-deb >/dev/null || die "dpkg-deb not found; install dpkg (Debian/Ubuntu) or use PACKIE_STAGE_ONLY=1"
out="$OUT_DIR/packie_${deb_version}_${arch}.deb"
rm -f "$out"
dpkg-deb --root-owner-group --build "$stage" "$out" >/dev/null
rm -rf "$stage"
echo "built $out"
