//! Deployment in a project window: Tools › Deployment's actions (upload, download, delete from the
//! server, Sync with Deployed, Automatic Upload, Open in Browser), the Remote Host tool window and
//! the dialogs they wait on (Sync with Deployed, SSH Password, the server chooser, confirmations).
//!
//! The decisions are the models' of [`crate::features_b::deployment`]; this module carries out
//! what they return: the backend calls of `ide_host::ops::deploy` (off the main thread, each
//! answer handed back with the request id, generation or ticket it was asked with), the
//! notifications, and the views of [`crate::deployment`].
//!
//! Remote work over SSH is retried after the user trusts an unknown host key or types a password
//! or passphrase the server asked for ([`ssh_retry`]); a local server needs neither.

use super::Workbench;
use super::notifications::{NotificationLevel, NotificationRequest};
use super::services_window::on_workbench;
use crate::backend::Backend;
use crate::database::store::SecretText;
use crate::deployment::remote_host_view::{HostCommand, HostServer, RemoteHostEvent, RemoteHostState, RemoteHostView};
use crate::deployment::secret_dialog::{SecretDialog, SecretDialogEvent};
use crate::deployment::sync_dialog::{SyncDialog, SyncDialogEvent};
use crate::deployment::sync_preview::LoadedPreview;
use crate::features_b::deployment::REMOTE_HOST_WINDOW;
use crate::features_b::deployment::actions::{
    self as rules, BrowserOpen, CONFIGURE_ACTION, Eligibility, GROUP, Notice, NoticeBody, ServerPick, TransferDirection,
};
use crate::features_b::deployment::comparison::{
    ComparisonOp, ComparisonStep, DeploymentComparison, DeploymentPreview, DiffSide,
};
use crate::features_b::deployment::model::{
    FocusedPart, ReportVerb, SshRemedy, TargetContext, join_remote, remote_parent, resolve_server, server_label,
    server_ssh, ssh_retry, target_paths,
};
use crate::features_b::deployment::preview::{PreviewLoad, PreviewRequests};
use crate::features_b::deployment::remote_host::{HostEffect, HostRequest, RemoteHost};
use crate::features_b::deployment::store::{DeploymentStore, SyncEffect};
use crate::features_b::deployment::sync_view::ScanRequest;
use crate::features_b::deployment::types::{DeploymentCoverage, DeploymentRoot, SyncEntry, SyncStep, TransferReport};
use crate::features_b::deployment::window::{ComparisonWindow, LaunchRequest, LaunchStep};
use crate::platform::actions::{ActionRegistry, ActionSource, execute_action_from};
use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::settings;
use crate::platform_more::remote_settings::{deployment_connection, normalized_deployment};
use crate::vcs::prompt_dialogs::{DialogButton, FieldRule, Message, OK, PromptDialog};
use crate::vcs_ops::dialogs::{ConfirmRequest, PromptRequest};
use crate::vcs_ops::menu::MenuItem;
use futures::channel::oneshot;
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, AsyncWindowContext, ClipboardItem, Context, Entity, FocusHandle, Focusable,
    Subscription, WeakEntity, Window, div, prelude::*, px,
};
use ide_core::settings::{DeploymentServer, DeploymentSettings};
use ide_core::{IdeError, IdeResult, IpcPath};
use ide_host::Host;
use ide_ssh::RemoteEntry;
use ide_vfs::{ByteRange, FileProbe};
use ide_workspace::settings::SettingsScope;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

type This = WeakEntity<Workbench>;

/// The page Deployment Configuration… opens in Settings.
const SETTINGS_PAGE: &str = "build.deployment";
/// How long an operation waits before it looks again at a snapshot another one is downloading.
const SNAPSHOT_POLL: Duration = Duration::from_millis(20);

/// What deployment keeps for a window: the part that had the keyboard last, and the state of the
/// project open in it.
#[derive(Default)]
pub(super) struct DeploymentState {
    part: Option<FocusedPart>,
    slot: Option<DeploymentSlot>,
}

/// Deployment's state for one project: it starts over when another project opens in the window.
pub(super) struct DeploymentSlot {
    root: String,
    store: Entity<DeploymentStore>,
    sync: Option<SyncSlot>,
    secret: Option<SecretSlot>,
    /// The Remote Host tool window asked for its server's root.
    host_started: bool,
    /// The window the Remote Host tool window is in, for work that starts outside an update of it.
    host_window: Option<AnyWindowHandle>,
}

/// The Sync with Deployed comparison on screen.
struct SyncSlot {
    dialog: Entity<SyncDialog>,
    window: ComparisonWindow,
    comparison: DeploymentComparison,
    /// Whether a preview load was replaced by another (it ends as aborted), by ticket.
    previews: HashMap<u64, Rc<Cell<bool>>>,
    restore: Option<FocusHandle>,
    _events: Subscription,
}

/// The SSH Password dialog and the work waiting on its answer.
struct SecretSlot {
    dialog: Entity<SecretDialog>,
    answer: Option<oneshot::Sender<Option<(SecretText, bool)>>>,
    restore: Option<FocusHandle>,
    _events: Subscription,
}

/// The answer of `deploy_info` that Open in Browser reads.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeployedUrl {
    web_url: Option<String>,
}

/// Which server an action uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pick {
    /// The default server, else a chooser among the servers covering the selection.
    Default,
    /// Always the chooser, even for one server.
    Choose,
}

/// What a read of a comparison came back with, for its operation to take.
enum Answer {
    Downloaded(Result<String, IdeError>),
    Resume,
    Probed(Result<Vec<FileProbe>, IdeError>),
    Text(Result<String, IdeError>),
    Ranges(Result<(ByteRange, ByteRange), IdeError>),
}

/// A value of the backend's deployment types as the same shape of this crate's (and the other way):
/// the fields and their serde names are the same on both sides.
fn bridge<A: Serialize, B: DeserializeOwned>(value: A) -> IdeResult<B> {
    serde_json::to_value(value)
        .and_then(serde_json::from_value)
        .map_err(|error| IdeError::internal(format!("unexpected deployment data: {error}")))
}

fn ipc_paths(paths: &[String]) -> Vec<IpcPath> {
    paths.iter().map(|path| IpcPath::from(path.as_str())).collect()
}

/// Runs `work` on the backend for the window's project. A window that went away is a cancellation.
async fn deploy_call<T, F>(
    this: &This,
    cx: &mut AsyncWindowContext,
    work: impl FnOnce(Arc<Host>, String) -> F + Send + 'static,
) -> Result<T, IdeError>
where
    T: Send + 'static,
    F: Future<Output = IdeResult<T>> + Send + 'static,
{
    let call = this.update(cx, |workbench, cx| {
        let label = workbench.label.clone();
        cx.try_global::<Backend>().map(|backend| backend.with_host(move |host| work(host, label)))
    });
    match call {
        Err(_) => Err(IdeError::cancelled()),
        Ok(None) => Err(IdeError::internal("the backend is not running")),
        Ok(Some(call)) => call.await.unwrap_or_else(|error| Err(IdeError::internal(error.to_string()))),
    }
}

