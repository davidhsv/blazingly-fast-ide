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

new("point.registerMenuGroup:search.structure.selectIn", "done", names("workbench::project_view_actions::tests::select_in_lists_the_places_that_show_the_file_and_the_project_view_selects_it"),
    "Select In lists Structure for the edited file between Project View and the file manager, and choosing it shows the Structure tool window")
new("point.registerMenuGroup:http.new", "done", names("workbench::project_view_actions::tests::menu_groups::new_http_request_in_a_folders_project_view_menu_creates_the_request_file_there"),
    "New › HTTP Request in a folder's Project view menu asks for the name and creates the request file there with the example request")
new("point.registerMenuGroup:debugger.evaluate", "done", names("workbench::run_context::tests::editor_menu_evaluate::the_editors_menu_offers_evaluate_expression_while_the_session_is_paused"),
    "the editor's menu offers Evaluate Expression only while a session is paused, and the row opens the Evaluate dialog")
new("action:debug", "partial", names("workbench::run_context::tests::services_debug::debug_on_the_services_toolbar_debugs_the_selected_configuration"),
    "Debug on the Services toolbar debugs the selected configuration (the session stops at its breakpoint); missing: Rerun in the debugger from Services only logs (`src/features/run/services.tsx:624`)")
new("ipc:plugin_run_targets", "done", names("workbench::tests::run_targets_plugins::run_targets_lists_the_target_a_plugin_registered_for_the_project"),
    "Settings › Run Targets lists the read-only target a plugin registered for the project, through the plugin host's run-targets listing, which is the call the adapter's function makes")
addcite("ipc:plugin_subscribe", names("workbench::plugin_runtime::tests::proofs10_unsubscribe::a_file_the_window_opens_reaches_the_plugin_subscribed_to_file_opened"), "A file the window opens reaches a plugin subscribed to file-opened, and no more once it unsubscribed (the test fails without the unsubscribe).")
addcite("unit:src/editor/core/cm/interpolation.test.ts", under("editor::view::tests::proofs_editor_highlight_interpolation_templates::", "keeps_nested_braces_strings_and_comments_inside_expressions", "handles_kotlin_multiline_raw_templates_and_nested_templates"))
json.dump(p, open(path, "w"), indent=2, ensure_ascii=False); open(path, "a").write("\n")
print("ok")
