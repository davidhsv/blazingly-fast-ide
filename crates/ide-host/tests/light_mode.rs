//! Light mode: no language server runs, and Find Usages / Call Hierarchy fall back
//! to whole-word text matches placed with the syntax outline.

use ide_core::settings::IdeMode;
use ide_core::{CancellationToken, IdeEvent, Pos, RequestId};
use ide_host::ops::{self, Channel};
use ide_host::{Host, HostConfig};
use ide_lsp::types::{HierarchyDirection, References};
use ide_workspace::settings::SettingsScope;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const APP: &str = "class App {\n    // helper() in a comment\n    String note = \"helper() in a string\";\n    int helper(int x) { return x; }\n    void run() { helper(1); this.helper(2); }\n    void other() { int helperCount = helper(3); }\n}\n";
const UTIL: &str = "class Util {\n    void go() { new App().helper(4); }\n}\n";
const TOOL: &str = "def helper(x):\n    return x\n\n# helper() in a comment\ns = \"helper() in a string\"\n\ndef main():\n    helper(1)\n";

struct Harness {
    host: Arc<Host>,
    root: PathBuf,
    events: Arc<Mutex<Vec<IdeEvent>>>,
    _tmp: tempfile::TempDir,
}

impl Harness {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(tmp.path()).unwrap().join("project");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("build")).unwrap();
        std::fs::write(root.join("src/App.java"), APP).unwrap();
        std::fs::write(root.join("src/Util.java"), UTIL).unwrap();
        std::fs::write(root.join("src/tool.py"), TOOL).unwrap();
        std::fs::write(root.join("build/Gen.java"), "class Gen { void x() { helper(9); } }\n").unwrap();
        std::fs::write(root.join(".gitignore"), "build/\n").unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let host = Arc::new(Host::new(HostConfig {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            local_data_dir: tmp.path().join("local"),
            languages: Arc::new(ide_language::LanguageRegistry::load(None).unwrap()),
            syntax: Arc::default(),
            sink_for: Box::new(move |_| {
                let events = sink.clone();
                Arc::new(move |e| events.lock().unwrap().push(e))
            }),
            owner: |w| w,
            runtime: tokio::runtime::Handle::current(),
            project_environment: false,
        }));
        host.trust.decide(&root, true, false).unwrap();
        host.open_project("main", &root).unwrap();
        Self { host, root, events, _tmp: tmp }
    }

    fn project(&self) -> Arc<ide_host::Project> {
        self.host.project("main").unwrap()
    }

    fn open(&self, rel: &str) -> ide_core::DocId {
        self.project().documents.open(&self.root.join(rel)).unwrap().id
    }

    fn rel(&self, path: &std::path::Path) -> String {
        path.strip_prefix(&self.root).unwrap().to_string_lossy().replace('\\', "/")
    }

    async fn usages(&self, doc: ide_core::DocId, pos: Pos, request: u32) -> (References, Vec<References>) {
        let partials = Arc::new(Mutex::new(Vec::new()));
        let channel = {
            let partials = partials.clone();
            Channel::new(move |refs: References| partials.lock().unwrap().push(refs))
        };
        let refs =
            ops::lsp::lsp_references(&self.host, "main", RequestId(request), doc, pos, false, true, false, channel)
                .await
                .unwrap();
        let partials = partials.lock().unwrap().clone();
        (refs, partials)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn light_mode_is_the_default_and_leaves_language_servers_off() {
    let h = Harness::new();
    let project = h.project();
    assert_eq!(project.ide_mode(), IdeMode::Light);
    h.open("src/App.java");
    h.open("src/tool.py");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(project.lsp.servers().is_empty(), "{:?}", project.lsp.servers());
    assert!(!h.events.lock().unwrap().iter().any(|e| matches!(e, IdeEvent::LanguageServer { .. })));
    // A project value overrides the global one and applies without a restart.
    ops::project::set_settings(&h.host, "main", SettingsScope::Project, "ide.mode", "\"full\"").unwrap();
    assert_eq!(project.ide_mode(), IdeMode::Full);
    ops::project::set_settings(&h.host, "main", SettingsScope::Global, "ide.mode", "\"full\"").unwrap();
    ops::project::set_settings(&h.host, "main", SettingsScope::Project, "ide.mode", "\"light\"").unwrap();
    assert_eq!(project.ide_mode(), IdeMode::Light);
    assert_eq!(h.host.global_settings.effective().ide.mode, IdeMode::Full);
    h.host.close_project("main");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn find_usages_without_a_server_returns_text_matches_outside_comments_and_strings() {
    let h = Harness::new();
    let doc = h.open("src/App.java");
    // The caret on the declaration `helper`.
    let (refs, partials) = h.usages(doc, Pos::new(3, 10), 1).await;
    assert!(refs.approximate);
    assert_eq!(refs.incomplete, None);
    let found: Vec<(String, u32, u32)> =
        refs.locations.iter().map(|l| (h.rel(&l.path.0), l.range.start.line, l.range.start.col)).collect();
    assert_eq!(
        found,
        [
            ("src/App.java".to_owned(), 3, 8),
            ("src/App.java".to_owned(), 4, 17),
            ("src/App.java".to_owned(), 4, 33),
            ("src/App.java".to_owned(), 5, 37),
            ("src/Util.java".to_owned(), 1, 26),
            ("src/tool.py".to_owned(), 0, 4),
            ("src/tool.py".to_owned(), 7, 4),
        ]
    );
    for refs in partials {
        assert!(refs.approximate);
        assert!(refs.locations.iter().all(|l| found.iter().any(|(p, line, col)| {
            *p == h.rel(&l.path.0) && *line == l.range.start.line && *col == l.range.start.col
        })));
    }
    // A local the syntax tree resolves stays in its scope: no project search, exact result.
    let (refs, _) = h.usages(doc, Pos::new(3, 19), 4).await;
    assert!(!refs.approximate);
    assert_eq!(refs.locations.len(), 1);
    assert_eq!(refs.locations[0].range.start, Pos::new(3, 31));
    // The word before a caret at the end of an identifier, as the editor picks it.
    let (refs, _) = h.usages(doc, Pos::new(4, 23), 2).await;
    assert_eq!(refs.locations.len(), 7);
    // Unsaved editor text counts, the disk copy does not.
    let project = h.project();
    let edit = ide_core::TextEdit { range: ide_core::Range::default(), new_text: "int helper2 = helper(0);\n".into() };
    project.documents.apply_edits(doc, &[edit]).unwrap();
    let (refs, _) = h.usages(doc, Pos::new(4, 10), 3).await;
    assert_eq!(refs.locations.len(), 8);
    assert_eq!(refs.locations[0].range.start, Pos::new(0, 14));
    h.host.close_project("main");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn text_usages_honour_cancellation_and_refuse_non_identifiers() {
    let h = Harness::new();
    let project = h.project();
    let syntax = h.host.syntax();
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let err = ops::text_usages::text_references(&project, &syntax, "helper", &cancelled, &|_| {}).unwrap_err();
    assert_eq!(err.kind, ide_core::error::ErrorKind::Cancelled);
    let err =
        ops::text_usages::text_references(&project, &syntax, "a.b", &CancellationToken::new(), &|_| {}).unwrap_err();
    assert_eq!(err.kind, ide_core::error::ErrorKind::NotFound);
    let doc = h.open("src/App.java");
    let err = ops::text_usages::call_hierarchy(
        &project,
        &syntax,
        doc,
        Pos::new(4, 20),
        HierarchyDirection::Incoming,
        &cancelled,
    )
    .unwrap_err();
    assert_eq!(err.kind, ide_core::error::ErrorKind::Cancelled);
    // Cancelling the request while the search runs ends it as cancelled.
    let request = RequestId(7);
    let host = h.host.clone();
    let search = tokio::spawn(async move {
        ops::lsp::lsp_references(&host, "main", request, doc, Pos::new(3, 10), false, true, false, Channel::new(|_| {}))
            .await
    });
    h.host.cancel_request(request);
    let result = search.await.unwrap();
    assert!(result.is_ok() || result.as_ref().unwrap_err().kind == ide_core::error::ErrorKind::Cancelled, "{result:?}");
    h.host.close_project("main");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn call_hierarchy_without_a_server_groups_text_calls_by_the_enclosing_method() {
    let h = Harness::new();
    let project = h.project();
    assert!(project.index.wait_idle(Duration::from_secs(30)));
    let doc = h.open("src/App.java");
    // Callers of `helper`, from inside its body.
    let incoming =
        ops::lsp::lsp_call_hierarchy(&h.host, "main", RequestId(1), doc, Pos::new(3, 26), HierarchyDirection::Incoming)
            .await
            .unwrap();
    assert!(incoming.approximate);
    let callers: Vec<(String, String, usize)> =
        incoming.items.iter().map(|i| (h.rel(&i.path.0), i.name.clone(), i.from_ranges.len())).collect();
    assert_eq!(
        callers,
        [
            ("src/App.java".to_owned(), "run".to_owned(), 2),
            ("src/App.java".to_owned(), "other".to_owned(), 1),
            ("src/Util.java".to_owned(), "go".to_owned(), 1),
            ("src/tool.py".to_owned(), "main".to_owned(), 1),
        ]
    );
    let run = &incoming.items[0];
    assert_eq!(run.selection_range.start, Pos::new(4, 9));
    assert_eq!(run.from_ranges[0], ide_core::Range::new(Pos::new(4, 17), Pos::new(4, 23)));
    // Callees of `run`: `helper` resolved through the index; `this` is no declaration.
    let outgoing =
        ops::lsp::lsp_call_hierarchy(&h.host, "main", RequestId(2), doc, Pos::new(4, 12), HierarchyDirection::Outgoing)
            .await
            .unwrap();
    assert!(outgoing.approximate);
    let callees: Vec<(String, String, u32, usize)> = outgoing
        .items
        .iter()
        .map(|i| (h.rel(&i.path.0), i.name.clone(), i.selection_range.start.line, i.from_ranges.len()))
        .collect();
    assert_eq!(
        callees,
        [("src/App.java".to_owned(), "helper".to_owned(), 3, 2), ("src/tool.py".to_owned(), "helper".to_owned(), 0, 2)]
    );
    // Outside any method there is no hierarchy to show.
    let err =
        ops::lsp::lsp_call_hierarchy(&h.host, "main", RequestId(3), doc, Pos::new(0, 0), HierarchyDirection::Incoming)
            .await
            .unwrap_err();
    assert_eq!(err.kind, ide_core::error::ErrorKind::NotFound);
    // Python: the same by its grammar.
    let tool = h.open("src/tool.py");
    let incoming =
        ops::lsp::lsp_call_hierarchy(&h.host, "main", RequestId(4), tool, Pos::new(1, 4), HierarchyDirection::Incoming)
            .await
            .unwrap();
    let callers: Vec<(String, String)> = incoming.items.iter().map(|i| (h.rel(&i.path.0), i.name.clone())).collect();
    assert_eq!(
        callers,
        [
            ("src/App.java".to_owned(), "run".to_owned()),
            ("src/App.java".to_owned(), "other".to_owned()),
            ("src/Util.java".to_owned(), "go".to_owned()),
            ("src/tool.py".to_owned(), "main".to_owned()),
        ]
    );
    h.host.close_project("main");
}

/// Timing of the text search on a synthetic tree: `LIGHT_MODE_BENCH_FILES` Java files
/// (default 15 000), or the project at `LIGHT_MODE_BENCH_ROOT`. Prints the elapsed time.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn text_usages_timing_on_a_large_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = match std::env::var_os("LIGHT_MODE_BENCH_ROOT") {
        Some(root) => PathBuf::from(root),
        None => {
            let root = dunce::canonicalize(tmp.path()).unwrap().join("project");
            let files: usize =
                std::env::var("LIGHT_MODE_BENCH_FILES").ok().and_then(|v| v.parse().ok()).unwrap_or(15_000);
            for i in 0..files {
                let dir = root.join(format!("src/main/java/com/acme/m{}", i % 200));
                std::fs::create_dir_all(&dir).unwrap();
                let body: String = (0..40)
                    .map(|j| format!("    int m{j}(int x) {{ return x + {j}; }} // helper note {j}\n    String s{j} = \"helper {j}\";\n"))
                    .collect();
                let call = if i % 20 == 0 { "    int use() { return helper(1); }\n" } else { "" };
                std::fs::write(
                    dir.join(format!("C{i}.java")),
                    format!("package com.acme.m{};\nclass C{i} {{\n{body}{call}}}\n", i % 200),
                )
                .unwrap();
            }
            std::fs::write(
                root.join("src/main/java/com/acme/Helper.java"),
                "class Helper { int helper(int x) { return x; } }\n",
            )
            .unwrap();
            root
        }
    };
    let host = Arc::new(Host::new(HostConfig {
        config_dir: tmp.path().join("config"),
        data_dir: tmp.path().join("data"),
        local_data_dir: tmp.path().join("local"),
        languages: Arc::new(ide_language::LanguageRegistry::load(None).unwrap()),
        syntax: Arc::default(),
        sink_for: Box::new(|_| Arc::new(ide_core::NullSink)),
        owner: |w| w,
        runtime: tokio::runtime::Handle::current(),
        project_environment: false,
    }));
    host.trust.decide(&root, true, false).unwrap();
    let project = host.open_project("main", &root).unwrap();
    let syntax = host.syntax();
    let name = std::env::var("LIGHT_MODE_BENCH_NAME").unwrap_or_else(|_| "helper".into());
    for round in 0..3 {
        let started = std::time::Instant::now();
        let refs =
            ops::text_usages::text_references(&project, &syntax, &name, &CancellationToken::new(), &|_| {}).unwrap();
        eprintln!("round {round}: {} text usages of {name} in {:?}", refs.locations.len(), started.elapsed());
    }
    host.close_project("main");
}