/// Runs remote work over SSH configuration `ssh` ("" for a local server): when it fails on an
/// unknown host key or a missing or refused secret, offers to trust the key or asks for the secret,
/// then runs it again, up to three times.
async fn with_ssh<T, F, W>(this: &This, cx: &mut AsyncWindowContext, ssh: &str, work: W) -> Result<T, IdeError>
where
    T: Send + 'static,
    F: Future<Output = IdeResult<T>> + Send + 'static,
    W: Fn(Arc<Host>, String) -> F + Send + Sync + 'static,
{
    let work = Arc::new(work);
    let mut attempt = 0;
    loop {
        let once = Arc::clone(&work);
        let error = match deploy_call(this, cx, move |host, label| once(host, label)).await {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        let configs = this
            .update(cx, |workbench, cx| settings::effective(&workbench.label, cx).ssh.configs)
            .map_err(|_| IdeError::cancelled())?;
        let Some(remedy) = ssh_retry(attempt, ssh, &error, &configs) else { return Err(error) };
        attempt += 1;
        let id = ssh.to_owned();
        match remedy {
            SshRemedy::TrustHostKey => {
                let request = ConfirmRequest {
                    title: "Unknown Host Key".to_owned(),
                    message: error.message.clone(),
                    ok_label: "Trust and Connect".to_owned(),
                    danger: true,
                };
                let answer = this
                    .update_in(cx, |workbench, window, cx| {
                        workbench.show_prompt_dialog(
                            move |window, cx| PromptDialog::confirm(request, window, cx),
                            window,
                            cx,
                        )
                    })
                    .map_err(|_| IdeError::cancelled())?;
                if !answer.await.is_ok_and(|answer| answer.confirmed()) {
                    return Err(error);
                }
                deploy_call(this, cx, move |host, _| async move { host.ssh.trust_host(&id).await }).await?;
            }
            SshRemedy::Secret => {
                let message = error.message.clone();
                let answer = this
                    .update_in(cx, |workbench, window, cx| workbench.ask_ssh_secret(&id, &message, window, cx))
                    .map_err(|_| IdeError::cancelled())?;
                let Ok(Some((secret, save))) = answer.await else { return Err(error) };
                deploy_call(this, cx, move |host, _| async move {
                    host.ssh.set_secret(&id, Some(secret.into_inner()), save);
                    Ok(())
                })
                .await?;
            }
        }
    }
}

/// `f` with the UI language; `None` once the window went away.
fn with_i18n<R>(this: &This, cx: &mut AsyncWindowContext, f: impl FnOnce(&mut I18n) -> R) -> Option<R> {
    this.update(cx, |_, cx| f(&mut cx.default_global::<Localization>().0)).ok()
}

fn notify_notice(this: &This, cx: &mut AsyncWindowContext, notice: Notice) {
    if this.update(cx, |workbench, cx| workbench.notify_deployment(notice, cx)).is_err() {
        tracing::debug!("the window closed before its deployment notification");
    }
}

/// Which of `paths` the servers deploy (`deploy_coverage`); a failure is told.
async fn coverage(this: &This, cx: &mut AsyncWindowContext, paths: &[String]) -> Option<Vec<DeploymentCoverage>> {
    let paths = ipc_paths(paths);
    let answer: Result<Vec<DeploymentCoverage>, IdeError> = deploy_call(this, cx, move |host, label| async move {
        bridge(ide_host::ops::deploy::deploy_coverage(&label, &host, paths).await?)
    })
    .await;
    match answer {
        Ok(coverage) => Some(coverage),
        Err(error) if rules::is_cancelled(&error) => None,
        Err(error) => {
            let notice = with_i18n(this, cx, |i18n| rules::coverage_failed_notice(&error, i18n))?;
            notify_notice(this, cx, notice);
            None
        }
    }
}

/// The server chosen among those whose mappings cover `paths` (the chooser shows even for one);
/// without one, why is told.
async fn pick_server(this: &This, cx: &mut AsyncWindowContext, paths: &[String]) -> Option<DeploymentServer> {
    let configured = this.update(cx, |workbench, cx| workbench.deployment_settings(cx).servers).ok()?;
    let coverage = if configured.is_empty() { Vec::new() } else { coverage(this, cx, paths).await? };
    match with_i18n(this, cx, |i18n| rules::pick_server(&configured, &coverage, i18n))? {
        ServerPick::Notify(notice) => {
            notify_notice(this, cx, notice);
            None
        }
        ServerPick::Choose { labels, servers } => {
            let shown = labels.clone();
            let answer = this.update_in(cx, |workbench, window, cx| workbench.choose_server(shown, window, cx)).ok()?;
            let picked = answer.await.ok()?;
            let id = rules::chosen_server(&labels, &servers, &picked)?.to_owned();
            configured.into_iter().find(|server| server.id == id)
        }
    }
}

/// The default server, else the one chosen.
async fn default_server(this: &This, cx: &mut AsyncWindowContext, paths: &[String]) -> Option<DeploymentServer> {
    let settings = this.update(cx, |workbench, cx| workbench.deployment_settings(cx)).ok()?;
    match resolve_server(&settings, None) {
        Some(server) => Some(server.clone()),
        None => pick_server(this, cx, paths).await,
    }
}

/// The coverage of `paths` by `server`, when something can be deployed to it; otherwise why not
/// is told.
async fn covered_by(this: &This, cx: &mut AsyncWindowContext, server: &DeploymentServer, paths: &[String]) -> bool {
    let Some(coverage) = coverage(this, cx, paths).await else { return false };
    let refused = with_i18n(this, cx, |i18n| match rules::server_coverage(&coverage, server, i18n) {
        Eligibility::Covered(_) => Ok(()),
        Eligibility::Unknown => Err(None),
        Eligibility::Rejected(notice) => Err(Some(notice)),
    });
    match refused {
        Some(Ok(())) => true,
        Some(Err(Some(notice))) => {
            notify_notice(this, cx, notice);
            false
        }
        _ => false,
    }
}

/// Uploads or downloads `paths` with `server`, and tells how it went.
async fn transfer(
    this: &This,
    cx: &mut AsyncWindowContext,
    direction: TransferDirection,
    server: DeploymentServer,
    paths: Vec<String>,
) {
    if !covered_by(this, cx, &server, &paths).await {
        return;
    }
    let request = Backend::next_request_id();
    let (ipc, id) = (ipc_paths(&paths), server.id.clone());
    let report: Result<TransferReport, IdeError> = with_ssh(this, cx, server_ssh(&server), move |host, label| {
        let (paths, id) = (ipc.clone(), Some(id.clone()));
        async move {
            let report = match direction {
                TransferDirection::Upload => {
                    ide_host::ops::deploy::deploy_upload(&label, &host, paths, id, request).await?
                }
                TransferDirection::Download => {
                    ide_host::ops::deploy::deploy_download(&label, &host, paths, id, request).await?
                }
            };
            bridge(report)
        }
    })
    .await;
    let notice = with_i18n(this, cx, |i18n| match &report {
        Ok(report) => Some(rules::report_notice(report, direction.verb(), &server, i18n)),
        Err(error) => rules::failure_notice(direction.failure_title(), &server, error, i18n),
    });
    if let Some(Some(notice)) = notice {
        notify_notice(this, cx, notice);
    }
}

/// Delete from Server: asks, then deletes the remote copies of `paths` on the default server.
async fn delete_from_server(this: &This, cx: &mut AsyncWindowContext, paths: Vec<String>) {
    let Some(server) = default_server(this, cx, &paths).await else { return };
    if !covered_by(this, cx, &server, &paths).await {
        return;
    }
    let root = this.update(cx, |workbench, cx| workbench.project_root(cx)).ok().flatten();
    let root = root.map(|root| root.to_string_lossy().into_owned());
    let Some(message) =
        with_i18n(this, cx, |i18n| rules::delete_confirm_message(&paths, root.as_deref(), &server, i18n))
    else {
        return;
    };
    let request = ConfirmRequest::danger("Delete from Server", message, "Delete");
    let Ok(answer) = this.update_in(cx, |workbench, window, cx| {
        workbench.show_prompt_dialog(move |window, cx| PromptDialog::confirm(request, window, cx), window, cx)
    }) else {
        return;
    };
    if !answer.await.is_ok_and(|answer| answer.confirmed()) {
        return;
    }
    let (ipc, id) = (ipc_paths(&paths), server.id.clone());
    let report: Result<TransferReport, IdeError> = with_ssh(this, cx, server_ssh(&server), move |host, label| {
        let (paths, id) = (ipc.clone(), Some(id.clone()));
        async move { bridge(ide_host::ops::deploy::deploy_delete_remote(&label, &host, paths, id).await?) }
    })
    .await;
    let notice = with_i18n(this, cx, |i18n| match &report {
        Ok(report) => Some(rules::report_notice(report, ReportVerb::Delete, &server, i18n)),
        Err(error) => rules::failure_notice("Cannot delete from {server}", &server, error, i18n),
    });
    if let Some(Some(notice)) = notice {
        notify_notice(this, cx, notice);
    }
}

/// Open in Browser (Deployed): the deployed URL of the first path on the default server.
async fn open_in_browser(this: &This, cx: &mut AsyncWindowContext, path: String) {
    let answer: Result<Option<DeployedUrl>, IdeError> = deploy_call(this, cx, move |host, label| async move {
        let info = ide_host::ops::deploy::deploy_info(&label, &host, IpcPath::from(path.as_str()), None).await?;
        bridge(info)
    })
    .await;
    let result = match answer {
        Ok(info) => {
            let url = info.as_ref().and_then(|info| info.web_url.clone());
            with_i18n(this, cx, |i18n| rules::open_in_browser(info.is_some(), url.as_deref(), i18n))
        }
        Err(error) => {
            let notice = with_i18n(this, cx, |i18n| Notice {
                level: NotificationLevel::Error,
                title: i18n.tx("Cannot open in browser"),
                body: Some(NoticeBody::Text(i18n.tx_template(&error.message))),
                key: None,
                actions: Vec::new(),
            });
            notice.map(BrowserOpen::Notify)
        }
    };
    match result {
        Some(BrowserOpen::Url(url)) => {
            if this.update(cx, |_, cx| cx.open_url(&url)).is_err() {
                tracing::debug!("the window closed before the deployed URL opened");
            }
        }
        Some(BrowserOpen::Notify(notice)) => notify_notice(this, cx, notice),
        None => {}
    }
}

/// Runs a comparison operation to its end: every read it asks for goes to the backend, and its
/// answer back to the operation. `aborted` turns true once another load replaced it.
async fn drive(
    this: &This,
    cx: &mut AsyncWindowContext,
    mut op: ComparisonOp,
    mut step: ComparisonStep,
    aborted: Option<Rc<Cell<bool>>>,
) -> ComparisonStep {
    loop {
        let answer = match step {
            ComparisonStep::Download { connection, remote_path } => Answer::Downloaded(
                deploy_call(this, cx, move |host, _| async move {
                    let local = ide_host::ops::deploy::remote_open(&host, connection, remote_path).await?;
                    Ok(local.to_string_lossy().into_owned())
                })
                .await,
            ),
            ComparisonStep::AwaitSnapshot { .. } => {
                cx.background_executor().timer(SNAPSHOT_POLL).await;
                Answer::Resume
            }
            ComparisonStep::Probe { paths } => Answer::Probed(
                deploy_call(this, cx, move |host, _| async move {
                    let mut probes = Vec::with_capacity(paths.len());
                    for path in paths {
                        probes.push(ide_host::ops::bytes::fs_probe(&host, "", PathBuf::from(path)).await?);
                    }
                    Ok(probes)
                })
                .await,
            ),
            ComparisonStep::ReadText { path } => Answer::Text(
                deploy_call(this, cx, move |host, label| async move {
                    ide_host::ops::fs::read_text(&host, &label, PathBuf::from(path)).await
                })
                .await,
            ),
            ComparisonStep::ReadRanges { local, remote, offset, len } => {
                let len = u32::try_from(len).unwrap_or(u32::MAX);
                Answer::Ranges(
                    deploy_call(this, cx, move |host, _| async move {
                        let ours =
                            ide_host::ops::bytes::fs_read_range(&host, "", PathBuf::from(local), offset, len).await?;
                        let theirs =
                            ide_host::ops::bytes::fs_read_range(&host, "", PathBuf::from(remote), offset, len).await?;
                        Ok((ours, theirs))
                    })
                    .await,
                )
            }
            done => return done,
        };
        if aborted.as_ref().is_some_and(|aborted| aborted.get()) {
            op.abort();
        }
        let next = this.update(cx, |workbench, cx| {
            workbench.with_comparison(cx, |comparison, i18n| match answer {
                Answer::Downloaded(result) => op.downloaded(comparison, result, i18n),
                Answer::Resume => op.resume(comparison, i18n),
                Answer::Probed(Ok(probes)) => op.probed(&probes, i18n),
                Answer::Text(Ok(text)) => op.text_read(text, i18n),
                Answer::Ranges(Ok((ours, theirs))) => op.ranges_read(&ours, &theirs, i18n),
                Answer::Probed(Err(error)) | Answer::Text(Err(error)) | Answer::Ranges(Err(error)) => {
                    op.failed(&error, i18n)
                }
            })
        });
        step = match next {
            Ok(Some(step)) => step,
            // The comparison closed meanwhile.
            _ => return ComparisonStep::Aborted,
        };
    }
}

/// Registers the deployment actions.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    let transfers = [
        ("deployment.uploadToDefault", TransferDirection::Upload, Pick::Default),
        ("deployment.uploadTo", TransferDirection::Upload, Pick::Choose),
        ("deployment.downloadFromDefault", TransferDirection::Download, Pick::Default),
        ("deployment.downloadFrom", TransferDirection::Download, Pick::Choose),
    ];
    for (id, direction, pick) in transfers {
        registry.handle(id, move |call, window, cx| {
            let arg = call.arg.cloned().unwrap_or(Value::Null);
            on_workbench(window, cx, |workbench, window, cx| {
                workbench.deployment_transfer(direction, pick, &arg, window, cx)
            })
        });
    }
    registry.handle("deployment.syncWith", |call, window, cx| {
        let arg = call.arg.cloned().unwrap_or(Value::Null);
        on_workbench(window, cx, |workbench, window, cx| workbench.deployment_sync_with(Pick::Choose, &arg, window, cx))
    });
    registry.handle("deployment.syncWithDefault", |call, window, cx| {
        let arg = call.arg.cloned().unwrap_or(Value::Null);
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.deployment_sync_with(Pick::Default, &arg, window, cx)
        })
    });
    registry.handle("deployment.deleteFromServer", |call, window, cx| {
        let arg = call.arg.cloned().unwrap_or(Value::Null);
        on_workbench(window, cx, |workbench, window, cx| {
            let Some(paths) = workbench.deployment_targets(&arg, cx) else { return };
            cx.spawn_in(window, async move |this, cx| delete_from_server(&this, cx, paths).await).detach();
        })
    });
    registry.handle("deployment.openInBrowser", |call, window, cx| {
        let arg = call.arg.cloned().unwrap_or(Value::Null);
        on_workbench(window, cx, |workbench, window, cx| {
            let Some(path) = workbench.deployment_targets(&arg, cx).and_then(|paths| paths.into_iter().next()) else {
                return;
            };
            cx.spawn_in(window, async move |this, cx| open_in_browser(&this, cx, path).await).detach();
        })
    });
    registry.handle_toggle(
        "deployment.toggleAutoUpload",
        |window, cx| {
            super::for_window(window, cx).is_some_and(|workbench| {
                let label = workbench.read(cx).label.clone();
                rules::auto_upload_on(&settings::effective(&label, cx).deployment)
            })
        },
        |_, window, cx| on_workbench(window, cx, |workbench, _, cx| workbench.toggle_auto_upload(cx)),
    );
    registry.handle("deployment.browseRemoteHost", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.with_layout(window, cx, |layout| layout.show(REMOTE_HOST_WINDOW, true));
        })
    });
    registry.handle(CONFIGURE_ACTION, |_, window, cx| {
        window.defer(cx, |window, cx| {
            if !execute_action_from(ActionSource::Other, "file.settings", Some(&json!(SETTINGS_PAGE)), window, cx) {
                tracing::info!("file.settings is not available in this window");
            }
        });
        Ok(())
    });
}

impl Workbench {
    // ---------------------------------------------------------------------------------------------
    // State

