#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# One run of the accessibility check, inside its invisible X server and
# private session bus (run.sh starts it): a private accessibility bus with
# the registry daemon, the app, then the dump of the app's AT-SPI tree.
# at-spi-bus-launcher is not used: it needs a session of its own, and GTK
# and libatspi both take the bus from AT_SPI_BUS_ADDRESS first.
#
# Environment: APP (the app), A11Y_DIR (this folder), A11Y_OUT (the JSON
# to write), A11Y_LOG (the app's log), A11Y_HOT (optional: a temperature
# file of a fake tree to set to 93 °C after the dump).
address=$A11Y_LOG.address
# The file may still hold the address of a bus from an earlier run, and the
# redirection below empties it only once the background job starts: empty it
# here, so that the wait reads the address of this run's bus.
: > "$address"
dbus-daemon --config-file="$A11Y_DIR/a11y-bus.conf" --nofork --print-address \
    > "$address" 2> "$A11Y_LOG.bus" &
bus=$!
tries=0
while [ ! -s "$address" ] && [ $tries -lt 50 ]; do
    sleep 0.1
    tries=$((tries + 1))
done
AT_SPI_BUS_ADDRESS=$(head -n 1 "$address")
export AT_SPI_BUS_ADDRESS
/usr/libexec/at-spi2-registryd > "$A11Y_LOG.registry" 2>&1 &
registry=$!
sleep 1
"$APP" > "$A11Y_LOG" 2>&1 &
app=$!
python3 -I "$A11Y_DIR/dump.py" "$A11Y_OUT"
status=$?
# The processor gets close to its limit while the app runs: the status
# sentence turns into a warning, which must not make GTK warn.
if [ -n "${A11Y_HOT:-}" ]; then
    echo 93000 > "$A11Y_HOT"
    sleep 5
fi
kill "$app" 2> /dev/null
wait "$app" 2> /dev/null
kill "$registry" "$bus" 2> /dev/null
exit $status
