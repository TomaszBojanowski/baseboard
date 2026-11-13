#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Every translation of po/LINGUAS is valid and complete, without fuzzy or
# obsolete (#~) entries (docs/PROJEKT.md §8, Z7). After
# `meson compile baseboard-update-po`, `msgattrib --no-obsolete` removes
# the obsolete entries.
#
# Usage: translations.sh PO_FOLDER
set -eu
po=$1
status=0
for lang in $(grep -v '^#' "$po/LINGUAS"); do
    file="$po/$lang.po"
    stats=$(LC_ALL=C msgfmt --check --statistics --output-file=/dev/null "$file" 2>&1)
    case "$stats" in
        *fuzzy* | *untranslated*)
            echo "$lang.po: $stats"
            status=1
            ;;
    esac
    if [ -n "$(msgattrib --only-obsolete "$file")" ]; then
        echo "$lang.po has obsolete entries (#~)"
        status=1
    fi
done
exit $status
