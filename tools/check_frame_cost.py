#!/usr/bin/env python3
"""Fail when the reference scene got more than 10% costlier (M3.8).

Runs the headless frame benchmark (one worker, so the figure does not depend
on the runner's core count) and compares its ratio of frame cost to the
calibration loop with docs/perf/baseline.json. The ratio is machine
independent to first order; absolute milliseconds are not gated here.

  python3 tools/check_frame_cost.py            # check against the baseline
  python3 tools/check_frame_cost.py --update   # rewrite the baseline
  python3 tools/check_frame_cost.py --result run.json   # check a saved run
  python3 tools/check_frame_cost.py --selftest
"""

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = ROOT / "docs" / "perf" / "baseline.json"
TOLERANCE = 0.10
BENCH = [
    "cargo", "run", "--release", "-q", "-p", "mscent-render",
    "--example", "frame_bench", "--", "--workers", "1", "--json",
]


def run_bench():
    out = subprocess.run(BENCH, cwd=ROOT, check=True, capture_output=True, text=True)
    return json.loads(out.stdout.strip().splitlines()[-1])


def verdict(measured, baseline, tolerance=TOLERANCE):
    """Return (ok, message) for a measured ratio against the baseline ratio."""
    limit = baseline * (1.0 + tolerance)
    change = (measured / baseline - 1.0) * 100.0
    msg = f"ratio {measured:.4f}, baseline {baseline:.4f} ({change:+.1f}%), limit {limit:.4f}"
    return measured <= limit, msg


def selftest():
    assert verdict(8.0, 8.0)[0]
    assert verdict(8.79, 8.0)[0]
    assert not verdict(8.81, 8.0)[0]
    assert verdict(5.0, 8.0)[0], "faster must pass"
    print("selftest ok")
    return 0


def main(argv):
    if "--selftest" in argv:
        return selftest()
    if "--result" in argv:
        result = json.loads(Path(argv[argv.index("--result") + 1]).read_text())
    else:
        result = run_bench()
    if "--update" in argv:
        BASELINE.parent.mkdir(parents=True, exist_ok=True)
        data = {"ratio": round(result["ratio"], 4), "tolerance": TOLERANCE,
                "ms_per_frame": result["ms_per_frame"],
                "calibration_ms": result["calibration_ms"]}
        BASELINE.write_text(json.dumps(data, indent=2) + "\n")
        print(f"baseline written: ratio {data['ratio']}")
        return 0
    base = json.loads(BASELINE.read_text())
    ok, msg = verdict(result["ratio"], base["ratio"], base.get("tolerance", TOLERANCE))
    print(("ok: " if ok else "FAIL: frame cost regressed, ") + msg)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
