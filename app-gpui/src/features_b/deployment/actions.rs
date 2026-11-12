//! What the Deployment actions decide: which server a transfer goes to (the default one, or one
//! chosen among the servers whose mappings cover the selection), why nothing can be deployed, the
//! notification of a finished transfer, and where Sync with Deployed opens.
//!
//! A transfer runs in this order: [`no_targets_notice`] without targets; the server from
//! [`resolve_server`](super::model::resolve_server) or [`pick_server`] (after `deploy_coverage`);
//! [`server_coverage`] for that server; then `deploy_upload` / `deploy_download` with a fresh
//! request id, and [`report_notice`] or [`failure_notice`] with the answer.

use super::model::{ReportVerb, server_label, summarize_report};
use super::types::{DeploymentCoverage, TransferError, TransferReport};
use crate::platform::i18n::{I18n, Param};
use crate::platform::virtual_docs::relative_path;
use crate::workbench::notifications::{NotificationAction, NotificationLevel};
use ide_core::IdeError;
use ide_core::error::ErrorKind;
use ide_core::settings::{AutoUpload, DeploymentServer, DeploymentSettings};
use std::borrow::Cow;

/// The notification group of every deployment notification.
pub const GROUP: &str = "tools";
/// The action that opens Settings › Build, Execution, Deployment › Deployment.
pub const CONFIGURE_ACTION: &str = "deployment.configure";
/// Error rows a report body lists before "and N more".
const REPORT_ROWS: usize = 10;

#[derive(Debug, Clone, PartialEq)]
pub enum NoticeBody {
    Text(String),
    /// Skipped files and failures, shown with [`report_lines`].
    Report(TransferReport),
}

/// A notification to raise in [`GROUP`].
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub level: NotificationLevel,
    pub title: String,
    pub body: Option<NoticeBody>,
    /// A newer notification of the same key replaces an older one.
    pub key: Option<String>,
    pub actions: Vec<NotificationAction>,
}

impl Notice {
    fn new(level: NotificationLevel, title: String) -> Self {
        Self { level, title, body: None, key: None, actions: Vec::new() }
    }

    fn keyed(mut self, key: &str) -> Self {
        self.key = Some(key.to_owned());
        self
    }

    fn with_body(mut self, body: Option<NoticeBody>) -> Self {
        self.body = body;
        self
    }

    fn configurable(mut self, i18n: &I18n) -> Self {
        self.actions.push(NotificationAction { label: i18n.tx("Configure…"), command: CONFIGURE_ACTION.to_owned() });
        self
    }
}

pub fn is_cancelled(error: &IdeError) -> bool {
    error.kind == ErrorKind::Cancelled
}

pub fn no_servers_notice(i18n: &I18n) -> Notice {
    Notice::new(NotificationLevel::Warning, i18n.tx("No deployment server configured"))
        .keyed("deployment.noServer")
        .configurable(i18n)
}

pub fn no_targets_notice(i18n: &I18n) -> Notice {
    let title = i18n.tx("Select files in the Project view or open a file in the editor");
    Notice::new(NotificationLevel::Info, title).keyed("deployment.noTarget")
}

/// `deploy_coverage` failed.
pub fn coverage_failed_notice(error: &IdeError, i18n: &I18n) -> Notice {
    Notice::new(NotificationLevel::Error, i18n.tx("Cannot check deployment mappings"))
        .with_body(Some(NoticeBody::Text(i18n.tx_template(&error.message))))
}

/// Nothing selected can be deployed: to `server`, or (without one) to any server; `skipped` says
/// why for each path.
pub fn no_coverage_notice(server: Option<&DeploymentServer>, skipped: &[TransferError], i18n: &mut I18n) -> Notice {
    let title = match server {
        Some(server) => {
            i18n.tx_built(&["No selected paths can be deployed to ", ""], &[Param::from(server_label(server))])
        }
        None => i18n.tx("No deployment server covers the selected paths"),
    };
    let body = (!skipped.is_empty()).then(|| {
        NoticeBody::Report(TransferReport { files: 0, skipped: skipped.len() as u32, errors: skipped.to_vec() })
    });
    Notice::new(NotificationLevel::Warning, title).with_body(body).keyed("deployment.noCoverage").configurable(i18n)
}

