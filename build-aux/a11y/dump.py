#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Dumps the AT-SPI tree of the running Baseboard to JSON, as a screen
# reader sees it: for each element its role, name, description, states,
# actions, attributes, text and children; then presses Tab over AT-SPI and
# records, under "tab", what each press reaches. Waits for the app and for
# its pages to fill. session.sh runs it.
#
#   python3 -I dump.py OUT.json

import json
import sys
import time
import warnings

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, GLib  # noqa: E402

warnings.filterwarnings("ignore", category=DeprecationWarning)

# Seconds for the first samples and for the curve page to open.
SETTLE = 10
TAB = 0xFF09
PRESSES = 70


def safe(call, default=None):
    try:
        return call()
    except GLib.Error:
        return default


def find_app():
    deadline = time.time() + 60
    while time.time() < deadline:
        desktop = Atspi.get_desktop(0)
        for i in range(desktop.get_child_count()):
            child = desktop.get_child_at_index(i)
            if child is not None and (child.get_name() or "").lower() == "baseboard":
                if child.get_child_count() > 0:
                    return child
        time.sleep(0.5)
    return None


def text_of(acc):
    if safe(acc.get_text_iface) is None:
        return ""
    length = safe(lambda: Atspi.Text.get_character_count(acc), 0)
    return safe(lambda: Atspi.Text.get_text(acc, 0, length), "") or ""


def node(acc, depth=0):
    entry = {
        "role": safe(acc.get_role_name, ""),
        "name": safe(acc.get_name, "") or "",
        "description": safe(acc.get_description, "") or "",
        "states": safe(lambda: [s.value_nick for s in acc.get_state_set().get_states()], []),
        "attributes": safe(acc.get_attributes, {}) or {},
        "text": text_of(acc),
    }
    if safe(acc.get_action_iface) is not None:
        count = safe(lambda: Atspi.Action.get_n_actions(acc), 0)
        entry["actions"] = [
            safe(lambda i=i: Atspi.Action.get_action_name(acc, i), "") for i in range(count)
        ]
    children = []
    if depth < 80:
        for i in range(safe(acc.get_child_count, 0)):
            child = safe(lambda i=i: acc.get_child_at_index(i))
            if child is not None:
                children.append(node(child, depth + 1))
    entry["children"] = children
    return entry


def focused(acc, depth=0):
    """The focused element under `acc`, looking only into shown ones."""
    states = safe(acc.get_state_set)
    if states is None:
        return None
    if states.contains(Atspi.StateType.FOCUSED):
        return acc
    if depth > 0 and not states.contains(Atspi.StateType.SHOWING):
        return None
    for i in range(safe(acc.get_child_count, 0)):
        child = safe(lambda i=i: acc.get_child_at_index(i))
        if child is not None:
            found = focused(child, depth + 1)
            if found is not None:
                return found
    return None


def inner_labels(acc, depth=0, found=None):
    """The names of the shown labels under `acc`: what a screen reader
    reads of an element without a name of its own."""
    found = [] if found is None else found
    for i in range(safe(acc.get_child_count, 0)):
        child = safe(lambda i=i: acc.get_child_at_index(i))
        if child is None or len(found) >= 8:
            continue
        states = safe(child.get_state_set)
        if states is None or not states.contains(Atspi.StateType.SHOWING):
            continue
        name = safe(child.get_name, "") or ""
        if safe(child.get_role_name, "") == "label" and name:
            found.append(name)
        elif depth < 6:
            inner_labels(child, depth + 1, found)
    return found


def tab_order(app):
    """What Tab reaches, press by press."""
    stops = []
    for _ in range(PRESSES):
        Atspi.generate_keyboard_event(TAB, None, Atspi.KeySynthType.SYM)
        time.sleep(0.3)
        acc = focused(app)
        if acc is None:
            stops.append({"role": "", "name": "", "description": "", "inner": []})
            continue
        stops.append(
            {
                "role": safe(acc.get_role_name, ""),
                "name": safe(acc.get_name, "") or "",
                "description": safe(acc.get_description, "") or "",
                "inner": inner_labels(acc),
            }
        )
    return stops


def main():
    app = find_app()
    if app is None:
        sys.exit("baseboard did not appear on the accessibility bus")
    time.sleep(SETTLE)
    tree = node(app)
    tree["tab"] = tab_order(app)
    with open(sys.argv[1], "w", encoding="utf-8") as f:
        json.dump(tree, f, ensure_ascii=False, indent=1)


main()
