//! The application's own actions that act on a window: project trust (the Restricted Mode banner
//! and the question asked before an undecided project opens), Window › Tool Window Layouts,
//! Distraction Free Mode, Help › About, File › New Project…, Invalidate Caches… and Restart IDE,
//! Manage IDE Settings › Export / Import Settings…, and Power Save Mode.
//!
//! The dialogs are the views of [`crate::app_views`]; they are shown in the window's dialog slot
//! (the one a Git operation's questions use), one at a time. Backend work runs on the backend's
//! runtime and comes back to the window. What is application-wide in the IDE is a global here:
//! the saved layouts (in the UI storage), the Distraction Free Mode flag, the questions waiting for
//! a trust decision, and what restarts the app.

use super::app_shell;
use super::branches::call;
use super::menu_spec::DynamicItem;
use super::plugin_runtime::plugin_host;
use super::services_window::on_workbench;
use super::tool_windows::{NAMED_LAYOUTS_STORAGE_KEY, NamedLayouts, ToolWindowLayout};
use super::{Workbench, for_window};
use crate::app_models::invalidate_caches::{CachesEffect, schedule_failed};
use crate::app_models::project_actions::{CreateProject, CreateStep, exported_storage, initial_location, is_exported};
use crate::app_models::windows::open_failed_notice;
use crate::app_views::about_dialog::{AboutDialog, AboutEvent, KEYMAP_REFERENCE, SHOW_LOG, this_build};
use crate::app_views::invalidate_caches_dialog::{InvalidateCachesDialog, InvalidateCachesEvent};
use crate::app_views::new_project_dialog::{NewProjectDialog, NewProjectEvent};
use crate::app_views::trust_dialog::{TrustDialog, TrustDialogEvent};
use crate::app_views::{self, tx};
use crate::backend::Backend;
use crate::platform::actions::{Action, ActionMeta, ActionRegistry, ActionSource, execute_action_from, manifest};
use crate::platform::context::set_context;
use crate::platform::i18n::{Localization, Param};
use crate::platform::input;
use crate::platform::keymap::is_mac_host;
use crate::platform::settings;
use crate::platform::storage::UiStorage;
use crate::platform_more::distraction_free::{
    DISTRACTION_FREE_ACTION, DISTRACTION_FREE_CONTEXT, DISTRACTION_FREE_HINT_GROUP, DISTRACTION_FREE_HINT_KEY,
    DISTRACTION_FREE_HINT_TITLE, DISTRACTION_FREE_STORAGE_KEY, DistractionFree, Effect, distraction_free_hint,
    distraction_free_title, exit_hint,
};
use crate::theme;
use crate::vcs::prompt_dialogs::{FieldRule, OK, PromptDialog};
use crate::vcs_ops::dialogs::{AskButton, AskRequest, ConfirmRequest, PromptRequest};
use crate::vcs_ops::menu::MenuItem;
use crate::workbench::notifications::{NotificationAction, NotificationLevel, NotificationRequest};
use futures::channel::oneshot;
use gpui_kit::{
    AnyElement, App, AsyncWindowContext, ClickEvent, Context, Entity, Focusable, Global, IntoElement,
    PathPromptOptions, SharedString, WeakEntity, Window, WindowId, div, prelude::*, px,
};
use ide_core::IdeError;
use ide_host::plugins::PluginHost;
use ide_workspace::settings::SettingsScope;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

type This = WeakEntity<Workbench>;

/// The notification key of Export Settings….
const EXPORT_KEY: &str = "app.settingsExport";
/// The notification key of Import Settings….
const IMPORT_KEY: &str = "app.settingsImport";
/// The suggested name of an exported settings file.
const EXPORT_FILE_NAME: &str = "openidea-settings.json";

// -------------------------------------------------------------------------------------------------
// Globals

/// What ends the app and starts it again once every window agreed to close. Without one the app's
/// own runs: [`App::restart`], which has the platform wait for this process to end and launch the
/// app again, then quits. Tests put a recorder in its place, so that nothing is relaunched.
pub struct Relaunch(pub Rc<dyn Fn(&mut App)>);

impl Global for Relaunch {}

fn relaunch(cx: &mut App) {
    match cx.try_global::<Relaunch>().map(|relaunch| Rc::clone(&relaunch.0)) {
        Some(relaunch) => relaunch(cx),
        None => restart_app(cx),
    }
}

#[cfg(not(test))]
fn restart_app(cx: &mut App) {
    cx.restart();
}

/// A test build never restarts or quits the process: a test that reaches here without a
/// [`Relaunch`] recorder fails instead.
#[cfg(test)]
fn restart_app(_: &mut App) {
    panic!("a test asked for a restart without installing a Relaunch recorder");
}

/// What answering a trust question does.
enum TrustAnswer {
    /// Tells an open flow whether the project opens.
    Flow(oneshot::Sender<bool>),
    /// Opens this project in the asking window on Trust Project or Preview in Restricted Mode.
    Open(PathBuf),
}

/// The trust questions waiting for an answer, by the window that asks.
#[derive(Default)]
struct TrustPrompts(HashMap<WindowId, TrustAnswer>);

impl Global for TrustPrompts {}

/// Distraction Free Mode, once read from the UI storage.
#[derive(Default)]
struct ZenMode(Option<DistractionFree>);

impl Global for ZenMode {}

/// The mode, read from the UI storage the first time it is asked for. The toggle action is titled
/// after it from then on.
fn zen_mode(cx: &mut App) -> &mut DistractionFree {
    if cx.try_global::<ZenMode>().is_none_or(|zen| zen.0.is_none()) {
        let stored = cx.try_global::<UiStorage>().and_then(|storage| storage.get(DISTRACTION_FREE_STORAGE_KEY));
        let (mode, effects) = DistractionFree::restore(stored.map(str::to_owned).as_deref(), false);
        for effect in effects {
            if let Effect::Title(title) = effect {
                retitle_zen(title, cx);
            }
        }
        cx.set_global(ZenMode(Some(mode)));
    }
    cx.default_global::<ZenMode>().0.get_or_insert_default()
}

/// Whether Distraction Free Mode is on.
pub(crate) fn zen_active(cx: &App) -> bool {
    match cx.try_global::<ZenMode>().and_then(|zen| zen.0.as_ref()) {
        Some(mode) => mode.is_active(),
        None => cx.try_global::<UiStorage>().and_then(|s| s.get(DISTRACTION_FREE_STORAGE_KEY)) == Some("1"),
    }
}

/// The toggle, titled Enter or Exit Distraction Free Mode.
fn zen_action(title: &str) -> Action {
    let declared = manifest().get(DISTRACTION_FREE_ACTION).cloned();
    let meta = ActionMeta {
        title: title.to_owned(),
        ..declared.unwrap_or_else(|| ActionMeta::new(DISTRACTION_FREE_ACTION, title))
    };
    Action::new(meta, |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.toggle_distraction_free(window, cx))
    })
}

fn retitle_zen(title: &str, cx: &mut App) {
    if cx.has_global::<ActionRegistry>() {
        cx.global_mut::<ActionRegistry>().register(zen_action(title));
    }
}

/// A project's tool window layout was loaded: in Distraction Free Mode its windows are hidden like
/// the others. Whether the hint on how to leave the mode is to be shown now.
pub(super) fn zen_layout_loaded(layout: &mut ToolWindowLayout, cx: &mut App) -> bool {
    let mut hint = false;
    for effect in zen_mode(cx).layout_loaded() {
        match effect {
            // The layout to restore on exit is what is stored meanwhile, so nothing is stored here.
            Effect::EnterLayout => {
                let _ = layout.enter_distraction_free();
            }
            Effect::ShowHint => hint = true,
            _ => {}
        }
    }
    hint
}

/// The saved tool window layouts, as the UI storage holds them.
fn named_layouts(cx: &App) -> NamedLayouts {
    NamedLayouts::parse(cx.try_global::<UiStorage>().and_then(|storage| storage.get(NAMED_LAYOUTS_STORAGE_KEY)))
}

/// Window › Tool Window Layouts: one item per saved layout, restoring it, checked when it is the
/// one last saved or restored. The names are the user's, shown as written.
fn layout_items(layouts: &NamedLayouts) -> Vec<DynamicItem> {
    let active = layouts.active();
    layouts
        .names()
        .into_iter()
        .map(|name| DynamicItem {
            action: "layout.restore".to_owned(),
            arg: Some(name.to_owned()),
            label: name.to_owned(),
            checked: Some(active == Some(name)),
        })
        .collect()
}

/// The items of the menu bar's saved-layouts slot.
pub(super) fn layout_menu_items(cx: &App) -> Vec<DynamicItem> {
    layout_items(&named_layouts(cx))
}

fn store_named_layouts(layouts: &NamedLayouts, cx: &mut App) {
    match serde_json::to_string(layouts) {
        Ok(json) => cx.default_global::<UiStorage>().set(NAMED_LAYOUTS_STORAGE_KEY, json),
        Err(error) => tracing::warn!("cannot store the tool window layouts: {error}"),
    }
}

/// A notification of the settings export or import.
fn settings_notice(
    level: NotificationLevel,
    title: &str,
    body: Option<String>,
    key: &str,
    cx: &mut App,
) -> NotificationRequest {
    let mut request = NotificationRequest::new(level, tx(cx, title));
    request.body = body;
    request.key = Some(key.to_owned());
    request
}

/// Logs how a task of a window ended when it could not finish (its window went first).
fn finished(result: anyhow::Result<()>) {
    if let Err(error) = result {
        tracing::debug!("an application action ended with its window: {error:#}");
    }
}

/// The string argument of an action.
fn text_arg(arg: Option<&Value>) -> Option<String> {
    arg.and_then(Value::as_str).map(str::to_owned)
}

// -------------------------------------------------------------------------------------------------
// The actions

