#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The tour of the app (crates/app/src/tour.rs, debug builds) from the fake
# sysfs trees in tests/fixtures, on an invisible X server: every page and
# subpage, every large number and clickable row, the dialogs. Fails when a
# check of the tour fails, when the app does not finish it, or when GTK or
# GLib print any warning or error.
#
# The app runs on a private session bus on which nothing can be activated
# (no second document portal), with its own runtime, configuration and
# cache folders and the settings in memory: it touches neither the session
# nor the files of the user who runs the tests.
#
# Usage: tour.sh BINARY. `meson test` sets GSETTINGS_SCHEMA_DIR,
# BASEBOARD_BOARDS_DIR and BASEBOARD_LOCALEDIR.
set -eu
binary=$1
fixtures=$(cd "$(dirname "$0")" && pwd)/fixtures

for tool in xvfb-run dbus-run-session timeout; do
    if ! command -v "$tool" > /dev/null; then
        echo "$tool not found: the tour cannot run"
        exit 77
    fi
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
printf '%s\n' \
    '<busconfig>' \
    '  <type>session</type>' \
    '  <listen>unix:tmpdir=/tmp</listen>' \
    '  <auth>EXTERNAL</auth>' \
    '  <policy context="default">' \
    '    <allow send_destination="*" eavesdrop="true"/>' \
    '    <allow eavesdrop="true"/>' \
    '    <allow own="*"/>' \
    '  </policy>' \
    '</busconfig>' > "$work/bus.conf"

failed=0

# tour TREE LANGUAGE LOCALE SIZE
tour() {
    name=$1-$2-$4
    home=$work/$name
    mkdir -p "$home/runtime" "$home/config" "$home/data" "$home/cache" "$home/state"
    chmod 700 "$home/runtime"
    status=0
    env -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS \
        GDK_BACKEND=x11 GTK_A11Y=none NO_AT_BRIDGE=1 \
        ADW_DISABLE_PORTAL=1 GDK_DEBUG=no-portals GSETTINGS_BACKEND=memory \
        XDG_RUNTIME_DIR="$home/runtime" XDG_CONFIG_HOME="$home/config" \
        XDG_DATA_HOME="$home/data" XDG_CACHE_HOME="$home/cache" \
        XDG_STATE_HOME="$home/state" \
        LC_ALL="$3" LANGUAGE="$2" \
        BASEBOARD_SYSFS_ROOT="$fixtures/sysfs-ms7e61-$1" \
        BASEBOARD_DEBUG_SIZE="$4" BASEBOARD_DEBUG_SHOW_CONTENT=1 \
        BASEBOARD_DEBUG_TOUR=1 \
        xvfb-run -a -s "-screen 0 1600x1000x24" \
        dbus-run-session --config-file="$work/bus.conf" -- \
        timeout 120 "$binary" > "$home/log" 2>&1 || status=$?
    echo "== $name"
    cat "$home/log"
    problems=
    [ "$status" -eq 0 ] || problems="$problems exit status $status;"
    grep -Eq -- '-(WARNING|CRITICAL|ERROR) \*\*' "$home/log" &&
        problems="$problems warnings of GTK or GLib;"
    grep -q '^baseboard: tour done$' "$home/log" ||
        problems="$problems the tour did not pass;"
    if [ "$1" = v1 ] && ! grep -q '^baseboard: tour: ok: Fans ' "$home/log"; then
        problems="$problems no fan row opened its curve;"
    fi
    if [ -n "$problems" ]; then
        echo "== $name FAILED:$problems"
        failed=1
    fi
}

# Wide and narrow (the narrowest of 360 px; on X11 the window has 10 px
# more for its shadow), on the three drivers (§5.5.2).
tour v1 en C.UTF-8 1200x800
tour v1 en C.UTF-8 370x720
tour hwmon-next en C.UTF-8 1200x800
tour fedora-7.3.3 en C.UTF-8 1200x800
if locale -a | grep -qx 'pl_PL.utf8'; then
    tour v1 pl pl_PL.UTF-8 1200x800
    tour v1 pl pl_PL.UTF-8 370x720
else
    echo "== no Polish locale (pl_PL.UTF-8): the tour runs in English only"
fi
exit $failed
