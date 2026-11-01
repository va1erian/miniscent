#!/usr/bin/env python3
"""Fail if a relative link in a tracked markdown file does not resolve."""
import os
import re
import subprocess
import sys

LINK = re.compile(r"\[[^\]]*\]\(([^)\s]+)\)")
SKIP_PREFIXES = ("http://", "https://", "mailto:", "#")


def markdown_files():
    out = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "*.md"],
        check=True, capture_output=True, text=True,
    ).stdout
    return sorted(set(out.splitlines()))


def broken_links(path):
    with open(path, encoding="utf-8") as f:
        text = f.read()
    base = os.path.dirname(path)
    for target in LINK.findall(text):
        if target.startswith(SKIP_PREFIXES):
            continue
        file_part = target.split("#", 1)[0]
        if file_part and not os.path.exists(os.path.join(base, file_part)):
            yield target


def main():
    failed = False
    for path in markdown_files():
        if not os.path.exists(path):
            continue
        for target in broken_links(path):
            print(f"{path}: broken link {target}")
            failed = True
    if not failed:
        print("docs ok")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
