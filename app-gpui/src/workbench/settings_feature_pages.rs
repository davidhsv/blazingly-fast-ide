//! The Settings pages whose values are not settings: Security › Project Trust (the backend's trust
//! store), Editor › TODO, Version Control › Log and Version Control › GitHub (the UI storage). Each
//! is a view drawn for its whole page, and the workbench carries out what it asks: the trust
//! store's requests on the backend, with the page given the lists again once one is done; the
//! other pages' changes stored at once, which every TODO window follows, and handed to the window's
//! Git log. None of them stages anything, so Apply stays off and Reset, Cancel and OK leave them
//! alone, as their options take effect the moment they change.

use super::Workbench;
use super::plugin_runtime::plugin_host;
use crate::backend::Backend;
use crate::features_a::todo::{TODO_SETTINGS_KEY, stored_todo_settings, todo_settings_json};
use crate::github::diff::REVIEW_DIFF_SETTINGS_KEY;
use crate::platform::storage::UiStorage;
use crate::settings_model::staging::Scope;
use crate::settings_view::dialog::SettingsDialogView;
use crate::settings_view::pages::Binding;
use crate::settings_view::pages::editor_todo::{self, TodoSettingsEvent, TodoSettingsView};
use crate::settings_view::pages::security_project_trust::{self, ProjectTrustEvent, ProjectTrustView, TrustLists};
use crate::settings_view::pages::vcs_github::{
    self, GithubSettingsEvent, GithubSettingsView, review_settings_with_auto_mark,
};
use crate::settings_view::pages::vcs_log::{self, LogSettingsEvent, LogSettingsView};
use gpui_kit::{AnyView, AppContext, Context, Entity, PathPromptOptions, Subscription, WeakEntity, Window};
use ide_core::IdeResult;
use ide_host::Host;
use ide_host::plugins::PluginHost;
use ide_host::trust::TrustDecisions;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What Project Trust asks of the trust store; each ends with the decisions read again.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TrustRequest {
    /// Only read the decisions (the page was opened).
    Read,
    /// Revoke Trust of the project at the path.
    Revoke(PathBuf),
    /// Remove the trusted location at the path.
    RemoveLocation(PathBuf),
    /// Trust the folder at the path as a location.
    AddLocation(PathBuf),
}

/// Carries out `request` on the trust store and the projects open, then reads the decisions. A
/// project that is no longer trusted has its project plugins unloaded, as Revoke Trust and Remove
/// Trusted Location do when run as actions. Like those actions, a Revoke Trust or Remove that fails
/// is logged and the page lists what the store holds; a failed read or Add folder… is the page's.
async fn carry_out(
    host: Arc<Host>,
    plugins: Option<Arc<PluginHost>>,
    request: TrustRequest,
) -> IdeResult<TrustDecisions> {
    match request {
        TrustRequest::Read => {}
        TrustRequest::Revoke(path) => {
            if let Err(error) = revoke(&host, plugins, path).await {
                tracing::error!("action project.revokeTrust failed: {}", error.message);
            }
        }
        TrustRequest::RemoveLocation(path) => {
            if let Err(error) = remove_location(&host, plugins, &path).await {
                tracing::error!("action project.removeTrustedLocation failed: {}", error.message);
            }
        }
        TrustRequest::AddLocation(path) => {
            host.trust.add_location(&path)?;
            host.refresh_project_trust().await?;
        }
    }
    host.trust.decisions()
}

async fn revoke(host: &Host, plugins: Option<Arc<PluginHost>>, path: PathBuf) -> IdeResult<()> {
    host.set_project_trust(&path, false, false).await?;
    if let Some(plugins) = plugins {
        // The folder as the trust store names it, links resolved.
        let root = host.trust.status(&path).map(|status| status.root.into_path_buf()).unwrap_or(path);
        plugins.restrict_project(&root);
    }
    Ok(())
}

async fn remove_location(host: &Host, plugins: Option<Arc<PluginHost>>, path: &Path) -> IdeResult<()> {
    host.trust.remove_location(path)?;
    host.refresh_project_trust().await?;
    if let Some(plugins) = plugins {
        for project in host.projects().into_iter().filter(|project| !project.is_trusted()) {
            plugins.restrict_project(&project.root);
        }
    }
    Ok(())
}

/// Draws `view` for the whole of `page`, its labels found by the Settings search.
pub(super) fn mount_page(
    dialog: &Entity<SettingsDialogView>,
    page: &'static str,
    view: AnyView,
    labels: &[&str],
    cx: &mut Context<Workbench>,
) {
    let labels = labels.iter().map(|label| (*label).to_owned()).collect();
    dialog.update(cx, |dialog, cx| {
        dialog.set_page_view(page, view, cx);
        dialog.set_page_labels(page, labels, cx);
    });
}

