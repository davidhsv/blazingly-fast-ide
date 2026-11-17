#!/usr/bin/python3 -I
"""Aggregates <base>/runs/*/ into one Markdown table per scenario: median [min - max] over the
repetitions of each app, the spread, and native / Tauri of the medians.

usage: aggregate.py --base B [--dry-run]

Definitions (DESIGN.md, part 4):
- peak: the largest 1 Hz sample of the summed phys_footprint (resp. resident size) of the set
  within the scenario; "lifetime max" also sums each process's own ri_lifetime_max_phys_footprint
  at the end of the launch (an upper bound of the peak: the processes need not peak together).
- steady: the median of the summed footprint over the scenario's last 10 samples (idle: last 60).
- CPU: the increase of the summed user+system time of the set over the scenario (processes that
  exited keep their last reading), in CPU seconds; idle CPU = that / the idle duration, in % of one core.
"""
from __future__ import annotations

import argparse
import glob
import json
import os
import re
import statistics
import sys

SCENARIOS = ["cold_start", "cold_steady", "open_project", "open_files", "find_in_files", "git_log", "deep_mode", "blaze_again", "idle"]
MiB = 1024 * 1024


def load_samples(path):
    with open(path, encoding="utf-8") as f:
        return [json.loads(l) for l in f if l.strip()]


def group(kind: str) -> str:
    return "webkit" if kind.startswith("webkit") else ("child" if kind == "child" else "app")


def scenario_metrics(samples, name):
    s = [x for x in samples if x["scenario"] == name]
    if len(s) < 2:
        return None
    tail = s[-60:] if name == "idle" else s[-10:]
    dur = s[-1]["t"] - s[0]["t"]
    cpu = (s[-1]["cpu_ns_total"] - s[0]["cpu_ns_total"]) / 1e9
    wk = s[-1]["wakeups_total"] - s[0]["wakeups_total"]
    by = {}
    for x in s:
        sums = {}
        for p in x["procs"]:
            g = group(p["kind"])
            sums[g] = sums.get(g, 0) + p["footprint"]
        for g, v in sums.items():
            by[g] = max(by.get(g, 0), v)
    return {
        "duration_s": dur,
        "peak_footprint_mib": max(x["sum_footprint"] for x in s) / MiB,
        "peak_resident_mib": max(x["sum_resident"] for x in s) / MiB,
        "steady_footprint_mib": statistics.median(x["sum_footprint"] for x in tail) / MiB,
        "cpu_s": cpu,
        "cpu_pct_core": 100.0 * cpu / dur if dur > 0 else 0.0,
        "wakeups_per_s": wk / dur if dur > 0 else 0.0,
        "procs_max": max(len(x["procs"]) for x in s),
        **{f"peak_{g}_mib": v / MiB for g, v in by.items()},
    }


def lifetime_upper_bound(samples):
    last = {}
    for x in samples:
        for p in x["procs"]:
            last[(p["pid"], p["kind"])] = p["lifetime_max"]
    return sum(last.values()) / MiB


def waits(events_path):
    out = {}
    for l in open(events_path, encoding="utf-8"):
        e = json.loads(l)
        if e.get("event") == "waited" and e.get("launch"):
            key = f"{e['launch']}: {e['what']}"
            if key not in out:
                out[key] = e["seconds"]
    return out


def time_l(path):
    """Fields of /usr/bin/time -l for the launcher's child (the app process alone, not its helpers)."""
    try:
        text = open(path).read()
    except OSError:
        return {}
    out = {}
    m = re.search(r"([\d.]+)\s+real\s+([\d.]+)\s+user\s+([\d.]+)\s+sys", text)
    if m:
        out.update(real_s=float(m.group(1)), user_s=float(m.group(2)), sys_s=float(m.group(3)))
    m = re.search(r"(\d+)\s+maximum resident set size", text)
    if m:
        out["maxrss_mib"] = int(m.group(1)) / MiB
    m = re.search(r"(\d+)\s+peak memory footprint", text)
    if m:
        out["peak_footprint_mib"] = int(m.group(1)) / MiB
    return out


