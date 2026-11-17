#!/usr/bin/env python3
"""Assigns every open inventory row to exactly one bucket and writes the plan's data.

Usage: python3 -I slices.py <worktree>
Reads rows3.tsv (mark, section, folder, key, missing, owner, cases) next to this script, the
worktree's src/ (never edited by other agents) and `git ls-tree HEAD` for app-gpui paths.
Writes assignment.tsv and buckets.json, prints unmatched / multi-matched rows and checks paths.
"""
import json, os, re, subprocess, sys

ROOT = sys.argv[1]
HERE = os.path.dirname(os.path.abspath(__file__))

SEC = [
    ("Behaviour specification: unit", "unit"), ("Behaviour specification: end", "e2e"), ("Actions", "act"),
    ("IPC", "ipc"), ("Editor, platform", "mod"), ("Settings pages", "set"), ("Overlays", "ovl"),
    ("Context keys", "ctx"), ("Features", "feat"), ("Plugin API", "wit"), ("Tool windows", "tw"),
    ("Native shell", "shell"), ("Editor kinds", "kind"), ("Backend events", "evt"), ("Status bar", "stat"),
    ("Title bar", "title"), ("Keymap presets", "keymap"), ("Keymap ids", "boundid"), ("File editor", "fep"),
    ("Extension point", "pt"),
]


def abbr(section):
    for prefix, a in SEC:
        if section.startswith(prefix):
            return a
    raise SystemExit("unknown section " + section)


rows = []
for line in open(os.path.join(HERE, "rows3.tsv"), encoding="utf8"):
    mark, section, folder, key, missing, owner, cases = line.rstrip("\n").split("\t")
    point = re.search(r"`(register\w+)`", section)
    rows.append(dict(mark=mark, section=section, sec=abbr(section), pt=point.group(1) if point else "",
                     folder=folder, key=key, missing=missing, owner=owner, cases=int(cases or 0)))


def K(*keys, sec=None):
    keys = set(keys)
    return lambda r: r["key"] in keys and (sec is None or r["sec"] == sec)


def R(pattern, sec=None, owner=None, pt=None):
    rx = re.compile(pattern)
    orx = re.compile(owner) if owner else None
    return lambda r: (rx.fullmatch(r["key"]) is not None and (sec is None or r["sec"] in sec.split("|"))
                      and (orx is None or orx.fullmatch(r["owner"]) is not None) and (pt is None or r["pt"] == pt))


def U(*names):
    """Spec rows (unit or e2e) by path suffix."""
    return lambda r: r["sec"] in ("unit", "e2e") and any(r["key"].endswith(n) for n in names)


def M(*names):
    """Module rows by path suffix."""
    return lambda r: r["sec"] == "mod" and any(r["key"].endswith(n) for n in names)


def OWNER(owner, sec=None):
    rx = re.compile(owner)
    return lambda r: rx.fullmatch(r["owner"]) is not None and (sec is None or r["sec"] in sec.split("|"))


exec(open(os.path.join(HERE, "buckets_def.py"), encoding="utf8").read())  # defines BUCKETS

BUCKETS.sort(key=lambda b: bool(b.get("catchall")))  # catch-alls match last
assign, multi, unmatched = {}, [], []
for i, r in enumerate(rows):
    hits = [b["id"] for b in BUCKETS if any(rule(r) for rule in b["rules"])]
    if not hits:
        unmatched.append(r)
    else:
        assign[i] = hits[0]  # first bucket in BUCKETS order wins (specific before catch-all)
        if len([h for h in hits if not BY_ID[h].get("catchall")]) > 1:
            multi.append((r["section"], r["key"], hits))

# Splits: a multi-part bucket becomes writer-sized sub-buckets, each row in the first sub that takes it.
exec(open(os.path.join(HERE, "splits.py"), encoding="utf8").read())
new_buckets = []
for b in BUCKETS:
    if b["id"] not in SPLITS:
        new_buckets.append(b)
        continue
    subs = []
    for suffix, title, matcher in SPLITS[b["id"]]:
        sub = dict(b, id=f"{b['id']}-{suffix}", title=f"{b['title'].split(':')[0]}: {title}", parent=b["id"], web=[])
        sub.pop("write", None)
        sub["_match"] = matcher
        subs.append(sub)
    for i, bid in list(assign.items()):
        if bid == b["id"]:
            r = rows[i]
            assign[i] = next(s["id"] for s in subs if s["_match"] is None or s["_match"](r))
    for s in subs:
        s.pop("_match")
        if not any(rows[i]["sec"] in ("unit", "e2e", "mod") for i, bid in assign.items() if bid == s["id"]):
            s["web"] = list(b.get("web", []))
    new_buckets.extend(subs)
BUCKETS = new_buckets
BY_ID = {b["id"]: b for b in BUCKETS}

