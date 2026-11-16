#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Reports what run.sh found (docs/PROJEKT.md M2.8, D79). For each page and
# language: the elements Tab reaches, in order, with their role, name and
# description, for a person to read; the images (charts) and their names.
# A problem, which makes the exit status 1:
#   - an element Tab reaches without a name, unless labels inside it name
#     it (the rows of libadwaita's AdwSidebar), or with a role that says
#     nothing of it;
#   - an image without a name, or an element that can be clicked but not
#     reached with the keyboard;
#   - in Polish, an English msgid of po/pl.po inside a text;
#   - a warning or error of GTK or GLib in the app's log.
# The texts that are the same in both languages are listed for a person
# to judge (names of headers and sensors, units and the board's name are).
#
#   python3 -I check.py OUTDIR

import json
import pathlib
import re
import sys

PO = pathlib.Path(__file__).resolve().parents[2] / "po" / "pl.po"
PAGES = ["overview", "fans", "curve", "system", "hot"]
# Roles that say nothing about an element Tab reaches.
VAGUE = {"filler", "panel", "unknown", "invalid", "redundant object", "section"}
WARNING = re.compile(r"-(WARNING|CRITICAL|ERROR) \*\*")


def walk(node, path=()):
    yield path, node
    for i, child in enumerate(node["children"]):
        yield from walk(child, path + (i,))


def shown(node):
    return "showing" in node["states"] and "visible" in node["states"]


def po_fragments():
    """The fixed parts of the English msgids that Polish translates
    differently, at least two words each."""
    fragments = []
    text = PO.read_text(encoding="utf-8")
    for block in text.split("\n\n"):
        if "msgid_plural" in block:
            continue
        mid = re.search(r'^msgid ((?:".*"\n?)+)', block, re.M)
        mstr = re.search(r'^msgstr ((?:".*"\n?)+)', block, re.M)
        if not mid or not mstr:
            continue
        msgid = "".join(re.findall(r'"(.*)"', mid.group(1))).replace('\\"', '"')
        msgstr = "".join(re.findall(r'"(.*)"', mstr.group(1))).replace('\\"', '"')
        if not msgid or not msgstr or msgid == msgstr:
            continue
        for part in re.split(r"\{[^}]*\}|%[sd]|\\n", msgid):
            part = part.strip(" .,:;")
            if len(part.split()) >= 2:
                fragments.append(part)
    return fragments


def texts(node):
    for key in ("name", "description", "text"):
        value = node.get(key) or ""
        if value.strip():
            yield value


def tab_stops(tree):
    """The stops of one round of Tab."""
    stops = []
    for stop in tree.get("tab", []):
        key = (stop["role"], stop["name"], stop["description"])
        if stops and key == (stops[0]["role"], stops[0]["name"], stops[0]["description"]):
            break
        stops.append(stop)
    return stops


def check_tree(label, tree, problems):
    print(f"\n== {label}: Tab stops (role | name | description)")
    stops = tab_stops(tree)
    if not stops:
        problems.append(f"{label}: Tab reaches nothing")
    for stop in stops:
        name, role = stop["name"].strip(), stop["role"]
        note = ""
        if not name and stop.get("inner"):
            note = f"  (no name; the label inside: {' / '.join(stop['inner'])})"
        elif not name:
            problems.append(f"{label}: a {role or 'nothing'} without a name")
            note = "  ! NO NAME"
        if role in VAGUE:
            problems.append(f"{label}: {name!r} with the role {role!r}")
            note += "  ! VAGUE ROLE"
        description = " ".join(stop["description"].split())
        print(f"  {role:14} | {name[:60]:60} | {description[:80]}{note}")
    print(f"== {label}: images")
    for _, node in walk(tree):
        if shown(node) and node["role"] == "image":
            if not node["name"].strip():
                problems.append(f"{label}: an image without a name")
            print(f"  {node['name'][:100]}")
    for _, node in walk(tree):
        clicks = [a for a in node.get("actions", []) if a in ("click", "activate", "press")]
        if shown(node) and clicks and "focusable" not in node["states"]:
            problems.append(f"{label}: {node['name']!r} can be clicked but not focused")


def main():
    out = pathlib.Path(sys.argv[1])
    problems = []
    fragments = po_fragments()
    for page in PAGES:
        trees = {}
        for lang in ("en", "pl"):
            path = out / f"{page}-{lang}.json"
            if path.exists():
                trees[lang] = json.loads(path.read_text(encoding="utf-8"))
        if not trees:
            problems.append(f"{page}: no dump")
            continue
        for lang, tree in trees.items():
            check_tree(f"{page} {lang}", tree, problems)
        if "pl" not in trees:
            continue
        english = set()
        for _, node in walk(trees["pl"]):
            if shown(node):
                for value in texts(node):
                    for fragment in fragments:
                        if re.search(r"(?<!\w)" + re.escape(fragment) + r"(?!\w)", value):
                            english.add((fragment, value[:100]))
        for fragment, value in sorted(english):
            problems.append(f"{page} pl: English {fragment!r} in {value!r}")
        if "en" in trees:
            en_nodes = dict(walk(trees["en"]))
            same = set()
            for path, node in walk(trees["pl"]):
                other = en_nodes.get(path)
                if other is not None and shown(node):
                    for key in ("name", "description"):
                        value = node.get(key) or ""
                        if re.search(r"[A-Za-z]{3}", value) and other.get(key) == value:
                            same.add(value[:80])
            print(f"== {page}: the same in Polish as in English")
            for value in sorted(same):
                print(f"  {value}")
    for log in sorted(out.glob("*.log")):
        for line in log.read_text(encoding="utf-8", errors="replace").splitlines():
            if WARNING.search(line):
                problems.append(f"{log.name}: {line.strip()[:160]}")
    print()
    for problem in problems:
        print("PROBLEM:", problem)
    print(f"== accessibility check: {'FAILED' if problems else 'passed'}")
    sys.exit(1 if problems else 0)


main()
