#!/usr/bin/env python3
"""Writes the built-in levels (showcase, corridor) to assets/levels/*.msl.

Usage: python tools/write_levels.py [out_dir]
"""
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
cmd = ["cargo", "run", "--release", "-p", "miniscent-game", "--example", "write_levels"]
if len(sys.argv) > 1:
    cmd += ["--", sys.argv[1]]
sys.exit(subprocess.call(cmd, cwd=root))
