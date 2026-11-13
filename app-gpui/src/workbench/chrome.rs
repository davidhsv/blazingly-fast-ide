//! The window's title bar and status bar, as hosts of registered items.
//!
//! [`Chrome`] owns the two hosts and what the window keeps for them: the status items it feeds
//! (and the language servers' item, a view the window owns), the title widgets it hands over
//! (the branch, merge/rebase and run widgets, the IDE mode switch, the Build hammer) and the
//! Customize Main Toolbar dialog while it is open.
//!
//! The window feeds both hosts each time it is drawn ([`Workbench::sync_chrome`]): the items'
//! texts from the shown editor and the background tasks, the user's choices from the settings
//! (`appearance.statusBarWidgets`, `appearance.mainToolbar`, `appearance.showMainToolbar`), the
//! window's title and project. The hosts read nothing of the window while they draw, so a host is
//! never drawn against a window that is being updated. What the hosts ask for arrives as events:
//! actions run once the event is over, menus open in the popup slot, choices are written to the
//! global settings.

use super::main_toolbar::{MainToolbarSettings, TitleItem, VCS_WIDGET, save_toolbar};
use super::services_window::on_workbench;
use super::status_bar::{StatusBar, StatusBarEvent, StatusClick, StatusContent, StatusText};
use super::status_bar_widgets::SetVisible;
use super::status_items::{
    self, Indent, caret_item, encoding_item, indent_item, line_ending_item, read_only_item, task_item,
};
use super::title_bar::{
    ActionChoice, BUILD_ITEM, CustomizeToolbarDialog, CustomizeToolbarEvent, IDE_MODE_ITEM, MERGE_REBASE_ITEM,
    ProjectWidget, RUN_ITEM, TitleBar, TitleBarEvent, TitleBarFrame, TitleBarKind, TitleBarMode, TitleSlot,
    ToolbarCommand, built_in_items, project_menu, saved_toolbar_of, title_bar_mode, vcs_fallback_menu,
};
use super::{Content, Workbench};
use crate::build_window::view::{BuildTitleEvent, BuildTitleItem, TitleItemContext};
use crate::platform::actions::{ActionRegistry, ActionSource, execute_action_from};
use crate::platform::context::{ContextValue, window_context};
use crate::platform::i18n::{I18n, Localization};
use crate::platform::settings::{self, SettingsWrite};
use crate::platform::virtual_docs::is_virtual_uri;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, AnyView, App, AppContext, Context, Entity, EntityId, FocusHandle, Focusable, IntoElement, Pixels,
    Point, Subscription, Window, div, prelude::*,
};
use serde_json::Value;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

/// The status items whose text the window feeds, from what it knows: the background tasks and the
/// shown editor's caret, line separator, encoding, indentation and read-only attribute.
const FED_STATUS_ITEMS: [&str; 6] = [
    status_items::TASKS,
    status_items::CARET,
    status_items::LINE_ENDING,
    status_items::ENCODING,
    status_items::INDENT,
    status_items::READ_ONLY,
];

/// The fed items that describe the shown editor, cleared while there is none.
const EDITOR_STATUS_ITEMS: [&str; 5] = [
    status_items::CARET,
    status_items::LINE_ENDING,
    status_items::ENCODING,
    status_items::INDENT,
    status_items::READ_ONLY,
];

/// The title the Welcome screen's bar carries.
const WELCOME_TITLE: &str = "Welcome to OpenIDEA";

/// The title of a LightEdit window whose editor has no file name.
const LIGHT_EDIT_TITLE: &str = "LightEdit";

/// The Build hammer of the title bar, over the project's build.
struct BuildItem {
    /// The build it shows: a project opened again has a build of its own.
    run: EntityId,
    item: Entity<BuildTitleItem>,
    _events: Subscription,
}

/// Customize Main Toolbar, while it is open.
struct CustomizeWindow {
    dialog: Entity<CustomizeToolbarDialog>,
    /// What had the keyboard before the dialog opened.
    restore: Option<FocusHandle>,
    _events: Subscription,
}