impl Workbench {
    /// Puts the views of Project Trust, TODO, Log and GitHub on their pages of `dialog`, and returns
    /// the subscriptions that answer them; the caller keeps them as long as the dialog is open.
    pub(super) fn mount_feature_settings_pages(
        &mut self,
        dialog: &Entity<SettingsDialogView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Subscription> {
        let model = dialog.read(cx).model().clone();
        // The pages have no Global / This project switch: their values are the user's everywhere.
        let binding = |page: &'static str| Binding::fixed(model.clone(), page, Scope::Global);

        let trust = cx.new(|cx| ProjectTrustView::new(binding(security_project_trust::PAGE), cx));
        mount_page(dialog, security_project_trust::PAGE, trust.clone().into(), &security_project_trust::LABELS, cx);
        self.trust_page_request(trust.downgrade(), TrustRequest::Read, cx);

        let settings = stored_todo_settings(cx);
        let todo = cx.new(|cx| TodoSettingsView::new(binding(editor_todo::PAGE), settings, cx));
        mount_page(dialog, editor_todo::PAGE, todo.clone().into(), &editor_todo::LABELS, cx);

        let log = cx.new(|cx| LogSettingsView::new(binding(vcs_log::PAGE), cx));
        mount_page(dialog, vcs_log::PAGE, log.clone().into(), &vcs_log::labels(), cx);

        let github = cx.new(|cx| GithubSettingsView::new(binding(vcs_github::PAGE), cx));
        mount_page(dialog, vcs_github::PAGE, github.clone().into(), &vcs_github::LABELS, cx);

        vec![
            cx.subscribe_in(&trust, window, Self::project_trust_page_event),
            cx.subscribe_in(&todo, window, Self::todo_settings_page_event),
            cx.subscribe_in(&log, window, Self::log_settings_page_event),
            cx.subscribe_in(&github, window, Self::github_settings_page_event),
        ]
    }

    fn project_trust_page_event(
        &mut self,
        page: &Entity<ProjectTrustView>,
        event: &ProjectTrustEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let page = page.downgrade();
        match event {
            ProjectTrustEvent::Revoke(path) => self.trust_page_request(page, TrustRequest::Revoke(path.into()), cx),
            ProjectTrustEvent::RemoveLocation(path) => {
                self.trust_page_request(page, TrustRequest::RemoveLocation(path.into()), cx)
            }
            ProjectTrustEvent::AddLocation => self.add_trusted_location(page, cx),
            ProjectTrustEvent::Shown => self.trust_page_request(page, TrustRequest::Read, cx),
        }
    }

    /// Runs `request` on the backend and gives `page` the decisions after it, or why it failed (the
    /// lists then stay as they were).
    fn trust_page_request(
        &mut self,
        page: WeakEntity<ProjectTrustView>,
        request: TrustRequest,
        cx: &mut Context<Self>,
    ) {
        let Some(backend) = cx.try_global::<Backend>() else {
            page.update(cx, |page, cx| page.set_error("The backend is not running".to_owned(), cx)).ok();
            return;
        };
        let plugins = plugin_host(cx);
        let work = backend.with_host(move |host| carry_out(host, plugins, request));
        cx.spawn(async move |_, cx| {
            // The page shows a failure as the text of the error the backend answered with.
            let shown = match work.await {
                Ok(Ok(decisions)) => Ok(TrustLists::from_decisions(&decisions)),
                Ok(Err(error)) => Err(format!("IpcError: {}", error.message)),
                Err(error) => Err(format!("Error: {error}")),
            };
            // The page is gone when the dialog closed meanwhile.
            page.update(cx, |page, cx| match shown {
                Ok(lists) => page.set_lists(lists, cx),
                Err(error) => page.set_error(error, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Add folder…: the folder chosen in the system's chooser becomes a trusted location; nothing
    /// when the chooser is dismissed.
    fn add_trusted_location(&mut self, page: WeakEntity<ProjectTrustView>, cx: &mut Context<Self>) {
        let options = PathPromptOptions { files: false, directories: true, multiple: false, prompt: None };
        let picked = cx.prompt_for_paths(options);
        cx.spawn(async move |this, cx| match picked.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    this.update(cx, |workbench, cx| {
                        workbench.trust_page_request(page, TrustRequest::AddLocation(path), cx)
                    })
                    .ok();
                }
            }
            Ok(Ok(None)) | Err(_) => {}
            Ok(Err(error)) => {
                page.update(cx, |page, cx| page.set_error(format!("Error: {error}"), cx)).ok();
            }
        })
        .detach();
    }

    /// TODO: the settings are stored; every TODO window follows the storage and matches with them.
    fn todo_settings_page_event(
        &mut self,
        _: &Entity<TodoSettingsView>,
        event: &TodoSettingsEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TodoSettingsEvent::Changed(settings) => {
                cx.default_global::<UiStorage>().set(TODO_SETTINGS_KEY, todo_settings_json(settings));
            }
            TodoSettingsEvent::Submit => {
                if let Some(dialog) = self.settings_dialog().cloned() {
                    dialog.update(cx, |dialog, cx| dialog.ok(cx));
                }
            }
            TodoSettingsEvent::Cancel => {
                if let Some(dialog) = self.settings_dialog().cloned() {
                    dialog.update(cx, |dialog, cx| dialog.cancel(cx));
                }
            }
        }
    }

    /// Log: the options are stored and the open log shows them.
    fn log_settings_page_event(
        &mut self,
        _: &Entity<LogSettingsView>,
        event: &LogSettingsEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let LogSettingsEvent::Changed(options) = event;
        self.set_log_options((**options).clone(), cx);
    }

    /// GitHub: the option is stored with the pull request diff's other settings.
    fn github_settings_page_event(
        &mut self,
        _: &Entity<GithubSettingsView>,
        event: &GithubSettingsEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let GithubSettingsEvent::SetAutoMarkViewed(on) = *event;
        let text = review_settings_with_auto_mark(cx, on);
        cx.default_global::<UiStorage>().set(REVIEW_DIFF_SETTINGS_KEY, text);
    }
}