    /// Deployment's state for the project open here, made (again) for a project opened since.
    fn deployment_slot(&mut self, cx: &mut App) -> Option<&mut DeploymentSlot> {
        let root = self.project_root(cx)?.to_string_lossy().into_owned();
        if self.deployment.slot.as_ref().is_none_or(|slot| slot.root != root) {
            let store = cx.new(|_| DeploymentStore::default());
            self.deployment.slot =
                Some(DeploymentSlot { root, store, sync: None, secret: None, host_started: false, host_window: None });
        }
        self.deployment.slot.as_mut()
    }

    fn deployment_store(&mut self, cx: &mut App) -> Option<Entity<DeploymentStore>> {
        self.deployment_slot(cx).map(|slot| slot.store.clone())
    }

    /// Settings › Deployment as in effect for this window, every server with an id of its own.
    fn deployment_settings(&self, cx: &App) -> DeploymentSettings {
        normalized_deployment(&settings::effective(&self.label, cx).deployment)
    }

    fn deployment_server(&self, id: &str, cx: &App) -> Option<DeploymentServer> {
        self.deployment_settings(cx).servers.into_iter().find(|server| server.id == id)
    }

    /// Notes which part had the keyboard last: the files an action run from elsewhere (Find
    /// Action) works on are its selection, or the editor's file.
    pub(super) fn track_deployment_focus(&mut self, project_view: bool, editor: bool) {
        if project_view {
            self.deployment.part = Some(FocusedPart::Project);
        } else if editor {
            self.deployment.part = Some(FocusedPart::Editor);
        }
    }

    /// The files an action works on (the menu's paths, the Project view's selection or the editor's
    /// file); without any, how to pick some is told and `None` returned.
    fn deployment_targets(&mut self, arg: &Value, cx: &mut Context<Self>) -> Option<Vec<String>> {
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        let selection: Vec<String> =
            self.project_tree().and_then(|tree| tree.read(cx).selected_path().map(str::to_owned)).into_iter().collect();
        let editor = self.editor().map(|editor| editor.read(cx).path().to_string_lossy().into_owned());
        let context = TargetContext {
            part: self.deployment.part,
            selection: &selection,
            editor_path: editor.as_deref(),
            project_root: root.as_deref(),
        };
        let paths = target_paths(arg, &context);
        if paths.is_empty() {
            let notice = rules::no_targets_notice(&cx.default_global::<Localization>().0);
            self.notify_deployment(notice, cx);
            return None;
        }
        Some(paths)
    }

    /// Raises `notice` in the External Tools group; a report body lists its skipped files and
    /// failures, project-relative.
    fn notify_deployment(&mut self, notice: Notice, cx: &mut Context<Self>) {
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        let body = notice.body.map(|body| match body {
            NoticeBody::Text(text) => text,
            NoticeBody::Report(report) => {
                rules::report_lines(&report, root.as_deref(), &mut cx.default_global::<Localization>().0).join("\n")
            }
        });
        let mut request = NotificationRequest::new(notice.level, notice.title);
        request.body = body;
        request.key = notice.key;
        request.actions = notice.actions;
        request.group = Some(GROUP.to_owned());
        self.notify(request, cx);
    }

    /// Shows the servers to choose from under the window's top; the receiver gets the label chosen,
    /// and is dropped when the menu closes without a choice.
    fn choose_server(
        &mut self,
        labels: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<String> {
        let (sender, answer) = oneshot::channel();
        let sender = Rc::new(RefCell::new(Some(sender)));
        let items: Vec<MenuItem<String>> =
            labels.into_iter().map(|label| MenuItem::item(label.clone(), label)).collect();
        let position = gpui_kit::point(px(f32::from(window.viewport_size().width) / 2.0 - 120.0), px(80.0));
        self.show_menu(
            &items,
            position,
            move |_, label: String, _, _| {
                if let Some(sender) = sender.borrow_mut().take() {
                    // The work waiting on it may have ended meanwhile.
                    let _ = sender.send(label);
                }
            },
            window,
            cx,
        );
        answer
    }

    // ---------------------------------------------------------------------------------------------
    // Actions

    fn deployment_transfer(
        &mut self,
        direction: TransferDirection,
        pick: Pick,
        arg: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(paths) = self.deployment_targets(arg, cx) else { return };
        cx.spawn_in(window, async move |this, cx| {
            let server = match pick {
                Pick::Default => default_server(&this, cx, &paths).await,
                Pick::Choose => pick_server(&this, cx, &paths).await,
            };
            if let Some(server) = server {
                transfer(&this, cx, direction, server, paths).await;
            }
        })
        .detach();
    }

    /// Sync with Deployed to… (a chooser) or to the default server: the comparison of the targets
    /// (or, for the chooser without targets, of the project) with the server.
    fn deployment_sync_with(&mut self, pick: Pick, arg: &Value, window: &mut Window, cx: &mut Context<Self>) {
        let paths = match pick {
            Pick::Choose => {
                let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
                let selection: Vec<String> = self
                    .project_tree()
                    .and_then(|tree| tree.read(cx).selected_path().map(str::to_owned))
                    .into_iter()
                    .collect();
                let editor = self.editor().map(|editor| editor.read(cx).path().to_string_lossy().into_owned());
                let context = TargetContext {
                    part: self.deployment.part,
                    selection: &selection,
                    editor_path: editor.as_deref(),
                    project_root: root.as_deref(),
                };
                let targets = target_paths(arg, &context);
                match (targets.is_empty(), root) {
                    (false, _) => targets,
                    (true, Some(root)) => vec![root],
                    (true, None) => return,
                }
            }
            Pick::Default => match self.deployment_targets(arg, cx) {
                Some(paths) => paths,
                None => return,
            },
        };
        cx.spawn_in(window, async move |this, cx| {
            let server = match pick {
                Pick::Default => default_server(&this, cx, &paths).await,
                Pick::Choose => pick_server(&this, cx, &paths).await,
            };
            let Some(server) = server else { return };
            if this.update_in(cx, |workbench, window, cx| workbench.open_comparison(server, paths, window, cx)).is_err()
            {
                tracing::debug!("the window closed before the comparison opened");
            }
        })
        .detach();
    }

    fn toggle_auto_upload(&mut self, cx: &mut Context<Self>) {
        let value = rules::toggled_auto_upload(&self.deployment_settings(cx));
        let stored = serde_json::to_value(value)
            .map_err(|error| IdeError::internal(error.to_string()))
            .and_then(|value| settings::set(&self.label, SettingsScope::Project, "deployment.autoUpload", &value, cx));
        if let Err(error) = stored {
            let i18n = &cx.default_global::<Localization>().0;
            let notice = Notice {
                level: NotificationLevel::Error,
                title: i18n.tx("Cannot change automatic upload"),
                body: Some(NoticeBody::Text(i18n.tx_template(&error.message))),
                key: None,
                actions: Vec::new(),
            };
            self.notify_deployment(notice, cx);
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Sync with Deployed

    /// Opens the comparison of `paths` with `server` as the comparison window does: the server is
    /// looked up again in the project's settings, the selection compared, and an error shown when
    /// nothing could be compared.
    fn open_comparison(
        &mut self,
        server: DeploymentServer,
        paths: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(store) = self.deployment_store(cx) else { return };
        let mut model = ComparisonWindow::new(&self.label, &format!("deployment-{}", self.label));
        let launch = LaunchRequest { server: server.id.clone(), paths };
        cx.default_global::<Localization>();
        let step = model.launch_taken(Some(launch), store.read(cx), &cx.global::<Localization>().0);
        match step {
            LaunchStep::LoadProject => {
                // The comparison open is replaced: what it waits for is cancelled.
                if let Some(open) = self.deployment.slot.as_mut().and_then(|slot| slot.sync.take()) {
                    if let Some(restore) = open.restore {
                        window.focus(&restore, cx);
                    }
                    let effects = store.update(cx, |store, cx| {
                        let effects = store.close_sync();
                        cx.notify();
                        effects
                    });
                    self.carry_out_sync(effects, window, cx);
                }
            }
            LaunchStep::ShowError { title, message } => {
                let message = Message {
                    title,
                    body: message,
                    warning: true,
                    checkbox: None,
                    buttons: vec![DialogButton {
                        id: OK.to_owned(),
                        label: "OK".to_owned(),
                        primary: true,
                        danger: false,
                    }],
                };
                // The message only informs: its answer changes nothing.
                drop(self.show_prompt_dialog(move |window, cx| PromptDialog::message(message, window, cx), window, cx));
                return;
            }
            _ => return,
        }
        let deployment = self.deployment_settings(cx);
        let step = store.update(cx, |store, cx| {
            let step = model.project_loaded(Some(&deployment), store, &mut cx.default_global::<Localization>().0);
            cx.notify();
            step
        });
        let LaunchStep::Open { server: launched, paths, .. } = step else {
            self.comparison_failed(&server, &model, cx);
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let Some(coverage) = coverage(&this, cx, &paths).await else { return };
            let request = Backend::next_request_id();
            let opened = this.update_in(cx, |workbench, window, cx| {
                let effects = store.update(cx, |store, cx| {
                    let i18n = &mut cx.default_global::<Localization>().0;
                    let effects = store.open_sync(&launched, paths, &coverage, request, i18n);
                    cx.notify();
                    effects
                });
                workbench.carry_out_sync(effects, window, cx);
                let step =
                    store.update(cx, |store, cx| model.sync_opened(store, &cx.default_global::<Localization>().0));
                if step == LaunchStep::Failed {
                    workbench.comparison_failed(&launched, &model, cx);
                } else {
                    workbench.show_comparison(model, &launched, window, cx);
                }
            });
            if opened.is_err() {
                tracing::debug!("the window closed before the comparison opened");
            }
        })
        .detach();
    }

    /// The comparison could not open: its error is told as the comparison window's would be.
    fn comparison_failed(&mut self, server: &DeploymentServer, model: &ComparisonWindow, cx: &mut Context<Self>) {
        let Some(message) = model.startup_error.clone() else { return };
        let error = IdeError::not_found(message);
        let notice = rules::failure_notice(
            "Cannot open comparison for {server}",
            server,
            &error,
            &cx.default_global::<Localization>().0,
        );
        if let Some(notice) = notice {
            self.notify_deployment(notice, cx);
        }
    }

    fn show_comparison(
        &mut self,
        model: ComparisonWindow,
        server: &DeploymentServer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.deployment_slot(cx) else { return };
        let (store, root) = (slot.store.clone(), slot.root.clone());
        let server_root = server.root_path.clone();
        let dialog = cx.new(|cx| SyncDialog::new(store, root.clone(), server_root, true, window, cx));
        let events = cx.subscribe_in(&dialog, window, Self::sync_dialog_event);
        let restore = window.focused(cx);
        window.focus(&dialog.focus_handle(cx), cx);
        let comparison = DeploymentComparison::new(server, &root);
        if let Some(slot) = self.deployment.slot.as_mut() {
            slot.sync = Some(SyncSlot {
                dialog: dialog.clone(),
                window: model,
                comparison,
                previews: HashMap::new(),
                restore,
                _events: events,
            });
        }
        dialog.update(cx, |dialog, cx| dialog.store_changed(cx));
        cx.notify();
    }

    /// The Sync with Deployed dialog, while it is open.
    pub fn sync_dialog(&self) -> Option<Entity<SyncDialog>> {
        self.deployment.slot.as_ref()?.sync.as_ref().map(|sync| sync.dialog.clone())
    }

    /// The SSH Password dialog, while it is open.
    pub fn ssh_secret_dialog(&self) -> Option<Entity<SecretDialog>> {
        self.deployment.slot.as_ref()?.secret.as_ref().map(|secret| secret.dialog.clone())
    }

    /// `f` on the open comparison's snapshots with the UI language; `None` once it closed.
    fn with_comparison<R>(
        &mut self,
        cx: &mut App,
        f: impl FnOnce(&mut DeploymentComparison, &mut I18n) -> R,
    ) -> Option<R> {
        let comparison = &mut self.deployment.slot.as_mut()?.sync.as_mut()?.comparison;
        Some(f(comparison, &mut cx.default_global::<Localization>().0))
    }

    /// Carries out what the store asks for.
    fn carry_out_sync(&mut self, effects: Vec<SyncEffect>, window: &mut Window, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                SyncEffect::Compare { path, server, request } => {
                    let ssh = self.deployment_server(&server, cx).map(|server| server_ssh(&server).to_owned());
                    cx.spawn_in(window, async move |this, cx| {
                        let entries: Result<Vec<SyncEntry>, IdeError> =
                            with_ssh(&this, cx, ssh.as_deref().unwrap_or_default(), move |host, label| {
                                let (path, server) = (IpcPath::from(path.as_str()), Some(server.clone()));
                                async move {
                                    bridge(
                                        ide_host::ops::deploy::deploy_compare(&label, &host, path, server, request)
                                            .await?,
                                    )
                                }
                            })
                            .await;
                        let _ = this.update_in(cx, |workbench, window, cx| {
                            workbench.sync_step(window, cx, |store, i18n| store.compared(request, entries, i18n));
                        });
                    })
                    .detach();
                }
                SyncEffect::SaveDocuments { paths, request } => {
                    let saved = self.save_open_documents(&paths, cx);
                    cx.spawn_in(window, async move |this, cx| {
                        let result = saved.await;
                        let _ = this.update_in(cx, |workbench, window, cx| {
                            workbench.sync_step(window, cx, |store, i18n| store.documents_saved(request, result, i18n));
                        });
                    })
                    .detach();
                }
                SyncEffect::Sync { steps, server, request } => {
                    let ssh = self.deployment_server(&server, cx).map(|server| server_ssh(&server).to_owned());
                    cx.spawn_in(window, async move |this, cx| {
                        let report: Result<TransferReport, IdeError> =
                            with_ssh(&this, cx, ssh.as_deref().unwrap_or_default(), move |host, label| {
                                let (steps, server) = (steps.clone(), Some(server.clone()));
                                async move {
                                    let steps = bridge::<Vec<SyncStep>, Vec<_>>(steps)?;
                                    bridge(
                                        ide_host::ops::deploy::deploy_sync(&label, &host, steps, server, request)
                                            .await?,
                                    )
                                }
                            })
                            .await;
                        let _ = this.update_in(cx, |workbench, window, cx| {
                            workbench.sync_step(window, cx, |store, i18n| store.synced(request, report, i18n));
                        });
                    })
                    .detach();
                }
                SyncEffect::Cancel(request) => {
                    if let Some(backend) = cx.try_global::<Backend>() {
                        backend.host.cancel_request(request);
                    }
                }
                SyncEffect::Notify(notice) => self.notify_deployment(notice, cx),
                SyncEffect::Finished(report) => {
                    if let Some(dialog) = self.sync_dialog() {
                        dialog.update(cx, |dialog, cx| dialog.synchronized(Some(&report), cx));
                    }
                }
            }
        }
        self.close_finished_comparison(window, cx);
    }

