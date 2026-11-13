//! The language servers' status: a snapshot (`lsp_servers`) kept current by the
//! `languageServer` / `languageServerRemoved` events.
//!
//! A snapshot asked for before an event may answer after it: each server remembers the event count
//! at its latest event, and the count when it left the list, so that an older snapshot neither
//! undoes a newer status nor brings a removed server back.

use crate::platform::collate;
use ide_core::{ServerId, ServerStatus};
use ide_lsp::types::ServerInfo;
use std::collections::{HashMap, HashSet};

/// A language server as the status bar lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerEntry {
    pub id: ServerId,
    pub name: String,
    pub status: ServerStatus,
    /// Why it failed or is missing, what to change.
    pub message: Option<String>,
    pub languages: Vec<String>,
    pub root: Option<String>,
}

/// What the servers' status needs carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerEffect {
    /// Ask for `lsp_servers()` and answer with [`Servers::snapshot`] and `since`.
    Snapshot { since: u64 },
    /// Tell the user `name` is not installed (nothing else offers to install it).
    NotifyMissing { name: String, title: String, body: String },
}

/// The servers of the window's project, in the order they were first reported.
#[derive(Debug, Clone, Default)]
pub struct Servers {
    servers: Vec<ServerEntry>,
    event_seq: u64,
    last_event: HashMap<ServerId, u64>,
    removed_at: HashMap<ServerId, u64>,
    notified_missing: HashSet<String>,
}

/// How a status reads in the list.
pub fn status_label(status: ServerStatus) -> &'static str {
    match status {
        ServerStatus::Starting => "Starting…",
        ServerStatus::Ready => "Running",
        ServerStatus::Busy => "Busy",
        ServerStatus::Crashed => "Crashed",
        ServerStatus::Stopped => "Stopped",
        ServerStatus::Missing => "Not installed",
        ServerStatus::Misconfigured => "Not started",
    }
}

/// The colour of a status dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerTone {
    Ok,
    Busy,
    Error,
    Idle,
}

pub fn tone_of(status: ServerStatus) -> ServerTone {
    match status {
        ServerStatus::Ready => ServerTone::Ok,
        ServerStatus::Starting | ServerStatus::Busy => ServerTone::Busy,
        ServerStatus::Crashed | ServerStatus::Missing | ServerStatus::Misconfigured => ServerTone::Error,
        ServerStatus::Stopped => ServerTone::Idle,
    }
}

/// A missing server whose languages another installed one serves (vtsls missing,
/// typescript-language-server running as its fallback) is not a problem.
pub fn served_by_other(server: &ServerEntry, servers: &[ServerEntry]) -> bool {
    !server.languages.is_empty()
        && server.languages.iter().all(|language| {
            servers.iter().any(|other| {
                other.id != server.id && other.status != ServerStatus::Missing && other.languages.contains(language)
            })
        })
}

/// The tone of a server's own dot in the popup.
pub fn entry_tone(server: &ServerEntry, servers: &[ServerEntry]) -> ServerTone {
    if server.status == ServerStatus::Missing && served_by_other(server, servers) {
        ServerTone::Idle
    } else {
        tone_of(server.status)
    }
}

/// The status bar's one-line summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServersSummary {
    pub text: String,
    pub tone: ServerTone,
}

