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

AA = "workbench::app_actions::tests::"
row("ipc:settings_export", "done", under(AA, "export_settings_moves_mcp_server_values_to_the_secret_store_and_writes_only_their_names"),
    before_missing("ipc:settings_export") + "; MCP servers' values in the user settings go to the secret store first, as the adapter does, so the exported file holds their names only (the test fails without the call)")
addcite("ipc:settings_import", under(AA, "import_settings_moves_mcp_server_values_out_of_the_user_settings"), "An imported MCP server value leaves the user settings for the secret store at once.")
row("ipc:doc_set_encoding", "partial", names("workbench::tests::reload_with_encoding_shows_the_byte_order_mark_state_the_backend_answers"),
    "Reload with Encoding and Convert go through ide_host::ops::doc::doc_set_encoding as the adapter does, and the encoding item shows the byte-order-mark state of its answer; not proven: the re-read of a virtual document (no native code opens a library source, a server's document or a revision as an editor tab, so that branch is not reached); missing: the UTF-8 BOM entry of the encoding menu and Reload with Encoding disabled when the document cannot be re-read (src/editor/core/view/StatusItems.tsx:368-372)")
row("ipc:lsp_completion", "done", names("editor_lsp::completion::tests::an_explicit_completion_bypasses_the_back_off_and_an_answer_clears_it", "editor_lsp::completion_view::tests::the_type_column_shows_the_label_description_before_the_detail"),
    before_missing("ipc:lsp_completion") + "; an explicit completion bypasses the document's back-off and an answer clears it (on the source the editor asks through, which passes the explicit flag), and the type column shows the item's label description before its detail")
row("ipc:window_close_context", "partial", names("workbench::tests::proofs13_close_guard::closing_the_last_project_window_closes_it_when_close_without_welcome_is_on"),
    before_missing("ipc:window_close_context") + ", or closes when general.closeWithoutWelcome is on; missing: general.closeTabsWithWindow drops the window's saved editor tabs on close and puts them back on a cancelled close (src/features/app/closeGuard.ts:204), which the native app does not read")
row("ipc:recent_projects", "done", names("workbench::tests::recent_live_branch::the_welcome_screen_shows_the_live_branch_of_a_recent_project"),
    before_missing("ipc:recent_projects") + "; each recent project shows the live branch read from its repository's HEAD file, as the adapter returns it")
row("ipc:plugin_market", "done", names("plugin_models::plugins_page::tests::a_marketplace_plugin_shows_its_publishers_name"),
    before_missing("ipc:plugin_market") + "; the publisher shown is the index's publisher name when it gives one (on the page's model)")
PT = "platform::paths::tests::"
row("unit:src/platform/paths.test.ts", "done", under(PT, "is_true_for_a_share_the_base_is_not_on", "reads_the_server_and_share_of_a_device_path", "is_false_on_the_bases_own_share_and_off_any_share", "is_true_for_a_share_the_text_names_itself_in_any_mix_of_slashes", "is_true_for_a_share_that_only_appears_once_the_path_is_resolved", "is_false_on_a_local_drive_and_on_the_share_the_text_is_on", "slashes_network_shares", "leaves_ui_paths_and_posix_paths_alone"),
    "9 of 9 cases by their names; the other-share check is called where the native app follows a path that text names: an agent conversation's file references and the Markdown preview's links, and the functions behind document links and hover links, which no window reaches yet")
row("unit:src/editor/lsp/hint.test.ts", "partial", names("editor_lsp::hint::tests::shows_a_slow_request_within_the_feedback_budget_and_removes_its_hint_on_completion"),
    "5 of 7 cases by their names; not covered: 'marks only progress as pending', 'puts what the hint answers in its data attributes'")
row("unit:src/platform/dateFormat.test.ts", "partial", names("platform::date_format::tests::keeps_the_time_of_dates_before_yesterday_as_ideas_pretty_date_time_does", "platform::date_format::tests::formats_like_the_locale_methods_it_replaces", "platform::date_format::tests::formats_another_date_with_the_same_cached_formatter"),
    "3 of 3 cases by their names; system formats are implemented for en-US only (Node prints `09:07 AM` with a plain space; what WebKit prints was not measured)")
json.dump(p, open(path, "w"), indent=2, ensure_ascii=False); open(path, "a").write("\n")
print("ok")
