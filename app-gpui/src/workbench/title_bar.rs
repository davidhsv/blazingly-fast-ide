//! The title bar as a host of the main toolbar's widgets, and the Customize Main Toolbar dialog.
//!
//! Widgets are registered by id with their side and priority ([`TitleItem`]) and what they draw
//! ([`TitleSlot`]); [`resolve_toolbar`] puts the saved layout (`appearance.mainToolbar`) over the
//! default order, so each side shows the user's entries: the project widget, contributed widgets,
//! the "Version Control" fallback while no VCS widget is registered, and `action:<id>` buttons.
//! The gear menu, the main menu button of a frameless window (where there is no native menu bar)
//! and the window title complete the bar. With the main toolbar hidden (View › Appearance) or in
//! Distraction Free Mode the bar is the compact header: the window title, centred.
//!
//! [`TitleBar`] reports what the user does as [`TitleBarEvent`]s; the window shows the menus,
//! runs the actions and writes the settings the bar's commands return. [`CustomizeToolbar`] is
//! the dialog's editable layout, [`CustomizeToolbarDialog`] the dialog drawn on it.

use super::main_toolbar::{
    MainToolbarSettings, PROJECT_WIDGET, SEARCH_ENTRY, Side, TitleItem, ToolbarLayout, ToolbarSides, VCS_WIDGET,
    default_toolbar, editable_toolbar, entry_action, is_action_entry, resolve_toolbar, save_toolbar, toggle_widget,
    widget_title,
};
use super::status_bar::font_family;
use crate::platform::actions::ActionRegistry;
use crate::platform::collate;
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::platform::settings::SettingsWrite;
use crate::platform_more::distraction_free::shell_chrome;
use crate::theme;
use crate::ui::matching::fuzzy_match;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::ui::tooltip::{TOOLTIP_DELAY, Tooltip, WithTooltip};
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, AnyView, App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    IntoElement, Keystroke, MouseButton, MouseDownEvent, Pixels, Point, Render, SharedString, Stateful, Subscription,
    Window, div, prelude::*, px,
};
use serde_json::Value;
use std::collections::HashMap;
use std::rc::Rc;

pub const IDE_MODE_ITEM: &str = "app.ideMode";
pub const BUILD_ITEM: &str = "build.project";
pub const RUN_ITEM: &str = "run.widget";
pub const MERGE_REBASE_ITEM: &str = "vcs.mergeRebase";

/// The actions "Version Control ▾" offers while the project has no repository.
pub const VCS_FALLBACK_ACTIONS: [&str; 3] = ["vcs.clone", "vcs.localHistory", "vcs.showLog"];

/// The built-in title widgets: id, side, priority and the name Customize Main Toolbar shows.
pub fn built_in_items() -> Vec<TitleItem> {
    let item = |id: &str, align: Side, priority: f64, title: &str| TitleItem {
        id: id.to_owned(),
        align,
        priority,
        title: Some(title.to_owned()),
    };
    vec![
        item(IDE_MODE_ITEM, Side::Right, 1.0, "IDE Mode"),
        item(BUILD_ITEM, Side::Right, -1.0, "Build Project"),
        item(RUN_ITEM, Side::Right, 0.0, "Run Widget"),
        item(MERGE_REBASE_ITEM, Side::Left, 11.0, "Merge/Rebase Widget"),
        item(VCS_WIDGET, Side::Left, 10.0, "VCS Widget"),
    ]
}

/// The stored toolbar of the effective settings, in the shape [`resolve_toolbar`] takes.
pub fn saved_toolbar_of(stored: Option<&ide_core::settings::MainToolbarSettings>) -> Option<MainToolbarSettings> {
    stored.map(|stored| MainToolbarSettings {
        left: Some(stored.left.clone()),
        right: Some(stored.right.clone()),
        hidden: Some(stored.hidden.clone()),
    })
}

/// What a row of the toolbar's own menus does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolbarCommand {
    /// Show or remove this entry.
    Toggle(String),
    /// Restore the default toolbar.
    Reset,
}

/// Customize Main Toolbar ›: a check mark per widget (and added action), then the dialog and
/// Restore Default Toolbar. Also the title bar's context menu. `action_title` names an action.
pub fn customize_toolbar_items(
    shown: &ToolbarSides,
    hidden: &[String],
    items: &[TitleItem],
    action_title: impl Fn(&str) -> Option<String>,
) -> Vec<MenuItem<ToolbarCommand>> {
    let entries = shown.left.iter().chain(&shown.right).chain(hidden.iter().filter(|id| !is_action_entry(id)));
    let mut menu: Vec<MenuItem<ToolbarCommand>> = entries
        .map(|id| {
            let row = MenuItem::item(widget_title(id, items, &action_title), ToolbarCommand::Toggle(id.clone()));
            if hidden.contains(id) { row } else { row.icon("check") }
        })
        .collect();
    menu.push(MenuItem::Separator);
    menu.push(MenuItem::action("toolbar.customize").labelled("Customize Toolbar…"));
    menu.push(MenuItem::item("Restore Default Toolbar", ToolbarCommand::Reset));
    menu
}

/// The main toolbar's gear: Run Anything, Settings, Plugins, View Mode, toolbar customization and
/// updates.
pub fn gear_menu(customize: Vec<MenuItem<ToolbarCommand>>) -> Vec<MenuItem<ToolbarCommand>> {
    vec![
        MenuItem::action("runAnything.show"),
        MenuItem::Separator,
        MenuItem::action("file.settings"),
        MenuItem::action("plugins.settings"),
        MenuItem::Separator,
        MenuItem::submenu(
            "View Mode",
            vec![MenuItem::action("editor.toggleZen"), MenuItem::action("window.toggleFullScreen")],
        ),
        MenuItem::Separator,
        MenuItem::submenu("Customize Main Toolbar", customize),
        MenuItem::Separator,
        MenuItem::action("app.checkForUpdates"),
    ]
}

/// The project widget's menu: open a project, the recent ones, close this one.
pub fn project_menu() -> Vec<MenuItem<ToolbarCommand>> {
    vec![
        MenuItem::action("file.openProject").labelled("Open…"),
        MenuItem::action("welcome.recentProjects"),
        MenuItem::Separator,
        MenuItem::action("file.closeProject"),
    ]
}

/// "Version Control ▾"'s menu; actions that are not registered are left out when it opens.
pub fn vcs_fallback_menu() -> Vec<MenuItem<ToolbarCommand>> {
    VCS_FALLBACK_ACTIONS.into_iter().map(MenuItem::action).collect()
}

/// The write a toolbar command makes against the saved layout and the registered widgets.
pub fn toolbar_write(
    command: &ToolbarCommand,
    saved: Option<&MainToolbarSettings>,
    items: &[TitleItem],
) -> SettingsWrite {
    match command {
        ToolbarCommand::Reset => save_toolbar(None),
        ToolbarCommand::Toggle(id) => {
            let layout = editable_toolbar(saved, items);
            let side = items
                .iter()
                .find(|item| item.id == *id)
                .map(|item| item.align)
                .unwrap_or(if id == SEARCH_ENTRY { Side::Right } else { Side::Left });
            save_toolbar(Some(&toggle_widget(&layout, id, side)))
        }
    }
}

/// How much of the bar a window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleBarMode {
    /// The main toolbar.
    Full,
    /// The window title in the middle of a low bar (main toolbar hidden, Distraction Free).
    Compact,
    /// The window frame only, while a project opens.
    Bare,
}

/// The bar of a window in Distraction Free Mode or with the main toolbar switched off is the
/// compact header.
pub fn title_bar_mode(distraction_free: bool, show_main_toolbar: bool) -> TitleBarMode {
    if shell_chrome(distraction_free, show_main_toolbar, true).main_toolbar {
        TitleBarMode::Full
    } else {
        TitleBarMode::Compact
    }
}

/// The window around the bar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TitleBarFrame {
    /// The window draws its own frame: the bar carries the main menu button (Windows, Linux).
    pub frameless: bool,
    /// The OS's window buttons overlay the bar's left edge (macOS).
    pub overlay: bool,
    /// Native full screen took the window buttons out of the bar.
    pub fullscreen: bool,
}

impl TitleBarFrame {
    /// The frame of this platform's windows.
    pub fn native() -> Self {
        Self { frameless: false, overlay: cfg!(target_os = "macos"), fullscreen: false }
    }

    fn leading(self) -> f32 {
        if self.overlay && !self.fullscreen { super::WINDOW_BUTTONS_WIDTH } else { 10.0 }
    }
}

/// The project widget: the project's name on its avatar, its folder as tooltip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectWidget {
    pub name: String,
    pub root: String,
}

/// Computes a widget's view each time the bar is drawn; `None` while it shows nothing.
pub type SlotProvider = Rc<dyn Fn(&Window, &App) -> Option<AnyView>>;

/// What a registered widget draws.
#[derive(Clone)]
pub enum TitleSlot {
    View(AnyView),
    /// A view that comes and goes (the merge/rebase buttons while an operation is stopped).
    Dynamic(SlotProvider),
    /// The IDE mode switch; the function says whether the project is in Deep (full) mode, `None`
    /// while there is no project.
    IdeMode(Rc<dyn Fn(&Window, &App) -> Option<bool>>),
}