/// The status bar's summary: the servers' names (the first, "+N" more) and the worst state; `None`
/// without servers.
pub fn servers_summary(servers: &[ServerEntry]) -> Option<ServersSummary> {
    if servers.is_empty() {
        return None;
    }
    let bad = servers.iter().any(|server| match server.status {
        ServerStatus::Crashed | ServerStatus::Misconfigured => true,
        ServerStatus::Missing => !served_by_other(server, servers),
        _ => false,
    });
    let busy = servers.iter().any(|server| matches!(server.status, ServerStatus::Starting | ServerStatus::Busy));
    let tone = if bad {
        ServerTone::Error
    } else if busy {
        ServerTone::Busy
    } else if servers.iter().any(|server| server.status == ServerStatus::Ready) {
        ServerTone::Ok
    } else {
        ServerTone::Idle
    };
    // A stopped server (one a preferred server took over from) isn't worth naming next to running ones.
    let active: Vec<&ServerEntry> = servers.iter().filter(|server| server.status != ServerStatus::Stopped).collect();
    let named: Vec<&ServerEntry> = if active.is_empty() { servers.iter().collect() } else { active };
    let mut names: Vec<&str> = Vec::new();
    for server in named {
        if !names.contains(&server.name.as_str()) {
            names.push(&server.name);
        }
    }
    let text = match names.as_slice() {
        [one] => (*one).to_owned(),
        [first, rest @ ..] => format!("{first} +{}", rest.len()),
        [] => return None,
    };
    Some(ServersSummary { text, tone })
}

impl Servers {
    /// The servers in the order they were first reported.
    pub fn list(&self) -> &[ServerEntry] {
        &self.servers
    }

    /// The servers by name, as the popup lists them.
    pub fn sorted(&self) -> Vec<ServerEntry> {
        let mut sorted = self.servers.clone();
        sorted.sort_by(|a, b| collate::compare(&a.name, &b.name));
        sorted
    }

    pub fn get(&self, id: ServerId) -> Option<&ServerEntry> {
        self.servers.iter().find(|server| server.id == id)
    }

    fn notify_missing(&mut self, entry: &ServerEntry, install_handler: bool) -> Option<ServerEffect> {
        if !self.notified_missing.insert(entry.name.clone()) || install_handler {
            // The tools feature shows the same notification with an Install button.
            return None;
        }
        Some(ServerEffect::NotifyMissing {
            name: entry.name.clone(),
            title: format!("Language server '{}' is not installed", entry.name),
            body: entry.message.clone().unwrap_or_else(|| {
                "Code insight features are unavailable for these files until it is installed.".to_owned()
            }),
        })
    }

    /// A `languageServer` event. `install_handler`: something offers to install missing servers. A
    /// server not listed yet asks for a snapshot (its languages and root).
    pub fn server_event(
        &mut self,
        id: ServerId,
        name: &str,
        status: ServerStatus,
        message: Option<String>,
        install_handler: bool,
    ) -> Vec<ServerEffect> {
        self.event_seq += 1;
        self.last_event.insert(id, self.event_seq);
        let known = self.get(id).is_some();
        let entry = match self.servers.iter_mut().find(|server| server.id == id) {
            Some(entry) => {
                (entry.name, entry.status, entry.message) = (name.to_owned(), status, message);
                entry.clone()
            }
            None => {
                let entry =
                    ServerEntry { id, name: name.to_owned(), status, message, languages: Vec::new(), root: None };
                self.servers.push(entry.clone());
                entry
            }
        };
        let mut effects = Vec::new();
        if entry.status == ServerStatus::Missing {
            effects.extend(self.notify_missing(&entry, install_handler));
        }
        if !known {
            effects.push(ServerEffect::Snapshot { since: self.event_seq });
        }
        effects
    }

    /// A `languageServerRemoved` event.
    pub fn removed(&mut self, id: ServerId) {
        self.event_seq += 1;
        self.removed_at.insert(id, self.event_seq);
        self.last_event.remove(&id);
        self.servers.retain(|server| server.id != id);
    }

    /// The window's project changed (`has_project`: one is open): the list is asked for again, and
    /// missing servers are told about again.
    pub fn project_changed(&mut self, has_project: bool) -> Vec<ServerEffect> {
        self.notified_missing.clear();
        if has_project {
            vec![ServerEffect::Snapshot { since: self.event_seq }]
        } else {
            self.servers.clear();
            Vec::new()
        }
    }