/// The title bar and the status bar of a window, and what the window keeps for them.
pub(super) struct Chrome {
    status_bar: Entity<StatusBar>,
    title_bar: Entity<TitleBar>,
    /// Whether the project runs in Deep mode, `None` without a project: what the IDE mode switch
    /// shows. The window sets it; the switch reads it while the bar draws.
    ide_mode: Rc<Cell<Option<bool>>>,
    /// The view each title widget was last handed to the bar with, by widget id (`None`: none).
    title_views: HashMap<&'static str, Option<EntityId>>,
    build_item: Option<BuildItem>,
    customize: Option<CustomizeWindow>,
    _events: [Subscription; 2],
}

/// A title slot that draws nothing: a built-in widget stays listed (Customize Main Toolbar names
/// it) while the window has nothing for it to show.
fn empty_slot() -> TitleSlot {
    TitleSlot::Dynamic(Rc::new(|_: &Window, _: &App| None::<AnyView>))
}

/// The built-in title widget `id`.
fn title_item(id: &str) -> Option<TitleItem> {
    built_in_items().into_iter().find(|item| item.id == id)
}

/// `text` without its click when the click runs an action nobody handles: the item then tells
/// what it tells and does not pretend to do something.
fn reachable(mut text: StatusText, registry: Option<&ActionRegistry>) -> StatusText {
    let unhandled = matches!(
        &text.click,
        Some(StatusClick::Action { id, .. }) if !registry.is_some_and(|registry| registry.contains(id))
    );
    if unhandled {
        text.click = None;
    }
    text
}

/// Runs action `id` once the event that asked for it is over: its handler may reach for the
/// window, which is being updated while the event is delivered.
fn run_later(id: String, arg: Option<Value>, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| {
        execute_action_from(ActionSource::Toolbar, &id, arg.as_ref(), window, cx);
    });
}

impl Chrome {
    /// The hosts of a new window, with every item the window feeds registered.
    pub(super) fn new(window: &mut Window, cx: &mut Context<Workbench>) -> Self {
        let status_bar = cx.new(|cx| {
            let mut bar = StatusBar::new();
            for spec in FED_STATUS_ITEMS.into_iter().filter_map(status_items::spec) {
                bar.register_fed(spec, cx);
            }
            bar
        });
        let ide_mode = Rc::new(Cell::new(None));
        let shown_mode = Rc::clone(&ide_mode);
        let title_bar = cx.new(|cx| {
            let mut bar = TitleBar::new(TitleBarFrame::native());
            for item in built_in_items() {
                let slot = if item.id == IDE_MODE_ITEM {
                    let shown_mode = Rc::clone(&shown_mode);
                    TitleSlot::IdeMode(Rc::new(move |_: &Window, _: &App| shown_mode.get()))
                } else if item.id == VCS_WIDGET {
                    // Until the project's branch widget is handed over the bar shows "Version Control".
                    continue;
                } else {
                    empty_slot()
                };
                bar.register(item, slot, cx);
            }
            bar
        });
        let title_views = [RUN_ITEM, MERGE_REBASE_ITEM, BUILD_ITEM].into_iter().map(|id| (id, None)).collect();
        let events = [
            cx.subscribe_in(&status_bar, window, Workbench::status_bar_event),
            cx.subscribe_in(&title_bar, window, Workbench::title_bar_event),
        ];
        Self { status_bar, title_bar, ide_mode, title_views, build_item: None, customize: None, _events: events }
    }

    /// Hands widget `id` to the title bar: `slot` drawing the view `shown`, or nothing. Unchanged
    /// widgets are left as they are, so the bar redraws only when one comes or goes.
    fn set_title_slot(&mut self, id: &'static str, shown: Option<EntityId>, slot: Option<TitleSlot>, cx: &mut App) {
        if self.title_views.get(id) == Some(&shown) {
            return;
        }
        self.title_views.insert(id, shown);
        let Some(item) = title_item(id) else { return };
        self.title_bar.update(cx, |bar, cx| match slot {
            Some(slot) => bar.register(item, slot, cx),
            // Without a widget of its own the VCS entry is the "Version Control" fallback.
            None if id == VCS_WIDGET => bar.unregister(id, cx),
            None => bar.register(item, empty_slot(), cx),
        });
    }

