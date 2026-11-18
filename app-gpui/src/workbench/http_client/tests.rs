//! The HTTP Client run through the registry in a project window. Requests go only to a server
//! the test opens on a loopback port the OS chose; the history and the scratches live under the
//! test's own home.

use super::*;
use crate::backend::AppDirs;
use crate::http_client::request_toolbar::RUN_ALL as TOOLBAR_RUN_ALL;
use crate::http_client::response_view::{BodyShown, CLEAR_HISTORY, StatusLine};
use crate::http_client::store::tests::exchange;
use crate::platform::input;
use crate::ui::popup_menu::PopupMenu;
use crate::workbench::tests::{context_is_on, notification_titles, run_action, wait_for_notice, wait_long};
use gpui_kit::{Focusable, Modifiers, TestAppContext, VisualTestContext};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::thread::JoinHandle;

/// A Spring controller of the project: a mapping with two paths and one with one.
const USERS: &str = "package demo;\n\nimport org.springframework.web.bind.annotation.*;\n\n@RestController\n\
@RequestMapping(\"/api\")\npublic class Users {\n    @GetMapping({\"/a\", \"/b\"})\n    \
public String list() { return \"\"; }\n\n    @PostMapping(\"/c\")\n    public String add() { return \"\"; }\n}\n";
const USERS_PATH: &str = "src/main/java/demo/Users.java";

/// A project in a window, with its folders.
struct Fixture {
    workbench: Entity<Workbench>,
    root: PathBuf,
    data: PathBuf,
    _folders: (tempfile::TempDir, tempfile::TempDir),
}

impl Fixture {
    fn editor(&self, cx: &mut VisualTestContext) -> Entity<Editor> {
        self.workbench.read_with(cx, |workbench, _| workbench.editor().cloned()).expect("an editor")
    }

    fn path(&self, name: &str) -> String {
        self.root.join(name).to_string_lossy().into_owned()
    }

    /// The generated-requests scratch, as the editor names it.
    fn scratch(&self) -> String {
        format!("{}/scratches/generated-requests.http", self.data.to_string_lossy())
    }

    /// The text of the open editor of `path`, once there is one.
    fn text_of(&self, path: &str, cx: &mut VisualTestContext) -> Option<String> {
        self.workbench.read_with(cx, |workbench, cx| workbench.editor_of_path(path, cx).map(|e| e.read(cx).text()))
    }

    fn open(&self, name: &str, cx: &mut VisualTestContext) -> Entity<Editor> {
        let file = self.root.join(name);
        self.workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(file, window, cx));
        let path = self.path(name);
        wait_long(cx, "the editor", |cx| {
            self.workbench
                .read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path().to_string_lossy() == path))
        });
        self.editor(cx)
    }

    /// Puts the caret at UTF-16 offset `at` of `editor` and gives it the keyboard.
    fn caret_at(&self, editor: &Entity<Editor>, at: u32, cx: &mut VisualTestContext) {
        self.workbench.update_in(cx, |_, window, cx| {
            editor.update(cx, |editor, cx| editor.select(at, at, cx));
            window.focus(&editor.focus_handle(cx), cx);
        });
        wait_long(cx, "the editor has the keyboard", |cx| context_is_on(cx, "editorTextFocus"));
    }

    fn history_len(&self, cx: &mut VisualTestContext) -> usize {
        self.workbench.read_with(cx, |workbench, _| workbench.http_store().history().len())
    }

    /// What the backend keeps of the project's response history.
    fn backend_history(&self, cx: &mut VisualTestContext) -> Vec<ide_http::HistoryEntry> {
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.project("w1").expect("the project is open").http_history.list()
    }
}

/// A loopback port nothing listens on.
fn dead_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    listener.local_addr().expect("the port").port()
}

