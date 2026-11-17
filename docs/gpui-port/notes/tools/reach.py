import os,re,json,sys
H=sys.argv[1]; R=H+'/app-gpui/src'
seen=set()
def strip_comments(t): return re.sub(r'//[^\n]*','',t)
def walk(f, moddir, inline_prefix=''):
    if f in seen or not os.path.exists(f): return
    seen.add(f)
    t=strip_comments(open(f).read())
    # handle inline mod blocks crudely: find `mod name {` and `mod name;` with optional #[path]
    for m in re.finditer(r'(#\[path\s*=\s*"([^"]+)"\]\s*)?(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+([a-z0-9_]+)\s*([;{])',t):
        path,name,kind=m.group(2),m.group(3),m.group(4)
        # nesting: compute inline module stack at this position
        pre=t[:m.start()]
        stack=[]
        for mm in re.finditer(r'mod\s+([a-z0-9_]+)\s*\{|\{|\}',pre):
            s=mm.group(0)
            if s.startswith('mod'): stack.append(mm.group(1))
            elif s=='{': stack.append(None)
            elif stack: stack.pop()
        inl=[x for x in stack if x]
        base=os.path.join(moddir,*inl)
        if kind=='{': continue
        if path:
            walk(os.path.normpath(os.path.join(os.path.dirname(f) if not inl else base, path)), os.path.dirname(os.path.normpath(os.path.join(os.path.dirname(f),path))))
            continue
        for c in (os.path.join(base,name+'.rs'), os.path.join(base,name,'mod.rs')):
            if os.path.exists(c):
                walk(c, os.path.join(base,name)); break
for root in ('lib.rs','main.rs'):
    walk(os.path.join(R,root), R)
allf=[os.path.join(dp,f) for dp,dn,fn in os.walk(R) for f in fn if f.endswith('.rs')]
un=sorted(os.path.relpath(f,H) for f in allf if f not in seen)
json.dump({'reached':sorted(os.path.relpath(f,H) for f in seen),'unreached':un},open(sys.argv[2],'w'),indent=0)
print(len(seen),'reached;',len(un),'unreached')
for u in un: print(' ',u)
