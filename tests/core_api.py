#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Every public item of the core crate is used outside the tests, by the
# app or by core itself, or is listed in tests/core-api-exceptions.txt with
# the step of the plan that needs it (docs/PROJEKT.md §8, Z3). Rustc does
# not warn about an unused `pub` item of a library, so without this check
# remains of a replaced layout stay. Code under `#[cfg(test)]` and the
# integration tests count neither as a definition nor as a use.
#
# What counts as a use, outside the item's own line: a type, trait or
# constant by its name; a method as `.name(` or `::name`; a free function
# as `name(` or `::name`; a field when it is read, as `.name`, or taken
# apart in a pattern of the app. Names that are common words can hide an
# unused item; the check never reports a used one.
#
# Usage: core_api.py SOURCE_ROOT

import pathlib
import re
import sys

ITEM = re.compile(
    r"^(?P<indent>\s*)pub (?:const fn|unsafe fn|fn|struct|enum|trait|type|const|static|mod)"
    r"\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)"
)
FIELD = re.compile(r"^\s+pub (?P<name>[a-z_][a-z0-9_]*)\s*:")
TEST_MODULE = re.compile(r"^#\[cfg\(test\)\]\s*\n\s*mod tests", re.MULTILINE)
TEST_ONLY_FILE = re.compile(r"^#\[cfg\(test\)\]\s*\n\s*(?:pub )?mod (\w+);", re.MULTILINE)


def without_tests(text):
    """The text before the test module at the end of a file."""
    match = TEST_MODULE.search(text)
    return text[: match.start()] if match else text


def test_only(paths):
    """The files of modules declared under `#[cfg(test)]`."""
    skipped = set()
    for path in paths:
        base = path.parent if path.name in ("lib.rs", "main.rs", "mod.rs") else (
            path.parent / path.stem
        )
        for name in TEST_ONLY_FILE.findall(path.read_text()):
            skipped.add(base / f"{name}.rs")
            skipped.update((base / name).rglob("*.rs"))
    return skipped


def sources(folder):
    """The code of each file under `folder`, without its tests, as lines."""
    paths = sorted(folder.rglob("*.rs"))
    skipped = test_only(paths)
    return {
        path: without_tests(path.read_text()).splitlines()
        for path in paths
        if path not in skipped
    }


def module(path, src):
    """The module of a file, e.g. `fans::driver` for fans/driver.rs."""
    parts = list(path.relative_to(src).with_suffix("").parts)
    if parts[-1] in ("lib", "mod"):
        parts.pop()
    return "::".join(parts)


def items(core):
    """(kind, name, path, line index) of each public item of core."""
    found = []
    for path, lines in core.items():
        cfg_test = False
        for index, line in enumerate(lines):
            if line.strip() == "#[cfg(test)]":
                cfg_test = True
                continue
            item = ITEM.match(line)
            if item:
                if not cfg_test:
                    kind = line.split()[1]
                    if kind == "fn" and item["indent"]:
                        kind = "method"
                    found.append((kind, item["name"], path, index))
                cfg_test = False
                continue
            field = FIELD.match(line)
            if field and not cfg_test:
                found.append(("field", field["name"], path, index))
            if line.strip() and not line.strip().startswith(("#", "//")):
                cfg_test = False
    return found


def patterns(kind, name):
    """What counts as a use in core and in the app."""
    word = re.escape(name)
    call = r"\s*(?:::<[^>]*>)?\("
    if kind == "method":
        anywhere = rf"\.{word}{call}|::{word}\b"
    elif kind == "fn":
        anywhere = rf"\b{word}{call}|::{word}\b|\b{word}\)"
    elif kind == "field":
        anywhere = rf"\.{word}\b"
        return anywhere, rf"{anywhere}|[{{,]\s*{word}\s*[:,}}]"
    else:
        anywhere = rf"\b{word}\b"
    return anywhere, anywhere


def main():
    root = pathlib.Path(sys.argv[1])
    core = sources(root / "crates" / "core" / "src")
    app = "\n".join(
        "\n".join(lines) for lines in sources(root / "crates" / "app" / "src").values()
    )
    exceptions = {}
    listed = root / "tests" / "core-api-exceptions.txt"
    for line in listed.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            name, _, reason = line.partition(" ")
            exceptions[name] = reason.strip()
    for name, reason in exceptions.items():
        if not reason:
            print(f"{listed.name}: {name} has no reason")
            return 1

    src = root / "crates" / "core" / "src"
    unused = []
    listed_names = set()
    for kind, name, path, index in items(core):
        qualified = f"{module(path, src)}::{name}".lstrip(":")
        if qualified in exceptions:
            listed_names.add(qualified)
            continue
        if kind == "mod":
            continue
        in_core, in_app = patterns(kind, name)
        rest = "\n".join(
            "\n".join(line for i, line in enumerate(lines) if (p, i) != (path, index))
            for p, lines in core.items()
        )
        if not re.search(in_core, rest) and not re.search(in_app, app):
            unused.append(f"{path.relative_to(root)}:{index + 1}: pub {kind} {qualified}")
    for line in unused:
        print(f"{line} is not used outside the tests")
    gone = sorted(set(exceptions) - listed_names)
    for name in gone:
        print(f"{listed.name}: {name} is not a public item of core")
    if unused or gone:
        print(
            "remove the public items nobody uses, or list them with the step of the "
            f"plan that needs them in tests/{listed.name}"
        )
        return 1
    print("every public item of core is used outside the tests or listed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
