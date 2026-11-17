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

LA = "workbench::log_actions::tests::"
REFUSE = under(LA, "refuses_every_rewrite_of_pushed_commits_of_a_protected_branch", "asks_before_rewriting_pushed_commits_and_rewrites_nothing_when_declined", "the_commit_menu_offers_history_rewriting_for_a_pushed_commit_too")
PUSHED = "the Git Log menu offers it on a pushed commit too; a range that reaches the upstream asks first and rewrites nothing when declined, and on a protected branch with an upstream it is refused with the webview's message and nothing is rewritten (both tests fail with the guard switched off)"
for k in ("action:vcs.log.dropCommit", "action:vcs.log.editMessage", "action:vcs.log.interactiveRebase", "action:vcs.log.fixup"):
    row(k, "done", REFUSE, before_missing(k) + "; " + PUSHED)
row("action:vcs.log.squashInto", "partial", names(LA + "the_commit_menu_offers_history_rewriting_for_a_pushed_commit_too"),
    before_missing("action:vcs.log.squashInto") + "; the Git Log menu offers it on a pushed commit too; missing: no test drives Squash Into on a range that reaches the upstream (the question and the protected-branch refusal are read to be the ones Fixup goes through, not tested)")
row("action:vcs.log.undoCommit", "done", names(LA + "refuses_every_rewrite_of_pushed_commits_of_a_protected_branch"),
    before_missing("action:vcs.log.undoCommit") + "; Undo Commit of a pushed commit on a protected branch is refused and nothing is undone")
sub("action:vcs.ignoreLocal", "partial", names("vcs_ops::ignore::tests::opens_the_local_exclude_file_the_rule_was_written_to"),
    "; opens `.git/info/exclude` after writing (`src/features/vcs/ignore.ts:18`)", "; the exclude file is opened after the rule is written")
row("unit:src/features/vcs/__tests__/gitOps.test.ts", "done", names("vcs_ops::rebase_plan::tests::refuses_history_rewriting_on_a_protected_branch_with_an_upstream") + REFUSE[:1],
    "15 of 15 cases")
row("unit:src/features/vcs/__tests__/historyOps.test.ts", "done", names("vcs_ops::git_log::tests::offers_history_rewriting_for_pushed_commits_too_idea_allows_it_the_run_confirms") + REFUSE[1:],
    "10 of 10 cases")
row("unit:src/features/vcs/__tests__/ignore.test.ts", "done", names("vcs_ops::ignore::tests::opens_the_local_exclude_file_the_rule_was_written_to"), "7 of 7 cases")
row("unit:src/features/vcs/__tests__/branchFilterPopup.test.tsx", "done",
    names("vcs::log_filters::tests::lists_head_and_the_favorites_with_local_and_remote_collapsed_until_opened_no_remote_head", "vcs::log_filters::tests::shows_the_users_favorite_branches_local_and_remote_under_favorites_by_name"),
    "5 of 5 cases; not drawn: the favorite star and the indentation of branches inside a group")
SV = "run_testing::services::tests::"
row("unit:src/features/run/services.test.ts", "partial",
    under(SV, "lists_added_types_fully_and_keeps_other_types_out", "shows_nothing_for_a_type_that_was_never_added", "starts_empty_and_persists_the_users_list_per_project", "keeps_the_type_out_of_services", "drops_a_deleted_configuration_once_its_run_has_ended", "hides_a_configuration_even_while_it_runs", "keeps_an_added_types_group_whose_configurations_are_all_hidden", "does_not_duplicate_docker_compose_docker_dockerfile_or_docker_image_runs_in_the_run_configurations_provider", "does_not_apply_a_picker_result_after_switching_projects"),
    "41 of 46 cases; not covered: groups configurations by their folders first, as Run/Debug Configurations does; counts one per listed running configuration; leaves out types not added and Docker kinds, and counts a live debugger (the running-services badge); the sbt and jar-application categories: the Services badge and folder grouping do not exist natively")
CV = "run::chooser_view::tests::"
row("unit:src/features/run/chooserPinned.test.tsx", "partial",
    under(CV, "lists_pinned_configurations_first_in_pin_order_and_not_again_in_their_group", "pins_and_unpins_from_the_rows_actions_menu", "offers_the_configurations_executors_edit_and_delete_in_its_actions_menu_also_on_right", "proofs8_chooser::lists_the_configurations_started_last_after_the_pinned_ones_newest_first") + names("workbench::tests::proofs9_run_chooser::keeps_pins_per_project_across_reloads"),
    "6 of 11 cases; not covered: adds and selects a Unity attach configuration without hiding other configurations; keeps a configuration whose id collides with the Unity default; retains a saved Unity configuration and selected configuration; folds the other configurations under All Configurations once some are pinned or recent; expands All Configurations in place and remembers it per project: Unity configurations and the All Configurations fold do not exist natively; the actions menu's header is drawn as a separator")
addcite("unit:src/features/agents/AgentsView.test.tsx", names("agents::task_actions::tests::keeps_placement_out_of_session_menus", "agents::task_list_view::tests::proofs11_task_list::keeps_placement_out_of_session_menus"), "The task menu no longer has Open and starts with Rename, as the case 'keeps placement out of session menus' asserts, except its Terminal entry, which the native menu still offers for a local task at the project root.")
addcite("unit:src/features/agents/agents.test.ts", names("agents::composer::tests::picks_the_main_settings_and_cycles_the_mode"), "The composer's main settings now follow 'picks the main settings and cycles the mode' (model, mode, collaboration mode, permissions, approval, effort, model parameters).")
row("unit:src/platform/deploymentSettings.test.ts", "partial", names("platform_more::remote_settings::tests::normalizes_unsupported_types_to_sftp"),
    "3 of 5 cases; not covered: connects %s servers with their host and the password saved under the server; defaults FTP to anonymous passive mode on port 21 (the FTP fields of the Deployment page are not built)")
json.dump(p, open(path, "w"), indent=2, ensure_ascii=False); open(path, "a").write("\n")
print("ok")