/// A project with `files` open in a window; `open` is opened. The home's settings name a manual
/// proxy on a dead loopback port, which a request to a loopback server never goes through.
///
/// The proxy setting is process-wide and every backend of this test process applies its own home's
/// on start: a request of this test can run under another test's setting. Every mode sends a
/// loopback request directly when no proxy variable is set: "No proxy" always, the manual proxy
/// for every host but loopback, and auto-detect only through those variables, which are checked
/// unset here.
fn open_project<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, String)],
    open: Option<&str>,
) -> (Fixture, &'a mut VisualTestContext) {
    for name in ["http_proxy", "HTTP_PROXY", "https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"] {
        assert!(std::env::var_os(name).is_none(), "{name} is set: a loopback request could reach that proxy");
    }
    cx.executor().allow_parking();
    let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
    let dirs = AppDirs::under(&home.path().canonicalize().expect("the home exists"));
    std::fs::create_dir_all(&dirs.config).expect("the configuration folder is made");
    let settings = format!("[proxy]\nmode = \"manual\"\nhost = \"127.0.0.1\"\nport = {}\n", dead_port());
    std::fs::write(dirs.config.join("settings.toml"), settings).expect("the settings are written");
    let data = dirs.data.clone();
    let root = folder.path().canonicalize().expect("the folder exists");
    for (name, text) in files {
        let path = root.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the folder is made");
        }
        std::fs::write(path, text).expect("the file is written");
    }
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        input::init(cx);
        crate::editor::actions::register(cx);
        super::super::register_actions(cx);
        cx.set_global(UiStorage::default());
        Backend::init(dirs, cx).expect("the backend starts");
        super::super::menu::init(cx);
        cx.global::<Backend>().host.trust.decide(&root, true, false).expect("the project is trusted");
    });
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    wait_long(cx, "the project to open", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    let fixture = Fixture { workbench, root, data, _folders: (home, folder) };
    if let Some(name) = open {
        fixture.open(name, cx);
    }
    (fixture, cx)
}

/// A server on a loopback port the OS chose that answers `count` requests, one connection each,
/// with `body` as JSON, then ends; it hands back each request's line and headers.
fn serve(count: usize, body: &'static str) -> (u16, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let port = listener.local_addr().expect("the port").port();
    let handle = std::thread::spawn(move || {
        let mut heads = Vec::new();
        for _ in 0..count {
            let (mut stream, _) = listener.accept().expect("a connection");
            let mut reader = BufReader::new(stream.try_clone().expect("the stream"));
            let (mut head, mut length) = (String::new(), 0usize);
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).expect("a line") == 0 || line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap_or(0);
                }
                head.push_str(&line);
            }
            let mut sent = vec![0; length];
            reader.read_exact(&mut sent).expect("the body");
            heads.push(head);
            let answer = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(answer.as_bytes()).expect("the answer is sent");
        }
        heads
    });
    (port, handle)
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

/// The labels of the root of the open menu, without the selection mark.
fn menu_labels<C: Clone + 'static>(menu: &Entity<PopupMenu<C>>, cx: &mut VisualTestContext) -> Vec<String> {
    let lines = menu.read_with(cx, |menu, _| menu.lines());
    lines[0].iter().map(|line| line.trim_start_matches("> ").to_owned()).collect()
}

/// Every label and action id of `items`, submenus included, with each row's argument.
fn menu_rows<C>(items: &[MenuItem<C>]) -> Vec<(String, Option<Value>)> {
    let mut rows = Vec::new();
    for item in items {
        match item {
            MenuItem::Separator => {}
            MenuItem::Action(action) => {
                rows.push((action.label.clone().unwrap_or_else(|| action.action.to_owned()), action.arg.clone()));
            }
            MenuItem::Item(entry) => {
                rows.push((entry.label.clone(), None));
                rows.extend(menu_rows(&entry.submenu));
            }
        }
    }
    rows
}

