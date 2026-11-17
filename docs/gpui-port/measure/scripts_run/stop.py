#!/usr/bin/python3 -I
"""Emergency stop: every process a run recorded (events.jsonl "record" lines) that still runs with
the same start time and an executable under the base folder gets SIGTERM, then SIGKILL after 10 s.
Nothing else is signalled.

usage: stop.py --base B [--runs-root R ...] [--dry-run]   (default: B/runs; every R must be under B)
"""
from __future__ import annotations

import argparse
import glob
import json
import os
import signal
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import macos  # noqa: E402


def recorded(roots):
    for path in [p for r in roots for p in glob.glob(f"{r}/*/events.jsonl")]:
        for line in open(path, encoding="utf-8"):
            e = json.loads(line)
            if e.get("event") == "record":
                yield e["pid"], e["start_us"]


def alive(base, pid, start):
    info = macos.bsdinfo(pid)
    return bool(info) and info[2] == start and macos.pid_path(pid).startswith(base + "/")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", required=True)
    ap.add_argument("--runs-root", action="append", help="folder of run folders (repeatable; default: <base>/runs)")
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    base = os.path.realpath(a.base)
    roots = [os.path.realpath(r) for r in (a.runs_root or [f"{base}/runs"])]
    if any(not r.startswith(base + "/") for r in roots):
        sys.exit("every runs root must be under the base")
    targets = [(p, s) for p, s in set(recorded(roots)) if alive(base, p, s)]
    if not targets:
        print("nothing of ours is running")
        return 0
    for sig in (signal.SIGTERM, signal.SIGKILL):
        for p, s in targets:
            if alive(base, p, s):
                print(("[dry-run] " if a.dry_run else "") + f"kill -{int(sig)} {p} ({macos.pid_path(p)})")
                if not a.dry_run:
                    os.kill(p, sig)
        if a.dry_run:
            break
        time.sleep(10)
    left = [p for p, s in targets if alive(base, p, s)]
    print(f"still running: {left}" if left else "stopped")
    return 1 if left else 0


if __name__ == "__main__":
    sys.exit(main())