    /// Hands a widget that is a view of its own to the title bar, or takes it away.
    fn set_title_view(&mut self, id: &'static str, view: Option<AnyView>, cx: &mut App) {
        let shown = view.as_ref().map(AnyView::entity_id);
        self.set_title_slot(id, shown, view.map(TitleSlot::View), cx);
    }
}

impl Workbench {
    /// The window's status bar.
    pub fn status_bar(&self) -> &Entity<StatusBar> {
        &self.chrome.status_bar
    }

    /// The window's title bar.
    pub fn title_bar(&self) -> &Entity<TitleBar> {
        &self.chrome.title_bar
    }

    /// Customize Main Toolbar, while it is open.
    pub fn customize_toolbar_dialog(&self) -> Option<&Entity<CustomizeToolbarDialog>> {
        self.chrome.customize.as_ref().map(|open| &open.dialog)
    }

    /// Feeds both hosts what the window shows now. Called each time the window is drawn; the hosts
    /// redraw only for what changed.
    pub(super) fn sync_chrome(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let appearance = settings::global(cx).appearance;
        let texts = self.status_texts(cx);
        let light_edit = matches!(self.content, Content::Editor(_) | Content::Viewer(_));
        let overrides = appearance.status_bar_widgets;
        let servers = AnyView::from(self.server_status_item().clone());
        self.chrome.status_bar.update(cx, |bar, cx| {
            // The language servers' item is a view of its own, the same one for the window's life.
            let servers_spec =
                status_items::spec(status_items::LANGUAGE_SERVICES).filter(|spec| bar.model().spec(&spec.id).is_none());
            if let Some(spec) = servers_spec {
                bar.register(spec, StatusContent::View(servers), cx);
            }
            bar.set_overrides(Some(overrides), cx);
            bar.set_light_edit(light_edit, cx);
            for (id, text) in texts {
                bar.set_text(id, text, cx);
            }
        });
        let saved = saved_toolbar_of(appearance.main_toolbar.as_ref());
        self.sync_title_bar(appearance.show_main_toolbar, saved, window, cx);
    }

    /// The text of each fed status item, `None` for an item with nothing to say.
    fn status_texts(&self, cx: &App) -> Vec<(&'static str, Option<StatusText>)> {
        let fallback;
        let i18n = match cx.try_global::<Localization>() {
            Some(localization) => &localization.0,
            None => {
                fallback = I18n::default();
                &fallback
            }
        };
        let editor_items = self.editor().map(|editor| {
            let editor = editor.read(cx);
            let status = editor.status();
            let facts = editor.facts();
            // The editor has no command that changes a file's indentation: the item tells it and
            // opens nothing.
            let mut indent = indent_item(Indent::of_unit(&facts.indent_unit, facts.tab_size));
            indent.click = None;
            let on_disk = !is_virtual_uri(&editor.path().to_string_lossy());
            [
                (status_items::CARET, Some(caret_item(&status, editor.selections().len()))),
                (status_items::LINE_ENDING, Some(line_ending_item(&status))),
                (status_items::ENCODING, Some(encoding_item(&status))),
                (status_items::INDENT, Some(indent)),
                (status_items::READ_ONLY, read_only_item(status.read_only, on_disk)),
            ]
        });
        let editor_items = editor_items.unwrap_or_else(|| EDITOR_STATUS_ITEMS.map(|id| (id, None)));
        let registry = cx.try_global::<ActionRegistry>();
        std::iter::once((status_items::TASKS, task_item(self.tasks.tasks(), i18n)))
            .chain(editor_items)
            .map(|(id, text)| (id, text.map(|text| reachable(text, registry))))
            .collect()
    }

