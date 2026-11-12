//! Pure helpers of deployment: the files an action targets, the Sync dialog's defaults, the
//! server an action uses, transfer reports, remote paths and what to do about a failed SSH
//! connection.

use super::types::{ResolvedMapping, SyncAction, SyncEntry, SyncStatus, SyncStep, TransferReport};
use crate::platform::i18n::{I18n, Param};
use crate::workbench::notifications::NotificationLevel;
use chrono::{DateTime, Datelike as _, Timelike as _};
use ide_core::IdeError;
use ide_core::error::ErrorKind;
use ide_core::ipc_path::to_ipc;
use ide_core::settings::{DeploymentServer, DeploymentSettings, DeploymentType, SshAuth, SshConfiguration};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

pub use crate::settings_model::deployment_model::remote_parent;

/// Which part had focus last: the project view's selection or the editor's file is the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPart {
    Project,
    Editor,
}

/// Paths passed to an action: a path, a list of paths or `{ "paths": [...] }`; `None` when none
/// were passed. Entries that are not non-empty strings are dropped.
pub fn paths_from_arg(arg: &Value) -> Option<Vec<String>> {
    let list = match arg {
        Value::String(path) => return (!path.is_empty()).then(|| vec![path.clone()]),
        Value::Array(list) => list,
        Value::Object(fields) => match fields.get("paths") {
            Some(Value::Array(list)) => list,
            _ => return None,
        },
        _ => return None,
    };
    let paths: Vec<String> =
        list.iter().filter_map(Value::as_str).filter(|path| !path.is_empty()).map(str::to_owned).collect();
    (!paths.is_empty()).then_some(paths)
}

/// What an action run without paths falls back to.
#[derive(Debug, Clone, Copy)]
pub struct TargetContext<'a> {
    pub part: Option<FocusedPart>,
    pub selection: &'a [String],
    pub editor_path: Option<&'a str>,
    pub project_root: Option<&'a str>,
}

fn is_under(path: &str, folder: &str) -> bool {
    path.strip_prefix(folder).is_some_and(|rest| rest.starts_with('/'))
}

/// Files an action works on: the paths passed by a menu, else the project view selection when it
/// had focus last, else the active editor's file. Paths outside the project are dropped, and so
/// are paths inside another target folder (the folder covers them).
pub fn target_paths(arg: &Value, ctx: &TargetContext) -> Vec<String> {
    let editor = ctx.editor_path.filter(|path| !path.is_empty());
    let picked = match paths_from_arg(arg) {
        Some(passed) => passed,
        None if ctx.part == Some(FocusedPart::Project) && !ctx.selection.is_empty() => ctx.selection.to_vec(),
        None => match editor {
            Some(editor) => vec![editor.to_owned()],
            None if !ctx.selection.is_empty() && ctx.part != Some(FocusedPart::Editor) => ctx.selection.to_vec(),
            None => Vec::new(),
        },
    };
    let Some(root) = ctx.project_root.filter(|root| !root.is_empty()) else { return Vec::new() };
    let inside: Vec<String> = picked.into_iter().filter(|path| path == root || is_under(path, root)).collect();
    inside.iter().filter(|path| !inside.iter().any(|other| other != *path && is_under(path, other))).cloned().collect()
}

/// An operation of a Sync dialog row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncChoice {
    Skip,
    Upload,
    Download,
    DeleteRemote,
    DeleteLocal,
}

impl SyncChoice {
    /// The step the backend runs; `None` for a skipped row.
    pub fn action(self) -> Option<SyncAction> {
        match self {
            Self::Skip => None,
            Self::Upload => Some(SyncAction::Upload),
            Self::Download => Some(SyncAction::Download),
            Self::DeleteRemote => Some(SyncAction::DeleteRemote),
            Self::DeleteLocal => Some(SyncAction::DeleteLocal),
        }
    }
}

/// The chosen operation per row, in the order rows were first given one (the order steps run in).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncChoices(Vec<(String, SyncChoice)>);

impl SyncChoices {
    pub fn from_pairs(pairs: &[(&str, SyncChoice)]) -> Self {
        let mut choices = Self::default();
        for (path, choice) in pairs {
            choices.set(path, *choice);
        }
        choices
    }

