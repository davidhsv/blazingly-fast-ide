//! The Settings dialog of a window: File › Settings… (and Settings for New Projects…, Keymap…,
//! Appearance…, Plugins…) over the window's settings, with Apply and OK carried out on the backend.
//!
//! The dialog is a layer of its own over the window, below the popup slot (as Edit Configurations
//! is): only its own buttons and Escape close it, so its staged edits are not lost to a stray click
//! or to a menu opened over it. Its pages stage into one [`SettingsDialog`] model; the Keymap page
//! and the Plugins page are views of their own given to the dialog's slots.
//!
//! Apply writes the staged values one at a time through the backend, stopping at the first that
//! fails; every window then hears which key changed, and what follows a setting (the theme, the
//! keymap, the editors) follows it from there.

use super::Workbench;
use super::services_window::on_workbench;
use crate::backend::Backend;
use crate::platform::actions::ActionRegistry;
use crate::platform::input;
use crate::platform::keymap::Keymap as Bindings;
use crate::platform::settings::{effective, global, set};
use crate::plugin_models::contribution::Contribution;
use crate::plugin_models::plugins_page::{PluginsPage, PluginsPageEvent};
use crate::plugin_models::store::{InstallStep, SETTINGS_PAGE_ID};
use crate::plugins::Plugins;
use crate::settings_model::builtin_pages::{code_style_language_pages, settings_registry};
use crate::settings_model::schema::settings_value;
use crate::settings_model::staging::{SecretWrite, SettingsDialog, StagedChange};
use crate::settings_view::dialog::{SettingsDialogEvent, SettingsDialogView, SettingsPageSet};
use crate::settings_view::keymap::{KEYMAP_PAGE, KeymapView, registered_actions};
use gpui_kit::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent, Subscription, Window, div,
    prelude::*,
};
use ide_core::IdeResult;
use ide_host::plugins::PluginHost;
use serde_json::Value;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The page Settings for New Projects opens on: the JDK, Maven and the language server are what
/// new projects most often need.
const NEW_PROJECTS_PAGE: &str = "languages.java";
/// The page of Appearance….
const APPEARANCE_PAGE: &str = "appearance";
/// The action whose presence tells that this build updates itself (Settings › Updates).
const UPDATES_ACTION: &str = "app.checkForUpdates";
/// The settings the keymap is made of.
const KEYMAP_KEYS: [&str; 3] = ["keymap", "keymapOverrides", "customKeymaps"];
/// How long a listing of the plugins waits for the installed ones to have loaded.
const INITIAL_LOAD_WAIT: Duration = Duration::from_secs(5);
/// Why an operation of the Plugins page that this window does not carry out failed.
const NOT_CARRIED_OUT: &str = "This window cannot do this";

/// The Settings dialog, while it is open.
pub(super) struct SettingsWindow {
    dialog: Entity<SettingsDialogView>,
    /// The Keymap page, which stages into the dialog's model.
    keymap: Entity<KeymapView>,
    /// The Plugins page; none without a plugin host.
    plugins: Option<Entity<PluginsPage>>,
    /// What had the keyboard before the dialog opened.
    restore: Option<FocusHandle>,
    _subscriptions: Vec<Subscription>,
}

/// The string argument of an action.
fn text_arg(arg: Option<&Value>) -> Option<String> {
    arg.and_then(Value::as_str).map(str::to_owned)
}

/// Registers the actions that open the Settings dialog, each on its page.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    // The argument is the page, or `page#focus` to also select something on it.
    registry.handle("file.settings", |call, window, cx| {
        let page = text_arg(call.arg);
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.open_settings(page.as_deref(), false, window, cx);
        })
    });
    registry.handle("file.newProjectsSettings", |call, window, cx| {
        let page = text_arg(call.arg).unwrap_or_else(|| NEW_PROJECTS_PAGE.to_owned());
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.open_settings(Some(&page), true, window, cx);
        })
    });
    let pages = [
        ("settings.keymap", KEYMAP_PAGE),
        ("settings.appearance", APPEARANCE_PAGE),
        ("plugins.settings", SETTINGS_PAGE_ID),
    ];
    for (id, page) in pages {
        registry.handle(id, move |_, window, cx| {
            on_workbench(window, cx, |workbench, window, cx| workbench.open_settings(Some(page), false, window, cx))
        });
    }
}