/// The answer to "which server" for an explicit chooser.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerPick {
    /// Ask with `labels` (even for one server); `servers` are the matching ids.
    Choose {
        labels: Vec<String>,
        servers: Vec<String>,
    },
    Notify(Notice),
}

/// The servers whose mappings cover some of the selection, to choose from; with none, why. With
/// no server configured at all there is no need to ask for the coverage (pass an empty one).
pub fn pick_server(configured: &[DeploymentServer], coverage: &[DeploymentCoverage], i18n: &mut I18n) -> ServerPick {
    if configured.is_empty() {
        return ServerPick::Notify(no_servers_notice(i18n));
    }
    let servers: Vec<&DeploymentServer> = configured
        .iter()
        .filter(|server| coverage_of(coverage, server).is_some_and(|covered| !covered.paths.is_empty()))
        .collect();
    if !servers.is_empty() {
        return ServerPick::Choose {
            labels: servers.iter().map(|server| server_label(server).to_owned()).collect(),
            servers: servers.iter().map(|server| server.id.clone()).collect(),
        };
    }
    if let [only] = configured {
        // One server: say why, as an explicit transfer to it would.
        let skipped = coverage_of(coverage, only).map_or(&[][..], |covered| covered.skipped.as_slice());
        return ServerPick::Notify(no_coverage_notice(Some(only), skipped, i18n));
    }
    let skipped: Vec<TransferError> = configured
        .iter()
        .flat_map(|server| {
            let label = server_label(server);
            coverage_of(coverage, server).into_iter().flat_map(move |covered| {
                covered.skipped.iter().map(move |error| TransferError {
                    path: error.path.clone(),
                    message: format!("{label}: {}", error.message),
                })
            })
        })
        .collect();
    ServerPick::Notify(no_coverage_notice(None, &skipped, i18n))
}

fn coverage_of<'c>(coverage: &'c [DeploymentCoverage], server: &DeploymentServer) -> Option<&'c DeploymentCoverage> {
    coverage.iter().find(|covered| covered.server == server.id)
}

/// The id of the server chosen by its label in a [`ServerPick::Choose`].
pub fn chosen_server<'p>(labels: &[String], servers: &'p [String], picked: &str) -> Option<&'p str> {
    let index = labels.iter().position(|label| label == picked)?;
    servers.get(index).map(String::as_str)
}

/// Whether a transfer to `server` can go ahead.
#[derive(Debug, Clone, PartialEq)]
pub enum Eligibility<'c> {
    Covered(&'c DeploymentCoverage),
    /// The coverage does not know the server: nothing to do, nothing to say.
    Unknown,
    Rejected(Notice),
}

pub fn server_coverage<'c>(
    coverage: &'c [DeploymentCoverage],
    server: &DeploymentServer,
    i18n: &mut I18n,
) -> Eligibility<'c> {
    match coverage.iter().find(|covered| covered.server == server.id) {
        None => Eligibility::Unknown,
        Some(covered) if covered.paths.is_empty() => {
            Eligibility::Rejected(no_coverage_notice(Some(server), &covered.skipped, i18n))
        }
        Some(covered) => Eligibility::Covered(covered),
    }
}

/// The notification of a finished transfer; its body lists skipped files and failures.
pub fn report_notice(report: &TransferReport, verb: ReportVerb, server: &DeploymentServer, i18n: &I18n) -> Notice {
    let summary = summarize_report(report, verb, server_label(server));
    let body = (!report.errors.is_empty() || report.skipped > 0).then(|| NoticeBody::Report(report.clone()));
    Notice::new(summary.level, summary.text(i18n)).with_body(body).keyed(&format!("deployment.{}", verb.key()))
}