/// Run HTTP Request at Caret sends the caret's request with the environment chosen in the toolbar
/// (its variables from the public and the private environment file), keeps the choice, and the
/// answer opens on the HTTP Client tab of the Services tool window, newest first, formatted, and
/// in the project's history; the private token shows only where the request is.
#[gpui_kit::test]
async fn run_request_at_the_caret_sends_it_with_the_chosen_environment_and_shows_the_response(cx: &mut TestAppContext) {
    let (port, server) = serve(1, r#"{"users":[1]}"#);
    let env = format!(r#"{{"dev": {{"host": "127.0.0.1:{port}"}}, "prod": {{"host": "127.0.0.1:1"}}}}"#);
    let files = [
        ("http-client.env.json", env),
        ("http-client.private.env.json", r#"{"dev": {"token": "s3cret-token"}}"#.to_owned()),
        ("api.http", "### Users\nGET http://{{host}}/users\nAuthorization: Bearer {{token}}\n".to_owned()),
    ];
    let (f, cx) = open_project(cx, &files, Some("api.http"));
    let proxy = cx.update(|_, cx| cx.global::<Backend>().host.global_settings.effective().proxy);
    assert_eq!((proxy.mode, proxy.host.as_str()), (ide_core::settings::ProxyMode::Manual, "127.0.0.1"));
    let editor = f.editor(cx);
    let path = f.path("api.http");
    wait_long(cx, "the environments", |cx| {
        f.workbench.read_with(cx, |w, _| w.http_store().environments(&path).is_some())
    });
    let toolbar = f.workbench.read_with(cx, |w, _| w.http_toolbar_of(&editor)).expect("the request file's toolbar");
    let labels: Vec<String> = toolbar.read_with(cx, |t, _| t.options().into_iter().map(|(_, label)| label).collect());
    assert_eq!(labels, ["No Environment", "dev", "prod"]);
    toolbar.update(cx, |toolbar, cx| toolbar.choose("dev", cx));
    cx.run_until_parked();
    let stored = cx.update(|_, cx| cx.global::<UiStorage>().get(ENV_KEY).map(str::to_owned));
    assert!(stored.is_some_and(|stored| stored.contains("\"dev\"")), "the choice is kept");

    let request_line = editor.read_with(cx, |e, _| e.offset_of(1, 4));
    f.caret_at(&editor, request_line, cx);
    run_action(cx, RUN_REQUEST, None);
    wait_long(cx, "the response", |cx| f.history_len(cx) == 1);
    let heads = server.join().expect("the server ends");
    assert_eq!(heads[0].lines().next(), Some("GET /users HTTP/1.1"));
    assert!(heads[0].to_ascii_lowercase().contains("authorization: bearer s3cret-token"));

    assert!(f.workbench.read_with(cx, |w, _| w.is_tool_window_visible(SERVICES_TOOL_WINDOW)));
    let tabs = f.workbench.read_with(cx, |w, _| w.http_service_tabs().cloned()).expect("the Services tabs");
    assert_eq!(tabs.read_with(cx, |tabs, _| tabs.active()), ServiceTab::HttpClient);
    let view = f.workbench.read_with(cx, |w, _| w.http_view().cloned()).expect("the HTTP Client view");
    let rows = view.read_with(cx, |v, _| v.rows());
    assert_eq!((rows[0].status.as_str(), rows[0].method.as_str()), ("200", "GET"));
    assert_eq!(rows[0].label, "Users");
    let Some(StatusLine::Response { status, .. }) = view.read_with(cx, |v, _| v.status_line()) else {
        panic!("the response is shown")
    };
    assert_eq!(status, "200 OK");
    let body = view.read_with(cx, |v, _| v.body());
    assert_eq!(body, Some(BodyShown::Text { text: "{\n  \"users\": [\n    1\n  ]\n}".to_owned(), truncated: None }));
    let sent = view.read_with(cx, |v, _| v.request_text()).expect("the request as sent");
    assert!(sent.contains(&format!("GET http://127.0.0.1:{port}/users")) && sent.contains("Bearer s3cret-token"));
    let shown = format!("{:?} {:?}", view.read_with(cx, |v, _| v.status_line()), notification_titles(&f.workbench, cx));
    assert!(!shown.contains("s3cret"), "the token stays out of debug output: {shown}");
    wait_long(cx, "the project's history", |cx| f.backend_history(cx).len() == 1);
    assert_eq!(f.backend_history(cx)[0].status, Some(200));
}

/// Run All sends the file's requests in order, the last answer first in the history; a request's
/// run mark in the gutter sends that request alone.
#[gpui_kit::test]
async fn run_all_sends_every_request_of_the_file_in_order_and_a_run_mark_sends_its_own(cx: &mut TestAppContext) {
    let (port, server) = serve(3, "{}");
    let text = format!("### One\nGET http://127.0.0.1:{port}/one\n\n### Two\nGET http://127.0.0.1:{port}/two\n");
    let (f, cx) = open_project(cx, &[("two.http", text)], Some("two.http"));
    let editor = f.editor(cx);
    wait_long(cx, "the run marks", |cx| f.workbench.read_with(cx, |w, _| w.http_gutter_lines(&editor)) == [1, 4]);
    f.caret_at(&editor, 0, cx);
    run_action(cx, RUN_ALL, None);
    wait_long(cx, "both answers", |cx| f.history_len(cx) == 2);
    let urls: Vec<String> =
        f.workbench.read_with(cx, |w, _| w.http_store().history().iter().map(|e| e.url.clone()).collect());
    assert_eq!(urls, [format!("http://127.0.0.1:{port}/two"), format!("http://127.0.0.1:{port}/one")]);

    click(cx, "http-gutter-1");
    wait_long(cx, "the mark's request", |cx| f.history_len(cx) == 3);
    let heads = server.join().expect("the server ends");
    let lines: Vec<&str> = heads.iter().filter_map(|head| head.lines().next()).collect();
    assert_eq!(lines, ["GET /one HTTP/1.1", "GET /two HTTP/1.1", "GET /one HTTP/1.1"]);
    let toolbar = f.workbench.read_with(cx, |w, _| w.http_toolbar_of(&editor)).expect("the toolbar");
    assert_eq!(
        toolbar.read_with(cx, |t, _| t.toolbar().iter().find(|b| b.id == TOOLBAR_RUN_ALL).map(|b| b.enabled)),
        Some(true)
    );
}

/// With no request in the file, Run HTTP Request at Caret says so and sends nothing.
#[gpui_kit::test]
async fn run_request_without_a_request_in_the_file_says_so(cx: &mut TestAppContext) {
    let (f, cx) = open_project(cx, &[("empty.http", "# nothing to send\n".to_owned())], Some("empty.http"));
    let editor = f.editor(cx);
    wait_long(cx, "the toolbar", |cx| f.workbench.read_with(cx, |w, _| w.http_toolbar_of(&editor).is_some()));
    f.caret_at(&editor, 0, cx);
    run_action(cx, RUN_REQUEST, None);
    wait_for_notice(&f.workbench, "No HTTP request at the caret", cx);
    assert_eq!(f.history_len(cx), 0);
}

/// A request whose variable no environment defines is not sent: the failure is told and nothing
/// is left running.
#[gpui_kit::test]
async fn a_request_that_cannot_be_prepared_is_told_and_not_sent(cx: &mut TestAppContext) {
    let (f, cx) = open_project(cx, &[("bad.http", "GET http://{{nowhere}}/x\n".to_owned())], Some("bad.http"));
    let editor = f.editor(cx);
    f.caret_at(&editor, 0, cx);
    run_action(cx, RUN_REQUEST, None);
    wait_for_notice(&f.workbench, "Cannot send the request", cx);
    let body = f.workbench.read_with(cx, |w, _| {
        w.notifications().items().iter().find(|n| n.title == "Cannot send the request").and_then(|n| n.body.clone())
    });
    assert!(body.is_some_and(|body| body.contains("nowhere")));
    assert!(f.workbench.read_with(cx, |w, _| w.http_store().running().is_none()));
    assert_eq!(f.history_len(cx), 0);
}

/// HTTP Client shows the project's response history; a click on an entry loads its exchange from
/// the backend, and Clear History empties the list and the project's history.
#[gpui_kit::test]
async fn the_http_client_lists_loads_and_clears_the_projects_history(cx: &mut TestAppContext) {
    let (f, cx) = open_project(cx, &[("a.txt", String::new())], None);
    let mut seeded = exchange("seeded1", Some(201));
    seeded.response = Some(ide_http::HttpResponse {
        status: 201,
        status_text: "Created".to_owned(),
        version: "HTTP/1.1".to_owned(),
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        content_type: Some("application/json".to_owned()),
        body: Some(r#"{"seeded":true}"#.to_owned()),
        size: 15.0,
        truncated: false,
        ttfb_ms: 1.0,
        total_ms: 2.0,
    });
    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    host.project("w1").expect("the project").http_history.add(&seeded).expect("the entry is saved");

    // Until the HTTP Client is used, the Services tool window shows its tree alone, without tabs.
    run_action(cx, "services.show", None);
    let services = f.workbench.read_with(cx, |w, _| w.tool_window_view(SERVICES_TOOL_WINDOW)).expect("the panel");
    let services = services.downcast::<ServicesView>().expect("the bare services view");
    cx.run_until_parked();
    assert!(cx.debug_bounds("service-tab-services").is_none(), "no tabs");
    assert!(f.workbench.read_with(cx, |w, _| w.http_service_tabs().is_none()));

    run_action(cx, SHOW_TOOL_WINDOW, None);
    let tabs = f.workbench.read_with(cx, |w, _| w.http_service_tabs().cloned()).expect("the tabs");
    let shown = f.workbench.read_with(cx, |w, _| w.tool_window_view(SERVICES_TOOL_WINDOW)).expect("the panel");
    assert_eq!(shown.entity_id(), tabs.entity_id(), "the window shows the tabs");
    cx.run_until_parked();
    assert!(cx.debug_bounds("service-tab-services").is_some() && cx.debug_bounds("service-tab-http").is_some());
    click(cx, "service-tab-services");
    assert_eq!(tabs.read_with(cx, |tabs, _| tabs.active()), ServiceTab::Services);
    let tree = tabs.read_with(cx, |tabs, _| tabs.services().clone());
    assert_eq!(tree.entity_id(), services.entity_id(), "the same services tree, under its tab");
    run_action(cx, SHOW_TOOL_WINDOW, None);
    assert_eq!(tabs.read_with(cx, |tabs, _| tabs.active()), ServiceTab::HttpClient);
    run_action(cx, "services.show", None);
    assert_eq!(tabs.read_with(cx, |tabs, _| tabs.active()), ServiceTab::Services, "Services shows the tree's tab");
    run_action(cx, SHOW_TOOL_WINDOW, None);
    assert_eq!(tabs.read_with(cx, |tabs, _| tabs.active()), ServiceTab::HttpClient);
    assert!(f.workbench.read_with(cx, |w, _| w.is_tool_window_visible(SERVICES_TOOL_WINDOW)));
    let view = f.workbench.read_with(cx, |w, _| w.http_view().cloned()).expect("the HTTP Client view");
    wait_long(cx, "the history", |cx| view.read_with(cx, |v, _| v.rows().len()) == 1);
    assert_eq!(view.read_with(cx, |v, _| v.rows()[0].status.clone()), "201");
    assert!(view.read_with(cx, |v, _| v.body()).is_none(), "nothing is selected yet");

    click(cx, "http-entry-0");
    wait_long(cx, "the exchange", |cx| view.read_with(cx, |v, _| v.body()).is_some());
    let body = view.read_with(cx, |v, _| v.body());
    assert_eq!(body, Some(BodyShown::Text { text: "{\n  \"seeded\": true\n}".to_owned(), truncated: None }));

    click(cx, CLEAR_HISTORY);
    wait_long(cx, "the cleared history", |cx| view.read_with(cx, |v, _| v.is_empty()));
    assert!(f.backend_history(cx).is_empty());
    assert!(view.read_with(cx, |v, _| v.body()).is_none());
}

/// New HTTP Request in a folder asks for the file's name and creates it there with the example request.
#[gpui_kit::test]
async fn new_http_request_creates_a_request_file_with_the_example_in_the_folder(cx: &mut TestAppContext) {
    let (f, cx) = open_project(cx, &[("api/.keep", String::new())], None);
    let directory = f.path("api");
    cx.update(|window, _| window.activate_window());
    run_action(cx, NEW_REQUEST, Some(json!({ "directory": directory })));
    cx.run_until_parked();
    assert!(f.workbench.read_with(cx, |w, _| w.new_file_dialog().is_some()), "the name is asked for");
    cx.simulate_input("requests");
    cx.simulate_keystrokes("enter");
    let first = format!("{directory}/requests.http");
    wait_long(cx, "the new request file", |cx| f.text_of(&first, cx).as_deref() == Some(NEW_REQUEST_TEMPLATE));
    assert!(Path::new(&first).is_file());
}

/// New HTTP Request without a folder creates `requests.http` in the scratches with the example request,
/// then `requests_1.http` once it is taken.
#[gpui_kit::test]
async fn new_http_request_without_a_folder_names_the_scratch_requests_then_requests_1(cx: &mut TestAppContext) {
    let (f, cx) = open_project(cx, &[("a.txt", String::new())], None);
    let scratches = Path::new(&f.scratch()).parent().expect("the scratches").to_string_lossy().into_owned();
    run_action(cx, NEW_REQUEST, None);
    let first = format!("{scratches}/requests.http");
    wait_long(cx, "the first request file", |cx| f.text_of(&first, cx).as_deref() == Some(NEW_REQUEST_TEMPLATE));
    run_action(cx, NEW_REQUEST, None);
    let second = format!("{scratches}/requests_1.http");
    wait_long(cx, "the second request file", |cx| f.text_of(&second, cx).as_deref() == Some(NEW_REQUEST_TEMPLATE));
}

/// Open in HTTP Client appends the endpoint's request to the generated-requests scratch: named by
/// its handler (else its method and URL), any method as GET, path variables as variables, on the
/// default local server when its module configures none.
#[gpui_kit::test]
async fn open_in_http_client_appends_the_endpoint_to_the_generated_requests_scratch(cx: &mut TestAppContext) {
    let (f, cx) = open_project(cx, &[("a.txt", String::new())], None);
    let endpoint = json!({ "method": "GET", "url": "/users/{id}", "handler": "users.show", "controller": "",
        "methodName": "users.show", "path": "/p/routes.txt", "rel": "", "line": 0, "col": 0 });
    run_action(cx, OPEN_ENDPOINT, Some(endpoint));
    let scratch = f.scratch();
    let first = "### users.show\nGET http://localhost:8080/users/{{id}}\n";
    wait_long(cx, "the scratch", |cx| f.text_of(&scratch, cx).as_deref() == Some(first));
    let any = json!({ "method": "*", "url": "/health", "path": "/p/routes.txt", "line": 0, "col": 0 });
    run_action(cx, OPEN_ENDPOINT, Some(any));
    let both = format!("{first}\n### * /health\nGET http://localhost:8080/health\n");
    wait_long(cx, "the appended request", |cx| f.text_of(&scratch, cx) == Some(both.clone()));
}

/// Open in HTTP Client puts the endpoint's request on the server address its module configures
/// (`server.port`, the servlet context path), as a Spring mapping's generated request has it.
#[gpui_kit::test]
async fn open_in_http_client_takes_the_server_address_of_the_endpoints_module(cx: &mut TestAppContext) {
    let config = "server.port=9090\nserver.servlet.context-path=/shop\n".to_owned();
    let files = [("shop/src/main/resources/application.properties", config), (USERS_PATH, USERS.to_owned())];
    let (f, cx) = open_project(cx, &files, None);
    let source = f.root.join("shop/src/main/java/demo/Users.java").to_string_lossy().into_owned();
    let endpoint = json!({ "method": "GET", "url": "/users/{id}", "handler": "Users.show", "controller": "Users",
        "methodName": "show", "path": source, "rel": "", "line": 0, "col": 0 });
    run_action(cx, OPEN_ENDPOINT, Some(endpoint));
    let scratch = f.scratch();
    let expected = "### Users.show\nGET http://localhost:9090/shop/users/{{id}}\n";
    wait_long(cx, "the request on the module's server", |cx| f.text_of(&scratch, cx).as_deref() == Some(expected));
}

/// Generate Request in HTTP Client renders the mapping at the caret with the module's server port,
/// a path chosen among several, and Generate Requests for Controller every mapping of it; with no
/// mapping at the caret it says so.
#[gpui_kit::test]
async fn generate_requests_from_the_spring_mapping_at_the_caret(cx: &mut TestAppContext) {
    let files = [
        (USERS_PATH, USERS.to_owned()),
        ("src/main/resources/application.properties", "server.port=9090\n".to_owned()),
    ];
    let (f, cx) = open_project(cx, &files, Some(USERS_PATH));
    let editor = f.editor(cx);
    let add = u32::try_from(USERS.find("add()").expect("the method")).expect("a small offset");
    f.caret_at(&editor, add, cx);
    run_action(cx, GENERATE_REQUEST, None);
    let scratch = f.scratch();
    wait_long(cx, "the generated request", |cx| {
        f.text_of(&scratch, cx).is_some_and(|text| text.contains("### Users.add\nPOST http://localhost:9090/api/c"))
    });

    let editor = f.open(USERS_PATH, cx);
    let list = u32::try_from(USERS.find("list()").expect("the method")).expect("a small offset");
    f.caret_at(&editor, list, cx);
    run_action(cx, GENERATE_REQUEST, Some(json!({ "path": "/api/b" })));
    wait_long(cx, "the chosen path", |cx| {
        f.text_of(&scratch, cx).is_some_and(|text| text.contains("GET http://localhost:9090/api/b"))
    });
    let editor = f.open(USERS_PATH, cx);
    f.caret_at(&editor, list, cx);
    run_action(cx, GENERATE_CONTROLLER_REQUESTS, None);
    wait_long(cx, "the controller's requests", |cx| {
        f.text_of(&scratch, cx).is_some_and(|text| text.matches("### Users.").count() == 4)
    });

    let editor = f.open(USERS_PATH, cx);
    f.caret_at(&editor, 0, cx);
    run_action(cx, GENERATE_REQUEST, None);
    wait_for_notice(&f.workbench, "No Spring request mapping at the caret", cx);
}

/// A Spring source gets a generate mark on its controller and on each mapping; the controller's
/// mark offers Generate Requests for Controller, a mapping with two paths a choice of them, and the
/// editor menu offers the same at the caret.
#[gpui_kit::test]
async fn the_spring_marks_and_the_editor_menu_offer_the_generate_actions(cx: &mut TestAppContext) {
    let (f, cx) = open_project(cx, &[(USERS_PATH, USERS.to_owned())], Some(USERS_PATH));
    let editor = f.editor(cx);
    wait_long(cx, "the generate marks", |cx| {
        f.workbench.read_with(cx, |w, _| w.http_gutter_lines(&editor)) == [6, 8, 11]
    });
    click(cx, "http-gutter-8");
    let menu = f.workbench.read_with(cx, |w, _| w.open_menu::<SpringCommand>()).expect("the mark's menu");
    assert_eq!(menu_labels(&menu, cx), ["Generate Request in HTTP Client"]);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    click(cx, "http-gutter-6");
    let menu = f.workbench.read_with(cx, |w, _| w.open_menu::<SpringCommand>()).expect("the mark's menu");
    assert_eq!(menu_labels(&menu, cx), ["Generate Requests for Controller"]);
    click(cx, "menu-row-0-0");
    let scratch = f.scratch();
    wait_long(cx, "the controller's requests", |cx| {
        f.text_of(&scratch, cx).is_some_and(|text| text.contains("### Users.list") && text.contains("### Users.add"))
    });

    let editor = f.open(USERS_PATH, cx);
    let list = u32::try_from(USERS.find("list()").expect("the method")).expect("a small offset");
    f.caret_at(&editor, list, cx);
    let items = f.workbench.update_in(cx, |w, window, cx| w.editor_menu_items(&editor, window, cx));
    let rows = menu_rows(&items);
    let paths: Vec<Option<Value>> =
        rows.iter().filter(|(label, _)| label.starts_with("GET /api/")).map(|(_, arg)| arg.clone()).collect();
    assert_eq!(paths, [Some(json!({ "path": "/api/a" })), Some(json!({ "path": "/api/b" }))]);
    assert!(rows.iter().any(|(label, _)| label == "Generate Request in HTTP Client"));
    assert!(rows.iter().any(|(label, _)| label == GENERATE_CONTROLLER_REQUESTS));
}

/// The Spring endpoint provider lists the project's request paths from its mapping sources, with
/// the handler's file and line; a change to a mapping source has them scanned again.
#[gpui_kit::test]
async fn the_spring_provider_lists_the_request_paths_and_rescans_after_a_change(cx: &mut TestAppContext) {
    let (f, cx) = open_project(cx, &[(USERS_PATH, USERS.to_owned())], None);
    let endpoints =
        |cx: &mut VisualTestContext| f.workbench.update_in(cx, |w, window, cx| w.http_endpoints(window, cx));
    wait_long(cx, "the request paths", |cx| !endpoints(cx).is_empty());
    let found: Vec<(String, String, String, u32)> =
        endpoints(cx).into_iter().map(|e| (e.method, e.url, e.rel, e.line)).collect();
    assert_eq!(
        found,
        [
            ("GET".to_owned(), "/api/a".to_owned(), USERS_PATH.to_owned(), 8),
            ("GET".to_owned(), "/api/b".to_owned(), USERS_PATH.to_owned(), 8),
            ("POST".to_owned(), "/api/c".to_owned(), USERS_PATH.to_owned(), 11)
        ]
    );
    let source = f.path(USERS_PATH);
    let changed = USERS.replace("@PostMapping(\"/c\")", "@DeleteMapping(\"/d\")");
    std::fs::write(&source, changed).expect("the source is changed");
    f.workbench.update(cx, |w, _| w.http_files_changed([source.as_str()]));
    wait_long(cx, "the rescanned paths", |cx| endpoints(cx).iter().any(|e| e.method == "DELETE" && e.url == "/api/d"));
}

/// A generate mark and a plugin's gutter icon on the same line are drawn side by side inside the
/// editor, neither over the other.
#[gpui_kit::test]
async fn a_generate_mark_and_a_plugin_gutter_icon_on_one_line_do_not_overlap(cx: &mut TestAppContext) {
    use super::super::plugins_window::test_support::{link, plugin_copy, start_with_plugins, trusted_project};
    let home = start_with_plugins(cx);
    let folder = plugin_copy(home.path(), "language-features", |text| {
        text.replacen("languages = [\"plaintext\", \"text\", \"sql\"]", "languages = [\"java\"]", 1)
    });
    link(cx, &folder, &[], "run");
    let source = "@RestController public class One { @GetMapping(\"/a\") public String a() { return \"\"; } }\n";
    let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("One.java", source)]);
    let file = root.join("One.java");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(file.clone(), window, cx));
    wait_long(cx, "the editor", |cx| {
        workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file))
    });
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    wait_long(cx, "the generate mark and the plugin's icon", |cx| {
        workbench
            .read_with(cx, |w, _| w.http_gutter_lines(&editor) == [0] && !w.plugin_gutter_markers(&editor).is_empty())
    });
    assert_eq!(workbench.read_with(cx, |w, _| w.plugin_gutter_markers(&editor)[0].line), 0);
    wait_long(cx, "both are drawn", |cx| {
        cx.debug_bounds("http-gutter-0").is_some() && cx.debug_bounds("plugin-gutter:0").is_some()
    });
    let mark = cx.debug_bounds("http-gutter-0").expect("the generate mark");
    let icon = cx.debug_bounds("plugin-gutter:0").expect("the plugin's icon");
    assert!(!mark.intersects(&icon), "the mark {mark:?} is over the icon {icon:?}");
    assert!((mark.center().y - icon.center().y).abs() < gpui_kit::px(1.0), "both are on the line");
    // The mark's column comes first, then the icon's: 14 wide, the icon centred in it.
    assert_eq!(icon.left() - mark.right(), gpui_kit::px(1.0), "the icon {icon:?} right after the mark {mark:?}");
    let number = editor
        .read_with(cx, |e, _| e.painted_line_numbers().into_iter().find(|(line, _)| *line == 0))
        .expect("line 1's number is drawn")
        .1;
    assert!(number.right() <= mark.left(), "the mark {mark:?} is over the number {number:?}");
    let view = editor.read_with(cx, |e, _| e.painted_bounds().expect("the editor is painted"));
    assert!(mark.left() >= view.left() && icon.right() <= view.right(), "both are inside the editor {view:?}");
}

