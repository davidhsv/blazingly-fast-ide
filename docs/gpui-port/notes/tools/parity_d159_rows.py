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
    j = text.lower().find("missing:"); assert j > 0, key
    return text[:j].rstrip(" ;.")

UL = "workbench::tests::ui_language::"
LOC = "workbench::plugin_runtime::tests::proofs10_locale::a_plugin_reads_the_ui_language_the_settings_choose"
new("ipc:plugin_set_ui_locale", "done", names(LOC),
    "the plugin host is told the UI language at start and again after every switch, once the catalog switched, so plugin code reads the language the settings choose")
row("wit:wit/host.wit#ui", "done", names(LOC),
    before_missing("wit:wit/host.wit#ui") + "; locale is the language the settings choose, told to the host at start and after every switch")
row("settings:general.language", "done", under(UL, "the_language_applied_in_settings_reaches_the_menus_a_tool_window_and_find_action", "a_home_whose_settings_choose_chinese_opens_its_first_window_in_chinese", "an_unset_language_is_english_system_is_the_systems_and_an_unknown_one_is_english"),
    "the webview's Language choice, drawn natively; applied in the dialog it switches the running app's language without a restart (the dialog, the main menu, a tool window's title, Find Action), a home whose settings choose a language opens in it, and unset, system, regional and unknown values resolve as the webview's do. One language per process, from the global settings: a project file's own value does not give its window another language, as the webview's would")
json.dump(p, open(path, "w"), indent=2, ensure_ascii=False); open(path, "a").write("\n")
print("ok")
