#!/usr/bin/env python3
"""Fail if a release game binary is over its size budget.

Nothing else watches binary size, and a single dependency can quadruple it
(docs/scripting-language.md). Over budget means shrink something; raising a
budget is a decision, recorded in the PR that does it.

Two builds are measured: the software-only game, as LazyOS builds it, and the
default one, which links the hardware renderer (docs/hardware-renderer.md).

Run from the repo root. Honours CARGO_TARGET_DIR. `--binary` measures an
existing file without building, against `--limit` or the software budget.
"""
import argparse
import os
import subprocess
import sys

# 1.6 MB until voxel volumes, which took the game from 1,576,960 to 1,885,184 bytes.
# The texture generator adds about 140 KB, and replaced 902 KB of PNGs in the assets.
BUDGET_BYTES = 2_200_000
# wgpu and the hardware renderer (M14.7) took the default game from 2,126,336 to
# 5,730,304 bytes; the plan expected about 5.7 MB.
HARDWARE_BUDGET_BYTES = 6_200_000
WARN_FRACTION = 0.8

# Each build: its name, its cargo feature arguments, its own target directory so
# that the two do not rebuild each other, and its budget.
BUILDS = [
    ("software only", ["--no-default-features", "--features", "desktop"], "soft", BUDGET_BYTES),
    ("default, with hardware", [], None, HARDWARE_BUDGET_BYTES),
]


def binary(subdir):
    target = os.environ.get("CARGO_TARGET_DIR", "target")
    if subdir:
        target = os.path.join(target, subdir)
    suffix = ".exe" if os.name == "nt" else ""
    return os.path.join(target, "release", f"miniscent{suffix}")


def build(features, subdir):
    command = ["cargo", "build", "--release", "-q", "-p", "miniscent-game", *features]
    if subdir:
        command += ["--target-dir", os.path.dirname(os.path.dirname(binary(subdir)))]
    subprocess.run(command, check=True)


def measure(name, path, limit):
    size = os.path.getsize(path)
    percent = size * 100 // limit
    print(f"{name}: {path}: {size} bytes, {percent}% of the {limit} byte budget")
    if size > limit:
        print("over budget: shrink something, or raise the budget in a PR that says why")
        return 1
    if size > limit * WARN_FRACTION:
        print(f"warning: above {WARN_FRACTION:.0%} of the budget")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", help="measure this file instead of building the game")
    parser.add_argument("--limit", type=int, default=BUDGET_BYTES, help="budget in bytes")
    args = parser.parse_args()
    if args.binary is not None:
        return measure("given", args.binary, args.limit)
    failed = 0
    for name, features, subdir, limit in BUILDS:
        build(features, subdir)
        failed |= measure(name, binary(subdir), limit)
    return failed


if __name__ == "__main__":
    sys.exit(main())
