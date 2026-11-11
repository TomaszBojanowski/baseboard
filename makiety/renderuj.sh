#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Render the HTML mockups to PNG, light and dark, with headless Chromium.
# Usage: makiety/renderuj.sh [page[@state] ...]   (default: all pages)
#   page@state  opens page.html?stan=state, e.g. wentylatory@podglad
#   OUT=dir     output directory (default: png/ next to this script)
#   NARROW=1    also a narrow window (400 px, sidebar collapsed)
#   ACCENT=x    a libadwaita accent color instead of the default blue
#               (teal, green, yellow, orange, red, pink, purple, slate)
#   SCALE=1     smaller files (default: 2)
set -eu

cd "$(dirname "$0")"
out=${OUT:-png}
mkdir -p "$out"

browser=$(command -v chromium-browser || command -v chromium || command -v google-chrome)
pages=${*:-"przeglad wentylatory wentylatory-krzywa wentylatory-stany rgb wydajnosc system"}
scale=${SCALE:-2}
widths=wide
if [ "${NARROW:-0}" = 1 ]; then
    widths="wide narrow"
fi

for spec in $pages; do
    page=${spec%%@*}
    state=
    if [ "$spec" != "$page" ]; then
        state=${spec#*@}
    fi
    for size in $widths; do
        for theme in light dark; do
            case $theme in
                light) suffix=jasny ;;
                dark) suffix=ciemny ;;
            esac
            query="theme=$theme"
            name=$page
            if [ -n "$state" ]; then
                query="$query&stan=$state"
                name="$name-$state"
            fi
            if [ -n "${ACCENT:-}" ]; then
                query="$query&accent=$ACCENT"
                name="$name-akcent-$ACCENT"
            fi
            width=1264
            if [ "$size" = narrow ]; then
                query="$query&width=narrow"
                name="$name-waski"
                width=464
            fi
            url="file://$PWD/$page.html?$query"
            height=$("$browser" --headless --disable-gpu --hide-scrollbars \
                --window-size=$width,800 --dump-dom "$url" 2>/dev/null |
                sed -n 's/.*data-height="\([0-9]*\)".*/\1/p' | head -n 1)
            "$browser" --headless --disable-gpu --hide-scrollbars \
                --force-device-scale-factor="$scale" \
                --window-size=$width,"${height:-1200}" \
                --screenshot="$out/$name-$suffix.png" "$url" 2>/dev/null
            echo "$out/$name-$suffix.png ($width x ${height:-?})"
        done
    done
done