/// A request file's run mark stands in its own gutter column between the line numbers and the fold
/// chevrons: it covers no digit of its line's number, the numbers keep their right padding before
/// it, and the change bars' column and the fold chevrons follow it before the text.
#[gpui_kit::test]
async fn a_run_mark_stands_in_its_own_gutter_column_right_of_the_line_number(cx: &mut TestAppContext) {
    let text = "### One\nGET http://127.0.0.1:1/one\n".to_owned();
    let (f, cx) = open_project(cx, &[("one.http", text)], Some("one.http"));
    let editor = f.editor(cx);
    wait_long(cx, "the run mark", |cx| f.workbench.read_with(cx, |w, _| w.http_gutter_lines(&editor)) == [1]);
    wait_long(cx, "the mark is drawn", |cx| cx.debug_bounds("http-gutter-1").is_some());
    let mark = cx.debug_bounds("http-gutter-1").expect("the run mark");
    let number = editor
        .read_with(cx, |e, _| e.painted_line_numbers().into_iter().find(|(line, _)| *line == 1))
        .expect("line 2's number is drawn")
        .1;
    assert!(number.right() <= mark.left(), "the mark {mark:?} is over the number {number:?}");
    // The numbers' right padding, then the bookmarks' 15 px column, which stands left of the HTTP marks.
    assert_eq!(mark.left() - number.right(), px(8.0 + 15.0), "the numbers' right padding and the bookmarks");
    let (text_left, fold) =
        editor.read_with(cx, |e, cx| (e.position_of(e.offset_of(1, 0)).expect("painted").x, e.fold_column_width(cx)));
    assert_eq!(mark.size.width, px(14.0), "the column's width");
    assert_eq!(
        text_left - mark.right(),
        px(4.0) + fold + px(1.0),
        "the change bars' column, the fold chevrons and the gutter's hairline"
    );
}