/// What the title bar asks of its window.
#[derive(Debug, Clone, PartialEq)]
pub enum TitleBarEvent {
    RunAction {
        id: String,
        arg: Option<Value>,
    },
    /// Show [`project_menu`] at `position`.
    ProjectMenu {
        position: Point<Pixels>,
    },
    /// Show [`vcs_fallback_menu`] at `position`.
    VcsMenu {
        position: Point<Pixels>,
    },
    /// Show [`TitleBar::gear_menu`] at `position`.
    GearMenu {
        position: Point<Pixels>,
    },
    /// Show the main menu as a popup at `position`.
    MainMenu {
        position: Point<Pixels>,
    },
    /// Show [`TitleBar::customize_menu`] at `position`.
    ContextMenu {
        position: Point<Pixels>,
    },
}

/// An entry as drawn.
enum Drawn {
    Project,
    View(AnyView),
    IdeMode(bool),
    VcsFallback,
    Action { id: String, title: String, icon: String },
}

fn tx(cx: &App, text: &str) -> String {
    match cx.try_global::<Localization>() {
        Some(localization) => localization.0.tx(text),
        None => text.to_owned(),
    }
}

/// A registered action's title in the active language.
fn action_title(id: &str, cx: &App) -> Option<String> {
    let action = cx.try_global::<ActionRegistry>()?.get(id)?;
    Some(match cx.try_global::<Localization>() {
        Some(localization) => action.meta.localized(&localization.0).title,
        None => action.meta.title.clone(),
    })
}

/// The ▾ after a dropdown widget's label.
fn chevron(cx: &App) -> AnyElement {
    theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-muted"))
}

fn is_registered(id: &str, cx: &App) -> bool {
    cx.try_global::<ActionRegistry>().is_some_and(|registry| registry.contains(id))
}

/// The main toolbar of a window.
pub struct TitleBar {
    items: Vec<TitleItem>,
    slots: HashMap<String, TitleSlot>,
    saved: Option<MainToolbarSettings>,
    mode: TitleBarMode,
    frame: TitleBarFrame,
    /// The window's document title, shown centred in the compact header.
    document_title: Option<String>,
    /// A title of the window's own (LightEdit's file): after the main menu in the full bar, and
    /// instead of the document title in the compact header.
    title: Option<String>,
    project: Option<ProjectWidget>,
    /// The main menu is open across the bar: the toolbar makes room for it.
    menu_expanded: bool,
}

impl EventEmitter<TitleBarEvent> for TitleBar {}

impl TitleBar {
    pub fn new(frame: TitleBarFrame) -> Self {
        Self {
            items: Vec::new(),
            slots: HashMap::new(),
            saved: None,
            mode: TitleBarMode::Full,
            frame,
            document_title: None,
            title: None,
            project: None,
            menu_expanded: false,
        }
    }

    /// Adds a widget, or replaces the one registered under the same id.
    pub fn register(&mut self, item: TitleItem, slot: TitleSlot, cx: &mut Context<Self>) {
        self.slots.insert(item.id.clone(), slot);
        match self.items.iter_mut().find(|known| known.id == item.id) {
            Some(known) => *known = item,
            None => self.items.push(item),
        }
        cx.notify();
    }

    pub fn unregister(&mut self, id: &str, cx: &mut Context<Self>) {
        self.items.retain(|item| item.id != id);
        if self.slots.remove(id).is_some() {
            cx.notify();
        }
    }

    pub fn items(&self) -> &[TitleItem] {
        &self.items
    }

    /// The stored toolbar (`appearance.mainToolbar`).
    pub fn set_saved(&mut self, saved: Option<MainToolbarSettings>, cx: &mut Context<Self>) {
        if self.saved != saved {
            self.saved = saved;
            cx.notify();
        }
    }

    pub fn saved(&self) -> Option<&MainToolbarSettings> {
        self.saved.as_ref()
    }

    pub fn set_mode(&mut self, mode: TitleBarMode, cx: &mut Context<Self>) {
        if self.mode != mode {
            self.mode = mode;
            cx.notify();
        }
    }

    pub fn mode(&self) -> TitleBarMode {
        self.mode
    }

    pub fn set_frame(&mut self, frame: TitleBarFrame, cx: &mut Context<Self>) {
        if self.frame != frame {
            self.frame = frame;
            cx.notify();
        }
    }

    pub fn set_document_title(&mut self, title: Option<String>, cx: &mut Context<Self>) {
        if self.document_title != title {
            self.document_title = title;
            cx.notify();
        }
    }

    pub fn set_title(&mut self, title: Option<String>, cx: &mut Context<Self>) {
        if self.title != title {
            self.title = title;
            cx.notify();
        }
    }

    pub fn set_project(&mut self, project: Option<ProjectWidget>, cx: &mut Context<Self>) {
        if self.project != project {
            self.project = project;
            cx.notify();
        }
    }

