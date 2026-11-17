import json, re
ROOT = "${W}"
S = "${S}"
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

W = "workbench::http_client::tests::"
run_at = W + "run_request_at_the_caret_sends_it_with_the_chosen_environment_and_shows_the_response"
none = W + "run_request_without_a_request_in_the_file_says_so"
unprepared = W + "a_request_that_cannot_be_prepared_is_told_and_not_sent"
when = W + "the_http_client_actions_run_only_where_their_when_clause_holds"
run_all = W + "run_all_sends_every_request_of_the_file_in_order_and_a_run_mark_sends_its_own"
history = W + "the_http_client_lists_loads_and_clears_the_projects_history"
new_request = W + "new_http_request_creates_a_request_file_with_the_example_in_the_folder"
endpoint = W + "open_in_http_client_appends_the_endpoint_to_the_generated_requests_scratch"
generate = W + "generate_requests_from_the_spring_mapping_at_the_caret"
spring_marks = W + "the_spring_marks_and_the_editor_menu_offer_the_generate_actions"
overlap = W + "a_generate_mark_and_a_plugin_gutter_icon_on_one_line_do_not_overlap"
provider = W + "the_spring_provider_lists_the_request_paths_and_rescans_after_a_change"
toolbar = under("http_client::request_toolbar::tests::", "runs_all_requests_and_stops_only_while_one_is_sent",
                "chooses_an_environment_of_the_file", "a_buffer_without_a_file_has_no_environments",
                "the_environment_list_closes_on_escape_a_press_outside_and_the_chooser")
tabs = "http_client::service_tabs::tests::switches_between_the_services_and_the_http_client"
response = sorted(t for t in tests if t.startswith("http_client::response_view::tests::"))
assert len(response) >= 7, response
names(tabs)

on = "; its condition is read on and off"
new("action:http.runRequest", "done", names(run_at, none, unprepared, when),
    "the request at the caret is sent with the chosen environment and its response shown in the HTTP Client; a file without a request says so, and a request that cannot be prepared is told and not sent" + on)
new("action:http.runAll", "done", names(run_all, when), "every request of the file is sent in order, and a run mark in the gutter sends its own" + on)
new("action:http.showToolWindow", "done", names(history, when), "shows the HTTP Client tab of the Services tool window with the project's history, which loads an entry and clears" + on)
new("action:http.newRequest", "partial", names(new_request, when),
    "creates a request file with the example in the chosen folder" + on + ". Missing: the scratch file when there is no folder, and the hand-off to New from Template, have no test")
new("action:http.openEndpoint", "done", names(endpoint), "appends the endpoint's request to the generated requests scratch file and opens it")
new("action:http.generateSpringRequest", "done", names(generate, when), "generates the request of the Spring mapping at the caret" + on)
new("action:http.generateSpringControllerRequests", "done", names(generate, spring_marks, when), "generates the requests of every mapping of the controller, from the action, the gutter mark and the editor's menu" + on)

adapter = ", the function the Tauri adapter calls with the same arguments"
new("ipc:http_requests", "done", names(run_all), "the run marks on the request lines can only come from the backend's parse of the file" + adapter)
new("ipc:http_environments", "done", names(run_at), "the environments offered for the file can only come from the backend's read of the environment files" + adapter)
new("ipc:http_execute", "done", names(run_at), "the response shown, sent to a listener the test opened on the loopback address, can only come from the backend's execution" + adapter)
for k in ("http_history", "http_history_get", "http_history_clear"):
    new(f"ipc:{k}", "done", names(history), "the project's history listed, an entry loaded and the history cleared can only come from the backend's history functions" + adapter)
new("ipc:spring_route_sources", "done", names(provider), "the request paths the Spring provider lists, and their rescan after a change, can only come from ide_host::ops::nav::spring_route_sources" + adapter)

new("point.registerEditorContribution:http.requests", "done", names(run_at, run_all, *toolbar),
    "a request file's editor gets the toolbar (Run All, Stop only while a request is sent, the environment chooser, reload) and a run mark on each request line")
new("point.registerEditorContribution:http.spring", "partial", names(spring_marks, overlap),
    "a Spring controller's editor gets the generate marks and the editor menu's rows, and a mark beside a plugin's gutter icon does not overlap it. Missing: the intentions")
new("point.registerEndpointProvider:http.spring", "done", names(provider), "the Spring provider lists the request paths of the project and rescans after a change")
new("point.registerMenuGroup:http.spring", "done", names(spring_marks), "the editor's menu offers the generate rows in a Spring controller")
new("point.registerServiceView:httpClient", "partial", names(history, tabs, *response),
    "the HTTP Client beside the Services tree: the history list, the response with its status, headers and formatted body, Stop, Reload, Clear. Missing: the resizable split, selectable response text, a spinner, and its tabs in the tool window's header row (they are on a row of their own)")
new("feature:http-client", "partial", names(run_at, run_all, history, new_request, endpoint, generate, provider),
    "request files run from the caret, the toolbar and the gutter with an environment, the HTTP Client view with history, New HTTP Request, Open in HTTP Client, and the Spring request generation. Missing: highlighting of request files, URL completion from the endpoints, the endpoint gutter with a plugin's endpoints, the Spring intentions, the Endpoints tab of Search Everywhere, marks that follow an edit at once, a localized time in the history")
row("unit:src/features/http-client/endpoints.test.ts", "partial", [],
    "3 of 3 cases on the model; the app reaches the rendering only. Missing: completion from the endpoints and its disposal are not wired")
row("unit:src/features/http-client/httpClient.test.ts", "partial", [],
    "18 of 18 cases on the model (the tables of the spec are loops inside one test each); the app reaches the 5 format cases through the response view. Missing: the 13 highlighting cases, since request files are not highlighted")
open(path, "w").write(json.dumps(p, indent=2, ensure_ascii=False) + "\n")
print("written")
