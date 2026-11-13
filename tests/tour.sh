#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The tour of the app (crates/app/src/tour.rs, debug builds) from the fake
# sysfs trees in tests/fixtures, on an invisible X server: every page and
# subpage, every large number and clickable row, the main menu and its
# dialogs, the clipboard. Fails when a check of the tour fails (also when
# something expected is missing), when the app does not finish it, or when
# GTK or GLib print any warning or error.
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

# Without a tool the tour is skipped (77), but never under
# BASEBOARD_TESTS_STRICT=1, which `meson test` sets (docs/PROJEKT.md §8).
for tool in xvfb-run dbus-run-session timeout; do
    if ! command -v "$tool" > /dev/null; then
        echo "$tool not found: the tour cannot run"
        [ "${BASEBOARD_TESTS_STRICT:-}" = 1 ] && exit 1
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

# The v1 tree with the processor at 93 °C, close to its limit of 95 °C in
# the profile: the status sentence warns of it (docs/PROJEKT.md §5.1).
cp -RP "$fixtures/sysfs-ms7e61-v1" "$work/sysfs-ms7e61-hot"
echo 93000 > "$work/sysfs-ms7e61-hot/devices/platform/nct6683.2592/hwmon/hwmon7/temp1_input"

# tour TREE LANGUAGE LOCALE SIZE CURVES; CURVES is how many fans of the
# tree open their curve.
tour() {
    name=$1-$2-$4
    home=$work/$name
    root=$fixtures/sysfs-ms7e61-$1
    [ -d "$root" ] || root=$work/sysfs-ms7e61-$1
    hot=
    [ "$1" = hot ] && hot=1
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
        BASEBOARD_SYSFS_ROOT="$root" \
        BASEBOARD_DEBUG_SIZE="$4" BASEBOARD_DEBUG_SHOW_CONTENT=1 \
        BASEBOARD_DEBUG_TOUR=1 BASEBOARD_TOUR_CURVES="$5" BASEBOARD_TOUR_HOT="$hot" \
        xvfb-run -a -s "-screen 0 1920x1200x24" \
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
    if [ -n "$problems" ]; then
        echo "== $name FAILED:$problems"
        failed=1
    fi
}

# Wide and narrow (the narrowest of 360 px; on X11 the window has 10 px
# more for its shadow), on the three drivers (§5.5.2); the warning in the
# widest window.
tour v1 en C.UTF-8 1200x800 6
tour v1 en C.UTF-8 370x720 6
tour hot en C.UTF-8 1850x1100 6
tour hwmon-next en C.UTF-8 1200x800 6
tour fedora-7.3.3 en C.UTF-8 1200x800 0
if locale -a | grep -qx 'pl_PL.utf8'; then
    tour v1 pl pl_PL.UTF-8 1200x800 6
    tour v1 pl pl_PL.UTF-8 370x720 6
else
    echo "== no Polish locale (pl_PL.UTF-8): the tour runs in English only"
    [ "${BASEBOARD_TESTS_STRICT:-}" = 1 ] && failed=1
fi
exit $failed
