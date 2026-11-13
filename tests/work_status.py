#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
#
# docs/PROJEKT.md stays a specification (docs/PROJEKT.md §8, Z13): "Stan
# prac" is a short list of at most 250 words, replaced rather than added
# to, and the head of the document does not count the decisions, which
# live in docs/DECYZJE.md.
#
# Usage: work_status.py SOURCE_ROOT

import pathlib
import re
import sys

LIMIT = 250


def main():
    root = pathlib.Path(sys.argv[1])
    lines = (root / "docs" / "PROJEKT.md").read_text().split("\n")
    problems = []
    starts = [i for i, line in enumerate(lines) if line.startswith("**Stan prac**")]
    if len(starts) != 1:
        problems.append(f"PROJEKT.md has {len(starts)} paragraphs \"Stan prac\", not one")
    else:
        end = next(
            (i for i in range(starts[0], len(lines)) if not lines[i].strip()), len(lines)
        )
        words = sum(len(line.split()) for line in lines[starts[0] : end])
        if words > LIMIT:
            problems.append(f"\"Stan prac\" has {words} words, more than {LIMIT}")
    head = "\n".join(lines[:10])
    if re.search(r"\bD\d+\s*[–-]\s*D\d+\b", head):
        problems.append("the head of PROJEKT.md counts the decisions; they are in DECYZJE.md")
    if not (root / "docs" / "DECYZJE.md").is_file():
        problems.append("docs/DECYZJE.md is missing")
    for problem in problems:
        print(problem)
    if problems:
        return 1
    print("\"Stan prac\" is short and the decisions are in DECYZJE.md")
    return 0


if __name__ == "__main__":
    sys.exit(main())