    /// The main menu opened or closed across the bar.
    pub fn set_menu_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        if self.menu_expanded != expanded {
            self.menu_expanded = expanded;
            cx.notify();
        }
    }

    /// The entries of each side, in the saved order.
    pub fn sides(&self) -> ToolbarSides {
        resolve_toolbar(self.saved.as_ref(), &self.items)
    }

    /// The full editable layout, for the Customize Main Toolbar dialog.
    pub fn editable_layout(&self) -> ToolbarLayout {
        editable_toolbar(self.saved.as_ref(), &self.items)
    }

    /// Whether the bar shows the toolbar entries and the gear.
    fn shows_toolbar(&self) -> bool {
        self.mode == TitleBarMode::Full
    }

    /// What entry `id` draws, when it draws something.
    fn drawn(&self, id: &str, window: &Window, cx: &App) -> Option<Drawn> {
        if id == PROJECT_WIDGET {
            return Some(Drawn::Project);
        }
        if let Some(slot) = self.slots.get(id) {
            return match slot {
                TitleSlot::View(view) => Some(Drawn::View(view.clone())),
                TitleSlot::Dynamic(provider) => provider(window, cx).map(Drawn::View),
                TitleSlot::IdeMode(mode) => mode(window, cx).map(Drawn::IdeMode),
            };
        }
        if id == VCS_WIDGET {
            return VCS_FALLBACK_ACTIONS.iter().any(|action| is_registered(action, cx)).then_some(Drawn::VcsFallback);
        }
        if !is_action_entry(id) {
            return None;
        }
        let action = cx.try_global::<ActionRegistry>()?.get(entry_action(id))?;
        let title = action_title(&action.meta.id, cx).unwrap_or_else(|| action.meta.title.clone());
        let icon = action.meta.icon.clone().unwrap_or_else(|| "run".to_owned());
        Some(Drawn::Action { id: action.meta.id.clone(), title, icon })
    }

    /// The entries drawn on each side, in order; none while the toolbar is hidden or the main menu
    /// is expanded.
    pub fn shown(&self, window: &Window, cx: &App) -> (Vec<String>, Vec<String>) {
        if !self.shows_toolbar() || self.menu_expanded {
            return (Vec::new(), Vec::new());
        }
        let ToolbarSides { left, right } = self.sides();
        let keep = |ids: Vec<String>| -> Vec<String> {
            ids.into_iter().filter(|id| self.drawn(id, window, cx).is_some()).collect()
        };
        (keep(left), keep(right))
    }

    /// Whether the gear is drawn.
    pub fn shows_gear(&self) -> bool {
        self.shows_toolbar() && !self.menu_expanded
    }

    /// Whether the main menu button is drawn: a frameless window has no menu bar of its own.
    pub fn shows_main_menu(&self) -> bool {
        self.frame.frameless
    }

    /// The text of the title the bar shows, if any.
    pub fn shown_title(&self) -> Option<&str> {
        match self.mode {
            TitleBarMode::Full => self.title.as_deref(),
            TitleBarMode::Compact => self.title.as_deref().or(self.document_title.as_deref()),
            TitleBarMode::Bare => None,
        }
    }

    /// Customize Main Toolbar ›, and the bar's context menu.
    pub fn customize_menu(&self, cx: &App) -> Vec<MenuItem<ToolbarCommand>> {
        let hidden = self.saved.as_ref().and_then(|saved| saved.hidden.clone()).unwrap_or_default();
        customize_toolbar_items(&self.sides(), &hidden, &self.items, |id| action_title(id, cx))
    }

    pub fn gear_menu(&self, cx: &App) -> Vec<MenuItem<ToolbarCommand>> {
        gear_menu(self.customize_menu(cx))
    }

    /// The write that carries out a chosen row of the toolbar's menus.
    pub fn write_for(&self, command: &ToolbarCommand) -> SettingsWrite {
        toolbar_write(command, self.saved.as_ref(), &self.items)
    }

    fn widget_button(id: &str, cx: &App) -> Stateful<Div> {
        let selector = format!("title:{id}");
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .h(px(30.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(8.0))
            .rounded(theme::length(cx, "radius-popup"))
            .text_color(theme::color(cx, "fg-strong"))
            .hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
    }

    fn render_entry(&self, id: &str, drawn: Drawn, cx: &mut Context<Self>) -> AnyElement {
        match drawn {
            Drawn::Project => {
                let label = self.project.as_ref().map_or_else(|| "OpenIDEA".to_owned(), |project| project.name.clone());
                let root = self.project.as_ref().map(|project| project.root.clone()).unwrap_or_default();
                let avatar = div()
                    .flex_none()
                    .size(px(20.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme::length(cx, "radius-small"))
                    .bg(theme::color(cx, "accent"))
                    .text_color(theme::color(cx, "fg-button-primary"))
                    .text_size(theme::length(cx, "fs-tiny"))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(super::project_color::project_initials(&label)));
                Self::widget_button(id, cx)
                    .child(avatar)
                    .child(div().min_w_0().truncate().child(SharedString::from(label)))
                    .child(chevron(cx))
                    .with_tooltip(root)
                    .on_click(cx.listener(|_, event: &ClickEvent, _, cx| {
                        cx.emit(TitleBarEvent::ProjectMenu { position: event.position() });
                    }))
                    .into_any_element()
            }
            Drawn::VcsFallback => Self::widget_button(id, cx)
                .child(div().min_w_0().truncate().child(SharedString::from(tx(cx, "Version Control"))))
                .child(chevron(cx))
                .on_click(cx.listener(|_, event: &ClickEvent, _, cx| {
                    cx.emit(TitleBarEvent::VcsMenu { position: event.position() });
                }))
                .into_any_element(),
            Drawn::IdeMode(full) => {
                let label = tx(cx, if full { "Deep mode" } else { "Blaze mode" });
                let tooltip = if full {
                    "Deep mode: language servers and the tooling feeding them are on. Click to switch to Blaze mode."
                } else {
                    "Blaze mode: editor, git and search only, no language servers. Click to switch to Deep mode."
                };
                let color = theme::color(cx, "fg-strong");
                Self::widget_button(id, cx)
                    .gap(px(5.0))
                    .child(theme::icon(cx, if full { "ide-mode-full" } else { "ide-mode-light" }, 16.0, color))
                    .child(SharedString::from(label))
                    .with_tooltip_shortcut(tooltip, "ide.toggleMode")
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                        cx.emit(TitleBarEvent::RunAction {
                            id: "ide.toggleMode".to_owned(),
                            arg: Some(Value::from("widget")),
                        });
                    }))
                    .into_any_element()
            }
            Drawn::Action { id: action, title, icon } => {
                let selector = format!("title:{id}");
                let tip_action = action.clone();
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .ml(px(12.0))
                    .size(theme::length(cx, "titlebar-tool-button"))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme::length(cx, "radius-popup"))
                    .hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
                    .child(theme::icon(cx, &icon, 20.0, theme::color(cx, "fg-strong")))
                    .tooltip(move |_, cx| {
                        let tooltip = Tooltip::new(title.clone()).with_action_shortcut(&tip_action, cx);
                        // Search Everywhere's double Shift is no binding the keymap can name.
                        let tooltip = if tooltip.shortcut().is_none() && tip_action == "navigate.searchEverywhere" {
                            tooltip.with_shortcut("⇧⇧")
                        } else {
                            tooltip
                        };
                        cx.new(|_| tooltip).into()
                    })
                    .tooltip_show_delay(TOOLTIP_DELAY)
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.emit(TitleBarEvent::RunAction { id: action.clone(), arg: None });
                    }))
                    .into_any_element()
            }
            Drawn::View(view) => {
                let selector = format!("title:{id}");
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .min_w_0()
                    .child(view)
                    .into_any_element()
            }
        }
    }

    fn main_menu_button(cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("title:mainMenu")
            .debug_selector(|| "title:mainMenu".to_owned())
            .size(theme::length(cx, "titlebar-tool-button"))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-popup"))
            .hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
            .child(theme::icon(cx, "main-menu", 20.0, theme::color(cx, "fg-strong")))
            .with_tooltip("Main Menu")
            .on_click(cx.listener(|_, event: &ClickEvent, _, cx| {
                cx.emit(TitleBarEvent::MainMenu { position: event.position() });
            }))
            .into_any_element()
    }

    fn gear(cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("title:gear")
            .debug_selector(|| "title:gear".to_owned())
            .ml(px(12.0))
            .size(theme::length(cx, "titlebar-tool-button"))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-popup"))
            .hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
            .child(theme::icon(cx, "settings", 20.0, theme::color(cx, "fg-strong")))
            .with_tooltip("IDE and Project Settings")
            .on_click(cx.listener(|_, event: &ClickEvent, _, cx| {
                cx.emit(TitleBarEvent::GearMenu { position: event.position() });
            }))
            .into_any_element()
    }
}

impl Render for TitleBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let compact = self.mode == TitleBarMode::Compact;
        let height = if compact { theme::length(cx, "h-compact-header") } else { theme::length(cx, "h-titlebar") };
        let leading = self.frame.leading();
        // The compact header keeps its title centred between the window buttons.
        let trailing = if compact { leading } else { 8.0 };
        let main_menu = self.shows_main_menu().then(|| Self::main_menu_button(cx));
        let bar = div()
            .window_control_area(gpui_kit::WindowControlArea::Drag)
            .id("title-bar")
            .debug_selector(|| "title-bar".to_owned())
            .w_full()
            .h(height)
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .pl(px(leading))
            .pr(px(trailing))
            .whitespace_nowrap()
            .font_family(font_family("font-ui", cx))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg-strong"))
            .children(main_menu);
        match self.mode {
            TitleBarMode::Bare => bar.child(div().flex_1().h_full()).into_any_element(),
            TitleBarMode::Compact => {
                let title = self.shown_title().unwrap_or_default().to_owned();
                bar.child(
                    div()
                        .debug_selector(|| "title:compact".to_owned())
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_center()
                        .text_color(theme::color(cx, "fg-secondary"))
                        .child(SharedString::from(title)),
                )
                .into_any_element()
            }
            TitleBarMode::Full => {
                let title = self.title.clone().map(|title| {
                    div()
                        .debug_selector(|| "title:window".to_owned())
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(SharedString::from(tx(cx, &title)))
                });
                let (left, right) = self.shown(window, cx);
                let mut entries = |ids: Vec<String>| -> Vec<AnyElement> {
                    ids.into_iter()
                        .filter_map(|id| {
                            let drawn = self.drawn(&id, window, cx)?;
                            Some(self.render_entry(&id, drawn, cx))
                        })
                        .collect()
                };
                let left = entries(left);
                let right = entries(right);
                let gear = self.shows_gear().then(|| Self::gear(cx));
                bar.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|_, event: &MouseDownEvent, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        cx.emit(TitleBarEvent::ContextMenu { position: event.position });
                    }),
                )
                .children(title)
                .children(left)
                .child(div().flex_1().min_w(px(24.0)).h_full())
                .children(right)
                .children(gear)
                .into_any_element()
            }
        }
    }
}

/// A row of the Add… picker: a removed widget, or an action that can go on the toolbar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolbarCandidate {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub icon: String,
}

/// An action the picker can offer, in the active language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionChoice {
    pub id: String,
    pub title: String,
    pub category: Option<String>,
    pub icon: Option<String>,
    pub hidden: bool,
}

impl ActionChoice {
    /// Every registered action, titled in the active language.
    pub fn registered(cx: &App) -> Vec<Self> {
        let Some(registry) = cx.try_global::<ActionRegistry>() else { return Vec::new() };
        let localization = cx.try_global::<Localization>();
        registry
            .all()
            .map(|action| {
                let meta = &action.meta;
                let localized = localization.map(|localization| meta.localized(&localization.0));
                Self {
                    id: meta.id.clone(),
                    title: localized.as_ref().map_or_else(|| meta.title.clone(), |found| found.title.clone()),
                    category: localized.and_then(|found| found.category).or_else(|| meta.category.clone()),
                    icon: meta.icon.clone(),
                    hidden: meta.hidden,
                }
            })
            .collect()
    }
}

/// The Customize Main Toolbar dialog's state: the layout being edited and the selected entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomizeToolbar {
    layout: ToolbarLayout,
    side: Side,
    index: usize,
}

impl CustomizeToolbar {
    pub fn new(layout: ToolbarLayout) -> Self {
        Self { layout, side: Side::Left, index: 0 }
    }

    pub fn layout(&self) -> &ToolbarLayout {
        &self.layout
    }

    fn list(&self, side: Side) -> &Vec<String> {
        match side {
            Side::Left => &self.layout.left,
            Side::Right => &self.layout.right,
        }
    }

    fn list_mut(&mut self, side: Side) -> &mut Vec<String> {
        match side {
            Side::Left => &mut self.layout.left,
            Side::Right => &mut self.layout.right,
        }
    }

    /// The selected side and row.
    pub fn selection(&self) -> (Side, usize) {
        (self.side, self.index)
    }

    pub fn selected(&self) -> Option<&str> {
        self.list(self.side).get(self.index).map(String::as_str)
    }