    /// Hands an answer to the store and carries out what it asks for next.
    fn sync_step(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        step: impl FnOnce(&mut DeploymentStore, &mut I18n) -> Vec<SyncEffect>,
    ) {
        let Some(store) = self.deployment.slot.as_ref().map(|slot| slot.store.clone()) else { return };
        let effects = store.update(cx, |store, cx| {
            let effects = step(store, &mut cx.default_global::<Localization>().0);
            cx.notify();
            effects
        });
        self.carry_out_sync(effects, window, cx);
    }

    /// The comparison ended by itself (Synchronize All, a cancelled comparison): its dialog goes.
    fn close_finished_comparison(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.deployment.slot.as_mut() else { return };
        let has_sync = slot.store.read(cx).sync.is_some();
        if !slot.sync.as_ref().is_some_and(|sync| sync.window.should_close(true, has_sync)) {
            return;
        }
        if let Some(SyncSlot { restore: Some(restore), .. }) = slot.sync.take() {
            window.focus(&restore, cx);
        }
        cx.notify();
    }

    /// Closing the comparison (Cancel, Escape): its local edits are saved as the window's close
    /// does, the comparison and a prompt waiting on it are cancelled, and the dialog goes.
    fn close_comparison(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.deployment.slot.as_mut() else { return };
        let store = slot.store.clone();
        let Some(sync) = slot.sync.as_mut() else { return };
        let i18n = &cx.default_global::<Localization>().0;
        // The comparison's local side is shown, not edited: nothing of it is unsaved.
        let step = match sync.window.close_requested(false, false, i18n) {
            crate::features_b::deployment::window::CloseStep::Save => sync.window.saved(Ok(()), i18n),
            other => other,
        };
        if step != crate::features_b::deployment::window::CloseStep::Teardown {
            return;
        }
        let _ = sync.window.torn_down(Ok(()), i18n);
        let restore = slot.sync.take().and_then(|sync| sync.restore);
        let effects = store.update(cx, |store, cx| {
            let effects = store.close_sync();
            cx.notify();
            effects
        });
        self.answer_ssh_secret(None, window, cx);
        self.carry_out_sync(effects, window, cx);
        if let Some(restore) = restore {
            window.focus(&restore, cx);
        }
        cx.notify();
    }