/// Whether `key` is `root` or a key inside it.
fn is_under(key: &str, root: &str) -> bool {
    key.strip_prefix(root).is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// Makes the keymap that dispatches shortcuts the one the global settings name: the keymap with
/// its overrides, or a user keymap. Windows are drawn again when it changed (menus and tooltips
/// show shortcuts).
pub fn apply_keymap_settings(cx: &mut App) {
    if !cx.has_global::<input::Keymap>() {
        return;
    }
    let settings = settings_value(&global(cx));
    let [keymap, overrides, custom] = KEYMAP_KEYS.map(|key| settings.get(key));
    if cx.global_mut::<input::Keymap>().0.apply_settings(keymap, overrides, custom) {
        cx.refresh_windows();
    }
}

/// The languages of the backend as (id, name), for their Code Style pages.
fn code_style_languages(cx: &App) -> Vec<(String, String)> {
    let Some(backend) = cx.try_global::<Backend>() else { return Vec::new() };
    backend.host.languages.all().into_iter().map(|language| (language.id.0, language.name)).collect()
}

/// Where the local catalog of plugins is read from: the repository's official plugins in debug
/// builds; a released build has none next to it.
fn catalog_dirs() -> Vec<PathBuf> {
    if cfg!(debug_assertions) {
        vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins/official")]
    } else {
        Vec::new()
    }
}

/// Runs `work` on the plugin host off the main thread; the failure is its message.
fn plugin_work<T, W>(cx: &App, work: W) -> impl Future<Output = Result<T, String>> + 'static + use<T, W>
where
    T: Send + 'static,
    W: FnOnce(&PluginHost) -> IdeResult<T> + Send + 'static,
{
    let host = cx.try_global::<Plugins>().map(|plugins| plugins.host.clone());
    let task = host.map(|host| cx.background_executor().spawn(async move { work(&host) }));
    async move {
        match task {
            None => Err("The plugin host is not running".to_owned()),
            Some(task) => task.await.map_err(|error| error.message),
        }
    }
}

/// Stores the credential of an SSH configuration, in the keychain when `write.save`.
fn store_ssh_secret(write: &SecretWrite, cx: &App) -> Result<(), String> {
    let Some(backend) = cx.try_global::<Backend>() else { return Err("The backend is not running".to_owned()) };
    backend.host.ssh.set_secret(&write.id, Some(write.secret.expose().to_owned()), write.save);
    Ok(())
}

/// The name of a Plugins page event, for the log: never its fields, which may hold a secret.
fn event_name(event: &PluginsPageEvent) -> &'static str {
    match event {
        PluginsPageEvent::Refresh { .. } => "Refresh",
        PluginsPageEvent::SetEnabled { .. } => "SetEnabled",
        PluginsPageEvent::Reload(_) => "Reload",
        PluginsPageEvent::Uninstall(_) => "Uninstall",
        PluginsPageEvent::Grant { .. } => "Grant",
        PluginsPageEvent::SetCapability { .. } => "SetCapability",
        PluginsPageEvent::RollBack(_) => "RollBack",
        PluginsPageEvent::AskConsent(_) => "AskConsent",
        PluginsPageEvent::Install(_) => "Install",
        PluginsPageEvent::DownloadMarket(_) => "DownloadMarket",
        PluginsPageEvent::EditRepository { .. } => "EditRepository",
        PluginsPageEvent::ImportInstalledIntellij => "ImportInstalledIntellij",
        PluginsPageEvent::ImportIntellijFile => "ImportIntellijFile",
        PluginsPageEvent::OpenUrl(_) => "OpenUrl",
        PluginsPageEvent::SetTheme(_) => "SetTheme",
        PluginsPageEvent::SelectIconTheme(_) => "SelectIconTheme",
        PluginsPageEvent::RunCommand { .. } => "RunCommand",
        PluginsPageEvent::LoadSecrets(_) => "LoadSecrets",
        PluginsPageEvent::SetSecret { .. } => "SetSecret",
        PluginsPageEvent::Notify(_) => "Notify",
    }
}

impl Workbench {
    /// The Settings dialog, while it is open.
    pub fn settings_dialog(&self) -> Option<&Entity<SettingsDialogView>> {
        self.settings.as_ref().map(|open| &open.dialog)
    }

    /// The Keymap page of the open Settings dialog.
    pub fn settings_keymap(&self) -> Option<&Entity<KeymapView>> {
        self.settings.as_ref().map(|open| &open.keymap)
    }

    /// The Plugins page of the open Settings dialog, when there is a plugin host.
    pub fn settings_plugins_page(&self) -> Option<&Entity<PluginsPage>> {
        self.settings.as_ref().and_then(|open| open.plugins.as_ref())
    }

