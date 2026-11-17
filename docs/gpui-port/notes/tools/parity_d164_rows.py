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

SE = "workbench::tests::proofs41_e2e_search_everywhere::"
SL = "workbench::tests::proofs41_e2e_search_layout::"
RE = "workbench::tests::proofs41_recent_edited::"
ST = "everywhere::view::proofs41_stale::"
keys_tests = under(SE, "labels_the_actions_tab_s_checkbox_and_assigns_a_shortcut_with_alt_enter", "cmd_enter_on_text_opens_find_in_files_with_the_query", "cmd_enter_off_the_text_tab_opens_the_selection", "alt_up_and_alt_down_browse_the_tab_s_search_history_and_restore_the_draft")
addcite("point.registerKeyClaim:(at) src/features/search/everywhere/SearchEverywhere.tsx:86", keys_tests,
    "Since 2026-11-17 the claimed keys act: ⌥↵ on an action row assigns a shortcut, ⌘↵ on Text opens Find in Files with the query and elsewhere opens the selection, ⌥↑ and ⌥↓ walk the tab's earlier queries; still missing: the ⌥↵ menu on rows that are not actions (`src/features/search/everywhere/SearchEverywhere.tsx:364-390`).")
addcite("point.registerOverlay:search.everywhere",
    keys_tests + under(SE, "searches_everywhere_when_nothing_is_found_in_the_project", "a_scope_chosen_by_hand_says_nothing_was_found_in_the_project_files", "actions_run_from_the_popup_are_recent_and_the_footer_describes_the_selection", "two_project_windows_share_the_search_history_and_the_recent_actions", "tab_cycles_scopes_and_repeating_the_scope_action_toggles_non_project_items", "a_toggled_action_run_from_the_popup_remembers_the_query_while_it_stays_open")
    + under(SL, "mixed_file_and_action_rows_align_icons_and_names_and_keep_shortcuts_readable", "tabs_and_query_fit_narrow_windows", "without_the_preview_the_results_list_shows_its_rows") + under(ST, "a_late_answer_of_an_old_query_is_not_shown", "a_late_wide_answer_of_an_old_query_is_not_shown_after_a_new_query"),
    "Since 2026-11-17 also: the Include non-project items box with the automatic widening when the project has nothing, the tab's query history and its last query on opening, recent actions, shortcuts beside actions, the footer describing the selection, the webview's sizes and the header wrapping in a narrow window; a late answer is never shown, also across the widening. Of the earlier missing list these are now built; still missing: the preview pane, the funnel and file-set scope, the Recent Searches menu, multi-selection, Open to the Side, the ⌥↵ menu on other rows, the Open in Find Tool Window button, language-server symbols and endpoints, refresh when indexing ends, toggle switches in action rows, syntax colours of text matches, folder results.")
for k in ("action:navigate.searchEverywhere", "action:navigate.gotoFile", "action:navigate.gotoSymbol", "action:navigate.findAction", "action:navigate.searchText"):
    addcite(k, under(SE, "tab_cycles_scopes_and_repeating_the_scope_action_toggles_non_project_items"), "Asked for again on the shown tab, the action toggles the scope box, as the webview's does.")
for k in ("action:navigate.recentFiles", "action:navigate.switcher", "point.registerOverlay:search.recent"):
    addcite(k, under(RE, "recent_files_asked_for_again_lists_only_the_files_edited_in_this_window", "the_edited_files_of_the_project_before_are_not_listed_after_another_is_opened"), "Show edited only lists the files edited in this window and forgets them when the window leaves the project.")
row("unit:src/features/search/__tests__/dialogs.test.tsx", "partial",
    under(SE, "alt_up_and_alt_down_browse_the_tab_s_search_history_and_restore_the_draft", "labels_the_actions_tab_s_checkbox_and_assigns_a_shortcut_with_alt_enter", "searches_everywhere_when_nothing_is_found_in_the_project", "tab_cycles_scopes_and_repeating_the_scope_action_toggles_non_project_items"),
    "8 of 23 cases in full and 3 in part (an integrator's reading of each case against its test); not covered: the refresh after indexing, the preview, the funnel filters, the nine multi-selection cases (extend, open several, copy, per operating system), Find in Files' shortcuts spelled per operating system; in part: the scope cycling (an editor selection ignored while the popup is open is not exercised), the switcher (not the focus order), replace while streaming (not Replace All's enabled state)")
new("e2e:e2e/searchEverywhereLayout.spec.ts", "partial",
    under(SL, "without_the_preview_the_results_list_shows_its_rows", "tabs_and_query_fit_narrow_windows", "mixed_file_and_action_rows_align_icons_and_names_and_keep_shortcuts_readable"),
    "3 of 5 cases as native view tests on element bounds with the webview's numbers (the list's rows without the preview, the tabs and query in narrow windows in English and Chinese at normal scale, icons and names aligned with shortcuts readable); not covered: the two preview cases (the preview pane is not built) and the 125% browser zoom, which has no native counterpart")
json.dump(p, open(path, "w"), indent=2, ensure_ascii=False); open(path, "a").write("\n")
print("ok")
