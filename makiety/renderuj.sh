#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Render the HTML mockups to PNG, light and dark, with headless Chromium.
# Usage: makiety/renderuj.sh [page ...]   (default: all pages)
set -eu

cd "$(dirname "$0")"
mkdir -p png

browser=$(command -v chromium-browser || command -v chromium || command -v google-chrome)
pages=${*:-"przeglad wentylatory wentylatory-krzywa rgb wydajnosc system"}
scale=${SCALE:-2}
width=1264

for page in $pages; do
    for theme in light dark; do
        case $theme in
            light) suffix=jasny ;;
            dark) suffix=ciemny ;;
        esac
        url="file://$PWD/$page.html?theme=$theme"
        height=$("$browser" --headless --disable-gpu --hide-scrollbars \
            --window-size=$width,800 --dump-dom "$url" 2>/dev/null |
            sed -n 's/.*data-height="\([0-9]*\)".*/\1/p' | head -n 1)
        "$browser" --headless --disable-gpu --hide-scrollbars \
            --force-device-scale-factor="$scale" \
            --window-size=$width,"${height:-1200}" \
            --screenshot="png/$page-$suffix.png" "$url" 2>/dev/null
        echo "png/$page-$suffix.png ($width x ${height:-?})"
    done
done
