#!/usr/bin/env python3
"""Fail if the release game binary is over its size budget.

Nothing else watches binary size, and a single dependency can quadruple it
(docs/scripting-language.md). Over budget means shrink something; raising
BUDGET_BYTES is a decision, recorded in the PR that does it.

Run from the repo root. Honours CARGO_TARGET_DIR. `--binary` measures an
existing file without building, `--limit` overrides the budget.
"""
import argparse
import os
import subprocess
import sys

# 1.6 MB until voxel volumes, which took the game from 1,576,960 to 1,885,184 bytes.
# The texture generator adds about 140 KB, and replaced 902 KB of PNGs in the assets.
BUDGET_BYTES = 2_200_000
WARN_FRACTION = 0.8


def default_binary():
    target = os.environ.get("CARGO_TARGET_DIR", "target")
    suffix = ".exe" if os.name == "nt" else ""
    return os.path.join(target, "release", f"miniscent{suffix}")


def build():
    subprocess.run(["cargo", "build", "--release", "-q", "-p", "miniscent-game"], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", help="measure this file instead of building the game")
    parser.add_argument("--limit", type=int, default=BUDGET_BYTES, help="budget in bytes")
    args = parser.parse_args()
    if args.binary is None:
        build()
    path = args.binary or default_binary()
    size = os.path.getsize(path)
    percent = size * 100 // args.limit
    print(f"{path}: {size} bytes, {percent}% of the {args.limit} byte budget")
    if size > args.limit:
        print("over budget: shrink something, or raise the budget in a PR that says why")
        return 1
    if size > args.limit * WARN_FRACTION:
        print(f"warning: above {WARN_FRACTION:.0%} of the budget")
    return 0


if __name__ == "__main__":
    sys.exit(main())