/// The lines of a report body: the skipped count, the first failures (project-relative) and how
/// many more there are.
pub fn report_lines(report: &TransferReport, project_root: Option<&str>, i18n: &mut I18n) -> Vec<String> {
    let mut lines = Vec::new();
    if report.skipped > 0 {
        lines.push(i18n.tx_built(&["Skipped: ", ""], &[Param::from(f64::from(report.skipped))]));
    }
    for error in report.errors.iter().take(REPORT_ROWS) {
        let path = project_root.map_or(Cow::Borrowed(error.path.as_str()), |root| relative_path(root, &error.path));
        lines.push(format!("{path}: {}", i18n.tx_template(&error.message)));
    }
    if report.errors.len() > REPORT_ROWS {
        let more = report.errors.len() - REPORT_ROWS;
        lines.push(i18n.tx_built(&["and ", " more"], &[Param::from(more)]));
    }
    lines
}

/// A failed transfer or window: `title` has a `{server}` placeholder. `None` for a cancelled
/// request, which needs no notification.
pub fn failure_notice(title: &str, server: &DeploymentServer, error: &IdeError, i18n: &I18n) -> Option<Notice> {
    if is_cancelled(error) {
        return None;
    }
    let title = i18n.tx(title).replace("{server}", server_label(server));
    let body = Some(NoticeBody::Text(i18n.tx_template(&error.message)));
    Some(Notice::new(NotificationLevel::Error, title).with_body(body).keyed("deployment.failure"))
}

/// An upload or a download of the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferDirection {
    Upload,
    Download,
}

impl TransferDirection {
    pub fn verb(self) -> ReportVerb {
        match self {
            Self::Upload => ReportVerb::Upload,
            Self::Download => ReportVerb::Download,
        }
    }

    /// The title of [`failure_notice`] for this direction.
    pub fn failure_title(self) -> &'static str {
        match self {
            Self::Upload => "Cannot upload to {server}",
            Self::Download => "Cannot download from {server}",
        }
    }
}

/// The question Delete from Server asks before deleting the remote copies of `paths`.
pub fn delete_confirm_message(
    paths: &[String],
    project_root: Option<&str>,
    server: &DeploymentServer,
    i18n: &mut I18n,
) -> String {
    let label = Param::from(server_label(server));
    match paths {
        [path] => {
            let name = match project_root {
                Some(root) => {
                    let relative = relative_path(root, path);
                    if relative.is_empty() { "/".to_owned() } else { relative.into_owned() }
                }
                None => path.clone(),
            };
            i18n.tx_built(&["Delete the remote copy of ", " from ", "?"], &[Param::from(name), label])
        }
        _ => i18n.tx_built(&["Delete the remote copies of ", " items from ", "?"], &[Param::from(paths.len()), label]),
    }
}

/// Where Sync with Deployed opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComparisonOpen {
    /// Its own window on desktop (`window_open_deployment(server, paths, background)`); a failure
    /// is reported with [`failure_notice`] and "Cannot open comparison for {server}".
    Window { server: String, paths: Vec<String> },
    /// The dialog in this window: [`DeploymentStore::open_sync`](super::store::DeploymentStore::open_sync).
    InPage,
}

pub fn open_comparison(server: &DeploymentServer, paths: &[String], desktop: bool) -> ComparisonOpen {
    if desktop {
        ComparisonOpen::Window { server: server.id.clone(), paths: paths.to_vec() }
    } else {
        ComparisonOpen::InPage
    }
}

/// What Open in Browser (Deployed) does with the answer of `deploy_info`.
#[derive(Debug, Clone, PartialEq)]
pub enum BrowserOpen {
    Url(String),
    Notify(Notice),
}

/// `mapped` is whether `deploy_info` found the file on the default server, `web_url` its URL.
pub fn open_in_browser(mapped: bool, web_url: Option<&str>, i18n: &I18n) -> BrowserOpen {
    match web_url.filter(|url| !url.is_empty()) {
        Some(url) => BrowserOpen::Url(url.to_owned()),
        None => {
            let title = if mapped {
                "No web server URL for this file"
            } else {
                "This file is not mapped to the default server"
            };
            let notice = Notice::new(NotificationLevel::Info, i18n.tx(title)).keyed("deployment.openInBrowser");
            BrowserOpen::Notify(notice.configurable(i18n))
        }
    }
}

