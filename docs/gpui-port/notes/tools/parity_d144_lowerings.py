import json
S="/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad"
P="docs/gpui-port/parity.json"
low=json.load(open(f"{S}/dret1/lowerings.json")); par=json.load(open(P))
assert len(low)==96
for k,v in low.items():
    assert par[k]["status"]=="done", k
    assert v["status"]=="partial" and ";" in v["proof"], k
    par[k]={"status":"partial","proof":v["proof"]}
open(P,"w").write(json.dumps(par,indent=2,ensure_ascii=False)+"\n")
print("lowered", len(low))
