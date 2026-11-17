import json,re
S="/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad"
P="docs/gpui-port/parity.json"
low=json.load(open(f"{S}/dunwired/lowerings.json")); par=json.load(open(P))
tests=[l.strip() for l in open(f"{S}/rust_tests.txt")]; ts=set(tests)
def alive(n): return n in ts or any(t.startswith(n+"::") for t in tests)
assert len(low)==109
# The audit read the tree before the UI language was wired: that row's only missing point is fixed since.
skip={"module:src/platform/i18n.ts"}
n=0
for k,v in low.items():
    if k in skip: continue
    assert par[k]["status"]=="done", k
    assert set(v)=={"status","proof"} and v["status"]=="partial" and "missing:" in v["proof"], k
    c=re.findall(r"`([^`]+)`", v["proof"].split(";")[0]); assert c, k
    for t in c: assert alive(t), (k,t)
    par[k]=v; n+=1
open(P,"w").write(json.dumps(par,indent=2,ensure_ascii=False)+"\n")
print("lowered", n)