    /// Opens Settings on `page` (`page#focus` also selects something on it; none: the page shown
    /// last), over the settings in effect for the window. `new_projects` opens Settings for New
    /// Projects, where every page edits the global layer. An open dialog shows `page` and takes the
    /// keyboard instead.
    pub fn open_settings(
        &mut self,
        page: Option<&str>,
        new_projects: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.popup.is_some() {
            self.close_popup(window, cx);
        }
        if let Some(open) = &self.settings {
            let dialog = open.dialog.clone();
            if let Some(page) = page {
                let model = dialog.read(cx).model().clone();
                model.update(cx, |model, cx| {
                    model.select_page(page);
                    cx.notify();
                });
            }
            window.focus(&dialog.focus_handle(cx), cx);
            return;
        }
        let has_project = self.project_root(cx).is_some();
        let settings = settings_value(&effective(&self.label, cx));
        let global_layer = has_project.then(|| settings_value(&global(cx)));
        let model = cx.new(|_| {
            let mut dialog = SettingsDialog::default();
            dialog.open(page, new_projects, settings);
            if let Some(global_layer) = global_layer {
                dialog.set_global_base(global_layer);
            }
            dialog
        });
        let updates = cx.try_global::<ActionRegistry>().is_some_and(|registry| registry.contains(UPDATES_ACTION));
        let languages = code_style_languages(cx);
        let languages: Vec<(&str, &str)> = languages.iter().map(|(id, name)| (id.as_str(), name.as_str())).collect();
        let set = SettingsPageSet { specs: settings_registry(updates), pages: code_style_language_pages(&languages) };
        let dialog = cx.new(|cx| SettingsDialogView::new(model.clone(), set, has_project, cx));
        // No plugin registers default shortcuts with the keymap: there are none to list.
        let keymap = cx.new(|cx| KeymapView::new(model.clone(), registered_actions(cx), Bindings::new(), window, cx));
        dialog.update(cx, |dialog, cx| {
            dialog.set_page_view(KEYMAP_PAGE, keymap.clone().into(), cx);
            let labels = keymap.read(cx).search_labels(cx);
            dialog.set_page_labels(KEYMAP_PAGE, labels, cx);
        });
        let mut subscriptions = vec![cx.subscribe_in(&dialog, window, Self::settings_dialog_event)];
        let plugins = if cx.has_global::<Plugins>() {
            let page = cx.new(|cx| PluginsPage::new(window, cx));
            subscriptions.push(cx.subscribe_in(&page, window, Self::plugins_page_event));
            self.follow_plugins_settings(&page, cx);
            dialog.update(cx, |dialog, cx| dialog.set_page_view(SETTINGS_PAGE_ID, page.clone().into(), cx));
            Some(page)
        } else {
            None
        };
        let restore = window.focused(cx);
        window.focus(&dialog.focus_handle(cx), cx);
        self.settings =
            Some(SettingsWindow { dialog, keymap, plugins: plugins.clone(), restore, _subscriptions: subscriptions });
        // The page lists the plugins once it is shown, which asks the workbench subscribed above.
        if let Some(page) = plugins {
            page.update(cx, |page, cx| page.opened(cx));
        }
        cx.notify();
    }

    /// The project goes, or another opens: the dialog goes with the settings it was opened over.
    pub(super) fn drop_settings(&mut self) {
        self.settings = None;
    }