/// Registers the actions this module carries out.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    // Project trust.
    registry.handle("project.trust", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.decide_trust(Some(true), None, window, cx))
    });
    registry.handle("project.previewRestricted", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.decide_trust(Some(false), None, window, cx))
    });
    registry.handle("project.dontOpen", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.finish_trust_prompt(false, window, cx))
    });
    registry.handle("project.revokeTrust", |call, window, cx| {
        let path = text_arg(call.arg).map(PathBuf::from);
        on_workbench(window, cx, |workbench, window, cx| workbench.decide_trust(None, path, window, cx))
    });
    registry.handle("project.removeTrustedLocation", |call, window, cx| {
        let Some(location) = text_arg(call.arg).map(PathBuf::from) else { return Ok(()) };
        on_workbench(window, cx, |workbench, window, cx| workbench.remove_trusted_location(location, window, cx))
    });
    // Window › Tool Window Layouts.
    registry.handle("layout.save", |_, window, cx| on_workbench(window, cx, Workbench::save_layout));
    registry.handle("layout.restore", |call, window, cx| {
        let name = text_arg(call.arg);
        on_workbench(window, cx, |workbench, window, cx| match name {
            Some(name) => workbench.restore_layout(name, window, cx),
            None => workbench.pick_layout("Restore Layout", Workbench::restore_layout, window, cx),
        })
    });
    registry
        .handle("layout.restoreDefault", |_, window, cx| on_workbench(window, cx, Workbench::restore_default_layout));
    registry.handle("layout.rename", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.pick_layout("Rename Layout", Workbench::rename_layout, window, cx)
        })
    });
    registry.handle("layout.delete", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.pick_layout("Delete Layout", Workbench::delete_layout, window, cx)
        })
    });
    // View › Appearance › Enter / Exit Distraction Free Mode: its title follows the mode.
    registry.register(zen_action(distraction_free_title(false)));
    // Help, File.
    registry.handle("app.about", |_, window, cx| on_workbench(window, cx, Workbench::show_about));
    registry.handle("file.newProject", |_, window, cx| on_workbench(window, cx, Workbench::show_new_project));
    registry
        .handle("file.invalidateCaches", |_, window, cx| on_workbench(window, cx, Workbench::show_invalidate_caches));
    registry.handle("app.restart", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.carry_out_caches([CachesEffect::Schedule(None), CachesEffect::Restart], window, cx)
        })
    });
    registry.handle("file.exportSettings", |_, window, cx| on_workbench(window, cx, Workbench::export_settings));
    registry.handle("file.importSettings", |_, window, cx| on_workbench(window, cx, Workbench::import_settings));
    registry.handle_toggle(
        "file.togglePowerSave",
        |_, cx| settings::global(cx).general.power_save_mode,
        |_, window, cx| on_workbench(window, cx, |workbench, _, cx| workbench.toggle_power_save(cx)),
    );
}

impl Workbench {
    /// Publishes `toolWindowLayoutsSaved` (Rename and Delete need a saved layout) and
    /// `distractionFree`.
    pub(super) fn publish_app_context(&mut self, window: &mut Window, cx: &mut App) {
        set_context(window, cx, "toolWindowLayoutsSaved", !named_layouts(cx).is_empty());
        set_context(window, cx, DISTRACTION_FREE_CONTEXT, zen_active(cx));
    }

    // ---------------------------------------------------------------------------------------------
    // Project trust

    /// Whether the window's project runs in Restricted Mode: its commands and tools are off.
    pub fn project_restricted(&self, cx: &App) -> bool {
        self.project_tree().is_some()
            && cx
                .try_global::<Backend>()
                .and_then(|backend| backend.host.project(&self.label).ok())
                .is_some_and(|project| !project.is_trusted())
    }

    /// The trust question, while it is shown.
    pub fn trust_dialog(&self) -> Option<Entity<TrustDialog>> {
        self.vcs_dialog_view()
    }

