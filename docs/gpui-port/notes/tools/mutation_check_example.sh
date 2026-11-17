#!/bin/bash
# Fail-first check by mutation: each safety test must fail when its guard is switched off.
S=${S}
W=${W}
F=$W/app-gpui/src/workbench/log_actions.rs
cd $W/app-gpui || exit 1
cp $F $S/mut2a/log_actions.orig
before=$(shasum $F | cut -d' ' -f1)
mutate() { python3 - "$1" <<'PY'
import sys
S="${S}"
F="${W}/app-gpui/src/workbench/log_actions.rs"
t=open(f"{S}/mut2a/log_actions.orig").read()
if sys.argv[1]=="A":
    old="    Ok(protected_branch_problem(head, upstream, |name| is_protected_branch(name, &patterns)))\n"
    new="    let _ = protected_branch_problem(head, upstream, |name| is_protected_branch(name, &patterns));\n    Ok(None)\n"
else:
    old="""        let request = ConfirmRequest::danger(&title, pushed_warning(upstream, &title), "Continue");
        if !confirm(this, cx, request).await? {
            return Ok(None);
        }
"""
    new="""        let request = ConfirmRequest::danger(&title, pushed_warning(upstream, &title), "Continue");
        let _ = confirm(this, cx, request).await?;
"""
assert t.count(old)==1, sys.argv[1]
open(F,"w").write(t.replace(old,new))
PY
}
mutate A && { cargo test --lib refuses_every_rewrite_of_pushed_commits_of_a_protected_branch 2>&1 | grep -E '^test |^test result|panicked|error(\[|:)' | head -12; } > $S/mut2a/A.log
mutate B && { cargo test --lib asks_before_rewriting_pushed_commits_and_rewrites_nothing_when_declined 2>&1 | grep -E '^test |^test result|panicked|error(\[|:)' | head -12; } > $S/mut2a/B.log
cp $S/mut2a/log_actions.orig $F
after=$(shasum $F | cut -d' ' -f1)
echo "restored: $([ "$before" = "$after" ] && echo yes || echo NO)" > $S/mut2a/restored.txt
