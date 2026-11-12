//! Check for Updates and Install and Restart. Both run as background tasks of the status bar: the
//! check while its request is out, the install with download progress and Cancel. Their balloons
//! show the progress too, since the Welcome screen has no status bar. Every entry point goes
//! through one [`Updater`], so a repeated click joins the running check or install.
//!
//! The backend owns the schedule of automatic checks and which version was announced already;
//! a window only says when it is ready for their notifications ([`UpdateChecks`]).

use crate::platform::i18n::I18n;
use crate::workbench::notifications::{NotificationAction, NotificationLevel, NotificationRequest};
use crate::workbench::tasks::{Cancel, Change, TaskId, TaskUpdate};
use ide_core::IdeError;
use ide_core::error::ErrorKind;
use ide_core::settings::Settings;
use ide_core::updates::{UpdateCheck, UpdateInfo, UpdateSource};

/// The key every update notification replaces the previous one under.
pub const UPDATE_KEY: &str = "app.update";
pub const CHECK_TASK_ID: &str = "app.update.check";
pub const INSTALL_TASK_ID: &str = "app.update.install";
/// The command of the Cancel action of the install balloon.
pub const CANCEL_COMMAND: &str = "app.update.cancel";
/// The command of Restart.
pub const RESTART_COMMAND: &str = "app.restart";

/// The command of an action that installs `version` (from `source`, or the configured one).
pub fn install_command(version: &str, source: Option<UpdateSource>) -> String {
    match source {
        None => format!("app.update.install:{version}"),
        Some(source) => format!("app.update.install:{version}:{}", source_id(source)),
    }
}

/// The version and source an [`install_command`] names.
pub fn parse_install_command(command: &str) -> Option<(String, Option<UpdateSource>)> {
    let rest = command.strip_prefix("app.update.install:")?;
    let (version, source) = match rest.split_once(':') {
        Some((version, "github")) => (version, Some(UpdateSource::Github)),
        Some((version, "cnb")) => (version, Some(UpdateSource::Cnb)),
        Some(_) => return None,
        None => (rest, None),
    };
    Some((version.to_owned(), source))
}

fn source_id(source: UpdateSource) -> &'static str {
    match source {
        UpdateSource::Github => "github",
        UpdateSource::Cnb => "cnb",
    }
}

fn source_name(source: UpdateSource) -> &'static str {
    match source {
        UpdateSource::Github => "GitHub",
        UpdateSource::Cnb => "CNB",
    }
}

fn other(source: UpdateSource) -> UpdateSource {
    match source {
        UpdateSource::Cnb => UpdateSource::Github,
        UpdateSource::Github => UpdateSource::Cnb,
    }
}

/// System Settings › Updates › Update source; `auto` picks the CNB mirror for the Chinese UI
/// (the system's language when the UI follows it).
pub fn update_source(settings: &Settings, system_locale: &str) -> UpdateSource {
    UpdateSource::resolve(&settings.general.update_source, &settings.appearance.locale, system_locale)
}

/// Download progress as the backend streams it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UpdateProgress {
    Downloading { downloaded: f64, total: Option<f64> },
    Installing,
}

/// A number of bytes in megabytes with one decimal, ties rounded up.
fn megabytes(bytes: f64) -> String {
    let tenths = (bytes / (1024.0 * 1024.0) * 10.0).round();
    format!("{:.1}", tenths / 10.0)
}

/// The install task's message and fraction (`None`: indeterminate) for `progress`.
pub fn progress_text(progress: UpdateProgress, i18n: &I18n) -> (String, Option<f64>) {
    match progress {
        UpdateProgress::Installing => (i18n.t("app.update.installingMessage", &[]), None),
        UpdateProgress::Downloading { downloaded, total: Some(total) } if total > 0.0 => {
            let message = i18n.t(
                "app.update.progress",
                &[("done", megabytes(downloaded).into()), ("total", megabytes(total).into())],
            );
            (message, Some((downloaded / total).min(1.0)))
        }
        UpdateProgress::Downloading { downloaded, .. } => {
            (i18n.t("app.update.progressUnknown", &[("done", megabytes(downloaded).into())]), None)
        }
    }
}