    pub fn select(&mut self, side: Side, index: usize) {
        self.side = side;
        self.index = index;
    }

    pub fn can_move_up(&self) -> bool {
        self.selected().is_some() && self.index > 0
    }

    pub fn can_move_down(&self) -> bool {
        self.selected().is_some() && self.index + 1 < self.list(self.side).len()
    }

    /// Takes the selected entry off its side. A removed widget is remembered as hidden; a removed
    /// action is dropped.
    pub fn remove(&mut self) {
        let Some(id) = self.selected().map(str::to_owned) else { return };
        let index = self.index;
        self.list_mut(self.side).remove(index);
        if !is_action_entry(&id) && !self.layout.hidden.contains(&id) {
            self.layout.hidden.push(id);
        }
        self.index = index.saturating_sub(1);
    }

    fn swap(&mut self, up: bool) {
        let index = self.index;
        let target = if up { index.checked_sub(1) } else { Some(index + 1) };
        let Some(target) = target.filter(|target| *target < self.list(self.side).len()) else { return };
        if self.selected().is_none() {
            return;
        }
        self.list_mut(self.side).swap(index, target);
        self.index = target;
    }

    pub fn move_up(&mut self) {
        self.swap(true);
    }

    pub fn move_down(&mut self) {
        self.swap(false);
    }

    /// Moves the selected entry to the end of the other side, keeping it selected.
    pub fn move_across(&mut self) {
        let Some(id) = self.selected().map(str::to_owned) else { return };
        let (from, index) = (self.side, self.index);
        let other = match from {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        };
        self.list_mut(from).remove(index);
        self.list_mut(other).push(id);
        self.side = other;
        self.index = self.list(other).len() - 1;
    }

    /// Puts `id` after the selected entry (at the end of an empty or unselected side), and selects
    /// it; it is no longer hidden.
    pub fn add(&mut self, id: &str) {
        let side = self.side;
        let at = if self.selected().is_some() { self.index + 1 } else { self.list(side).len() };
        let list = self.list_mut(side);
        list.retain(|entry| entry != id);
        let at = at.min(list.len());
        list.insert(at, id.to_owned());
        self.layout.hidden.retain(|entry| entry != id);
        self.index = at;
    }

    /// Restore Defaults: the default toolbar, the first left entry selected.
    pub fn restore_defaults(&mut self, items: &[TitleItem]) {
        self.layout = default_toolbar(items);
        self.select(Side::Left, 0);
    }

    /// Removed widgets, then every visible action with an icon that is not on the toolbar yet by
    /// title, filtered by `query`.
    pub fn candidates(&self, items: &[TitleItem], actions: &[ActionChoice], query: &str) -> Vec<ToolbarCandidate> {
        let present: Vec<&String> = self.layout.left.iter().chain(&self.layout.right).collect();
        let widgets = self.layout.hidden.iter().map(|id| ToolbarCandidate {
            id: id.clone(),
            label: widget_title(id, items, |action| {
                actions.iter().find(|known| known.id == action).map(|known| known.title.clone())
            }),
            detail: "Widget".to_owned(),
            icon: "main-menu".to_owned(),
        });
        let mut offered: Vec<ToolbarCandidate> = actions
            .iter()
            .filter(|action| !action.hidden)
            .filter_map(|action| {
                let icon = action.icon.clone()?;
                let id = format!("{}{}", super::main_toolbar::ACTION_PREFIX, action.id);
                (!present.contains(&&id)).then(|| ToolbarCandidate {
                    id,
                    label: action.title.clone(),
                    detail: action.category.clone().unwrap_or_default(),
                    icon,
                })
            })
            .collect();
        offered.sort_by(|a, b| collate::compare(&a.label, &b.label));
        let query = query.trim();
        widgets
            .chain(offered)
            .filter(|candidate| query.is_empty() || fuzzy_match(&candidate.label, query).is_some())
            .collect()
    }

    /// The write that saves the edited layout.
    pub fn save(&self) -> SettingsWrite {
        save_toolbar(Some(&self.layout))
    }
}

/// What the dialog asks of its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomizeToolbarEvent {
    /// OK: save this layout ([`save_toolbar`]) and close.
    Apply(ToolbarLayout),
    /// Cancel, Escape: close without saving.
    Cancelled,
}

/// A button of the dialog, with whether it can be pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogButton {
    pub id: &'static str,
    pub label: &'static str,
    pub enabled: bool,
}

const ROW_HEIGHT: f32 = 24.0;

/// The Customize Main Toolbar dialog: both sides' entries, reorderable, and the Add… picker.
pub struct CustomizeToolbarDialog {
    state: CustomizeToolbar,
    items: Vec<TitleItem>,
    actions: Vec<ActionChoice>,
    focus: FocusHandle,
    /// The Add… picker's query field, while the picker is open.
    query: Option<Entity<TextField>>,
    /// The highlighted candidate of the picker.
    active: usize,
    key_claims: Vec<u64>,
    _query_events: Option<Subscription>,
}

impl EventEmitter<CustomizeToolbarEvent> for CustomizeToolbarDialog {}