    pub fn get(&self, path: &str) -> Option<SyncChoice> {
        self.0.iter().find(|(seen, _)| seen == path).map(|(_, choice)| *choice)
    }

    /// Changes a row's operation in place, or adds the row at the end.
    pub fn set(&mut self, path: &str, choice: SyncChoice) {
        match self.0.iter_mut().find(|(seen, _)| seen == path) {
            Some(row) => row.1 = choice,
            None => self.0.push((path.to_owned(), choice)),
        }
    }

    pub fn remove(&mut self, path: &str) {
        self.0.retain(|(seen, _)| seen != path);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, SyncChoice)> {
        self.0.iter().map(|(path, choice)| (path.as_str(), *choice))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The Sync dialog's preselected action for a row.
pub fn default_sync_action(status: SyncStatus) -> SyncChoice {
    match status {
        SyncStatus::LocalOnly | SyncStatus::LocalNewer | SyncStatus::Different => SyncChoice::Upload,
        SyncStatus::RemoteOnly | SyncStatus::RemoteNewer => SyncChoice::Download,
        SyncStatus::Same => SyncChoice::Skip,
    }
}

/// Actions that make sense for a row (deleting needs the file on that side).
pub fn sync_choices(entry: &SyncEntry) -> Vec<SyncChoice> {
    let (local, remote) = (entry.local.is_some(), entry.remote.is_some());
    let mut out = vec![SyncChoice::Skip];
    out.extend(local.then_some(SyncChoice::Upload));
    out.extend(remote.then_some(SyncChoice::Download));
    out.extend(remote.then_some(SyncChoice::DeleteRemote));
    out.extend(local.then_some(SyncChoice::DeleteLocal));
    out
}

pub fn initial_sync_choices(entries: &[SyncEntry]) -> SyncChoices {
    let mut choices = SyncChoices::default();
    for entry in entries {
        choices.set(&entry.path, default_sync_action(entry.status));
    }
    choices
}

/// Steps to send to `deploy_sync`, skipped rows left out.
pub fn sync_steps(choices: &SyncChoices) -> Vec<SyncStep> {
    choices
        .iter()
        .filter_map(|(path, choice)| choice.action().map(|action| SyncStep { path: path.to_owned(), action }))
        .collect()
}

/// The server an action uses: `id`, else the default server, else the only one.
pub fn resolve_server<'s>(settings: &'s DeploymentSettings, id: Option<&str>) -> Option<&'s DeploymentServer> {
    let servers = &settings.servers;
    if let Some(id) = id.filter(|id| !id.is_empty()) {
        return servers.iter().find(|server| server.id == id);
    }
    servers.iter().find(|server| server.id == settings.default_server).or(match servers.as_slice() {
        [only] => Some(only),
        _ => None,
    })
}

/// The name a server is shown under: its trimmed name, else its id.
pub fn server_label(server: &DeploymentServer) -> &str {
    let name = server.name.trim();
    if name.is_empty() { &server.id } else { name }
}

/// The SSH configuration a server's transfers authenticate with ("" for a local server).
pub fn server_ssh(server: &DeploymentServer) -> &str {
    match server.kind {
        DeploymentType::Sftp => &server.ssh_config,
        DeploymentType::Local => "",
    }
}

/// A transfer entry of the Deployment menu: the action and, for the default server's entries,
/// its label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferMenuEntry {
    pub action: &'static str,
    pub label: Option<String>,
}

/// Transfer entries of the Deployment menu: named after the default server, choosers when there
/// are several servers or no default.
pub fn transfer_menu_entries(settings: &DeploymentSettings, i18n: &mut I18n) -> Vec<TransferMenuEntry> {
    let target = resolve_server(settings, None);
    let mut out = Vec::new();
    if let Some(target) = target {
        let name = server_label(target);
        let named = [
            ("deployment.uploadToDefault", ["Upload to ", ""]),
            ("deployment.downloadFromDefault", ["Download from ", ""]),
            ("deployment.syncWithDefault", ["Sync with Deployed to ", "…"]),
        ];
        for (action, strings) in named {
            out.push(TransferMenuEntry { action, label: Some(i18n.tx_built(&strings, &[Param::from(name)])) });
        }
    }
    if settings.servers.len() > 1 || target.is_none() {
        for action in ["deployment.uploadTo", "deployment.downloadFrom", "deployment.syncWith"] {
            out.push(TransferMenuEntry { action, label: None });
        }
    }
    out
}

