#!/usr/bin/env python3
"""Fail if any tracked or untracked-but-not-ignored .rs/.py file exceeds the line limit."""
import subprocess
import sys

LIMIT = 500
SUFFIXES = (".rs", ".py")


def candidate_files():
    out = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard"],
        check=True, capture_output=True, text=True,
    ).stdout
    return sorted({p for p in out.splitlines() if p.endswith(SUFFIXES)})


def line_count(path):
    try:
        with open(path, "rb") as f:
            return sum(1 for _ in f)
    except FileNotFoundError:
        return 0  # listed by git but deleted in the working tree


def main():
    bad = [(p, n) for p in candidate_files() if (n := line_count(p)) > LIMIT]
    for path, n in bad:
        print(f"{path}: {n} lines (limit {LIMIT})")
    if bad:
        return 1
    print("file length ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