    /// Feeds the title bar: how much of it shows, its title, the project, the saved toolbar and
    /// the widgets the window has.
    fn sync_title_bar(
        &mut self,
        show_main_toolbar: bool,
        saved: Option<MainToolbarSettings>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (kind, mode, title, project) = match &self.content {
            Content::Project { name, .. } => {
                let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned()).unwrap_or_default();
                let project = ProjectWidget { name: name.clone(), root };
                (TitleBarKind::Project, title_bar_mode(false, show_main_toolbar), None, Some(project))
            }
            Content::Editor(editor) => {
                let name = editor.read(cx).path().file_name().map(|name| name.to_string_lossy().into_owned());
                let title = name.unwrap_or_else(|| LIGHT_EDIT_TITLE.to_owned());
                (TitleBarKind::LightEdit, TitleBarMode::Full, Some(title), None)
            }
            Content::Viewer(view) => {
                let title = view.input.path.as_deref().map(|path| path.rsplit('/').next().unwrap_or(path).to_owned());
                (
                    TitleBarKind::LightEdit,
                    TitleBarMode::Full,
                    Some(title.unwrap_or_else(|| LIGHT_EDIT_TITLE.to_owned())),
                    None,
                )
            }
            Content::Opening(_) => (TitleBarKind::Minimal, TitleBarMode::Bare, None, None),
            Content::Empty => (TitleBarKind::Minimal, TitleBarMode::Full, Some(WELCOME_TITLE.to_owned()), None),
            Content::Failed(_) => (TitleBarKind::Minimal, TitleBarMode::Full, self.shown_title.clone(), None),
        };
        let document_title = self.shown_title.clone();
        self.chrome.title_bar.update(cx, |bar, cx| {
            bar.set_kind(kind, cx);
            bar.set_mode(mode, cx);
            bar.set_title(title, cx);
            bar.set_document_title(document_title, cx);
            bar.set_project(project, cx);
            bar.set_saved(saved, cx);
        });
        let ide_mode = self.project_tree().is_some().then(|| self.ide_mode(cx).is_full());
        if self.chrome.ide_mode.replace(ide_mode) != ide_mode {
            self.chrome.title_bar.update(cx, |_, cx| cx.notify());
        }
        // The branch widget is the VCS entry only while the project is a repository; otherwise the
        // entry is the "Version Control" fallback.
        let repository = self.vcs_state().is_some_and(|state| state.available);
        let views = [
            (VCS_WIDGET, self.branch_widget().filter(|_| repository).cloned().map(AnyView::from)),
            (MERGE_REBASE_ITEM, self.merge_rebase_widget(cx).cloned().map(AnyView::from)),
            (RUN_ITEM, self.run_widget().cloned().map(AnyView::from)),
        ];
        for (id, view) in views {
            self.chrome.set_title_view(id, view, cx);
        }
        let build = self.sync_build_item(window, cx);
        let shown = build.as_ref().map(|item| item.entity_id());
        // The hammer shows only for what it can build (a CMake target, a compiled Maven or Gradle
        // configuration); otherwise its entry draws nothing.
        let slot = build.map(|item| {
            TitleSlot::Dynamic(Rc::new(move |_: &Window, cx: &App| {
                item.read(cx).is_shown().then(|| AnyView::from(item.clone()))
            }))
        });
        self.chrome.set_title_slot(BUILD_ITEM, shown, slot, cx);
    }

    /// The Build hammer of the open project's build, told the context keys it shows by; `None`
    /// without a project.
    fn sync_build_item(&mut self, window: &Window, cx: &mut Context<Self>) -> Option<Entity<BuildTitleItem>> {
        let run = if self.project_tree().is_some() { self.build_run(cx) } else { None };
        let Some(run) = run else {
            self.chrome.build_item = None;
            return None;
        };
        let current = self.chrome.build_item.as_ref().filter(|built| built.run == run.entity_id());
        let current = current.map(|built| built.item.clone());
        let item = match current {
            Some(item) => item,
            None => {
                let item = cx.new(|cx| BuildTitleItem::new(run.clone(), cx));
                let events = cx.subscribe_in(&item, window, |_, _, event: &BuildTitleEvent, window, cx| {
                    let BuildTitleEvent::RunAction(id) = event;
                    run_later((*id).to_owned(), None, window, cx);
                });
                self.chrome.build_item = Some(BuildItem { run: run.entity_id(), item: item.clone(), _events: events });
                item
            }
        };
        let keys = window_context(window, cx);
        let selected_kind = match keys.get("run.selectedKind") {
            Some(ContextValue::Text(kind)) => kind.to_string(),
            _ => String::new(),
        };
        let context = TitleItemContext {
            cmake_build: keys.is_truthy("run.cmakeBuild"),
            maven: keys.is_truthy("build.maven"),
            gradle: keys.is_truthy("build.gradle"),
            selected_kind,
        };
        item.update(cx, |item, cx| item.set_context(context, cx));
        Some(item)
    }

    /// Writes `writes` to the global settings and redraws; a refusal is logged.
    fn write_global(&self, writes: &[SettingsWrite], cx: &mut Context<Self>) {
        if let Err(error) = settings::set_global(&self.label, writes, cx) {
            tracing::error!("the settings could not be written: {}", error.message);
        }
        cx.notify();
    }

    /// View › Appearance › Status Bar Widgets: shows or hides the status item `id`. Untitled and
    /// unknown items stay as they are.
    pub fn toggle_status_widget(&self, id: &str, cx: &mut Context<Self>) {
        let write = self.chrome.status_bar.read(cx).model().toggle(id);
        if let Some(write) = write {
            self.write_global(&[write], cx);
        }
    }

    pub(super) fn status_bar_event(
        &mut self,
        bar: &Entity<StatusBar>,
        event: &StatusBarEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            // Go to Line:Column opens for the shown editor wherever the keyboard is: a click on the
            // bar does not move it, and the action's `when` clause asks for the editor's focus.
            StatusBarEvent::RunAction { item, .. } if item == status_items::CARET => self.show_goto_line(window, cx),
            StatusBarEvent::RunAction { id, arg, .. } => run_later(id, arg, window, cx),
            StatusBarEvent::OpenItem { item, position } => {
                let items = match item.as_str() {
                    status_items::LINE_ENDING => self.line_ending_menu(cx),
                    status_items::ENCODING => self.encoding_menu(cx),
                    other => {
                        tracing::warn!("status item {other} has no menu to open");
                        return;
                    }
                };
                self.show_menu(
                    &items,
                    position,
                    |workbench, command, _, cx| workbench.run_status_command(command, cx),
                    window,
                    cx,
                );
            }
            StatusBarEvent::ContextMenu { position } => {
                let items = bar.read(cx).context_menu();
                self.show_menu(
                    &items,
                    position,
                    |workbench, choice: SetVisible, _, cx| {
                        let write = workbench.chrome.status_bar.read(cx).write_for(&choice);
                        workbench.write_global(&[write], cx);
                    },
                    window,
                    cx,
                );
            }
        }
    }

    pub(super) fn title_bar_event(
        &mut self,
        bar: &Entity<TitleBar>,
        event: &TitleBarEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            TitleBarEvent::RunAction { id, arg } => run_later(id, arg, window, cx),
            TitleBarEvent::ProjectMenu { position } => self.show_toolbar_menu(&project_menu(), position, window, cx),
            TitleBarEvent::VcsMenu { position } => self.show_toolbar_menu(&vcs_fallback_menu(), position, window, cx),
            TitleBarEvent::GearMenu { position } => {
                let items = bar.read(cx).gear_menu(cx);
                self.show_toolbar_menu(&items, position, window, cx);
            }
            TitleBarEvent::ContextMenu { position } => {
                let items = bar.read(cx).customize_menu(cx);
                self.show_toolbar_menu(&items, position, window, cx);
            }
            // The bar carries the main menu button only in a frameless window; this app's windows
            // keep the OS frame ([`TitleBarFrame::native`]), whose menu bar is the main menu.
            TitleBarEvent::MainMenu { .. } => {}
        }
    }

    /// Opens one of the toolbar's menus; a row of its own (show or remove an entry, restore the
    /// default toolbar) is written to the saved toolbar.
    fn show_toolbar_menu(
        &mut self,
        items: &[MenuItem<ToolbarCommand>],
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_menu(
            items,
            position,
            |workbench, command, _, cx| {
                let write = workbench.chrome.title_bar.read(cx).write_for(&command);
                workbench.write_global(&[write], cx);
            },
            window,
            cx,
        );
    }

    /// Customize Main Toolbar: a dialog over the window on the saved layout of the bar's widgets.
    /// It takes the keyboard; only its own buttons and Escape close it.
    pub fn show_customize_toolbar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.chrome.customize.is_some() {
            return;
        }
        let (layout, items) = {
            let bar = self.chrome.title_bar.read(cx);
            (bar.editable_layout(), bar.items().to_vec())
        };
        let actions = ActionChoice::registered(cx);
        let dialog = cx.new(|cx| CustomizeToolbarDialog::new(layout, items, actions, cx));
        let events = cx.subscribe_in(&dialog, window, Self::customize_toolbar_event);
        let restore = window.focused(cx);
        window.focus(&dialog.focus_handle(cx), cx);
        self.chrome.customize = Some(CustomizeWindow { dialog, restore, _events: events });
        cx.notify();
    }

    fn customize_toolbar_event(
        &mut self,
        _: &Entity<CustomizeToolbarDialog>,
        event: &CustomizeToolbarEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let CustomizeToolbarEvent::Apply(layout) = event {
            self.write_global(&[save_toolbar(Some(layout))], cx);
        }
        self.close_customize_toolbar(window, cx);
    }

    /// Closes Customize Main Toolbar; the keyboard goes back to what had it before, or the editor.
    fn close_customize_toolbar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.chrome.customize.take() else { return };
        if let Some(target) = open.restore.or_else(|| self.editor().map(|editor| editor.focus_handle(cx))) {
            window.focus(&target, cx);
        }
        cx.notify();
    }

    /// Customize Main Toolbar as a layer over the window, below the popup slot.
    pub(super) fn customize_toolbar_layer(&self) -> Option<AnyElement> {
        let dialog = self.customize_toolbar_dialog()?.clone();
        let layer = div()
            .id("customize-toolbar-layer")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .child(dialog);
        Some(gpui_kit::deferred(layer).with_priority(1).into_any_element())
    }
}

