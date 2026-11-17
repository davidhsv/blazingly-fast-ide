#!/usr/bin/env python3
"""Compares two Cargo.lock files: packages both use, and those at different versions.
usage: lockdiff.py <root Cargo.lock> <app-gpui Cargo.lock>"""
import re, sys

def load(path):
    text = open(path, encoding="utf-8").read()
    pkgs = {}
    for block in text.split("[[package]]")[1:]:
        name = re.search(r'^name = "([^"]+)"', block, re.M)
        ver = re.search(r'^version = "([^"]+)"', block, re.M)
        if name and ver:
            pkgs.setdefault(name.group(1), set()).add(ver.group(1))
    return pkgs

a, b = load(sys.argv[1]), load(sys.argv[2])
shared = sorted(set(a) & set(b))
differ = [n for n in shared if a[n] != b[n]]
ide = [n for n in shared if n.startswith("ide-")]
print(f"packages: root {len(a)}, app-gpui {len(b)}, shared {len(shared)}, shared at different versions {len(differ)}")
print(f"backend ide-* crates shared: {len(ide)}")
for n in differ:
    print(f"  {n}: root {sorted(a[n])} app-gpui {sorted(b[n])}")
