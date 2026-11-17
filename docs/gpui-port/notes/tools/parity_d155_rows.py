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

RS = "workbench::tests::proofs40_run_start::"
DG = "workbench::debug::tests::proofs40_debug_guards::"
RG = "run::store::tests::proofs40_run_guards::"
row("ipc:run_start", "done",
    under(RS, "cancels_startup_and_handles_a_late_cancelled_response", "cancels_startup_and_handles_a_late_success_response", "a_start_let_go_before_stop_runs_its_program", "a_late_answer_after_stop_and_rerun_stops_only_the_first_starts_process"),
    "a run configuration runs into the Run tool window through ide_host::ops::run::start_request with a request id, as the adapter does; Stop while the start is still out cancels the request, a process that started anyway is stopped quietly and no other is (each guard's test fails with the guard switched off), and a start nobody stopped runs its program")
row("action:run.debugConfig", "done",
    under(DG, "debug_on_a_running_configuration_asks_to_stop_it_instead_of_starting_another", "debug_on_a_running_configuration_stops_it_without_asking_when_the_setting_is_off", "a_second_debug_while_the_first_one_prepares_does_nothing", "debug_on_a_debugged_configuration_asks_then_stops_the_session_and_debugs_again"),
    "the debugger's other actions reach the adapter and the editor; Debug on a running configuration asks to stop it instead of starting another (and stops it without asking when the stored setting says so), leaving other configurations' sessions running; a second Debug while the first one prepares does nothing; Debug of a configuration on a run target is refused with the notice and starts nothing. Not in the native prompt: the Do not ask again checkbox (the stored setting is honoured)")
row("action:run.runConfig", "partial", under(RG, "gives_runs_started_together_their_own_tabs", "takes_the_place_of_the_same_configurations_finished_tab_then_the_selected_one"),
    "run through the registry by id the configuration runs, exits 0 and its output shows in the Run tool window; given whole with temporary it runs and is kept as a temporary configuration; a run takes the place of the same configuration's finished tab, then the selected one, and runs started together each get their own tab (on the store). Missing: a rerun asks to stop the configuration's live debug sessions too (`src/features/run/store.ts:865`); a new run tab is pinned under the pin-tabs setting (`src/features/run/store.ts:929`)")
addcite("action:debug.rerun", under(DG, "rerun_stops_the_live_session_first_and_takes_its_own_tabs_place"), "Rerun stops the live session first and its session takes its own tab's place; the pin is not built.")
row("unit:src/features/run/resolveCancellation.test.ts", "done",
    under(RG, "cancels_startup_and_handles_a_late_cancelled_response", "cancels_startup_and_handles_a_late_success_response") + under(RS, "cancels_startup_and_handles_a_late_cancelled_response", "cancels_startup_and_handles_a_late_success_response"),
    "3 of 3 cases; the startup case on the store and through a project window with a stand-in whose start is held")
row("unit:src/features/run/javaModule.test.ts", "partial", under(RS, "a_start_let_go_before_stop_runs_its_program"),
    "13 of 19 cases; the other six (runs from %s using the stored module's classpath; finds the source module for an old saved configuration without a module; colours Spring Boot runs without classpath resolution (%j); preserves an explicit Spring Boot console setting (%s); keeps plain Java runs with an explicit classpath free of Spring Boot options; adds the Spring Boot console option once when resolving the module classpath) have model tests of their names that do not assert the request id the cases now expect with the start; the id is passed for every start (one start path, read) and proven end to end for a program launch only")
new("unit:src/features/run/runTabReuse.test.ts", "partial",
    under(RG, "takes_the_place_of_the_same_configurations_finished_tab_then_the_selected_one", "keeps_an_agents_commands_out_of_other_tabs", "gives_runs_started_together_their_own_tabs"),
    "2 of 4 cases whole and one in part, on the store; not covered: the pinned clause of 'takes the place of the same configuration's finished tab, then the selected one, never a pinned one'; opens a new tab when every finished tab is pinned, pinned when the setting asks: pinned run tabs do not exist natively")
json.dump(p, open(path, "w"), indent=2, ensure_ascii=False); open(path, "a").write("\n")
print("ok")
