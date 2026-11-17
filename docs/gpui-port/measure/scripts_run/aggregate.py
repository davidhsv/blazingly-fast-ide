#!/usr/bin/python3 -I
"""Aggregates <runs root>/*-*/ into one Markdown table per scenario: median [min - max] over the
repetitions of each app, the spread, and native / Tauri of the medians. measure.py imports
`scenario_metrics` from here, so the driver's per-scenario numbers and the table share one definition.

usage: aggregate.py --base B [--runs-root R] [--include-degraded] [--dry-run]

Definitions (DESIGN.md, part 4), each split into app (the app process and its /usr/bin/time
launcher), WebKit helpers and children (language tools, git, java, mvn), with a total with and a
total without children:
- A scenario is every sample labelled with its name. It can have several contiguous segments: the
  keys that close it (the Escape after Find, the Cmd+9 that hides the Git log) and the settle after
  them come back to it after its tools_* snapshot window. Each segment's boundary is the last sample
  before it (none for a launch's first sample: then the launch itself is the boundary).
- CPU: per segment, for every process of the set, its cumulative user+system time at the segment's
  last sample minus its reading at the boundary. A process with no reading at the boundary counts
  from zero if it started inside the segment (its start time >= the boundary sample's time), else
  from its first reading. A process that exited keeps its last reading. The scenario's CPU is the
  sum over its segments; its duration is the sum of the segments' spans (boundary to last sample).
  Wakeups use the same rule.
- peak: the largest 1 Hz sample of the summed phys_footprint over all of the scenario's samples.
- steady: the median of the summed footprint over the last 10 samples (idle: last 60) of the
  scenario's first segment, the state it shows before its snapshot window and closing keys.
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
GROUPS = ("app", "webkit", "child")
MiB = 1024 * 1024


def load_samples(path):
    with open(path, encoding="utf-8") as f:
        return [json.loads(l) for l in f if l.strip()]


def group(kind: str) -> str:
    return "webkit" if kind.startswith("webkit") else ("child" if kind == "child" else "app")


def _key(p):
    # Older samples (the pilot) carry no start time: pid and kind then name the process.
    return (p["pid"], p.get("start_us", p["kind"]))


class Readings:
    """Cumulative readings of one launch's samples: for every sample index, each process's last
    reading at or before it, and the index at which each process was first read."""

    def __init__(self, samples):
        self.samples = samples
        self.last = []  # index -> {key: (cpu_ns, wakeups, group)}
        self.first = {}  # key -> (index, cpu_ns, wakeups, start_us or None)
        cur = {}
        for i, x in enumerate(samples):
            for p in x["procs"]:
                k = _key(p)
                cur[k] = (p["cpu_ns"], p["wakeups"], group(p["kind"]))
                if k not in self.first:
                    self.first[k] = (i, p["cpu_ns"], p["wakeups"], p.get("start_us"))
            self.last.append(dict(cur))

    def base(self, key, b):
        """(cpu_ns, wakeups, how) a process counts from in a segment whose boundary is index b."""
        if b is not None and key in self.last[b]:
            c, w, _g = self.last[b][key]
            return c, w, "boundary"
        _i, c, w, start = self.first[key]
        if b is None:
            return 0, 0, "launch"
        if start is None:
            # Hypothesis (no start time recorded): first read after the boundary = started inside.
            return 0, 0, "zero_no_start_time"
        if start / 1e6 >= self.samples[b]["t"]:
            return 0, 0, "zero_started_inside"
        return c, w, "first_reading"

    def span(self, b, e):
        """CPU (ns) and wakeups per group over (b, e]."""
        cpu = {g: 0 for g in GROUPS}
        wk = {g: 0 for g in GROUPS}
        for k, (c, w, g) in self.last[e].items():
            c0, w0, _how = self.base(k, b)
            cpu[g] += c - c0
            wk[g] += w - w0
        return cpu, wk


def segments(samples, name):
    """[(boundary index or None, first index, last index)] of the contiguous runs labelled `name`."""
    out, i = [], 0
    while i < len(samples):
        if samples[i]["scenario"] != name:
            i += 1
            continue
        j = i
        while j + 1 < len(samples) and samples[j + 1]["scenario"] == name:
            j += 1
        out.append((i - 1 if i > 0 else None, i, j))
        i = j + 1
    return out


def _group_sums(x):
    d = {g: 0 for g in GROUPS}
    for p in x["procs"]:
        d[group(p["kind"])] += p["footprint"]
    return d


def scenario_metrics(samples, name, readings=None):
    segs = segments(samples, name)
    if not segs or sum(e - s + 1 for _b, s, e in segs) < 2:
        return None
    r = readings or Readings(samples)
    dur = 0.0
    cpu = {g: 0 for g in GROUPS}
    wk = {g: 0 for g in GROUPS}
    for b, s, e in segs:
        dur += samples[e]["t"] - samples[b if b is not None else s]["t"]
        c, w = r.span(b, e)
        for g in GROUPS:
            cpu[g] += c[g]
            wk[g] += w[g]
    s_all = [samples[i] for _b, s, e in segs for i in range(s, e + 1)]
    _b0, s0, e0 = segs[0]
    first = samples[s0:e0 + 1]
    tail = first[-60:] if name == "idle" else first[-10:]
    pg = [_group_sums(x) for x in s_all]
    tg = [_group_sums(x) for x in tail]
    m = {
        "duration_s": dur,
        "samples": len(s_all),
        "segments": len(segs),
        "peak_resident_total_mib": max(x["sum_resident"] for x in s_all) / MiB,
        "peak_footprint_total_mib": max(x["sum_footprint"] for x in s_all) / MiB,
        "peak_footprint_total_without_children_mib": max(d["app"] + d["webkit"] for d in pg) / MiB,
        "steady_footprint_total_mib": statistics.median(x["sum_footprint"] for x in tail) / MiB,
        "steady_footprint_total_without_children_mib": statistics.median(d["app"] + d["webkit"] for d in tg) / MiB,
        "cpu_total_s": sum(cpu.values()) / 1e9,
        "cpu_total_without_children_s": (cpu["app"] + cpu["webkit"]) / 1e9,
        "cpu_pct_core": 100.0 * sum(cpu.values()) / 1e9 / dur if dur > 0 else 0.0,
        "wakeups_per_s": sum(wk.values()) / dur if dur > 0 else 0.0,
        "procs_max": max(len(x["procs"]) for x in s_all),
        "not_frontmost_samples": sum(1 for x in s_all if x.get("front_is_app") is False),
        "frontmost_unknown_samples": sum(1 for x in s_all if "front_is_app" in x and x["front_is_app"] is None),
    }
    for g in GROUPS:
        m[f"peak_footprint_{g}_mib"] = max(d[g] for d in pg) / MiB
        m[f"steady_footprint_{g}_mib"] = statistics.median(d[g] for d in tg) / MiB
        m[f"cpu_{g}_s"] = cpu[g] / 1e9
    return m


def self_check(samples, readings=None):
    """(per-label CPU summed over every label, the launch's total by the same rule) in s. The second
    is the driver's own running total (`cpu_ns_rule`, kept per sample) when the samples carry it;
    else (older samples) the sum of every process's last reading, which is what the rule gives
    when every process counts from zero."""
    r = readings or Readings(samples)
    labels = []
    for x in samples:
        if x["scenario"] not in labels:
            labels.append(x["scenario"])
    total = 0
    for name in labels:
        for b, _s, e in segments(samples, name):
            c, _w = r.span(b, e)
            total += sum(c.values())
    if "cpu_ns_rule" in samples[-1]:
        expected = samples[-1]["cpu_ns_rule"]
    else:
        expected = sum(c for c, _w, _g in r.last[-1].values())
    return total / 1e9, expected / 1e9


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
    if len(vals) == 1:
        return f"{med:.1f}"
    spread = (max(vals) - min(vals)) / med * 100 if med else 0.0
    return f"{med:.1f} [{min(vals):.1f}-{max(vals):.1f}] ±{spread:.0f}%"


ROWS = [
    ("duration (s)", "duration_s"),
    ("peak footprint MiB: app", "peak_footprint_app_mib"),
    ("peak footprint MiB: WebKit", "peak_footprint_webkit_mib"),
    ("peak footprint MiB: children", "peak_footprint_child_mib"),
    ("peak footprint MiB: total", "peak_footprint_total_mib"),
    ("peak footprint MiB: total without children", "peak_footprint_total_without_children_mib"),
    ("steady footprint MiB: app", "steady_footprint_app_mib"),
    ("steady footprint MiB: WebKit", "steady_footprint_webkit_mib"),
    ("steady footprint MiB: children", "steady_footprint_child_mib"),
    ("steady footprint MiB: total", "steady_footprint_total_mib"),
    ("steady footprint MiB: total without children", "steady_footprint_total_without_children_mib"),
    ("CPU s: app", "cpu_app_s"),
    ("CPU s: WebKit", "cpu_webkit_s"),
    ("CPU s: children", "cpu_child_s"),
    ("CPU s: total", "cpu_total_s"),
    ("CPU s: total without children", "cpu_total_without_children_s"),
    ("CPU % of one core (total)", "cpu_pct_core"),
    ("wakeups/s (total)", "wakeups_per_s"),
    ("peak resident MiB: total", "peak_resident_total_mib"),
    ("processes (max)", "procs_max"),
    ("samples not frontmost", "not_frontmost_samples"),
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", required=True)
    ap.add_argument("--runs-root", help="folder of the run folders (default: <base>/runs)")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--include-degraded", action="store_true", help="also count runs (and scenarios) marked degraded")
    a = ap.parse_args()
    root = a.runs_root or f"{a.base}/runs"
    runs = sorted(d for d in glob.glob(f"{root}/*-*") if os.path.isdir(d))
    if a.dry_run:
        print(f"[dry-run] would aggregate {len(runs)} runs under {root}")
        return
    table = {}  # (scenario, metric) -> app -> [values]
    extra = {}  # label -> app -> [values]
    status = []
    checks = []
    for r in runs:
        rep, app = os.path.basename(r).split("-", 1)
        st = json.load(open(f"{r}/status.json")) if os.path.exists(f"{r}/status.json") else {"status": "no status"}
        findings = {}
        for l in open(f"{r}/events.jsonl", encoding="utf-8") if os.path.exists(f"{r}/events.jsonl") else []:
            e = json.loads(l)
            if e.get("event") in ("timeout", "file_not_opened", "mode_not_switched", "warning", "abort", "idle_not_frontmost", "capture_failed"):
                findings[e["event"]] = findings.get(e["event"], 0) + 1
            if e.get("event") == "stopped":
                for k in ("network", "left_running", "unreadable"):
                    if e.get(k):
                        findings[k] = findings.get(k, 0) + len(e[k])
        bad_sc = set(st.get("degraded_scenarios", []))
        status.append((rep, app, st["status"], findings, sorted(bad_sc)))
        if st["status"] != "ok" and not (a.include_degraded and st["status"] == "degraded"):
            continue
        for launch in ("cold", "project"):
            path = f"{r}/samples-{launch}.jsonl"
            if not os.path.exists(path):
                continue
            samples = load_samples(path)
            rd = Readings(samples)
            got, want = self_check(samples, rd)
            checks.append(f"{os.path.basename(r)} {launch}: per-label CPU summed {got:.3f} s, launch total by the same rule {want:.3f} s{'' if abs(got - want) < 1e-6 else '  MISMATCH'}")
            for sc in SCENARIOS:
                if sc in bad_sc and not a.include_degraded:
                    continue
                m = scenario_metrics(samples, sc, rd)
                for k, v in (m or {}).items():
                    table.setdefault((sc, k), {}).setdefault(app, []).append(v)
            for k, v in time_l(f"{r}/time-{launch}.txt").items():
                extra.setdefault(f"{launch}: /usr/bin/time -l, app process only: {k}", {}).setdefault(app, []).append(v)
        for k, v in waits(f"{r}/events.jsonl").items():
            extra.setdefault(f"wait (s) {k}", {}).setdefault(app, []).append(v)
    print("# Measurement results\n")
    print(f"Runs root: `{root}`.\n")
    print("| run | status | scenarios degraded | findings |\n|---|---|---|---|")
    for rep, app, s, f, bad in status:
        print(f"| {rep}-{app} | {s} | {', '.join(bad) or 'none'} | {', '.join(f'{k} {v}' for k, v in sorted(f.items())) or 'none'} |")
    print(f"\nCounted: runs with status ok{' or degraded' if a.include_degraded else ''}; a scenario a run marks degraded (idle not frontmost) is left out of that run{' (here kept: --include-degraded)' if a.include_degraded else ''}.\n")
    print("Each cell: median [min-max] ±spread over the counted repetitions (one value when there is one). Read DESIGN.md's fairness caveats beside every number. Definitions: the docstring of aggregate.py.\n")
    print("The sum of each process's lifetime maximum footprint is no longer given: in the pilot the word read as `ri_lifetime_max_phys_footprint` was below the current footprint 31 times, so the bound it gave was wrong.\n")
    for sc in SCENARIOS:
        if not any(s == sc for (s, _k) in table):
            continue
        print(f"## {sc}\n\n| metric | native | Tauri | native / Tauri |\n|---|---|---|---|")
        for label, k in ROWS:
            v = table.get((sc, k), {})
            n, t = v.get("native", []), v.get("tauri", [])
            ratio = f"{statistics.median(n) / statistics.median(t):.2f}" if n and t and statistics.median(t) else "-"
            print(f"| {label} | {fmt(n)} | {fmt(t)} | {ratio} |")
        print()
    print("## Timings and cross-checks\n\n| what | native | Tauri |\n|---|---|---|")
    for k, v in sorted(extra.items()):
        print(f"| {k} | {fmt(v.get('native', []))} | {fmt(v.get('tauri', []))} |")
    print("\n## CPU self-check\n")
    for c in checks:
        print(f"- {c}")


if __name__ == "__main__":
    sys.exit(main())