    /// The banner over a project in Restricted Mode, with Trust Project.
    pub(super) fn restricted_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.project_restricted(cx) {
            return None;
        }
        let text = tx(cx, "Restricted Mode: project commands and tools are disabled.");
        let label = tx(cx, "Trust Project");
        let trust = app_views::button(cx, label, false, true)
            .id("restricted-banner-trust")
            .debug_selector(|| "restricted-banner-trust".to_owned())
            .on_click(cx.listener(|_, _: &ClickEvent, window, cx| {
                window.defer(cx, |window, cx| {
                    execute_action_from(ActionSource::Other, "project.trust", None, window, cx);
                });
            }));
        let banner = div()
            .id("restricted-banner")
            .debug_selector(|| "restricted-banner".to_owned())
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .px(px(16.0))
            .py(px(6.0))
            .bg(theme::color(cx, "bg-elev"))
            .text_color(theme::color(cx, "fg"))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(SharedString::from(text))
            .child(trust);
        Some(banner.into_any_element())
    }

    /// Asks "Trust and Open Project?" for the folder `path`; `answer` says what an answer does. A
    /// question already waiting in this window is answered Don't Open.
    fn show_trust_prompt(&mut self, path: String, answer: TrustAnswer, window: &mut Window, cx: &mut Context<Self>) {
        let asking = window.window_handle().window_id();
        if let Some(TrustAnswer::Flow(previous)) = cx.default_global::<TrustPrompts>().0.insert(asking, answer) {
            // That question is gone: nothing opens for it.
            let _ = previous.send(false);
        }
        let dialog = cx.new(|cx| TrustDialog::new(path, window, cx));
        let events = cx.subscribe_in(&dialog, window, |_, _, event: &TrustDialogEvent, window, cx| {
            let TrustDialogEvent::Run(action) = *event;
            // Once this event is over: the action reaches for the window.
            window.defer(cx, move |window, cx| {
                execute_action_from(ActionSource::Other, action, None, window, cx);
            });
        });
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.into(), focus, events, window, cx);
    }

    /// Answers the trust question of this window, if one waits: `open` or not. A project this
    /// window waits to open opens in this same update, so that whoever waits for the opening never
    /// sees the window without a question and without an opening.
    fn finish_trust_prompt(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.trust_dialog().is_some() {
            self.close_vcs_dialog(window, cx);
        }
        let asking = window.window_handle().window_id();
        match cx.default_global::<TrustPrompts>().0.remove(&asking) {
            // The opening may have ended meanwhile.
            Some(TrustAnswer::Flow(sender)) => {
                let _ = sender.send(open);
            }
            Some(TrustAnswer::Open(root)) if open => self.open_project(root, window, cx),
            Some(TrustAnswer::Open(_)) | None => {}
        }
    }

    /// Decides the trust of the folder the trust question asks about, else of `path`, else of the
    /// window's project: `Some(true)` trusts it (with its parent folder when the question's
    /// checkbox says so), `Some(false)` previews it in Restricted Mode, `None` revokes the trust.
    /// The question, if any, is answered once the decision is stored.
    fn decide_trust(
        &mut self,
        trusted: Option<bool>,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prompt = self.trust_dialog().filter(|_| trusted.is_some());
        let (target, folder) = match (&prompt, path) {
            (Some(dialog), _) => {
                let dialog = dialog.read(cx);
                (PathBuf::from(dialog.path()), trusted == Some(true) && dialog.folder())
            }
            (None, Some(path)) => (path, false),
            (None, None) => match self.project_root(cx) {
                Some(root) => (root, false),
                None => return,
            },
        };
        let trusted = trusted == Some(true);
        let prompted = prompt.is_some();
        let plugins = plugin_host(cx);
        cx.spawn_in(window, async move |this, cx| {
            finished(set_trust(&this, cx, target, trusted, folder, prompted, plugins).await);
        })
        .detach();
    }

    /// Project Trust › Remove: projects inside `location` are no longer trusted for being there,
    /// and the project plugins of every project that lost its trust are unloaded.
    fn remove_trusted_location(&mut self, location: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let plugins = plugin_host(cx);
        cx.spawn_in(window, async move |this, cx| {
            let removed = call(&this, cx, move |host, _| async move {
                host.trust.remove_location(&location)?;
                host.refresh_project_trust().await?;
                if let Some(plugins) = plugins {
                    for project in host.projects().into_iter().filter(|project| !project.is_trusted()) {
                        plugins.restrict_project(&project.root);
                    }
                }
                ide_core::IdeResult::Ok(())
            })
            .await;
            match removed {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::error!("cannot remove the trusted location: {error}"),
                Err(error) => tracing::debug!("removing a trusted location ended with its window: {error:#}"),
            }
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // Tool window layouts

    /// Save Current Layout as…: asks for a name, the last saved or restored one first.
    fn save_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = named_layouts(cx).active().unwrap_or_default().to_owned();
        let request = PromptRequest {
            title: "Save Current Layout as".to_owned(),
            label: Some("Layout name:".to_owned()),
            initial: active,
            ok_label: "Save".to_owned(),
            ..PromptRequest::default()
        };
        let rule = FieldRule::NonEmpty("Enter a layout name".to_owned());
        let answer =
            self.show_prompt_dialog(move |window, cx| PromptDialog::prompt(request, rule, window, cx), window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(result) = answer.await.ok().and_then(|answer| answer.prompt_result()) else { return };
            let _ = this.update(cx, |workbench, cx| workbench.store_layout_as(&result.value, cx));
        })
        .detach();
    }

    fn store_layout_as(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(snapshot) = self.tool_windows().map(ToolWindowLayout::snapshot) else { return };
        let mut layouts = named_layouts(cx);
        if layouts.save(name, snapshot) {
            store_named_layouts(&layouts, cx);
            cx.notify();
        }
    }

    /// Lets the user choose a saved layout, then goes on with `then`. Without one, says so.
    fn pick_layout(
        &mut self,
        title: &'static str,
        then: fn(&mut Workbench, String, &mut Window, &mut Context<Workbench>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let names: Vec<String> = named_layouts(cx).names().into_iter().map(str::to_owned).collect();
        if names.is_empty() {
            let request = AskRequest {
                title: title.to_owned(),
                body: "No saved layouts. Use Save Current Layout as… first.".to_owned(),
                buttons: vec![AskButton { id: OK, label: "OK", primary: true }],
                ..AskRequest::default()
            };
            // The message only informs: its answer changes nothing.
            drop(self.show_prompt_dialog(move |window, cx| PromptDialog::ask(request, window, cx), window, cx));
            return;
        }
        let items: Vec<MenuItem<String>> =
            names.into_iter().map(|name| MenuItem::entry(name.clone(), Some(name))).collect();
        let position = gpui_kit::point(px(f32::from(window.viewport_size().width) / 2.0 - 120.0), px(80.0));
        self.show_menu(&items, position, then, window, cx);
    }

    /// Restore Tool Window Layout: the layout saved as `name`, leaving Distraction Free Mode first.
    fn restore_layout(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.set_distraction_free(false, window, cx);
        let mut layouts = named_layouts(cx);
        let Some(snapshot) = layouts.restore(&name) else { return };
        store_named_layouts(&layouts, cx);
        self.with_layout(window, cx, |layout| layout.apply(snapshot));
        cx.notify();
    }

    /// Restore Default Layout, leaving Distraction Free Mode first.
    fn restore_default_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_distraction_free(false, window, cx);
        let mut layouts = named_layouts(cx);
        let snapshot = layouts.restore_default();
        store_named_layouts(&layouts, cx);
        self.with_layout(window, cx, |layout| layout.apply(snapshot));
        cx.notify();
    }

    /// Rename Tool Window Layout…: asks for the new name of `from`.
    fn rename_layout(&mut self, from: String, window: &mut Window, cx: &mut Context<Self>) {
        let request = PromptRequest {
            title: "Rename Layout".to_owned(),
            label: Some("New name:".to_owned()),
            initial: from.clone(),
            ok_label: "Rename".to_owned(),
            ..PromptRequest::default()
        };
        let rule = FieldRule::NonEmpty("Enter a layout name".to_owned());
        let answer =
            self.show_prompt_dialog(move |window, cx| PromptDialog::prompt(request, rule, window, cx), window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(result) = answer.await.ok().and_then(|answer| answer.prompt_result()) else { return };
            let _ = this.update_in(cx, |workbench, window, cx| {
                let mut layouts = named_layouts(cx);
                if layouts.rename(&from, &result.value) {
                    store_named_layouts(&layouts, cx);
                    cx.notify();
                    return;
                }
                // Another layout has the name: nothing is renamed, and the user is told why.
                let request = AskRequest {
                    title: "Rename Layout".to_owned(),
                    body: "A layout with this name exists".to_owned(),
                    warning: true,
                    buttons: vec![AskButton { id: OK, label: "OK", primary: true }],
                    ..AskRequest::default()
                };
                drop(workbench.show_prompt_dialog(
                    move |window, cx| PromptDialog::ask(request, window, cx),
                    window,
                    cx,
                ));
            });
        })
        .detach();
    }

    /// Delete Tool Window Layout…: once confirmed.
    fn delete_layout(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let message = cx
            .default_global::<Localization>()
            .0
            .tx_built(&["Delete the layout '", "'?"], &[Param::Text(name.clone())]);
        let request = ConfirmRequest::danger("Delete Layout", message, "Delete");
        let answer = self.show_prompt_dialog(move |window, cx| PromptDialog::confirm(request, window, cx), window, cx);
        cx.spawn_in(window, async move |this, cx| {
            if !answer.await.is_ok_and(|answer| answer.confirmed()) {
                return;
            }
            let _ = this.update(cx, |_, cx| {
                let mut layouts = named_layouts(cx);
                layouts.delete(&name);
                store_named_layouts(&layouts, cx);
                cx.notify();
            });
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // Distraction Free Mode

    /// Whether the window is in Distraction Free Mode: a project's frame without its tool windows,
    /// stripes, main toolbar contents and status bar.
    pub fn distraction_free(&self, cx: &App) -> bool {
        self.project_tree().is_some() && zen_active(cx)
    }

    /// Whether the tool window stripes show: View › Appearance › Tool Window Bars, outside
    /// Distraction Free Mode.
    pub fn shows_tool_window_bars(&self, cx: &App) -> bool {
        settings::global(cx).appearance.show_tool_window_bars && !self.distraction_free(cx)
    }

    fn toggle_distraction_free(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = zen_mode(cx).is_active();
        self.set_distraction_free(!active, window, cx);
    }

    /// Turns the mode on or off in every window; setting it to what it is does nothing.
    fn set_distraction_free(&mut self, active: bool, window: &mut Window, cx: &mut Context<Self>) {
        let layout_loaded = self.tool_windows().is_some_and(|layout| !layout.scope().is_empty());
        let effects = zen_mode(cx).set(active, layout_loaded);
        let here = window.window_handle().window_id();
        // The other windows, without reading their workbenches: this one is being updated.
        let others: Vec<_> = cx.windows().into_iter().filter(|handle| handle.window_id() != here).collect();
        for effect in effects {
            match effect {
                Effect::Title(title) => retitle_zen(title, cx),
                Effect::SetContext(on) => set_context(window, cx, DISTRACTION_FREE_CONTEXT, on),
                Effect::Store(true) => {
                    cx.default_global::<UiStorage>().set(DISTRACTION_FREE_STORAGE_KEY, "1".to_owned())
                }
                Effect::Store(false) => cx.default_global::<UiStorage>().remove(DISTRACTION_FREE_STORAGE_KEY),
                Effect::EnterLayout | Effect::ExitLayout => {
                    let enter = effect == Effect::EnterLayout;
                    self.distraction_free_layout(enter, window, cx);
                    for handle in &others {
                        // A window that is gone has no layout to change.
                        let _ = handle.update(cx, |_, window, cx| {
                            if let Some(workbench) = for_window(window, cx) {
                                workbench
                                    .update(cx, |workbench, cx| workbench.distraction_free_layout(enter, window, cx));
                            }
                        });
                    }
                }
                Effect::ShowHint => self.show_distraction_free_hint(cx),
                Effect::ExpireHint => {
                    self.notifications.expire_notification(DISTRACTION_FREE_HINT_KEY);
                    for handle in &others {
                        let _ = handle.update(cx, |_, window, cx| {
                            if let Some(workbench) = for_window(window, cx) {
                                workbench.update(cx, |workbench, cx| {
                                    workbench.notifications.expire_notification(DISTRACTION_FREE_HINT_KEY);
                                    cx.notify();
                                });
                            }
                        });
                    }
                }
            }
        }
        cx.notify();
    }

    /// Hides the tool windows remembering the shown ones, or brings those back.
    fn distraction_free_layout(&mut self, enter: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.with_layout(window, cx, |layout| {
            if enter { layout.enter_distraction_free() } else { layout.exit_distraction_free() }
        });
        cx.notify();
    }

    /// How to leave the mode, with Exit Distraction Free Mode, in a group of its own (so that its
    /// Don't Show Again turns off this hint only).
    pub(super) fn show_distraction_free_hint(&mut self, cx: &mut Context<Self>) {
        let body = match cx.try_global::<input::Keymap>() {
            Some(keymap) => distraction_free_hint(&keymap.0, is_mac_host()),
            None => exit_hint(None, None),
        };
        let mut request = NotificationRequest::new(NotificationLevel::Info, tx(cx, DISTRACTION_FREE_HINT_TITLE));
        request.body = Some(body);
        request.key = Some(DISTRACTION_FREE_HINT_KEY.to_owned());
        request.group = Some(DISTRACTION_FREE_HINT_GROUP.to_owned());
        request.actions = vec![NotificationAction {
            label: tx(cx, distraction_free_title(true)),
            command: DISTRACTION_FREE_ACTION.to_owned(),
        }];
        self.notify(request, cx);
    }

    // ---------------------------------------------------------------------------------------------
    // About

    /// The About dialog, while it is open.
    pub fn about_dialog(&self) -> Option<Entity<AboutDialog>> {
        self.vcs_dialog_view()
    }

    fn show_about(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let config = cx.try_global::<Backend>().map(|backend| backend.dirs.config.to_string_lossy().into_owned());
        let registry = cx.try_global::<ActionRegistry>();
        let actions = [KEYMAP_REFERENCE, SHOW_LOG]
            .into_iter()
            .filter(|id| registry.is_some_and(|registry| registry.contains(id)))
            .collect();
        let dialog = cx.new(|cx| AboutDialog::new(this_build(), config, actions, window, cx));
        let events = cx.subscribe_in(&dialog, window, |workbench, _, event: &AboutEvent, window, cx| match event {
            AboutEvent::Closed => workbench.close_vcs_dialog(window, cx),
            AboutEvent::RunAction(id) => {
                let id = *id;
                workbench.close_vcs_dialog(window, cx);
                window.defer(cx, move |window, cx| {
                    execute_action_from(ActionSource::Other, id, None, window, cx);
                });
            }
            AboutEvent::RevealConfig(dir) => cx.reveal_path(Path::new(dir)),
            AboutEvent::OpenUrl(url) => cx.open_url(url),
        });
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.into(), focus, events, window, cx);
    }

    // ---------------------------------------------------------------------------------------------
    // New Project

    /// The New Project dialog, while it is open.
    pub fn new_project_dialog(&self) -> Option<Entity<NewProjectDialog>> {
        self.vcs_dialog_view()
    }

    fn show_new_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let preset = settings::effective(&self.label, cx).general.default_project_directory;
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        // Neither: the user's home, as the folder the dialog proposes.
        let location = initial_location(&preset, root.as_deref())
            .or_else(|| dirs::home_dir().map(|home| home.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let dialog = cx.new(|cx| NewProjectDialog::new(&location, window, cx));
        let events = cx.subscribe_in(&dialog, window, |workbench, dialog, event: &NewProjectEvent, window, cx| {
            match event.clone() {
                NewProjectEvent::Cancelled => workbench.close_vcs_dialog(window, cx),
                NewProjectEvent::Browse(_) => workbench.browse_project_location(dialog, window, cx),
                NewProjectEvent::Create { path, git } => {
                    let has_project = workbench.project_tree().is_some();
                    let dialog = dialog.downgrade();
                    cx.spawn_in(window, async move |this, cx| {
                        finished(create_project(&this, cx, &dialog, &path, git, has_project).await);
                    })
                    .detach();
                }
            }
        });
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.into(), focus, events, window, cx);
    }

    /// Browse…: a folder for the New Project dialog. The platform's folder dialog starts where it
    /// decides: it is not told the dialog's location.
    fn browse_project_location(
        &mut self,
        dialog: &Entity<NewProjectDialog>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let options = PathPromptOptions { files: false, directories: true, multiple: false, prompt: None };
        let picked = cx.prompt_for_paths(options);
        let dialog = dialog.downgrade();
        cx.spawn_in(window, async move |this, cx| {
            let folder = match picked.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => {
                    let _ = this.update(cx, |workbench, cx| {
                        let mut request =
                            NotificationRequest::new(NotificationLevel::Error, tx(cx, "Cannot open the file dialog"));
                        request.body = Some(error.to_string());
                        workbench.notify(request, cx);
                    });
                    None
                }
            };
            if let Some(folder) = folder {
                let _ = dialog.update(cx, |dialog, cx| dialog.set_location(&folder.to_string_lossy(), cx));
            }
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // Invalidate Caches, Restart

    /// The Invalidate Caches dialog, while it is open.
    pub fn invalidate_caches_dialog(&self) -> Option<Entity<InvalidateCachesDialog>> {
        self.vcs_dialog_view()
    }

    fn show_invalidate_caches(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = cx.new(|cx| InvalidateCachesDialog::new(window, cx));
        let events = cx.subscribe_in(&dialog, window, |workbench, _, event: &InvalidateCachesEvent, window, cx| {
            workbench.close_vcs_dialog(window, cx);
            if let InvalidateCachesEvent::Run(effects) = event {
                workbench.carry_out_caches(*effects, window, cx);
            }
        });
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.into(), focus, events, window, cx);
    }

    /// Schedules the invalidation (none cancels a pending one) in the data folder, then restarts
    /// the app the way it quits: every window decides its close first. A failure is reported and
    /// nothing restarts.
    fn carry_out_caches(&mut self, effects: [CachesEffect; 2], window: &mut Window, cx: &mut Context<Self>) {
        let schedule = effects.iter().find_map(|effect| match effect {
            CachesEffect::Schedule(options) => Some(*options),
            CachesEffect::Restart => None,
        });
        let restart = effects.contains(&CachesEffect::Restart);
        let data = cx.try_global::<Backend>().map(|backend| backend.dirs.data.clone());
        cx.spawn_in(window, async move |this, cx| {
            let scheduled = match (data, schedule) {
                (None, _) => Err("The backend is not running".to_owned()),
                (Some(_), None) => Ok(()),
                (Some(data), Some(options)) => {
                    let work = async move {
                        match options {
                            Some(options) => {
                                ide_workspace::caches::schedule(&data, &options).map_err(|error| error.message)
                            }
                            None => {
                                ide_workspace::caches::cancel(&data);
                                Ok(())
                            }
                        }
                    };
                    cx.background_executor().spawn(work).await
                }
            };
            let _ = this.update_in(cx, |workbench, window, cx| match scheduled {
                Ok(()) if restart => app_shell::quit_then(window, cx, relaunch),
                Ok(()) => {}
                Err(error) => {
                    let notice = schedule_failed(&error, &cx.default_global::<Localization>().0);
                    workbench.notify(notice, cx);
                }
            });
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // Settings

    /// Export Settings…: the global settings and the IDE-level UI state, to a file the user names.
    fn export_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let storage: BTreeMap<String, String> = cx
            .try_global::<UiStorage>()
            .map(|storage| exported_storage(storage.entries()).into_iter().collect())
            .unwrap_or_default();
        let start = dirs::home_dir().unwrap_or_default();
        let picked = cx.prompt_for_new_path(&start, Some(EXPORT_FILE_NAME));
        cx.spawn_in(window, async move |this, cx| {
            let path = match picked.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    let _ = this.update(cx, |workbench, cx| {
                        let notice = settings_notice(
                            NotificationLevel::Error,
                            "Cannot export settings",
                            Some(error.to_string()),
                            EXPORT_KEY,
                            cx,
                        );
                        workbench.notify(notice, cx);
                    });
                    return;
                }
            };
            let target = path.clone();
            let exported =
                call(&this, cx, move |host, _| async move { host.global_settings.export_bundle(&target, storage) })
                    .await;
            let _ = this.update(cx, |workbench, cx| {
                let notice = match exported {
                    Ok(Ok(())) => {
                        let body = Some(path.to_string_lossy().into_owned());
                        settings_notice(NotificationLevel::Info, "Settings exported", body, EXPORT_KEY, cx)
                    }
                    Ok(Err(error)) => {
                        settings_notice(NotificationLevel::Error, "Cannot export settings", Some(error), EXPORT_KEY, cx)
                    }
                    Err(error) => settings_notice(
                        NotificationLevel::Error,
                        "Cannot export settings",
                        Some(format!("{error:#}")),
                        EXPORT_KEY,
                        cx,
                    ),
                };
                workbench.notify(notice, cx);
            });
        })
        .detach();
    }

    /// Import Settings…: replaces the global settings with the file's and writes its UI state back;
    /// every window re-reads the settings. Templates, macros, layouts and view options apply after
    /// a restart, which the notification offers.
    fn import_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let options = PathPromptOptions { files: true, directories: false, multiple: false, prompt: None };
        let picked = cx.prompt_for_paths(options);
        cx.spawn_in(window, async move |this, cx| {
            let path = match picked.await {
                Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                    Some(path) => path,
                    None => return,
                },
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    let _ = this.update(cx, |workbench, cx| {
                        let notice = settings_notice(
                            NotificationLevel::Error,
                            "Cannot import settings",
                            Some(error.to_string()),
                            IMPORT_KEY,
                            cx,
                        );
                        workbench.notify(notice, cx);
                    });
                    return;
                }
            };
            let imported = call(&this, cx, move |host, _| async move {
                let (_, storage) = host.global_settings.import_bundle(&path)?;
                for project in host.projects() {
                    let effective = project.settings.reload();
                    let defaults = ide_workspace::FileDefaults::from_settings(&effective, Some(&project.root));
                    project.documents.set_file_defaults(defaults);
                    project.lsp.reconfigure().await;
                }
                host.ssh.set_configs(host.global_settings.effective().ssh.configs);
                host.apply_proxy();
                ide_core::IdeResult::Ok(storage)
            })
            .await;
            let _ = this.update(cx, |workbench, cx| {
                let storage = match imported {
                    Ok(Ok(storage)) => storage,
                    Ok(Err(error)) => {
                        let notice = settings_notice(
                            NotificationLevel::Error,
                            "Cannot import settings",
                            Some(error),
                            IMPORT_KEY,
                            cx,
                        );
                        workbench.notify(notice, cx);
                        return;
                    }
                    Err(error) => {
                        let body = Some(format!("{error:#}"));
                        let notice =
                            settings_notice(NotificationLevel::Error, "Cannot import settings", body, IMPORT_KEY, cx);
                        workbench.notify(notice, cx);
                        return;
                    }
                };
                let ui = cx.default_global::<UiStorage>();
                for (key, value) in storage.into_iter().filter(|(key, _)| is_exported(key)) {
                    ui.set(&key, value);
                }
                if let Some(backend) = cx.try_global::<Backend>() {
                    // Every window reads the settings again.
                    backend.emit(None, ide_core::IdeEvent::SettingsChanged { key: String::new() });
                }
                let body = Some(tx(cx, "Restart to apply imported templates, macros, layouts and view options."));
                let mut notice = settings_notice(NotificationLevel::Info, "Settings imported", body, IMPORT_KEY, cx);
                notice.sticky = Some(true);
                notice.actions =
                    vec![NotificationAction { label: tx(cx, "Restart"), command: "app.restart".to_owned() }];
                workbench.notify(notice, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// File › Power Save Mode: flips `general.powerSaveMode` globally.
    fn toggle_power_save(&mut self, cx: &mut Context<Self>) {
        let on = !settings::global(cx).general.power_save_mode;
        if let Err(error) =
            settings::set(&self.label, SettingsScope::Global, "general.powerSaveMode", &Value::Bool(on), cx)
        {
            let mut request =
                NotificationRequest::new(NotificationLevel::Error, tx(cx, "Cannot change Power Save Mode"));
            request.body = Some(error.message);
            self.notify(request, cx);
        }
        cx.notify();
    }
}

/// Asks "Trust and Open Project?" in this window before the folder `path` opens, unless its trust
/// was decided already (trusted, or previewed in Restricted Mode before): whether it opens, or why
/// its trust could not be read. `Err` only when the window went away.
pub(super) async fn confirm_project_trust(
    this: &This,
    cx: &mut AsyncWindowContext,
    path: &Path,
) -> anyhow::Result<Result<bool, IdeError>> {
    let asked = path.to_path_buf();
    let status = match call(this, cx, move |host, _| async move { host.trust.status(&asked) }).await? {
        Ok(status) => status,
        Err(error) => return Ok(Err(IdeError::internal(error))),
    };
    if status.decided {
        return Ok(Ok(true));
    }
    let root = status.root.into_path_buf().to_string_lossy().into_owned();
    let (sender, answer) = oneshot::channel();
    let answered = TrustAnswer::Flow(sender);
    this.update_in(cx, |workbench, window, cx| workbench.show_trust_prompt(root, answered, window, cx))?;
    // A question dropped without an answer opens nothing.
    Ok(Ok(answer.await.unwrap_or(false)))
}

impl Workbench {
    /// Opens the project at `root` in this window once its trust is decided: a decided folder
    /// opens at once; otherwise "Trust and Open Project?" is shown at once and the project opens on
    /// Trust Project or Preview in Restricted Mode, while Don't Open leaves the window as it is. A
    /// window opened for a project (a new window chosen for it, File › Open in New Window, the
    /// folder given on the command line) asks here, in the window the project opens in. The trust
    /// is read here, on the main thread, as the open flow reads it before attaching: the store is
    /// in memory and the read resolves the folder's links only. Deciding synchronously keeps a
    /// caller that waits for the opening (files to open once it is done) waiting for the answer.
    pub(super) fn open_project_asking(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let status = match cx.try_global::<Backend>() {
            Some(backend) => backend.host.trust.status(&root),
            None => Err(IdeError::internal("The backend is not running")),
        };
        let status = match status {
            Ok(status) if status.decided => return self.open_project(root, window, cx),
            Ok(status) => status,
            Err(error) => {
                let shown = root.to_string_lossy();
                let notice = open_failed_notice(&shown, &error, &cx.default_global::<Localization>().0);
                if let Some(notice) = notice {
                    self.notify(notice, cx);
                }
                return;
            }
        };
        let asked = status.root.into_path_buf().to_string_lossy().into_owned();
        self.show_trust_prompt(asked, TrustAnswer::Open(root), window, cx);
    }
}

/// Stores a trust decision, then answers the question that asked for it. A project that is not
/// trusted any more has its project plugins unloaded.
async fn set_trust(
    this: &This,
    cx: &mut AsyncWindowContext,
    path: PathBuf,
    trusted: bool,
    folder: bool,
    prompted: bool,
    plugins: Option<Arc<PluginHost>>,
) -> anyhow::Result<()> {
    let shown = path.display().to_string();
    let decided = call(this, cx, move |host, _| async move {
        host.set_project_trust(&path, trusted, folder).await?;
        if !trusted && let Some(plugins) = plugins {
            // The folder as the trust store names it, links resolved.
            let root = host.trust.status(&path).map(|status| status.root.into_path_buf()).unwrap_or(path);
            plugins.restrict_project(&root);
        }
        ide_core::IdeResult::Ok(())
    })
    .await?;
    match decided {
        // The project's windows hear of it as `ProjectTrustChanged`, and redraw.
        Ok(()) if prompted => {
            this.update_in(cx, |workbench, window, cx| workbench.finish_trust_prompt(true, window, cx))?
        }
        Ok(()) => {}
        // The question stays open: nothing was decided.
        Err(error) => tracing::error!("cannot change the trust of {shown}: {error}"),
    }
    Ok(())
}

/// Creates the folder of a new project as [`CreateProject`] says, then opens it (in a new window
/// when this one has a project). A failure stays in the dialog.
async fn create_project(
    this: &This,
    cx: &mut AsyncWindowContext,
    dialog: &WeakEntity<NewProjectDialog>,
    path: &str,
    git: bool,
    has_project: bool,
) -> anyhow::Result<()> {
    let (mut flow, mut step) = CreateProject::start(path, git, has_project);
    loop {
        step = match step {
            CreateStep::ReadDir(dir) => {
                let listed = call(this, cx, move |host, label| async move {
                    ide_host::ops::fs::read_dir(&host, &label, PathBuf::from(dir)).await
                })
                .await?;
                // A folder that cannot be listed is one to create.
                flow.listed(listed.ok().map(|entries| entries.len()))
            }
            CreateStep::CreateDir(dir) => {
                let created = call(this, cx, move |_, _| async move { ide_vfs::create_dir(Path::new(&dir)) }).await?;
                flow.created(created)
            }
            CreateStep::GitInit(dir) => {
                let initialized = call(this, cx, move |host, _| async move {
                    ide_host::ops::vcs::vcs_init(&host, "", PathBuf::from(dir)).await
                })
                .await?;
                flow.created(initialized)
            }
            CreateStep::Open { path, new_window } => {
                this.update_in(cx, |workbench, window, cx| {
                    workbench.close_vcs_dialog(window, cx);
                    // Once this update is over: opening reads the window's workbench.
                    window.defer(cx, move |window, cx| open_created(&path, new_window, window, cx));
                })?;
                return Ok(());
            }
            CreateStep::Failed(error) => {
                dialog.update(cx, |dialog, cx| dialog.set_busy(false, Some(error), cx))?;
                return Ok(());
            }
        };
    }
}

/// Opens the project just created, here or in a new window; a failure is reported in this window.
fn open_created(path: &str, new_window: bool, window: &mut Window, cx: &mut App) {
    let Err(error) = app_shell::open_project_explicit(Path::new(path), new_window, window, cx) else { return };
    let Some(workbench) = for_window(window, cx) else { return };
    workbench.update(cx, |workbench, cx| {
        let mut request = NotificationRequest::new(NotificationLevel::Error, tx(cx, "Cannot open the project"));
        request.body = Some(format!("{error:#}"));
        workbench.notify(request, cx);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::actions::execute_action_or_throw;
    use crate::workbench::tests::{click_on, notification_titles, run_action, start, wait_long};
    use crate::workbench::windows::{self, Target};
    use crate::workbench::{RUN_TOOL_WINDOW, Workbenches};
    use gpui_kit::{AnyWindowHandle, TestAppContext, VisualTestContext};
    use std::cell::Cell;

    /// A project of `files` (path, text) in an active window, trusted when `trusted` says so.
    fn project<'a>(
        cx: &'a mut TestAppContext,
        trusted: bool,
        files: &[(&str, &str)],
    ) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext) {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        for (path, text) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder is made");
            std::fs::write(path, text).expect("the file is written");
        }
        if trusted {
            cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("trusted");
        }
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        wait_long(cx, "the project opens", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        (folder, root, workbench, cx)
    }

    fn close(cx: &mut VisualTestContext) {
        cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
    }

    fn restricted(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
        workbench.read_with(cx, |w, cx| w.project_restricted(cx))
    }

    fn context_is_on(cx: &mut VisualTestContext, key: &str) -> bool {
        cx.update(|window, cx| crate::platform::context::window_context(window, cx).is_truthy(key))
    }

    /// The trust store's answer for `path`: (decided, trusted).
    fn trust_of(path: &Path, cx: &mut VisualTestContext) -> (bool, bool) {
        let status = cx.update(|_, cx| cx.global::<Backend>().host.trust.status(path)).expect("the status reads");
        (status.decided, status.trusted)
    }

    /// The text field of the window's prompt dialog gets `text` in place of what it holds.
    fn type_in_prompt(workbench: &Entity<Workbench>, text: &str, cx: &mut VisualTestContext) {
        let prompt = workbench.read_with(cx, |w, _| w.vcs_dialog_view::<PromptDialog>()).expect("a prompt is open");
        let length = prompt.read_with(cx, |prompt, cx| prompt.field().map(|f| f.read(cx).text().chars().count()));
        for _ in 0..length.unwrap_or(0) {
            cx.simulate_keystrokes("backspace");
        }
        cx.simulate_input(text);
        cx.run_until_parked();
    }

    // src/features/project-trust/trust.test.tsx › updates the running project's banner after a backend trust
    // transition
    #[gpui_kit::test]
    async fn updates_the_running_projects_banner_after_a_backend_trust_transition(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = project(cx, false, &[("main.rs", "fn main() {}\n")]);
        assert!(restricted(&workbench, cx), "an undecided project opens in Restricted Mode");
        assert!(cx.debug_bounds("restricted-banner").is_some(), "the banner is drawn");
        let execution = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| cx.global::<Backend>().host.project("w1").map(|p| p.require_trust().is_ok()))
                .unwrap_or(false)
        };
        assert!(!execution(cx), "project commands and tools are off");
        click_on(cx, "restricted-banner-trust");
        wait_long(cx, "the project is trusted", |cx| !restricted(&workbench, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("restricted-banner").is_none(), "the banner is gone");
        assert!(execution(cx), "language servers and tools may run now");
        assert_eq!(trust_of(&root, cx), (true, true));
        close(cx);
    }

    /// An empty, active window: what a user sees before opening a project.
    fn empty_window(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        (workbench, cx)
    }

    /// A new folder nobody decided about, with a file in it.
    fn undecided_folder() -> (tempfile::TempDir, PathBuf) {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("main.rs"), "fn main() {}\n").expect("the file is written");
        (folder, root)
    }

    /// Opens `path` the way the user does (Open, Open Recent, the Welcome screen), through the
    /// window's open flow.
    fn open_through_the_flow(path: &Path, cx: &mut VisualTestContext) {
        run_action(cx, "file.openProjectPath", Some(Value::from(path.to_string_lossy().into_owned())));
    }

    /// The folder the window's trust question asks about, while one is shown.
    fn asked_about(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
        let dialog = workbench.read_with(cx, |w, _| w.trust_dialog())?;
        Some(dialog.read_with(cx, |dialog, _| dialog.path().to_owned()))
    }

    fn shown_root(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<PathBuf> {
        workbench.read_with(cx, |w, cx| w.project_root(cx))
    }

    // src/features/project-trust/trust.test.tsx › offers three choices, cancels without a trust decision, and
    // supports restricted preview
    #[gpui_kit::test]
    async fn offers_three_choices_cancels_without_a_trust_decision_and_supports_restricted_preview(
        cx: &mut TestAppContext,
    ) {
        let _home = start(cx);
        let (_folder, root) = undecided_folder();
        let (workbench, cx) = empty_window(cx);
        let wanted = root.to_string_lossy().into_owned();

        // Don't Open: nothing opens and nothing is decided.
        open_through_the_flow(&root, cx);
        wait_long(cx, "the question is shown", |cx| asked_about(&workbench, cx).is_some());
        assert_eq!(asked_about(&workbench, cx).as_deref(), Some(wanted.as_str()));
        for selector in ["trust-button-project.previewRestricted", "trust-button-project.dontOpen"] {
            assert!(cx.debug_bounds(selector).is_some(), "{selector} is drawn");
        }
        assert!(cx.debug_bounds("trust-button-project.trust").is_some());
        assert!(shown_root(&workbench, cx).is_none(), "the project waits for the answer");
        click_on(cx, "trust-button-project.dontOpen");
        assert!(asked_about(&workbench, cx).is_none(), "the question is closed");
        assert!(shown_root(&workbench, cx).is_none() && workbench.read_with(cx, |w, _| w.welcome().is_some()));
        assert_eq!(trust_of(&root, cx), (false, false), "Don't Open decides nothing");
        let name = root.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let reported = notification_titles(&workbench, cx).into_iter().any(|title| title.contains(&name));
        assert!(!reported, "a cancelled opening reports nothing");

        // Preview in Restricted Mode: the project opens with its commands and tools off.
        open_through_the_flow(&root, cx);
        wait_long(cx, "the question is shown again", |cx| asked_about(&workbench, cx).is_some());
        click_on(cx, "trust-button-project.previewRestricted");
        wait_long(cx, "the project opens", |cx| shown_root(&workbench, cx).as_deref() == Some(&*root));
        assert!(restricted(&workbench, cx), "it opens in Restricted Mode");
        cx.run_until_parked();
        assert!(cx.debug_bounds("restricted-banner").is_some(), "the banner is drawn");
        assert_eq!(trust_of(&root, cx), (true, false));

        // Decided: the next opening asks nothing.
        run_action(cx, "file.closeProject", None);
        assert!(shown_root(&workbench, cx).is_none());
        open_through_the_flow(&root, cx);
        wait_long(cx, "the project opens without a question", |cx| {
            shown_root(&workbench, cx).as_deref() == Some(&*root)
        });
        assert!(asked_about(&workbench, cx).is_none() && restricted(&workbench, cx));
        close(cx);
    }

    // src/features/project-trust/trust.test.tsx › saves the parent-folder choice and reuses a persisted decision
    #[gpui_kit::test]
    async fn saves_the_parent_folder_choice_and_reuses_a_persisted_decision(cx: &mut TestAppContext) {
        let _home = start(cx);
        let projects = tempfile::tempdir().expect("a temporary folder");
        let parent = projects.path().canonicalize().expect("the folder exists");
        for name in ["new", "other"] {
            std::fs::create_dir(parent.join(name)).expect("the folder is made");
        }
        let (workbench, cx) = empty_window(cx);
        open_through_the_flow(&parent.join("new"), cx);
        wait_long(cx, "the question is shown", |cx| asked_about(&workbench, cx).is_some());
        let named = workbench.read_with(cx, |w, cx| w.trust_dialog().map(|d| d.read(cx).parent().to_owned()));
        assert_eq!(named.as_deref(), Some(&*parent.to_string_lossy()), "the checkbox names the parent folder");
        click_on(cx, "trust-folder");
        click_on(cx, "trust-button-project.trust");
        wait_long(cx, "the project opens", |cx| shown_root(&workbench, cx).as_deref() == Some(&*parent.join("new")));
        assert!(!restricted(&workbench, cx), "a trusted project runs its tools");
        let decisions = cx.update(|_, cx| cx.global::<Backend>().host.trust.decisions()).expect("the decisions read");
        assert!(decisions.locations.contains(&parent), "{:?}", decisions.locations);
        assert!(decisions.projects.contains(&parent.join("new")), "{:?}", decisions.projects);

        // A sibling is trusted for being in that folder: nothing is asked.
        run_action(cx, "file.closeProject", None);
        open_through_the_flow(&parent.join("other"), cx);
        wait_long(cx, "the sibling opens", |cx| shown_root(&workbench, cx).as_deref() == Some(&*parent.join("other")));
        assert!(asked_about(&workbench, cx).is_none(), "nothing is asked");
        assert!(!restricted(&workbench, cx));
        close(cx);
    }

    /// A project that opens in a window of its own (New Window, Open in New Window) is asked about
    /// in that window; the window it was asked from shows nothing new.
    #[gpui_kit::test]
    async fn a_project_opening_in_a_new_window_is_asked_about_in_that_window(cx: &mut TestAppContext) {
        let _home = start(cx);
        // Windows opened through `windows::open_window`, as the app does.
        cx.update(gpui_kit::init);
        let (_folder, root) = undecided_folder();
        let workbench_of = |handle: AnyWindowHandle, cx: &App| -> Option<Entity<Workbench>> {
            cx.global::<Workbenches>().0.get(&handle.window_id())?.upgrade()
        };
        let main = cx.update(|cx| windows::open_window(Target::Empty, cx)).expect("a window opens");
        let request = serde_json::json!({ "path": root.to_string_lossy(), "newWindow": true });
        main.update(cx, |_, window, cx| execute_action_or_throw("file.openProjectPath", Some(&request), window, cx))
            .expect("the window is open")
            .expect("the action runs");
        let mut visual = VisualTestContext::from_window(main, cx);
        wait_long(&mut visual, "the second window asks", |cx| {
            cx.update(|_, cx| {
                windows::windows(cx)
                    .into_iter()
                    .any(|(_, handle)| workbench_of(handle, cx).is_some_and(|w| w.read(cx).trust_dialog().is_some()))
            })
        });
        let (label, second) = cx
            .update(|cx| windows::windows(cx).into_iter().find(|(_, handle)| handle.window_id() != main.window_id()))
            .expect("a second window opened");
        assert_eq!(label, "project-1");
        let first = cx.update(|cx| workbench_of(main, cx)).expect("the first window's workbench");
        assert!(cx.update(|cx| first.read(cx).trust_dialog().is_none()), "the first window asks nothing");

        let mut there = VisualTestContext::from_window(second, cx);
        click_on(&mut there, "trust-button-project.trust");
        let opened = cx.update(|cx| workbench_of(second, cx)).expect("the second window's workbench");
        wait_long(&mut there, "the project opens there", |cx| {
            opened.read_with(cx, |w, cx| w.project_root(cx)).as_deref() == Some(&*root)
        });
        assert!(!opened.read_with(&there, |w, cx| w.project_restricted(cx)));
        assert_eq!(trust_of(&root, &mut there), (true, true));
        assert!(cx.update(|cx| first.read(cx).project_root(cx).is_none()));
        cx.update(|cx| cx.global::<Backend>().host.close_project(&label));
    }

    /// Revoke Trust puts the window's project back in Restricted Mode, and removing the trusted
    /// location a project was trusted for does the same.
    #[gpui_kit::test]
    async fn revoking_trust_or_removing_its_location_restricts_the_project(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = project(cx, true, &[("main.rs", "fn main() {}\n")]);
        assert!(!restricted(&workbench, cx));
        run_action(cx, "project.revokeTrust", None);
        wait_long(cx, "the trust is revoked", |cx| restricted(&workbench, cx));
        assert_eq!(trust_of(&root, cx), (true, false));
        close(cx);
    }

    #[gpui_kit::test]
    async fn removing_a_trusted_location_restricts_the_projects_inside_it(cx: &mut TestAppContext) {
        let _home = start(cx);
        let location = tempfile::tempdir().expect("a temporary folder");
        let parent = location.path().canonicalize().expect("the folder exists");
        let root = parent.join("p");
        std::fs::create_dir(&root).expect("the folder is made");
        std::fs::write(root.join("main.rs"), "fn main() {}\n").expect("the file is written");
        cx.update(|cx| cx.global::<Backend>().host.trust.add_location(&parent)).expect("the location is added");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        wait_long(cx, "the project opens", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        assert!(!restricted(&workbench, cx), "a project in a trusted location is trusted");
        run_action(cx, "project.removeTrustedLocation", Some(Value::from(parent.to_string_lossy().into_owned())));
        wait_long(cx, "the location is removed", |cx| restricted(&workbench, cx));
        close(cx);
    }

    // src/workbench/toolwindows/layouts.test.ts › tool window layouts › saves and restores visible windows and
    // sizes by name; › restores the default layout; › replaces a layout of the same name, renames and deletes
    #[gpui_kit::test]
    async fn saves_and_restores_visible_windows_and_sizes_by_name(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, _root, workbench, cx) = project(cx, true, &[("main.rs", "fn main() {}\n")]);
        let visible =
            |cx: &mut VisualTestContext, id: &str| workbench.read_with(cx, |w, _| w.is_tool_window_visible(id));
        assert!(visible(cx, "project") && !visible(cx, RUN_TOOL_WINDOW));
        assert!(!context_is_on(cx, "toolWindowLayoutsSaved"));
        // Without a saved layout, Restore says so and changes nothing.
        run_action(cx, "layout.restore", None);
        let message = workbench.read_with(cx, |w, _| w.vcs_dialog_view::<PromptDialog>()).expect("a message is shown");
        assert_eq!(message.read_with(cx, |m, _| m.title().to_owned()), "Restore Layout");
        click_on(cx, "prompt-button-ok");

        run_action(cx, "layout.save", None);
        type_in_prompt(&workbench, "Coding", cx);
        click_on(cx, "prompt-button-ok");
        assert!(context_is_on(cx, "toolWindowLayoutsSaved"));
        let names = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| named_layouts(cx).names().into_iter().map(str::to_owned).collect::<Vec<_>>())
        };
        assert_eq!(names(cx), ["Coding"]);

        // The layout changes: Run shows at the bottom, the Project view is hidden.
        run_action(cx, "toolwindow.run", None);
        workbench.update_in(cx, |w, window, cx| w.with_layout(window, cx, |layout| layout.hide("project")));
        assert!(!visible(cx, "project") && visible(cx, RUN_TOOL_WINDOW));
        run_action(cx, "layout.restore", Some(Value::from("Coding")));
        assert!(visible(cx, "project") && !visible(cx, RUN_TOOL_WINDOW), "the saved layout is back");

        run_action(cx, "toolwindow.run", None);
        run_action(cx, "layout.restoreDefault", None);
        assert!(visible(cx, "project") && !visible(cx, RUN_TOOL_WINDOW), "the default layout shows the Project view");
        assert_eq!(cx.update(|_, cx| named_layouts(cx).active().map(str::to_owned)), None);

        // Rename: the only layout is chosen, then named.
        run_action(cx, "layout.rename", None);
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        type_in_prompt(&workbench, "Review", cx);
        click_on(cx, "prompt-button-ok");
        assert_eq!(names(cx), ["Review"]);

        // Delete, once confirmed.
        run_action(cx, "layout.delete", None);
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        click_on(cx, "prompt-button-ok");
        assert!(names(cx).is_empty());
        assert!(!context_is_on(cx, "toolWindowLayoutsSaved"));
        close(cx);
    }

    // src/workbench/distractionFree.test.tsx › distraction free mode › toggles with one action whose title flips
    // Enter / Exit, publishing a context key; › hides tool windows and restores the previous layout on exit;
    // › distraction free shell › hides stripes, status bar and main toolbar contents, keeping the drag region
    #[gpui_kit::test]
    async fn hides_tool_windows_and_restores_the_previous_layout_on_exit(cx: &mut TestAppContext) {
        use crate::workbench::title_bar::TitleBarMode;
        let _home = start(cx);
        let (_folder, root, workbench, cx) = project(cx, true, &[("main.rs", "fn main() {}\n")]);
        workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("main.rs"), window, cx));
        wait_long(cx, "the file opens", |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        let title = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| cx.global::<ActionRegistry>().get(DISTRACTION_FREE_ACTION).map(|a| a.meta.title.clone()))
        };
        let frame = |cx: &mut VisualTestContext| {
            workbench.read_with(cx, |w, cx| {
                (w.shows_status_bar(cx), w.shows_tool_window_bars(cx), w.title_bar().read(cx).mode())
            })
        };
        assert_eq!(title(cx).as_deref(), Some("Enter Distraction Free Mode"));
        assert_eq!(frame(cx), (true, true, TitleBarMode::Full));
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible("project")));

        run_action(cx, DISTRACTION_FREE_ACTION, None);
        assert_eq!(title(cx).as_deref(), Some("Exit Distraction Free Mode"));
        assert!(context_is_on(cx, DISTRACTION_FREE_CONTEXT));
        assert_eq!(frame(cx), (false, false, TitleBarMode::Compact));
        let layout =
            workbench.read_with(cx, |w, _| w.tool_windows().map(|l| (l.visible().is_empty(), l.is_distraction_free())));
        assert_eq!(layout, Some((true, true)), "no tool window shows");
        assert!(notification_titles(&workbench, cx).contains(&"Distraction Free Mode".to_owned()), "the hint shows");
        let stored = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| cx.global::<UiStorage>().get(DISTRACTION_FREE_STORAGE_KEY).map(str::to_owned))
        };
        assert_eq!(stored(cx).as_deref(), Some("1"));

        run_action(cx, DISTRACTION_FREE_ACTION, None);
        assert_eq!(title(cx).as_deref(), Some("Enter Distraction Free Mode"));
        assert!(!context_is_on(cx, DISTRACTION_FREE_CONTEXT));
        assert_eq!(frame(cx), (true, true, TitleBarMode::Full));
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible("project")), "the Project view is back");
        assert_eq!(stored(cx), None);
        close(cx);
    }

    /// Help › About names the version this build was made from.
    #[gpui_kit::test]
    async fn about_shows_the_version_of_the_build(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        run_action(cx, "app.about", None);
        let about = workbench.read_with(cx, |w, _| w.about_dialog()).expect("About is open");
        assert_eq!(about.read_with(cx, |a, _| a.product()), format!("OpenIDEA {}", env!("CARGO_PKG_VERSION")));
        let facts = about.update(cx, |a, cx| a.facts(cx));
        let home = cx.update(|_, cx| cx.global::<Backend>().dirs.config.to_string_lossy().into_owned());
        assert!(facts.contains(&("Configuration".to_owned(), home)), "{facts:?}");
        click_on(cx, "about-close");
        assert!(workbench.read_with(cx, |w, _| w.about_dialog()).is_none());
    }

    /// File › New Project… proposes Settings › Default Project Directory, refuses a folder that
    /// exists and is not empty (the error stays in the dialog, the folder is untouched), creates an
    /// empty one and opens it here, the window having no project: the new folder is undecided, so
    /// its opening asks "Trust and Open Project?" first.
    #[gpui_kit::test]
    async fn new_project_creates_the_folder_and_opens_it(cx: &mut TestAppContext) {
        let _home = start(cx);
        let location = tempfile::tempdir().expect("a temporary folder");
        let parent = location.path().canonicalize().expect("the folder exists");
        std::fs::create_dir(parent.join("taken")).expect("the folder is made");
        std::fs::write(parent.join("taken/a.txt"), "a\n").expect("the file is written");
        let preset = Value::from(parent.to_string_lossy().into_owned());
        cx.update(|cx| settings::set("w1", SettingsScope::Global, "general.defaultProjectDirectory", &preset, cx))
            .expect("the setting is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        run_action(cx, "file.newProject", None);
        let dialog = workbench.read_with(cx, |w, _| w.new_project_dialog()).expect("New Project is open");
        let (name_field, location_field) =
            dialog.read_with(cx, |d, _| (d.name_field().clone(), d.location_field().clone()));
        let proposed = location_field.read_with(cx, |field, _| field.text().to_owned());
        assert_eq!(proposed, parent.to_string_lossy(), "the dialog proposes the default project directory");
        let name = |cx: &mut VisualTestContext, name: &str| {
            name_field.update(cx, |field, cx| field.set_text(name, cx));
            cx.run_until_parked();
        };
        // Git is left out: the backend's git would read this machine's configuration.
        click_on(cx, "new-project-git");
        name(cx, "taken");
        click_on(cx, "new-project-create");
        wait_long(cx, "the error shows", |cx| dialog.read_with(cx, |d, _| d.error().is_some()));
        let error = dialog.read_with(cx, |d, _| d.error().map(str::to_owned)).unwrap_or_default();
        assert_eq!(error, format!("{} already exists and is not empty", parent.join("taken").display()));
        let left: Vec<_> = std::fs::read_dir(parent.join("taken")).expect("readable").flatten().collect();
        assert_eq!(left.len(), 1, "the folder that exists is left as it is");

        name(cx, "demo");
        click_on(cx, "new-project-create");
        let created = parent.join("demo");
        wait_long(cx, "the new folder is asked about", |cx| workbench.read_with(cx, |w, _| w.trust_dialog().is_some()));
        assert!(created.is_dir() && workbench.read_with(cx, |w, _| w.new_project_dialog()).is_none());
        click_on(cx, "trust-button-project.trust");
        wait_long(cx, "the project opens", |cx| {
            workbench.read_with(cx, |w, cx| w.project_root(cx)).is_some_and(|root| root == created)
        });
        assert!(!created.join(".git").exists(), "no repository was asked for");
        let entries = std::fs::read_dir(&parent).expect("readable").flatten().count();
        assert_eq!(entries, 2, "only the new folder was made in the location");
        close(cx);
    }

    /// The relaunches asked for, in place of the app's own.
    fn record_relaunches(cx: &mut VisualTestContext) -> Rc<Cell<u32>> {
        let count: Rc<Cell<u32>> = Rc::default();
        let counted = Rc::clone(&count);
        cx.update(|_, cx| {
            cx.set_global(Relaunch(Rc::new(move |_| counted.set(counted.get() + 1))));
            // The exit is not confirmed: the windows decide their close without asking.
            settings::set("w1", SettingsScope::Global, "general.confirmExit", &Value::Bool(false), cx)
                .expect("the setting is written");
        });
        count
    }

    // src/features/app/__tests__/invalidateCaches.test.tsx › Invalidate Caches › schedules the chosen caches and
    // restarts
    #[gpui_kit::test]
    async fn invalidate_and_restart_schedules_the_chosen_caches_and_restarts(cx: &mut TestAppContext) {
        use ide_workspace::caches::{InvalidateCachesOptions, pending};
        let _home = start(cx);
        let (_folder, _root, workbench, cx) = project(cx, true, &[("main.rs", "fn main() {}\n")]);
        let relaunched = record_relaunches(cx);
        let data = cx.update(|_, cx| cx.global::<Backend>().dirs.data.clone());
        run_action(cx, "file.invalidateCaches", None);
        assert!(workbench.read_with(cx, |w, _| w.invalidate_caches_dialog()).is_some());
        click_on(cx, "invalidate-local-history");
        click_on(cx, "invalidate-and-restart");
        wait_long(cx, "the app restarts", |_| relaunched.get() == 1);
        let chosen = InvalidateCachesOptions { local_history: true, ..InvalidateCachesOptions::default() };
        assert_eq!(pending(&data), Some(chosen), "the invalidation waits for the next start");
        assert!(workbench.read_with(cx, |w, _| w.invalidate_caches_dialog()).is_none());
        assert!(cx.windows().is_empty(), "the window closed before the relaunch");
    }

    // src/features/app/__tests__/invalidateCaches.test.tsx › Invalidate Caches › Just Restart cancels a pending
    // invalidation; Cancel does nothing
    #[gpui_kit::test]
    async fn just_restart_cancels_a_pending_invalidation_cancel_does_nothing(cx: &mut TestAppContext) {
        use ide_workspace::caches::{InvalidateCachesOptions, pending, schedule};
        let _home = start(cx);
        let (_folder, _root, workbench, cx) = project(cx, true, &[("main.rs", "fn main() {}\n")]);
        let relaunched = record_relaunches(cx);
        let data = cx.update(|_, cx| cx.global::<Backend>().dirs.data.clone());
        schedule(&data, &InvalidateCachesOptions::default()).expect("an invalidation is scheduled");
        run_action(cx, "file.invalidateCaches", None);
        click_on(cx, "invalidate-cancel");
        assert!(workbench.read_with(cx, |w, _| w.invalidate_caches_dialog()).is_none());
        assert!(pending(&data).is_some() && relaunched.get() == 0, "Cancel does nothing");
        // Restart IDE is Just Restart.
        run_action(cx, "app.restart", None);
        wait_long(cx, "the app restarts", |_| relaunched.get() == 1);
        assert_eq!(pending(&data), None);
        assert!(cx.windows().is_empty(), "the window closed before the relaunch");
    }

    /// Invalidate Caches deletes, at the next start, only the cache folders under the app's own
    /// directories: everything else in them, and anything outside them, stays.
    #[gpui_kit::test]
    async fn invalidating_every_cache_removes_only_the_apps_cache_folders(cx: &mut TestAppContext) {
        use ide_workspace::caches::apply_pending;
        let home = start(cx);
        let outside = tempfile::tempdir().expect("a temporary folder");
        // No project is open: nothing writes to the caches while they are planted and removed.
        let (workbench, cx) = empty_window(cx);
        let relaunched = record_relaunches(cx);
        let dirs = cx.update(|_, cx| cx.global::<Backend>().dirs.clone());
        let caches = [
            dirs.local_data.join("index"),
            dirs.data.join("history"),
            dirs.local_data.join("lsp"),
            dirs.local_data.join("cache"),
            dirs.data.join("index"),
            dirs.data.join("lsp"),
            dirs.data.join("cache"),
        ];
        for cache in &caches {
            std::fs::create_dir_all(cache).expect("the cache folder is made");
            std::fs::write(cache.join("entry"), "cached\n").expect("the cache entry is written");
        }
        let kept = [
            outside.path().join("index/sentinel"),
            outside.path().join("sentinel"),
            home.path().join("sentinel"),
            dirs.config.join("sentinel"),
            dirs.data.join("sentinel"),
            dirs.local_data.join("tools/sentinel"),
        ];
        for sentinel in &kept {
            std::fs::create_dir_all(sentinel.parent().expect("a folder")).expect("the folder is made");
            std::fs::write(sentinel, "keep\n").expect("the sentinel is written");
        }
        run_action(cx, "file.invalidateCaches", None);
        for option in ["invalidate-local-history", "invalidate-language-servers", "invalidate-build-caches"] {
            click_on(cx, option);
        }
        click_on(cx, "invalidate-and-restart");
        wait_long(cx, "the app restarts", |_| relaunched.get() == 1);
        assert!(workbench.read_with(cx, |w, _| w.invalidate_caches_dialog()).is_none());
        // What the next start does before any project opens.
        let removed = apply_pending(&dirs.data, &dirs.local_data);
        assert_eq!(removed, caches, "exactly the cache folders are removed");
        assert!(caches.iter().all(|cache| !cache.exists()));
        for sentinel in &kept {
            assert_eq!(std::fs::read_to_string(sentinel).ok().as_deref(), Some("keep\n"), "{}", sentinel.display());
        }
        assert!(apply_pending(&dirs.data, &dirs.local_data).is_empty(), "the invalidation runs once");
    }

    /// Import Settings… refuses a file that is not a settings bundle with the backend's reason, and
    /// leaves the global settings and the UI state as they were.
    #[gpui_kit::test]
    async fn importing_a_file_that_is_not_a_settings_bundle_changes_nothing(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, _root, workbench, cx) = project(cx, true, &[("main.rs", "fn main() {}\n")]);
        let out = tempfile::tempdir().expect("a temporary folder");
        cx.update(|_, cx| {
            cx.global_mut::<UiStorage>().set("openidea.macros.v1", "{\"last\":[]}".to_owned());
            settings::set("w1", SettingsScope::Global, "general.powerSaveMode", &Value::Bool(true), cx)
                .expect("the setting is written");
        });
        let global_file = cx.update(|_, cx| cx.global::<Backend>().dirs.config.join("settings.toml"));
        let before = std::fs::read(&global_file).expect("the global settings are on disk");
        let newer = serde_json::json!({ "format": "openidea-settings", "version": 99, "settings": "" });
        // The file, and the start of the reason the refusal gives (the backend's own text).
        let cases = [
            ("broken.json", "{ not json".to_owned(), "not an OpenIDEA settings file: "),
            ("newer.json", newer.to_string(), "not an OpenIDEA settings file, or one from a newer version"),
        ];
        for (name, text, reason) in cases {
            let file = out.path().join(name);
            std::fs::write(&file, text).expect("the file is written");
            run_action(cx, "file.importSettings", None);
            cx.simulate_path_prompt_response(move |_| Some(vec![file]));
            let refused = |cx: &mut VisualTestContext| {
                workbench.read_with(cx, |w, _| {
                    w.notifications().items().iter().any(|n| {
                        n.title == "Cannot import settings" && n.body.as_deref().is_some_and(|b| b.starts_with(reason))
                    })
                })
            };
            wait_long(cx, name, refused);
        }
        assert_eq!(std::fs::read(&global_file).expect("readable"), before, "the global settings are untouched");
        assert!(cx.update(|_, cx| settings::global(cx).general.power_save_mode));
        let macros = cx.update(|_, cx| cx.global::<UiStorage>().get("openidea.macros.v1").map(str::to_owned));
        assert_eq!(macros.as_deref(), Some("{\"last\":[]}"));
        assert!(!notification_titles(&workbench, cx).contains(&"Settings imported".to_owned()));
        close(cx);
    }

    /// Restore Default Layout's key (⇧F12) needs an open project: it does nothing in a window
    /// without one, and restores the default layout in a project.
    #[gpui_kit::test]
    async fn the_restore_default_layout_key_works_only_with_a_project(cx: &mut TestAppContext) {
        use super::super::tool_windows::LayoutSnapshot;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("trusted");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        cx.update(|_, cx| {
            let mut layouts = NamedLayouts::default();
            assert!(layouts.save("Coding", LayoutSnapshot::default()));
            store_named_layouts(&layouts, cx);
        });
        let active = |cx: &mut VisualTestContext| cx.update(|_, cx| named_layouts(cx).active().map(str::to_owned));
        cx.simulate_keystrokes("shift-f12");
        cx.run_until_parked();
        assert_eq!(active(cx).as_deref(), Some("Coding"), "without a project the key does nothing");
        let refused = cx.update(|window, cx| execute_action_or_throw("layout.restoreDefault", None, window, cx));
        assert!(refused.is_err(), "the action is unavailable without a project");

        workbench.update_in(cx, |w, window, cx| w.open_project(root.clone(), window, cx));
        wait_long(cx, "the project opens", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let visible =
            |cx: &mut VisualTestContext, id: &str| workbench.read_with(cx, |w, _| w.is_tool_window_visible(id));
        run_action(cx, "toolwindow.run", None);
        workbench.update_in(cx, |w, window, cx| w.with_layout(window, cx, |layout| layout.hide("project")));
        assert!(!visible(cx, "project") && visible(cx, RUN_TOOL_WINDOW));
        cx.simulate_keystrokes("shift-f12");
        cx.run_until_parked();
        assert!(visible(cx, "project") && !visible(cx, RUN_TOOL_WINDOW), "the default layout is back");
        assert_eq!(active(cx), None);
        close(cx);
    }

    // src/features/app/__tests__/newProject.test.ts › Manage IDE Settings › Export Settings… › exports
    // templates, macros, layouts and view options, not per-project state
    #[gpui_kit::test]
    async fn exports_templates_macros_layouts_and_view_options_not_per_project_state(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, _root, workbench, cx) = project(cx, true, &[("main.rs", "fn main() {}\n")]);
        let out = tempfile::tempdir().expect("a temporary folder");
        let file = out.path().join(EXPORT_FILE_NAME);
        cx.update(|_, cx| {
            let storage = cx.global_mut::<UiStorage>();
            storage.set("openidea.macros.v1", "{\"last\":[]}".to_owned());
            storage.set("openidea.todo.v1", "{}".to_owned());
            storage.set("openidea.layout.v1:/somewhere", "{}".to_owned());
            settings::set("w1", SettingsScope::Global, "general.powerSaveMode", &Value::Bool(true), cx)
                .expect("the setting is written");
        });
        run_action(cx, "file.exportSettings", None);
        let target = file.clone();
        cx.simulate_new_path_selection(move |_| Some(target));
        wait_long(cx, "the export", |cx| notification_titles(&workbench, cx).contains(&"Settings exported".to_owned()));
        let bundle: Value = serde_json::from_str(&std::fs::read_to_string(&file).expect("the file is written"))
            .expect("the file is JSON");
        let storage = bundle["storage"].as_object().expect("the UI state is exported");
        assert!(storage.contains_key("openidea.macros.v1") && storage.contains_key("openidea.todo.v1"));
        assert!(!storage.contains_key("openidea.layout.v1:/somewhere"), "per-project state stays behind");

        // Import brings both back.
        cx.update(|_, cx| {
            cx.global_mut::<UiStorage>().remove("openidea.macros.v1");
            settings::set("w1", SettingsScope::Global, "general.powerSaveMode", &Value::Bool(false), cx)
                .expect("the setting is written");
        });
        run_action(cx, "file.importSettings", None);
        let picked = file.clone();
        cx.simulate_path_prompt_response(move |options| {
            assert!(options.files && !options.directories && !options.multiple);
            Some(vec![picked])
        });
        wait_long(cx, "the import", |cx| notification_titles(&workbench, cx).contains(&"Settings imported".to_owned()));
        let macros = cx.update(|_, cx| cx.global::<UiStorage>().get("openidea.macros.v1").map(str::to_owned));
        assert_eq!(macros.as_deref(), Some("{\"last\":[]}"));
        assert!(cx.update(|_, cx| settings::global(cx).general.power_save_mode));
        close(cx);
    }

    // src/workbench/toolwindows/layouts.test.ts › tool window layouts › restores by name through the action
    // (Window › Tool Window Layouts items)
    #[test]
    fn restores_by_name_through_the_action_window_tool_window_layouts_items() {
        use super::super::tool_windows::LayoutSnapshot;
        let mut layouts = NamedLayouts::default();
        assert!(layout_items(&layouts).is_empty());
        assert!(layouts.save("Coding", LayoutSnapshot::default()));
        assert!(layouts.save("Review", LayoutSnapshot::default()));
        let items = layout_items(&layouts);
        let shown: Vec<(&str, Option<&str>, Option<bool>)> =
            items.iter().map(|item| (item.label.as_str(), item.arg.as_deref(), item.checked)).collect();
        assert_eq!(shown, [("Coding", Some("Coding"), Some(false)), ("Review", Some("Review"), Some(true))]);
        assert!(items.iter().all(|item| item.action == "layout.restore"));
    }

    /// File › Power Save Mode flips the global setting, and the toggle shows it.
    #[gpui_kit::test]
    async fn power_save_mode_toggles_the_global_setting(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        let on = |cx: &mut VisualTestContext| cx.update(|_, cx| settings::global(cx).general.power_save_mode);
        let toggled = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let action = cx.global::<ActionRegistry>().get("file.togglePowerSave").expect("registered");
                action.is_toggled(window, cx)
            })
        };
        assert!(!on(cx) && toggled(cx) == Some(false));
        run_action(cx, "file.togglePowerSave", None);
        assert!(on(cx) && toggled(cx) == Some(true));
        run_action(cx, "file.togglePowerSave", None);
        assert!(!on(cx));
    }
}