# Paths: webview files from the worktree, native files from HEAD.
native = set(subprocess.run(["git", "-C", ROOT, "ls-tree", "-r", "--name-only", "HEAD", "app-gpui/src"],
                            capture_output=True, text=True, check=True).stdout.split())
native_dirs = {p.rsplit("/", i)[0] for p in native for i in range(1, p.count("/"))}


def wc(path):
    full = os.path.join(ROOT, path)
    if os.path.isdir(full):
        total = 0
        for d, _, files in os.walk(full):
            for f in files:
                if re.search(r"\.(ts|tsx|rs)$", f) and ".test." not in f and "__tests__" not in d:
                    total += sum(1 for _ in open(os.path.join(d, f), encoding="utf8"))
        return total
    if os.path.isfile(full):
        return sum(1 for _ in open(full, encoding="utf8"))
    return None


def impl_of(spec):
    """The implementation file a spec is named after, if one exists beside it."""
    base = re.sub(r"\.test\.(tsx?)$", "", spec).replace("/__tests__/", "/")
    for ext in (".ts", ".tsx"):
        if os.path.isfile(os.path.join(ROOT, base + ext)):
            return base + ext
    return None


MISSING = {}
for line in open(os.path.join(HERE, "missing_by_spec.txt"), encoding="utf8"):
    n, _, spec = line.strip().partition(" ")
    if n.isdigit():
        MISSING[spec] = int(n)


def todo_cases(r):
    """Spec cases still to prove: all for a row with no record, a third for model-only rows (wiring),
    the count a proof or a re-verification report names for a partial row, else half."""
    if r["mark"] == "[ ]":
        return r["cases"]
    if r["mark"] == "[m]":
        return max(1, r["cases"] // 3)
    if r["key"] in MISSING:
        return MISSING[r["key"]]
    m = re.search(r"(\d+) of (\d+) cases", r["missing"])
    if m:
        return max(1, int(m.group(2)) - int(m.group(1)))
    return max(1, r["cases"] // 2)


def row_weight(r):
    """Lines per non-spec row (Hypothesis): an unbuilt user-facing row costs more than an unbuilt plumbing
    row, and both far more than a partial or model-only row whose code exists."""
    if r["mark"] == "[ ]":
        return 120 if r["sec"] in ("act", "ovl", "tw", "kind", "set", "stat", "title", "fep") else 60
    return 30


problems = []
out = []
for b in BUCKETS:
    mine = [rows[i] for i, bid in assign.items() if bid == b["id"]]
    specs = sorted({(r["key"], r["cases"]) for r in mine if r["sec"] in ("unit", "e2e")})
    web = list(dict.fromkeys(b.get("web", []) + [r["key"] for r in mine if r["sec"] == "mod"]
                             + [x for x in (impl_of(s) for s, _ in specs if s.startswith("src/")) if x]))
    web_lines = {}
    for w in web:
        n = wc(w)
        if n is None:
            problems.append(f"{b['id']}: webview path missing: {w}")
        web_lines[w] = n or 0
    for n in b.get("native", []):
        p = "app-gpui/src/" + n
        if p not in native and p not in native_dirs:
            problems.append(f"{b['id']}: native path missing at HEAD: {p}")
    spec_lines = sum(wc(s) or 0 for s, _ in specs)
    cases = sum(c for _, c in specs)
    todo = sum(todo_cases(r) for r in mine if r["sec"] in ("unit", "e2e"))
    read = sum(web_lines.values()) + spec_lines
    dir_lines = sum(n for w_, n in web_lines.items() if os.path.isdir(os.path.join(ROOT, w_)))
    write = b.get("write") or int((sum(web_lines.values()) - dir_lines) * 0.35 + dir_lines * 0.05 + todo * 18 + sum(row_weight(r) for r in mine if r["sec"] not in ("unit", "e2e", "feat")))
    out.append(dict(b, todo=todo, rows=len(mine), closes=sum(1 for r in mine), specs=specs, cases=cases,
                    web_lines=web_lines, spec_lines=spec_lines, read=read, write_est=write,
                    by_sec={s: sum(1 for r in mine if r["sec"] == s) for s in sorted({r["sec"] for r in mine})},
                    keys=[f"{r['sec']}:{r['key']}" for r in mine], rules=None))

with open(os.path.join(HERE, "assignment.tsv"), "w", encoding="utf8") as f:
    for i, r in enumerate(rows):
        f.write("\t".join([assign.get(i, "UNMATCHED"), r["mark"], r["sec"], r["owner"], r["key"]]) + "\n")
with open(os.path.join(HERE, "buckets.json"), "w", encoding="utf8") as f:
    json.dump(out, f, indent=1, ensure_ascii=False)

print(f"rows {len(rows)} assigned {len(assign)} unmatched {len(unmatched)} multi {len(multi)}")
for r in unmatched[:400]:
    print("UNMATCHED", r["sec"], r["owner"], r["key"])
for m in multi[:200]:
    print("MULTI", *m)
for p in problems:
    print("PATH", p)
