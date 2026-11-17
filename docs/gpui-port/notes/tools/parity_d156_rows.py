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

row("unit:src/features/settings/__tests__/inspections.test.ts", "done", names("settings_model::inspections_model::tests::groups_servers_by_language_with_seen_and_configured_codes_built_ins_by_category"), "13 of 13 cases")
row("unit:src/features/settings/__tests__/pages.test.ts", "partial", names("settings_model::builtin_pages::tests::indexes_built_in_option_labels_for_search"),
    "5 of 7 cases; not covered: is the page titled like the query, not one that only mentions it; is none when no title starts with the query: choosing the page a search names (titledPage) does not exist natively")
AT = "debugger::attach::tests::"
row("unit:src/features/debugger/__tests__/attach.test.ts", "partial", under(AT, "attaches_the_chosen_unity_editor_by_its_exact_id_without_launching_or_using_a_native_adapter", "groups_processes_by_debugger_in_ideas_order_with_counts", "builds_the_attach_request_of_each_debugger"),
    "5 of 5 cases on the model; missing: no native view lists the attachable processes from this model (nothing calls it outside its tests), so the Attach popup's rows are not drawn")
row("unit:src/platform/keymaps/ideaParity.test.ts", "partial", names("platform::keymap::tests::lets_scoped_actions_win_shared_keys", "platform::keymap::tests::loads_the_written_out_presets_as_their_sources_declare_them"),
    "1 of 2 cases, and the other in part: the generated copy of three presets holds the keys their sources declare, in order; not covered: each IDEA action's key against IDEA's own keymap (%s), and the three presets built from spreads")
RM = "platform_more::remote::tests::"
row("unit:src/platform/remoteSettings.test.ts", "done", under(RM, "keeps_client_settings_writes_local_in_either_scope", "preserves_routing_for_host_settings", "preserves_client_keymaps_and_ui_settings_when_loading_remote_settings") + names("platform_more::client_settings::tests::keeps_every_client_setting_of_the_shared_model_and_nothing_else"),
    "16 of 16 cases; the client-only keys are the webview's 107, read from its list in the test and asserted one by one, so none of them is sent to a host")
row("unit:src/features/github/data/data.test.ts", "done", names("github::repository::tests::recognizes_github_com_remotes"), "24 of 24 cases")
row("unit:src/features/plugins/__tests__/decorations.test.ts", "partial", names("plugin_models::decorations::tests::filters_tree_badges_by_project_root_but_not_editor_annotations_and_drops_unknown_styles"),
    "3 of 7 cases on the model; not covered: renders plain annotations, maps them through edits, and withdraws them; shows a tree note after the name without colouring the file; resolves dynamic file and folder icons declared by the plugin; names the language server of an lsp grant in consent; nothing in the app draws these models yet")
row("unit:src/features/plugins/indexedMarkers.test.ts", "partial", names("plugin_models::indexed_markers::tests::keeps_markers_on_their_lines_while_typing_and_hides_them_when_the_entry_goes_away", "plugin_models::indexed_markers::tests::stays_before_text_typed_at_its_line_start_and_goes_when_its_line_joins_the_one_above"),
    "8 of 8 cases on the model; missing: nothing in the app draws indexed markers (the model has no caller outside its tests)")
row("unit:src/features/plugins/lineMarkerGutter.test.ts", "partial", names("plugin_models::line_markers::tests::refreshes_a_fallback_when_the_package_icon_declaration_arrives_after_its_markers"),
    "2 of 3 cases; not covered: withdraws old interactive markers on reload and does not refresh on idle status changes: the shared gutter icon area (one column for run icons, inheritance and plugin markers) does not exist natively")
row("unit:src/features/plugins/markerIcons.test.ts", "done", names("plugin_models::marker_icons::tests::retains_built_in_names_legacy_aliases_and_unknown_icon_fallback", "plugin_models::marker_icons::tests::falls_back_when_an_asset_fails_to_load"), "5 of 5 cases")
EP = "workbench::build_tools::tests::proofs21_execute_popup::"
row("unit:src/features/build-tools/ExecutePopup.test.tsx", "done",
    under(EP, "starts_page_up_on_a_completion_rather_than_a_section", "page_down_selects_a_completion_rather_than_a_section_at_its_landing_position", "shows_a_clicked_completion_in_the_input_instead_of_appending_it", "enter_runs_the_selected_completion_instead_of_completing_it", "a_double_click_on_a_completion_runs_its_line"),
    "10 of 10 cases, the three changed ones through the popup in a window: the field shows the selected row, Enter runs the shown line, a double click runs a row")
json.dump(p, open(path, "w"), indent=2, ensure_ascii=False); open(path, "a").write("\n")
print("ok")