impl CustomizeToolbarDialog {
    /// The dialog on the saved layout of the registered `items`; `actions` are what Add… offers.
    pub fn new(
        layout: ToolbarLayout,
        items: Vec<TitleItem>,
        actions: Vec<ActionChoice>,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let claim = input::claim_keys(
            focus.clone(),
            move |keystroke, window, cx| {
                claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|dialog, cx| {
            for claim in dialog.key_claims.drain(..) {
                input::release_keys(claim, cx);
            }
        })
        .detach();
        Self {
            state: CustomizeToolbar::new(layout),
            items,
            actions,
            focus,
            query: None,
            active: 0,
            key_claims: vec![claim],
            _query_events: None,
        }
    }

    pub fn state(&self) -> &CustomizeToolbar {
        &self.state
    }

    /// The label of each entry of `side`, top to bottom.
    pub fn rows(&self, side: Side) -> Vec<String> {
        let title = |action: &str| self.action_title(action);
        self.state.list(side).iter().map(|id| widget_title(id, &self.items, title)).collect()
    }

    fn action_title(&self, id: &str) -> Option<String> {
        self.actions.iter().find(|action| action.id == id).map(|action| action.title.clone())
    }

    pub fn is_adding(&self) -> bool {
        self.query.is_some()
    }

    fn query_text(&self, cx: &App) -> String {
        self.query.as_ref().map(|query| query.read(cx).text().to_owned()).unwrap_or_default()
    }

    /// What the open picker offers for its query.
    pub fn candidates(&self, cx: &App) -> Vec<ToolbarCandidate> {
        if !self.is_adding() {
            return Vec::new();
        }
        self.state.candidates(&self.items, &self.actions, &self.query_text(cx))
    }

    /// The highlighted candidate's index.
    pub fn active_candidate(&self) -> usize {
        self.active
    }

    /// The buttons beside the lists and under them, with whether each can be pressed.
    pub fn buttons(&self) -> Vec<DialogButton> {
        let selected = self.state.selected().is_some();
        let across = if self.state.side == Side::Left { "Move to Right" } else { "Move to Left" };
        vec![
            DialogButton { id: "add", label: "Add…", enabled: true },
            DialogButton { id: "remove", label: "Remove", enabled: selected },
            DialogButton { id: "up", label: "Move Up", enabled: self.state.can_move_up() },
            DialogButton { id: "down", label: "Move Down", enabled: self.state.can_move_down() },
            DialogButton { id: "across", label: across, enabled: selected },
            DialogButton { id: "restore", label: "Restore Defaults", enabled: true },
            DialogButton { id: "ok", label: "OK", enabled: true },
            DialogButton { id: "cancel", label: "Cancel", enabled: true },
        ]
    }

    pub fn select(&mut self, side: Side, index: usize, cx: &mut Context<Self>) {
        self.state.select(side, index);
        cx.notify();
    }

    /// Presses button `id` (see [`Self::buttons`]); a disabled one does nothing.
    pub fn press(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.buttons().iter().any(|button| button.id == id && button.enabled) {
            return;
        }
        match id {
            "add" => self.toggle_picker(window, cx),
            "remove" => self.state.remove(),
            "up" => self.state.move_up(),
            "down" => self.state.move_down(),
            "across" => self.state.move_across(),
            "restore" => self.state.restore_defaults(&self.items),
            "ok" => cx.emit(CustomizeToolbarEvent::Apply(self.state.layout.clone())),
            "cancel" => cx.emit(CustomizeToolbarEvent::Cancelled),
            _ => return,
        }
        cx.notify();
    }

    fn toggle_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.query.take().is_some() {
            for claim in self.key_claims.drain(1..) {
                input::release_keys(claim, cx);
            }
            self._query_events = None;
            window.focus(&self.focus, cx);
            return;
        }
        let field = cx.new(|cx| {
            let placeholder = tx(cx, "Search actions");
            TextField::new(cx).with_placeholder(placeholder)
        });
        self._query_events = Some(cx.subscribe(&field, |dialog: &mut Self, _, event: &TextFieldEvent, cx| {
            if *event == TextFieldEvent::Changed {
                dialog.active = 0;
                cx.notify();
            }
        }));
        let claimant = cx.weak_entity();
        let field_focus = field.focus_handle(cx);
        self.key_claims.push(input::claim_keys(
            field_focus.clone(),
            move |keystroke, window, cx| {
                claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        ));
        window.focus(&field_focus, cx);
        self.query = Some(field);
        self.active = 0;
    }

    /// Adds candidate `index` of the picker and closes it.
    pub fn pick(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(candidate) = self.candidates(cx).into_iter().nth(index) else { return };
        self.state.add(&candidate.id);
        self.toggle_picker(window, cx);
        cx.notify();
    }

    fn claim_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        let adding = self.is_adding();
        match keystroke.key.as_str() {
            "escape" if adding => self.toggle_picker(window, cx),
            "escape" => cx.emit(CustomizeToolbarEvent::Cancelled),
            "enter" if adding => self.pick(self.active, window, cx),
            "enter" => cx.emit(CustomizeToolbarEvent::Apply(self.state.layout.clone())),
            "up" | "down" if adding => {
                let count = self.candidates(cx).len();
                if count > 0 {
                    self.active = if keystroke.key == "up" {
                        self.active.checked_sub(1).unwrap_or(count - 1)
                    } else {
                        (self.active + 1) % count
                    };
                }
            }
            "up" | "down" => {
                let (side, index) = self.state.selection();
                let count = self.state.list(side).len();
                if count > 0 {
                    let next = if keystroke.key == "up" { index.saturating_sub(1) } else { (index + 1).min(count - 1) };
                    self.state.select(side, next);
                }
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    fn button(&self, button: DialogButton, primary: bool, cx: &mut Context<Self>) -> AnyElement {
        let label = tx(cx, button.label);
        let id = button.id;
        let face = div()
            .id(SharedString::from(format!("customize-{id}")))
            .debug_selector(move || format!("customize-{id}"))
            .flex_none()
            .h(px(28.0))
            .min_w(px(72.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .child(SharedString::from(label));
        let face = if primary {
            let fill = theme::color(cx, "bg-button-primary");
            face.bg(fill).border_color(fill).text_color(theme::color(cx, "fg-button-primary"))
        } else {
            face.bg(theme::color(cx, "bg-button"))
                .border_color(theme::color(cx, "border-button"))
                .text_color(theme::color(cx, "fg-strong"))
        };
        if button.enabled {
            face.on_click(cx.listener(move |dialog, _: &ClickEvent, window, cx| dialog.press(id, window, cx)))
                .into_any_element()
        } else {
            face.opacity(0.5).into_any_element()
        }
    }

    fn side_list(&self, side: Side, cx: &mut Context<Self>) -> AnyElement {
        let (name, key) = match side {
            Side::Left => ("Left", "left"),
            Side::Right => ("Right", "right"),
        };
        let (selected_side, selected) = self.state.selection();
        let rows: Vec<AnyElement> = self
            .state
            .list(side)
            .iter()
            .enumerate()
            .map(|(index, id)| {
                let active = selected_side == side && selected == index;
                let icon = if is_action_entry(id) {
                    self.actions
                        .iter()
                        .find(|action| action.id == entry_action(id))
                        .and_then(|action| action.icon.clone())
                        .unwrap_or_else(|| "run".to_owned())
                } else {
                    "main-menu".to_owned()
                };
                let label = widget_title(id, &self.items, |action| self.action_title(action));
                let selector = format!("customize-row:{key}:{index}");
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .h(px(ROW_HEIGHT))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .when(active, |el| el.bg(theme::color(cx, "bg-selected")))
                    .child(theme::icon(cx, &icon, 14.0, theme::color(cx, "fg-muted")))
                    .child(div().min_w_0().truncate().child(SharedString::from(label)))
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.select(side, index, cx)))
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty().then(|| {
            div().p(px(8.0)).text_color(theme::color(cx, "fg-faint")).child(SharedString::from(tx(cx, "Empty")))
        });
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .text_color(theme::color(cx, "fg-muted"))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(tx(cx, name))),
            )
            .child(
                div()
                    .h(px(ROW_HEIGHT * 10.0))
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .border_1()
                    .border_color(theme::color(cx, "border"))
                    .rounded(theme::length(cx, "radius-small"))
                    .children(rows)
                    .children(empty),
            )
            .into_any_element()
    }

    fn picker(&self, field: Entity<TextField>, cx: &mut Context<Self>) -> AnyElement {
        let candidates = self.candidates(cx);
        let rows: Vec<AnyElement> = candidates
            .into_iter()
            .enumerate()
            .map(|(index, candidate)| {
                let active = index == self.active;
                let selector = format!("customize-candidate:{index}");
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .h(px(ROW_HEIGHT))
                    .flex_none()
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .when(active, |el| el.bg(theme::color(cx, "bg-selected")))
                    .child(theme::icon(cx, &candidate.icon, 14.0, theme::color(cx, "fg-muted")))
                    .child(div().flex_1().min_w_0().truncate().child(SharedString::from(candidate.label)))
                    .child(div().text_color(theme::color(cx, "fg-faint")).child(SharedString::from(candidate.detail)))
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, window, cx| dialog.pick(index, window, cx)))
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .h(px(28.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .border_1()
                    .border_color(theme::color(cx, "border"))
                    .rounded(theme::length(cx, "radius-small"))
                    .child(div().flex_1().min_w_0().child(field)),
            )
            .child(
                div()
                    .id("customize-candidates")
                    .h(px(ROW_HEIGHT * 8.0))
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .into_any_element()
    }
}

impl Focusable for CustomizeToolbarDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for CustomizeToolbarDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let buttons = self.buttons();
        let side_buttons: Vec<AnyElement> = buttons
            .iter()
            .filter(|button| ["add", "remove", "up", "down", "across"].contains(&button.id))
            .map(|button| self.button(button.clone(), false, cx))
            .collect();
        let restore = buttons.iter().find(|button| button.id == "restore").cloned();
        let ok = buttons.iter().find(|button| button.id == "ok").cloned();
        let cancel = buttons.iter().find(|button| button.id == "cancel").cloned();
        let left = self.side_list(Side::Left, cx);
        let right = self.side_list(Side::Right, cx);
        let picker = self.query.clone().map(|field| self.picker(field, cx));
        let footer_start = restore.map(|button| self.button(button, false, cx));
        let ok = ok.map(|button| self.button(button, true, cx));
        let cancel = cancel.map(|button| self.button(button, false, cx));
        let title = tx(cx, "Customize Main Toolbar");
        div()
            .debug_selector(|| "customize-toolbar".to_owned())
            .track_focus(&self.focus)
            .w(px(620.0))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            // A click on the dialog is not a click beside it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_color(theme::color(cx, "fg-strong"))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .px(px(16.0))
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(
                        div().flex().gap(px(12.0)).child(left).child(right).child(
                            div().flex_none().flex().flex_col().gap(px(6.0)).pt(px(20.0)).children(side_buttons),
                        ),
                    )
                    .children(picker),
            )
            .child(
                div()
                    .px(px(16.0))
                    .py(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .children(footer_start)
                    .child(div().flex_1())
                    .children(ok)
                    .children(cancel),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::actions::{Action, ActionMeta};
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use serde_json::json;
    use std::cell::RefCell;

    fn item(id: &str, align: Side, priority: f64) -> TitleItem {
        TitleItem { id: id.to_owned(), align, priority, title: None }
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| (*id).to_owned()).collect()
    }

    fn checked(row: &MenuItem<ToolbarCommand>) -> bool {
        matches!(row, MenuItem::Item(entry) if entry.icon == Some("check"))
    }

    // src/workbench/Workbench.test.tsx › status bar ordering › title items sort by priority within each side
    #[test]
    fn title_items_sort_by_priority_within_each_side() {
        let items = [item("x", Side::Right, 10.0), item("y", Side::Right, 0.0), item("z", Side::Left, 0.0)];
        let sides = resolve_toolbar(None, &items);
        assert_eq!(sides.left, ids(&[PROJECT_WIDGET, VCS_WIDGET, "z"]));
        assert_eq!(sides.right, ids(&["y", "x", SEARCH_ENTRY]));
    }

    #[test]
    fn the_built_in_widgets_take_their_default_places() {
        let sides = resolve_toolbar(None, &built_in_items());
        assert_eq!(sides.left, ids(&[PROJECT_WIDGET, VCS_WIDGET, MERGE_REBASE_ITEM]));
        assert_eq!(sides.right, ids(&[BUILD_ITEM, RUN_ITEM, IDE_MODE_ITEM, SEARCH_ENTRY]));
    }

    #[test]
    fn every_built_in_widget_is_registered_as_the_contributions_list_says() {
        let listed: Value =
            serde_json::from_str(include_str!("../../assets/gen/contributions.json")).expect("the list is JSON");
        let listed = listed["registerTitleItem"].as_array().expect("title items are listed").clone();
        let built_in = built_in_items();
        assert_eq!(listed.len(), built_in.len());
        for entry in listed {
            let id = entry["id"].as_str().expect("an id");
            let item = built_in.iter().find(|item| item.id == id).unwrap_or_else(|| panic!("{id} is built in"));
            let align = if entry["align"] == "left" { Side::Left } else { Side::Right };
            assert_eq!((item.align, Some(item.priority)), (align, entry["priority"].as_f64()), "{id}");
            assert_eq!(item.title.as_deref(), entry["title"].as_str(), "{id}");
        }
    }

    // src/workbench/TitleBar.test.tsx › title bar gear menu › lists every widget with a checkbox in Customize
    // Main Toolbar
    #[test]
    fn lists_every_widget_with_a_checkbox_in_customize_main_toolbar() {
        let shown = ToolbarSides { left: ids(&["project", "vcs.branch"]), right: ids(&[SEARCH_ENTRY]) };
        let items = [TitleItem { title: Some("Run Widget".to_owned()), ..item("run.widget", Side::Right, 1.0) }];
        let menu = customize_toolbar_items(&shown, &ids(&["run.widget"]), &items, |_| None);
        let rows: Vec<(Option<&str>, bool)> =
            menu.iter().filter(|row| row.command().is_some()).map(|row| (row.label(), checked(row))).collect();
        assert_eq!(
            rows,
            [
                (Some("Project Widget"), true),
                (Some("VCS Widget"), true),
                (Some("navigate.searchEverywhere"), true),
                (Some("Run Widget"), false),
                (Some("Restore Default Toolbar"), false),
            ]
        );
        assert_eq!(menu[3].command(), Some(&ToolbarCommand::Toggle("run.widget".to_owned())));
        assert_eq!(menu.last().and_then(MenuItem::command), Some(&ToolbarCommand::Reset));
        assert_eq!(menu[menu.len() - 2].action_id(), Some("toolbar.customize"));
    }

    #[test]
    fn a_toggle_shows_or_hides_the_entry_and_reset_restores_the_default() {
        let items = built_in_items();
        let write = toolbar_write(&ToolbarCommand::Toggle(RUN_ITEM.to_owned()), None, &items);
        assert_eq!(write.key, "appearance.mainToolbar");
        assert_eq!(
            write.value,
            json!({
                "left": [PROJECT_WIDGET, VCS_WIDGET, MERGE_REBASE_ITEM],
                "right": [BUILD_ITEM, IDE_MODE_ITEM, SEARCH_ENTRY],
                "hidden": [RUN_ITEM],
            })
        );
        let saved = super::super::main_toolbar::saved_toolbar(&json!({ "appearance": { "mainToolbar": write.value } }));
        let back = toolbar_write(&ToolbarCommand::Toggle(RUN_ITEM.to_owned()), saved.as_ref(), &items);
        assert_eq!(back.value["right"], json!([BUILD_ITEM, IDE_MODE_ITEM, SEARCH_ENTRY, RUN_ITEM]));
        // Search Everywhere goes back to the right; an unknown entry to the left.
        let without = toolbar_write(&ToolbarCommand::Toggle(SEARCH_ENTRY.to_owned()), None, &items);
        let stored = json!({ "appearance": { "mainToolbar": without.value } });
        let saved = super::super::main_toolbar::saved_toolbar(&stored);
        let again = toolbar_write(&ToolbarCommand::Toggle(SEARCH_ENTRY.to_owned()), saved.as_ref(), &items);
        assert_eq!(again.value["right"].as_array().and_then(|right| right.last()), Some(&json!(SEARCH_ENTRY)));
        assert_eq!(toolbar_write(&ToolbarCommand::Reset, saved.as_ref(), &items).value, Value::Null);
    }

    #[test]
    fn the_stored_toolbar_reads_as_the_layout_resolve_takes() {
        let stored = ide_core::settings::MainToolbarSettings {
            left: ids(&[PROJECT_WIDGET]),
            right: ids(&[SEARCH_ENTRY]),
            hidden: ids(&[RUN_ITEM]),
        };
        let saved = saved_toolbar_of(Some(&stored)).expect("stored");
        let sides = resolve_toolbar(Some(&saved), &built_in_items());
        assert_eq!(sides.left, ids(&[PROJECT_WIDGET, VCS_WIDGET, MERGE_REBASE_ITEM]));
        assert_eq!(sides.right, ids(&[SEARCH_ENTRY, BUILD_ITEM, IDE_MODE_ITEM]));
        assert_eq!(saved_toolbar_of(None), None);
    }

    #[test]
    fn the_main_toolbar_switch_and_distraction_free_make_the_compact_header() {
        assert_eq!(title_bar_mode(false, true), TitleBarMode::Full);
        assert_eq!(title_bar_mode(false, false), TitleBarMode::Compact);
        assert_eq!(title_bar_mode(true, true), TitleBarMode::Compact);
    }

    #[test]
    fn the_gear_menu_lists_run_anything_settings_plugins_view_mode_customization_and_updates() {
        let menu = gear_menu(vec![MenuItem::item("Restore Default Toolbar", ToolbarCommand::Reset)]);
        let rows: Vec<String> = menu
            .iter()
            .map(|row| match row {
                MenuItem::Separator => "separator".to_owned(),
                MenuItem::Action(action) => action.action.to_owned(),
                MenuItem::Item(entry) => entry.label.clone(),
            })
            .collect();
        assert_eq!(
            rows,
            [
                "runAnything.show",
                "separator",
                "file.settings",
                "plugins.settings",
                "separator",
                "View Mode",
                "separator",
                "Customize Main Toolbar",
                "separator",
                "app.checkForUpdates",
            ]
        );
        let MenuItem::Item(view_mode) = &menu[5] else { panic!("a submenu") };
        let modes: Vec<Option<&str>> = view_mode.submenu.iter().map(MenuItem::action_id).collect();
        assert_eq!(modes, [Some("editor.toggleZen"), Some("window.toggleFullScreen")]);
    }

    fn layout() -> ToolbarLayout {
        ToolbarLayout {
            left: ids(&[PROJECT_WIDGET, VCS_WIDGET]),
            right: ids(&[RUN_ITEM, SEARCH_ENTRY]),
            hidden: Vec::new(),
        }
    }

    #[test]
    fn the_dialog_moves_entries_within_and_across_the_sides() {
        let mut dialog = CustomizeToolbar::new(layout());
        assert!(!dialog.can_move_up() && dialog.can_move_down());
        dialog.move_down();
        assert_eq!(dialog.layout().left, ids(&[VCS_WIDGET, PROJECT_WIDGET]));
        assert_eq!(dialog.selection(), (Side::Left, 1));
        dialog.move_down();
        assert_eq!(dialog.selection(), (Side::Left, 1), "the last row stays where it is");
        dialog.move_up();
        assert_eq!(dialog.layout().left, ids(&[PROJECT_WIDGET, VCS_WIDGET]));
        dialog.select(Side::Left, 1);
        dialog.move_across();
        assert_eq!(dialog.layout().left, ids(&[PROJECT_WIDGET]));
        assert_eq!(dialog.layout().right, ids(&[RUN_ITEM, SEARCH_ENTRY, VCS_WIDGET]));
        assert_eq!((dialog.selection(), dialog.selected()), ((Side::Right, 2), Some(VCS_WIDGET)));
    }

    #[test]
    fn the_dialog_remembers_a_removed_widget_but_drops_a_removed_action() {
        let mut dialog = CustomizeToolbar::new(layout());
        dialog.select(Side::Right, 1);
        dialog.remove();
        assert_eq!(dialog.layout().right, ids(&[RUN_ITEM]));
        assert!(dialog.layout().hidden.is_empty());
        assert_eq!(dialog.selection(), (Side::Right, 0));
        dialog.remove();
        assert_eq!(dialog.layout().hidden, ids(&[RUN_ITEM]));
        assert_eq!(dialog.selected(), None);
        dialog.remove();
        assert_eq!(dialog.layout().hidden, ids(&[RUN_ITEM]), "nothing is selected");
    }

    #[test]
    fn the_dialog_adds_after_the_selection_and_restores_the_defaults() {
        let mut dialog = CustomizeToolbar::new(ToolbarLayout { hidden: ids(&[BUILD_ITEM]), ..layout() });
        dialog.select(Side::Right, 0);
        dialog.add(BUILD_ITEM);
        assert_eq!(dialog.layout().right, ids(&[RUN_ITEM, BUILD_ITEM, SEARCH_ENTRY]));
        assert!(dialog.layout().hidden.is_empty());
        assert_eq!(dialog.selection(), (Side::Right, 1));
        // Nothing selected on this side: at its end.
        dialog.select(Side::Left, 9);
        dialog.add("action:build.rebuildProject");
        assert_eq!(dialog.layout().left, ids(&[PROJECT_WIDGET, VCS_WIDGET, "action:build.rebuildProject"]));
        dialog.restore_defaults(&built_in_items());
        assert_eq!(dialog.layout(), &default_toolbar(&built_in_items()));
        assert_eq!(dialog.selection(), (Side::Left, 0));
        let write = dialog.save();
        assert_eq!(write.value["right"], json!([BUILD_ITEM, RUN_ITEM, IDE_MODE_ITEM, SEARCH_ENTRY]));
    }

    fn choice(id: &str, title: &str, icon: Option<&str>, hidden: bool) -> ActionChoice {
        ActionChoice {
            id: id.to_owned(),
            title: title.to_owned(),
            category: Some("Build".to_owned()),
            icon: icon.map(str::to_owned),
            hidden,
        }
    }

    #[test]
    fn the_picker_offers_removed_widgets_then_actions_with_icons_by_title() {
        let dialog = CustomizeToolbar::new(ToolbarLayout { hidden: ids(&[RUN_ITEM]), ..layout() });
        let items = [TitleItem { title: Some("Run Widget".to_owned()), ..item(RUN_ITEM, Side::Right, 0.0) }];
        let actions = [
            choice("build.rebuildProject", "Rebuild Project", Some("build-rebuild"), false),
            choice("build.buildProject", "Build Project", Some("build-project"), false),
            choice("navigate.searchEverywhere", "Search Everywhere", Some("search"), false),
            choice("edit.copy", "Copy", None, false),
            choice("internal.thing", "Internal", Some("run"), true),
        ];
        let labels = |query: &str| -> Vec<String> {
            dialog.candidates(&items, &actions, query).into_iter().map(|candidate| candidate.label).collect()
        };
        assert_eq!(labels(""), ["Run Widget", "Build Project", "Rebuild Project"]);
        assert_eq!(labels("  rebuild "), ["Rebuild Project"]);
        let first = &dialog.candidates(&items, &actions, "")[0];
        assert_eq!((first.detail.as_str(), first.icon.as_str()), ("Widget", "main-menu"));
    }

    /// A widget view standing in for a feature's own.
    struct Stand(String);

    impl Render for Stand {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().child(SharedString::from(self.0.clone()))
        }
    }

    type Events = Rc<RefCell<Vec<TitleBarEvent>>>;

    fn register_action(cx: &mut TestAppContext, id: &str, icon: Option<&str>) {
        let mut meta = ActionMeta::new(id, id);
        meta.icon = icon.map(str::to_owned);
        cx.update(|cx| cx.default_global::<ActionRegistry>().register(Action::new(meta, |_, _, _| Ok(()))));
    }

    /// A bar in a window of its own with the built-in widgets drawn by stand-ins, and what it has
    /// asked of the window so far.
    fn open(cx: &mut TestAppContext, frame: TitleBarFrame) -> (Entity<TitleBar>, Events, &mut VisualTestContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        register_action(cx, "navigate.searchEverywhere", Some("search"));
        let (bar, cx) = cx.add_window_view(move |_, cx| {
            let mut bar = TitleBar::new(frame);
            for item in built_in_items() {
                let slot = if item.id == IDE_MODE_ITEM {
                    TitleSlot::IdeMode(Rc::new(|_: &Window, _: &App| Some(false)))
                } else {
                    let name = item.id.clone();
                    TitleSlot::View(cx.new(|_| Stand(name)).into())
                };
                bar.register(item, slot, cx);
            }
            bar.set_project(Some(ProjectWidget { name: "py-demo".to_owned(), root: "/work/py-demo".to_owned() }), cx);
            bar
        });
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&bar, move |_, event: &TitleBarEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        (bar, events, cx)
    }

    fn shown(bar: &Entity<TitleBar>, cx: &mut VisualTestContext) -> (Vec<String>, Vec<String>) {
        cx.update(|window, cx| bar.read(cx).shown(window, cx))
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let at = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
        cx.simulate_click(at, Modifiers::default());
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    async fn draws_the_entries_of_each_side_in_order_around_the_drag_space(cx: &mut TestAppContext) {
        let (bar, _, cx) = open(cx, TitleBarFrame::default());
        assert_eq!(
            shown(&bar, cx),
            (
                ids(&[PROJECT_WIDGET, VCS_WIDGET, MERGE_REBASE_ITEM]),
                ids(&[BUILD_ITEM, RUN_ITEM, IDE_MODE_ITEM, SEARCH_ENTRY])
            )
        );
        let left = |cx: &mut VisualTestContext, selector: &'static str| {
            cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).left()
        };
        let edges = [
            left(cx, "title:project"),
            left(cx, "title:vcs.branch"),
            left(cx, "title:vcs.mergeRebase"),
            left(cx, "title:build.project"),
            left(cx, "title:run.widget"),
            left(cx, "title:app.ideMode"),
            left(cx, "title:action:navigate.searchEverywhere"),
            left(cx, "title:gear"),
        ];
        assert!(edges.windows(2).all(|pair| pair[0] < pair[1]), "{edges:?}");
        // The left side starts at the leading edge, the gear ends at the trailing one.
        let bar_bounds = cx.debug_bounds("title-bar").expect("drawn");
        assert!(edges[0] - bar_bounds.left() < px(16.0));
        assert!(bar_bounds.right() - cx.debug_bounds("title:gear").expect("drawn").right() < px(16.0));
    }

    #[gpui_kit::test]
    async fn hidden_widgets_and_widgets_with_nothing_to_show_are_left_out(cx: &mut TestAppContext) {
        let (bar, _, cx) = open(cx, TitleBarFrame::default());
        bar.update(cx, |bar, cx| {
            let saved = MainToolbarSettings {
                left: Some(ids(&[PROJECT_WIDGET, VCS_WIDGET, MERGE_REBASE_ITEM])),
                right: Some(ids(&[IDE_MODE_ITEM, BUILD_ITEM])),
                hidden: Some(ids(&[RUN_ITEM])),
            };
            bar.set_saved(Some(saved), cx);
            let merge = item(MERGE_REBASE_ITEM, Side::Left, 11.0);
            bar.register(merge, TitleSlot::Dynamic(Rc::new(|_: &Window, _: &App| None)), cx);
        });
        cx.run_until_parked();
        assert_eq!(shown(&bar, cx), (ids(&[PROJECT_WIDGET, VCS_WIDGET]), ids(&[IDE_MODE_ITEM, BUILD_ITEM])));
        assert!(cx.debug_bounds("title:run.widget").is_none());
        assert!(cx.debug_bounds("title:vcs.mergeRebase").is_none());
        assert!(cx.debug_bounds("title:action:navigate.searchEverywhere").is_none());
        // Without a VCS widget the fallback shows only while one of its actions is registered.
        bar.update(cx, |bar, cx| bar.unregister(VCS_WIDGET, cx));
        cx.run_until_parked();
        assert_eq!(shown(&bar, cx).0, ids(&[PROJECT_WIDGET]));
        register_action(cx, "vcs.clone", None);
        bar.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(shown(&bar, cx).0, ids(&[PROJECT_WIDGET, VCS_WIDGET]));
    }

    #[gpui_kit::test]
    async fn a_click_runs_the_entrys_action_or_asks_for_its_menu(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, TitleBarFrame::default());
        click(cx, "title:action:navigate.searchEverywhere");
        click(cx, "title:app.ideMode");
        click(cx, "title:project");
        let events = events.borrow();
        assert_eq!(events[0], TitleBarEvent::RunAction { id: "navigate.searchEverywhere".to_owned(), arg: None });
        assert_eq!(events[1], TitleBarEvent::RunAction { id: "ide.toggleMode".to_owned(), arg: Some(json!("widget")) });
        assert!(matches!(events[2], TitleBarEvent::ProjectMenu { .. }), "{:?}", events[2]);
        assert_eq!(events.len(), 3);
    }

    // src/workbench/TitleBar.test.tsx › title bar gear menu › opens a menu instead of Settings directly, and
    // switches the theme
    #[gpui_kit::test]
    async fn opens_a_menu_instead_of_settings_directly(cx: &mut TestAppContext) {
        let (bar, events, cx) = open(cx, TitleBarFrame::default());
        click(cx, "title:gear");
        let asked = events.borrow().clone();
        let [TitleBarEvent::GearMenu { position }] = asked.as_slice() else { panic!("{asked:?}") };
        let gear = cx.debug_bounds("title:gear").expect("drawn");
        assert!(gear.contains(position), "the menu opens at the gear");
        let menu = cx.update(|_, cx| bar.read(cx).gear_menu(cx));
        assert_eq!(menu[2].action_id(), Some("file.settings"));
    }

    #[gpui_kit::test]
    async fn the_context_menu_lists_the_entries_and_its_rows_write_the_layout(cx: &mut TestAppContext) {
        let (bar, events, cx) = open(cx, TitleBarFrame::default());
        let at = cx.debug_bounds("title-bar").expect("drawn").center();
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        assert!(matches!(events.borrow().last(), Some(TitleBarEvent::ContextMenu { .. })));
        let menu = cx.update(|_, cx| bar.read(cx).customize_menu(cx));
        let rows: Vec<Option<&str>> = menu.iter().filter(|row| checked(row)).map(MenuItem::label).collect();
        assert_eq!(
            rows,
            [
                Some("Project Widget"),
                Some("VCS Widget"),
                Some("Merge/Rebase Widget"),
                Some("Build Project"),
                Some("Run Widget"),
                Some("IDE Mode"),
                Some("navigate.searchEverywhere"),
            ]
        );
        let toggle = menu[4].command().cloned().expect("Run Widget's row");
        let write = bar.read_with(cx, |bar, _| bar.write_for(&toggle));
        assert_eq!(write.value["hidden"], json!([RUN_ITEM]));
        let saved = super::super::main_toolbar::saved_toolbar(&json!({ "appearance": { "mainToolbar": write.value } }));
        bar.update(cx, |bar, cx| bar.set_saved(saved, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("title:run.widget").is_none());
    }

    // src/workbench/TitleBar.test.tsx › title bar in Distraction Free Mode (bare) › $os: keeps the main menu
    // button where there is no native menu bar
    #[gpui_kit::test]
    async fn keeps_the_main_menu_button_where_there_is_no_native_menu_bar(cx: &mut TestAppContext) {
        let platforms = [
            ("windows", TitleBarFrame { frameless: true, overlay: false, fullscreen: false }, true),
            ("macos", TitleBarFrame { frameless: false, overlay: true, fullscreen: false }, false),
            ("linux", TitleBarFrame { frameless: false, overlay: false, fullscreen: false }, false),
        ];
        let (bar, _, cx) = open(cx, TitleBarFrame::default());
        for (os, frame, main_menu) in platforms {
            bar.update(cx, |bar, cx| {
                bar.set_frame(frame, cx);
                bar.set_mode(TitleBarMode::Bare, cx);
            });
            cx.run_until_parked();
            assert!(cx.debug_bounds("title:gear").is_none(), "{os}");
            assert_eq!(cx.debug_bounds("title:mainMenu").is_some(), main_menu, "{os}");
            assert_eq!(shown(&bar, cx), (Vec::new(), Vec::new()), "{os}");
        }
    }

    // src/workbench/TitleBar.test.tsx › expanded main menu in a frameless title bar › hides the right-hand
    // toolbar while the menu is expanded and restores it on close
    #[gpui_kit::test]
    async fn hides_the_right_hand_toolbar_while_the_menu_is_expanded_and_restores_it_on_close(cx: &mut TestAppContext) {
        let (bar, events, cx) = open(cx, TitleBarFrame { frameless: true, overlay: false, fullscreen: false });
        assert!(cx.debug_bounds("title:gear").is_some());
        click(cx, "title:mainMenu");
        assert!(matches!(events.borrow().last(), Some(TitleBarEvent::MainMenu { .. })));
        bar.update(cx, |bar, cx| bar.set_menu_expanded(true, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("title:gear").is_none());
        assert_eq!(shown(&bar, cx), (Vec::new(), Vec::new()));
        bar.update(cx, |bar, cx| bar.set_menu_expanded(false, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("title:gear").is_some());
    }

    // src/workbench/TitleBar.test.tsx › compact window header › shows the window title and follows it, without
    // toolbar widgets
    #[gpui_kit::test]
    async fn shows_the_window_title_and_follows_it_without_toolbar_widgets(cx: &mut TestAppContext) {
        let (bar, _, cx) = open(cx, TitleBarFrame::default());
        bar.update(cx, |bar, cx| {
            bar.set_document_title(Some("demo – src/main.rs".to_owned()), cx);
            bar.set_mode(TitleBarMode::Compact, cx);
        });
        cx.run_until_parked();
        let title = |cx: &mut VisualTestContext| bar.read_with(cx, |bar, _| bar.shown_title().map(str::to_owned));
        assert_eq!(title(cx).as_deref(), Some("demo – src/main.rs"));
        assert!(cx.debug_bounds("title:compact").is_some());
        assert!(cx.debug_bounds("title:gear").is_none() && cx.debug_bounds("title:project").is_none());
        bar.update(cx, |bar, cx| bar.set_document_title(Some("demo – Cargo.toml".to_owned()), cx));
        assert_eq!(title(cx).as_deref(), Some("demo – Cargo.toml"));
    }

    // src/workbench/TitleBar.test.tsx › compact window header › prefers an explicit title (LightEdit)
    #[gpui_kit::test]
    async fn prefers_an_explicit_title(cx: &mut TestAppContext) {
        let (bar, _, cx) = open(cx, TitleBarFrame::default());
        bar.update(cx, |bar, cx| {
            bar.set_document_title(Some("ignored".to_owned()), cx);
            bar.set_title(Some("notes.txt".to_owned()), cx);
            bar.set_mode(TitleBarMode::Compact, cx);
        });
        assert_eq!(bar.read_with(cx, |bar, _| bar.shown_title().map(str::to_owned)).as_deref(), Some("notes.txt"));
        // The full bar shows its own title after the main menu, before the left side.
        bar.update(cx, |bar, cx| bar.set_mode(TitleBarMode::Full, cx));
        cx.run_until_parked();
        let title = cx.debug_bounds("title:window").expect("the title is drawn");
        assert!(title.right() <= cx.debug_bounds("title:project").expect("drawn").left());
    }

    type DialogEvents = Rc<RefCell<Vec<CustomizeToolbarEvent>>>;

    fn open_dialog(cx: &mut TestAppContext) -> (Entity<CustomizeToolbarDialog>, DialogEvents, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let actions = vec![
            choice("build.rebuildProject", "Rebuild Project", Some("build-rebuild"), false),
            choice("navigate.searchEverywhere", "Search Everywhere", Some("search"), false),
        ];
        let (dialog, cx) =
            cx.add_window_view(move |_, cx| CustomizeToolbarDialog::new(layout(), built_in_items(), actions, cx));
        let events: DialogEvents = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&dialog, move |_, event: &CustomizeToolbarEvent, _| seen.borrow_mut().push(event.clone()))
                .detach();
            window.activate_window();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (dialog, events, cx)
    }

    #[gpui_kit::test]
    async fn the_customize_dialog_edits_the_layout_and_ok_applies_it(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open_dialog(cx);
        let rows = |cx: &mut VisualTestContext, side: Side| dialog.read_with(cx, |dialog, _| dialog.rows(side));
        assert_eq!(rows(cx, Side::Left), ["Project Widget", "VCS Widget"]);
        assert_eq!(rows(cx, Side::Right), ["Run Widget", "Search Everywhere"]);
        let enabled = |cx: &mut VisualTestContext, id: &str| {
            dialog.read_with(cx, |dialog, _| dialog.buttons().iter().any(|button| button.id == id && button.enabled))
        };
        assert!(!enabled(cx, "up") && enabled(cx, "down"));

        click(cx, "customize-row:right:0");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.state().selection()), (Side::Right, 0));
        click(cx, "customize-down");
        assert_eq!(rows(cx, Side::Right), ["Search Everywhere", "Run Widget"]);
        click(cx, "customize-across");
        assert_eq!(rows(cx, Side::Left), ["Project Widget", "VCS Widget", "Run Widget"]);
        click(cx, "customize-remove");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.state().layout().hidden.clone()), ids(&[RUN_ITEM]));