pub fn auto_upload_on(settings: &DeploymentSettings) -> bool {
    settings.auto_upload != AutoUpload::Never
}

/// The `deployment.autoUpload` value the Automatic Upload toggle stores.
pub fn toggled_auto_upload(settings: &DeploymentSettings) -> AutoUpload {
    if auto_upload_on(settings) { AutoUpload::Never } else { AutoUpload::Always }
}

#[cfg(test)]
mod tests {
    use super::super::model::resolve_server;
    use super::*;

    fn server(id: &str) -> DeploymentServer {
        DeploymentServer { id: id.to_owned(), name: id.to_owned(), ..DeploymentServer::default() }
    }

    fn skipped_b() -> TransferError {
        TransferError { path: "/project/b".to_owned(), message: "Not covered by this server's mappings".to_owned() }
    }

    fn coverage() -> Vec<DeploymentCoverage> {
        vec![
            DeploymentCoverage {
                server: "one".to_owned(),
                paths: vec!["/project/a".to_owned()],
                skipped: vec![skipped_b()],
            },
            DeploymentCoverage { server: "two".to_owned(), paths: Vec::new(), skipped: Vec::new() },
        ]
    }

    fn settings() -> DeploymentSettings {
        DeploymentSettings {
            servers: vec![server("one"), server("two")],
            default_server: "one".to_owned(),
            ..DeploymentSettings::default()
        }
    }

    fn paths() -> Vec<String> {
        vec!["/project/a".to_owned(), "/project/b".to_owned()]
    }

    fn report_errors(notice: &Notice) -> &[TransferError] {
        match &notice.body {
            Some(NoticeBody::Report(report)) => &report.errors,
            other => panic!("no report body: {other:?}"),
        }
    }

    fn notified(pick: ServerPick) -> Notice {
        match pick {
            ServerPick::Notify(notice) => notice,
            other => panic!("not a notification: {other:?}"),
        }
    }

    // deployment eligibility › keeps upstream's server-specific explanation when only one server
    // is configured
    #[test]
    fn keeps_upstreams_server_specific_explanation_when_only_one_server_is_configured() {
        let mut i18n = I18n::default();
        let mut covered = coverage();
        covered[0].paths.clear();
        let notice = notified(pick_server(&[server("one")], &covered, &mut i18n));
        assert_eq!(notice.title, "No selected paths can be deployed to one");
        assert_eq!(report_errors(&notice), covered[0].skipped);
    }

    // deployment eligibility › opens the selected server and paths in a native window on desktop
    // without an in-page comparison
    #[test]
    fn opens_the_selected_server_and_paths_in_a_native_window_on_desktop_without_an_in_page_comparison() {
        let mut i18n = I18n::default();
        let settings = settings();
        let ServerPick::Choose { labels, servers } = pick_server(&settings.servers, &coverage(), &mut i18n) else {
            panic!("a chooser");
        };
        let id = chosen_server(&labels, &servers, "one").expect("one is offered");
        let chosen = settings.servers.iter().find(|server| server.id == id).expect("a configured server");
        let expected = ComparisonOpen::Window { server: "one".to_owned(), paths: paths() };
        assert_eq!(open_comparison(chosen, &paths(), true), expected);
        assert_eq!(open_comparison(chosen, &paths(), false), ComparisonOpen::InPage);
    }

    // deployment eligibility › opens default-server synchronization in a native window on desktop too
    #[test]
    fn opens_default_server_synchronization_in_a_native_window_on_desktop_too() {
        let settings = settings();
        // The default server needs no choice, and on desktop nothing opens in the page.
        let default = resolve_server(&settings, None).expect("the default server");
        let expected = ComparisonOpen::Window { server: "one".to_owned(), paths: paths() };
        assert_eq!(open_comparison(default, &paths(), true), expected);
    }