    /// Saves the open editors of `paths`; the first failure's message, if any.
    fn save_open_documents(
        &self,
        paths: &[String],
        cx: &App,
    ) -> impl Future<Output = Result<(), String>> + 'static + use<> {
        let saving: Vec<_> = self
            .editors()
            .iter()
            .filter(|editor| paths.contains(&editor.read(cx).path().to_string_lossy().into_owned()))
            .map(|editor| editor.read(cx).save(cx))
            .collect();
        async move {
            let mut failure = None;
            for save in saving {
                match save.await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => failure = failure.or(Some(error.message)),
                    Err(error) => failure = failure.or(Some(error.to_string())),
                }
            }
            failure.map_or(Ok(()), Err)
        }
    }

    /// The paths of the files open in editors.
    fn open_document_paths(&self, cx: &App) -> Vec<String> {
        self.editors().iter().map(|editor| editor.read(cx).path().to_string_lossy().into_owned()).collect()
    }

    fn sync_dialog_event(
        &mut self,
        dialog: &Entity<SyncDialog>,
        event: &SyncDialogEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            SyncDialogEvent::BeforeSynchronize { steps } => {
                let root = self.deployment.slot.as_ref().map(|slot| slot.root.clone()).unwrap_or_default();
                let paths: Vec<String> = steps
                    .iter()
                    .filter(|step| step.action == crate::features_b::deployment::types::SyncAction::Upload)
                    .map(|step| format!("{}/{}", root.trim_end_matches('/'), step.path))
                    .collect();
                let saved = self.save_open_documents(&paths, cx);
                let dialog = dialog.downgrade();
                cx.spawn_in(window, async move |_, cx| {
                    let result = saved.await;
                    if dialog.update(cx, |dialog, cx| dialog.prepared(result, cx)).is_err() {
                        tracing::debug!("the comparison closed before its documents were saved");
                    }
                })
                .detach();
            }
            SyncDialogEvent::Run { steps, mode } => {
                let deployment = self.deployment_settings(cx);
                let open = self.open_document_paths(cx);
                let root = self.deployment.slot.as_ref().map(|slot| slot.root.clone());
                let request = Backend::next_request_id();
                self.sync_step(window, cx, |store, _| {
                    store.run_sync(steps, mode, &deployment, root.as_deref(), &open, request)
                });
            }
            SyncDialogEvent::Refresh { server, paths } => {
                let Some(server) = self.deployment_server(&server, cx) else { return };
                let root = self.deployment.slot.as_ref().map(|slot| slot.root.clone()).unwrap_or_default();
                if let Some(sync) = self.deployment.slot.as_mut().and_then(|slot| slot.sync.as_mut()) {
                    sync.comparison = DeploymentComparison::new(&server, &root);
                    sync.previews.clear();
                }
                cx.spawn_in(window, async move |this, cx| {
                    let Some(coverage) = coverage(&this, cx, &paths).await else { return };
                    let request = Backend::next_request_id();
                    let _ = this.update_in(cx, |workbench, window, cx| {
                        workbench.sync_step(window, cx, |store, i18n| {
                            store.open_sync(&server, paths, &coverage, request, i18n)
                        });
                    });
                })
                .detach();
            }
            SyncDialogEvent::Scan(scan) => self.run_scan(scan, window, cx),
            SyncDialogEvent::Preview(requests) => self.run_preview(requests, window, cx),
            SyncDialogEvent::ShowMenu { menu, position } => {
                let items = dialog.read(cx).menu_items(&menu, cx);
                let target = dialog.clone();
                self.show_menu(
                    &items,
                    position,
                    move |_, command, _, cx| target.update(cx, |dialog, cx| dialog.run_command(command, cx)),
                    window,
                    cx,
                );
            }
            SyncDialogEvent::Close => self.close_comparison(window, cx),
        }
    }

    /// Compares the bytes of a row for the content scan.
    fn run_scan(&mut self, scan: ScanRequest, window: &mut Window, cx: &mut Context<Self>) {
        let ScanRequest { ticket, entry } = scan;
        let Some((op, step)) = self.with_comparison(cx, |comparison, i18n| comparison.equal(entry, i18n)) else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let answer = match drive(&this, cx, op, step, None).await {
                ComparisonStep::Equal(equal) => Ok(equal),
                ComparisonStep::Failed(message) => Err(message),
                _ => return,
            };
            let _ = this.update(cx, |workbench, cx| {
                if let Some(dialog) = workbench.sync_dialog() {
                    dialog.update(cx, |dialog, cx| dialog.scan_answered(ticket, answer, cx));
                }
            });
        })
        .detach();
    }

    /// Loads the preview of a row, and marks the load it replaces as aborted.
    fn run_preview(&mut self, requests: PreviewRequests, window: &mut Window, cx: &mut Context<Self>) {
        let PreviewRequests { abort, load } = requests;
        let Some(sync) = self.deployment.slot.as_mut().and_then(|slot| slot.sync.as_mut()) else { return };
        if let Some(aborted) = abort.and_then(|ticket| sync.previews.remove(&ticket)) {
            aborted.set(true);
        }
        let Some(PreviewLoad { ticket, entry }) = load else { return };
        let aborted = Rc::new(Cell::new(false));
        sync.previews.insert(ticket, Rc::clone(&aborted));
        let Some((op, step)) = self.with_comparison(cx, |comparison, i18n| comparison.preview(entry, i18n)) else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let answer = match drive(&this, cx, op, step, Some(Rc::clone(&aborted))).await {
                ComparisonStep::Preview(preview) => {
                    let local = match &preview {
                        DeploymentPreview::Text(diff) => match &diff.left {
                            DiffSide::File { path, .. } => Some(path.clone()),
                            DiffSide::Text { .. } => None,
                        },
                        DeploymentPreview::Binary | DeploymentPreview::TooLarge => None,
                    };
                    let local_text = match local {
                        Some(path) => deploy_call(&this, cx, move |host, label| async move {
                            ide_host::ops::fs::read_text(&host, &label, PathBuf::from(path)).await
                        })
                        .await
                        .map_err(|error| error.message),
                        None => Ok(String::new()),
                    };
                    local_text.map(|local_text| LoadedPreview { preview, local_text })
                }
                ComparisonStep::Failed(message) => Err(message),
                _ => return,
            };
            if aborted.get() {
                return;
            }
            let _ = this.update(cx, |workbench, cx| {
                if let Some(sync) = workbench.deployment.slot.as_mut().and_then(|slot| slot.sync.as_mut()) {
                    sync.previews.remove(&ticket);
                }
                if let Some(dialog) = workbench.sync_dialog() {
                    dialog.update(cx, |dialog, cx| dialog.preview_loaded(ticket, answer, cx));
                }
            });
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // SSH Password

    /// Asks for the password or passphrase of SSH configuration `ssh`; a prompt already open is
    /// answered with nothing.
    fn ask_ssh_secret(
        &mut self,
        ssh: &str,
        message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<Option<(SecretText, bool)>> {
        let (sender, answer) = oneshot::channel();
        self.answer_ssh_secret(None, window, cx);
        let Some(slot) = self.deployment_slot(cx) else { return answer };
        let store = slot.store.clone();
        store.update(cx, |store, _| store.prompt_secret(ssh, message));
        let message = message.to_owned();
        let dialog = cx.new(|cx| SecretDialog::new(message, window, cx));
        let events = cx.subscribe_in(&dialog, window, |workbench, _, event: &SecretDialogEvent, window, cx| {
            let answer = match event.clone() {
                SecretDialogEvent::Answered { secret, save } => Some((secret, save)),
                SecretDialogEvent::Cancelled => None,
            };
            workbench.answer_ssh_secret(answer, window, cx);
        });
        let restore = window.focused(cx);
        window.focus(&dialog.focus_handle(cx), cx);
        if let Some(slot) = self.deployment.slot.as_mut() {
            slot.secret = Some(SecretSlot { dialog, answer: Some(sender), restore, _events: events });
        }
        cx.notify();
        answer
    }

    /// Closes the SSH Password dialog with `answer` for the work waiting on it.
    fn answer_ssh_secret(&mut self, answer: Option<(SecretText, bool)>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.deployment.slot.as_mut() else { return };
        let Some(mut secret) = slot.secret.take() else { return };
        slot.store.update(cx, |store, _| store.close_secret());
        if let Some(sender) = secret.answer.take() {
            // The work waiting on it may have ended meanwhile.
            let _ = sender.send(answer);
        }
        if let Some(restore) = secret.restore {
            window.focus(&restore, cx);
        }
        cx.notify();
    }

    /// The Sync with Deployed dialog and, over it, the SSH Password dialog.
    pub(super) fn deployment_layer(&self, cx: &App) -> Option<AnyElement> {
        let slot = self.deployment.slot.as_ref()?;
        let sync = slot.sync.as_ref().map(|sync| {
            div()
                .id("deployment-sync-layer")
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .child(super::dialog_frame(sync.dialog.clone(), cx))
        });
        let secret = slot.secret.as_ref().map(|secret| {
            div()
                .id("deployment-secret-layer")
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .child(super::dialog_frame(secret.dialog.clone(), cx))
        });
        if sync.is_none() && secret.is_none() {
            return None;
        }
        let layer = div().absolute().inset_0().children(sync).children(secret);
        Some(gpui_kit::deferred(layer).with_priority(1).into_any_element())
    }

    // ---------------------------------------------------------------------------------------------
    // Remote Host

    /// The view of the Remote Host tool window, once it has been shown.
    pub fn remote_host_view(&self) -> Option<Entity<RemoteHostView>> {
        self.tool_window_view(REMOTE_HOST_WINDOW).and_then(|view| view.downcast::<RemoteHostView>().ok())
    }

    /// The tool window's panel; its tree is asked for once it is in place
    /// ([`Workbench::sync_remote_host`]).
    pub(super) fn mount_remote_host(&mut self, window: &mut Window, cx: &mut Context<Self>) -> super::Panel {
        let view = cx.new(RemoteHostView::new);
        let events = cx.subscribe_in(&view, window, Self::remote_host_event);
        if let Some(slot) = self.deployment_slot(cx) {
            slot.host_started = false;
            slot.host_window = Some(window.window_handle());
        }
        super::Panel::new(view, Some(events))
    }

    /// On the tool window's first showing, resolves its server's root and lists it.
    pub(super) fn sync_remote_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.remote_host_view().is_none() {
            return;
        }
        let Some(slot) = self.deployment_slot(cx) else { return };
        if slot.host_started {
            return;
        }
        slot.host_started = true;
        self.ensure_host_root(false, window, cx);
    }

    /// The settings changed: the tool window shows the servers as they are now, and a server whose
    /// connection, root or mappings changed is listed again.
    pub(super) fn deployment_settings_changed(&mut self, cx: &mut Context<Self>) {
        let slot = self.deployment.slot.as_ref().filter(|slot| slot.host_started);
        let Some(handle) = slot.and_then(|slot| slot.host_window) else { return };
        cx.spawn(async move |this, cx| {
            // The window is gone: there is nothing left to show.
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |workbench, cx| workbench.ensure_host_root(false, window, cx));
            });
        })
        .detach();
    }

    /// The server the tool window shows: the one chosen, else the default one, else the first.
    fn host_server(&mut self, cx: &mut App) -> Option<DeploymentServer> {
        let settings = self.deployment_settings(cx);
        let chosen = self.deployment_store(cx)?.read(cx).remote_server.clone();
        chosen
            .and_then(|id| settings.servers.iter().find(|server| server.id == id).cloned())
            .or_else(|| resolve_server(&settings, None).cloned())
            .or_else(|| settings.servers.first().cloned())
    }

    /// Gives the tool window the servers and the shown server's tree as they are now.
    fn push_host_state(&mut self, cx: &mut App) {
        let Some(view) = self.remote_host_view() else { return };
        let settings = self.deployment_settings(cx);
        let server = self.host_server(cx);
        let tree = match (&server, self.deployment_store(cx)) {
            (Some(server), Some(store)) => store.read(cx).host.tree(&server.id).cloned(),
            _ => None,
        };
        let state = RemoteHostState {
            servers: settings
                .servers
                .iter()
                .map(|server| HostServer { id: server.id.clone(), label: server_label(server).to_owned() })
                .collect(),
            server: server.map(|server| server.id),
            tree,
        };
        view.update(cx, |view, cx| view.set_state(state, cx));
    }

    /// `f` on the Remote Host trees with the UI language.
    fn with_host_model<R>(&mut self, cx: &mut App, f: impl FnOnce(&mut RemoteHost, &mut I18n) -> R) -> Option<R> {
        let store = self.deployment_store(cx)?;
        Some(store.update(cx, |store, cx| {
            let result = f(&mut store.host, &mut cx.default_global::<Localization>().0);
            cx.notify();
            result
        }))
    }

    fn project_root_text(&self, cx: &App) -> Option<String> {
        self.project_root(cx).map(|root| root.to_string_lossy().into_owned())
    }

    fn ensure_host_root(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(server), root) = (self.host_server(cx), self.project_root_text(cx)) else {
            self.push_host_state(cx);
            return;
        };
        let request = self.with_host_model(cx, |host, _| host.ensure_root(&server, root.as_deref(), force)).flatten();
        self.push_host_state(cx);
        self.carry_out_host(request.into_iter().collect(), window, cx);
    }

    /// Asks the backend what the Remote Host trees asked for, and hands back the answers.
    fn carry_out_host(&mut self, requests: Vec<HostRequest>, window: &mut Window, cx: &mut Context<Self>) {
        for request in requests {
            let id = match &request {
                HostRequest::ServerRoot { server, .. } | HostRequest::List { server, .. } => server.clone(),
            };
            let Some(server) = self.deployment_server(&id, cx) else { continue };
            let ssh = server_ssh(&server).to_owned();
            cx.spawn_in(window, async move |this, cx| {
                let effects = match request {
                    HostRequest::ServerRoot { server: id, generation } => {
                        let root: Result<DeploymentRoot, IdeError> = with_ssh(&this, cx, &ssh, move |host, label| {
                            let id = id.clone();
                            async move { bridge(ide_host::ops::deploy::deploy_server_root(&label, &host, id).await?) }
                        })
                        .await;
                        this.update(cx, |workbench, cx| {
                            workbench
                                .with_host_model(cx, |host, i18n| host.root_resolved(&server, generation, root, i18n))
                        })
                    }
                    HostRequest::List { connection, path, generation, .. } => {
                        let listed = path.clone();
                        let entries: Result<Vec<RemoteEntry>, IdeError> = with_ssh(&this, cx, &ssh, move |host, _| {
                            let (connection, path) = (connection.clone(), path.clone());
                            async move { ide_host::ops::deploy::remote_list(&host, connection, path).await }
                        })
                        .await;
                        this.update(cx, |workbench, cx| {
                            workbench.with_host_model(cx, |host, i18n| {
                                host.listed(&server, generation, &listed, entries, i18n)
                            })
                        })
                    }
                };
                let _ = this.update_in(cx, |workbench, window, cx| {
                    if let Ok(Some(effects)) = effects {
                        workbench.host_effects(effects, window, cx);
                    }
                    workbench.push_host_state(cx);
                });
            })
            .detach();
        }
    }

    fn host_effects(&mut self, effects: Vec<HostEffect>, window: &mut Window, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                HostEffect::Request(request) => self.carry_out_host(vec![request], window, cx),
                HostEffect::Notify(notice) => self.notify_deployment(notice, cx),
                HostEffect::Download { server, paths } => {
                    let Some(server) = self.deployment_server(&server, cx) else { continue };
                    cx.spawn_in(window, async move |this, cx| {
                        transfer(&this, cx, TransferDirection::Download, server, paths).await;
                    })
                    .detach();
                }
            }
        }
    }

    /// A failure of the Remote Host window's work on a server's files.
    fn notify_host_failure(&mut self, title: String, error: &IdeError, cx: &mut Context<Self>) {
        if rules::is_cancelled(error) {
            return;
        }
        let body = cx.default_global::<Localization>().0.tx_template(&error.message);
        let notice = Notice {
            level: NotificationLevel::Error,
            title,
            body: Some(NoticeBody::Text(body)),
            key: Some("deployment.remoteHost".to_owned()),
            actions: Vec::new(),
        };
        self.notify_deployment(notice, cx);
    }

    fn remote_host_event(
        &mut self,
        view: &Entity<RemoteHostView>,
        event: &RemoteHostEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            RemoteHostEvent::ShowServerMenu { position } => {
                let servers = view.read(cx).state().servers.clone();
                let items: Vec<MenuItem<String>> =
                    servers.into_iter().map(|server| MenuItem::item(server.label, server.id)).collect();
                self.show_menu(
                    &items,
                    position,
                    |workbench, id: String, window, cx| {
                        if let Some(store) = workbench.deployment_store(cx) {
                            store.update(cx, |store, _| store.remote_server = Some(id));
                        }
                        workbench.ensure_host_root(false, window, cx);
                    },
                    window,
                    cx,
                );
            }
            RemoteHostEvent::ShowMenu { position } => {
                let items = view.read(cx).menu_items();
                self.show_menu(&items, position, Self::run_host_command, window, cx);
            }
            RemoteHostEvent::Refresh => self.run_host_command(HostCommand::RefreshAll, window, cx),
            RemoteHostEvent::CollapseAll => {
                let (Some(server), root) = (self.host_server(cx), self.project_root_text(cx)) else { return };
                let expanded: Vec<String> = self
                    .deployment_store(cx)
                    .and_then(|store| {
                        store.read(cx).host.tree(&server.id).map(|tree| tree.expanded.iter().cloned().collect())
                    })
                    .unwrap_or_default();
                self.with_host_model(cx, |host, _| {
                    for path in &expanded {
                        host.set_expanded(&server, root.as_deref(), path, false);
                    }
                });
                self.push_host_state(cx);
            }
            RemoteHostEvent::Retry => self.ensure_host_root(true, window, cx),
            RemoteHostEvent::RunAction(id) => {
                window.defer(cx, move |window, cx| {
                    if !execute_action_from(ActionSource::Other, id, None, window, cx) {
                        tracing::info!("{id} is not available in this window");
                    }
                });
            }
            RemoteHostEvent::Expand { path, expanded } => {
                let (Some(server), root) = (self.host_server(cx), self.project_root_text(cx)) else { return };
                let request = self
                    .with_host_model(cx, |host, _| host.set_expanded(&server, root.as_deref(), &path, expanded))
                    .flatten();
                self.push_host_state(cx);
                self.carry_out_host(request.into_iter().collect(), window, cx);
            }
            RemoteHostEvent::Open(entry) => self.run_host_command(HostCommand::Open(entry), window, cx),
        }
    }

    /// Lists `folder` of the shown server again.
    fn reload_host_folder(&mut self, folder: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(server), root) = (self.host_server(cx), self.project_root_text(cx)) else { return };
        let request = self.with_host_model(cx, |host, _| host.load_folder(&server, root.as_deref(), folder));
        self.carry_out_host(request.into_iter().collect(), window, cx);
    }

    /// Asks for a name: New Folder and Rename. A name with "/" is refused.
    fn ask_remote_name(
        &mut self,
        title: &str,
        label: &str,
        initial: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<crate::vcs::prompt_dialogs::DialogAnswer> {
        let request = PromptRequest {
            title: title.to_owned(),
            label: Some(label.to_owned()),
            initial: initial.to_owned(),
            ..PromptRequest::default()
        };
        let rule = FieldRule::NonEmpty("Enter a name".to_owned());
        self.show_prompt_dialog(move |window, cx| PromptDialog::prompt(request, rule, window, cx), window, cx)
    }

    /// Carries out a row of the Remote Host menu (and Refresh, Open of the window itself).
    fn run_host_command(&mut self, command: HostCommand, window: &mut Window, cx: &mut Context<Self>) {
        let Some(server) = self.host_server(cx) else { return };
        let connection = deployment_connection(&server);
        let ssh = server_ssh(&server).to_owned();
        match command {
            HostCommand::Open(entry) => {
                cx.spawn_in(window, async move |this, cx| {
                    let path = entry.path.clone();
                    let opened: Result<PathBuf, IdeError> =
                        with_ssh(&this, cx, &ssh, move |host, _| {
                            let (connection, path) = (connection.clone(), path.clone());
                            async move {
                                Ok(ide_host::ops::deploy::remote_open(&host, connection, path).await?.to_path_buf())
                            }
                        })
                        .await;
                    let _ = this.update_in(cx, |workbench, window, cx| match opened {
                        Ok(local) => workbench.open_project_file(local, window, cx),
                        Err(error) => {
                            let title = cx
                                .default_global::<Localization>()
                                .0
                                .tx_built(&["Cannot open ", ""], &[Param::from(entry.name.as_str())]);
                            workbench.notify_host_failure(title, &error, cx);
                        }
                    });
                })
                .detach();
            }
            HostCommand::Download(entries) => {
                let root = self.project_root_text(cx);
                let request =
                    self.with_host_model(cx, |host, _| host.download_to_project(&server, root.as_deref(), entries));
                self.carry_out_host(request.flatten().into_iter().collect(), window, cx);
            }
            HostCommand::NewFolder(parent) => {
                let answer = self.ask_remote_name("New Folder", "Name:", "", window, cx);
                cx.spawn_in(window, async move |this, cx| {
                    let Some(result) = answer.await.ok().and_then(|answer| answer.prompt_result()) else { return };
                    if result.value.contains('/') {
                        let _ = this.update(cx, |workbench, cx| {
                            let error = IdeError::invalid("A name cannot contain “/”");
                            let title = cx.default_global::<Localization>().0.tx("Cannot create folder");
                            workbench.notify_host_failure(title, &error, cx);
                        });
                        return;
                    }
                    let target = join_remote(&parent, &result.value);
                    let made: Result<(), IdeError> = with_ssh(&this, cx, &ssh, move |host, _| {
                        let (connection, target) = (connection.clone(), target.clone());
                        async move { ide_host::ops::deploy::remote_mkdir(&host, connection, target).await }
                    })
                    .await;
                    let _ = this.update_in(cx, |workbench, window, cx| match made {
                        Ok(()) => {
                            let (Some(server), root) = (workbench.host_server(cx), workbench.project_root_text(cx))
                            else {
                                return;
                            };
                            workbench.with_host_model(cx, |host, _| {
                                host.set_expanded(&server, root.as_deref(), &parent, true)
                            });
                            workbench.reload_host_folder(&parent, window, cx);
                        }
                        Err(error) => {
                            let title = cx.default_global::<Localization>().0.tx("Cannot create folder");
                            workbench.notify_host_failure(title, &error, cx);
                        }
                    });
                })
                .detach();
            }
            HostCommand::Rename(entry) => {
                let answer = self.ask_remote_name("Rename", "New name:", &entry.name, window, cx);
                cx.spawn_in(window, async move |this, cx| {
                    let Some(result) = answer.await.ok().and_then(|answer| answer.prompt_result()) else { return };
                    if result.value == entry.name {
                        return;
                    }
                    let title = with_i18n(&this, cx, |i18n| {
                        i18n.tx_built(&["Cannot rename ", ""], &[Param::from(entry.name.as_str())])
                    })
                    .unwrap_or_default();
                    if result.value.contains('/') {
                        let _ = this.update(cx, |workbench, cx| {
                            workbench.notify_host_failure(title, &IdeError::invalid("A name cannot contain “/”"), cx);
                        });
                        return;
                    }
                    let parent = remote_parent(&entry.path).to_owned();
                    let (from, to) = (entry.path.clone(), join_remote(&parent, &result.value));
                    let renamed: Result<(), IdeError> = with_ssh(&this, cx, &ssh, move |host, _| {
                        let (connection, from, to) = (connection.clone(), from.clone(), to.clone());
                        async move { ide_host::ops::deploy::remote_rename(&host, connection, from, to).await }
                    })
                    .await;
                    let _ = this.update_in(cx, |workbench, window, cx| match renamed {
                        Ok(()) => workbench.reload_host_folder(&parent, window, cx),
                        Err(error) => workbench.notify_host_failure(title, &error, cx),
                    });
                })
                .detach();
            }
            HostCommand::Delete(entries) => {
                if entries.is_empty() {
                    return;
                }
                let message = {
                    let i18n = &mut cx.default_global::<Localization>().0;
                    match entries.as_slice() {
                        [only] if only.is_dir => i18n
                            .tx_built(&["Delete folder “", "” from the server?"], &[Param::from(only.path.as_str())]),
                        [only] => {
                            i18n.tx_built(&["Delete file “", "” from the server?"], &[Param::from(only.path.as_str())])
                        }
                        _ => i18n.tx_built(&["Delete ", " items from the server?"], &[Param::from(entries.len())]),
                    }
                };
                let request = ConfirmRequest::danger("Delete", message, "Delete");
                let answer =
                    self.show_prompt_dialog(move |window, cx| PromptDialog::confirm(request, window, cx), window, cx);
                cx.spawn_in(window, async move |this, cx| {
                    if !answer.await.is_ok_and(|answer| answer.confirmed()) {
                        return;
                    }
                    let mut parents: Vec<String> = Vec::new();
                    for entry in entries {
                        let path = entry.path.clone();
                        let connection = connection.clone();
                        let deleted: Result<(), IdeError> = with_ssh(&this, cx, &ssh, move |host, _| {
                            let (connection, path) = (connection.clone(), path.clone());
                            async move { ide_host::ops::deploy::remote_delete(&host, connection, path).await }
                        })
                        .await;
                        match deleted {
                            Ok(()) => {
                                let parent = remote_parent(&entry.path).to_owned();
                                if !parents.contains(&parent) {
                                    parents.push(parent);
                                }
                            }
                            Err(error) => {
                                let _ = this.update(cx, |workbench, cx| {
                                    let title = cx
                                        .default_global::<Localization>()
                                        .0
                                        .tx_built(&["Cannot delete ", ""], &[Param::from(entry.name.as_str())]);
                                    workbench.notify_host_failure(title, &error, cx);
                                });
                            }
                        }
                    }
                    let _ = this.update_in(cx, |workbench, window, cx| {
                        for parent in &parents {
                            workbench.reload_host_folder(parent, window, cx);
                        }
                    });
                })
                .detach();
            }
            HostCommand::RefreshFolder(folder) => self.reload_host_folder(&folder, window, cx),
            HostCommand::RefreshAll => {
                let root = self.project_root_text(cx);
                let requests = self.with_host_model(cx, |host, _| host.refresh_all(&server, root.as_deref()));
                self.push_host_state(cx);
                self.carry_out_host(requests.unwrap_or_default(), window, cx);
            }
            HostCommand::CopyPath(path) => cx.write_to_clipboard(ClipboardItem::new_string(path)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{notification_titles, run_action, start, trusted_project, wait_long};
    use super::*;
    use crate::features_b::deployment::model::SyncChoice;
    use crate::features_b::deployment::store::SyncRunMode;
    use crate::ui::popup_menu::PopupMenu;
    use gpui_kit::{Modifiers, MouseButton, TestAppContext, VisualTestContext};
    use std::path::Path;

    /// A project with `src/a.php` and `README.md`, and a folder that stands in for a server
    /// mounted on this machine: the server `local1` ("Mounted", the default one) maps the
    /// project's `src` onto that folder, its web root at `http://localhost:8080`. With `second`,
    /// another server `local2` ("Backup") on a second folder maps the same.
    struct Fixture<'a> {
        _home: tempfile::TempDir,
        _project: tempfile::TempDir,
        _servers: Vec<tempfile::TempDir>,
        root: PathBuf,
        server: PathBuf,
        backup: Option<PathBuf>,
        workbench: Entity<Workbench>,
        cx: &'a mut VisualTestContext,
    }

    fn server_json(id: &str, name: &str, root: &Path) -> Value {
        json!({
            "id": id,
            "name": name,
            "kind": "local",
            "rootPath": root.to_string_lossy(),
            "webUrl": "http://localhost:8080",
            "mappings": [{ "local": "src", "deployment": "/", "web": "app" }],
        })
    }

    fn fixture(cx: &mut TestAppContext, second: bool) -> Fixture<'_> {
        let home = start(cx);
        let (project, root, workbench, cx) =
            trusted_project(cx, &[("src/a.php", "<?php echo 1;\n"), ("README.md", "# p\n")]);
        let mut folders = Vec::new();
        let mut servers = Vec::new();
        for _ in 0..if second { 2 } else { 1 } {
            let folder = tempfile::tempdir().expect("a server folder");
            folders.push(folder.path().canonicalize().expect("the server folder exists"));
            servers.push(folder);
        }
        let mut entries = vec![server_json("local1", "Mounted", &folders[0])];
        if second {
            entries.push(server_json("local2", "Backup", &folders[1]));
        }
        cx.update(|_, cx| {
            settings::set("w1", SettingsScope::Project, "deployment.servers", &Value::Array(entries), cx)
                .expect("the servers are stored");
            settings::set("w1", SettingsScope::Project, "deployment.defaultServer", &json!("local1"), cx)
                .expect("the default server is stored");
        });
        cx.run_until_parked();
        Fixture {
            _home: home,
            _project: project,
            _servers: servers,
            server: folders[0].clone(),
            backup: folders.get(1).cloned(),
            root,
            workbench,
            cx,
        }
    }

    impl Fixture<'_> {
        fn path(&self, rel: &str) -> String {
            self.root.join(rel).to_string_lossy().into_owned()
        }

        fn notice(&mut self, title: &str) -> super::super::notifications::IdeNotification {
            let title = title.to_owned();
            wait_long(self.cx, &format!("the notice {title}"), |cx| {
                notification_titles(&self.workbench, cx).contains(&title)
            });
            self.workbench
                .read_with(self.cx, |w, _| w.notifications().items().iter().find(|n| n.title == title).cloned())
                .expect("the notice is raised")
        }
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{} is read: {error}", path.display()))
    }

    /// Chooses the row `label` of the open menu of commands `C` with the keyboard.
    fn choose_in_menu<C: Clone + 'static>(workbench: &Entity<Workbench>, label: &str, cx: &mut VisualTestContext) {
        wait_long(cx, "the menu", |cx| workbench.read_with(cx, |w, _| w.open_menu::<C>().is_some()));
        let menu: Entity<PopupMenu<C>> = workbench.read_with(cx, |w, _| w.open_menu::<C>()).expect("a menu is open");
        let selected = format!("> {label}");
        let is_selected = |cx: &mut VisualTestContext| {
            menu.read_with(cx, |menu, _| menu.lines().first().is_some_and(|rows| rows.contains(&selected)))
        };
        for _ in 0..30 {
            if is_selected(cx) {
                break;
            }
            cx.simulate_keystrokes("down");
        }
        assert!(is_selected(cx), "{label} in {:?}", menu.read_with(cx, |menu, _| menu.lines()));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
    }

    fn menu_labels<C: Clone + 'static>(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
        let menu: Entity<PopupMenu<C>> = workbench.read_with(cx, |w, _| w.open_menu::<C>()).expect("a menu is open");
        let lines = menu.read_with(cx, |menu, _| menu.lines());
        lines[0].iter().map(|line| line.trim_start_matches("> ").trim().to_owned()).collect()
    }

    fn press_ok(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
        wait_long(cx, "the dialog", |cx| workbench.read_with(cx, |w, _| w.vcs_prompt().is_some()));
        super::super::tests::click_on(cx, "prompt-button-ok");
    }

    /// Upload to Default Server copies the editor's file to its mapped place on the server and
    /// says how many files went.
    #[gpui_kit::test]
    async fn upload_to_default_server_copies_the_file_to_the_server_and_reports_it(cx: &mut TestAppContext) {
        let mut f = fixture(cx, false);
        let file = f.path("src/a.php");
        run_action(f.cx, "deployment.uploadToDefault", Some(json!([file])));
        let notice = f.notice("Uploaded 1 files to Mounted");
        assert_eq!(notice.level, NotificationLevel::Info);
        assert_eq!(read(&f.server.join("a.php")), "<?php echo 1;\n");
    }

    /// Upload to… with one server that does not deploy the file says why, naming the server and
    /// the path's reason; with two servers, nothing covered says so for every server, with
    /// Configure….
    #[gpui_kit::test]
    async fn upload_to_explains_the_rejected_paths_and_links_settings(cx: &mut TestAppContext) {
        let mut f = fixture(cx, true);
        let readme = f.path("README.md");
        run_action(f.cx, "deployment.uploadTo", Some(json!([readme])));
        let notice = f.notice("No deployment server covers the selected paths");
        let actions: Vec<(&str, &str)> =
            notice.actions.iter().map(|action| (action.label.as_str(), action.command.as_str())).collect();
        assert_eq!(actions, [("Configure…", CONFIGURE_ACTION)]);
        let body = notice.body.unwrap_or_default();
        assert!(body.contains("README.md: Mounted: Not covered by this server's mappings"), "{body}");
        assert!(body.contains("README.md: Backup: Not covered by this server's mappings"), "{body}");
        assert!(std::fs::read_dir(&f.server).expect("the server folder").next().is_none(), "nothing is uploaded");
    }

    /// Download from… offers the servers that deploy the file, even one, and the one chosen
    /// replaces the local file with its copy.
    #[gpui_kit::test]
    async fn download_from_chooses_the_server_and_replaces_the_local_file(cx: &mut TestAppContext) {
        let mut f = fixture(cx, true);
        let backup = f.backup.clone().expect("a second server");
        std::fs::write(backup.join("a.php"), "<?php echo 2;\n").expect("the server's copy is written");
        let target = f.path("src/a.php");
        run_action(f.cx, "deployment.downloadFrom", Some(json!([target])));
        wait_long(f.cx, "the chooser", |cx| f.workbench.read_with(cx, |w, _| w.open_menu::<String>().is_some()));
        assert_eq!(menu_labels::<String>(&f.workbench, f.cx), ["Mounted", "Backup"]);
        choose_in_menu::<String>(&f.workbench, "Backup", f.cx);
        f.notice("Downloaded 1 files from Backup");
        assert_eq!(read(&f.root.join("src/a.php")), "<?php echo 2;\n");
    }

    /// Download from Default Server takes the default server's copy without asking.
    #[gpui_kit::test]
    async fn download_from_default_server_replaces_the_local_file_without_asking(cx: &mut TestAppContext) {
        let mut f = fixture(cx, true);
        std::fs::write(f.server.join("a.php"), "<?php echo 3;\n").expect("the server's copy is written");
        let target = f.path("src/a.php");
        run_action(f.cx, "deployment.downloadFromDefault", Some(json!([target])));
        f.notice("Downloaded 1 files from Mounted");
        assert!(f.workbench.read_with(f.cx, |w, _| w.open_menu::<String>().is_none()), "no chooser");
        assert_eq!(read(&f.root.join("src/a.php")), "<?php echo 3;\n");
    }

    /// Delete from Server asks first; Delete removes the remote copy and says so.
    #[gpui_kit::test]
    async fn delete_from_server_asks_and_deletes_the_remote_copy(cx: &mut TestAppContext) {
        let mut f = fixture(cx, false);
        std::fs::write(f.server.join("a.php"), "old\n").expect("the server's copy is written");
        let target = f.path("src/a.php");
        run_action(f.cx, "deployment.deleteFromServer", Some(json!([target])));
        wait_long(f.cx, "the question", |cx| f.workbench.read_with(cx, |w, _| w.vcs_prompt().is_some()));
        let message = f
            .workbench
            .read_with(f.cx, |w, _| w.vcs_prompt())
            .expect("the question")
            .read_with(f.cx, |d, _| d.message_text().map(str::to_owned));
        assert_eq!(message.as_deref(), Some("Delete the remote copy of src/a.php from Mounted?"));
        assert!(f.server.join("a.php").exists(), "nothing is deleted before the answer");
        press_ok(&f.workbench, f.cx);
        f.notice("Deleted 1 files from Mounted");
        assert!(!f.server.join("a.php").exists());
        assert!(f.root.join("src/a.php").exists(), "the local file stays");
    }

    /// Open in Browser (Deployed) opens the file's URL under the server's web root.
    #[gpui_kit::test]
    async fn open_in_browser_opens_the_deployed_url_of_the_file(cx: &mut TestAppContext) {
        let f = fixture(cx, false);
        let target = f.path("src/a.php");
        run_action(f.cx, "deployment.openInBrowser", Some(json!([target])));
        wait_long(f.cx, "the URL opens", |cx| cx.opened_url().is_some());
        assert_eq!(f.cx.opened_url().as_deref(), Some("http://localhost:8080/app/a.php"));
    }

    /// Automatic Upload shows off, turns on as "always" in the project's settings, and off again.
    #[gpui_kit::test]
    async fn automatic_upload_toggles_the_projects_setting(cx: &mut TestAppContext) {
        use ide_core::settings::AutoUpload;
        let f = fixture(cx, false);
        let toggled = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                cx.default_global::<ActionRegistry>()
                    .get("deployment.toggleAutoUpload")
                    .and_then(|action| action.is_toggled(window, cx))
            })
        };
        let stored =
            |cx: &mut VisualTestContext| cx.update(|_, cx| settings::effective("w1", cx).deployment.auto_upload);
        assert_eq!(toggled(f.cx), Some(false));
        run_action(f.cx, "deployment.toggleAutoUpload", None);
        assert_eq!(stored(f.cx), AutoUpload::Always);
        assert_eq!(toggled(f.cx), Some(true));
        run_action(f.cx, "deployment.toggleAutoUpload", None);
        assert_eq!(stored(f.cx), AutoUpload::Never);
        assert_eq!(toggled(f.cx), Some(false));
    }

    /// Deployment Configuration… opens Settings on the Deployment page.
    #[gpui_kit::test]
    async fn deployment_configuration_opens_the_deployment_settings_page(cx: &mut TestAppContext) {
        let f = fixture(cx, false);
        run_action(f.cx, CONFIGURE_ACTION, None);
        wait_long(f.cx, "the settings", |cx| f.workbench.read_with(cx, |w, _| w.settings_dialog().is_some()));
        let page = f
            .workbench
            .read_with(f.cx, |w, cx| w.settings_dialog().and_then(|dialog| dialog.read(cx).selected_page(cx)));
        assert_eq!(page.as_deref(), Some(SETTINGS_PAGE));
    }

    const ACTIONS: [&str; 11] = [
        "deployment.uploadToDefault",
        "deployment.uploadTo",
        "deployment.downloadFromDefault",
        "deployment.downloadFrom",
        "deployment.syncWith",
        "deployment.syncWithDefault",
        "deployment.deleteFromServer",
        "deployment.toggleAutoUpload",
        "deployment.browseRemoteHost",
        "deployment.openInBrowser",
        CONFIGURE_ACTION,
    ];

    fn enabled(id: &str, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| {
            let context = crate::platform::context::window_context(window, cx);
            let action = cx.global::<ActionRegistry>().get(id).unwrap_or_else(|| panic!("{id} is registered"));
            crate::platform::actions::is_action_enabled(&action.meta, &context)
        })
    }

    /// Every deployment action waits for a project (`projectOpen`): the Welcome window refuses
    /// each one, and a project window offers each one.
    #[gpui_kit::test]
    async fn the_deployment_actions_are_available_only_with_a_project_open(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (welcome, welcome_cx) = cx.add_window_view(|window, cx| Workbench::new("w0", window, cx));
        welcome_cx.update(|window, _| window.activate_window());
        welcome_cx.run_until_parked();
        assert!(welcome.read_with(welcome_cx, |w, _| w.project_tree().is_none()), "the Welcome window");
        assert!(!super::super::tests::context_is_on(welcome_cx, "projectOpen"));
        for id in ACTIONS {
            assert!(!enabled(id, welcome_cx), "{id} is not available without a project");
            let ran =
                welcome_cx.update(|window, cx| crate::platform::actions::execute_action_or_throw(id, None, window, cx));
            assert!(ran.is_err(), "the registry refuses {id}");
        }
        welcome_cx.run_until_parked();
        assert!(welcome.read_with(welcome_cx, |w, _| w.notifications().items().is_empty()), "nothing ran");

        let (_project, _, _, cx) = trusted_project(cx, &[("src/a.php", "<?php\n")]);
        assert!(super::super::tests::context_is_on(cx, "projectOpen"));
        for id in ACTIONS {
            assert!(enabled(id, cx), "{id} is available with a project");
        }
    }

    /// The comparison of the project's `src` with the server, once listed.
    fn open_sync_with_default(f: &mut Fixture<'_>) -> Entity<SyncDialog> {
        let src = f.path("src");
        run_action(f.cx, "deployment.syncWithDefault", Some(json!([src])));
        wait_long(f.cx, "the comparison", |cx| {
            f.workbench.read_with(cx, |w, cx| {
                w.sync_dialog()
                    .is_some_and(|dialog| dialog.read(cx).sync(cx).is_some_and(|sync| sync.entries.is_some()))
            })
        });
        f.workbench.read_with(f.cx, |w, _| w.sync_dialog()).expect("the comparison is open")
    }

    fn rows(dialog: &Entity<SyncDialog>, cx: &mut VisualTestContext) -> Vec<(String, SyncChoice)> {
        dialog.read_with(cx, |dialog, cx| dialog.rows(cx).into_iter().map(|row| (row.path, row.choice)).collect())
    }

    /// Sync with Deployed to Default Server lists the file only the project has and the file only
    /// the server has, each with the operation that brings the other side in line; the preview
    /// shows the server's text next to the local one; Synchronize All uploads and downloads,
    /// closes the comparison and says how many files changed.
    #[gpui_kit::test]
    async fn sync_with_default_compares_previews_and_synchronizes_all(cx: &mut TestAppContext) {
        let mut f = fixture(cx, false);
        std::fs::write(f.server.join("b.php"), "<?php echo 'b';\n").expect("the server's file is written");
        let dialog = open_sync_with_default(&mut f);
        let title = dialog.update(f.cx, |dialog, cx| dialog.title(cx));
        assert_eq!(title, "Diff Between 'local files' and 'files on Mounted'");
        let expected = [("src/a.php".to_owned(), SyncChoice::Upload), ("src/b.php".to_owned(), SyncChoice::Download)];
        assert_eq!(rows(&dialog, f.cx), expected);

        dialog.update(f.cx, |dialog, cx| dialog.select("src/b.php", cx));
        let preview = dialog.read_with(f.cx, |dialog, _| dialog.preview().clone());
        wait_long(f.cx, "the preview", |cx| preview.read_with(cx, |preview, _| preview.diff().is_some()));
        let diff = preview.read_with(f.cx, |preview, _| preview.diff().cloned()).expect("the comparison shows");
        let right = diff.read_with(f.cx, |diff, _| diff.side_text(crate::vcs_diff::diff_options::Side::B));
        assert_eq!(right, "<?php echo 'b';\n");

        dialog.update(f.cx, |dialog, cx| dialog.synchronize(SyncRunMode::All, cx));
        f.notice("Synchronized 2 files with Mounted");
        wait_long(f.cx, "the comparison closes", |cx| f.workbench.read_with(cx, |w, _| w.sync_dialog().is_none()));
        assert_eq!(read(&f.server.join("a.php")), "<?php echo 1;\n");
        assert_eq!(read(&f.root.join("src/b.php")), "<?php echo 'b';\n");
    }

    /// Sync with Deployed to… asks for the server; Synchronize Selected uploads the edited text of
    /// the file open in the editor (saved first), keeps the comparison open with the row skipped,
    /// and Cancel closes it.
    #[gpui_kit::test]
    async fn sync_with_chooses_the_server_saves_the_edited_file_and_synchronizes_the_selected_row(
        cx: &mut TestAppContext,
    ) {
        let mut f = fixture(cx, true);
        let file = f.root.join("src/a.php");
        f.workbench.update_in(f.cx, |w, window, cx| w.open_project_file(file.clone(), window, cx));
        wait_long(f.cx, "the editor", |cx| f.workbench.read_with(cx, |w, _| w.editor().is_some()));
        let editor = f.workbench.read_with(f.cx, |w, _| w.editor().cloned()).expect("the editor");
        editor.update(f.cx, |editor, cx| {
            let len = u32::try_from(editor.text().encode_utf16().count()).expect("a short text");
            editor.insert(Some(0..len), "<?php echo 'edited';\n", cx);
        });
        assert!(editor.read_with(f.cx, |editor, _| editor.is_dirty()));

        let target = f.path("src");
        run_action(f.cx, "deployment.syncWith", Some(json!([target])));
        assert_eq!(
            {
                wait_long(f.cx, "the chooser", |cx| {
                    f.workbench.read_with(cx, |w, _| w.open_menu::<String>().is_some())
                });
                menu_labels::<String>(&f.workbench, f.cx)
            },
            ["Mounted", "Backup"]
        );
        choose_in_menu::<String>(&f.workbench, "Mounted", f.cx);
        wait_long(f.cx, "the comparison", |cx| {
            f.workbench.read_with(cx, |w, cx| {
                w.sync_dialog()
                    .is_some_and(|dialog| dialog.read(cx).sync(cx).is_some_and(|sync| sync.entries.is_some()))
            })
        });
        let dialog = f.workbench.read_with(f.cx, |w, _| w.sync_dialog()).expect("the comparison is open");
        dialog.update(f.cx, |dialog, cx| dialog.select("src/a.php", cx));
        dialog.update(f.cx, |dialog, cx| dialog.synchronize(SyncRunMode::Selected, cx));
        f.notice("Synchronized 1 files with Mounted");
        assert_eq!(read(&f.server.join("a.php")), "<?php echo 'edited';\n", "the edited text is uploaded");
        assert!(!editor.read_with(f.cx, |editor, _| editor.is_dirty()), "the editor is saved first");
        wait_long(f.cx, "the row is refreshed", |cx| {
            rows(&dialog, cx).is_empty() || rows(&dialog, cx)[0].1 == SyncChoice::Skip
        });
        assert!(f.workbench.read_with(f.cx, |w, _| w.sync_dialog().is_some()), "the comparison stays open");

        super::super::tests::click_on(f.cx, "sync-cancel");
        assert!(f.workbench.read_with(f.cx, |w, _| w.sync_dialog().is_none()), "Cancel closes the comparison");
    }

    fn host_rows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
        workbench.read_with(cx, |w, cx| {
            w.remote_host_view()
                .map_or(Vec::new(), |view| view.read(cx).rows().into_iter().map(|row| row.label).collect())
        })
    }

    fn right_click(cx: &mut VisualTestContext, selector: String) {
        let selector: &'static str = Box::leak(selector.into_boxed_str());
        cx.run_until_parked();
        let at = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
    }

    fn type_into_prompt(workbench: &Entity<Workbench>, text: &str, cx: &mut VisualTestContext) {
        wait_long(cx, "the prompt", |cx| workbench.read_with(cx, |w, _| w.vcs_prompt().is_some()));
        let prompt = workbench.read_with(cx, |w, _| w.vcs_prompt()).expect("the prompt");
        let field = prompt.read_with(cx, |prompt, _| prompt.field().cloned()).expect("the prompt has a field");
        cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
        field.update(cx, |field, cx| field.select_all(cx));
        cx.simulate_input(text);
        cx.run_until_parked();
    }

    /// Browse Remote Host shows the server's root and files; its menu creates a folder, renames a
    /// file, copies a path, opens a file in the editor, downloads a mapped file to the project and
    /// deletes on the server.
    #[gpui_kit::test]
    async fn the_remote_host_window_lists_the_server_and_its_menu_changes_its_files(cx: &mut TestAppContext) {
        let mut f = fixture(cx, false);
        std::fs::write(f.server.join("a.php"), "<?php echo 'server';\n").expect("the server's file is written");
        std::fs::write(f.server.join("notes.txt"), "remote notes\n").expect("the server's file is written");
        run_action(f.cx, "deployment.browseRemoteHost", None);
        wait_long(f.cx, "the server's files", |cx| host_rows(&f.workbench, cx) == ["a.php", "notes.txt"]);
        let server = f.server.to_string_lossy().into_owned();
        let view = f.workbench.read_with(f.cx, |w, _| w.remote_host_view()).expect("the tool window");
        assert_eq!(view.read_with(f.cx, |view, _| view.server_label().map(str::to_owned)).as_deref(), Some("Mounted"));
        let root = view.read_with(f.cx, |view, _| view.state().tree.as_ref().and_then(|tree| tree.root.clone()));
        assert_eq!(root.as_deref(), Some(server.as_str()));

        // Refresh on a file lists the server again; New Folder… below the rows makes one in the root.
        right_click(f.cx, format!("remote-host-row:{server}/notes.txt"));
        choose_in_menu::<HostCommand>(&f.workbench, "Refresh", f.cx);
        wait_long(f.cx, "the server's files again", |cx| host_rows(&f.workbench, cx) == ["a.php", "notes.txt"]);
        let area = f.cx.debug_bounds("remote-host-tree").expect("the tree is drawn");
        let below = gpui_kit::point(area.origin.x + px(10.0), area.origin.y + area.size.height - px(10.0));
        f.cx.simulate_mouse_down(below, MouseButton::Right, Modifiers::default());
        f.cx.simulate_mouse_up(below, MouseButton::Right, Modifiers::default());
        f.cx.run_until_parked();
        choose_in_menu::<HostCommand>(&f.workbench, "New Folder…", f.cx);
        type_into_prompt(&f.workbench, "logs", f.cx);
        press_ok(&f.workbench, f.cx);
        wait_long(f.cx, "the new folder", |cx| host_rows(&f.workbench, cx).contains(&"logs".to_owned()));
        assert!(f.server.join("logs").is_dir());

        // Rename… a file.
        right_click(f.cx, format!("remote-host-row:{server}/notes.txt"));
        choose_in_menu::<HostCommand>(&f.workbench, "Rename…", f.cx);
        type_into_prompt(&f.workbench, "todo.txt", f.cx);
        press_ok(&f.workbench, f.cx);
        wait_long(f.cx, "the renamed file", |cx| host_rows(&f.workbench, cx).contains(&"todo.txt".to_owned()));
        assert_eq!(read(&f.server.join("todo.txt")), "remote notes\n");

        // Copy Path.
        right_click(f.cx, format!("remote-host-row:{server}/todo.txt"));
        choose_in_menu::<HostCommand>(&f.workbench, "Copy Path", f.cx);
        let copied = f.cx.read_from_clipboard().and_then(|item| item.text());
        assert_eq!(copied, Some(format!("{server}/todo.txt")));

        // Open: a copy of the server's file opens in the editor.
        right_click(f.cx, format!("remote-host-row:{server}/todo.txt"));
        choose_in_menu::<HostCommand>(&f.workbench, "Open", f.cx);
        wait_long(f.cx, "the editor of the server's file", |cx| {
            f.workbench
                .read_with(cx, |w, cx| w.editor().is_some_and(|editor| editor.read(cx).text() == "remote notes\n"))
        });

        // Download to Project through the backend's mappings.
        right_click(f.cx, format!("remote-host-row:{server}/a.php"));
        choose_in_menu::<HostCommand>(&f.workbench, "Download to Project", f.cx);
        f.notice("Downloaded 1 files from Mounted");
        assert_eq!(read(&f.root.join("src/a.php")), "<?php echo 'server';\n");

        // Delete… asks, then deletes on the server.
        right_click(f.cx, format!("remote-host-row:{server}/todo.txt"));
        choose_in_menu::<HostCommand>(&f.workbench, "Delete…", f.cx);
        wait_long(f.cx, "the question", |cx| f.workbench.read_with(cx, |w, _| w.vcs_prompt().is_some()));
        let message = f
            .workbench
            .read_with(f.cx, |w, _| w.vcs_prompt())
            .expect("the question")
            .read_with(f.cx, |d, _| d.message_text().map(str::to_owned));
        assert_eq!(message, Some(format!("Delete file “{server}/todo.txt” from the server?")));
        press_ok(&f.workbench, f.cx);
        wait_long(f.cx, "the file goes", |cx| !host_rows(&f.workbench, cx).contains(&"todo.txt".to_owned()));
        assert!(!f.server.join("todo.txt").exists());
        assert!(!notification_titles(&f.workbench, f.cx).iter().any(|title| title.starts_with("Cannot")));
    }

    /// The server chooser of the Remote Host window switches to the other server's files.
    #[gpui_kit::test]
    async fn the_remote_host_server_chooser_shows_the_other_servers_files(cx: &mut TestAppContext) {
        let f = fixture(cx, true);
        let backup = f.backup.clone().expect("a second server");
        std::fs::write(backup.join("backup.txt"), "b\n").expect("the server's file is written");
        run_action(f.cx, "deployment.browseRemoteHost", None);
        wait_long(f.cx, "the default server", |cx| {
            f.workbench.read_with(cx, |w, cx| {
                w.remote_host_view().is_some_and(|view| view.read(cx).server_label() == Some("Mounted"))
            })
        });
        super::super::tests::click_on(f.cx, "remote-host-server");
        assert_eq!(menu_labels::<String>(&f.workbench, f.cx), ["Mounted", "Backup"]);
        choose_in_menu::<String>(&f.workbench, "Backup", f.cx);
        wait_long(f.cx, "the other server's files", |cx| host_rows(&f.workbench, cx) == ["backup.txt"]);
    }
}