        // Add… lists the removed widget first; typing filters; Enter adds the highlighted one.
        click(cx, "customize-add");
        assert!(dialog.read_with(cx, |dialog, _| dialog.is_adding()));
        let labels = |cx: &mut VisualTestContext| -> Vec<String> {
            cx.update(|_, cx| dialog.read(cx).candidates(cx).into_iter().map(|candidate| candidate.label).collect())
        };
        assert_eq!(labels(cx), ["Run Widget", "Rebuild Project"]);
        cx.simulate_input("rebuild");
        assert_eq!(labels(cx), ["Rebuild Project"]);
        cx.simulate_keystrokes("enter");
        assert!(!dialog.read_with(cx, |dialog, _| dialog.is_adding()));
        assert_eq!(rows(cx, Side::Left), ["Project Widget", "VCS Widget", "Rebuild Project"]);

        click(cx, "customize-ok");
        let expected = ToolbarLayout {
            left: ids(&[PROJECT_WIDGET, VCS_WIDGET, "action:build.rebuildProject"]),
            right: ids(&[SEARCH_ENTRY]),
            hidden: ids(&[RUN_ITEM]),
        };
        assert_eq!(*events.borrow(), [CustomizeToolbarEvent::Apply(expected)]);
    }

    #[gpui_kit::test]
    async fn escape_closes_the_picker_first_then_cancels_the_dialog(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open_dialog(cx);
        click(cx, "customize-row:right:0");
        click(cx, "customize-remove");
        click(cx, "customize-add");
        cx.simulate_keystrokes("down");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.active_candidate()), 1);
        cx.simulate_keystrokes("escape");
        assert!(!dialog.read_with(cx, |dialog, _| dialog.is_adding()));
        assert!(events.borrow().is_empty());
        cx.simulate_keystrokes("escape");
        assert_eq!(*events.borrow(), [CustomizeToolbarEvent::Cancelled]);
        click(cx, "customize-restore");
        let restored = dialog.read_with(cx, |dialog, _| dialog.state().layout().clone());
        assert_eq!(restored, default_toolbar(&built_in_items()));
    }
}