/// What the caller carries out, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateEffect {
    Task(TaskUpdate),
    Notify(NotificationRequest),
    /// `app_check_update`; report [`Updater::check_answered`] with the ticket.
    Check {
        source: UpdateSource,
        ticket: u64,
    },
    /// `app_update_announcement` (`automatic`: from the schedule, not a manual check); report
    /// [`Updater::announced`].
    Announce {
        update: UpdateInfoShown,
        automatic: bool,
    },
    /// `app_install_update` with the request id and a progress channel; report
    /// [`Updater::progress`] and [`Updater::install_finished`].
    Install {
        source: UpdateSource,
        request: u64,
    },
    /// `cancel_request` for the install.
    CancelRequest(u64),
    /// Store the update source globally; the install it precedes uses it whatever the outcome.
    WriteSource(UpdateSource),
    /// `app_restart`: a window may refuse (unsaved changes) and the update applies next start.
    Restart,
}

/// The part of an [`UpdateInfo`] the notifications show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfoShown {
    pub version: String,
    pub notes: Option<String>,
}

impl From<&UpdateInfo> for UpdateInfoShown {
    fn from(update: &UpdateInfo) -> Self {
        Self { version: update.version.clone(), notes: update.notes.clone() }
    }
}

fn keyed(level: NotificationLevel, title: String, user_initiated: bool) -> NotificationRequest {
    let mut request = NotificationRequest::new(level, title);
    request.key = Some(UPDATE_KEY.to_owned());
    request.user_initiated = user_initiated;
    request
}

fn action(label: String, command: impl Into<String>) -> NotificationAction {
    NotificationAction { label, command: command.into() }
}

fn task(id: &str, title: &str, done: bool) -> TaskUpdate {
    let mut update = TaskUpdate::new(TaskId::from(id), title);
    if done {
        update.fraction = Some(1.0);
        update.done = true;
    }
    update
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Installing {
    version: String,
    source: UpdateSource,
    request: u64,
    title: String,
    installing_shown: bool,
}

/// The update check and install of the app.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Updater {
    /// The running check's ticket and its task title.
    checking: Option<(u64, String)>,
    installing: Option<Installing>,
}

impl Updater {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_checking(&self) -> bool {
        self.checking.is_some()
    }

    pub fn is_installing(&self) -> bool {
        self.installing.is_some()
    }

    /// Check for Updates. A check already running is joined (nothing to do). `ticket` is fresh;
    /// `status_bar`: the window shows a project or LightEdit session, so the status bar shows the
    /// check and no balloon is needed.
    pub fn check(&mut self, source: UpdateSource, ticket: u64, status_bar: bool, i18n: &I18n) -> Vec<UpdateEffect> {
        if self.checking.is_some() {
            return Vec::new();
        }
        let title = i18n.t("app.update.checking", &[]);
        let mut running = task(CHECK_TASK_ID, &title, false);
        running.cancel = Change::Set(Cancel::Producer);
        let mut effects = vec![UpdateEffect::Task(running)];
        if self.installing.is_none() && !status_bar {
            effects.push(UpdateEffect::Notify(keyed(NotificationLevel::Info, title.clone(), true)));
        }
        effects.push(UpdateEffect::Check { source, ticket });
        self.checking = Some((ticket, title));
        effects
    }

    fn end_check(&mut self) -> Vec<UpdateEffect> {
        match self.checking.take() {
            Some((_, title)) => vec![UpdateEffect::Task(task(CHECK_TASK_ID, &title, true))],
            None => Vec::new(),
        }
    }

    /// The check's task was cancelled: its answer is dropped (the request ends on its own).
    pub fn cancel_check(&mut self) -> Vec<UpdateEffect> {
        self.end_check()
    }