def fmt(vals):
    if not vals:
        return "-"
    med = statistics.median(vals)
    spread = (max(vals) - min(vals)) / med * 100 if med else 0.0
    return f"{med:.1f} [{min(vals):.1f}-{max(vals):.1f}] ±{spread:.0f}%"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", required=True)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--include-degraded", action="store_true", help="also count runs with timeouts, warnings or findings")
    a = ap.parse_args()
    runs = sorted(glob.glob(f"{a.base}/runs/*-*"))
    if a.dry_run:
        print(f"[dry-run] would aggregate {len(runs)} runs under {a.base}/runs")
        return
    table = {}  # (scenario, metric) -> app -> [values]
    extra = {}  # (label) -> app -> [values]
    status = []
    for r in runs:
        rep, app = os.path.basename(r).split("-", 1)
        st = json.load(open(f"{r}/status.json")) if os.path.exists(f"{r}/status.json") else {"status": "no status"}
        findings = {}
        for l in open(f"{r}/events.jsonl", encoding="utf-8") if os.path.exists(f"{r}/events.jsonl") else []:
            e = json.loads(l)
            if e.get("event") in ("timeout", "file_not_opened", "mode_not_switched", "warning", "abort"):
                findings[e["event"]] = findings.get(e["event"], 0) + 1
            if e.get("event") == "stopped":
                for k in ("network", "left_running", "unreadable"):
                    if e.get(k):
                        findings[k] = findings.get(k, 0) + len(e[k])
        status.append((rep, app, st["status"], findings))
        if st["status"] != "ok" and not (a.include_degraded and st["status"] == "degraded"):
            continue
        for launch in ("cold", "project"):
            path = f"{r}/samples-{launch}.jsonl"
            if not os.path.exists(path):
                continue
            samples = load_samples(path)
            for sc in SCENARIOS:
                m = scenario_metrics(samples, sc)
                for k, v in (m or {}).items():
                    table.setdefault((sc, k), {}).setdefault(app, []).append(v)
            extra.setdefault(f"{launch}: sum of per-process lifetime max footprint (MiB)", {}).setdefault(app, []).append(lifetime_upper_bound(samples))
            for k, v in time_l(f"{r}/time-{launch}.txt").items():
                extra.setdefault(f"{launch}: /usr/bin/time -l, app process only: {k}", {}).setdefault(app, []).append(v)
        for k, v in waits(f"{r}/events.jsonl").items():
            extra.setdefault(f"wait (s) {k}", {}).setdefault(app, []).append(v)
    print("# Measurement results\n")
    print("| run | status | findings (timeouts, files not opened, mode not switched, warnings, network lines, left running, unreadable) |\n|---|---|---|")
    for rep, app, s, f in status:
        print(f"| {rep}-{app} | {s} | {', '.join(f'{k} {v}' for k, v in sorted(f.items())) or 'none'} |")
    print(f"\nCounted: runs with status ok{' or degraded' if a.include_degraded else ''}.\n")
    print("Each cell: median [min-max] ±spread over the successful repetitions. Read DESIGN.md's fairness caveats beside every number.\n")
    for sc in SCENARIOS:
        rows = [(k, v) for (s, k), v in table.items() if s == sc]
        if not rows:
            continue
        print(f"## {sc}\n\n| metric | native | Tauri | native / Tauri |\n|---|---|---|---|")
        for k, v in sorted(rows):
            n, t = v.get("native", []), v.get("tauri", [])
            ratio = f"{statistics.median(n) / statistics.median(t):.2f}" if n and t and statistics.median(t) else "-"
            print(f"| {k} | {fmt(n)} | {fmt(t)} | {ratio} |")
        print()
    print("## Timings and cross-checks\n\n| what | native | Tauri |\n|---|---|---|")
    for k, v in sorted(extra.items()):
        print(f"| {k} | {fmt(v.get('native', []))} | {fmt(v.get('tauri', []))} |")


if __name__ == "__main__":
    sys.exit(main())