    /// Closes the dialog; the keyboard goes back to what had it before, else to the editor.
    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.settings.take() else { return };
        if let Some(target) = open.restore.or_else(|| self.editor().map(|editor| editor.focus_handle(cx))) {
            window.focus(&target, cx);
        }
        cx.notify();
    }

    fn settings_dialog_event(
        &mut self,
        dialog: &Entity<SettingsDialogView>,
        event: &SettingsDialogEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings_dialog() != Some(dialog) {
            return;
        }
        match event {
            SettingsDialogEvent::Apply { changes } => self.apply_settings(dialog, changes, cx),
            SettingsDialogEvent::Closed => self.close_settings(window, cx),
        }
    }

    /// Apply (or OK): writes `changes` in order and stops at the first that fails, then tells the
    /// dialog the settings now in effect (and the global layer, with a project open), stores the
    /// staged SSH credentials and the values kept outside the settings, and ends the Apply. The
    /// dialog says what failed in its footer; after OK a full save closes it.
    fn apply_settings(
        &mut self,
        dialog: &Entity<SettingsDialogView>,
        changes: &[StagedChange],
        cx: &mut Context<Self>,
    ) {
        let label = self.label.clone();
        let mut written = 0;
        let mut failure = None;
        for change in changes {
            match set(&label, change.scope, &change.key, &change.value, cx) {
                Ok(_) => written += 1,
                Err(error) => {
                    failure = Some(error.message);
                    break;
                }
            }
        }
        let has_project = self.project_root(cx).is_some();
        let base = settings_value(&effective(&label, cx));
        let global_layer = has_project.then(|| settings_value(&global(cx)));
        let keychain = cx.has_global::<Backend>();
        dialog.update(cx, |view, cx| {
            view.apply_writes_done(written, failure.as_deref(), base, has_project, cx);
            if let Some(global_layer) = global_layer {
                view.model().clone().update(cx, |model, cx| {
                    model.set_global_base(global_layer);
                    cx.notify();
                });
            }
            while let Some(write) = view.apply_next_secret(keychain, cx) {
                let stored = store_ssh_secret(&write, cx);
                view.apply_secret_done(stored, cx);
            }
            // No page of this window stages a value kept outside the settings; one that did would
            // need its writer here, so it is refused rather than reported as saved.
            while let Some((id, _)) = view.apply_next_external(cx) {
                tracing::warn!("Settings: nothing in this window saves {id}");
                view.apply_external_done(Err(NOT_CARRIED_OUT.to_owned()), cx);
            }
            view.finish_apply(cx);
        });
    }

    /// A setting changed, in this window or another: the keymap follows the keymap settings, and the
    /// Plugins page the theme and the file icon theme.
    pub(super) fn settings_changed(&mut self, key: &str, cx: &mut Context<Self>) {
        if key.is_empty() || KEYMAP_KEYS.iter().any(|root| is_under(key, root)) {
            apply_keymap_settings(cx);
        }
        if (key.is_empty() || is_under(key, "appearance"))
            && let Some(page) = self.settings_plugins_page().cloned()
        {
            self.follow_plugins_settings(&page, cx);
        }
    }

    /// Settings over the window, while it is open: below the popup slot, so that a menu opened from
    /// one of its pages shows over it. Escape that no part of it took cancels it.
    pub(super) fn settings_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.settings_dialog()?.clone();
        let layer = div()
            .id("settings-layer")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .on_key_down(cx.listener(|workbench, event: &KeyDownEvent, _, cx| {
                let keystroke = &event.keystroke;
                if keystroke.key != "escape" || keystroke.modifiers.modified() {
                    return;
                }
                if let Some(dialog) = workbench.settings_dialog().cloned() {
                    dialog.update(cx, |dialog, cx| dialog.cancel(cx));
                    cx.stop_propagation();
                }
            }))
            .child(dialog);
        Some(gpui_kit::deferred(layer).with_priority(1).into_any_element())
    }

    // ---------------------------------------------------------------------------------------------
    // The Plugins page

    /// Tells the Plugins page whether the window's project is trusted, and the theme and the file
    /// icon theme the settings name.
    fn follow_plugins_settings(&self, page: &Entity<PluginsPage>, cx: &mut Context<Self>) {
        let root = self.project_root(cx);
        let trusted = cx
            .try_global::<Backend>()
            .zip(root)
            .is_some_and(|(backend, root)| backend.host.trust.trusted(&root).unwrap_or(false));
        let appearance = global(cx).appearance;
        let icon_theme = Some(appearance.file_icon_theme).filter(|key| !key.is_empty());
        page.update(cx, |page, cx| {
            page.set_trusted_project(trusted, cx);
            page.set_appearance(Some(appearance.theme), icon_theme, cx);
        });
    }

    /// What the Plugins page asks: its listing and enabling, disabling and reloading a plugin are
    /// carried out on the plugin host, and its notifications are raised. Every other operation is
    /// logged and answered as failed, so that nothing on the page stays busy waiting for it.
    fn plugins_page_event(
        &mut self,
        page: &Entity<PluginsPage>,
        event: &PluginsPageEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        fn refused<T>() -> Result<T, String> {
            Err(NOT_CARRIED_OUT.to_owned())
        }
        match event.clone() {
            PluginsPageEvent::Refresh { plugins, catalog, market } => {
                if plugins {
                    self.list_plugins(page, cx);
                }
                if catalog {
                    Self::read_plugin_catalog(page, cx);
                }
                if let Some(refresh) = market {
                    Self::read_plugin_market(page, refresh, cx);
                }
            }
            PluginsPageEvent::SetEnabled { id, enabled } => {
                let target = id.clone();
                let work = plugin_work(cx, move |host| host.set_enabled(&target, enabled));
                Self::answer_plugins_page(page, work, move |page, result, cx| page.finished(&id, result, cx), cx);
            }
            PluginsPageEvent::Reload(id) => {
                let target = id.clone();
                let work = plugin_work(cx, move |host| host.reload(&target).map(|_| ()));
                Self::answer_plugins_page(page, work, move |page, result, cx| page.finished(&id, result, cx), cx);
            }
            PluginsPageEvent::Notify(request) => self.notify(request, cx),
            other => {
                tracing::info!("Settings › Plugins: {} is not carried out in this window", event_name(&other));
                page.update(cx, |page, cx| match other {
                    PluginsPageEvent::Uninstall(id)
                    | PluginsPageEvent::Grant { id, .. }
                    | PluginsPageEvent::SetCapability { id, .. } => page.finished(&id, refused(), cx),
                    PluginsPageEvent::RollBack(id) => page.rolled_back(&id, refused(), cx),
                    PluginsPageEvent::AskConsent(_) => page.consented(None, cx),
                    PluginsPageEvent::Install(InstallStep::Pick { .. }) => page.picked(refused(), cx),
                    PluginsPageEvent::Install(InstallStep::Unpack(_)) => page.unpacked(refused(), cx),
                    PluginsPageEvent::Install(InstallStep::Preview(_)) => page.previewed(refused(), cx),
                    PluginsPageEvent::Install(InstallStep::Install { .. }) => page.installed(refused(), cx),
                    PluginsPageEvent::DownloadMarket(id) => page.downloaded(&id, refused(), cx),
                    // Nothing on the page waits for these. The secrets are not read, so that no
                    // secret field is shown before it can mask what is typed.
                    _ => {}
                });
            }
        }
    }

    /// Lists the plugins as the window sees them: the installed and linked ones and the plugins of
    /// its project (loaded first when the project is trusted).
    fn list_plugins(&self, page: &Entity<PluginsPage>, cx: &mut Context<Self>) {
        let root = self.project_root(cx);
        let work = plugin_work(cx, move |host| {
            host.wait_initial_load(INITIAL_LOAD_WAIT);
            host.sync_links();
            if let Some(root) = &root {
                host.sync_project(root);
            }
            Ok(host.list(root.as_deref()))
        });
        Self::answer_plugins_page(
            page,
            work,
            |page, listed, cx| match listed {
                Ok(snapshot) => {
                    let contributions = Contribution::from_snapshot(&snapshot);
                    page.set_listing(snapshot.plugins, contributions, cx);
                }
                Err(error) => page.listing_failed(error, cx),
            },
            cx,
        );
    }

    fn read_plugin_catalog(page: &Entity<PluginsPage>, cx: &mut Context<Self>) {
        let dirs = catalog_dirs();
        let work = plugin_work(cx, move |host| Ok(host.catalog(&dirs)));
        Self::answer_plugins_page(page, work, |page, catalog, cx| page.set_catalog(catalog, cx), cx);
    }

    /// The marketplace's plugins, its index read again with `refresh`; none without an index.
    fn read_plugin_market(page: &Entity<PluginsPage>, refresh: bool, cx: &mut Context<Self>) {
        let work = plugin_work(cx, move |host| {
            let url = host.market_url();
            let listing = match url {
                Some(_) => host.market_listing(refresh).map_err(|error| error.message),
                None => Ok(Vec::new()),
            };
            Ok((url, listing))
        });
        Self::answer_plugins_page(
            page,
            work,
            |page, market, cx| match market {
                Ok((url, listing)) => page.set_market(url, listing, cx),
                Err(error) => page.set_market(None, Err(error), cx),
            },
            cx,
        );
    }

    /// Hands the outcome of `work` to the page with `answer`, unless the page is gone by then.
    fn answer_plugins_page<T: 'static>(
        page: &Entity<PluginsPage>,
        work: impl Future<Output = Result<T, String>> + 'static,
        answer: impl FnOnce(&mut PluginsPage, Result<T, String>, &mut Context<PluginsPage>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let page = page.downgrade();
        cx.spawn(async move |_, cx| {
            let outcome = work.await;
            // The dialog was closed meanwhile: nobody waits for the answer.
            let _ = page.update(cx, |page, cx| answer(page, outcome, cx));
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: a key is under a root when it is the root or a key inside it, never when it
    // only starts with the root's name.
    #[test]
    fn tells_the_keymap_settings_from_others() {
        assert!(is_under("keymap", "keymap"));
        assert!(is_under("keymapOverrides.file.save", "keymapOverrides"));
        assert!(!is_under("keymapOverrides", "keymap"));
        assert!(!is_under("appearanceX", "appearance"));
        assert!(is_under("appearance.theme", "appearance"));
    }
}