    /// The backend answered the check `ticket`; `version` is the running one. An answer for a
    /// check that was cancelled or replaced is dropped.
    pub fn check_answered(
        &mut self,
        ticket: u64,
        answer: Result<UpdateCheck, IdeError>,
        version: &str,
        i18n: &I18n,
    ) -> Vec<UpdateEffect> {
        if self.checking.as_ref().map(|(current, _)| *current) != Some(ticket) {
            return Vec::new();
        }
        let mut effects = match answer {
            Ok(UpdateCheck { enabled: false, .. }) => {
                let mut request = keyed(NotificationLevel::Info, i18n.t("app.update.disabled", &[]), true);
                request.body = Some(i18n.t("app.update.disabledBody", &[]));
                vec![UpdateEffect::Notify(request)]
            }
            Ok(UpdateCheck { update: None, .. }) => {
                let title = i18n.t("app.update.none", &[("version", version.into())]);
                vec![UpdateEffect::Notify(keyed(NotificationLevel::Info, title, true))]
            }
            Ok(UpdateCheck { update: Some(update), .. }) if self.installing.is_none() => {
                vec![UpdateEffect::Announce { update: UpdateInfoShown::from(&update), automatic: false }]
            }
            Ok(_) => Vec::new(),
            Err(error) => {
                let mut request = keyed(NotificationLevel::Error, i18n.t("app.update.failed", &[]), true);
                request.body = Some(error.message);
                vec![UpdateEffect::Notify(request)]
            }
        };
        effects.extend(self.end_check());
        effects
    }

    /// The schedule found `update`: announced unless an install runs.
    pub fn update_available(&self, update: &UpdateInfo) -> Vec<UpdateEffect> {
        if self.installing.is_some() {
            return Vec::new();
        }
        vec![UpdateEffect::Announce { update: UpdateInfoShown::from(update), automatic: true }]
    }

    /// The backend said whether this window announces `update` (`claimed`); a version announced
    /// already is not announced again automatically, a manual check always answers. When the
    /// backend cannot say, only a manual check announces.
    pub fn announced(
        &self,
        update: &UpdateInfoShown,
        automatic: bool,
        claimed: Result<bool, IdeError>,
        i18n: &I18n,
    ) -> Vec<UpdateEffect> {
        if !claimed.unwrap_or(!automatic) {
            return Vec::new();
        }
        let title = i18n.t("app.update.available", &[("version", update.version.as_str().into())]);
        let mut request = keyed(NotificationLevel::Info, title, !automatic);
        request.body = update.notes.clone();
        request.sticky = Some(true);
        request.actions = vec![action(i18n.t("app.update.install", &[]), install_command(&update.version, None))];
        vec![UpdateEffect::Notify(request)]
    }

    fn balloon(&self, actions: Vec<NotificationAction>) -> Option<UpdateEffect> {
        let installing = self.installing.as_ref()?;
        let mut request = keyed(NotificationLevel::Info, installing.title.clone(), true);
        request.sticky = Some(true);
        request.actions = actions;
        Some(UpdateEffect::Notify(request))
    }

    /// Install and Restart `version` from `source` with the fresh request id `request`. An install
    /// already running is joined (nothing to do).
    pub fn install(&mut self, version: &str, source: UpdateSource, request: u64, i18n: &I18n) -> Vec<UpdateEffect> {
        if self.installing.is_some() {
            return Vec::new();
        }
        let title = i18n.t("app.update.downloading", &[("version", version.into())]);
        let mut running = task(INSTALL_TASK_ID, &title, false);
        running.cancel = Change::Set(Cancel::Producer);
        self.installing =
            Some(Installing { version: version.to_owned(), source, request, title, installing_shown: false });
        let mut effects = vec![UpdateEffect::Task(running)];
        // Replaces the notification whose action is now spent.
        effects.extend(self.balloon(vec![action(i18n.t("app.update.cancel", &[]), CANCEL_COMMAND)]));
        effects.push(UpdateEffect::Install { source, request });
        effects
    }

    /// Retry with <other source>: the source is stored, then the install runs from it.
    pub fn retry_from(&mut self, version: &str, source: UpdateSource, request: u64, i18n: &I18n) -> Vec<UpdateEffect> {
        let mut effects = vec![UpdateEffect::WriteSource(source)];
        effects.extend(self.install(version, source, request, i18n));
        effects
    }