    /// The `lsp_servers()` snapshot asked for when the event count was `since`.
    pub fn snapshot(&mut self, since: u64, list: Vec<ServerInfo>, install_handler: bool) -> Vec<ServerEffect> {
        let mut next = Vec::new();
        for info in list {
            if self.removed_at.get(&info.id).copied().unwrap_or(0) > since {
                continue;
            }
            let previous = self.get(info.id);
            let newer = previous.is_some() && self.last_event.get(&info.id).copied().unwrap_or(0) > since;
            next.push(ServerEntry {
                id: info.id,
                name: info.name,
                status: match previous {
                    Some(previous) if newer => previous.status,
                    _ => info.status,
                },
                message: previous.and_then(|previous| previous.message.clone()),
                languages: info.languages.into_iter().map(|language| language.0).collect(),
                root: Some(ide_core::ipc_path::to_ipc(&info.root)),
            });
        }
        self.servers = next;
        let missing: Vec<ServerEntry> =
            self.servers.iter().filter(|server| server.status == ServerStatus::Missing).cloned().collect();
        missing.iter().filter_map(|entry| self.notify_missing(entry, install_handler)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::IpcPath;
    use ide_core::LanguageId;
    use std::path::PathBuf;

    fn info(id: u32, name: &str, languages: &[&str], status: ServerStatus) -> ServerInfo {
        ServerInfo {
            id: ServerId(id),
            name: name.to_owned(),
            root: IpcPath::from(PathBuf::from("/p")),
            languages: languages.iter().map(|language| LanguageId::new(*language)).collect(),
            status,
            pid: None,
        }
    }

    fn entry(id: u32, name: &str, status: ServerStatus, languages: &[&str]) -> ServerEntry {
        ServerEntry {
            id: ServerId(id),
            name: name.to_owned(),
            status,
            message: None,
            languages: languages.iter().map(|language| (*language).to_owned()).collect(),
            root: Some("/p".to_owned()),
        }
    }

    fn since(effects: &[ServerEffect]) -> u64 {
        effects
            .iter()
            .find_map(|effect| match effect {
                ServerEffect::Snapshot { since } => Some(*since),
                ServerEffect::NotifyMissing { .. } => None,
            })
            .expect("a snapshot is asked for")
    }

    #[test]
    fn keeps_a_status_that_arrived_after_the_snapshot_was_requested() {
        let mut servers = Servers::default();
        let id = ServerId(1);
        // The first event asks for a snapshot; the server reports missing before it answers.
        let asked = since(&servers.server_event(id, "jdtls", ServerStatus::Starting, None, false));
        let missing = servers.server_event(id, "jdtls", ServerStatus::Missing, Some("install jdtls".to_owned()), false);
        assert!(matches!(missing.as_slice(), [ServerEffect::NotifyMissing { name, .. }] if name == "jdtls"));
        servers.snapshot(asked, vec![info(1, "jdtls", &[], ServerStatus::Starting)], false);
        let kept = servers.get(id).expect("listed");
        assert_eq!((kept.status, kept.message.as_deref()), (ServerStatus::Missing, Some("install jdtls")));
        assert!(kept.languages.is_empty());
    }

    #[test]
    fn drops_a_server_the_backend_removed_even_if_an_older_snapshot_still_lists_it() {
        let mut servers = Servers::default();
        let asked = since(&servers.project_changed(true));
        let python = |id: u32, name: &str| info(id, name, &["python"], ServerStatus::Ready);
        servers.snapshot(asked, vec![python(11, "basedpyright"), python(12, "pyright")], false);
        assert_eq!(servers.list().len(), 2);
        let text = |servers: &Servers| servers_summary(servers.list()).map(|summary| summary.text);
        assert_eq!(text(&servers).as_deref(), Some("basedpyright +1"));

        // The fallback stops and leaves the list; a snapshot taken before that answers late.
        let asked = since(&servers.server_event(ServerId(13), "ty", ServerStatus::Starting, None, false));
        servers.server_event(ServerId(12), "pyright", ServerStatus::Stopped, None, false);
        servers.removed(ServerId(12));
        assert!(servers.get(ServerId(12)).is_none());
        servers.snapshot(asked, vec![python(11, "basedpyright"), python(12, "pyright"), python(13, "ty")], false);
        assert_eq!(servers.get(ServerId(13)).map(|server| server.languages.clone()), Some(vec!["python".to_owned()]));
        let ids: Vec<u32> = servers.list().iter().map(|server| server.id.0).collect();
        assert_eq!(ids, [11, 13]);

        servers.removed(ServerId(13));
        assert_eq!(text(&servers).as_deref(), Some("basedpyright"));
    }

    #[test]
    fn doesnt_flag_a_missing_server_whose_languages_another_one_serves() {
        let ts = ["typescript"];
        let pair = [
            entry(1, "vtsls", ServerStatus::Missing, &ts),
            entry(2, "typescript-language-server", ServerStatus::Ready, &ts),
        ];
        assert_eq!(servers_summary(&pair).map(|summary| summary.tone), Some(ServerTone::Ok));
        let alone = [entry(1, "vtsls", ServerStatus::Missing, &ts)];
        assert_eq!(servers_summary(&alone).map(|summary| summary.tone), Some(ServerTone::Error));
        assert_eq!(entry_tone(&pair[0], &pair), ServerTone::Idle);
    }

    #[test]
    fn doesnt_count_stopped_servers_next_to_running_ones() {
        let ts = ["typescript"];
        let text = |servers: &[ServerEntry]| servers_summary(servers).map(|summary| summary.text);
        let ready = entry(1, "basedpyright", ServerStatus::Ready, &ts);
        let stopped = [ready.clone(), entry(2, "pyright", ServerStatus::Stopped, &ts)];
        assert_eq!(text(&stopped).as_deref(), Some("basedpyright"));
        let starting = [ready, entry(2, "ty", ServerStatus::Starting, &ts)];
        assert_eq!(text(&starting).as_deref(), Some("basedpyright +1"));
        let only = [entry(2, "pyright", ServerStatus::Stopped, &ts)];
        assert_eq!(servers_summary(&only), Some(ServersSummary { text: "pyright".to_owned(), tone: ServerTone::Idle }));
    }

    #[test]
    fn summarises_names_and_the_worst_state() {
        let s = |id: u32, name: &str, status: ServerStatus| entry(id, name, status, &[]);
        let summary = |text: &str, tone: ServerTone| Some(ServersSummary { text: text.to_owned(), tone });
        assert_eq!(servers_summary(&[]), None);
        let rust = [s(1, "rust-analyzer", ServerStatus::Ready)];
        assert_eq!(servers_summary(&rust), summary("rust-analyzer", ServerTone::Ok));
        assert_eq!(
            servers_summary(&[s(1, "vtsls", ServerStatus::Ready), s(2, "eslint", ServerStatus::Starting)]),
            summary("vtsls +1", ServerTone::Busy)
        );
        let tone = |status: ServerStatus| servers_summary(&[s(1, "x", status)]).map(|summary| summary.tone);
        assert_eq!(tone(ServerStatus::Missing), Some(ServerTone::Error));
        assert_eq!(tone(ServerStatus::Misconfigured), Some(ServerTone::Error));
    }

    // Not from a spec: a missing server is told about once per project, and not at all when the tools
    // feature offers to install it.
    #[test]
    fn tells_about_a_missing_server_once_per_project() {
        let mut servers = Servers::default();
        let first = servers.server_event(ServerId(1), "gopls", ServerStatus::Missing, None, false);
        let told = |effect: &ServerEffect| matches!(effect, ServerEffect::NotifyMissing { .. });
        assert!(matches!(first.first(), Some(ServerEffect::NotifyMissing { body, .. }) if body.starts_with("Code")));
        assert!(!servers.server_event(ServerId(1), "gopls", ServerStatus::Missing, None, false).iter().any(told));
        servers.project_changed(true);
        let installable = servers.server_event(ServerId(1), "gopls", ServerStatus::Missing, None, true);
        assert!(installable.is_empty());
        assert!(servers.project_changed(false).is_empty());
        assert!(servers.list().is_empty());
        assert_eq!(status_label(ServerStatus::Misconfigured), "Not started");
    }
}
