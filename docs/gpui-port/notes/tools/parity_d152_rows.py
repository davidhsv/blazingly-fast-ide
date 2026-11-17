import json, re
ROOT = "/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01"
S = "/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad"
path = f"{ROOT}/docs/gpui-port/parity.json"
p = json.load(open(path))
tests = set(open(f"{S}/rust_tests.txt").read().split())
def names(*ns):
    for n in ns: assert n in tests, n
    return list(ns)
def under(prefix, *leaves):
    return names(*[prefix + l for l in leaves])
def cite(ns): return ", ".join(f"`{n}`" for n in ns)
def alive(n): return n in tests or any(t.startswith(n + "::") for t in tests)
def cited(key):
    """The names an existing row cites that the build still has; a name that is gone is printed."""
    if key not in p: return []
    pr = p[key]["proof"]; i = pr.find(";")
    old = re.findall(r"`([^`]+)`", pr[:i] if i > 0 else pr)
    for n in old:
        if not alive(n): print("gone:", key, n)
    return [n for n in old if alive(n)]
def row(key, status, ns, text):
    assert key in p, key
    old = cited(key)
    p[key] = {"status": status, "proof": cite(old + [n for n in ns if n not in old]) + "; " + text}
def new(key, status, ns, text):
    assert key not in p, key
    p[key] = {"status": status, "proof": cite(ns) + "; " + text}

def sub(key, status, ns, old, new):
    """Keeps the row's citations and text, replacing the fragment `old` of the text by `new`."""
    assert key in p, key
    pr = p[key]["proof"]; i = pr.find(";")
    head, text = (pr[:i], pr[i + 1:]) if i > 0 else (pr, "")
    assert old in text, (key, old)
    kept = cited(key)
    proof = cite(kept + [n for n in ns if n not in kept]) + ";" + text.replace(old, new)
    p[key] = {"status": status, "proof": proof}


def recite(key, old_name, new_name):
    pass

def addcite(key, ns, extra=""):
    assert key in p, key
    pr = p[key]["proof"]; i = pr.find(";")
    head, text = (pr[:i], pr[i:]) if i > 0 else (pr, "")
    kept = cited(key)
    if extra: text = (text + " " + extra) if text else "; " + extra
    p[key]["proof"] = cite(kept + [n for n in ns if n not in kept]) + text

def before_missing(key):
    """The row's text up to its missing clause."""
    pr = p[key]["proof"]; i = pr.find(";"); text = pr[i + 1:].strip()
    j = text.find("missing:"); assert j > 0, key
    return text[:j].rstrip(" ;")

AU = "workbench::tests::proofs39_agent_undo::"
JV = "workbench::tests::proofs39_java_var::"
row("event:WorkspaceEditApplied", "partial",
    under(AU, "edit_undo_takes_back_an_agents_edit_over_two_files_and_redo_applies_it_again", "does_not_announce_an_agents_edit_as_a_refactoring", "reports_a_refused_undo_instead_of_discarding_newer_work", "an_agents_edit_made_before_any_editor_opened_is_undone_from_the_editor_opened_later", "each_agents_edit_is_remembered_once_before_and_after_an_editor_opens", "undoing_an_agents_edit_touches_only_the_files_it_changed_created_or_deleted", "an_undo_from_another_editor_keeps_newer_unsaved_typing_in_an_editor_of_the_edit", "typing_made_before_an_agents_edit_is_kept_but_no_longer_undoable"),
    "the rename's applied edit raises Refactoring applied with Undo, and after the undo Refactoring undone with Redo; the window remembers every applied edit from the moment the project opens, an agent's too (without the notification), so Edit › Undo takes it back over all its files and Redo applies it again, and a refused undo keeps every file. Missing: typing made in an open editor before a workspace edit can no longer be undone afterwards (the edit clears that editor's own history; the webview keeps it: src/editor/core/model/workspaceHistory.ts), and when another editor of the edit has newer unsaved typing the backend's refusal is shown where the webview says \"Undo newer editor changes before undoing this refactoring\" (workspaceHistory.ts:156)")
row("unit:src/features/code-insight/workspaceUndo.test.ts", "done", under(AU, "does_not_announce_an_agents_edit_as_a_refactoring", "reports_a_refused_undo_instead_of_discarding_newer_work"),
    "4 of 4 cases by their names")
row("unit:src/features/templates/__tests__/editor.test.ts", "done",
    under(JV, "runs_introduce_variable_on_the_expression_ended_as_a_statement", "falls_back_to_the_var_template_without_introduce_variable") + names("editor::intelligence::tests::proofs39_completion_snippet::leaves_enter_to_the_editor_in_a_completions_snippet", "templates::proofs39_completion_enter::leaves_enter_to_the_editor_in_a_completions_snippet"),
    "17 of 17 cases: the three added upstream by their names, through the app (Java .var runs Introduce Variable or falls back; Enter in a picked completion's snippet is the editor's)")
row("unit:src/features/templates/__tests__/postfix.test.ts", "done",
    names("templates::proofs39_zig_postfix::zig_optional_unwrap_and_expansion", "templates::postfix_model::tests::every_language_has_the_core_keys"),
    "10 of 10 cases; the Zig set is the webview's twelve templates in its order (compared by script)")
row("unit:src/features/templates/__tests__/live.test.ts", "done",
    names("templates::live_builtins::tests::ships_the_requested_built_ins", "templates::proofs39_zig_live::the_zig_group_holds_its_list_in_order"),
    "18 of 18 cases; the Zig group is the webview's nineteen templates in its order (compared by script)")
row("unit:src/features/templates/__tests__/surrounders.test.ts", "done",
    names("templates::proofs39_zig_surrounders::wraps_zig_in_its_parenthesised_brace_syntax"),
    "4 of 4 cases")
row("unit:src/features/templates/__tests__/emmet.test.ts", "partial", names("templates::proofs39_django_emmet::expands_markup_in_django_templates_not_inside_template_tags_variables_or_comments"),
    "5 of 8 cases by their names; not covered: expands abbreviations into snippets with fields, fills attribute placeholders and escapes snippet syntax, expands CSS properties and ignores unknown words: the expansions need the Emmet engine's tables, which are behind a trait with no implementation yet")
addcite("settings:editor.general.postfixCompletion", names("settings_view::pages::editor_postfix_completion::tests::proofs39_zig_tab::the_zig_tab_lists_the_zig_templates", "settings_view::pages::editor_postfix_completion::tests::proofs39_java_var_row::the_java_tab_describes_var_as_introduce_variable_does"))
addcite("settings:editor.liveTemplates", names("settings_view::pages::editor_live_templates::tests::proofs39_zig_group::the_page_lists_the_zig_templates"))
json.dump(p, open(path, "w"), indent=2, ensure_ascii=False); open(path, "a").write("\n")
print("ok")