/// What a finished transfer did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportVerb {
    Upload,
    Download,
    Delete,
    Sync,
}

impl ReportVerb {
    pub fn key(self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Download => "download",
            Self::Delete => "delete",
            Self::Sync => "sync",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportParams {
    pub n: u32,
    pub server: String,
    pub skipped: u32,
}

/// Notification for a finished transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportSummary {
    pub level: NotificationLevel,
    /// English source text with `{n}` / `{server}` placeholders.
    pub title: &'static str,
    pub params: ReportParams,
}

impl ReportSummary {
    /// The title, translated, with its placeholders filled in.
    pub fn text(&self, i18n: &I18n) -> String {
        i18n.tx(self.title).replace("{n}", &self.params.n.to_string()).replace("{server}", &self.params.server)
    }
}

pub fn summarize_report(report: &TransferReport, verb: ReportVerb, server: &str) -> ReportSummary {
    let failed = !report.errors.is_empty();
    let title = match (verb, failed) {
        (ReportVerb::Upload, true) => "Upload to {server} finished with errors",
        (ReportVerb::Upload, false) => "Uploaded {n} files to {server}",
        (ReportVerb::Download, true) => "Download from {server} finished with errors",
        (ReportVerb::Download, false) => "Downloaded {n} files from {server}",
        (ReportVerb::Delete, true) => "Delete from {server} finished with errors",
        (ReportVerb::Delete, false) => "Deleted {n} files from {server}",
        (ReportVerb::Sync, true) => "Sync with {server} finished with errors",
        (ReportVerb::Sync, false) => "Synchronized {n} files with {server}",
    };
    let level = match (failed, report.files) {
        (false, _) => NotificationLevel::Info,
        (true, 0) => NotificationLevel::Error,
        (true, _) => NotificationLevel::Warning,
    };
    ReportSummary {
        level,
        title,
        params: ReportParams { n: report.files, server: server.to_owned(), skipped: report.skipped },
    }
}

/// Joins an absolute remote folder and a name / relative path.
pub fn join_remote(dir: &str, rel: &str) -> String {
    let rel = rel.trim_matches('/');
    if rel.is_empty() {
        dir.to_owned()
    } else if dir.ends_with('/') {
        format!("{dir}{rel}")
    } else {
        format!("{dir}/{rel}")
    }
}

/// The last segment of a remote path (`/` for the root).
pub fn remote_name(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    let name = trimmed.rfind('/').map_or(trimmed, |slash| &trimmed[slash + 1..]);
    if name.is_empty() { "/" } else { name }
}

/// The local path of a remote one through the most specific backend-resolved mapping; `None`
/// when unmapped or when the path climbs out with `.` / `..`.
pub fn local_path_for_remote(remote_path: &str, mappings: &[ResolvedMapping]) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for mapping in mappings {
        let deployed = mapping.deployment.as_str();
        let Some(rest) = remote_path.strip_prefix(deployed) else { continue };
        let within = rest.is_empty() || deployed == "/" || rest.starts_with('/');
        if !within || best.as_ref().is_some_and(|(len, _)| *len >= deployed.len()) {
            continue;
        }
        let rest = rest.trim_start_matches('/');
        if rest.split('/').any(|part| part == "." || part == "..") {
            return None;
        }
        best = Some((deployed.len(), join_remote(&to_ipc(&mapping.local), rest)));
    }
    best.map(|(_, local)| local)
}

/// What the user can do about a failed SSH connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshRemedy {
    /// Trust an unknown host key (never a changed one).
    TrustHostKey,
    /// Type a missing or rejected password / passphrase.
    Secret,
}

static CHANGED_HOST_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^SSH host key .* has changed").expect("the pattern is valid"));
static SECRET_NEEDED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^SSH (password|key passphrase) required|^cannot decrypt SSH key").expect("the pattern is valid")
});

