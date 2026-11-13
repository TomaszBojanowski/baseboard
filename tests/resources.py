#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The app's resources and styles have no orphans (docs/PROJEKT.md §8, Z6):
# - every file of baseboard.gresource.xml is used: a .ui file or the
#   style sheet by its name, an icon by its icon name;
# - every class of style.css is set in the code or in a .ui file;
# - every class the code or a .ui file sets has a rule in style.css, or is
#   a class of GTK or libadwaita.
#
# Usage: resources.py SOURCE_ROOT

import pathlib
import re
import sys

# Loaded by AdwApplication from the resource base path, never named.
LOADED_BY_ADWAITA = {"style.css", "shortcuts-dialog.ui"}

# Style classes of GTK and libadwaita the app uses as they are.
TOOLKIT_CLASSES = {
    "accent", "body", "boxed-list", "caption", "card", "destructive-action",
    "dimmed", "error", "flat", "heading", "linked", "monospace",
    "navigation-sidebar", "numeric", "osd", "pill", "property", "subtitle",
    "success", "suggested-action", "title", "title-1", "title-2", "title-3",
    "title-4", "toolbar", "view", "warning",
}

# The calls and elements that set classes; only literal names count.
SET_IN_CODE = [
    re.compile(r"css_classes\(\s*&?\[([^\]]*)\]"),
    re.compile(r"(?:add|remove|has)_css_class\(\s*(\"[^\"]+\")"),
]
SET_IN_UI = re.compile(r"<class name=\"([^\"]+)\"")


def main():
    root = pathlib.Path(sys.argv[1])
    src = root / "crates" / "app" / "src"
    code = "\n".join(p.read_text() for p in sorted(src.rglob("*.rs")))
    ui = "\n".join(p.read_text() for p in sorted(src.glob("*.ui")))
    problems = []

    bundle = (src / "baseboard.gresource.xml").read_text()
    for attributes, name in re.findall(r"<file([^>]*)>([^<]+)</file>", bundle):
        alias = re.search(r"alias=\"([^\"]+)\"", attributes)
        published = alias[1] if alias else name
        if published in LOADED_BY_ADWAITA:
            continue
        if published.startswith("icons/"):
            icon = pathlib.PurePath(published).stem
            if f'"{icon}"' not in code and f">{icon}<" not in ui:
                problems.append(f"the icon {icon} ({name}) is not used")
        elif f"/{published}" not in code and f'"{published}"' not in code:
            problems.append(f"the resource {published} is not used")

    css = (src / "style.css").read_text()
    rules = re.sub(r"/\*.*?\*/", "", css, flags=re.DOTALL)
    selectors = "\n".join(re.findall(r"([^{}]+)\{", rules))
    styled = set(re.findall(r"\.([a-zA-Z][\w-]*)", selectors))
    for name in sorted(styled - TOOLKIT_CLASSES):
        if f'"{name}"' not in code and f'<class name="{name}"' not in ui:
            problems.append(f"style.css has a rule for .{name}, which nothing sets")

    set_classes = set(SET_IN_UI.findall(ui))
    for pattern in SET_IN_CODE:
        for found in pattern.findall(code):
            set_classes.update(re.findall(r"\"([^\"]+)\"", found))
    for name in sorted(set_classes - styled - TOOLKIT_CLASSES):
        problems.append(f"the class {name} is set, but style.css has no rule for it")

    for problem in problems:
        print(problem)
    if problems:
        return 1
    print("every resource and style class is used")
    return 0


if __name__ == "__main__":
    sys.exit(main())
