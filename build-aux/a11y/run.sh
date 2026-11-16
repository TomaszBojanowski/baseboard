#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The accessibility check (docs/PROJEKT.md §10.7, M2.8, D79): what a
# screen reader gets from the Overview, the Fans page, the curve details of
# SYS 1 and the System page, in English and Polish, on the v1 tree, and
# from the Overview while the processor gets hot. Each run is the debug
# build on an invisible X server with a private session bus without
# services and a private accessibility bus; check.py then reports. See
# README.md.
#
#   sh build-aux/a11y/run.sh [OUTDIR]      (default: build/a11y)
#
# Exits with 1 when check.py finds a problem.
set -u
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
out=${1:-$repo/build/a11y}

for tool in xvfb-run dbus-run-session dbus-daemon timeout python3 /usr/libexec/at-spi2-registryd; do
    if ! command -v "$tool" > /dev/null; then
        echo "$tool not found: the check cannot run"
        exit 1
    fi
done
mkdir -p "$out"
rm -f "$out"/*.json "$out"/*.log
meson compile -C "$repo/build" > /dev/null || exit 1

# run LANGUAGE LOCALE NAME DISPLAYNUM [VAR=value...]
run() {
    lang=$1 locale=$2 name=$3 num=$4
    shift 4
    home=$(mktemp -d "$out/home.XXXXXX")
    mkdir -p "$home/runtime" "$home/config/gtk-4.0" "$home/data" "$home/cache" "$home/state"
    chmod 700 "$home/runtime"
    # The font of a GNOME session, as in tests/tour.sh.
    printf '%s\n' '[Settings]' 'gtk-font-name=Adwaita Sans 11' 'gtk-xft-antialias=1' \
        'gtk-xft-hinting=1' 'gtk-xft-hintstyle=hintslight' 'gtk-xft-rgba=none' \
        > "$home/config/gtk-4.0/settings.ini"
    env -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u GTK_A11Y -u NO_AT_BRIDGE \
        -u BASEBOARD_SYSFS_ROOT \
        GDK_BACKEND=x11 ADW_DISABLE_PORTAL=1 GDK_DEBUG=no-portals GSETTINGS_BACKEND=memory \
        XDG_RUNTIME_DIR="$home/runtime" XDG_CONFIG_HOME="$home/config" \
        XDG_DATA_HOME="$home/data" XDG_CACHE_HOME="$home/cache" XDG_STATE_HOME="$home/state" \
        LC_ALL="$locale" LANGUAGE="$lang" \
        GSETTINGS_SCHEMA_DIR="$repo/build/data" BASEBOARD_LOCALEDIR="$repo/build/po" \
        BASEBOARD_BOARDS_DIR="$repo/data/boards" \
        BASEBOARD_SYSFS_ROOT="$repo/tests/fixtures/sysfs-ms7e61-v1" \
        BASEBOARD_DEBUG_SIZE=1210x810 BASEBOARD_DEBUG_SHOW_CONTENT=1 \
        BASEBOARD_DEBUG_NO_ANIMATIONS=1 "$@" \
        APP="$repo/build/crates/app/baseboard" A11Y_DIR="$here" \
        A11Y_OUT="$out/$name-$lang.json" A11Y_LOG="$out/$name-$lang.log" \
        xvfb-run -n "$num" -s "-screen 0 1920x1200x24" \
        dbus-run-session --config-file="$here/session-bus.conf" -- \
        timeout 300 sh "$here/session.sh"
    echo "== $name $lang: exit $?"
    rm -r "$home"
}

# A copy of the v1 tree whose processor gets hot while the app runs.
tree=$(mktemp -d "$out/tree.XXXXXX")
cp -RP "$repo/tests/fixtures/sysfs-ms7e61-v1/." "$tree/"
temp=$tree/devices/platform/nct6683.2592/hwmon/hwmon7/temp1_input
chmod u+w "$temp"

run en C.UTF-8 overview 261 &
run pl pl_PL.UTF-8 overview 262 &
run en C.UTF-8 fans 263 BASEBOARD_DEBUG_PAGE=fans &
run pl pl_PL.UTF-8 fans 264 BASEBOARD_DEBUG_PAGE=fans &
run en C.UTF-8 curve 265 BASEBOARD_DEBUG_PAGE=fans BASEBOARD_DEBUG_CURVE=sys1 &
run pl pl_PL.UTF-8 curve 266 BASEBOARD_DEBUG_PAGE=fans BASEBOARD_DEBUG_CURVE=sys1 &
run en C.UTF-8 system 267 BASEBOARD_DEBUG_PAGE=system &
run pl pl_PL.UTF-8 system 268 BASEBOARD_DEBUG_PAGE=system &
run en C.UTF-8 hot 269 BASEBOARD_SYSFS_ROOT="$tree" A11Y_HOT="$temp" &
wait
rm -r "$tree"
python3 -I "$here/check.py" "$out"