/// Classifies an `ide-ssh` failure by its kind and message.
pub fn ssh_remedy(kind: Option<ErrorKind>, message: &str, auth: SshAuth) -> Option<SshRemedy> {
    if message.starts_with("Unknown SSH host key") {
        return Some(SshRemedy::TrustHostKey);
    }
    if kind != Some(ErrorKind::PermissionDenied) || CHANGED_HOST_KEY.is_match(message) {
        return None;
    }
    if SECRET_NEEDED.is_match(message) {
        return Some(SshRemedy::Secret);
    }
    // With OpenSSH config + agent there is nothing to type unless a key is encrypted.
    if message.starts_with("SSH authentication failed") {
        return (auth != SshAuth::OpenSsh).then_some(SshRemedy::Secret);
    }
    None
}

/// What to do after remote work over SSH configuration `ssh` failed on attempt `attempt` (0-based):
/// give up, or offer the remedy and then run the work again. Up to three remedies are offered.
pub fn ssh_retry(attempt: u32, ssh: &str, error: &IdeError, configs: &[SshConfiguration]) -> Option<SshRemedy> {
    if attempt >= 3 || ssh.is_empty() {
        return None;
    }
    let auth = configs.iter().find(|config| config.id == ssh).map_or(SshAuth::OpenSsh, |config| config.auth);
    ssh_remedy(Some(error.kind), &error.message, auth)
}

/// `1.3 KB`: one decimal below 10, whole numbers above.
pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let units = ["KB", "MB", "GB", "TB"];
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    // Ties round up, as the size column has always shown them.
    if value < 10.0 {
        format!("{:.1} {}", (value * 10.0).round() / 10.0, units[unit])
    } else {
        format!("{} {}", value.round(), units[unit])
    }
}

/// `2026-10-27 14:03` from seconds since the epoch, shifted by the local UTC offset in force then
/// (`crate::platform::local_time::utc_offset_minutes`); "" without a time.
pub fn format_time(seconds: Option<u32>, utc_offset_minutes: i32) -> String {
    let Some(seconds) = seconds else { return String::new() };
    let wall = i64::from(seconds) + i64::from(utc_offset_minutes) * 60;
    DateTime::from_timestamp(wall, 0).map_or_else(String::new, |moment| {
        let moment = moment.naive_utc();
        format!(
            "{}-{:02}-{:02} {:02}:{:02}",
            moment.year(),
            moment.month(),
            moment.day(),
            moment.hour(),
            moment.minute()
        )
    })
}

