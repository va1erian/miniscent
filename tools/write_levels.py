#!/usr/bin/env python3
"""Writes the built-in levels (showcase, corridor) to assets/levels/*.msl and
bakes their lightmaps to assets/levels/*.msl.lit (git-ignored caches).

Usage: python tools/write_levels.py [out_dir]
"""
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
cmd = ["cargo", "run", "--release", "-p", "miniscent-game", "--example", "write_levels"]
if len(sys.argv) > 1:
    cmd += ["--", sys.argv[1]]
code = subprocess.call(cmd, cwd=root)
if code == 0:
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "assets" / "levels"
    for name in ("showcase", "corridor"):
        bake = ["cargo", "run", "--release", "-p", "miniscent-game", "--",
                "--bake", str(out / f"{name}.msl")]
        code = code or subprocess.call(bake, cwd=root)
sys.exit(code)