    /// Cancel of the install's task or balloon.
    pub fn cancel_install(&self) -> Vec<UpdateEffect> {
        self.installing.iter().map(|installing| UpdateEffect::CancelRequest(installing.request)).collect()
    }

    /// Download progress of the running install. Replacing the app is one step that must not be
    /// cut short: from then on there is no Cancel.
    pub fn progress(&mut self, progress: UpdateProgress, i18n: &I18n) -> Vec<UpdateEffect> {
        let mut effects = Vec::new();
        let Some(installing) = self.installing.as_mut() else { return effects };
        let title = installing.title.clone();
        let replacing = progress == UpdateProgress::Installing;
        let first = replacing && !installing.installing_shown;
        installing.installing_shown |= replacing;
        if first {
            effects.extend(self.balloon(Vec::new()));
        }
        let (message, fraction) = progress_text(progress, i18n);
        let mut update = task(INSTALL_TASK_ID, &title, false);
        update.message = Some(message);
        update.fraction = fraction;
        if replacing {
            update.cancel = Change::Clear;
            update.uncancellable = Change::Set(i18n.t("app.update.installUncancellable", &[]));
        } else {
            update.cancel = Change::Set(Cancel::Producer);
            update.uncancellable = Change::Clear;
        }
        effects.push(UpdateEffect::Task(update));
        effects
    }

    /// The install finished: installed (the app restarts), failed (Retry, and Retry with the other
    /// source) or cancelled (Retry with the other source, the download was likely too slow).
    pub fn install_finished(&mut self, result: Result<(), IdeError>, i18n: &I18n) -> Vec<UpdateEffect> {
        let Some(installing) = self.installing.take() else { return Vec::new() };
        let mut effects = vec![UpdateEffect::Task(task(INSTALL_TASK_ID, &installing.title, true))];
        let version = installing.version.as_str();
        let elsewhere = other(installing.source);
        let retry_other = action(
            i18n.t("app.update.retryWith", &[("source", source_name(elsewhere).into())]),
            install_command(version, Some(elsewhere)),
        );
        let request = match result {
            Err(error) if error.kind == ErrorKind::Cancelled => {
                let mut request = keyed(NotificationLevel::Info, i18n.t("app.update.cancelled", &[]), true);
                request.actions = vec![retry_other];
                request
            }
            Err(error) => {
                let mut request = keyed(NotificationLevel::Error, i18n.t("app.update.installFailed", &[]), true);
                request.body = Some(error.message);
                let retry = action(i18n.t("app.update.retry", &[]), install_command(version, Some(installing.source)));
                request.actions = vec![retry, retry_other];
                request
            }
            Ok(()) => {
                let title = i18n.t("app.update.installed", &[("version", version.into())]);
                let mut request = keyed(NotificationLevel::Info, title, true);
                request.body = Some(i18n.t("app.update.installedBody", &[]));
                request.sticky = Some(true);
                request.actions = vec![action(i18n.t("app.update.restart", &[]), RESTART_COMMAND)];
                effects.push(UpdateEffect::Notify(request));
                effects.push(UpdateEffect::Restart);
                return effects;
            }
        };
        effects.push(UpdateEffect::Notify(request));
        effects
    }
}

/// What a window's [`UpdateChecks`] needs done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChecksEffect {
    /// Listen for the backend's `updateAvailable` event ([`Updater::update_available`]).
    Subscribe,
    Unsubscribe,
    /// `app_update_window_ready` with the system's language.
    WindowReady(bool),
}

/// A window takes part in the backend's automatic checks once it can receive their
/// notifications, and leaves them when it goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UpdateChecks {
    disposed: bool,
}

impl UpdateChecks {
    pub fn install() -> (Self, ChecksEffect) {
        (Self::default(), ChecksEffect::Subscribe)
    }

    /// The window receives backend events now.
    pub fn events_ready(&self) -> Option<ChecksEffect> {
        (!self.disposed).then_some(ChecksEffect::WindowReady(true))
    }

