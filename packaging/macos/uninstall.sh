#!/bin/bash
# Removes PACK-IE from this Mac: the service, the packie binary and the console app.
# Keeps the bootstrap, state and logs (they hold laboratory data) unless --purge is given.
#
#   sudo packaging/macos/uninstall.sh [--purge]
set -euo pipefail

PACKIE=/usr/local/bin/packie
SUPPORT_DIR="/Library/Application Support/Positive ACK"
LOG_DIR="/Library/Logs/Positive ACK"
BOOTSTRAP="$SUPPORT_DIR/bootstrap.json"
CONSOLE_APP="/Applications/PACK-IE Console.app"
PLIST=/Library/LaunchDaemons/org.openbga.positiveack.plist

purge=0
case "${1:-}" in
    "") ;;
    --purge) purge=1 ;;
    *) echo "usage: $0 [--purge]" >&2; exit 2 ;;
esac

if [ "$(id -u)" -ne 0 ]; then
    echo "error: run as root (sudo $0)" >&2
    exit 1
fi

if [ -x "$PACKIE" ]; then
    "$PACKIE" --bootstrap "$BOOTSTRAP" service uninstall \
        || echo "warning: 'packie service uninstall' failed (service not installed?); continuing" >&2
elif [ -e "$PLIST" ]; then
    echo "error: $PLIST exists but $PACKIE is missing; remove the service by hand (launchctl bootout system/org.openbga.positiveack)" >&2
    exit 1
fi

rm -f "$PACKIE"
rm -rf "$CONSOLE_APP"
pkgutil --forget org.openbga.positiveack.engine >/dev/null 2>&1 || true
pkgutil --forget org.openbga.positiveack.console >/dev/null 2>&1 || true

if [ "$purge" -eq 1 ]; then
    rm -rf "$SUPPORT_DIR" "$LOG_DIR"
    echo "removed PACK-IE, including bootstrap, state and logs"
else
    echo "removed PACK-IE; kept $SUPPORT_DIR and $LOG_DIR (use --purge to delete them)"
fi