/// Registers the actions of the bars: View › Appearance › Status Bar Widgets and Customize Main
/// Toolbar.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    // The argument is the widget's id.
    registry.handle("view.toggleStatusBarWidget", |call, window, cx| {
        let Some(id) = call.arg.and_then(Value::as_str).map(str::to_owned) else { return Ok(()) };
        on_workbench(window, cx, move |workbench, _, cx| workbench.toggle_status_widget(&id, cx))
    });
    registry.handle("toolbar.customize", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.show_customize_toolbar(window, cx))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::actions::{Action, ActionMeta};

    #[test]
    fn a_click_that_runs_an_action_nobody_handles_is_taken_off_the_item() {
        let mut registry = ActionRegistry::default();
        registry.register(Action::new(ActionMeta::new("navigate.gotoLine", "Go to Line"), |_, _, _| Ok(())));
        let handled = StatusText::new("1:1").on_click(StatusClick::action("navigate.gotoLine"));
        assert_eq!(reachable(handled.clone(), Some(&registry)), handled);
        let unhandled = StatusText::new("").icon("lock").on_click(StatusClick::action("file.toggleReadOnly"));
        assert_eq!(reachable(unhandled.clone(), Some(&registry)).click, None);
        assert_eq!(reachable(unhandled, None).click, None, "without a registry nothing is handled");
        // A menu the window opens itself stays.
        let menu = StatusText::new("LF").on_click(StatusClick::Open);
        assert_eq!(reachable(menu.clone(), Some(&registry)), menu);
    }
}