/// Rows of a comparison by path: a later row of a path replaces an earlier one in its place.
pub fn unique_by_path(entries: Vec<SyncEntry>) -> Vec<SyncEntry> {
    let mut out: Vec<SyncEntry> = Vec::with_capacity(entries.len());
    let mut index: HashMap<String, usize> = HashMap::new();
    for entry in entries {
        match index.get(&entry.path) {
            Some(&at) => out[at] = entry,
            None => {
                index.insert(entry.path.clone(), out.len());
                out.push(entry);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::types::{FileStamp, TransferError};
    use super::*;
    use ide_core::IpcPath;
    use serde_json::json;
    use std::path::PathBuf;

    const ROOT: &str = "/work/app";

    fn p(rel: &str) -> String {
        format!("{ROOT}/{rel}")
    }

    fn ctx<'a>(selection: &'a [String], editor_path: Option<&'a str>) -> TargetContext<'a> {
        TargetContext { part: None, selection, editor_path, project_root: Some(ROOT) }
    }

    fn entry(path: &str, status: SyncStatus, local: bool, remote: bool) -> SyncEntry {
        SyncEntry {
            path: path.to_owned(),
            remote_path: format!("/srv/{path}"),
            local: local.then_some(FileStamp { size: 1, mtime: Some(1) }),
            remote: remote.then_some(FileStamp { size: 2, mtime: Some(2) }),
            status,
        }
    }

    fn server(id: &str, name: &str) -> DeploymentServer {
        DeploymentServer { id: id.to_owned(), name: name.to_owned(), ..DeploymentServer::default() }
    }

    fn settings(servers: &[DeploymentServer], default_server: &str) -> DeploymentSettings {
        DeploymentSettings {
            servers: servers.to_vec(),
            default_server: default_server.to_owned(),
            ..DeploymentSettings::default()
        }
    }

    fn mapping(local: &str, deployment: &str) -> ResolvedMapping {
        ResolvedMapping { local: IpcPath::from(PathBuf::from(local)), deployment: deployment.to_owned() }
    }

    fn report(files: u32, skipped: u32, errors: usize) -> TransferReport {
        let error = TransferError { path: "x".to_owned(), message: "denied".to_owned() };
        TransferReport { files, skipped, errors: vec![error; errors] }
    }

    // targetPaths › reads paths passed by menus
    #[test]
    fn reads_paths_passed_by_menus() {
        assert_eq!(paths_from_arg(&json!(p("a.php"))), Some(vec![p("a.php")]));
        assert_eq!(paths_from_arg(&json!([p("a"), 3, ""])), Some(vec![p("a")]));
        assert_eq!(paths_from_arg(&json!({ "paths": [p("b")] })), Some(vec![p("b")]));
        assert_eq!(paths_from_arg(&Value::Null), None);
        assert_eq!(paths_from_arg(&json!({ "paths": [] })), None);
    }

    // targetPaths › prefers the passed paths over the selection and the editor
    #[test]
    fn prefers_the_passed_paths_over_the_selection_and_the_editor() {
        let selection = [p("y")];
        let editor = p("z");
        let context = TargetContext { part: Some(FocusedPart::Project), ..ctx(&selection, Some(&editor)) };
        assert_eq!(target_paths(&json!(p("x")), &context), [p("x")]);
    }

    // targetPaths › uses the project view selection when it had focus last, else the editor file
    #[test]
    fn uses_the_project_view_selection_when_it_had_focus_last_else_the_editor_file() {
        let selection = [p("src")];
        let editor = p("index.php");
        let both = ctx(&selection, Some(&editor));
        let project = TargetContext { part: Some(FocusedPart::Project), ..both };
        assert_eq!(target_paths(&Value::Null, &project), [p("src")]);
        let in_editor = TargetContext { part: Some(FocusedPart::Editor), ..both };
        assert_eq!(target_paths(&Value::Null, &in_editor), [p("index.php")]);
        let no_editor = TargetContext { part: Some(FocusedPart::Editor), ..ctx(&selection, None) };
        assert!(target_paths(&Value::Null, &no_editor).is_empty());
    }

    // targetPaths › drops paths outside the project and paths inside selected folders
    #[test]
    fn drops_paths_outside_the_project_and_paths_inside_selected_folders() {
        let arg = json!([p("src"), p("src/a.php"), "/elsewhere/b", p("srcx")]);
        assert_eq!(target_paths(&arg, &ctx(&[], None)), [p("src"), p("srcx")]);
        let no_project = TargetContext { project_root: None, ..ctx(&[], None) };
        assert!(target_paths(&json!(p("a")), &no_project).is_empty());
    }

    // sync defaults › picks the action from the status
    #[test]
    fn picks_the_action_from_the_status() {
        let cases = [
            (SyncStatus::LocalOnly, SyncChoice::Upload),
            (SyncStatus::LocalNewer, SyncChoice::Upload),
            (SyncStatus::Different, SyncChoice::Upload),
            (SyncStatus::RemoteOnly, SyncChoice::Download),
            (SyncStatus::RemoteNewer, SyncChoice::Download),
            (SyncStatus::Same, SyncChoice::Skip),
        ];
        for (status, choice) in cases {
            assert_eq!(default_sync_action(status), choice, "{status:?}");
        }
    }

    // sync defaults › offers only actions possible for the row
    #[test]
    fn offers_only_actions_possible_for_the_row() {
        use SyncChoice::*;
        assert_eq!(sync_choices(&entry("a", SyncStatus::LocalOnly, true, false)), [Skip, Upload, DeleteLocal]);
        assert_eq!(sync_choices(&entry("b", SyncStatus::RemoteOnly, false, true)), [Skip, Download, DeleteRemote]);
    }

    // sync defaults › turns choices into steps without the skipped rows
    #[test]
    fn turns_choices_into_steps_without_the_skipped_rows() {
        let entries = [
            entry("a", SyncStatus::LocalOnly, true, false),
            entry("b", SyncStatus::Same, true, true),
            entry("c", SyncStatus::RemoteNewer, true, true),
        ];
        let mut choices = initial_sync_choices(&entries);
        let expected =
            SyncChoices::from_pairs(&[("a", SyncChoice::Upload), ("b", SyncChoice::Skip), ("c", SyncChoice::Download)]);
        assert_eq!(choices, expected);
        choices.set("b", SyncChoice::DeleteRemote);
        let step = |path: &str, action| SyncStep { path: path.to_owned(), action };
        assert_eq!(
            sync_steps(&choices),
            [step("a", SyncAction::Upload), step("b", SyncAction::DeleteRemote), step("c", SyncAction::Download)]
        );
    }

    // servers and reports › resolves the default server
    #[test]
    fn resolves_the_default_server() {
        let (a, b) = (server("a", "Staging"), server("b", ""));
        let id = |found: Option<&DeploymentServer>| found.map(|server| server.id.clone());
        let both = [a.clone(), b];
        assert_eq!(id(resolve_server(&settings(&both, "b"), None)), Some("b".to_owned()));
        assert_eq!(id(resolve_server(&settings(&both, ""), None)), None);
        assert_eq!(id(resolve_server(&settings(std::slice::from_ref(&a), ""), None)), Some("a".to_owned()));
        assert_eq!(id(resolve_server(&settings(&both, "b"), Some("a"))), Some("a".to_owned()));
    }

    // servers and reports › names the menu's transfers after the default server
    #[test]
    fn names_the_menus_transfers_after_the_default_server() {
        let (a, b) = (server("a", "Staging"), server("b", ""));
        let mut i18n = I18n::default();
        let mut actions = |servers: &[DeploymentServer], default_server: &str| -> Vec<String> {
            transfer_menu_entries(&settings(servers, default_server), &mut i18n)
                .into_iter()
                .map(|entry| entry.label.unwrap_or_else(|| entry.action.to_owned()))
                .collect()
        };
        let named = ["Upload to Staging", "Download from Staging", "Sync with Deployed to Staging…"];
        let choosers = ["deployment.uploadTo", "deployment.downloadFrom", "deployment.syncWith"];
        assert_eq!(actions(std::slice::from_ref(&a), ""), named);
        assert_eq!(actions(&[a.clone(), b.clone()], "a"), [named, choosers].concat());
        assert_eq!(actions(&[a, b], ""), choosers);
    }

    // servers and reports › summarizes transfer reports
    #[test]
    fn summarizes_transfer_reports() {
        assert_eq!(
            summarize_report(&report(3, 1, 0), ReportVerb::Upload, "Staging"),
            ReportSummary {
                level: NotificationLevel::Info,
                title: "Uploaded {n} files to {server}",
                params: ReportParams { n: 3, server: "Staging".to_owned(), skipped: 1 },
            }
        );
        assert_eq!(summarize_report(&report(2, 0, 1), ReportVerb::Download, "S").level, NotificationLevel::Warning);
        assert_eq!(summarize_report(&report(0, 0, 1), ReportVerb::Sync, "S").level, NotificationLevel::Error);
        let i18n = I18n::default();
        let uploaded = summarize_report(&report(3, 1, 0), ReportVerb::Upload, "Staging");
        assert_eq!(uploaded.text(&i18n), "Uploaded 3 files to Staging");
    }

    // servers and reports › classifies SSH failures
    #[test]
    fn classifies_ssh_failures() {
        use ErrorKind::{Io, PermissionDenied, Protocol};
        let denied = Some(PermissionDenied);
        let trust = Some(SshRemedy::TrustHostKey);
        let secret = Some(SshRemedy::Secret);
        assert_eq!(ssh_remedy(Some(Protocol), "Unknown SSH host key for example.com", SshAuth::OpenSsh), trust);
        assert_eq!(ssh_remedy(denied, "SSH password required for deploy@example.com", SshAuth::Password), secret);
        assert_eq!(ssh_remedy(denied, "SSH key passphrase required for ~/.ssh/id", SshAuth::KeyPair), secret);
        assert_eq!(ssh_remedy(denied, "SSH host key of box:22 has changed", SshAuth::Password), None);
        assert_eq!(ssh_remedy(denied, "SSH authentication failed", SshAuth::OpenSsh), None);
        assert_eq!(ssh_remedy(denied, "SSH authentication failed", SshAuth::Password), secret);
        assert_eq!(ssh_remedy(Some(Io), "Connection refused", SshAuth::Password), None);
    }

    // remote paths › joins and splits remote paths
    #[test]
    fn joins_and_splits_remote_paths() {
        assert_eq!(join_remote("/var/www", "app/"), "/var/www/app");
        assert_eq!(join_remote("/", "etc"), "/etc");
        assert_eq!(join_remote("/var/www", ""), "/var/www");
        assert_eq!(remote_parent("/var/www/app"), "/var/www");
        assert_eq!(remote_parent("/var"), "/");
        assert_eq!(remote_parent("/"), "/");
        assert_eq!(remote_name("/var/www/app/"), "app");
    }

    // remote paths › maps remote paths back to project paths through the most specific mapping
    #[test]
    fn maps_remote_paths_back_to_project_paths_through_the_most_specific_mapping() {
        let mappings = [
            mapping(ROOT, "/var/www"),
            mapping(&p("public"), "/var/www/htdocs"),
            mapping(&p("shared"), "/var/www/opt/shared"),
        ];
        let map = |path: &str| local_path_for_remote(path, &mappings);
        assert_eq!(map("/var/www/index.php"), Some(p("index.php")));
        assert_eq!(map("/var/www/htdocs/css/a.css"), Some(p("public/css/a.css")));
        assert_eq!(map("/var/www/htdocs"), Some(p("public")));
        assert_eq!(map("/var/www/opt/shared/lib"), Some(p("shared/lib")));
        assert_eq!(map("/opt/shared/lib"), None);
        assert_eq!(map("/var/wwwx/a"), None);
        assert_eq!(map("/etc/passwd"), None);
    }

    // remote paths › uses the backend folder for aliases and does not invent mappings
    #[test]
    fn uses_the_backend_folder_for_aliases_and_does_not_invent_mappings() {
        let mappings = [mapping(&p("src"), "/mnt/deploy")];
        assert_eq!(local_path_for_remote("/mnt/deploy/a.txt", &mappings), Some(p("src/a.txt")));
        assert_eq!(local_path_for_remote("/mnt/deploy/a.txt", &[]), None);
        assert_eq!(local_path_for_remote("/mnt/deploy/../a.txt", &mappings), None);
        assert_eq!(local_path_for_remote("/a.txt", &[mapping(ROOT, "/")]), Some(p("a.txt")));
    }

    // Not from a spec: sizes round ties up, times show the local wall clock, rows of a path keep
    // their first place, and only SSH work gets a remedy.
    #[test]
    fn formats_sizes_times_and_merges_rows() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1280), "1.3 KB");
        assert_eq!(format_size(20 * 1024 * 1024), "20 MB");
        assert_eq!(format_time(Some(0), 90), "1970-01-01 01:30");
        assert_eq!(format_time(None, 0), "");
        let rows = vec![
            entry("a", SyncStatus::Same, true, true),
            entry("b", SyncStatus::Same, true, true),
            entry("a", SyncStatus::LocalOnly, true, false),
        ];
        let merged = unique_by_path(rows);
        assert_eq!(merged.iter().map(|row| row.path.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(merged[0].status, SyncStatus::LocalOnly);
        let error = IdeError::new(ErrorKind::PermissionDenied, "SSH authentication failed");
        assert_eq!(ssh_retry(0, "ssh", &error, &[]), None);
        let config = SshConfiguration { id: "ssh".to_owned(), auth: SshAuth::Password, ..Default::default() };
        assert_eq!(ssh_retry(0, "ssh", &error, std::slice::from_ref(&config)), Some(SshRemedy::Secret));
        assert_eq!(ssh_retry(3, "ssh", &error, &[config]), None);
        assert_eq!(ssh_retry(0, "", &error, &[]), None);
    }
}