    pub fn dispose(&mut self) -> [ChecksEffect; 2] {
        self.disposed = true;
        [ChecksEffect::Unsubscribe, ChecksEffect::WindowReady(false)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(version: &str) -> UpdateInfo {
        UpdateInfo { version: version.to_owned(), current_version: "0.1.21".to_owned(), notes: None, date: None }
    }

    fn found(version: &str) -> UpdateCheck {
        UpdateCheck { enabled: true, update: Some(update(version)) }
    }

    fn notices(effects: &[UpdateEffect]) -> Vec<&NotificationRequest> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                UpdateEffect::Notify(request) => Some(request),
                _ => None,
            })
            .collect()
    }

    fn tasks(effects: &[UpdateEffect]) -> Vec<&TaskUpdate> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                UpdateEffect::Task(update) => Some(update),
                _ => None,
            })
            .collect()
    }

    fn unavailable(message: &str) -> IdeError {
        IdeError::new(ErrorKind::Unavailable, message)
    }

    const MB: f64 = 1024.0 * 1024.0;

    // Install and Restart › runs as a background task with download progress, then restarts
    #[test]
    fn runs_as_a_background_task_with_download_progress_then_restarts() {
        let i18n = I18n::default();
        let mut updater = Updater::new();
        let started = updater.install("0.1.22", UpdateSource::Github, 7, &i18n);
        let balloon = notices(&started)[0];
        assert_eq!((balloon.title.as_str(), balloon.key.as_deref()), ("Downloading OpenIDEA 0.1.22", Some(UPDATE_KEY)));
        assert_eq!((balloon.sticky, balloon.user_initiated), (Some(true), true));
        assert_eq!(balloon.actions[0].label, "Cancel");
        let running = tasks(&started)[0];
        assert_eq!(
            (running.title.as_str(), running.fraction, running.done),
            ("Downloading OpenIDEA 0.1.22", None, false)
        );
        assert_eq!(running.cancel, Change::Set(Cancel::Producer));
        assert!(started.contains(&UpdateEffect::Install { source: UpdateSource::Github, request: 7 }));

        let downloading =
            updater.progress(UpdateProgress::Downloading { downloaded: 21.0 * MB, total: Some(84.0 * MB) }, &i18n);
        let task = tasks(&downloading)[0];
        assert_eq!((task.fraction, task.message.as_deref()), (Some(0.25), Some("21.0 MB of 84.0 MB")));
        let replacing = updater.progress(UpdateProgress::Installing, &i18n);
        assert!(notices(&replacing)[0].actions.is_empty());
        let task = tasks(&replacing)[0];
        assert_eq!((task.fraction, task.message.as_deref()), (None, Some("Installing…")));
        assert_eq!(task.cancel, Change::Clear);

        let finished = updater.install_finished(Ok(()), &i18n);
        assert!(tasks(&finished)[0].done);
        assert_eq!(finished.iter().filter(|effect| **effect == UpdateEffect::Restart).count(), 1);
        let installed = notices(&finished)[0];
        assert_eq!(installed.title, "OpenIDEA 0.1.22 is installed");
        assert_eq!((installed.key.as_deref(), installed.sticky), (Some(UPDATE_KEY), Some(true)));
        assert_eq!(installed.actions[0].command, RESTART_COMMAND);
    }

    // Install and Restart › joins the running install on repeated clicks (balloon, log entry, a second check)
    #[test]
    fn joins_the_running_install_on_repeated_clicks() {
        let i18n = I18n::default();
        let mut updater = Updater::new();
        let available = updater.announced(&UpdateInfoShown::from(&update("0.1.22")), true, Ok(true), &i18n);
        let command = notices(&available)[0].actions[0].command.clone();
        assert_eq!(parse_install_command(&command), Some(("0.1.22".to_owned(), None)));
        assert!(!updater.install("0.1.22", UpdateSource::Github, 1, &i18n).is_empty());
        assert!(updater.install("0.1.22", UpdateSource::Github, 2, &i18n).is_empty());
        assert!(updater.install("0.1.22", UpdateSource::Github, 3, &i18n).is_empty());
        // A check while installing doesn't offer the install again.
        updater.check(UpdateSource::Github, 4, true, &i18n);
        let answered = updater.check_answered(4, Ok(found("0.1.22")), "0.1.21", &i18n);
        assert!(
            !answered.iter().any(|effect| matches!(effect, UpdateEffect::Announce { .. } | UpdateEffect::Notify(_)))
        );
        updater.install_finished(Ok(()), &i18n);
        assert!(
            updater
                .install("0.1.22", UpdateSource::Github, 5, &i18n)
                .contains(&UpdateEffect::Install { source: UpdateSource::Github, request: 5 })
        );
    }

    // Install and Restart › reports a failure with Retry, and a cancellation
    #[test]
    fn reports_a_failure_with_retry_and_a_cancellation() {
        let i18n = I18n::default();
        let mut updater = Updater::new();
        updater.install("0.1.22", UpdateSource::Github, 1, &i18n);
        let failed = updater.install_finished(Err(unavailable("connection reset")), &i18n);
        assert!(!failed.contains(&UpdateEffect::Restart));
        assert!(tasks(&failed)[0].done);
        let note = notices(&failed)[0];
        assert_eq!(
            (note.level, note.title.as_str(), note.body.as_deref()),
            (NotificationLevel::Error, "Could not install the update", Some("connection reset"))
        );
        assert_eq!(note.actions[0].label, "Retry");

        updater.install("0.1.22", UpdateSource::Github, 2, &i18n);
        assert_eq!(updater.cancel_install(), [UpdateEffect::CancelRequest(2)]);
        let cancelled = updater.install_finished(Err(IdeError::cancelled()), &i18n);
        let note = notices(&cancelled)[0];
        assert_eq!((note.level, note.title.as_str()), (NotificationLevel::Info, "Update cancelled"));
        assert_eq!(note.key.as_deref(), Some(UPDATE_KEY));
        assert!(!cancelled.contains(&UpdateEffect::Restart));
    }

    // Install and Restart › offers %s download from %s again from the other source
    #[test]
    fn offers_a_failed_or_cancelled_download_again_from_the_other_source() {
        let i18n = I18n::default();
        let cases = [
            (ErrorKind::Unavailable, UpdateSource::Github, "CNB", UpdateSource::Cnb),
            (ErrorKind::Unavailable, UpdateSource::Cnb, "GitHub", UpdateSource::Github),
            (ErrorKind::Cancelled, UpdateSource::Cnb, "GitHub", UpdateSource::Github),
        ];
        for (kind, from, label, to) in cases {
            let mut updater = Updater::new();
            updater.install("0.1.22", from, 1, &i18n);
            let ended = updater.install_finished(Err(IdeError::new(kind, "failed")), &i18n);
            let retry = notices(&ended)[0].actions.last().cloned().expect("a retry");
            assert_eq!(retry.label, format!("Retry with {label}"));
            let (version, source) = parse_install_command(&retry.command).expect("an install");
            assert_eq!(source, Some(to));
            let effects = updater.retry_from(&version, to, 2, &i18n);
            assert_eq!(effects[0], UpdateEffect::WriteSource(to));
            assert!(effects.contains(&UpdateEffect::Install { source: to, request: 2 }));
        }
    }

    // Check for Updates › shows a background task while checking and joins a running check
    #[test]
    fn shows_a_background_task_while_checking_and_joins_a_running_check() {
        let i18n = I18n::default();
        let mut updater = Updater::new();
        let first = updater.check(UpdateSource::Github, 1, false, &i18n);
        assert!(updater.check(UpdateSource::Github, 2, false, &i18n).is_empty());
        assert_eq!(first.iter().filter(|effect| matches!(effect, UpdateEffect::Check { .. })).count(), 1);
        let running = tasks(&first)[0];
        assert_eq!((running.title.as_str(), running.done), ("Checking for updates…", false));
        let answered = updater.check_answered(1, Ok(UpdateCheck { enabled: true, update: None }), "0.1.21", &i18n);
        assert!(tasks(&answered)[0].done);
        assert_eq!(notices(&answered)[0].title, "OpenIDEA 0.1.21 is up to date");
    }

    // Check for Updates › always ends in a notification (%#)
    #[test]
    fn always_ends_in_a_notification() {
        let i18n = I18n::default();
        let answers: [(Result<UpdateCheck, IdeError>, &str); 3] = [
            (Ok(UpdateCheck { enabled: false, update: None }), "Automatic updates are not configured"),
            (Ok(found("0.1.22")), "OpenIDEA 0.1.22 is available"),
            (Err(unavailable("offline")), "Update check failed"),
        ];
        for (answer, title) in answers {
            let mut updater = Updater::new();
            updater.check(UpdateSource::Github, 1, true, &i18n);
            let mut effects = updater.check_answered(1, answer, "0.1.21", &i18n);
            assert!(tasks(&effects)[0].done);
            if let Some(UpdateEffect::Announce { update, automatic }) = effects.first().cloned() {
                effects = updater.announced(&update, automatic, Ok(true), &i18n);
            }
            let note = notices(&effects)[0];
            assert_eq!((note.title.as_str(), note.user_initiated), (title, true));
        }
    }

    // Check for Updates › does not consume an announcement when a manual check is cancelled
    #[test]
    fn does_not_consume_an_announcement_when_a_manual_check_is_cancelled() {
        let i18n = I18n::default();
        let mut updater = Updater::new();
        updater.check(UpdateSource::Github, 1, true, &i18n);
        assert!(tasks(&updater.cancel_check())[0].done);
        assert!(updater.check_answered(1, Ok(found("0.1.22")), "0.1.21", &i18n).is_empty());
        let available = updater.update_available(&update("0.1.22"));
        assert_eq!(
            available,
            [UpdateEffect::Announce { update: UpdateInfoShown::from(&update("0.1.22")), automatic: true }]
        );
        let shown = updater.announced(&UpdateInfoShown::from(&update("0.1.22")), true, Ok(true), &i18n);
        assert_eq!(notices(&shown)[0].title, "OpenIDEA 0.1.22 is available");
    }

    // Check for Updates › suppresses an already announced automatic version but still answers a manual check
    #[test]
    fn suppresses_an_already_announced_automatic_version_but_still_answers_a_manual_check() {
        let i18n = I18n::default();
        let updater = Updater::new();
        let shown = UpdateInfoShown::from(&update("0.1.22"));
        assert!(updater.announced(&shown, true, Ok(false), &i18n).is_empty());
        // The backend answers a manual announcement with true whatever was announced before.
        let manual = updater.announced(&shown, false, Ok(true), &i18n);
        let note = notices(&manual)[0];
        assert_eq!((note.title.as_str(), note.user_initiated), ("OpenIDEA 0.1.22 is available", true));
        // When the backend cannot say, only a manual check announces.
        assert!(updater.announced(&shown, true, Err(unavailable("gone")), &i18n).is_empty());
        assert_eq!(notices(&updater.announced(&shown, false, Err(unavailable("gone")), &i18n)).len(), 1);
    }

    // Check for Updates › shows the check in a balloon where there is no status bar (the welcome screen)
    #[test]
    fn shows_the_check_in_a_balloon_where_there_is_no_status_bar() {
        let i18n = I18n::default();
        let mut updater = Updater::new();
        let started = updater.check(UpdateSource::Github, 1, false, &i18n);
        let balloon = notices(&started)[0];
        assert_eq!(
            (balloon.level, balloon.title.as_str(), balloon.key.as_deref()),
            (NotificationLevel::Info, "Checking for updates…", Some(UPDATE_KEY))
        );
        let answered = updater.check_answered(1, Ok(UpdateCheck { enabled: true, update: None }), "0.1.21", &i18n);
        let done = notices(&answered)[0];
        assert_eq!((done.title.as_str(), done.key.as_deref()), ("OpenIDEA 0.1.21 is up to date", Some(UPDATE_KEY)));
    }

    // Check for Updates › marks the manual check's progress as the user's own request
    #[test]
    fn marks_the_manual_checks_progress_as_the_users_own_request() {
        let mut updater = Updater::new();
        let started = updater.check(UpdateSource::Github, 1, false, &I18n::default());
        assert!(notices(&started)[0].user_initiated);
    }

    // Check for Updates › starts the process scheduler after subscribing and leaves automatic results to notification settings
    #[test]
    fn starts_the_process_scheduler_after_subscribing_and_leaves_automatic_results_to_notification_settings() {
        let i18n = I18n::default();
        let (mut checks, first) = UpdateChecks::install();
        assert_eq!(first, ChecksEffect::Subscribe);
        assert_eq!(checks.events_ready(), Some(ChecksEffect::WindowReady(true)));
        let updater = Updater::new();
        let Some(UpdateEffect::Announce { update, automatic }) = updater.update_available(&update("0.1.22")).pop()
        else {
            panic!("announces")
        };
        let shown = updater.announced(&update, automatic, Ok(true), &i18n);
        let note = notices(&shown)[0];
        assert_eq!((note.title.as_str(), note.user_initiated), ("OpenIDEA 0.1.22 is available", false));
        assert_eq!(checks.dispose()[0], ChecksEffect::Unsubscribe);
    }

    // Check for Updates › does not start a schedule after its window has been disposed
    #[test]
    fn does_not_start_a_schedule_after_its_window_has_been_disposed() {
        let (mut checks, _) = UpdateChecks::install();
        assert_eq!(checks.dispose(), [ChecksEffect::Unsubscribe, ChecksEffect::WindowReady(false)]);
        assert_eq!(checks.events_ready(), None);
    }

    // Check for Updates › leaves the check to the status bar in a project window
    #[test]
    fn leaves_the_check_to_the_status_bar_in_a_project_window() {
        let i18n = I18n::default();
        let mut updater = Updater::new();
        assert!(notices(&updater.check(UpdateSource::Github, 1, true, &i18n)).is_empty());
        let answered = updater.check_answered(1, Ok(UpdateCheck { enabled: true, update: None }), "0.1.21", &i18n);
        assert_eq!(notices(&answered)[0].title, "OpenIDEA 0.1.21 is up to date");
    }

    // updateSource › %s with the %s UI uses %s
    #[test]
    fn update_source_follows_the_setting_and_the_ui_language() {
        let cases = [
            ("auto", "zh", UpdateSource::Cnb),
            ("auto", "zh-CN", UpdateSource::Cnb),
            ("auto", "en", UpdateSource::Github),
            ("auto", "ko", UpdateSource::Github),
            ("", "zh", UpdateSource::Cnb),
            ("github", "zh", UpdateSource::Github),
            ("cnb", "en", UpdateSource::Cnb),
            ("bogus", "en", UpdateSource::Github),
        ];
        for (setting, locale, expected) in cases {
            let mut settings = Settings::default();
            settings.general.update_source = setting.to_owned();
            settings.appearance.locale = locale.to_owned();
            assert_eq!(update_source(&settings, "en-US"), expected, "{setting} {locale}");
        }
    }

    // updateSource › is passed to the check and the install
    #[test]
    fn is_passed_to_the_check_and_the_install() {
        let i18n = I18n::default();
        let mut settings = Settings::default();
        settings.general.update_source = "cnb".to_owned();
        settings.appearance.locale = "en".to_owned();
        let source = update_source(&settings, "en-US");
        let mut updater = Updater::new();
        assert!(
            updater
                .check(source, 1, true, &i18n)
                .contains(&UpdateEffect::Check { source: UpdateSource::Cnb, ticket: 1 })
        );
        let install = updater.install("0.1.22", source, 2, &i18n);
        assert!(install.contains(&UpdateEffect::Install { source: UpdateSource::Cnb, request: 2 }));
    }

    // progressText › formats known and unknown lengths
    #[test]
    fn formats_known_and_unknown_lengths() {
        let i18n = I18n::default();
        let unknown = progress_text(UpdateProgress::Downloading { downloaded: MB, total: None }, &i18n);
        assert_eq!(unknown, ("1.0 MB".to_owned(), None));
        let over = progress_text(UpdateProgress::Downloading { downloaded: 10.0, total: Some(5.0) }, &i18n);
        assert_eq!(over.1, Some(1.0));
        // Ties round up, as the download size is shown everywhere else.
        assert_eq!(megabytes(0.25 * MB), "0.3");
    }
}
