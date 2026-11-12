//! Attach to Process: the popup model (filter + groups by debugger) and the attach request each
//! debugger needs.

use crate::debugger::effects::ipc;
use crate::debugger::java::JavaAttachTarget;
use ide_dap::LaunchConfig;
use ide_dap::attach::{AttachKind, AttachableProcess};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// The debuggers in the order the popup lists them.
const KIND_ORDER: [AttachKind; 5] =
    [AttachKind::Java, AttachKind::Node, AttachKind::Python, AttachKind::Go, AttachKind::Native];

/// The header of a debugger's group.
pub fn kind_label(kind: AttachKind) -> &'static str {
    match kind {
        AttachKind::Java => "Java (JDWP)",
        AttachKind::Node => "Node.js",
        AttachKind::Python => "Python (debugpy)",
        AttachKind::Go => "Go (Delve)",
        AttachKind::Native => "Native (LLDB)",
    }
}

/// The name row keys are built from.
fn kind_id(kind: AttachKind) -> &'static str {
    match kind {
        AttachKind::Java => "java",
        AttachKind::Node => "node",
        AttachKind::Python => "python",
        AttachKind::Go => "go",
        AttachKind::Native => "native",
    }
}

/// A row of the popup.
#[derive(Debug, Clone, PartialEq)]
pub enum AttachRow {
    Group { key: String, label: &'static str, count: usize },
    Process { key: String, process: AttachableProcess },
}

impl AttachRow {
    pub fn key(&self) -> &str {
        match self {
            Self::Group { key, .. } | Self::Process { key, .. } => key,
        }
    }
}

/// Case-insensitive match on pid, name and command line; every word must match.
pub fn matches_filter(p: &AttachableProcess, filter: &str) -> bool {
    let filter = filter.to_lowercase();
    let hay = format!("{} {} {}", p.pid, p.name, p.command_line).to_lowercase();
    filter.split_whitespace().all(|word| hay.contains(word))
}

/// Rows of the popup: a header per debugger, then its processes (in the order listed).
pub fn attach_rows(list: &[AttachableProcess], filter: &str) -> Vec<AttachRow> {
    let mut rows = Vec::new();
    for kind in KIND_ORDER {
        let items: Vec<&AttachableProcess> =
            list.iter().filter(|p| p.kind == kind && matches_filter(p, filter)).collect();
        if items.is_empty() {
            continue;
        }
        let id = kind_id(kind);
        rows.push(AttachRow::Group { key: format!("group:{id}"), label: kind_label(kind), count: items.len() });
        for p in items {
            rows.push(AttachRow::Process { key: format!("{id}:{}", p.pid), process: p.clone() });
        }
    }
    rows
}

/// A short description of how the process will be attached to.
pub fn attach_detail(p: &AttachableProcess) -> String {
    match p.port {
        Some(port) => format!("{}:{port}", p.host.as_deref().unwrap_or("localhost")),
        None => format!("pid {}", p.pid),
    }
}

/// How to attach to a process.
#[derive(Debug, Clone)]
pub enum AttachRequest {
    /// The request is ready.
    Launch(Box<LaunchConfig>),
    /// A JVM: the request is built with the Java language server (`JavaFlow::attach`).
    Java(JavaAttachTarget),
}

fn base(p: &AttachableProcess, cwd: &str, extra: Value) -> AttachRequest {
    AttachRequest::Launch(Box::new(LaunchConfig {
        name: format!("{} ({})", p.name, p.pid),
        adapter: p.adapter.clone(),
        request: "attach".to_owned(),
        program: None,
        args: Vec::new(),
        cwd: Some(ipc(cwd)),
        env: BTreeMap::new(),
        extra_json: Some(extra.to_string()),
    }))
}

/// The attach request for `p`; `root` is the project root (sources, java module).
/// `skip_library_code`: Settings › Debugger › Stepping › "Do not step into library code".
pub fn attach_config(p: &AttachableProcess, root: &str, skip_library_code: bool) -> AttachRequest {
    let host = p.host.as_deref().unwrap_or("localhost");
    match p.kind {
        AttachKind::Java => AttachRequest::Java(JavaAttachTarget {
            name: format!("{} ({})", p.name, p.pid),
            module: String::new(),
            host_name: host.to_owned(),
            port: p.port.unwrap_or_default(),
        }),
        AttachKind::Node => {
            let skip_files: &[&str] =
                if skip_library_code { &["<node_internals>/**", "**/node_modules/**"] } else { &[] };
            let mut extra = Map::new();
            extra.insert("type".to_owned(), json!("pwa-node"));
            match p.port {
                Some(port) => {
                    extra.insert("address".to_owned(), json!(host));
                    extra.insert("port".to_owned(), json!(port));
                }
                None => {
                    extra.insert("processId".to_owned(), json!(p.pid.to_string()));
                }
            }
            extra.insert("skipFiles".to_owned(), json!(skip_files));
            // Without `--inspect`, js-debug enables the inspector by signalling the process.
            extra.insert("continueOnAttach".to_owned(), json!(true));
            base(p, root, Value::Object(extra))
        }
        // `debugpy --listen` runs its own adapter on that port: talk DAP to it directly.
        // (`--pid` injection ships only an x86_64 helper on macOS.)
        AttachKind::Python => match p.port {
            Some(port) => base(p, root, json!({ "debugServer": port, "justMyCode": skip_library_code })),
            None => base(p, root, json!({ "processId": p.pid, "justMyCode": skip_library_code })),
        },
        AttachKind::Go => base(p, root, json!({ "mode": "local", "processId": p.pid })),
        AttachKind::Native => base(p, root, json!({ "pid": p.pid })),
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// A process `kind` debugs, named after it, with nothing listening.
    pub fn proc(pid: u32, kind: AttachKind) -> AttachableProcess {
        let adapter = match kind {
            AttachKind::Java => "java-debug",
            AttachKind::Node => "js-debug",
            AttachKind::Python => "debugpy",
            AttachKind::Go => "delve",
            AttachKind::Native => "codelldb",
        };
        AttachableProcess {
            pid,
            name: kind_id(kind).to_owned(),
            command_line: format!("{} app", kind_id(kind)),
            kind,
            adapter: adapter.to_owned(),
            host: None,
            port: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::proc;
    use super::*;
    use crate::debugger::effects::path_text;
    use crate::debugger::launch::testing::{extra, includes};

    fn named(pid: u32, kind: AttachKind, name: &str, command_line: &str, port: Option<u16>) -> AttachableProcess {
        AttachableProcess { name: name.to_owned(), command_line: command_line.to_owned(), port, ..proc(pid, kind) }
    }

    fn list() -> Vec<AttachableProcess> {
        vec![
            named(30, AttachKind::Native, "a.out", "./a.out", None),
            named(10, AttachKind::Java, "java", "java -agentlib:jdwp=... -jar shop.jar", Some(5005)),
            named(20, AttachKind::Python, "python3", "python3 worker.py", None),
            named(21, AttachKind::Python, "python3", "python3 -m debugpy --listen 5678 api.py", Some(5678)),
            named(25, AttachKind::Node, "node", "node server.js", None),
            named(26, AttachKind::Go, "gosrv", "./gosrv", None),
        ]
    }

    fn launch(p: &AttachableProcess) -> LaunchConfig {
        match attach_config(p, "/proj", true) {
            AttachRequest::Launch(launch) => *launch,
            AttachRequest::Java(target) => panic!("a JVM is attached to through jdtls: {target:?}"),
        }
    }

    #[test]
    fn groups_processes_by_debugger_in_ideas_order_with_counts() {
        let rows = attach_rows(&list(), "");
        let groups: Vec<String> = rows
            .iter()
            .filter_map(|row| match row {
                AttachRow::Group { label, count, .. } => Some(format!("{label}:{count}")),
                AttachRow::Process { .. } => None,
            })
            .collect();
        assert_eq!(groups, ["Java (JDWP):1", "Node.js:1", "Python (debugpy):2", "Go (Delve):1", "Native (LLDB):1"]);
        assert!(matches!(rows[0], AttachRow::Group { .. }));
        assert!(matches!(&rows[1], AttachRow::Process { process, .. } if process.pid == 10));
    }

    #[test]
    fn filters_on_pid_name_and_command_line_all_words() {
        let list = list();
        assert!(matches_filter(&list[2], "worker"));
        assert!(matches_filter(&list[2], "20"));
        assert!(!matches_filter(&list[2], "python api"));
        let rows = attach_rows(&list, "debugpy");
        assert_eq!(rows.iter().map(AttachRow::key).collect::<Vec<_>>(), ["group:python", "python:21"]);
        assert!(attach_rows(&list, "nothing-matches").is_empty());
    }

    #[test]
    fn describes_how_each_process_is_reached() {
        let list = list();
        assert_eq!(attach_detail(&list[1]), "localhost:5005");
        assert_eq!(attach_detail(&list[2]), "pid 20");
    }

    #[test]
    fn builds_the_attach_request_of_each_debugger() {
        let list = list();
        // A JVM: java-debug attaches to its JDWP port, through the language server.
        let AttachRequest::Java(java) = attach_config(&list[1], "/proj", true) else { panic!("a JVM target") };
        assert_eq!((java.host_name.as_str(), java.port, java.module.as_str()), ("localhost", 5005, ""));
        assert_eq!(java.name, "java (10)");
        // debugpy: inject by pid, or talk to a `--listen` adapter directly.
        assert!(includes(&extra(&launch(&list[2])), &json!({ "processId": 20 })));
        assert!(includes(&extra(&launch(&list[3])), &json!({ "debugServer": 5678 })));
        // js-debug: the backend opens the inspector of a process started without --inspect.
        assert!(includes(&extra(&launch(&list[4])), &json!({ "type": "pwa-node", "processId": "25" })));
        let listening = AttachableProcess { port: Some(9230), ..proc(27, AttachKind::Node) };
        assert!(includes(&extra(&launch(&listening)), &json!({ "port": 9230, "address": "localhost" })));
        assert_eq!(extra(&launch(&list[5])), json!({ "mode": "local", "processId": 26 }));
        let native = launch(&list[0]);
        assert_eq!((native.adapter.as_str(), native.request.as_str()), ("codelldb", "attach"));
        assert_eq!(native.name, "a.out (30)");
        assert_eq!(native.cwd.as_ref().map(path_text).as_deref(), Some("/proj"));
        assert_eq!(extra(&native), json!({ "pid": 30 }));
    }
}