/// Whether action `id` may run in the window as it is now (its `when` clause holds).
fn enabled(cx: &mut VisualTestContext, id: &str) -> bool {
    cx.update(|window, cx| {
        let action = cx.global::<ActionRegistry>().get(id).expect("the action is registered");
        let ctx = crate::platform::context::window_context(window, cx);
        crate::platform::actions::is_action_enabled(&action.meta, &ctx)
    })
}

/// Each HTTP Client action runs only where its `when` clause holds: Run HTTP Request at Caret in a
/// request file's editor with the keyboard, Run All in a request file's editor, the Spring
/// generate actions in a Java or Kotlin editor, New HTTP Request and HTTP Client with a project.
#[gpui_kit::test]
async fn the_http_client_actions_run_only_where_their_when_clause_holds(cx: &mut TestAppContext) {
    let files = [("api.http", "GET http://127.0.0.1:1/\n".to_owned()), (USERS_PATH, USERS.to_owned())];
    let (f, cx) = open_project(cx, &files, Some("api.http"));
    let all = [RUN_REQUEST, RUN_ALL, GENERATE_REQUEST, GENERATE_CONTROLLER_REQUESTS, NEW_REQUEST, SHOW_TOOL_WINDOW];
    let state = |cx: &mut VisualTestContext| all.map(|id| enabled(cx, id));
    let editor = f.editor(cx);
    f.caret_at(&editor, 0, cx);
    assert_eq!(state(cx), [true, true, false, false, true, true], "a request file's editor with the keyboard");

    let tree = f.workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("the tree");
    cx.update(|window, cx| window.focus(&tree.focus_handle(cx), cx));
    wait_long(cx, "the tree has the keyboard", |cx| !context_is_on(cx, "editorTextFocus"));
    assert_eq!(state(cx), [false, true, false, false, true, true], "the request file's editor without it");

    let editor = f.open(USERS_PATH, cx);
    f.caret_at(&editor, 0, cx);
    assert_eq!(state(cx), [false, false, true, true, true, true], "a Java editor");

    f.workbench.update_in(cx, |w, window, cx| w.close_project(window, cx));
    wait_long(cx, "the project to close", |cx| !context_is_on(cx, "projectOpen"));
    assert_eq!(state(cx), [false; 6], "no project");
}