    // deployment eligibility › reports a comparison window that cannot open (%s)
    #[test]
    fn reports_a_comparison_window_that_cannot_open() {
        let i18n = I18n::default();
        let settings = settings();
        let error = IdeError::new(ErrorKind::Internal, "window failed");
        // Sync with Deployed to… chooses "one"; the default-server action resolves it.
        for target in [&settings.servers[0], resolve_server(&settings, None).expect("the default server")] {
            let notice = failure_notice("Cannot open comparison for {server}", target, &error, &i18n)
                .expect("a failure is reported");
            assert!(notice.title.contains("Cannot open comparison"));
            assert_eq!(notice.body, Some(NoticeBody::Text("window failed".to_owned())));
        }
        let cancelled = IdeError::new(ErrorKind::Cancelled, "cancelled");
        let title = "Cannot open comparison for {server}";
        assert_eq!(failure_notice(title, &settings.servers[0], &cancelled, &i18n), None);
    }

    // deployment eligibility › filters %s and keeps a single option visible
    #[test]
    fn filters_and_keeps_a_single_option_visible() {
        let mut i18n = I18n::default();
        // Upload to…, Download from… and Sync with Deployed to… all choose this way.
        let pick = pick_server(&settings().servers, &coverage(), &mut i18n);
        assert_eq!(pick, ServerPick::Choose { labels: vec!["one".to_owned()], servers: vec!["one".to_owned()] });
    }

    // deployment eligibility › explains the rejected paths for %s and links settings
    #[test]
    fn explains_the_rejected_paths_and_links_settings() {
        let mut i18n = I18n::default();
        let mut covered = coverage();
        covered[0].paths.clear();
        let notice = notified(pick_server(&settings().servers, &covered, &mut i18n));
        assert_eq!(notice.title, "No deployment server covers the selected paths");
        let actions: Vec<&str> = notice.actions.iter().map(|action| action.label.as_str()).collect();
        assert_eq!(actions, ["Configure…"]);
        let expected = TransferError {
            path: "/project/b".to_owned(),
            message: "one: Not covered by this server's mappings".to_owned(),
        };
        assert_eq!(report_errors(&notice), [expected]);
    }

    // deployment eligibility › explains why the default cannot deploy the named file
    #[test]
    fn explains_why_the_default_cannot_deploy_the_named_file() {
        let mut i18n = I18n::default();
        let settings = settings();
        let mut covered = coverage();
        covered[0].paths.clear();
        let default = resolve_server(&settings, None).expect("the default server");
        let Eligibility::Rejected(notice) = server_coverage(&covered, default, &mut i18n) else {
            panic!("nothing is uploaded");
        };
        assert_eq!(report_errors(&notice)[0].path, "/project/b");
    }

    // Not from a spec: report notifications, their body lines, deletes and the browser target.
    #[test]
    fn reports_transfers_and_opens_deployed_urls() {
        let mut i18n = I18n::default();
        let one = server("one");
        let report = TransferReport { files: 1, skipped: 1, errors: vec![skipped_b()] };
        let notice = report_notice(&report, ReportVerb::Upload, &one, &i18n);
        assert_eq!(notice.level, NotificationLevel::Warning);
        assert_eq!(notice.key.as_deref(), Some("deployment.upload"));
        let lines = report_lines(&report, Some("/project"), &mut i18n);
        assert_eq!(lines, ["Skipped: 1", "b: Not covered by this server's mappings"]);
        let question = delete_confirm_message(&["/project".to_owned()], Some("/project"), &one, &mut i18n);
        assert_eq!(question, "Delete the remote copy of /project from one?");
        assert_eq!(open_in_browser(true, Some("http://x/a"), &i18n), BrowserOpen::Url("http://x/a".to_owned()));
        assert!(matches!(open_in_browser(false, None, &i18n), BrowserOpen::Notify(_)));
        assert_eq!(toggled_auto_upload(&settings()), AutoUpload::Always);
    }
}
