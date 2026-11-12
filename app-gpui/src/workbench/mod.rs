//! The window shell. For now: a frame around one editor (LightEdit), which is the part of the
//! workbench the editor slice needs. Tool windows, the title bar and the status bar follow.

pub mod main_toolbar;
pub mod menu;
pub mod menu_spec;
pub mod notification_balloons;
pub mod notifications;
pub mod notifications_view;
pub mod project_color;
pub mod quick_switch;
pub mod status_bar_widgets;
pub mod tasks;
pub mod tool_windows;
pub mod welcome;
pub mod welcome_model;
pub mod windows;

use crate::backend::{Backend, BackendEvent};
use crate::editor::find_bar::{
    FindBar, FindBarEvent, FindResults, find_again_in, scroll_to_results, toggle_scroll_to_results,
};
use crate::editor::goto_line::{GotoLine, GotoLineEvent};
use crate::editor::session::{self, FileViewState};
use crate::editor::tabs::{
    EditorInput, LayoutNode, OpenTabOptions, SplitDir, SplitEdge, TabsState, disambiguate_titles,
};
use crate::editor::view::Editor;
use crate::everywhere::bookmarks_popup::{
    BookmarksPopup, BookmarksPopupEvent, MnemonicChooser, MnemonicChooserEvent, neighbour_bookmark,
    selection_description,
};
use crate::everywhere::bookmarks_store::{
    BookmarkStore, MnemonicTarget, bookmark_by_mnemonic, bookmarks_key, lists_key, toggle_bookmark, toggle_mnemonic,
    toggle_path_bookmarks,
};
use crate::everywhere::model::SeTab;
use crate::everywhere::open::StorageWrite;
use crate::everywhere::recent_model::OpenEditorTab;
use crate::everywhere::recent_view::{RecentFiles, RecentFilesEvent, RecentFilesInput, RecentFilesKind};
use crate::everywhere::structure_view::{FileStructure, FileStructureEvent, StructureFile};
use crate::everywhere::view::{SearchEverywhere, SearchEverywhereEvent};
use crate::find::popup_view::{FindPopup, FindPopupEvent, FindPopupInput};
use crate::find::store::{FindArg, FindPopupCommand, parse_find_arg};
use crate::platform::actions::{Action, ActionMeta, ActionRegistry, ActionSource, execute_action_from, manifest};
use crate::platform::input;
use crate::platform::local_time::now_ms;
use crate::platform::storage::UiStorage;
use crate::project_tree::{OpenFile, ProjectTree};
use crate::run::driver::{RunDriver, RunDriverEvent, SaveAll};
use crate::run::sessions::{StopTarget, rerun_target, stop_target};
use crate::run::store::RunStore;
use crate::run::view::{RunView, RunViewEvent};
use crate::theme;
use crate::ui::popup_menu::{Choice, PopupMenu, PopupMenuEvent};
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::Subscription;
use gpui_kit::{AnyView, FocusHandle};
use gpui_kit::{
    App, Context, Entity, Focusable, Global, IntoElement, ModifiersChangedEvent, MouseDownEvent, Render, SharedString,
    WeakEntity, Window, WindowId, div, prelude::*,
};
use ide_core::IdeEvent;
use ide_core::event::NotifyLevel;
use notification_balloons::{BalloonEvent, EntryAction, NotificationBalloons, balloon_entries};
use notifications::{
    BalloonClocks, BalloonView, NotificationLevel, NotificationRequest, NotificationSettings, Notifications,
    notification_settings, suppress_group,
};
use notifications_view::{NotificationsEvent, NotificationsView, logged_entries};
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;
use tasks::{TaskEvent, Tasks};
use tool_windows::{Anchor, Effects, Slot, Stripe, ToolWindow, ToolWindowLayout, storage_key};
use welcome::{Welcome, WelcomeEvent};

/// What a window shows.
#[expect(clippy::large_enum_variant, reason = "one per window, replaced only when a project opens or closes")]
enum Content {
    /// Nothing is open.
    Empty,
    Opening(PathBuf),
    Editor(Entity<Editor>),
    /// A project: its file tree and the file being edited.
    Project {
        name: String,
        /// Which tool windows are shown, where and how large.
        layout: Box<ToolWindowLayout>,
        /// The last focus request of `layout` that was carried out.
        focus_served: u64,
        tree: Entity<ProjectTree>,
        /// The view of every tool window that has been shown, by tool window id. A hidden
        /// window keeps its view, and what runs in it.
        panels: HashMap<String, Panel>,
        /// The editor tabs, and the editor of each tab that has finished opening, by tab id.
        tabs: Box<TabsState>,
        editors: HashMap<String, Entity<Editor>>,
        /// Tabs whose document is being opened.
        loading: std::collections::HashSet<String>,
        /// Where the files of the restored session were left, for the ones not opened yet.
        restored_views: std::collections::BTreeMap<String, FileViewState>,
        /// Where to put the caret in a file that is being opened: its line and UTF-16 column.
        reveals: HashMap<String, (u32, u32)>,
        /// What tells the window that a tab's editor took the keyboard, by tab id: its pane is
        /// the active one from then on.
        editor_focus: HashMap<String, Subscription>,
        /// The Find / Replace bar of each tab that has one open, by tab id.
        find_bars: HashMap<String, TabFindBar>,
        _opens: Subscription,
        /// What runs the project's run configurations, and what the window listens to it for.
        run: Entity<RunDriver>,
        _run_events: Subscription,
    },
    Failed(SharedString),
}

/// An editor tab's Find / Replace bar, with what the window listens to it for.
struct TabFindBar {
    bar: Entity<FindBar>,
    _subscriptions: Vec<Subscription>,
}

/// A tool window's content and where its keyboard focus goes.
struct Panel {
    view: AnyView,
    /// Where the keyboard goes when the window is given it: asked each time, since a view may
    /// hand it on to a part of itself that changes (the terminal's active tab).
    focus: Rc<dyn Fn(&App) -> FocusHandle>,
    /// What the window listens to the view for, kept for as long as the view is.
    _events: Option<Subscription>,
}

/// The longest editor selection, in UTF-16 code units, that Search Everywhere starts with.
const SELECTION_QUERY_LIMIT: usize = 200;

/// What a row of an editor tab's menu does to the tab it was opened on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabCommand {
    Close,
    CloseOthers,
    CloseLeft,
    CloseRight,
    CloseAll,
    CloseUnmodified,
    CloseAllButPinned,
    TogglePin,
    Keep,
    SplitAndMoveRight,
    SplitAndMoveDown,
    SplitRight,
    SplitDown,
    MoveToOpposite,
    OpenInOpposite,
    ChangeOrientation,
    Unsplit,
    UnsplitAll,
}

/// The encodings a file can be reloaded in or converted to, with the names they are shown under.
pub const ENCODINGS: [(&str, &str); 11] = [
    ("utf-8", "UTF-8"),
    ("utf-16le", "UTF-16LE"),
    ("utf-16be", "UTF-16BE"),
    ("gbk", "GBK"),
    ("gb18030", "GB18030"),
    ("big5", "Big5"),
    ("shift_jis", "Shift_JIS"),
    ("euc-kr", "EUC-KR"),
    ("windows-1252", "windows-1252"),
    ("iso-8859-1", "ISO-8859-1"),
    ("koi8-r", "KOI8-R"),
];

/// What a row of a status bar item's menu does to the file being edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusCommand {
    LineEnding(ide_text::LineEnding),
    /// Read the file again in this encoding.
    Reload(String),
    /// Keep the text and save it in this encoding.
    Convert(String),
}

/// A popup over the window and where the keyboard goes when it closes.
struct Popup {
    view: AnyView,
    /// What had the keyboard when the popup opened.
    restore: Option<FocusHandle>,
    _events: Subscription,
}

/// How long the tabs rest before the editor session is written.
const SESSION_SAVE_DELAY: std::time::Duration = std::time::Duration::from_millis(400);

impl Panel {
    fn new<V: Focusable + Render>(view: Entity<V>, events: Option<Subscription>) -> Self {
        let focused = view.clone();
        Self { view: view.into(), focus: Rc::new(move |cx| focused.focus_handle(cx)), _events: events }
    }
}

/// One window: its backend label and what it shows.
pub struct Workbench {
    /// The label the backend knows this window by.
    label: String,
    content: Content,
    /// Redraws the status bar when the shown editor changes.
    editor_observer: Option<Subscription>,
    /// The title the window currently carries.
    shown_title: Option<String>,
    /// Background work the backend reports for this window, and what it tells the user.
    tasks: Tasks,
    notifications: Notifications,
    /// Wakes the task list when its next deadline (a finished task leaving, a slow one
    /// appearing) falls due.
    task_timer: Option<gpui_kit::Task<()>>,
    listener: u64,
    /// The editor that was active before the last tab change: it is saved when another takes over.
    previous_editor: Option<gpui_kit::WeakEntity<Editor>>,
    /// The unsaved-changes question is on screen.
    closing: bool,
    /// A pending write of the editor session.
    session_save: Option<gpui_kit::Task<()>>,
    /// The popup that is open over everything else in the window.
    popup: Option<Popup>,
    /// When each tab's editor was last the active one, by the key of what it shows: a count that
    /// grows with every change of the active editor. The Switcher lists the open editors by it.
    activations: HashMap<String, u64>,
    /// The key of the editor that is active, to tell a change from a repeat.
    active_key: Option<String>,
    /// The bookmarks of the window's project.
    bookmarks: BookmarkStore,
    /// The balloons at the bottom right of the window, and what times their fading.
    balloons: Entity<NotificationBalloons>,
    balloon_clocks: BalloonClocks,
    balloon_view: BalloonView,
    /// Wakes the balloons when the next one is due to fade.
    balloon_timer: Option<gpui_kit::Task<()>>,
    _balloon_events: Subscription,
    /// The Welcome screen, while the window shows nothing, and what the window listens to it for.
    welcome: Option<(Entity<Welcome>, Subscription)>,
}

/// Window title, IDEA style: `<project> – <file>`; with "Always show full path in window header"
/// the file's full path. A remote project names its host: `<project> [<user@host>] – <file>`.
/// Without a project the file alone, or `default` when nothing is open.
pub fn window_title(
    project: Option<&str>,
    file: Option<&std::path::Path>,
    full_path: bool,
    host: Option<&str>,
    default: &str,
) -> String {
    let name = file.map(|file| match file.file_name().filter(|_| !full_path) {
        Some(name) => name.to_string_lossy().into_owned(),
        None => file.to_string_lossy().into_owned(),
    });
    let Some(project) = project.filter(|p| !p.is_empty()) else { return name.unwrap_or_else(|| default.to_owned()) };
    let place = match host {
        Some(host) => format!("{project} [{host}]"),
        None => project.to_owned(),
    };
    match name {
        Some(name) => format!("{place} – {name}"),
        None => place,
    }
}

/// What a tab shows: the file's name, the folder that tells it apart from a tab of the same
/// name, and whether it is the selected one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabTitle {
    pub name: String,
    pub hint: Option<String>,
    pub active: bool,
}

/// How a workbench window is created: the title bar is the app's own, drawn under the native
/// window buttons (the webview's "Overlay" title bar).
pub fn window_options() -> gpui_kit::WindowOptions {
    gpui_kit::WindowOptions {
        titlebar: Some(gpui_kit::TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: Some(gpui_kit::point(gpui_kit::px(13.0), gpui_kit::px(13.0))),
        }),
        ..Default::default()
    }
}

/// Room the title bar leaves for the native macOS window buttons.
const WINDOW_BUTTONS_WIDTH: f32 = 82.0;

/// The Project view's tool window id.
pub const PROJECT_TOOL_WINDOW: &str = "project";

/// The Terminal's tool window id.
pub const TERMINAL_TOOL_WINDOW: &str = "terminal";

/// The Git log's tool window id.
pub const GIT_TOOL_WINDOW: &str = "git";

/// The Find tool window's id.
pub const FIND_TOOL_WINDOW: &str = "find";

/// The Commit tool window's id.
pub const COMMIT_TOOL_WINDOW: &str = "commit";

/// The Notifications tool window's id.
pub const NOTIFICATIONS_TOOL_WINDOW: &str = "notifications";

/// The Run tool window's id.
pub const RUN_TOOL_WINDOW: &str = "run";

/// The tool windows this app has a view for.
const TOOL_WINDOWS: [&str; 7] = [
    PROJECT_TOOL_WINDOW,
    COMMIT_TOOL_WINDOW,
    TERMINAL_TOOL_WINDOW,
    GIT_TOOL_WINDOW,
    FIND_TOOL_WINDOW,
    NOTIFICATIONS_TOOL_WINDOW,
    RUN_TOOL_WINDOW,
];

/// What the webview UI declares for tool window `id` (title, icon, side, order…), from the
/// contributions manifest.
pub fn tool_window_declaration(id: &str) -> Option<ToolWindow> {
    static DECLARED: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let manifest = DECLARED.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/gen/contributions.json"))
            .expect("assets/gen/contributions.json is generated and valid")
    });
    let declared = manifest
        .get("registerToolWindow")?
        .as_array()?
        .iter()
        .find(|w| w.get("id").and_then(|v| v.as_str()) == Some(id))?;
    let text = |key: &str| declared.get(key).and_then(|v| v.as_str());
    let anchor = match text("anchor")? {
        "left" => Anchor::Left,
        "right" => Anchor::Right,
        "bottom" => Anchor::Bottom,
        _ => return None,
    };
    let flag = |key: &str| declared.get(key).and_then(|v| v.as_bool());
    Some(ToolWindow {
        order: declared.get("order").and_then(|v| v.as_f64()),
        secondary: flag("secondary").unwrap_or(false),
        stripe: if text("stripe") == Some("onUse") { Stripe::OnUse } else { Stripe::Shown },
        verbatim: flag("verbatim").unwrap_or(false),
        requires_project: flag("requiresProject").unwrap_or(true),
        ..ToolWindow::new(id, text("title")?, text("icon")?, anchor)
    })
}

/// Registers the workbench's actions: one `toolwindow.<id>` per tool window this app has a view
/// for, and the window-level ones (hide all, jump to last, hide active, focus editor).
pub fn register_actions(cx: &mut App) {
    fn on_layout(
        window: &mut Window,
        cx: &mut App,
        change: impl FnOnce(&mut ToolWindowLayout) -> Effects,
    ) -> anyhow::Result<()> {
        let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
        workbench.update(cx, |workbench, cx| workbench.with_layout(window, cx, change));
        Ok(())
    }
    windows::register_actions(cx);
    let registry = cx.default_global::<ActionRegistry>();
    // View › Appearance: each item shows and flips one global appearance setting.
    type Flag = fn(&ide_core::settings::AppearanceSettings) -> bool;
    let flags: [(&str, &str, Flag); 4] = [
        ("view.toggleCompactMode", "appearance.compactMode", |a| a.compact_mode),
        ("view.toggleToolbar", "appearance.showMainToolbar", |a| a.show_main_toolbar),
        ("view.toggleToolWindowBars", "appearance.showToolWindowBars", |a| a.show_tool_window_bars),
        ("view.toggleStatusBar", "appearance.showStatusBar", |a| a.show_status_bar),
    ];
    for (id, key, flag) in flags {
        registry.handle_toggle(
            id,
            move |_, cx| flag(&crate::platform::settings::global(cx).appearance),
            move |_, _, cx| {
                use ide_workspace::settings::SettingsScope;
                let next = !flag(&crate::platform::settings::global(cx).appearance);
                crate::platform::settings::set("", SettingsScope::Global, key, &next.into(), cx)?;
                Ok(())
            },
        );
    }
    registry.handle("toolwindow.maximize", |_, window, cx| {
        on_layout(window, cx, |layout| match layout.focused().or(layout.last_active()).map(str::to_owned) {
            Some(id) => layout.toggle_maximize(&id),
            None => Effects::default(),
        })
    });
    use tool_windows::ResizeDirection;
    // Their ids are built from the direction, so they are not in the generated list of actions.
    for (id, title, direction) in [
        ("toolwindow.resizeLeft", "Stretch to Left", ResizeDirection::Left),
        ("toolwindow.resizeRight", "Stretch to Right", ResizeDirection::Right),
        ("toolwindow.resizeUp", "Stretch to Top", ResizeDirection::Up),
        ("toolwindow.resizeDown", "Stretch to Bottom", ResizeDirection::Down),
    ] {
        let meta = ActionMeta {
            category: Some("Window".to_owned()),
            keywords: ["resize tool window", "tool window size"].map(str::to_owned).to_vec(),
            ..ActionMeta::new(id, title).when("toolWindowFocus")
        };
        registry.register(Action::new(meta, move |_, window, cx| {
            let size = window.viewport_size();
            on_layout(window, cx, |layout| {
                layout.resize_active(direction, f64::from(f32::from(size.width)), f64::from(f32::from(size.height)))
            })
        }));
    }
    for id in TOOL_WINDOWS {
        let Some(declared) = tool_window_declaration(id) else { continue };
        let meta = ActionMeta {
            category: Some("Tool Windows".to_owned()),
            icon: Some(declared.icon.clone()),
            keywords: ["tool window", "show", "hide"].map(str::to_owned).to_vec(),
            verbatim: declared.verbatim,
            ..ActionMeta::new(&format!("toolwindow.{id}"), &declared.title)
        };
        registry.register(Action::new(meta, move |_, window, cx| on_layout(window, cx, |layout| layout.activate(id))));
    }
    fn on_tabs(
        window: &mut Window,
        cx: &mut App,
        change: impl FnOnce(&mut TabsState, &str) -> bool,
    ) -> anyhow::Result<()> {
        let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
        workbench.update(cx, |workbench, cx| workbench.with_tabs(window, cx, change));
        Ok(())
    }
    /// The active tab of `pane`, for actions that act on it.
    fn active(tabs: &TabsState, pane: &str) -> Option<String> {
        tabs.pane(pane).and_then(|pane| pane.active.clone())
    }
    use crate::terminal::panel::TerminalPanel;
    fn on_terminals(
        window: &mut Window,
        cx: &mut App,
        act: impl FnOnce(&mut TerminalPanel, &mut Context<TerminalPanel>),
    ) -> anyhow::Result<()> {
        let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
        let terminals = workbench
            .read(cx)
            .tool_window_view(TERMINAL_TOOL_WINDOW)
            .and_then(|view| view.downcast::<TerminalPanel>().ok())
            .ok_or_else(|| anyhow::anyhow!("the Terminal has not been opened"))?;
        terminals.update(cx, act);
        Ok(())
    }
    registry.handle("terminal.new", |_, window, cx| {
        // A Terminal that exists gets the tab first and is shown then. One that this shows for
        // the first time starts with a tab of its own, which is the new tab, unless it came
        // back with remembered tabs: then the new one is added to them.
        let existed = on_terminals(window, cx, |terminals, cx| terminals.open_tab(cx)).is_ok();
        on_layout(window, cx, |layout| layout.show(TERMINAL_TOOL_WINDOW, true))?;
        if !existed {
            on_terminals(window, cx, |terminals, cx| {
                if !terminals.take_fresh() {
                    terminals.open_tab(cx);
                }
            })?;
        }
        // The keyboard goes to the tab that was opened.
        on_layout(window, cx, |layout| layout.focus(TERMINAL_TOOL_WINDOW))
    });
    registry.handle("terminal.clear", |_, window, cx| {
        on_terminals(window, cx, |terminals, cx| {
            if let Some(terminal) = terminals.active_view().cloned() {
                terminal.update(cx, |terminal, cx| terminal.clear(cx));
            }
        })
    });
    registry.handle("terminal.closeSession", |_, window, cx| {
        on_terminals(window, cx, |terminals, cx| terminals.close_active_tab(cx))
    });
    for (id, tab) in [
        ("navigate.searchEverywhere", SeTab::All),
        ("navigate.gotoClass", SeTab::Classes),
        ("navigate.gotoFile", SeTab::Files),
        ("navigate.gotoSymbol", SeTab::Symbols),
        ("navigate.findAction", SeTab::Actions),
        ("navigate.searchText", SeTab::Text),
    ] {
        registry.handle(id, move |_, window, cx| {
            let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
            workbench.update(cx, |workbench, cx| workbench.show_search_everywhere(tab, window, cx));
            Ok(())
        });
    }
    for (id, kind) in
        [("navigate.recentFiles", None), ("navigate.switcher", Some(false)), ("navigate.switcherBack", Some(true))]
    {
        registry.handle(id, move |_, window, cx| {
            let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
            let kind = match kind {
                None => RecentFilesKind::Recent,
                Some(backward) => RecentFilesKind::Switcher { backward, control_held: window.modifiers().control },
            };
            workbench.update(cx, |workbench, cx| workbench.show_recent_files(kind, window, cx));
            Ok(())
        });
    }
    type Shown = fn(&mut Workbench, &mut Window, &mut Context<Workbench>);
    let shown: [(&str, Shown); 5] = [
        ("navigate.gotoLine", Workbench::show_goto_line),
        ("navigate.showBookmarks", Workbench::show_bookmarks),
        ("navigate.toggleBookmarkWithMnemonic", Workbench::show_mnemonic_chooser),
        ("navigate.nextBookmark", |workbench, window, cx| workbench.go_to_neighbour_bookmark(true, window, cx)),
        ("navigate.prevBookmark", |workbench, window, cx| workbench.go_to_neighbour_bookmark(false, window, cx)),
    ];
    for (id, show) in shown {
        registry.handle(id, move |_, window, cx| {
            let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
            workbench.update(cx, |workbench, cx| show(workbench, window, cx));
            Ok(())
        });
    }
    registry.handle("navigate.toggleBookmark", |call, window, cx| {
        let place = call.arg.and_then(|arg| {
            Some((arg.get("path")?.as_str()?.to_owned(), u32::try_from(arg.get("line")?.as_u64()?).ok()?))
        });
        let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
        workbench.update(cx, |workbench, cx| workbench.toggle_bookmark(place, cx));
        Ok(())
    });
    registry.handle("bookmarks.toggleFileBookmark", |call, window, cx| {
        let targets: Vec<(String, bool)> = call
            .arg
            .and_then(|arg| arg.get("targets")?.as_array())
            .into_iter()
            .flatten()
            .filter_map(|target| {
                let dir = target.get("dir").and_then(serde_json::Value::as_bool).unwrap_or(false);
                Some((target.get("path")?.as_str()?.to_owned(), dir))
            })
            .collect();
        let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
        workbench.update(cx, |workbench, cx| workbench.toggle_file_bookmarks(targets, cx));
        Ok(())
    });
    // One pair of actions per digit: their ids are built from it, so they are not in the
    // generated list of actions.
    for digit in '0'..='9' {
        let toggle = ActionMeta {
            category: Some("Bookmarks".to_owned()),
            ..ActionMeta::new(&format!("navigate.toggleBookmark{digit}"), &format!("Toggle Bookmark {digit}"))
                .when("projectOpen && textEditorActive")
        };
        registry.register(Action::new(toggle, move |_, window, cx| {
            let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
            workbench.update(cx, |workbench, cx| workbench.toggle_bookmark_digit(digit, cx));
            Ok(())
        }));
        let go = ActionMeta {
            category: Some("Bookmarks".to_owned()),
            ..ActionMeta::new(&format!("navigate.gotoBookmark{digit}"), &format!("Go to Bookmark {digit}"))
                .when("projectOpen")
        };
        registry.register(Action::new(go, move |_, window, cx| {
            let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
            workbench.update(cx, |workbench, cx| workbench.go_to_bookmark(digit, window, cx));
            Ok(())
        }));
    }
    fn on_run(
        window: &mut Window,
        cx: &mut App,
        act: impl FnOnce(&mut RunDriver, &mut Context<RunDriver>),
    ) -> anyhow::Result<()> {
        let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
        let driver = workbench.read(cx).run_driver().cloned().ok_or_else(|| anyhow::anyhow!("no project is open"))?;
        driver.update(cx, act);
        Ok(())
    }
    registry.handle("run.run", |_, window, cx| {
        on_run(window, cx, |driver, cx| driver.when_loaded(|driver, cx| driver.run_selected(cx), cx))
    });
    registry.handle("run.runConfig", |call, window, cx| {
        let arg = call.arg.cloned();
        on_run(window, cx, move |driver, cx| {
            driver.when_loaded(
                move |driver, cx| {
                    if let Some(config) = driver.config_from_arg(arg.as_ref(), cx) {
                        driver.start(config, Default::default(), cx).detach();
                    }
                },
                cx,
            )
        })
    });
    registry.handle("run.rerun", |_, window, cx| {
        on_run(window, cx, |driver, cx| {
            let target = rerun_target(driver.store().read(cx)).map(str::to_owned);
            if let Some(rerun) = target.and_then(|session| driver.rerun(&session, cx)) {
                rerun.detach();
            }
        })
    });
    registry.handle("run.stop", |call, window, cx| {
        let focused = call.ctx.is_truthy("toolWindowFocus")
            && matches!(
                call.ctx.get("activeToolWindow"),
                Some(crate::platform::context::ContextValue::Text(id)) if &**id == RUN_TOOL_WINDOW
            );
        let from_widget = call.arg.and_then(serde_json::Value::as_str) == Some("widget");
        let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
        workbench.update(cx, |workbench, cx| workbench.stop_runs(focused, from_widget, window, cx));
        Ok(())
    });
    registry.handle("run.stopProcess", |call, window, cx| {
        let Some(run) = call.arg.and_then(serde_json::Value::as_u64).and_then(|run| u32::try_from(run).ok()) else {
            return Ok(());
        };
        on_run(window, cx, move |driver, cx| driver.stop_run(ide_core::RunId(run), cx))
    });
    registry.handle("run.reloadConfigs", |_, window, cx| {
        on_run(window, cx, |driver, cx| {
            if let Some(root) = driver.store().read(cx).project_root.clone() {
                driver.load(&root, cx);
            }
        })
    });
    registry.handle("view.notifications", |_, window, cx| {
        on_layout(window, cx, |layout| layout.activate(NOTIFICATIONS_TOOL_WINDOW))
    });
    registry.handle("navigate.fileStructure", |_, window, cx| {
        let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
        workbench.update(cx, |workbench, cx| workbench.show_file_structure(window, cx));
        Ok(())
    });
    for (id, replace) in [("find.inPath", false), ("find.replaceInPath", true)] {
        registry.handle(id, move |call, window, cx| {
            let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
            let arg = parse_find_arg(call.arg, call.ctx);
            workbench.update(cx, |workbench, cx| workbench.show_find_popup(replace, arg, window, cx));
            Ok(())
        });
    }
    for (id, replace) in [("find.inFile", false), ("find.replaceInFile", true)] {
        registry.handle(id, move |_, window, cx| {
            let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
            workbench.update(cx, |workbench, cx| workbench.show_find_bar(replace, window, cx));
            Ok(())
        });
    }
    for (id, forward) in [("find.next", true), ("find.previous", false)] {
        registry.handle(id, move |_, window, cx| {
            let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
            workbench.update(cx, |workbench, cx| workbench.find_again(forward, window, cx));
            Ok(())
        });
    }
    registry.handle("find.openInWindow", |_, window, cx| {
        let bar = for_window(window, cx).and_then(|workbench| workbench.read(cx).focused_find_bar(window, cx));
        if let Some(bar) = bar {
            bar.update(cx, FindBar::open_results);
        }
        Ok(())
    });
    registry.handle_toggle(
        "find.scrollToResults",
        |_, cx| scroll_to_results(cx),
        |_, _, cx| {
            toggle_scroll_to_results(cx);
            Ok(())
        },
    );
    for (id, command) in [
        ("find.popup.nextResult", FindPopupCommand::NextResult),
        ("find.popup.previousResult", FindPopupCommand::PreviousResult),
        ("find.popup.openResult", FindPopupCommand::OpenResult),
        ("find.popup.recentSearches", FindPopupCommand::RecentSearches),
    ] {
        registry.handle(id, move |_, window, cx| {
            let popup = for_window(window, cx).and_then(|workbench| workbench.read(cx).find_popup());
            if let Some(popup) = popup {
                popup.update(cx, |popup, cx| popup.run_command(command, window, cx));
            }
            Ok(())
        });
    }
    registry.handle("view.toggleTheme", |_, window, cx| {
        let label = for_window(window, cx).map(|workbench| workbench.read(cx).label.clone()).unwrap_or_default();
        crate::theme::appearance::toggle_theme(&label, cx)
    });
    registry.handle_toggle(
        "ide.toggleMode",
        |window, cx| for_window(window, cx).is_some_and(|workbench| workbench.read(cx).ide_mode(cx).is_full()),
        |_, window, cx| {
            let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
            workbench.update(cx, |workbench, cx| workbench.toggle_ide_mode(cx));
            Ok(())
        },
    );
    registry.handle("editor.closeTab", |_, window, cx| {
        on_tabs(window, cx, |tabs, pane| active(tabs, pane).is_some_and(|tab| tabs.close_tab(pane, &tab)))
    });
    registry.handle("editor.closeOtherTabs", |_, window, cx| {
        on_tabs(window, cx, |tabs, pane| active(tabs, pane).is_some_and(|tab| tabs.close_others(pane, &tab)))
    });
    registry
        .handle("editor.closeAllTabs", |_, window, cx| on_tabs(window, cx, |tabs, pane| tabs.close_all(pane, false)));
    registry.handle("editor.pinTab", |_, window, cx| {
        on_tabs(window, cx, |tabs, pane| active(tabs, pane).is_some_and(|tab| tabs.toggle_pin(pane, &tab)))
    });
    registry.handle("editor.nextTab", |_, window, cx| on_tabs(window, cx, |tabs, _| tabs.cycle_tab(1, None)));
    registry.handle("editor.prevTab", |_, window, cx| on_tabs(window, cx, |tabs, _| tabs.cycle_tab(-1, None)));
    type OnWorkbench = fn(&mut Workbench, &mut Window, &mut Context<Workbench>);
    let on_panes: [(&str, OnWorkbench); 8] = [
        ("editor.splitRight", |workbench, window, cx| workbench.split_editor(SplitDir::Row, window, cx)),
        ("editor.splitDown", |workbench, window, cx| workbench.split_editor(SplitDir::Column, window, cx)),
        ("editor.moveToOppositeGroup", Workbench::move_tab_to_opposite_group),
        ("editor.openInOppositeGroup", Workbench::open_in_opposite_group),
        ("editor.nextSplitter", |workbench, window, cx| workbench.focus_splitter(true, window, cx)),
        ("editor.prevSplitter", |workbench, window, cx| workbench.focus_splitter(false, window, cx)),
        ("editor.unsplit", |workbench, window, cx| {
            workbench.with_tabs(window, cx, |tabs, pane| tabs.remove_pane(pane))
        }),
        ("editor.unsplitAll", |workbench, window, cx| {
            workbench.with_tabs(window, cx, |tabs, pane| tabs.unsplit_all(pane))
        }),
    ];
    for (id, act) in on_panes {
        registry.handle(id, move |_, window, cx| {
            let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
            workbench.update(cx, |workbench, cx| act(workbench, window, cx));
            Ok(())
        });
    }
    registry.handle("editor.changeSplitOrientation", |_, window, cx| {
        on_tabs(window, cx, |tabs, pane| tabs.change_split_orientation(pane))
    });
    registry.handle("editor.reopenClosedTab", |_, window, cx| {
        let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
        workbench.update(cx, |workbench, cx| workbench.reopen_closed_tab(window, cx));
        Ok(())
    });
    // In a window showing the Welcome screen the keyboard goes to its search; a window with a
    // project has no Recent Projects popup to show.
    for id in ["welcome.recentProjects", "file.openRecent"] {
        registry.handle(id, |_, window, cx| {
            let welcome = for_window(window, cx).and_then(|workbench| workbench.read(cx).welcome().cloned());
            if let Some(welcome) = welcome {
                welcome.update(cx, |view, cx| view.focus_search(window, cx));
            }
            Ok(())
        });
    }
    debug_assert!(manifest().contains_key("toolwindow.hideAll"));
    registry.handle("toolwindow.hideAll", |_, window, cx| on_layout(window, cx, ToolWindowLayout::hide_all));
    registry.handle("toolwindow.jumpToLast", |_, window, cx| on_layout(window, cx, ToolWindowLayout::jump_to_last));
    registry.handle("toolwindow.hideActive", |_, window, cx| on_layout(window, cx, ToolWindowLayout::hide_active));
    registry.handle("toolwindow.focusEditor", |_, window, cx| {
        on_layout(window, cx, |_| Effects { persist: false, focus_editor: true })
    });
}

/// The workbench of each window, so that an action can find what it acts on.
#[derive(Default)]
struct Workbenches(HashMap<WindowId, WeakEntity<Workbench>>);

impl Global for Workbenches {}

/// The workbench shown in `window`.
pub fn for_window(window: &Window, cx: &App) -> Option<Entity<Workbench>> {
    cx.try_global::<Workbenches>()?.0.get(&window.window_handle().window_id())?.upgrade()
}

/// The editor an action invoked in `window` acts on.
pub fn active_editor(window: &Window, cx: &App) -> Option<Entity<Editor>> {
    for_window(window, cx)?.read(cx).editor().cloned()
}

impl Workbench {
    /// The workbench of `window`, known to the backend as `label`.
    pub fn new(label: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Closing the window asks about unsaved changes first.
        let guard = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            guard.update(cx, |workbench, cx| workbench.confirm_close(window, cx)).unwrap_or(true)
        });
        cx.observe_window_appearance(window, |_, window, cx| {
            crate::theme::appearance::system_appearance_changed(window.appearance(), cx);
        })
        .detach();
        // Leaving the window saves what is unsaved, as switching tabs does.
        cx.observe_window_activation(window, |workbench, window, cx| {
            if !window.is_window_active() {
                workbench.autosave(None, cx);
                workbench.save_session(cx);
            }
            // A balloon does not fade while nobody can see it.
            workbench.balloon_view.window_active = window.is_window_active();
            workbench.sync_notifications(cx);
            // Another window may have opened or forgotten a project meanwhile.
            if window.is_window_active() {
                workbench.sync_welcome(window, cx);
            }
        })
        .detach();
        let id = window.window_handle().window_id();
        let this = cx.weak_entity();
        cx.default_global::<Workbenches>().0.insert(id, this);
        // Events for this window, and those for every window, reach its models.
        let weak = cx.weak_entity();
        let listener = cx.try_global::<Backend>().is_some().then(|| {
            cx.global_mut::<Backend>().listen(move |event, cx| {
                let _ = weak.update(cx, |workbench, cx| workbench.backend_event(event, cx));
            })
        });
        cx.on_release(move |workbench, cx| {
            // The window is going: the session is written as it stands.
            workbench.save_session(cx);
            if cx.has_global::<Backend>() {
                cx.global_mut::<Backend>().unlisten(workbench.listener);
            }
            cx.default_global::<Workbenches>().0.remove(&id);
            input::window_blurred(id, cx);
            crate::platform::context::drop_window_context(id, cx);
        })
        .detach();
        let balloons = cx.new(|_| NotificationBalloons::new());
        let balloon_events = cx.subscribe_in(&balloons, window, |workbench, _, event: &BalloonEvent, window, cx| {
            match event.clone() {
                BalloonEvent::Dismissed(id) => workbench.notifications.dismiss_balloon(id),
                BalloonEvent::ActionChosen { id, action } => {
                    workbench.notifications.dismiss_balloon(id);
                    workbench.run_notification_action(action, window, cx);
                }
                BalloonEvent::HoverChanged(hovered) => workbench.balloon_view.hovered = hovered,
            }
            workbench.sync_notifications(cx);
        });
        let mut workbench = Self {
            label: label.to_owned(),
            content: Content::Empty,
            editor_observer: None,
            shown_title: None,
            tasks: Tasks::default(),
            notifications: Notifications::default(),
            task_timer: None,
            listener: listener.unwrap_or_default(),
            previous_editor: None,
            closing: false,
            session_save: None,
            popup: None,
            activations: HashMap::new(),
            active_key: None,
            bookmarks: BookmarkStore::default(),
            balloons,
            balloon_clocks: BalloonClocks::default(),
            balloon_view: BalloonView::default(),
            balloon_timer: None,
            _balloon_events: balloon_events,
            welcome: None,
        };
        workbench.sync_welcome(window, cx);
        workbench
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    /// How much of the IDE runs for this window: Blaze (editor, git and search only) or Deep
    /// (language servers and the tooling feeding them too).
    pub fn ide_mode(&self, cx: &App) -> ide_core::settings::IdeMode {
        crate::platform::settings::effective(&self.label, cx).ide.mode
    }

    /// Switches between Blaze and Deep mode, for every project; a project whose own settings
    /// name the mode is switched too, so the toggle always takes effect in this window.
    pub fn toggle_ide_mode(&mut self, cx: &mut Context<Self>) {
        use crate::platform::settings::set;
        use ide_core::settings::IdeMode;
        use ide_workspace::settings::SettingsScope;
        let next = if self.ide_mode(cx).is_full() { IdeMode::Light } else { IdeMode::Full };
        let value = serde_json::Value::from(next.as_str());
        let label = self.label.clone();
        let in_project = matches!(self.content, Content::Project { .. });
        let switched = set(&label, SettingsScope::Global, "ide.mode", &value, cx).and_then(|settings| {
            if in_project && settings.ide.mode != next {
                set(&label, SettingsScope::Project, "ide.mode", &value, cx).map(|_| ())
            } else {
                Ok(())
            }
        });
        if let Err(error) = switched {
            let mut request = NotificationRequest::new(NotificationLevel::Error, "Cannot change the IDE mode");
            request.body = Some(error.message);
            self.notify(request, cx);
        }
        cx.notify();
    }

    /// The folder of the project open in this window.
    pub fn project_root(&self, cx: &App) -> Option<PathBuf> {
        match &self.content {
            Content::Project { tree, .. } => Some(PathBuf::from(tree.read(cx).root())),
            _ => None,
        }
    }

    /// Shows `request` in this window, subject to the notification settings.
    pub fn notify(&mut self, request: NotificationRequest, cx: &mut Context<Self>) {
        let settings = self.notification_settings(cx);
        let _ = self.notifications.notify(request, settings.as_ref(), now_ms());
        self.sync_notifications(cx);
    }

    /// Settings › Notifications as they are in effect for the window.
    fn notification_settings(&self, cx: &App) -> Option<NotificationSettings> {
        let settings = serde_json::to_value(crate::platform::settings::effective(&self.label, cx)).ok()?;
        notification_settings(&settings)
    }

    /// Brings the balloons and the Notifications tool window in line with the notifications:
    /// after one was raised, dismissed or faded, and when the tool window shows or hides.
    fn sync_notifications(&mut self, cx: &mut Context<Self>) {
        let settings = self.notification_settings(cx);
        let list_open = self.is_tool_window_visible(NOTIFICATIONS_TOOL_WINDOW);
        // What is listed in front of the user has been seen.
        if list_open && self.notifications.has_unseen() {
            self.notifications.mark_all_read();
        }
        self.balloon_view.list_open = list_open;
        self.balloon_clocks.sync(&self.notifications, self.balloon_view, now_ms());
        let shown = balloon_entries(&self.notifications, settings.as_ref(), list_open);
        self.balloons.update(cx, |balloons, cx| balloons.set(shown, cx));
        let list =
            self.tool_window_view(NOTIFICATIONS_TOOL_WINDOW).and_then(|view| view.downcast::<NotificationsView>().ok());
        if let Some(list) = list {
            let logged = logged_entries(&self.notifications, settings.as_ref());
            list.update(cx, |list, cx| list.set(logged, cx));
        }
        self.balloon_timer = self.balloon_clocks.next_deadline().map(|due| {
            let wait = std::time::Duration::from_secs_f64(((due - now_ms()) / 1000.0).max(0.0));
            cx.spawn(async move |workbench, cx| {
                cx.background_executor().timer(wait).await;
                let _ = workbench.update(cx, |workbench, cx| {
                    let view = workbench.balloon_view;
                    workbench.balloon_clocks.fade_out(&mut workbench.notifications, view, now_ms());
                    workbench.sync_notifications(cx);
                });
            })
        });
        cx.notify();
    }

    /// Does what a notification's link says: "Don't Show Again" turns its group's balloons off,
    /// a link of the notification's own runs its command when that is a registered action.
    fn run_notification_action(&mut self, action: EntryAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            EntryAction::DontShowAgain { group } => {
                let write = suppress_group(self.notification_settings(cx).as_ref(), &group);
                if let Err(error) = crate::platform::settings::set_global(&self.label, &[write], cx) {
                    tracing::warn!("cannot store the notification setting: {}", error.message);
                }
                self.notifications.hide_group_balloons(&group);
            }
            EntryAction::Own(own)
                if own.command.starts_with("run.showSession:") || own.command.starts_with("run.rerunSession:") =>
            {
                let Some(driver) = self.run_driver().cloned() else { return };
                let (command, session) = own.command.split_once(':').unwrap_or_default();
                let session = session.to_owned();
                if command == "run.rerunSession" {
                    driver.update(cx, |driver, cx| {
                        if let Some(rerun) = driver.rerun(&session, cx) {
                            rerun.detach();
                        }
                    });
                } else {
                    driver.update(cx, |driver, cx| {
                        driver.store().update(cx, |store, cx| {
                            store.active_session = Some(session.clone());
                            cx.notify();
                        });
                    });
                    self.with_layout(window, cx, |layout| layout.show(RUN_TOOL_WINDOW, true));
                }
            }
            EntryAction::Own(own) => {
                let registered =
                    cx.try_global::<ActionRegistry>().is_some_and(|registry| registry.contains(&own.command));
                if registered {
                    window.defer(cx, move |window, cx| {
                        execute_action_from(ActionSource::Other, &own.command, None, window, cx);
                    });
                } else {
                    // Commands with arguments of their own (a run configuration, a repository) have
                    // no runner yet.
                    tracing::debug!("the notification command {} has no handler", own.command);
                }
            }
        }
    }

    pub fn editor(&self) -> Option<&Entity<Editor>> {
        match &self.content {
            Content::Editor(editor) => Some(editor),
            Content::Project { tabs, editors, .. } => {
                tabs.pane(&tabs.active_pane).and_then(|pane| pane.active.as_ref()).and_then(|id| editors.get(id))
            }
            _ => None,
        }
    }

    /// The editor tabs, when a project is open.
    pub fn tabs(&self) -> Option<&TabsState> {
        match &self.content {
            Content::Project { tabs, .. } => Some(tabs.as_ref()),
            _ => None,
        }
    }

    /// The titles of the active pane's tabs, in strip order. Files of the same name carry the
    /// folder that tells them apart as a hint.
    pub fn tab_titles(&self) -> Vec<TabTitle> {
        self.tabs().map(|tabs| self.tab_titles_of(&tabs.active_pane)).unwrap_or_default()
    }

    /// The titles of the tabs of `pane`, in strip order.
    pub fn tab_titles_of(&self, pane: &str) -> Vec<TabTitle> {
        let Some(pane) = self.tabs().and_then(|tabs| tabs.pane(pane)) else { return Vec::new() };
        let paths: Vec<&str> = pane.tabs.iter().filter_map(|tab| tab.input.path.as_deref()).collect();
        let hints = disambiguate_titles(&paths);
        pane.tabs
            .iter()
            .map(|tab| {
                let path = tab.input.path.as_deref().unwrap_or(&tab.input.key);
                TabTitle {
                    name: path.rsplit('/').next().unwrap_or(path).to_owned(),
                    hint: hints.get(path).cloned(),
                    active: pane.active.as_deref() == Some(tab.id.as_str()),
                }
            })
            .collect()
    }

    /// One pane of the editor area: its tabs over the editor of the selected one.
    fn pane_element(&self, pane_id: &str, window: &Window, cx: &mut Context<Self>) -> gpui_kit::Div {
        let (editor, find_bar) = match &self.content {
            Content::Project { tabs, editors, find_bars, .. } => {
                let active = tabs.pane(pane_id).and_then(|pane| pane.active.as_ref());
                (
                    active.and_then(|id| editors.get(id)).cloned(),
                    active.and_then(|id| find_bars.get(id)).map(|bar| bar.bar.clone()),
                )
            }
            _ => (None, None),
        };
        let editor_focused = editor.as_ref().is_some_and(|editor| editor.focus_handle(cx).is_focused(window));
        let titles = self.tab_titles_of(pane_id);
        let ids: Vec<String> = self
            .tabs()
            .and_then(|tabs| tabs.pane(pane_id))
            .map(|pane| pane.tabs.iter().map(|tab| tab.id.clone()).collect())
            .unwrap_or_default();
        // The tab strip: the selected tab is a rounded pill; the rest are plain titles.
        let strip =
            (!titles.is_empty()).then(|| {
                div()
                    .h(theme::length(cx, "h-header"))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(gpui_kit::px(2.0))
                    .px(gpui_kit::px(4.0))
                    .border_b_1()
                    .border_color(theme::color(cx, "border-editor-tabs"))
                    .children(titles.into_iter().zip(ids).map(|(TabTitle { name, hint, active }, id)| {
                        let (select, close) = (id.clone(), id.clone());
                        let (select_pane, close_pane) = (pane_id.to_owned(), pane_id.to_owned());
                        let (menu_pane, menu_tab) = (pane_id.to_owned(), id.clone());
                        let selected = if editor_focused { "bg-tab-selected" } else { "bg-tab-selected-inactive" };
                        div()
                            .id(SharedString::from(format!("tab:{id}")))
                            .debug_selector(|| format!("tab:{id}"))
                            .h(theme::length(cx, "h-tab"))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(gpui_kit::px(6.0))
                            .pl(gpui_kit::px(8.0))
                            .pr(gpui_kit::px(4.0))
                            .rounded(gpui_kit::px(6.0))
                            .text_color(theme::color(cx, "fg-strong"))
                            .when(active, |el| el.bg(theme::color(cx, selected)))
                            .when(!active, |el| el.hover(|el| el.bg(theme::color(cx, "bg-tab-hover"))))
                            .child(theme::icon(
                                cx,
                                theme::icons::file_icon_name(&name),
                                16.0,
                                theme::color(cx, "fg-strong"),
                            ))
                            .child(SharedString::from(name))
                            .children(hint.map(|hint| {
                                div().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(hint))
                            }))
                            .child(
                                div()
                                    .id(SharedString::from(format!("close:{id}")))
                                    .size(gpui_kit::px(16.0))
                                    .flex_none()
                                    .rounded(gpui_kit::px(4.0))
                                    .hover(|el| el.bg(theme::color(cx, "bg-tab-hover")))
                                    .child(theme::icon(cx, "close", 16.0, theme::color(cx, "fg-muted")))
                                    .on_click(cx.listener(move |workbench, _, window, cx| {
                                        workbench.with_tabs(window, cx, |tabs, _| tabs.close_tab(&close_pane, &close));
                                        cx.stop_propagation();
                                    })),
                            )
                            .on_click(cx.listener(move |workbench, _, window, cx| {
                                workbench.with_tabs(window, cx, |tabs, _| {
                                    tabs.set_active_pane(&select_pane);
                                    tabs.activate_tab(&select_pane, &select);
                                    true
                                });
                            }))
                            .on_mouse_down(
                                gpui_kit::MouseButton::Right,
                                cx.listener(move |workbench, event: &MouseDownEvent, window, cx| {
                                    let items = workbench.tab_menu(&menu_pane, &menu_tab, cx);
                                    let (pane, tab) = (menu_pane.clone(), menu_tab.clone());
                                    workbench.show_menu(
                                        &items,
                                        event.position,
                                        move |workbench, command, window, cx| {
                                            workbench.run_tab_command(&pane, &tab, command, window, cx);
                                        },
                                        window,
                                        cx,
                                    );
                                }),
                            )
                    }))
            });
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .children(strip)
            .children(find_bar.map(|bar| div().flex_none().child(bar)))
            .child(div().flex_1().min_h_0().children(editor))
    }

    /// The editor area: the panes as the splits arrange them, side by side or stacked, sharing
    /// the room evenly with a line between them.
    fn editor_area(&self, node: &LayoutNode, window: &Window, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        match node {
            LayoutNode::Pane { pane_id } => self.pane_element(pane_id, window, cx).into_any_element(),
            LayoutNode::Split { dir, children, .. } => {
                let stacked = *dir == SplitDir::Column;
                let line = theme::color(cx, "border-editor-tabs");
                let parts: Vec<gpui_kit::AnyElement> = children
                    .iter()
                    .enumerate()
                    .map(|(index, child)| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .when(index > 0 && stacked, |el| el.border_t_1().border_color(line))
                            .when(index > 0 && !stacked, |el| el.border_l_1().border_color(line))
                            .child(self.editor_area(child, window, cx))
                            .into_any_element()
                    })
                    .collect();
                div().size_full().flex().when(stacked, |el| el.flex_col()).children(parts).into_any_element()
            }
        }
    }

    /// Changes the editor tabs with `change` (given the model and the active pane's id), then
    /// brings the window in line: editors of closed tabs are saved and released, and the focus
    /// goes to the tab that is now active.
    pub fn with_tabs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut TabsState, &str) -> bool,
    ) {
        let Content::Project { tabs, .. } = &mut self.content else { return };
        let pane = tabs.active_pane.clone();
        if change(tabs, &pane) {
            self.tabs_changed(true, window, cx);
        }
    }

    fn tabs_changed(&mut self, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        let auto_save = self.auto_save(cx);
        let Content::Project { tabs, editors, editor_focus, find_bars, .. } = &mut self.content else { return };
        let open: std::collections::HashSet<&str> =
            tabs.panes.iter().flat_map(|pane| pane.tabs.iter().map(|tab| tab.id.as_str())).collect();
        find_bars.retain(|id, _| open.contains(id.as_str()));
        let closed: Vec<String> = editors.keys().filter(|id| !open.contains(id.as_str())).cloned().collect();
        for id in closed {
            editor_focus.remove(&id);
            let Some(editor) = editors.remove(&id) else { continue };
            // Closing never loses edits: an unsaved document is written before it is released,
            // and when it cannot be written its tab comes back with the text still in it. With
            // autosave off the user decides instead.
            if !editor.read(cx).is_dirty() {
                continue;
            }
            let decision = (!auto_save).then(|| {
                let path = editor.read(cx).path().to_string_lossy().into_owned();
                let i18n = &cx.default_global::<crate::platform::i18n::Localization>().0;
                let (title, body) = (
                    i18n.t("app.unsaved.title", &[]),
                    i18n.t("app.unsaved.body", &[("count", 1usize.into()), ("project", path.into())]),
                );
                let answers =
                    [i18n.t("app.unsaved.save", &[]), i18n.t("app.unsaved.discard", &[]), i18n.t("common.cancel", &[])];
                let answers: Vec<&str> = answers.iter().map(String::as_str).collect();
                window.prompt(gpui_kit::PromptLevel::Warning, &title, Some(&body), &answers, cx)
            });
            cx.spawn_in(window, async move |workbench, cx| {
                let answer = match decision {
                    Some(decision) => decision.await.ok(),
                    None => Some(0),
                };
                let failure = match answer {
                    Some(0) => {
                        let saving = editor.read_with(cx, |editor, cx| editor.save(cx));
                        match saving.await {
                            Ok(Ok(())) => return,
                            Ok(Err(error)) => Some(error.message),
                            Err(error) => Some(error.to_string()),
                        }
                    }
                    // Don't Save.
                    Some(1) => return,
                    // Cancel, or the question went away unanswered: the tab stays.
                    _ => None,
                };
                let _ = workbench
                    .update_in(cx, |workbench, window, cx| workbench.restore_unsaved(editor, failure, window, cx));
            })
            .detach();
        }
        self.load_active_tab(focus, window, cx);
        self.note_active_tab(cx);
        self.session_save = Some(cx.spawn(async move |workbench, cx| {
            cx.background_executor().timer(SESSION_SAVE_DELAY).await;
            let _ = workbench.update(cx, |workbench, cx| workbench.save_session(cx));
        }));
        let active = self.editor().cloned();
        // The editor that was active is saved when another one takes its place.
        let previous = self.previous_editor.take().and_then(|previous| previous.upgrade());
        if let Some(previous) = previous.filter(|previous| active.as_ref() != Some(previous)) {
            self.autosave(Some(&previous), cx);
        }
        self.previous_editor = active.as_ref().map(Entity::downgrade);
        match active {
            Some(editor) => {
                self.observe_editor(&editor, cx);
                if focus {
                    let handle = editor.focus_handle(cx);
                    window.focus(&handle, cx);
                }
            }
            None => self.editor_observer = None,
        }
        cx.notify();
    }

    /// A closed tab whose document was not saved, because saving failed or the user chose to
    /// stay: it reopens with its editor, and a failure is reported.
    fn restore_unsaved(
        &mut self,
        editor: Entity<Editor>,
        failure: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = editor.read(cx).path().to_string_lossy().into_owned();
        if let Some(failure) = failure {
            self.report_save_failure(failure, cx);
        }
        let Content::Project { tabs, editors, find_bars, .. } = &mut self.content else { return };
        let pane = tabs.active_pane.clone();
        let input = EditorInput { kind: "file".to_owned(), key: path.clone(), path: Some(path), data: None };
        if let Some(id) = tabs.open_tab(&pane, input, OpenTabOptions::default()) {
            // A tab for the same file opened meanwhile has its own editor: the unsaved one wins,
            // since it is the one holding the text that is not on disk. A bar over the editor it
            // replaces goes with that editor.
            find_bars.remove(&id);
            editors.insert(id, editor);
        }
        self.tabs_changed(true, window, cx);
    }

    fn report_save_failure(&mut self, failure: String, cx: &mut Context<Self>) {
        let title = cx.default_global::<crate::platform::i18n::Localization>().0.t("app.unsaved.saveFailed", &[]);
        let mut request = NotificationRequest::new(NotificationLevel::Error, title);
        request.body = Some(failure);
        self.notify(request, cx);
    }

    /// Every open editor, of every tab.
    fn editors(&self) -> Vec<Entity<Editor>> {
        match &self.content {
            Content::Editor(editor) => vec![editor.clone()],
            Content::Project { editors, .. } => editors.values().cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// How many open documents have changes that are not on disk.
    pub fn unsaved_count(&self, cx: &App) -> usize {
        self.editors().iter().filter(|editor| editor.read(cx).is_dirty()).count()
    }

    /// Files › Autosave (`files.autoSave`): save when a tab is left or closed and when the
    /// window loses focus.
    fn auto_save(&self, cx: &App) -> bool {
        crate::platform::settings::effective(&self.label, cx).files.auto_save
    }

    /// With autosave on, saves `editor` if it is unsaved, or every unsaved editor when none is
    /// named. A failure is reported; the text stays in its editor.
    fn autosave(&mut self, editor: Option<&Entity<Editor>>, cx: &mut Context<Self>) {
        if !self.auto_save(cx) {
            return;
        }
        let editors = match editor {
            Some(editor) => vec![editor.clone()],
            None => self.editors(),
        };
        for editor in editors.into_iter().filter(|editor| editor.read(cx).is_dirty()) {
            let saving = editor.read(cx).save(cx);
            cx.spawn(async move |workbench, cx| {
                let failure = match saving.await {
                    Ok(Ok(())) => return,
                    Ok(Err(error)) => error.message,
                    Err(error) => error.to_string(),
                };
                let _ = workbench.update(cx, |workbench, cx| workbench.report_save_failure(failure, cx));
            })
            .detach();
        }
    }

    /// Saves every unsaved editor; the first failure's message, if any.
    fn save_all(&self, cx: &App) -> impl Future<Output = Result<(), String>> + 'static + use<> {
        let saving: Vec<_> = self
            .editors()
            .iter()
            .filter(|editor| editor.read(cx).is_dirty())
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

    /// Whether the window may close now. With unsaved changes it may not: the user is asked to
    /// save them, discard them or stay, and the window closes once that is settled.
    pub fn confirm_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let unsaved = self.unsaved_count(cx);
        if unsaved == 0 {
            return true;
        }
        if self.closing {
            return false;
        }
        self.closing = true;
        let project = match &self.content {
            Content::Project { name, .. } => Some(name.clone()),
            _ => None,
        };
        let i18n = &cx.default_global::<crate::platform::i18n::Localization>().0;
        let project = project.unwrap_or_else(|| i18n.t("app.title.default", &[]));
        let (title, body) = (
            i18n.t("app.unsaved.title", &[]),
            i18n.t("app.unsaved.body", &[("count", unsaved.into()), ("project", project.into())]),
        );
        let answers =
            [i18n.t("app.unsaved.save", &[]), i18n.t("app.unsaved.discard", &[]), i18n.t("common.cancel", &[])];
        let answers: Vec<&str> = answers.iter().map(String::as_str).collect();
        let answer = window.prompt(gpui_kit::PromptLevel::Warning, &title, Some(&body), &answers, cx);
        cx.spawn_in(window, async move |workbench, cx| {
            let answer = answer.await.ok();
            let saving = workbench.update(cx, |workbench, cx| {
                workbench.closing = false;
                (answer == Some(0)).then(|| workbench.save_all(cx))
            });
            let close = match (answer, saving) {
                (Some(0), Ok(Some(saving))) => match saving.await {
                    Ok(()) => true,
                    Err(failure) => {
                        let _ = workbench.update(cx, |workbench, cx| workbench.report_save_failure(failure, cx));
                        false
                    }
                },
                // Don't Save.
                (Some(1), _) => true,
                _ => false,
            };
            if close {
                let _ = cx.update(|window, _| window.remove_window());
            }
        })
        .detach();
        false
    }

    /// Background tasks of this window, in listing order.
    pub fn tasks(&self) -> &Tasks {
        &self.tasks
    }

    pub fn notifications(&self) -> &Notifications {
        &self.notifications
    }

    /// What the status bar says about background work: the first task's title with its message,
    /// and how many more are running.
    pub fn task_status(&self) -> Option<String> {
        let tasks = self.tasks.tasks();
        let first = tasks.first()?;
        let mut text = match &first.message {
            Some(message) if !message.is_empty() => format!("{}: {message}", first.title),
            _ => first.title.clone(),
        };
        if tasks.len() > 1 {
            text.push_str(&format!(" (+{})", tasks.len() - 1));
        }
        Some(text)
    }

    /// A backend event: its task, indexing and notification reports feed this window's models.
    pub fn backend_event(&mut self, event: &BackendEvent, cx: &mut Context<Self>) {
        if event.window.as_deref().is_some_and(|window| window != self.label) {
            return;
        }
        let now = now_ms();
        match &event.event {
            IdeEvent::Task { id, title, message, fraction, done, cancellable } => self.tasks.on_task_event(
                TaskEvent {
                    id: u64::from(id.0),
                    title: title.clone(),
                    message: message.clone(),
                    fraction: fraction.map(f64::from),
                    done: *done,
                    cancellable: *cancellable,
                },
                now,
            ),
            IdeEvent::IndexState { indexing, files, symbols } => {
                self.tasks.set_indexing(*indexing, u64::from(*files), u64::from(*symbols), now);
            }
            IdeEvent::RunExited { .. } => {
                if let Some(driver) = self.run_driver().cloned() {
                    driver.update(cx, |driver, cx| driver.backend_event(&event.event, cx));
                }
                return;
            }
            // Settings › Console reaches the consoles and the Run tool window.
            IdeEvent::SettingsChanged { .. } => {
                if let Some(driver) = self.run_driver().cloned() {
                    driver.update(cx, |driver, cx| driver.apply_console_settings(cx));
                    self.sync_run_window(cx);
                }
                return;
            }
            IdeEvent::Notify { level, title, body } => {
                let level = match level {
                    NotifyLevel::Info => NotificationLevel::Info,
                    NotifyLevel::Warning => NotificationLevel::Warning,
                    NotifyLevel::Error => NotificationLevel::Error,
                };
                let mut request = NotificationRequest::new(level, title.clone());
                request.body = body.clone();
                let settings = self.notification_settings(cx);
                let _ = self.notifications.notify(request, settings.as_ref(), now);
                self.sync_notifications(cx);
            }
            _ => return,
        }
        self.schedule_tasks(cx);
        cx.notify();
    }

    /// Advances the task list to `now` and arranges to be woken at its next deadline.
    pub fn tick_tasks(&mut self, now: f64, cx: &mut Context<Self>) {
        self.tasks.expire(now);
        self.schedule_tasks(cx);
        cx.notify();
    }

    fn schedule_tasks(&mut self, cx: &mut Context<Self>) {
        self.task_timer = self.tasks.next_deadline().map(|due| {
            let wait = std::time::Duration::from_secs_f64(((due - now_ms()) / 1000.0).max(0.0));
            cx.spawn(async move |workbench, cx| {
                cx.background_executor().timer(wait).await;
                let _ = workbench.update(cx, |workbench, cx| workbench.tick_tasks(now_ms(), cx));
            })
        });
    }

    /// The status bar's right-hand items for the shown editor: caret position, line separator
    /// and encoding, in that order.
    pub fn status_items(&self, cx: &App) -> Vec<String> {
        let Some(status) = self.editor().map(|editor| editor.read(cx).status()) else { return Vec::new() };
        let encoding = status.encoding.to_lowercase();
        let shown = ENCODINGS.iter().find(|(id, _)| *id == encoding).map_or(status.encoding.as_str(), |(_, name)| name);
        vec![status.caret_text(), status.line_ending_text().to_owned(), shown.to_owned()]
    }

    /// The Line Separator item's menu: the three separators, the file's own marked.
    pub fn line_ending_menu(&self, cx: &App) -> Vec<MenuItem<StatusCommand>> {
        use ide_text::LineEnding;
        let Some(status) = self.editor().map(|editor| editor.read(cx).status()) else { return Vec::new() };
        [
            (LineEnding::Lf, "LF – Unix and macOS (\\n)"),
            (LineEnding::CrLf, "CRLF – Windows (\\r\\n)"),
            (LineEnding::Cr, "CR – Classic Mac OS (\\r)"),
        ]
        .into_iter()
        .map(|(line_ending, label)| {
            let row = MenuItem::item(label, StatusCommand::LineEnding(line_ending));
            if line_ending == status.line_ending { row.icon("check") } else { row }
        })
        .collect()
    }

    /// The File Encoding item's menu: reload the file in another encoding, or convert it to one.
    pub fn encoding_menu(&self, cx: &App) -> Vec<MenuItem<StatusCommand>> {
        use crate::vcs_ops::menu::MenuEntry;
        let Some(editor) = self.editor().map(|editor| editor.read(cx)) else { return Vec::new() };
        let (status, dirty) = (editor.status(), editor.is_dirty());
        let current = status.encoding.to_lowercase();
        let others = |reload: bool| -> Vec<MenuItem<StatusCommand>> {
            ENCODINGS
                .iter()
                .filter(|(id, _)| *id != current)
                .map(|(id, name)| {
                    let command = if reload {
                        StatusCommand::Reload((*id).to_owned())
                    } else {
                        StatusCommand::Convert((*id).to_owned())
                    };
                    MenuItem::item(*name, command)
                })
                .collect()
        };
        let group = |label: &str, disabled: bool, submenu: Vec<MenuItem<StatusCommand>>| {
            MenuItem::Item(MenuEntry {
                label: label.to_owned(),
                icon: None,
                shortcut: None,
                disabled,
                danger: false,
                run: None,
                submenu,
            })
        };
        vec![
            // Reloading would lose what is not saved; a read-only file cannot be written in another encoding.
            group("Reload with Encoding", dirty, others(true)),
            group("Convert to Encoding", status.read_only, others(false)),
            MenuItem::Separator,
            MenuItem::action_with("file.settings", "editor.fileEncodings".into()).labelled("File Encodings Settings…"),
        ]
    }

    /// Carries out a row of a status bar item's menu on the file being edited; a refusal is told
    /// in a notification.
    pub fn run_status_command(&mut self, command: StatusCommand, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let (title, done) = match &command {
            StatusCommand::LineEnding(line_ending) => (
                "Cannot change line separators",
                editor.update(cx, |editor, cx| editor.set_line_ending(*line_ending, cx)),
            ),
            StatusCommand::Reload(encoding) => {
                ("Cannot reload with encoding", editor.update(cx, |editor, cx| editor.set_encoding(encoding, true, cx)))
            }
            StatusCommand::Convert(encoding) => {
                ("Cannot convert encoding", editor.update(cx, |editor, cx| editor.set_encoding(encoding, false, cx)))
            }
        };
        if let Err(error) = done {
            let request = NotificationRequest {
                body: Some(error.message),
                ..NotificationRequest::new(NotificationLevel::Error, title)
            };
            self.notify(request, cx);
        }
        cx.notify();
    }

    /// The window's title for what it shows now.
    pub fn title(&self, cx: &App) -> String {
        let project = match &self.content {
            Content::Project { name, .. } => Some(name.as_str()),
            _ => None,
        };
        let file = self.editor().map(|editor| editor.read(cx).path().to_path_buf());
        let default = cx
            .try_global::<crate::platform::i18n::Localization>()
            .map_or_else(|| "app.title.default".to_owned(), |l| l.0.t("app.title.default", &[]));
        window_title(project, file.as_deref(), false, None, &default)
    }

    /// Publishes the facts `when` clauses read about this window: what is open and where the
    /// focus is. Unchanged values cost nothing.
    fn publish_context(&mut self, window: &mut Window, cx: &mut App) {
        use crate::platform::context::{clear_context, set_context};
        let editor = self.editor().cloned();
        let focused = editor.as_ref().is_some_and(|editor| editor.focus_handle(cx).is_focused(window));
        set_context(window, cx, "projectOpening", matches!(self.content, Content::Opening(_)));
        set_context(window, cx, "popupOpen", self.popup.is_some());
        set_context(window, cx, "findPopupOpen", self.find_popup().is_some());
        let focused_bar = self.focused_find_bar(window, cx);
        set_context(window, cx, "findBarOpen", !self.shown_find_bars().is_empty());
        set_context(window, cx, "findBarFocus", focused_bar.is_some());
        set_context(window, cx, "findBarHasResults", focused_bar.is_some_and(|bar| bar.read(cx).has_results()));
        set_context(window, cx, "projectOpen", self.project_tree().is_some());
        set_context(window, cx, "lightEdit", matches!(self.content, Content::Editor(_)));
        set_context(window, cx, "textEditorActive", editor.is_some());
        set_context(window, cx, "editorSplit", self.tabs().is_some_and(|tabs| tabs.panes.len() > 1));
        let tabs_open =
            self.tabs().and_then(|tabs| tabs.pane(&tabs.active_pane)).is_some_and(|pane| !pane.tabs.is_empty());
        set_context(window, cx, "editorTabsOpen", tabs_open);
        for key in ["editorFocus", "editorTextFocus", "editorAreaFocus"] {
            set_context(window, cx, key, focused);
        }
        let focused_panel = match &self.content {
            Content::Project { panels, .. } => panels
                .iter()
                .find(|(_, panel)| (panel.focus)(cx).contains_focused(window, cx))
                .map(|(id, _)| id.clone()),
            _ => None,
        };
        set_context(window, cx, "toolWindowFocus", focused_panel.is_some());
        set_context(window, cx, "terminalFocus", focused_panel.as_deref() == Some(TERMINAL_TOOL_WINDOW));
        if let Content::Project { layout, .. } = &mut self.content {
            // The layout decides by this what `toolwindow.<id>` does: focus, or hide.
            layout.set_focused(focused_panel.as_deref());
            for (key, visible) in layout.visibility_context() {
                set_context(window, cx, &key, visible);
            }
            match layout.focused().map(str::to_owned) {
                Some(id) => set_context(window, cx, "activeToolWindow", id),
                None => clear_context(window, cx, "activeToolWindow"),
            }
        }
        match editor.map(|editor| {
            let editor = editor.read(cx);
            (editor.path().to_string_lossy().into_owned(), editor.language().to_string(), editor.status())
        }) {
            Some((path, language, status)) => {
                set_context(window, cx, "editorPath", path);
                set_context(window, cx, "editorLangId", language);
                set_context(window, cx, "editorReadOnly", status.read_only);
                set_context(window, cx, "editorHasSelection", status.selected_chars > 0);
            }
            None => {
                clear_context(window, cx, "editorPath");
                clear_context(window, cx, "editorLangId");
                set_context(window, cx, "editorReadOnly", false);
                set_context(window, cx, "editorHasSelection", false);
            }
        }
    }

    fn observe_editor(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        self.editor_observer = Some(cx.observe(editor, |_, _, cx| cx.notify()));
    }

    /// The tool window layout, when a project is open.
    pub fn tool_windows(&self) -> Option<&ToolWindowLayout> {
        match &self.content {
            Content::Project { layout, .. } => Some(layout.as_ref()),
            _ => None,
        }
    }

    /// Changes the tool window layout with `change` and carries out what it asks for: storing
    /// the layout, moving the focus to the editor, or to the tool window that requested it.
    pub fn with_layout(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut ToolWindowLayout) -> Effects,
    ) {
        let editor = self.editor().cloned();
        let Content::Project { layout, .. } = &mut self.content else { return };
        let effects = change(layout);
        if effects.persist {
            match serde_json::to_string(&layout.serialized()) {
                Ok(json) => cx.default_global::<UiStorage>().set(&storage_key(layout.scope()), json),
                Err(error) => tracing::warn!("cannot store the tool window layout: {error}"),
            }
        }
        // A window shown for the first time gets its view before the focus can go to it.
        self.mount_panels(window, cx);
        let Content::Project { layout, focus_served, panels, .. } = &mut self.content else { return };
        let request = layout.focus_request().filter(|request| request.seq > *focus_served).cloned();
        if let Some(request) = request {
            *focus_served = request.seq;
            if let Some(panel) = panels.get(&request.id) {
                let focus = (panel.focus)(cx);
                window.focus(&focus, cx);
            }
        } else if effects.focus_editor {
            match editor {
                Some(editor) => {
                    let focus = editor.focus_handle(cx);
                    window.focus(&focus, cx);
                }
                // Nothing to edit: the focus leaves the tool window all the same.
                None => window.blur(cx),
            }
        }
        // The Notifications window may have opened or closed, or just got its view.
        self.sync_notifications(cx);
        self.sync_run_window(cx);
    }

    /// Creates the view of every visible tool window that has none yet.
    fn mount_panels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Content::Project { layout, panels, tree, .. } = &self.content else { return };
        let missing: Vec<String> =
            layout.visible().ids().filter(|id| !panels.contains_key(*id)).map(str::to_owned).collect();
        let root = PathBuf::from(tree.read(cx).root());
        for id in missing {
            let panel = match id.as_str() {
                TERMINAL_TOOL_WINDOW => {
                    use crate::terminal::panel::{TerminalPanel, TerminalPanelEvent};
                    let (label, root) = (self.label.clone(), root.clone());
                    let terminals = cx.new(|cx| TerminalPanel::new(&label, root, cx));
                    // The last tab closing leaves nothing to show.
                    let closes =
                        cx.subscribe_in(&terminals, window, |workbench, _, event: &TerminalPanelEvent, window, cx| {
                            let TerminalPanelEvent::Emptied = event;
                            workbench.with_layout(window, cx, |layout| layout.hide(TERMINAL_TOOL_WINDOW));
                        });
                    Panel::new(terminals, Some(closes))
                }
                GIT_TOOL_WINDOW => {
                    let label = self.label.clone();
                    let log = cx.new(|cx| crate::vcs::log_view::GitLog::new(&label, cx));
                    Panel::new(log, None)
                }
                NOTIFICATIONS_TOOL_WINDOW => {
                    let list = cx.new(NotificationsView::new);
                    let events =
                        cx.subscribe_in(&list, window, |workbench, _, event: &NotificationsEvent, window, cx| {
                            match event.clone() {
                                NotificationsEvent::Removed(id) => workbench.notifications.remove_notification(id),
                                // An entry of the list stays after its link was used.
                                NotificationsEvent::ActionChosen { action, .. } => {
                                    workbench.run_notification_action(action, window, cx);
                                }
                                NotificationsEvent::ClearAll => workbench.notifications.clear(),
                            }
                            workbench.sync_notifications(cx);
                        });
                    Panel::new(list, Some(events))
                }
                COMMIT_TOOL_WINDOW => {
                    use crate::vcs::commit_view::{CommitPanel, CommitPanelEvent};
                    let (label, root) = (self.label.clone(), root.clone());
                    let changes = cx.new(|cx| CommitPanel::new(&label, root, cx));
                    let opens =
                        cx.subscribe_in(&changes, window, |workbench, _, event: &CommitPanelEvent, window, cx| {
                            let CommitPanelEvent::Open(path) = event;
                            workbench.open_project_file(path.clone(), window, cx);
                        });
                    Panel::new(changes, Some(opens))
                }
                RUN_TOOL_WINDOW => {
                    let Content::Project { run, .. } = &self.content else { continue };
                    let (store, types) = (run.read(cx).store().clone(), run.read(cx).types());
                    let view = cx.new(|cx| RunView::new(store, types, cx));
                    let events = cx.subscribe_in(&view, window, Self::run_view_event);
                    Panel::new(view, Some(events))
                }
                FIND_TOOL_WINDOW => {
                    use crate::find::view::{FindPanel, FindPanelEvent};
                    let label = self.label.clone();
                    let find = cx.new(|cx| FindPanel::new(&label, cx));
                    let opens = cx.subscribe_in(&find, window, |workbench, _, event: &FindPanelEvent, window, cx| {
                        let FindPanelEvent::Open { path, line, col } = event;
                        workbench.open_project_file_at(path.clone(), *line, *col, window, cx);
                    });
                    Panel::new(find, Some(opens))
                }
                _ => continue,
            };
            if let Content::Project { panels, .. } = &mut self.content {
                panels.insert(id, panel);
            }
        }
        // The Terminal shown again after its last tab closed starts a new one.
        if self.is_tool_window_visible(TERMINAL_TOOL_WINDOW)
            && let Some(terminals) = self
                .tool_window_view(TERMINAL_TOOL_WINDOW)
                .and_then(|view| view.downcast::<crate::terminal::panel::TerminalPanel>().ok())
            && terminals.read(cx).terminals().tabs().is_empty()
        {
            terminals.update(cx, |terminals, cx| terminals.open_tab(cx));
        }
    }

    /// The view of tool window `id`, once it has been shown.
    pub fn tool_window_view(&self, id: &str) -> Option<AnyView> {
        match &self.content {
            Content::Project { panels, .. } => panels.get(id).map(|panel| panel.view.clone()),
            _ => None,
        }
    }

    /// Whether tool window `id` is showing.
    pub fn is_tool_window_visible(&self, id: &str) -> bool {
        matches!(&self.content, Content::Project { layout, .. } if layout.is_visible(id))
    }

    /// The run driver of the window's project, which starts and stops its processes.
    pub fn run_driver(&self) -> Option<&Entity<RunDriver>> {
        match &self.content {
            Content::Project { run, .. } => Some(run),
            _ => None,
        }
    }

    /// The run driver of a project just opened at `root`, listing its run configurations.
    fn new_run_driver(&mut self, root: &str, window: &mut Window, cx: &mut Context<Self>) -> Entity<RunDriver> {
        let store = cx.new(|_| RunStore::new());
        let weak = cx.weak_entity();
        let save_all: SaveAll = Rc::new(move |cx: &mut App| {
            use futures::FutureExt as _;
            match weak.upgrade() {
                Some(workbench) => workbench.read(cx).save_all(cx).boxed_local(),
                None => futures::future::ready(Ok(())).boxed_local(),
            }
        });
        let tool_windows = TOOL_WINDOWS
            .into_iter()
            .filter_map(tool_window_declaration)
            .map(|declared| (declared.id.clone(), declared.title.clone()))
            .collect();
        let (label, handle) = (self.label.clone(), window.window_handle());
        let driver = cx.new(|_| {
            RunDriver::new(&label, store).with_window(handle).with_save_all(save_all).with_tool_windows(tool_windows)
        });
        driver.update(cx, |driver, cx| driver.load(root, cx));
        driver
    }

    /// The project goes: its processes stop and its sessions are forgotten.
    fn reset_runs(&mut self, cx: &mut Context<Self>) {
        if let Some(driver) = self.run_driver().cloned() {
            driver.update(cx, |driver, cx| driver.reset(cx));
        }
    }

    /// Tells the run driver whether the Run tool window shows, and the Run view what it reads from the
    /// settings and the registered actions.
    fn sync_run_window(&mut self, cx: &mut Context<Self>) {
        let Some(driver) = self.run_driver().cloned() else { return };
        let visible = self.is_tool_window_visible(RUN_TOOL_WINDOW);
        driver.update(cx, |driver, _| driver.run_window_visible = visible);
        let Some(view) = self.tool_window_view(RUN_TOOL_WINDOW).and_then(|view| view.downcast::<RunView>().ok()) else {
            return;
        };
        let console = crate::platform::settings::effective(&self.label, cx).console;
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        let registered = |id: &str| cx.try_global::<ActionRegistry>().is_some_and(|registry| registry.contains(id));
        let (debug, edit) = (registered("run.debugConfig"), registered("run.editConfigs"));
        let chooser = registered("run.chooseConfig")
            .then(|| {
                let keymap = cx.try_global::<crate::platform::input::Keymap>()?;
                keymap.0.shortcut_label("run.chooseConfig", crate::platform::keymap::is_mac_host())
            })
            .flatten();
        view.update(cx, |view, cx| {
            view.set_console_settings(console, cx);
            view.set_project_root(root);
            view.set_capabilities(debug, edit, chooser, cx);
        });
    }

    /// Carries out what the run driver asks of the window.
    fn run_driver_event(
        &mut self,
        _: &Entity<RunDriver>,
        event: &RunDriverEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            RunDriverEvent::ShowRunWindow { focus } => {
                self.with_layout(window, cx, |layout| layout.show(RUN_TOOL_WINDOW, focus));
            }
            RunDriverEvent::Notify(request) => self.notify(request, cx),
            RunDriverEvent::ShowToolWindow(id) => self.with_layout(window, cx, |layout| layout.focus(&id)),
            RunDriverEvent::OpenFile { path, line, col } => self.open_project_file_at(path, line, col, window, cx),
            RunDriverEvent::RunAction { id, arg } => window.defer(cx, move |window, cx| {
                if !execute_action_from(ActionSource::Other, id, arg.as_ref(), window, cx) {
                    tracing::info!("{id} is not available in this window");
                }
            }),
        }
    }

    /// Carries out what the Run tool window asks for.
    fn run_view_event(
        &mut self,
        view: &Entity<RunView>,
        event: &RunViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(driver) = self.run_driver().cloned() else { return };
        match event.clone() {
            RunViewEvent::Rerun(session) => driver.update(cx, |driver, cx| {
                if let Some(rerun) = driver.rerun(&session, cx) {
                    rerun.detach();
                }
            }),
            RunViewEvent::Debug(config) => match serde_json::to_value(&config) {
                Ok(arg) => window.defer(cx, move |window, cx| {
                    execute_action_from(ActionSource::Toolbar, "run.debugConfig", Some(&arg), window, cx);
                }),
                Err(error) => tracing::error!("cannot pass the configuration to the debugger: {error}"),
            },
            RunViewEvent::Stop(session) => driver.update(cx, |driver, cx| driver.stop(&session, cx)),
            RunViewEvent::CloseTab(session) => driver.update(cx, |driver, cx| driver.close_tab(&session, cx)),
            RunViewEvent::EditConfigs(config) => window.defer(cx, move |window, cx| {
                let arg = config.map(serde_json::Value::from);
                execute_action_from(ActionSource::Toolbar, "run.editConfigs", arg.as_ref(), window, cx);
            }),
            RunViewEvent::SendInput { session, line } => {
                driver.update(cx, |driver, cx| driver.send_input(&session, &line, cx));
            }
            RunViewEvent::SendEof(session) => driver.update(cx, |driver, cx| driver.close_stdin(&session, cx)),
            RunViewEvent::OpenUrl(url) => driver.update(cx, |driver, cx| driver.open_url(&url, cx)),
            RunViewEvent::OpenLocation { target, cwd } => {
                driver.update(cx, |driver, cx| driver.open_location(target, cwd, cx));
            }
            RunViewEvent::ShowMenu(position) => {
                let items = view.read(cx).console_menu(cx);
                let view = view.clone();
                let run = move |_: &mut Self, command, window: &mut Window, cx: &mut Context<Self>| {
                    view.update(cx, |view, cx| view.run_console_command(command, window, cx));
                };
                self.show_menu(&items, position, run, window, cx);
            }
        }
    }

    /// Stop: the target [`stop_target`] picks; several running processes are offered in a menu with
    /// Stop All.
    pub fn stop_runs(&mut self, focused: bool, from_widget: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(driver) = self.run_driver().cloned() else { return };
        let target = stop_target(driver.read(cx).store().read(cx), focused, from_widget);
        let sessions = match target {
            StopTarget::Nothing => return,
            StopTarget::Sessions(sessions) => sessions,
            StopTarget::Choose(sessions) => {
                let store = driver.read(cx).store().read(cx);
                let mut items: Vec<MenuItem<Vec<String>>> = sessions
                    .iter()
                    .filter_map(|id| store.session(id).map(|session| (id, session.config.name.clone())))
                    .map(|(id, name)| MenuItem::item(name, vec![id.clone()]).icon("run"))
                    .collect();
                items.push(MenuItem::Separator);
                items.push(MenuItem::item("Stop All", sessions).icon("stop").danger());
                let width = f32::from(window.viewport_size().width);
                let position = gpui_kit::point(gpui_kit::px((width / 2.0 - 120.0).max(0.0)), gpui_kit::px(44.0));
                let run = move |_: &mut Self, sessions: Vec<String>, _: &mut Window, cx: &mut Context<Self>| {
                    driver.update(cx, |driver, cx| sessions.iter().for_each(|id| driver.stop(id, cx)));
                };
                self.show_menu(&items, position, run, window, cx);
                return;
            }
        };
        driver.update(cx, |driver, cx| sessions.iter().for_each(|id| driver.stop(id, cx)));
    }

    /// The file tree, when a project is open.
    pub fn project_tree(&self) -> Option<&Entity<ProjectTree>> {
        match &self.content {
            Content::Project { tree, .. } => Some(tree),
            _ => None,
        }
    }

    /// Opens the folder at `root` as this window's project.
    /// Records that another editor became the active one: it is the most recently used tab, and
    /// its file the most recent of the project's recent files.
    fn note_active_tab(&mut self, cx: &mut Context<Self>) {
        let active = self
            .tabs()
            .and_then(|tabs| tabs.pane(&tabs.active_pane))
            .and_then(|pane| pane.active_tab())
            .map(|tab| (tab.input.key.clone(), tab.input.path.clone().filter(|_| tab.input.kind == "file")));
        let key = active.as_ref().map(|(key, _)| key.clone());
        if key == self.active_key {
            return;
        }
        self.active_key = key;
        let Some((key, path)) = active else { return };
        let stamp = self.activations.values().max().copied().unwrap_or(0) + 1;
        self.activations.insert(key, stamp);
        if let (Some(path), Some(backend)) = (path, cx.try_global::<Backend>())
            && let Err(error) = ide_host::ops::nav::mark_recent(&backend.host, &self.label, PathBuf::from(path))
        {
            tracing::debug!("a file was not marked as recent: {}", error.message);
        }
    }

    /// Every open editor once, the most recently active first, as the Switcher lists them.
    fn open_editor_tabs(&self) -> Vec<OpenEditorTab> {
        let Some(tabs) = self.tabs() else { return Vec::new() };
        let mut seen = std::collections::HashSet::new();
        let mut open: Vec<OpenEditorTab> = tabs
            .panes
            .iter()
            .flat_map(|pane| &pane.tabs)
            .filter(|tab| seen.insert(tab.input.key.as_str()))
            .map(|tab| {
                let path = tab.input.path.as_deref().unwrap_or(&tab.input.key);
                let title = path.rsplit('/').next().unwrap_or(path).to_owned();
                OpenEditorTab {
                    input: tab.input.clone(),
                    icon: theme::icons::file_icon_name(&title).to_owned(),
                    title,
                    last_active: self.activations.get(&tab.input.key).copied().unwrap_or(0),
                }
            })
            .collect();
        open.sort_by_key(|tab| std::cmp::Reverse(tab.last_active));
        open
    }

    /// Reads the bookmarks stored for the project at `root`.
    fn load_bookmarks(&mut self, root: &str, cx: &App) {
        let storage = cx.try_global::<UiStorage>();
        let stored = |key: String| storage.and_then(|storage| storage.get(&key));
        self.bookmarks.load(Some(root), stored(bookmarks_key(root)), stored(lists_key(root)));
    }

    pub fn bookmarks(&self) -> &BookmarkStore {
        &self.bookmarks
    }

    /// Saves what a change of the bookmarks says to save.
    fn save_bookmark_writes(writes: Vec<StorageWrite>, cx: &mut App) {
        if let Some(storage) = cx.try_global::<UiStorage>().is_some().then(|| cx.global_mut::<UiStorage>()) {
            for write in writes {
                storage.set(&write.key, write.value);
            }
        }
    }

    /// The file, caret line and that line's text of the active editor.
    fn caret_line(&self, cx: &App) -> Option<(String, u32, String)> {
        let editor = self.editor()?.read(cx);
        let line = editor.caret_position().0;
        Some((editor.path().to_string_lossy().into_owned(), line, editor.line_text(line)))
    }

    /// Adds a bookmark on `line` of `path`, or removes the one there. Without a place it is the
    /// caret's line of the active editor, described by a short selection.
    pub fn toggle_bookmark(&mut self, place: Option<(String, u32)>, cx: &mut Context<Self>) {
        let now = now_ms() as u64;
        let at = match place {
            Some((path, line)) => {
                let text = self
                    .editor()
                    .map(|editor| editor.read(cx))
                    .filter(|editor| editor.path().to_string_lossy() == path.as_str())
                    .map(|editor| editor.line_text(line))
                    .unwrap_or_default();
                Some((path, line, text, None))
            }
            None => self.caret_line(cx).map(|(path, line, text)| {
                let selected = self.editor().map(|editor| editor.read(cx).selected_text()).unwrap_or_default();
                (path, line, text, selection_description(&selected))
            }),
        };
        let Some((path, line, text, description)) = at else { return };
        let writes =
            self.bookmarks.update(|items| toggle_bookmark(items, &path, line, &text, now, description.as_deref()));
        Self::save_bookmark_writes(writes, cx);
        cx.notify();
    }

    /// Gives the caret's line the bookmark `digit`, or takes it away again.
    pub fn toggle_bookmark_digit(&mut self, digit: char, cx: &mut Context<Self>) {
        let Some((path, line, text)) = self.caret_line(cx) else { return };
        let now = now_ms() as u64;
        let writes = self.bookmarks.update(|items| toggle_mnemonic(items, &path, line, &text, digit, now));
        Self::save_bookmark_writes(writes, cx);
        cx.notify();
    }

    /// Bookmarks the given files and folders, or the active editor's file, or removes their bookmarks.
    pub fn toggle_file_bookmarks(&mut self, targets: Vec<(String, bool)>, cx: &mut Context<Self>) {
        let targets = if targets.is_empty() {
            self.editor()
                .map(|editor| (editor.read(cx).path().to_string_lossy().into_owned(), false))
                .into_iter()
                .collect()
        } else {
            targets
        };
        let now = now_ms() as u64;
        let borrowed: Vec<(&str, bool)> = targets.iter().map(|(path, dir)| (path.as_str(), *dir)).collect();
        let writes = self.bookmarks.update_data(|data| toggle_path_bookmarks(data, &borrowed, now));
        Self::save_bookmark_writes(writes, cx);
        cx.notify();
    }

    /// Goes to the bookmark after (or before) the caret, in file and line order, going round.
    pub fn go_to_neighbour_bookmark(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.caret_line(cx);
        let (path, line) = (at.as_ref().map(|at| at.0.as_str()), at.as_ref().map(|at| at.1));
        let Some(target) = neighbour_bookmark(&self.bookmarks.data().items, path, line, forward) else { return };
        let (path, line) = (PathBuf::from(&target.path), target.line);
        self.open_project_file_at(path, line, 0, window, cx);
    }

    /// Goes to the bookmark that has the mnemonic `digit`.
    pub fn go_to_bookmark(&mut self, digit: char, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = bookmark_by_mnemonic(&self.bookmarks.data().items, digit) else { return };
        let (path, line) = (PathBuf::from(&target.path), target.line);
        self.open_project_file_at(path, line, 0, window, cx);
    }

    /// Opens the Bookmarks popup; asked for again while it is open, it stays as it is.
    pub fn show_bookmarks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.popup.as_ref().is_some_and(|popup| popup.view.clone().downcast::<BookmarksPopup>().is_ok()) {
            return;
        }
        let (label, store) = (self.label.clone(), self.bookmarks.clone());
        let popup = cx.new(|cx| BookmarksPopup::new(&label, store, cx));
        let events = cx.subscribe_in(&popup, window, |workbench, popup, event: &BookmarksPopupEvent, window, cx| {
            match event.clone() {
                BookmarksPopupEvent::Dismissed => workbench.close_popup(window, cx),
                BookmarksPopupEvent::Open { path, line } => {
                    workbench.close_popup(window, cx);
                    match line {
                        Some(line) => workbench.open_project_file_at(path, line, 0, window, cx),
                        None => workbench.open_project_file(path, window, cx),
                    }
                }
                BookmarksPopupEvent::SelectInProjectView(path) => {
                    workbench.close_popup(window, cx);
                    let arg = serde_json::json!({ "path": path.to_string_lossy() });
                    window.defer(cx, move |window, cx| {
                        execute_action_from(
                            ActionSource::Other,
                            "navigate.selectInProjectView",
                            Some(&arg),
                            window,
                            cx,
                        );
                    });
                }
                // The popup changed its own copy: the window takes it over and saves it.
                BookmarksPopupEvent::Changed(writes) => {
                    workbench.bookmarks = popup.read(cx).store().clone();
                    Self::save_bookmark_writes(writes, cx);
                }
            }
        });
        self.open_popup(&popup, events, window, cx);
    }

    /// Opens the chooser that gives the caret's line a bookmark with a mnemonic.
    pub fn show_mnemonic_chooser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, line, text)) = self.caret_line(cx) else { return };
        let target = MnemonicTarget { path, line, text, at: None };
        let store = self.bookmarks.clone();
        let chooser = cx.new(|cx| MnemonicChooser::new(store, target, cx));
        let events =
            cx.subscribe_in(&chooser, window, |workbench, chooser, event: &MnemonicChooserEvent, window, cx| {
                if let MnemonicChooserEvent::Done(writes) = event.clone() {
                    workbench.bookmarks = chooser.read(cx).store().clone();
                    Self::save_bookmark_writes(writes, cx);
                }
                workbench.close_popup(window, cx);
            });
        self.open_popup(&chooser, events, window, cx);
    }

    /// Opens Go to Line:Column for the active editor; the chosen place gets the caret.
    pub fn show_goto_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let position = self.editor().map(|editor| editor.read(cx).caret_position());
        let popup = cx.new(|cx| GotoLine::new(position, cx));
        let events = cx.subscribe_in(&popup, window, |workbench, _, event: &GotoLineEvent, window, cx| {
            workbench.close_popup(window, cx);
            if let GotoLineEvent::Go { line, col } = *event
                && let Some(editor) = workbench.editor().cloned()
            {
                editor.update(cx, |editor, cx| {
                    // The line and the column are held inside the document.
                    let at = editor.offset_of(line, col);
                    editor.select(at, at, cx);
                    editor.scroll_to_line(line.saturating_sub(5), cx);
                });
            }
        });
        self.open_popup(&popup, events, window, cx);
    }

    /// The id of the active pane's selected tab.
    fn active_tab_id(&self) -> Option<&str> {
        let tabs = self.tabs()?;
        tabs.pane(&tabs.active_pane)?.active.as_deref()
    }

    /// The Find / Replace bar of the active tab, when it has one open.
    pub fn find_bar(&self) -> Option<Entity<FindBar>> {
        let Content::Project { find_bars, .. } = &self.content else { return None };
        find_bars.get(self.active_tab_id()?).map(|bar| bar.bar.clone())
    }

    /// The bars drawn: those of each pane's selected tab.
    fn shown_find_bars(&self) -> Vec<Entity<FindBar>> {
        let Content::Project { tabs, find_bars, .. } = &self.content else { return Vec::new() };
        tabs.panes
            .iter()
            .filter_map(|pane| pane.active.as_ref().and_then(|id| find_bars.get(id)))
            .map(|bar| bar.bar.clone())
            .collect()
    }

    /// The drawn bar that has the keyboard.
    fn focused_find_bar(&self, window: &Window, cx: &App) -> Option<Entity<FindBar>> {
        self.shown_find_bars().into_iter().find(|bar| bar.read(cx).has_focus(window, cx))
    }

    /// Find (`replace` false) or Replace in the active tab: its open bar takes the keyboard again, or a
    /// new one opens above the editor.
    pub fn show_find_bar(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab_id().map(str::to_owned) else { return };
        let Some(editor) = self.editor().cloned() else { return };
        if let Some(bar) = self.find_bar().filter(|bar| bar.read(cx).editor() == &editor) {
            bar.update(cx, |bar, cx| bar.show(replace, window, cx));
            return;
        }
        let bar = cx.new(|cx| FindBar::new(editor, replace, window, cx));
        let events = {
            let tab = tab.clone();
            cx.subscribe_in(&bar, window, move |workbench, bar, event: &FindBarEvent, window, cx| {
                workbench.find_bar_event(&tab, bar, event, window, cx);
            })
        };
        // The bar redraws the window's context keys (whether it has results) as it changes.
        let changes = cx.observe(&bar, |_, _, cx| cx.notify());
        let mut subscriptions = vec![events, changes];
        // The keyboard in a bar makes its pane the active one, as it does in an editor.
        let (find_field, replace_field) = {
            let bar = bar.read(cx);
            (bar.find_field().focus_handle(cx), bar.replace_field().focus_handle(cx))
        };
        for field in [find_field, replace_field] {
            let tab = tab.clone();
            subscriptions.push(cx.on_focus_in(&field, window, move |workbench, window, cx| {
                workbench.editor_took_focus(&tab, window, cx);
            }));
        }
        if let Content::Project { find_bars, .. } = &mut self.content {
            find_bars.insert(tab, TabFindBar { bar, _subscriptions: subscriptions });
        }
        cx.notify();
    }

    /// Find Next / Previous Occurrence: the active tab's bar goes to the next or previous occurrence of its
    /// search; without a bar the last search is used. With nothing to search for the bar opens instead.
    pub fn find_again(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let found = match self.find_bar() {
            Some(bar) => bar.update(cx, |bar, cx| bar.find_again(forward, cx)),
            None => find_again_in(&editor, forward, cx),
        };
        if !found {
            self.show_find_bar(false, window, cx);
        }
    }

    fn find_bar_event(
        &mut self,
        tab: &str,
        bar: &Entity<FindBar>,
        event: &FindBarEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            FindBarEvent::Closed => {
                if let Content::Project { find_bars, .. } = &mut self.content
                    && find_bars.get(tab).is_some_and(|open| &open.bar == bar)
                {
                    find_bars.remove(tab);
                }
                cx.notify();
            }
            FindBarEvent::OpenResults(results) => self.show_find_results(results, window, cx),
            &FindBarEvent::RunAction(id) => {
                window.defer(cx, move |window, cx| {
                    execute_action_from(ActionSource::Other, id, None, window, cx);
                });
            }
        }
    }

    /// Shows a bar's occurrences in the Find tool window: it searches the bar's file with the bar's query.
    fn show_find_results(&mut self, results: &FindResults, window: &mut Window, cx: &mut Context<Self>) {
        self.with_layout(window, cx, |layout| layout.show(FIND_TOOL_WINDOW, true));
        let panel = self
            .tool_window_view(FIND_TOOL_WINDOW)
            .and_then(|view| view.downcast::<crate::find::view::FindPanel>().ok());
        if let Some(panel) = panel {
            let query = results.query.clone();
            panel.update(cx, |panel, cx| panel.run_query(query, cx));
        }
    }

    pub fn find_popup(&self) -> Option<Entity<FindPopup>> {
        self.popup.as_ref().and_then(|popup| popup.view.clone().downcast().ok())
    }

    /// Opens Find in Files (or Replace in Files), starting with the editor's selection; asked
    /// for again while it is open, the open popup takes the new mode and argument.
    pub fn show_find_popup(&mut self, replace: bool, arg: FindArg, window: &mut Window, cx: &mut Context<Self>) {
        let selection = self.editor().map(|editor| editor.read(cx).selected_text()).filter(|text| !text.is_empty());
        if let Some(open) = self.find_popup() {
            open.update(cx, |popup, cx| popup.show(replace, &arg, selection.as_deref(), window, cx));
            return;
        }
        let input = FindPopupInput {
            window_label: self.label.clone(),
            root: self.project_root(cx).map(|root| root.to_string_lossy().into_owned()),
            workspace_folders: Vec::new(),
            replace,
            arg,
            selection,
            active_path: self.editor().map(|editor| editor.read(cx).path().to_string_lossy().into_owned()),
            open_paths: self.open_editor_tabs().into_iter().filter_map(|tab| tab.input.path).collect(),
            modules: Vec::new(),
        };
        let popup = cx.new(|cx| FindPopup::new(input, cx));
        let events = cx.subscribe_in(&popup, window, |workbench, popup, event: &FindPopupEvent, window, cx| {
            match event.clone() {
                FindPopupEvent::Dismissed => workbench.close_popup(window, cx),
                FindPopupEvent::Open { path, line, col, keep_open, .. } => {
                    if keep_open {
                        // A pinned popup stays, with the keyboard.
                        workbench.open_project_file_at(path, line, col, window, cx);
                        window.focus(&popup.focus_handle(cx), cx);
                    } else {
                        workbench.close_popup(window, cx);
                        workbench.open_project_file_at(path, line, col, window, cx);
                    }
                }
                FindPopupEvent::OpenInFindWindow { query, .. } => {
                    workbench.close_popup(window, cx);
                    workbench.with_layout(window, cx, |layout| layout.show(FIND_TOOL_WINDOW, true));
                    let panel = workbench
                        .tool_window_view(FIND_TOOL_WINDOW)
                        .and_then(|view| view.downcast::<crate::find::view::FindPanel>().ok());
                    if let Some(panel) = panel {
                        panel.update(cx, |panel, cx| panel.run_query(query, cx));
                    }
                }
                // No scope editor exists yet for the popup to send the user to.
                FindPopupEvent::EditScopes { .. } => {}
                FindPopupEvent::Notify(request) => workbench.notify(request, cx),
            }
        });
        self.open_popup(&popup, events, window, cx);
    }

    /// Opens Recent Files or the Switcher. Recent Files asked for again shows only the files
    /// edited in this session, and back; the Switcher asked for again moves its selection.
    pub fn show_recent_files(&mut self, kind: RecentFilesKind, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.recent_files() {
            let switcher = open.read(cx).is_switcher();
            match kind {
                RecentFilesKind::Switcher { backward, .. } if switcher => {
                    open.update(cx, |popup, cx| popup.cycle(if backward { -1 } else { 1 }, cx));
                    return;
                }
                RecentFilesKind::Recent if !switcher => {
                    open.update(cx, RecentFiles::toggle_edited_only);
                    return;
                }
                _ => {}
            }
        }
        let input = RecentFilesInput {
            window_label: self.label.clone(),
            root: self.project_root(cx).map(|root| root.to_string_lossy().into_owned()),
            active_path: self.editor().map(|editor| editor.read(cx).path().to_string_lossy().into_owned()),
            tabs: self.open_editor_tabs(),
            tool_windows: self.tool_windows().map(|layout| layout.windows().cloned().collect()).unwrap_or_default(),
            ..RecentFilesInput::default()
        };
        let popup = cx.new(|cx| RecentFiles::new(kind, input, cx));
        let events = cx.subscribe_in(&popup, window, |workbench, popup, event: &RecentFilesEvent, window, cx| {
            let popup = popup.clone();
            match event.clone() {
                RecentFilesEvent::Dismissed | RecentFilesEvent::ShowRecentLocations => {
                    workbench.close_popup(window, cx)
                }
                RecentFilesEvent::OpenFile { path, .. } => {
                    workbench.close_popup(window, cx);
                    workbench.open_project_file(path, window, cx);
                }
                RecentFilesEvent::OpenFiles { paths, lead, .. } => {
                    workbench.close_popup(window, cx);
                    for path in paths {
                        workbench.open_project_file(path, window, cx);
                    }
                    // The one the selection was on is the one that shows.
                    workbench.open_project_file(lead, window, cx);
                }
                RecentFilesEvent::OpenEditor { input, .. } => {
                    workbench.close_popup(window, cx);
                    workbench.with_tabs(window, cx, |tabs, _| {
                        let found =
                            tabs.find_tab(&input.key, None).map(|(pane, tab)| (pane.to_owned(), tab.id.clone()));
                        found.is_some_and(|(pane, tab)| tabs.set_active_pane(&pane) | tabs.activate_tab(&pane, &tab))
                    });
                }
                RecentFilesEvent::ActivateToolWindow(id) => {
                    workbench.close_popup(window, cx);
                    workbench.with_layout(window, cx, |layout| layout.show(&id, true));
                }
                RecentFilesEvent::HideToolWindow(id) => {
                    workbench.with_layout(window, cx, |layout| layout.hide(&id));
                    // Hiding the window that had the keyboard hands it to the editor: the popup is
                    // still open and keeps it.
                    window.focus(&popup.focus_handle(cx), cx);
                }
                RecentFilesEvent::ClosePath(key) | RecentFilesEvent::CloseInput(key) => {
                    if let Content::Project { tabs, .. } = &mut workbench.content {
                        tabs.close_key(&key);
                    }
                    // Without taking the keyboard: the popup stays open.
                    workbench.tabs_changed(false, window, cx);
                    popup.update(cx, |popup, cx| popup.removed(&key, cx));
                }
                RecentFilesEvent::GoToFile(query) => {
                    workbench.close_popup(window, cx);
                    workbench.open_search_everywhere(SeTab::Files, Some(query), window, cx);
                }
                RecentFilesEvent::ShowSwitcher { backward } => {
                    let control_held = window.modifiers().control;
                    workbench.show_recent_files(RecentFilesKind::Switcher { backward, control_held }, window, cx);
                }
            }
        });
        self.open_popup(&popup, events, window, cx);
    }

    pub fn recent_files(&self) -> Option<Entity<RecentFiles>> {
        self.popup.as_ref().and_then(|popup| popup.view.clone().downcast().ok())
    }

    /// Whether the status bar is drawn: there is something for it to say and View › Appearance ›
    /// Status Bar is on.
    pub fn shows_status_bar(&self, cx: &App) -> bool {
        (self.editor().is_some() || self.task_status().is_some())
            && crate::platform::settings::global(cx).appearance.show_status_bar
    }

    /// Whether the window shows nothing: no project, no file.
    pub fn is_empty(&self) -> bool {
        matches!(self.content, Content::Empty | Content::Failed(_))
    }

    /// What is unsaved is saved and the tabs are remembered, before the window shows something else.
    fn leave(&mut self, cx: &mut Context<Self>) {
        self.autosave(None, cx);
        self.save_session(cx);
        self.popup = None;
    }

    /// Closes the window's project; the window stays, showing the Welcome screen.
    pub fn close_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project_tree().is_none() {
            return;
        }
        self.leave(cx);
        self.reset_runs(cx);
        // The project's servers and runs stop in the background: this is the UI thread.
        cx.global::<Backend>().host.close_project_detached(&self.label);
        self.content = Content::Empty;
        self.sync_welcome(window, cx);
        cx.notify();
    }

    /// The Welcome screen, while the window shows it.
    pub fn welcome(&self) -> Option<&Entity<Welcome>> {
        match self.content {
            Content::Empty => self.welcome.as_ref().map(|(welcome, _)| welcome),
            _ => None,
        }
    }

    /// Keeps a Welcome screen while the window shows nothing, listing the recent projects as they
    /// are stored now, and drops it once a project or a file opens. Without a backend there are no
    /// recent projects to show.
    fn sync_welcome(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_empty() {
            self.welcome = None;
            return;
        }
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let recent = ide_workspace::RecentProjects::load(&backend.dirs.config);
        let (list, groups) = (recent.list(), recent.groups());
        if let Some((welcome, _)) = &self.welcome {
            welcome.update(cx, |view, cx| view.set_recent(&list, &groups, cx));
            return;
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let welcome =
            cx.new(|cx| Welcome::new(&list, &groups, home.as_deref(), cx).with_version(env!("CARGO_PKG_VERSION")));
        let events = cx.subscribe_in(&welcome, window, Self::welcome_event);
        // When it is what the window shows, it takes the keyboard: typing searches the recent
        // projects. Behind a failure's message it is not drawn and leaves the keys alone.
        if matches!(self.content, Content::Empty) {
            welcome.update(cx, |view, cx| view.focus_search(window, cx));
        }
        self.welcome = Some((welcome, events));
    }

    /// Carries out what the Welcome screen asks for.
    fn welcome_event(
        &mut self,
        welcome: &Entity<Welcome>,
        event: &WelcomeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let config = cx.global::<Backend>().dirs.config.clone();
        match event.clone() {
            // Once this update is over: opening replaces what the window shows.
            WelcomeEvent::Open(path) => window.defer(cx, move |window, cx| {
                if let Err(error) = windows::open_project_with_policy(&path, window, cx) {
                    tracing::error!("cannot open {}: {error:#}", path.display());
                }
            }),
            WelcomeEvent::RunAction(id) => window.defer(cx, move |window, cx| {
                execute_action_from(ActionSource::Other, id, None, window, cx);
            }),
            WelcomeEvent::Remove(path) => {
                match ide_workspace::RecentProjects::load(&config).remove(&path) {
                    Ok(()) => menu::refresh_recent(cx),
                    Err(error) => {
                        let mut request =
                            NotificationRequest::new(NotificationLevel::Error, "Cannot remove recent project");
                        request.body = Some(error.message);
                        self.notify(request, cx);
                    }
                }
                // The list shows what is stored, also when forgetting failed.
                self.sync_welcome(window, cx);
            }
            WelcomeEvent::SetGroups(groups) => {
                let recent = ide_workspace::RecentProjects::load(&config);
                match recent.set_groups(groups) {
                    Ok(groups) => {
                        let list = recent.list();
                        welcome.update(cx, |view, cx| view.set_recent(&list, &groups, cx));
                    }
                    Err(error) => {
                        let mut request =
                            NotificationRequest::new(NotificationLevel::Error, "Cannot save project groups");
                        request.body = Some(error.message);
                        self.notify(request, cx);
                        self.sync_welcome(window, cx);
                    }
                }
            }
            WelcomeEvent::Reveal(path) => cx.reveal_path(&path),
            WelcomeEvent::ShowMenu { target, position } => {
                let items = welcome.update(cx, |view, cx| view.menu(&target, cx));
                let welcome = welcome.downgrade();
                let run = move |_: &mut Self, command, window: &mut Window, cx: &mut Context<Self>| {
                    // The view is gone when the window shows a project by now.
                    let _ = welcome.update(cx, |view, cx| view.run(command, window, cx));
                };
                self.show_menu(&items, position, run, window, cx);
            }
        }
    }

    pub fn open_project(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.leave(cx);
        self.reset_runs(cx);
        self.content = Content::Opening(root.clone());
        self.sync_welcome(window, cx);
        cx.notify();
        let label = self.label.clone();
        let config = cx.global::<Backend>().dirs.config.clone();
        let opened = cx.global::<Backend>().with_host(move |host| async move {
            let project = ide_host::ops::project::open(&host, &label, &root).await?;
            let info = ide_host::ops::project::info(&project);
            // An opened project is a recent one, under the name it shows.
            let attached = info.workspace.as_ref().is_some_and(|workspace| !workspace.is_container(&project.root));
            let branch = (!attached)
                .then(|| project.repos.read().primary().and_then(|repo| repo.branch_info().ok()).and_then(|b| b.head))
                .flatten();
            let recent = ide_workspace::RecentProjects::load(&config);
            recent.touch(&project.root, branch)?;
            recent.rename(&project.root, Some(&info.display_name))?;
            let name = if info.display_name.is_empty() { info.name } else { info.display_name };
            ide_core::IdeResult::Ok((project.root.clone(), name))
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let result = opened.await;
            let _ = workbench.update_in(cx, |workbench, window, cx| {
                let opened =
                    result.map_err(|error| error.to_string()).and_then(|opened| opened.map_err(|error| error.message));
                workbench.content = match opened {
                    Ok((root, name)) => {
                        // It is the most recent project now: Open Recent lists it first.
                        menu::refresh_recent(cx);
                        workbench.load_bookmarks(&root.to_string_lossy(), cx);
                        let label = workbench.label.clone();
                        // The layout is stored per project folder, as the webview keys it.
                        let scope = root.to_string_lossy().into_owned();
                        let stored =
                            cx.try_global::<UiStorage>().and_then(|s| s.get(&storage_key(&scope)).map(str::to_owned));
                        let mut layout = Box::new(ToolWindowLayout::new());
                        layout.load(&scope, stored.as_deref());
                        let run = workbench.new_run_driver(&root.to_string_lossy(), window, cx);
                        let run_events = cx.subscribe_in(&run, window, Self::run_driver_event);
                        let tree = cx.new(|cx| ProjectTree::new(&label, root, cx));
                        let opens = cx.subscribe_in(&tree, window, |workbench, _, event: &OpenFile, window, cx| {
                            workbench.open_project_file(event.0.clone(), window, cx);
                        });
                        let project_panel = Panel::new(tree.clone(), None);
                        // The tabs, splits and carets the project was left with.
                        let saved = cx.try_global::<UiStorage>().and_then(|s| session::load_session(s, &scope));
                        let (tabs, restored_views) = match saved {
                            Some(saved) => (session::restore_tabs_state(&saved, |path| path.to_owned()), saved.files),
                            None => (TabsState::new(), Default::default()),
                        };
                        Content::Project {
                            name,
                            layout,
                            focus_served: 0,
                            tree,
                            panels: HashMap::from([(PROJECT_TOOL_WINDOW.to_owned(), project_panel)]),
                            tabs: Box::new(tabs),
                            editors: HashMap::new(),
                            loading: Default::default(),
                            restored_views,
                            reveals: HashMap::new(),
                            editor_focus: HashMap::new(),
                            find_bars: HashMap::new(),
                            _opens: opens,
                            run,
                            _run_events: run_events,
                        }
                    }
                    Err(message) => Content::Failed(message.into()),
                };
                workbench.sync_welcome(window, cx);
                // Registering a window reconciles the stored layout with what exists; a project
                // opens with the focus in its Project view when that is showing.
                workbench.with_layout(window, cx, |layout| {
                    let mut effects = Effects::default();
                    let mut merge = |more: Effects| {
                        effects.persist |= more.persist;
                        effects.focus_editor |= more.focus_editor;
                    };
                    for declared in TOOL_WINDOWS.into_iter().filter_map(tool_window_declaration) {
                        merge(layout.register(declared).1);
                    }
                    if layout.is_visible(PROJECT_TOOL_WINDOW) {
                        merge(layout.focus(PROJECT_TOOL_WINDOW));
                    }
                    effects
                });
                // A restored tab gets its editor when it is the one showing.
                workbench.tabs_changed(false, window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Opens a file of the project in a tab of the active pane; a file that already has a tab
    /// there is brought to the front instead.
    pub fn open_project_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Content::Project { tabs, editors, .. } = &mut self.content else { return };
        let key = path.to_string_lossy().into_owned();
        let pane = tabs.active_pane.clone();
        let existing = tabs.find_tab(&key, Some(&pane)).map(|(_, tab)| tab.id.clone());
        let tab_id = match existing {
            Some(id) => {
                tabs.activate_tab(&pane, &id);
                id
            }
            None => {
                let input = EditorInput { kind: "file".to_owned(), key: key.clone(), path: Some(key), data: None };
                let Some(id) = tabs.open_tab(&pane, input, OpenTabOptions::default()) else { return };
                id
            }
        };
        let loaded = editors.contains_key(&tab_id);
        self.tabs_changed(true, window, cx);
        if !loaded {
            self.load_tab(tab_id, path, true, window, cx);
        }
    }

    /// Opens a file of the project with the caret at the zero-based `line` and UTF-16 column
    /// `col`: a search match, a commit's file, an error's location.
    pub fn open_project_file_at(
        &mut self,
        path: PathBuf,
        line: u32,
        col: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = path.to_string_lossy().into_owned();
        if let Content::Project { reveals, .. } = &mut self.content {
            reveals.insert(key, (line, col));
        }
        self.open_project_file(path, window, cx);
        self.serve_reveal(cx);
    }

    /// Shows `view` over the window and gives it the keyboard. One popup is open at a time.
    fn open_popup<V: Focusable + Render>(
        &mut self,
        view: &Entity<V>,
        events: Subscription,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_popup(window, cx);
        let restore = window.focused(cx);
        window.focus(&view.focus_handle(cx), cx);
        self.popup = Some(Popup { view: view.clone().into(), restore, _events: events });
        cx.notify();
    }

    /// Closes the popup. The keyboard goes back to where it was when the popup opened, or to the
    /// editor, and the context keys say so at once: what the popup chose to do next is judged
    /// against the window as it is without it.
    pub fn close_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(popup) = self.popup.take() else { return };
        if let Some(target) = popup.restore.or_else(|| self.editor().map(|editor| editor.focus_handle(cx))) {
            window.focus(&target, cx);
        }
        self.publish_context(window, cx);
        cx.notify();
    }

    /// Opens a menu of `items` with its corner at `position`. A chosen action runs in the window;
    /// a chosen row of the menu's own goes to `on_command`. Either way the menu closes first, so
    /// what was chosen finds the window as it was.
    pub fn show_menu<C: Clone + 'static>(
        &mut self,
        items: &[MenuItem<C>],
        position: gpui_kit::Point<gpui_kit::Pixels>,
        on_command: impl Fn(&mut Self, C, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = cx.new(|cx| PopupMenu::new(items, position, window, cx));
        let events = cx.subscribe_in(&menu, window, move |workbench, _, event: &PopupMenuEvent<C>, window, cx| {
            workbench.close_popup(window, cx);
            match event.clone() {
                PopupMenuEvent::Dismissed => {}
                PopupMenuEvent::Chosen(Choice::Command(command)) => on_command(workbench, command, window, cx),
                PopupMenuEvent::Chosen(Choice::Action { id, arg }) => {
                    // Once this update is over: the action may reach for the workbench.
                    window.defer(cx, move |window, cx| {
                        execute_action_from(ActionSource::ContextMenu, &id, arg.as_ref(), window, cx);
                    });
                }
            }
        });
        self.open_popup(&menu, events, window, cx);
    }

    /// The menu that is open, when it is one of commands `C`.
    pub fn open_menu<C: 'static>(&self) -> Option<Entity<PopupMenu<C>>> {
        self.popup.as_ref().and_then(|popup| popup.view.clone().downcast().ok())
    }

    /// The menu of the tab `tab` of `pane`: closing it and its neighbours, pinning, splitting,
    /// and what can be done with its file.
    pub fn tab_menu(&self, pane: &str, tab: &str, cx: &App) -> Vec<MenuItem<TabCommand>> {
        let Some(tabs) = self.tabs() else { return Vec::new() };
        let Some(open) = tabs.pane(pane) else { return Vec::new() };
        let Some(index) = open.tabs.iter().position(|other| other.id == tab) else { return Vec::new() };
        let (this, count) = (&open.tabs[index], open.tabs.len());
        let shortcut = |action: &str| {
            cx.try_global::<input::Keymap>()
                .and_then(|keymap| keymap.0.shortcut_label(action, crate::platform::keymap::is_mac_host()))
        };
        let row = |label: &str, command: TabCommand, enabled: bool| MenuItem::entry(label, enabled.then_some(command));
        let with_shortcut = |item: MenuItem<TabCommand>, action: &str| match (item, shortcut(action)) {
            (MenuItem::Item(mut entry), Some(shortcut)) => {
                entry.shortcut = Some(shortcut);
                MenuItem::Item(entry)
            }
            (item, _) => item,
        };
        let mut items = vec![
            with_shortcut(row("Close", TabCommand::Close, true), "editor.closeTab"),
            row("Close Other Tabs", TabCommand::CloseOthers, count >= 2),
            row("Close Tabs to the Left", TabCommand::CloseLeft, index > 0),
            row("Close Tabs to the Right", TabCommand::CloseRight, index + 1 < count),
            row("Close All Tabs", TabCommand::CloseAll, true),
            row("Close Unmodified Tabs", TabCommand::CloseUnmodified, true),
        ];
        if open.tabs.iter().any(|other| other.pinned) {
            items.push(row("Close All but Pinned", TabCommand::CloseAllButPinned, true));
        }
        items.push(MenuItem::Separator);
        items.push(row(if this.pinned { "Unpin Tab" } else { "Pin Tab" }, TabCommand::TogglePin, true).icon("pin"));
        if this.preview {
            items.push(row("Keep Tab Open", TabCommand::Keep, true));
        }
        items.extend([
            MenuItem::Separator,
            row("Split and Move Right", TabCommand::SplitAndMoveRight, true),
            row("Split and Move Down", TabCommand::SplitAndMoveDown, true),
            with_shortcut(row("Split Right", TabCommand::SplitRight, true).icon("split"), "editor.splitRight"),
            with_shortcut(row("Split Down", TabCommand::SplitDown, true).icon("split-horizontal"), "editor.splitDown"),
        ]);
        if tabs.panes.len() > 1 {
            items.extend([
                row("Move to Opposite Group", TabCommand::MoveToOpposite, true),
                row("Open in Opposite Group", TabCommand::OpenInOpposite, true),
                row("Change Splitter Orientation", TabCommand::ChangeOrientation, true),
                row("Unsplit", TabCommand::Unsplit, true),
                row("Unsplit All", TabCommand::UnsplitAll, true),
            ]);
        }
        items.push(MenuItem::Separator);
        if let Some(path) = this.input.path.clone().filter(|_| this.input.kind == "file") {
            let in_project = self.project_root(cx).is_some_and(|root| std::path::Path::new(&path).starts_with(root));
            let mut copy = vec![
                MenuItem::action_with("project.copyAbsolutePath", path.clone().into()).labelled("Absolute Path"),
                MenuItem::action_with("project.copyFileName", path.clone().into()).labelled("File Name"),
            ];
            if in_project {
                copy.push(
                    MenuItem::action_with("project.copyRelativePath", path.clone().into())
                        .labelled("Path From Content Root"),
                );
            }
            items.extend([
                MenuItem::action_with("diff.compareWith", path.clone().into()).labelled("Compare With…"),
                MenuItem::action_with("diff.compareWithClipboard", serde_json::json!({ "path": path }))
                    .labelled("Compare with Clipboard"),
                MenuItem::Separator,
                MenuItem::submenu("Copy Path/Reference…", copy),
                MenuItem::action_with("project.rename", path.into()).labelled("Rename File…"),
                MenuItem::Separator,
            ]);
        }
        items.push(
            MenuItem::action_with("file.settings", "editor.general.editorTabs".into())
                .labelled("Configure Editor Tabs…"),
        );
        items
    }

    /// Carries out a row of the menu of the tab `tab` of `pane`.
    pub fn run_tab_command(
        &mut self,
        pane: &str,
        tab: &str,
        command: TabCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (pane, tab) = (pane.to_owned(), tab.to_owned());
        // The splits copy or move the tab the menu was opened on: it is the active one first.
        let activate = |workbench: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            workbench.with_tabs(window, cx, |tabs, _| tabs.set_active_pane(&pane) | tabs.activate_tab(&pane, &tab));
        };
        match command {
            TabCommand::SplitRight | TabCommand::SplitDown => {
                activate(self, window, cx);
                let dir = if command == TabCommand::SplitRight { SplitDir::Row } else { SplitDir::Column };
                return self.split_editor(dir, window, cx);
            }
            TabCommand::MoveToOpposite => {
                activate(self, window, cx);
                return self.move_tab_to_opposite_group(window, cx);
            }
            TabCommand::OpenInOpposite => {
                activate(self, window, cx);
                return self.open_in_opposite_group(window, cx);
            }
            _ => {}
        }
        let dirty: std::collections::HashSet<String> = match &self.content {
            Content::Project { editors, .. } => {
                editors.iter().filter(|(_, editor)| editor.read(cx).is_dirty()).map(|(id, _)| id.clone()).collect()
            }
            _ => Default::default(),
        };
        self.with_tabs(window, cx, |tabs, _| match command {
            TabCommand::Close => tabs.close_tab(&pane, &tab),
            TabCommand::CloseOthers => tabs.close_others(&pane, &tab),
            TabCommand::CloseLeft => tabs.close_to_left(&pane, &tab),
            TabCommand::CloseRight => tabs.close_to_right(&pane, &tab),
            TabCommand::CloseAll => tabs.close_all(&pane, false),
            TabCommand::CloseAllButPinned => tabs.close_all(&pane, true),
            TabCommand::CloseUnmodified => tabs.close_unmodified(&pane, |tab| dirty.contains(&tab.id)),
            TabCommand::TogglePin => tabs.toggle_pin(&pane, &tab),
            TabCommand::Keep => tabs.promote_tab(&pane, &tab),
            TabCommand::SplitAndMoveRight => tabs.move_tab_to_new_split(&pane, &tab, &pane, SplitEdge::Right).is_some(),
            TabCommand::SplitAndMoveDown => tabs.move_tab_to_new_split(&pane, &tab, &pane, SplitEdge::Bottom).is_some(),
            TabCommand::ChangeOrientation => tabs.change_split_orientation(&pane),
            TabCommand::Unsplit => tabs.remove_pane(&pane),
            TabCommand::UnsplitAll => tabs.unsplit_all(&pane),
            TabCommand::SplitRight
            | TabCommand::SplitDown
            | TabCommand::MoveToOpposite
            | TabCommand::OpenInOpposite => false,
        });
    }

    /// Opens Search Everywhere on `tab`, or moves the open one to it. A short selection on one
    /// line of the editor is what it starts searching for.
    pub fn show_search_everywhere(&mut self, tab: SeTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.search_everywhere() {
            open.update(cx, |popup, cx| popup.select_tab(tab, cx));
            return;
        }
        let selected = self.editor().map(|editor| editor.read(cx).selected_text());
        let query = selected.filter(|text| {
            !text.is_empty() && text.encode_utf16().count() <= SELECTION_QUERY_LIMIT && !text.contains('\n')
        });
        self.open_search_everywhere(tab, query, window, cx);
    }

    /// Opens Search Everywhere on `tab`, searching for `query`.
    fn open_search_everywhere(
        &mut self,
        tab: SeTab,
        query: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        let context = crate::platform::context::window_context(window, cx);
        let label = self.label.clone();
        let popup = cx.new(|cx| SearchEverywhere::new(&label, tab, query.as_deref(), root, context, cx));
        let events = cx.subscribe_in(&popup, window, |workbench, _, event: &SearchEverywhereEvent, window, cx| {
            match event {
                SearchEverywhereEvent::Dismissed => workbench.close_popup(window, cx),
                SearchEverywhereEvent::OpenFile { path, position } => {
                    workbench.close_popup(window, cx);
                    match position {
                        Some((line, col)) => workbench.open_project_file_at(path.clone(), *line, *col, window, cx),
                        None => workbench.open_project_file(path.clone(), window, cx),
                    }
                }
                SearchEverywhereEvent::RunAction(id) => {
                    let (id, workbench) = (id.clone(), cx.weak_entity());
                    // Once this update is over: the action, and whether it shows a state, may
                    // reach for the workbench.
                    window.defer(cx, move |window, cx| {
                        let toggle = cx
                            .global::<ActionRegistry>()
                            .get(&id)
                            .is_some_and(|action| action.is_toggled(window, cx).is_some());
                        // An action that shows a state stays in the list, where its new state shows.
                        if !toggle {
                            let _ = workbench.update(cx, |workbench, cx| workbench.close_popup(window, cx));
                        }
                        execute_action_from(ActionSource::Search, &id, None, window, cx);
                    });
                }
            }
        });
        self.open_popup(&popup, events, window, cx);
    }

    /// Opens the File Structure popup for the active editor. Asked for again while it is open,
    /// it shows or hides the members the file's types inherit.
    pub fn show_file_structure(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.file_structure() {
            open.update(cx, |popup, cx| popup.toggle_show_inherited(cx));
            return;
        }
        let editor = self.editor().map(|editor| editor.read(cx));
        let file = editor
            .map(|editor| StructureFile { path: editor.path().to_string_lossy().into_owned(), doc: editor.doc() });
        let caret = editor.map(|editor| {
            let (line, col) = editor.caret_position();
            ide_core::Pos::new(line, col)
        });
        let label = self.label.clone();
        let popup = cx.new(|cx| FileStructure::new(&label, file, caret, cx));
        let events = cx.subscribe_in(&popup, window, |workbench, _, event: &FileStructureEvent, window, cx| {
            workbench.close_popup(window, cx);
            if let FileStructureEvent::Reveal { path, line, col } = event {
                workbench.open_project_file_at(path.clone(), *line, *col, window, cx);
            }
        });
        self.open_popup(&popup, events, window, cx);
    }

    pub fn file_structure(&self) -> Option<Entity<FileStructure>> {
        self.popup.as_ref().and_then(|popup| popup.view.clone().downcast().ok())
    }

    pub fn search_everywhere(&self) -> Option<Entity<SearchEverywhere>> {
        self.popup.as_ref().and_then(|popup| popup.view.clone().downcast().ok())
    }

    /// Moves the caret of the active editor to where its file was asked to be shown.
    fn serve_reveal(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let path = editor.read(cx).path().to_string_lossy().into_owned();
        let Content::Project { reveals, .. } = &mut self.content else { return };
        if let Some((line, col)) = reveals.remove(&path) {
            editor.update(cx, |editor, cx| {
                let at = editor.offset_of(line, col);
                editor.select(at, at, cx);
                editor.scroll_to_line(line.saturating_sub(5), cx);
            });
        }
    }

    /// Brings back the tab that was closed last, where it was.
    pub fn reopen_closed_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Content::Project { tabs, .. } = &mut self.content else { return };
        let Some(reopened) = tabs.reopen_closed() else { return };
        let path = tabs
            .pane(&reopened.pane_id)
            .and_then(|pane| pane.tab(&reopened.tab_id))
            .and_then(|tab| tab.input.path.clone());
        self.tabs_changed(true, window, cx);
        if let Some(path) = path {
            self.load_tab(reopened.tab_id, PathBuf::from(path), true, window, cx);
        }
    }

    /// Opens the document of the active tab if it has none yet: a tab restored with the project
    /// is only a title until it is shown.
    fn load_active_tab(&mut self, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Content::Project { tabs, editors, loading, .. } = &self.content else { return };
        // Every pane shows its selected tab; only the active pane's takes the keyboard.
        let wanted: Vec<(String, PathBuf, bool)> = tabs
            .panes
            .iter()
            .filter_map(|pane| {
                let tab = pane.active_tab()?;
                if editors.contains_key(&tab.id) || loading.contains(&tab.id) || tab.input.kind != "file" {
                    return None;
                }
                Some((tab.id.clone(), PathBuf::from(tab.input.path.clone()?), focus && pane.id == tabs.active_pane))
            })
            .collect();
        for (tab, path, focus) in wanted {
            self.load_tab(tab, path, focus, window, cx);
        }
    }

    /// The editor of `tab` took the keyboard: its pane is the active one.
    fn editor_took_focus(&mut self, tab: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Content::Project { tabs, .. } = &mut self.content else { return };
        let Some(pane) = tabs.panes.iter().find(|pane| pane.tab(tab).is_some()).map(|pane| pane.id.clone()) else {
            return;
        };
        if tabs.active_pane != pane && tabs.set_active_pane(&pane) {
            // The keyboard is already where it belongs.
            self.tabs_changed(false, window, cx);
            cx.notify();
        }
    }

    /// Splits the active pane: a second view of its file opens beside (or below) it, at the same
    /// place, and takes the keyboard.
    pub fn split_editor(&mut self, dir: SplitDir, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.editor().map(|editor| {
            let editor = editor.read(cx);
            (editor.path().to_string_lossy().into_owned(), editor.caret_position())
        });
        let Content::Project { tabs, reveals, .. } = &mut self.content else { return };
        let pane = tabs.active_pane.clone();
        if tabs.split_pane(&pane, dir).is_none() {
            return;
        }
        if let Some((path, caret)) = at {
            reveals.insert(path, caret);
        }
        self.tabs_changed(true, window, cx);
    }

    /// Moves the active tab to the neighbouring pane, or into a new pane on the right when there
    /// is none.
    pub fn move_tab_to_opposite_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.with_tabs(window, cx, |tabs, pane| {
            let Some(tab) = tabs.pane(pane).and_then(|pane| pane.active.clone()) else { return false };
            match tabs.neighbour_pane(pane).filter(|other| *other != pane).map(str::to_owned) {
                Some(target) => {
                    let end = tabs.pane(&target).map_or(0, |pane| pane.tabs.len());
                    tabs.move_tab(pane, &tab, &target, end, None) | tabs.set_active_pane(&target)
                }
                None => tabs.move_tab_to_new_split(pane, &tab, pane, SplitEdge::Right).is_some(),
            }
        });
    }

    /// Opens the active tab's file in the neighbouring pane as well, or splits when there is none.
    pub fn open_in_opposite_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tabs) = self.tabs() else { return };
        let pane = tabs.active_pane.clone();
        let Some(input) = tabs.pane(&pane).and_then(|pane| pane.active_tab()).map(|tab| tab.input.clone()) else {
            return;
        };
        match tabs.neighbour_pane(&pane).filter(|other| *other != pane).map(str::to_owned) {
            None => self.split_editor(SplitDir::Row, window, cx),
            Some(target) => self.with_tabs(window, cx, |tabs, _| {
                tabs.open_tab(&target, input, OpenTabOptions::default()).is_some() | tabs.set_active_pane(&target)
            }),
        }
    }

    /// Gives the keyboard to the next (or previous) pane that shows a tab, going round.
    pub fn focus_splitter(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.with_tabs(window, cx, |tabs, pane| {
            let shown: Vec<String> = tabs
                .pane_ids()
                .into_iter()
                .filter(|id| tabs.pane(id).is_some_and(|pane| pane.active.is_some()))
                .map(str::to_owned)
                .collect();
            let Some(at) = shown.iter().position(|id| id == pane).filter(|_| shown.len() > 1) else { return false };
            let next = if forward { (at + 1) % shown.len() } else { (at + shown.len() - 1) % shown.len() };
            tabs.set_active_pane(&shown[next])
        });
    }

    /// Stores the project's editor session now: its tabs and splits, and where each file's
    /// editor is; a file not opened since the session was restored keeps what was saved for it.
    pub fn save_session(&mut self, cx: &mut App) {
        self.session_save = None;
        let Content::Project { tabs, editors, restored_views, layout, .. } = &self.content else { return };
        let mut files = restored_views.clone();
        for editor in editors.values() {
            let editor = editor.read(cx);
            let view =
                FileViewState { anchor: editor.anchor(), head: editor.caret(), top_line: Some(editor.top_line(cx)) };
            files.insert(editor.path().to_string_lossy().into_owned(), view);
        }
        let saved = session::serialize_session(tabs, &files, |_| false);
        let root = layout.scope().to_owned();
        if cx.has_global::<UiStorage>() {
            session::store_session(cx.global_mut::<UiStorage>(), &root, saved.as_ref());
        }
    }

    /// Opens the document of tab `tab_id` in the backend and gives the tab its editor.
    /// With `focus` the editor takes the keyboard once it is there; a tab restored with the
    /// project loads without taking it from wherever it is.
    fn load_tab(&mut self, tab_id: String, path: PathBuf, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        // One document per tab: a tab that has its editor, or is getting it, is left alone.
        let Content::Project { loading, editors, .. } = &mut self.content else { return };
        if editors.contains_key(&tab_id) || !loading.insert(tab_id.clone()) {
            return;
        }
        let label = self.label.clone();
        let opened = cx
            .global::<Backend>()
            .with_host(move |host| async move { ide_host::ops::doc::open(&host, &label, path).await });
        cx.spawn_in(window, async move |workbench, cx| {
            let result = opened.await;
            let _ = workbench.update_in(cx, |workbench, window, cx| {
                let host = cx.global::<Backend>().host.clone();
                let label = workbench.label.clone();
                let text = result
                    .map_err(|error| error.to_string())
                    .and_then(|opened| opened.map_err(|error| error.message))
                    .and_then(|info| {
                        let project = host.project(&label).map_err(|error| error.message)?;
                        let (version, rope) =
                            project.documents.rope_snapshot(info.id).map_err(|error| error.message)?;
                        Ok((info, version, rope))
                    });
                let still_open =
                    workbench.tabs().is_some_and(|tabs| tabs.panes.iter().any(|pane| pane.tab(&tab_id).is_some()));
                if let Content::Project { loading, .. } = &mut workbench.content {
                    loading.remove(&tab_id);
                }
                match text {
                    Ok((info, version, rope)) => {
                        let editor = cx.new(|cx| Editor::with_text(host, &label, &info, version, rope, cx));
                        // A tab closed while its file was opening has no use for the editor;
                        // dropping it releases the document.
                        if let (true, Content::Project { editors, restored_views, .. }) =
                            (still_open, &mut workbench.content)
                        {
                            // The file is where it was left when the project was last open.
                            let path = editor.read(cx).path().to_string_lossy().into_owned();
                            if let Some(view) = restored_views.remove(&path) {
                                editor.update(cx, |editor, cx| {
                                    editor.select(view.anchor, view.head, cx);
                                    if let Some(line) = view.top_line {
                                        editor.scroll_to_line(line, cx);
                                    }
                                });
                            }
                            let focused = tab_id.clone();
                            let took_focus =
                                cx.on_focus_in(&editor.focus_handle(cx), window, move |workbench, window, cx| {
                                    workbench.editor_took_focus(&focused, window, cx);
                                });
                            editors.insert(tab_id.clone(), editor);
                            if let Content::Project { editor_focus, .. } = &mut workbench.content {
                                editor_focus.insert(tab_id.clone(), took_focus);
                            }
                        }
                        workbench.serve_reveal(cx);
                    }
                    Err(message) => {
                        tracing::error!("cannot open the file: {message}");
                        // A tab without a document would be a dead end: it goes away again.
                        if let Content::Project { tabs, .. } = &mut workbench.content {
                            let pane =
                                tabs.panes.iter().find(|pane| pane.tab(&tab_id).is_some()).map(|pane| pane.id.clone());
                            if let Some(pane) = pane {
                                tabs.close_tab(&pane, &tab_id);
                            }
                        }
                    }
                }
                workbench.tabs_changed(focus, window, cx);
            });
        })
        .detach();
    }

    /// The message shown instead of an editor, when opening failed.
    pub fn failure(&self) -> Option<&str> {
        match &self.content {
            Content::Failed(message) => Some(message),
            _ => None,
        }
    }

    /// Opens a loose file for editing (LightEdit: no project, no tool windows).
    pub fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.content = Content::Opening(path.clone());
        self.sync_welcome(window, cx);
        cx.notify();
        let label = self.label.clone();
        let opened = cx.global::<Backend>().with_host(move |host| async move {
            let (blocking_host, blocking_label, file) = (host.clone(), label.clone(), path.clone());
            tokio::task::spawn_blocking(move || blocking_host.open_light(&blocking_label, &file).map(|_| ()))
                .await
                .map_err(|e| ide_core::IdeError::internal(e.to_string()))??;
            let file = path.canonicalize().unwrap_or(path);
            ide_host::ops::doc::open(&host, &label, file).await
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let result = opened.await;
            let _ = workbench.update_in(cx, |workbench, window, cx| {
                let host = cx.global::<Backend>().host.clone();
                let label = workbench.label.clone();
                // The document's text is read before the view exists, so a failure is a message
                // in the window instead of an editor without a document.
                let text = result
                    .map_err(|error| error.to_string())
                    .and_then(|opened| opened.map_err(|error| error.message))
                    .and_then(|info| {
                        let project = host.project(&label).map_err(|error| error.message)?;
                        let (version, rope) =
                            project.documents.rope_snapshot(info.id).map_err(|error| error.message)?;
                        Ok((info, version, rope))
                    });
                workbench.content = match text {
                    Ok((info, version, rope)) => {
                        let editor = cx.new(|cx| Editor::with_text(host, &label, &info, version, rope, cx));
                        let focus = editor.focus_handle(cx);
                        window.focus(&focus, cx);
                        workbench.observe_editor(&editor, cx);
                        Content::Editor(editor)
                    }
                    Err(message) => Content::Failed(message.into()),
                };
                workbench.sync_welcome(window, cx);
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.title(cx);
        if self.shown_title.as_deref() != Some(title.as_str()) {
            window.set_window_title(&title);
            self.shown_title = Some(title);
        }
        self.publish_context(window, cx);
        if window.is_window_active() {
            menu::schedule_sync(window, cx);
        }
        let message = |text: SharedString| {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme::color(cx, "fg-muted"))
                .child(text)
        };
        let body = match &self.content {
            Content::Empty => match &self.welcome {
                Some((welcome, _)) => welcome.clone().into_any_element(),
                None => message("".into()).into_any_element(),
            },
            Content::Opening(path) => message(format!("Opening {}…", path.display()).into()).into_any_element(),
            Content::Failed(error) => message(error.clone()).into_any_element(),
            Content::Editor(editor) => editor.clone().into_any_element(),
            Content::Project { layout, panels, .. } => {
                let area = self.tabs().map(|tabs| self.editor_area(&tabs.layout, window, cx));
                let gap = theme::length(cx, "island-gap");
                let island = || {
                    div()
                        .bg(theme::color(cx, "bg-island"))
                        .rounded(theme::length(cx, "island-radius"))
                        .overflow_hidden()
                };
                let button = theme::length(cx, "stripe-icon-button");
                let stripe_gap = theme::length(cx, "h-titlebar") - button;
                let stripe_button = |declared: &ToolWindow| {
                    let shown = layout.is_visible(&declared.id);
                    let color = theme::color(cx, if shown { "fg-emphasis" } else { "fg-muted" });
                    let action = format!("toolwindow.{}", declared.id);
                    div()
                        .id(SharedString::from(format!("stripe:{}", declared.id)))
                        .debug_selector(|| format!("stripe:{}", declared.id))
                        .size(button)
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(theme::length(cx, "radius-toolbar-button"))
                        .when(shown, |el| el.bg(theme::color(cx, "bg-stripe-selected")))
                        .hover(|el| el.bg(theme::color(cx, "bg-stripe-hover")))
                        .child(theme::icon(cx, &declared.icon, 20.0, color))
                        .on_click(cx.listener(move |_, _, window, cx| {
                            execute_action_from(ActionSource::Toolbar, &action, None, window, cx);
                        }))
                };
                // A stripe lists the windows docked on its side, then the split area's, and at
                // its foot those of the bottom: Bottom Left on the left stripe, Bottom Right on
                // the right one.
                let on_stripe = |slot: Slot| -> Vec<&ToolWindow> {
                    let showing = layout.visible().get(slot);
                    layout
                        .windows_in(slot)
                        .into_iter()
                        .filter(|w| layout.has_stripe_button(w) || showing == Some(w.id.as_str()))
                        .collect()
                };
                let stripe = |side: Anchor| {
                    let group = |slot: Slot| {
                        let windows = on_stripe(slot);
                        (!windows.is_empty()).then(|| {
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(stripe_gap)
                                .children(windows.into_iter().map(&stripe_button))
                        })
                    };
                    let lower = if side == Anchor::Left { Slot::Bottom } else { Slot::BottomSplit };
                    let outer = theme::length(cx, "stripe-outer-padding");
                    div()
                        .w(button + outer + gap)
                        .h_full()
                        .flex_none()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(stripe_gap)
                        .when(side == Anchor::Left, |el| el.pl(outer).pr(gap))
                        .when(side == Anchor::Right, |el| el.pr(outer).pl(gap))
                        .children(group(Slot::new(side, false)))
                        .children(group(Slot::new(side, true)))
                        .child(div().flex_1())
                        .children(group(lower))
                };
                // A tool window: its title, a button to hide it, and its view.
                let tool_window = |id: &str| {
                    let (declared, panel) = (layout.window(id)?, panels.get(id)?);
                    let hidden = id.to_owned();
                    Some(
                        island()
                            .min_w_0()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .h(theme::length(cx, "h-header"))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .pl(gpui_kit::px(12.0))
                                    .pr(gpui_kit::px(6.0))
                                    .text_color(theme::color(cx, "fg-strong"))
                                    .child(SharedString::from(declared.title.clone()))
                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("hide:{id}")))
                                            .size(gpui_kit::px(22.0))
                                            .flex_none()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(gpui_kit::px(4.0))
                                            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                                            .child(theme::icon(cx, "minus", 16.0, theme::color(cx, "fg-muted")))
                                            .on_click(cx.listener(move |workbench, _, window, cx| {
                                                workbench.with_layout(window, cx, |layout| layout.hide(&hidden));
                                            })),
                                    ),
                            )
                            .child(div().flex_1().min_h_0().child(panel.view.clone())),
                    )
                };
                // A side: its primary and split areas, stacked on the left and right, side by
                // side at the bottom, sharing it by the stored ratio; a lone area takes it all.
                let side = |anchor: Anchor| {
                    let shown = |split: bool| layout.visible().get(Slot::new(anchor, split)).and_then(tool_window);
                    let (primary, split) = (shown(false), shown(true));
                    let both = primary.is_some() && split.is_some();
                    if primary.is_none() && split.is_none() {
                        return None;
                    }
                    let ratio = (layout.splits().get(anchor) as f32).clamp(0.0, 1.0);
                    let share = |area: gpui_kit::Div, part: f32| match (both, anchor) {
                        (false, _) => area.size_full(),
                        (true, Anchor::Bottom) => area.h_full().w(gpui_kit::relative(part)),
                        (true, _) => area.w_full().h(gpui_kit::relative(part)),
                    };
                    Some(
                        div()
                            .flex()
                            .gap(gap)
                            .when(anchor != Anchor::Bottom, |el| el.flex_col())
                            .children(primary.map(|area| share(area, ratio)))
                            .children(split.map(|area| share(area, 1.0 - ratio))),
                    )
                };
                let sizes = layout.sizes();
                // "Tool Window Bars" off hides the stripes; the windows stay reachable by action.
                let bars = crate::platform::settings::global(cx).appearance.show_tool_window_bars;
                let right_stripe = [Slot::Right, Slot::RightSplit, Slot::BottomSplit]
                    .into_iter()
                    .any(|slot| bars && !on_stripe(slot).is_empty())
                    .then(|| stripe(Anchor::Right));
                // The editor between the left and right sides; the bottom side spans below all three.
                let top = div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .children(
                        side(Anchor::Left).map(|s| s.w(gpui_kit::px(sizes.left as f32)).h_full().flex_none().mr(gap)),
                    )
                    .child(island().flex_1().h_full().min_w_0().flex().flex_col().children(area))
                    .children(
                        side(Anchor::Right).map(|s| s.w(gpui_kit::px(sizes.right as f32)).h_full().flex_none().ml(gap)),
                    );
                let bottom =
                    side(Anchor::Bottom).map(|s| s.h(gpui_kit::px(sizes.bottom as f32)).w_full().flex_none().mt(gap));
                // A maximized side covers the editor and the other sides.
                let centre = match layout.maximized().and_then(side) {
                    Some(maximized) => div().flex_1().min_w_0().h_full().child(maximized.size_full()),
                    None => div().flex_1().min_w_0().h_full().flex().flex_col().child(top).children(bottom),
                };
                div()
                    .size_full()
                    .flex()
                    // Without a stripe the islands keep the gap it would have given them.
                    .when(right_stripe.is_none(), |el| el.pr(gap))
                    .when(!bars, |el| el.pl(gap))
                    // Without a status bar the islands keep their distance from the window's edge.
                    .when(!self.shows_status_bar(cx), |el| el.pb(gap))
                    .children(bars.then(|| stripe(Anchor::Left)))
                    .child(centre)
                    .children(right_stripe)
                    .into_any_element()
            }
        };
        let framed = self.project_tree().is_some();
        div()
            .size_full()
            .bg(theme::color(cx, if framed { "bg-frame" } else { "bg-app" }))
            .text_color(theme::color(cx, "fg"))
            .text_size(theme::length(cx, "fs-ui"))
            // Modifier taps and mouse buttons have no application-level hook: forward them.
            .on_modifiers_changed(cx.listener(|_, event: &ModifiersChangedEvent, window, cx| {
                input::modifiers_changed(&event.modifiers, window, cx);
            }))
            .capture_any_mouse_down(cx.listener(|_, event: &MouseDownEvent, window, cx| {
                if input::mouse_down(event.button, &event.modifiers, false, window, cx).consumed() {
                    cx.stop_propagation();
                }
            }))
            .flex()
            .flex_col()
            .child(
                // The title bar: the window's title on the frame colour; dragging it moves the window.
                div()
                    .h(theme::length(cx, "h-titlebar"))
                    .flex_none()
                    .flex()
                    .items_center()
                    .pl(gpui_kit::px(if cfg!(target_os = "macos") { WINDOW_BUTTONS_WIDTH } else { 10.0 }))
                    .pr(gpui_kit::px(8.0))
                    .text_color(theme::color(cx, "fg-strong"))
                    .window_control_area(gpui_kit::WindowControlArea::Drag)
                    .child(SharedString::from(self.shown_title.clone().unwrap_or_default()))
                    .child(div().flex_1())
                    // The IDE mode of the window's project; a click switches it.
                    .children(framed.then(|| {
                        let full = self.ide_mode(cx).is_full();
                        let i18n = &cx.default_global::<crate::platform::i18n::Localization>().0;
                        let label = i18n.tx(if full { "Deep mode" } else { "Blaze mode" });
                        let color = theme::color(cx, "fg-strong");
                        div()
                            .id("title:app.ideMode")
                            .h(gpui_kit::px(30.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(gpui_kit::px(5.0))
                            .px(gpui_kit::px(8.0))
                            .rounded(theme::length(cx, "radius-toolbar-button"))
                            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                            .child(theme::icon(cx, if full { "ide-mode-full" } else { "ide-mode-light" }, 16.0, color))
                            .child(SharedString::from(label))
                            .on_click(cx.listener(|_, _, window, cx| {
                                execute_action_from(ActionSource::Toolbar, "ide.toggleMode", None, window, cx);
                            }))
                    })),
            )
            .child(div().flex_1().min_h_0().w_full().child(body))
            .child(notification_balloons::layer(&self.balloons, cx))
            .children(self.popup.as_ref().map(|popup| {
                // Over the whole window; a click beside the popup closes it.
                gpui_kit::deferred(
                    div()
                        .id("popup-backdrop")
                        .absolute()
                        .inset_0()
                        .occlude()
                        .flex()
                        .justify_center()
                        .pt(gpui_kit::px(80.0))
                        .on_mouse_down(
                            gpui_kit::MouseButton::Left,
                            cx.listener(|workbench, _, window, cx| {
                                // A pinned Find in Files stays when the click is beside it.
                                let pinned = workbench.find_popup().is_some_and(|popup| popup.read(cx).pinned());
                                if !pinned {
                                    workbench.close_popup(window, cx);
                                }
                            }),
                        )
                        .child(
                            div()
                                .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(popup.view.clone()),
                        ),
                )
                .with_priority(1)
            }))
            .children(self.shows_status_bar(cx).then(|| {
                // The status bar: plain UI text, items right-aligned as in the webview's footer.
                div()
                    .h(theme::length(cx, "h-statusbar") + gpui_kit::px(4.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(gpui_kit::px(16.0))
                    .pl(gpui_kit::px(12.0))
                    .pr(gpui_kit::px(10.0))
                    .text_color(theme::color(cx, "fg-muted"))
                    .children(self.task_status().map(SharedString::from))
                    .children(self.status_items(cx).into_iter().zip(["caret", "lineEnding", "encoding"]).map(
                        |(text, item)| {
                            let read_only = self.editor().is_some_and(|editor| editor.read(cx).status().read_only);
                            // A read-only file's line separator cannot be changed.
                            let enabled = !(item == "lineEnding" && read_only);
                            div()
                                .id(SharedString::from(format!("status:editor.{item}")))
                                .debug_selector(|| format!("status:editor.{item}"))
                                .px(gpui_kit::px(4.0))
                                .rounded(gpui_kit::px(4.0))
                                .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
                                .child(SharedString::from(text))
                                .when(enabled, |el| {
                                    el.on_click(cx.listener(
                                        move |workbench, event: &gpui_kit::ClickEvent, window, cx| {
                                            let items = match item {
                                                "caret" => return workbench.show_goto_line(window, cx),
                                                "lineEnding" => workbench.line_ending_menu(cx),
                                                _ => workbench.encoding_menu(cx),
                                            };
                                            workbench.show_menu(
                                                &items,
                                                event.position(),
                                                |workbench, command, _, cx| workbench.run_status_command(command, cx),
                                                window,
                                                cx,
                                            );
                                        },
                                    ))
                                })
                        },
                    ))
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AppDirs;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::time::Duration;

    fn start(cx: &mut TestAppContext) -> tempfile::TempDir {
        cx.executor().allow_parking();
        let home = tempfile::tempdir().expect("a temporary home");
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
            crate::editor::actions::register(cx);
            register_actions(cx);
            cx.set_global(UiStorage::default());
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            menu::init(cx);
            crate::terminal::test_support::isolate_shells(home.path(), cx);
        });
        home
    }

    /// View › Toggle Light/Dark Theme switches every window and stores the choice.
    #[gpui_kit::test]
    async fn the_theme_toggle_action_switches_and_stores_the_theme(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        let _home = start(cx);
        cx.update(crate::theme::appearance::init);
        let (_workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        assert!(!cx.update(|_, cx| cx.global::<theme::ActiveTheme>().is_dark()));
        cx.update(|window, cx| execute_action_or_throw("view.toggleTheme", None, window, cx)).expect("it runs");
        assert!(cx.update(|_, cx| cx.global::<theme::ActiveTheme>().is_dark()));
        assert_eq!(cx.update(|_, cx| crate::platform::settings::global(cx).appearance.theme), "dark");
        cx.update(|window, cx| execute_action_or_throw("view.toggleTheme", None, window, cx)).expect("it runs");
        assert!(!cx.update(|_, cx| cx.global::<theme::ActiveTheme>().is_dark()));
    }

    fn settle(cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..300 {
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the backend did not deliver within three seconds");
    }

    #[gpui_kit::test]
    async fn opens_a_loose_file_in_an_editor_and_types_into_it(cx: &mut TestAppContext) {
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let file = folder.path().join("todo.txt");
        std::fs::write(&file, "milk\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_file(file.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some() || w.failure().is_some()));
        let editor = workbench
            .read_with(cx, |w, _| w.editor().cloned())
            .unwrap_or_else(|| panic!("{:?}", workbench.read_with(cx, |w, _| w.failure().map(str::to_owned))));
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "milk\n");
        // The editor has focus: typing goes to the document without a click.
        cx.simulate_input("oat ");
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "oat milk\n");
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    /// ⌘S is `file.save` in the default keymap: the keystroke goes through the dispatcher, the
    /// registry and the editor to the backend, which writes the file.
    #[gpui_kit::test]
    async fn the_keymap_runs_editor_actions_on_the_windows_editor(cx: &mut TestAppContext) {
        use crate::platform::actions::{ActionSource, execute_action_from};
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let file = folder.path().join("todo.txt");
        std::fs::write(&file, "milk\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        // Without an editor the action has nothing to act on, and says so instead of failing silently.
        assert!(
            cx.update(|window, cx| crate::platform::actions::execute_action_or_throw("file.save", None, window, cx))
                .is_err()
        );
        workbench.update_in(cx, |workbench, window, cx| workbench.open_file(file.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");

        cx.simulate_input("oat ");
        cx.simulate_keystrokes("cmd-s");
        settle(cx, |_| std::fs::read_to_string(&file).is_ok_and(|text| text == "oat milk\n"));

        // The Edit menu's actions act on the same editor.
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.undo", None, window, cx)));
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "milk\n");
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.redo", None, window, cx)));
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.selectAll", None, window, cx)));
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.cut", None, window, cx)));
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "");
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.paste", None, window, cx)));
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.copy", None, window, cx)));
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "oat milk\n");
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    #[gpui_kit::test]
    async fn a_project_shows_its_tree_and_opens_files_in_the_editor(cx: &mut TestAppContext) {
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        std::fs::write(folder.path().join("main.zig"), "const a = 1;\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench
            .update_in(cx, |workbench, window, cx| workbench.open_project(folder.path().to_path_buf(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let tree = workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("a tree");
        settle(cx, |cx| tree.read_with(cx, |tree, _| tree.rows().len() == 2));
        // The tree has focus after a project opens: Down, Down, Enter opens the file.
        cx.simulate_keystrokes("down down enter");
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "const a = 1;\n");
        // The window is titled `<project> – <file>`.
        let root = folder.path().canonicalize().expect("the folder exists");
        let project_name = root.file_name().expect("a name").to_string_lossy().into_owned();
        assert_eq!(cx.window_title(), Some(format!("{project_name} – main.zig")));
        // The facts `when` clauses read follow what is open and focused.
        let truthy = |cx: &mut VisualTestContext, key: &str| {
            cx.update(|window, cx| crate::platform::context::window_context(window, cx).is_truthy(key))
        };
        for key in ["projectOpen", "textEditorActive", "editorFocus", "editorTextFocus"] {
            assert!(truthy(cx, key), "{key}");
        }
        for key in ["lightEdit", "editorHasSelection", "toolWindowFocus", "projectOpening", "editorReadOnly"] {
            assert!(!truthy(cx, key), "{key}");
        }
        let language =
            cx.update(|window, cx| crate::platform::context::window_context(window, cx).get("editorLangId").cloned());
        assert_eq!(language, Some(crate::platform::context::ContextValue::from("zig")));
        cx.simulate_keystrokes("cmd-a");
        cx.run_until_parked();
        assert!(truthy(cx, "editorHasSelection"));
        cx.simulate_keystrokes("left");
        // The editor takes focus, so typing edits the file that was opened.
        cx.simulate_input("// ");
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "// const a = 1;\n");
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    // src/editor/core/view/statusItems.test.tsx (caret, line separator and encoding widgets)
    #[gpui_kit::test]
    async fn the_status_bar_follows_the_editors_caret_and_selection(cx: &mut TestAppContext) {
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let file = folder.path().join("notes.txt");
        std::fs::write(&file, "ab\r\ncd\r\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        assert!(workbench.read_with(cx, |w, cx| w.status_items(cx)).is_empty());
        workbench.update_in(cx, |workbench, window, cx| workbench.open_file(file.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        let items = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, cx| w.status_items(cx));
        assert_eq!(items(cx), ["1:1", "CRLF", "UTF-8"]);
        cx.simulate_keystrokes("down right");
        assert_eq!(items(cx), ["2:2", "CRLF", "UTF-8"]);
        cx.simulate_keystrokes("shift-right");
        assert_eq!(items(cx), ["2:3 (1 chars)", "CRLF", "UTF-8"]);
        // A line break is one character of the selection, whatever the file's line separator.
        cx.simulate_keystrokes("cmd-a");
        assert_eq!(items(cx)[0], "3:1 (6 chars, 2 line breaks)");
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    // src/workbench/toolwindows/store.test.ts › registers toolwindow.<id> and generic actions
    #[gpui_kit::test]
    async fn tool_window_actions_show_focus_and_hide_the_project_view(cx: &mut TestAppContext) {
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        std::fs::write(folder.path().join("main.zig"), "const a = 1;\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench
            .update_in(cx, |workbench, window, cx| workbench.open_project(folder.path().to_path_buf(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let tree = workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("a tree");
        settle(cx, |cx| tree.read_with(cx, |tree, _| tree.rows().len() == 2));
        let shown = |cx: &mut VisualTestContext| {
            workbench.read_with(cx, |w, _| w.tool_windows().expect("a layout").is_visible("project"))
        };
        let truthy = |cx: &mut VisualTestContext, key: &str| {
            cx.update(|window, cx| crate::platform::context::window_context(window, cx).is_truthy(key))
        };
        let tree_focused =
            |cx: &mut VisualTestContext| cx.update(|window, cx| tree.focus_handle(cx).is_focused(window));
        // A project opens with its Project view shown and focused.
        assert!(shown(cx) && tree_focused(cx));
        assert!(truthy(cx, "toolWindow.project.visible") && truthy(cx, "toolWindowFocus"));

        // ⌘1 on the focused window hides it; again shows and focuses it.
        cx.simulate_keystrokes("cmd-1");
        cx.run_until_parked();
        assert!(!shown(cx) && !tree_focused(cx));
        assert!(!truthy(cx, "toolWindow.project.visible") && !truthy(cx, "toolWindowFocus"));
        cx.simulate_keystrokes("cmd-1");
        cx.run_until_parked();
        assert!(shown(cx) && tree_focused(cx));

        // With an editor open, Escape in the tool window goes to the editor and ⌘1 comes back.
        cx.simulate_keystrokes("down down enter");
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        cx.simulate_keystrokes("cmd-1");
        cx.run_until_parked();
        assert!(shown(cx) && tree_focused(cx));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(shown(cx) && !tree_focused(cx) && truthy(cx, "editorFocus"));
        // Hide All Tool Windows hides it; Jump to Last Tool Window (F12) brings it back, focused.
        cx.simulate_keystrokes("cmd-shift-f12");
        cx.run_until_parked();
        assert!(!shown(cx));
        cx.simulate_keystrokes("f12");
        cx.run_until_parked();
        assert!(shown(cx) && tree_focused(cx));

        // The layout is stored under the webview's key for the project folder, in its JSON shape.
        let scope = folder.path().canonicalize().expect("the folder exists").to_string_lossy().into_owned();
        let stored =
            cx.update(|_, cx| cx.global::<UiStorage>().get(&storage_key(&scope)).map(str::to_owned)).expect("stored");
        let stored: serde_json::Value = serde_json::from_str(&stored).expect("JSON");
        assert_eq!(stored["v"], 2);
        assert_eq!(stored["visible"]["left"], "project");
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    // src/editor/core/model/model.test.ts (tabs), driven through the window and the keymap
    #[gpui_kit::test]
    async fn files_open_in_tabs_that_the_keymap_cycles_closes_and_reopens(cx: &mut TestAppContext) {
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::create_dir(root.join("a")).expect("a folder");
        std::fs::create_dir(root.join("b")).expect("a folder");
        for (file, text) in [("a/mod.rs", "// a\n"), ("b/mod.rs", "// b\n"), ("notes.txt", "notes\n")] {
            std::fs::write(root.join(file), text).expect("the file is written");
        }
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let open = |cx: &mut VisualTestContext, file: &str| {
            let path = root.join(file);
            workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(path, window, cx));
            let expected = if file == "notes.txt" {
                "notes\n"
            } else if file.starts_with('a') {
                "// a\n"
            } else {
                "// b\n"
            };
            settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).text() == expected)));
        };
        let titles = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.tab_titles());
        let active = |cx: &mut VisualTestContext| {
            titles(cx).into_iter().find(|title| title.active).map(|title| title.name).unwrap_or_default()
        };
        let tab = |name: &str, hint: Option<&str>, active: bool| TabTitle {
            name: name.to_owned(),
            hint: hint.map(str::to_owned),
            active,
        };
        let text = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, cx| w.editor().map(|e| e.read(cx).text()));

        open(cx, "a/mod.rs");
        assert_eq!(titles(cx), [tab("mod.rs", None, true)]);
        open(cx, "notes.txt");
        open(cx, "b/mod.rs");
        // Two files of the same name are told apart by their folders.
        assert_eq!(
            titles(cx),
            [tab("mod.rs", Some("a"), false), tab("notes.txt", None, false), tab("mod.rs", Some("b"), true)]
        );
        assert_eq!(text(cx).as_deref(), Some("// b\n"));
        // Opening a file that has a tab brings that tab to the front instead of adding one.
        open(cx, "notes.txt");
        assert_eq!((titles(cx).len(), active(cx)), (3, "notes.txt".to_owned()));

        // Ctrl+Right / Ctrl+Left cycle through the tabs, wrapping around.
        cx.simulate_keystrokes("ctrl-right");
        cx.run_until_parked();
        assert_eq!(text(cx).as_deref(), Some("// b\n"));
        cx.simulate_keystrokes("ctrl-right");
        cx.run_until_parked();
        assert_eq!(text(cx).as_deref(), Some("// a\n"));
        cx.simulate_keystrokes("ctrl-left ctrl-left");
        cx.run_until_parked();
        assert_eq!(active(cx), "notes.txt");

        // ⌘W closes the active tab, saving what was typed into it; the editor of a neighbour takes over.
        cx.simulate_input("more ");
        cx.simulate_keystrokes("cmd-w");
        cx.run_until_parked();
        assert_eq!(titles(cx).len(), 2);
        assert!(text(cx).is_some());
        settle(cx, |_| std::fs::read_to_string(root.join("notes.txt")).is_ok_and(|text| text == "more notes\n"));
        // Reopen Closed Tab brings it back where it was, with the saved text.
        cx.update(|window, cx| crate::platform::actions::execute_action("editor.reopenClosedTab", None, window, cx));
        settle(cx, |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).text() == "more notes\n"))
        });
        assert_eq!((titles(cx).len(), active(cx)), (3, "notes.txt".to_owned()));

        // Close Other Tabs, then closing the last one leaves an empty editor area.
        cx.update(|window, cx| crate::platform::actions::execute_action("editor.closeOtherTabs", None, window, cx));
        cx.run_until_parked();
        assert_eq!(titles(cx), [tab("notes.txt", None, true)]);
        cx.simulate_keystrokes("cmd-w");
        cx.run_until_parked();
        assert!(titles(cx).is_empty() && text(cx).is_none());
        let truthy =
            cx.update(|window, cx| crate::platform::context::window_context(window, cx).is_truthy("editorTabsOpen"));
        assert!(!truthy);
        // Every closed tab released its document in the backend.
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        settle(cx, |_| host.project("w1").is_ok_and(|project| project.documents.open_documents().is_empty()));
        host.close_project("w1");
    }

    // src/workbench/Workbench.test.tsx › background tasks › tracks task events and shows the current task
    #[gpui_kit::test]
    async fn backend_tasks_and_notifications_reach_the_window(cx: &mut TestAppContext) {
        use ide_core::TaskId;
        let _home = start(cx);
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        let send = |cx: &mut VisualTestContext, window: Option<&str>, event: IdeEvent| {
            let event = BackendEvent { window: window.map(str::to_owned), event };
            workbench.update(cx, |workbench, cx| workbench.backend_event(&event, cx));
        };
        let task = |done: bool, message: &str| IdeEvent::Task {
            id: TaskId(7),
            title: "Importing".to_owned(),
            message: Some(message.to_owned()),
            fraction: Some(0.5),
            done,
            cancellable: true,
        };
        let status = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.task_status());
        // Indexing is listed at once, with its counts; a second task is counted behind it.
        send(cx, Some("w1"), IdeEvent::IndexState { indexing: true, files: 1200, symbols: 34 });
        assert_eq!(status(cx).as_deref(), Some("Indexing: 1,200 files · 34 symbols"));
        // A short task is not shown unless it outlives the show delay; another window's is ignored.
        send(cx, Some("w1"), task(false, "module a"));
        send(cx, Some("other"), IdeEvent::IndexState { indexing: false, files: 0, symbols: 0 });
        assert_eq!(workbench.read_with(cx, |w, _| w.tasks().tasks().len()), 1);
        workbench.update(cx, |workbench, cx| workbench.tick_tasks(now_ms() + tasks::SHOW_DELAY_MS + 1.0, cx));
        assert_eq!(status(cx).as_deref(), Some("Indexing: 1,200 files · 34 symbols (+1)"));
        // Indexing ends and leaves after its linger; the import remains.
        send(cx, Some("w1"), IdeEvent::IndexState { indexing: false, files: 1200, symbols: 34 });
        workbench.update(cx, |workbench, cx| workbench.tick_tasks(now_ms() + tasks::DONE_LINGER_MS + 1.0, cx));
        assert_eq!(status(cx).as_deref(), Some("Importing: module a"));

        // A notification for every window (no window named) is kept in this one's list.
        send(
            cx,
            None,
            IdeEvent::Notify { level: NotifyLevel::Warning, title: "Disk is almost full".to_owned(), body: None },
        );
        let titles: Vec<String> =
            workbench.read_with(cx, |w, _| w.notifications().items().iter().map(|n| n.title.clone()).collect());
        assert_eq!(titles, ["Disk is almost full"]);
    }

    // src/features/app/closeGuard.ts › decideClose (the unsaved-changes question)
    #[gpui_kit::test]
    async fn closing_a_window_asks_about_unsaved_changes(cx: &mut TestAppContext) {
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        // Nothing unsaved: the window may close without a question.
        assert!(workbench.update_in(cx, |workbench, window, cx| workbench.confirm_close(window, cx)));

        workbench
            .update_in(cx, |workbench, window, cx| workbench.open_project_file(root.join("notes.txt"), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        cx.simulate_input("my ");
        // Typed a moment ago: the document counts as unsaved before the backend has said so.
        assert_eq!(workbench.read_with(cx, |w, cx| w.unsaved_count(cx)), 1);
        assert!(!cx.simulate_close());
        let (title, body) = cx.pending_prompt().expect("the question is asked");
        assert_eq!(title, "Unsaved Changes");
        let project = root.file_name().expect("a name").to_string_lossy().into_owned();
        assert_eq!(body, format!("1 file has unsaved changes in {project}. Save before closing?"));
        // Cancel: the window and the text stay, nothing is written.
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(
            workbench.read_with(cx, |w, cx| w.editor().map(|e| e.read(cx).text())).as_deref(),
            Some("my notes\n")
        );
        assert_eq!(std::fs::read_to_string(root.join("notes.txt")).expect("readable"), "notes\n");

        // Save All writes the file, then the window closes.
        assert!(!cx.simulate_close());
        cx.simulate_prompt_answer("Save All");
        settle(cx, |_| std::fs::read_to_string(root.join("notes.txt")).is_ok_and(|text| text == "my notes\n"));
        settle(cx, |cx| cx.windows().is_empty());
    }

    /// A tab whose document cannot be saved comes back with its text, and the failure is reported.
    #[cfg(unix)]
    #[gpui_kit::test]
    async fn a_tab_that_cannot_be_saved_reopens_with_its_text(cx: &mut TestAppContext) {
        use std::os::unix::fs::PermissionsExt;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::create_dir(root.join("locked")).expect("a folder");
        let file = root.join("locked/notes.txt");
        std::fs::write(&file, "notes\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(file.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        cx.simulate_input("my ");
        // Neither the file nor its folder can be written any more.
        let lock = |mode: u32, path: &std::path::Path| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("permissions change")
        };
        lock(0o444, &file);
        lock(0o555, &root.join("locked"));

        cx.simulate_keystrokes("cmd-w");
        settle(cx, |cx| workbench.read_with(cx, |w, _| !w.notifications().items().is_empty()));
        workbench.read_with(cx, |w, cx| {
            assert_eq!(w.notifications().items()[0].title, "Could not save all files");
            assert_eq!(w.tab_titles().len(), 1, "the tab is back");
            assert_eq!(w.editor().map(|e| e.read(cx).text()).as_deref(), Some("my notes\n"));
            assert_eq!(w.unsaved_count(cx), 1);
        });
        assert_eq!(std::fs::read_to_string(&file).expect("readable"), "notes\n");

        // Once the file can be written again, saving succeeds and nothing was lost.
        lock(0o755, &root.join("locked"));
        lock(0o644, &file);
        cx.simulate_keystrokes("cmd-s");
        settle(cx, |_| std::fs::read_to_string(&file).is_ok_and(|text| text == "my notes\n"));
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    // src/features/app/title.ts
    /// With Files › Autosave off nothing is saved behind the user's back: leaving a tab keeps
    /// it unsaved, and closing it asks. Cancel brings the tab back, Don't Save drops the edits,
    /// Save All writes them.
    #[gpui_kit::test]
    async fn without_autosave_closing_an_unsaved_tab_asks(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use ide_workspace::settings::SettingsScope;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("a.txt"), "a\n").expect("the file is written");
        std::fs::write(root.join("b.txt"), "b\n").expect("the file is written");
        cx.update(|cx| {
            crate::platform::settings::set("w1", SettingsScope::Global, "files.autoSave", &false.into(), cx)
                .expect("the setting is stored");
        });
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let open = |name: &str, cx: &mut VisualTestContext| {
            let path = root.join(name);
            workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(path.clone(), window, cx));
            settle(cx, |cx| {
                workbench.read_with(cx, |w, cx| w.editor().is_some_and(|editor| editor.read(cx).path() == path))
            });
        };
        let on_disk = |name: &str| std::fs::read_to_string(root.join(name)).expect("readable");
        let close = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| execute_action_or_throw("editor.closeTab", None, window, cx)).expect("it runs");
        };
        let names = |cx: &mut VisualTestContext| -> Vec<String> {
            workbench.read_with(cx, |w, _| w.tab_titles().into_iter().map(|tab| tab.name).collect())
        };

        open("a.txt", cx);
        cx.simulate_input("my ");
        // Leaving the tab does not save it.
        open("b.txt", cx);
        cx.run_until_parked();
        assert_eq!(on_disk("a.txt"), "a\n");
        assert_eq!(workbench.read_with(cx, |w, cx| w.unsaved_count(cx)), 1);

        // Closing it asks; Cancel brings the tab back with its text.
        open("a.txt", cx);
        close(cx);
        let (title, body) = cx.pending_prompt().expect("the question is asked");
        assert_eq!(title, "Unsaved Changes");
        let path = root.join("a.txt").to_string_lossy().into_owned();
        assert_eq!(body, format!("1 file has unsaved changes in {path}. Save before closing?"));
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(names(cx), ["b.txt", "a.txt"]);
        assert_eq!(workbench.read_with(cx, |w, cx| w.editor().map(|e| e.read(cx).text())).as_deref(), Some("my a\n"));
        assert_eq!(on_disk("a.txt"), "a\n");

        // Save All writes it and the tab stays closed.
        close(cx);
        cx.simulate_prompt_answer("Save All");
        settle(cx, |_| on_disk("a.txt") == "my a\n");
        assert_eq!(names(cx), ["b.txt"]);

        // Don't Save drops the edits.
        cx.simulate_input("their ");
        close(cx);
        cx.simulate_prompt_answer("Don't Save");
        cx.run_until_parked();
        assert!(names(cx).is_empty());
        assert_eq!(on_disk("b.txt"), "b\n");
        assert_eq!(workbench.read_with(cx, |w, cx| w.unsaved_count(cx)), 0);
    }

    /// The Terminal docks at the bottom: its action shows it with a shell of the project's
    /// folder and the focus in it, typing reaches the shell, its stripe button is at the foot
    /// of the left stripe, and hiding it keeps the shell running for the next time.
    #[gpui_kit::test]
    async fn the_terminal_tool_window_runs_a_shell_at_the_bottom(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use crate::terminal::view::TerminalView;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        workbench.read_with(cx, |workbench, _| {
            let Content::Project { layout, .. } = &workbench.content else { panic!("a project is open") };
            assert_eq!(layout.slot_of(TERMINAL_TOOL_WINDOW), Some(Slot::Bottom));
            assert!(!workbench.is_tool_window_visible(TERMINAL_TOOL_WINDOW));
            assert!(workbench.tool_window_view(TERMINAL_TOOL_WINDOW).is_none(), "no shell before it is shown");
        });

        cx.update(|window, cx| execute_action_or_throw("toolwindow.terminal", None, window, cx)).expect("it runs");
        let terminals = |cx: &mut VisualTestContext| {
            workbench
                .read_with(cx, |w, _| w.tool_window_view(TERMINAL_TOOL_WINDOW))
                .and_then(|view| view.downcast::<crate::terminal::panel::TerminalPanel>().ok())
                .expect("the Terminal window holds its tabs")
        };
        let terminal: Entity<TerminalView> =
            terminals(cx).read_with(cx, |panel, _| panel.active_view().cloned()).expect("a first tab");
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(TERMINAL_TOOL_WINDOW)));
        assert_eq!(terminal.read_with(cx, |terminal, _| terminal.error().map(str::to_owned)), None);
        let long = |cx: &mut VisualTestContext, done: &dyn Fn(&mut VisualTestContext) -> bool| {
            for _ in 0..3000 {
                cx.run_until_parked();
                if done(cx) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("the shell did not answer within thirty seconds");
        };
        let screen = |cx: &mut VisualTestContext| terminal.read_with(cx, |t, _| t.emulator().lines().join("\n"));
        // The focus is in the terminal: typing goes to the shell, which starts in the project.
        cx.simulate_input("echo in-$(basename \"$PWD\")-$((6*7))");
        cx.simulate_keystrokes("enter");
        let expected = format!("in-{}-42", root.file_name().expect("a name").to_string_lossy());
        long(cx, &|cx| screen(cx).contains(&expected));
        assert!(cx.update(|window, cx| {
            crate::platform::context::window_context(window, cx).is_truthy("toolWindowFocus")
        }));

        // The same action hides the focused window; the shell stays, and shows again as it was.
        cx.update(|window, cx| execute_action_or_throw("toolwindow.terminal", None, window, cx)).expect("it runs");
        assert!(!workbench.read_with(cx, |w, _| w.is_tool_window_visible(TERMINAL_TOOL_WINDOW)));
        assert!(terminal.read_with(cx, |terminal, _| terminal.id().is_some()), "hidden, not ended");
        cx.update(|window, cx| execute_action_or_throw("toolwindow.terminal", None, window, cx)).expect("it runs");
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(TERMINAL_TOOL_WINDOW)));
        assert!(screen(cx).contains(&expected));
        let again = terminals(cx).read_with(cx, |panel, _| panel.active_view().cloned()).expect("a tab");
        assert_eq!(again.entity_id(), terminal.entity_id(), "the same session");

        // New Terminal Tab opens a second shell with the keyboard in it; Close Terminal Session
        // ends it and the first tab shows again.
        cx.update(|window, cx| execute_action_or_throw("terminal.new", None, window, cx)).expect("it runs");
        let titles = |cx: &mut VisualTestContext| terminals(cx).read_with(cx, |panel, _| panel.tab_titles());
        assert_eq!(titles(cx), [("Local".to_owned(), false), ("Local (2)".to_owned(), true)]);
        cx.run_until_parked();
        let second = terminals(cx).read_with(cx, |panel, _| panel.active_view().cloned()).expect("a tab");
        assert!(cx.update(|window, cx| second.focus_handle(cx).is_focused(window)));
        cx.update(|window, cx| execute_action_or_throw("terminal.closeSession", None, window, cx)).expect("it runs");
        assert_eq!(titles(cx), [("Local".to_owned(), true)]);
        // Clear Terminal Buffer empties the first tab's screen down to its prompt.
        cx.update(|window, cx| {
            let focus = terminal.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| execute_action_or_throw("terminal.clear", None, window, cx)).expect("it runs");
        assert!(!screen(cx).contains(&expected));

        // The shell ends: its tab closes, and with the last tab the tool window hides. Shown
        // again, it starts a new shell.
        cx.simulate_input("exit");
        cx.simulate_keystrokes("enter");
        long(cx, &|cx| !workbench.read_with(cx, |w, _| w.is_tool_window_visible(TERMINAL_TOOL_WINDOW)));
        cx.update(|window, cx| execute_action_or_throw("toolwindow.terminal", None, window, cx)).expect("it runs");
        let fresh = terminals(cx).read_with(cx, |panel, _| panel.active_view().cloned()).expect("a new tab");
        assert_ne!(fresh.entity_id(), terminal.entity_id());
        assert_eq!(titles(cx).len(), 1);

        // With no tab left, New Terminal Tab opens exactly one and shows the window.
        cx.update(|window, cx| execute_action_or_throw("terminal.closeSession", None, window, cx)).expect("it runs");
        cx.run_until_parked();
        assert!(!workbench.read_with(cx, |w, _| w.is_tool_window_visible(TERMINAL_TOOL_WINDOW)));
        cx.update(|window, cx| execute_action_or_throw("terminal.new", None, window, cx)).expect("it runs");
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(TERMINAL_TOOL_WINDOW)));
        assert_eq!(titles(cx), [("Local".to_owned(), true)]);
    }

    /// New Terminal Tab on a project whose Terminal was never opened shows it with one tab,
    /// the keyboard in it.
    #[gpui_kit::test]
    async fn a_new_terminal_tab_opens_the_terminal_with_one_tab(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use crate::terminal::panel::TerminalPanel;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        cx.update(|window, cx| execute_action_or_throw("terminal.new", None, window, cx)).expect("it runs");
        let terminals = workbench
            .read_with(cx, |w, _| w.tool_window_view(TERMINAL_TOOL_WINDOW))
            .and_then(|view| view.downcast::<TerminalPanel>().ok())
            .expect("the Terminal window holds its tabs");
        assert_eq!(terminals.read_with(cx, |panel, _| panel.tab_titles()), [("Local".to_owned(), true)]);
        cx.run_until_parked();
        let shell = terminals.read_with(cx, |panel, _| panel.active_view().cloned()).expect("a tab");
        assert!(cx.update(|window, cx| shell.focus_handle(cx).is_focused(window)));
    }

    /// Blaze and Deep mode: the toggle writes `ide.mode` for every project and the backend
    /// applies it to this one; a project that names the mode itself is switched too.
    #[gpui_kit::test]
    async fn the_ide_mode_toggle_switches_between_blaze_and_deep(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use crate::platform::settings;
        use ide_core::settings::IdeMode;
        use ide_workspace::settings::SettingsScope;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let toggle = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| execute_action_or_throw("ide.toggleMode", None, window, cx)).expect("it runs");
        };
        let backend_mode = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| cx.global::<Backend>().host.project("w1").map(|p| p.ide_mode()))
        };
        let toggled = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let action = cx.global::<ActionRegistry>().get("ide.toggleMode").expect("registered");
                action.is_toggled(window, cx)
            })
        };
        assert_eq!(workbench.read_with(cx, |w, cx| w.ide_mode(cx)), IdeMode::Light);
        assert_eq!(toggled(cx), Some(false));

        toggle(cx);
        assert_eq!(workbench.read_with(cx, |w, cx| w.ide_mode(cx)), IdeMode::Full);
        assert_eq!(cx.update(|_, cx| settings::global(cx).ide.mode), IdeMode::Full);
        assert_eq!(backend_mode(cx).ok(), Some(IdeMode::Full));
        assert_eq!(toggled(cx), Some(true));
        toggle(cx);
        assert_eq!(backend_mode(cx).ok(), Some(IdeMode::Light));

        // The project says Blaze itself: the global switch alone would change nothing here.
        cx.update(|_, cx| {
            settings::set("w1", SettingsScope::Project, "ide.mode", &"light".into(), cx).expect("stored");
        });
        toggle(cx);
        assert_eq!(cx.update(|_, cx| settings::global(cx).ide.mode), IdeMode::Full);
        assert_eq!(workbench.read_with(cx, |w, cx| w.ide_mode(cx)), IdeMode::Full);
        assert_eq!(backend_mode(cx).ok(), Some(IdeMode::Full));
    }

    /// A project's tabs come back when it is opened again: the same files in the same order,
    /// the same one active with its selection and scroll position, and the others loaded only
    /// when they are shown.
    #[gpui_kit::test]
    async fn a_projects_tabs_and_carets_are_restored(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        let long: String = (1..=200).map(|n| format!("line {n}\n")).collect();
        for name in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(root.join(name), &long).expect("the file is written");
        }
        let scope = root.to_string_lossy().into_owned();
        let key = format!("{}{scope}", session::STORAGE_PREFIX);
        let (first, cx1) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        first.update_in(cx1, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx1, |cx| first.read_with(cx, |w, _| w.project_tree().is_some()));
        assert_eq!(cx1.update(|_, cx| cx.global::<UiStorage>().get(&key).map(str::to_owned)), None);
        for name in ["a.txt", "b.txt"] {
            let path = root.join(name);
            first.update_in(cx1, |workbench, window, cx| workbench.open_project_file(path.clone(), window, cx));
            settle(cx1, |cx| {
                first.read_with(cx, |w, cx| w.editor().is_some_and(|editor| editor.read(cx).path() == path))
            });
        }
        let editor = first.read_with(cx1, |w, _| w.editor().cloned()).expect("b.txt is showing");
        editor.update(cx1, |editor, cx| {
            editor.select(700, 706, cx);
            editor.scroll_to_line(90, cx);
        });
        // Leaving the window writes the session as it stands.
        first.update(cx1, |workbench, cx| workbench.save_session(cx));
        let stored: serde_json::Value = cx1
            .update(|_, cx| serde_json::from_str(cx.global::<UiStorage>().get(&key).expect("stored")))
            .expect("JSON");
        let b = root.join("b.txt").to_string_lossy().into_owned();
        assert_eq!(stored["layout"]["active"], 1);
        assert_eq!(stored["layout"]["tabs"][1]["path"], b.as_str());
        assert_eq!(stored["files"][b.as_str()], serde_json::json!({ "anchor": 700, "head": 706, "topLine": 90 }));
        cx1.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));

        let (second, cx2) = cx.add_window_view(|window, cx| Workbench::new("w2", window, cx));
        second.update_in(cx2, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx2, |cx| second.read_with(cx, |w, _| w.editor().is_some()));
        let names: Vec<(String, bool)> =
            second.read_with(cx2, |w, _| w.tab_titles().into_iter().map(|tab| (tab.name, tab.active)).collect());
        assert_eq!(names, [("a.txt".to_owned(), false), ("b.txt".to_owned(), true)]);
        let restored = second.read_with(cx2, |w, _| w.editor().cloned()).expect("an editor");
        // Restoring a tab does not take the keyboard: it is in the Project view, as on any open.
        cx2.run_until_parked();
        assert!(cx2.update(|window, cx| {
            let tree = second.read(cx).project_tree().expect("a tree").focus_handle(cx);
            tree.is_focused(window) && !restored.focus_handle(cx).is_focused(window)
        }));
        restored.read_with(cx2, |editor, cx| {
            assert_eq!(editor.path(), root.join("b.txt"));
            assert_eq!((editor.anchor(), editor.caret(), editor.top_line(cx)), (700, 706, 90));
        });
        // Only the tab that shows has a document; the other gets its own when it is shown.
        let open_documents = |cx: &mut VisualTestContext| {
            second.read_with(cx, |w, _| match &w.content {
                Content::Project { editors, .. } => editors.len(),
                _ => 0,
            })
        };
        assert_eq!(open_documents(cx2), 1);
        // Tab switching belongs to the editor: with the keyboard there, the previous tab shows.
        cx2.update(|window, cx| {
            let focus = restored.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx2.run_until_parked();
        cx2.update(|window, cx| execute_action_or_throw("editor.prevTab", None, window, cx)).expect("it runs");
        settle(cx2, |cx| {
            second.read_with(cx, |w, cx| w.editor().is_some_and(|editor| editor.read(cx).path() == root.join("a.txt")))
        });
        assert_eq!(open_documents(cx2), 2);

        // Closing every tab leaves no session behind once the change has rested.
        cx2.update(|window, cx| execute_action_or_throw("editor.closeAllTabs", None, window, cx)).expect("it runs");
        cx2.executor().advance_clock(SESSION_SAVE_DELAY);
        cx2.run_until_parked();
        assert_eq!(cx2.update(|_, cx| cx.global::<UiStorage>().get(&key).map(str::to_owned)), None);
    }

    /// The Git tool window docks at the bottom and holds the project's log, read when the
    /// window is first shown.
    #[gpui_kit::test]
    async fn the_git_tool_window_shows_the_projects_log(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use crate::vcs::log_view::GitLog;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .output()
                .expect("git runs");
            assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["add", "."]);
        git(&["-c", "user.name=Ada", "-c", "user.email=ada@example.test", "commit", "-q", "-m", "notes"]);
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        assert!(
            workbench.read_with(cx, |w, _| w.tool_window_view(GIT_TOOL_WINDOW).is_none()),
            "not read before it shows"
        );
        cx.update(|window, cx| execute_action_or_throw("toolwindow.git", None, window, cx)).expect("it runs");
        let log = workbench
            .read_with(cx, |w, _| w.tool_window_view(GIT_TOOL_WINDOW))
            .and_then(|view| view.downcast::<GitLog>().ok())
            .expect("the Git window holds the log");
        workbench.read_with(cx, |workbench, _| {
            let Content::Project { layout, .. } = &workbench.content else { panic!("a project is open") };
            assert_eq!(layout.slot_of(GIT_TOOL_WINDOW), Some(Slot::Bottom));
            assert!(workbench.is_tool_window_visible(GIT_TOOL_WINDOW));
        });
        settle(cx, |cx| log.read_with(cx, |log, _| !log.is_loading()));
        let subjects: Vec<String> =
            log.read_with(cx, |log, _| log.commits().iter().map(|c| c.summary.clone()).collect());
        assert_eq!(subjects, ["notes"]);
        assert!(cx.update(|window, cx| log.focus_handle(cx).is_focused(window)), "showing it gives it the keyboard");

        // The Commit window shares the left side with the Project view: it takes its place, and
        // a changed file opens from it.
        std::fs::write(root.join("notes.txt"), "changed\n").expect("the file is written");
        cx.update(|window, cx| execute_action_or_throw("toolwindow.commit", None, window, cx)).expect("it runs");
        let changes = workbench
            .read_with(cx, |w, _| w.tool_window_view(COMMIT_TOOL_WINDOW))
            .and_then(|view| view.downcast::<crate::vcs::commit_view::CommitPanel>().ok())
            .expect("the Commit window holds the changes");
        workbench.read_with(cx, |workbench, _| {
            assert!(workbench.is_tool_window_visible(COMMIT_TOOL_WINDOW));
            assert!(!workbench.is_tool_window_visible(PROJECT_TOOL_WINDOW));
        });
        settle(cx, |cx| changes.read_with(cx, |changes, _| !changes.is_loading()));
        assert_eq!(changes.read_with(cx, |changes, _| changes.lines()), ["Changes (1)", "  notes.txt"]);
        cx.simulate_keystrokes("down down enter");
        settle(cx, |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == root.join("notes.txt")))
        });
    }

    /// Find in Files: the action shows the Find window with the keyboard in its query, Enter
    /// searches the project, the options narrow the search, and Enter on the results opens the
    /// selected match with the caret on it.
    #[gpui_kit::test]
    async fn find_in_files_searches_the_project_and_opens_a_match(cx: &mut TestAppContext) {
        use crate::find::session::FindStatus;
        use crate::find::view::{FindOption, FindPanel};
        use crate::platform::actions::execute_action_or_throw;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::create_dir(root.join("src")).expect("a folder");
        std::fs::write(root.join("src/a.txt"), "first line\n  a Needle here\nlast\n").expect("the file is written");
        std::fs::write(root.join("src/b.txt"), "needle one\nnothing\nneedle two, needle three\n")
            .expect("the file is written");
        std::fs::write(root.join("c.txt"), "hay\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));

        cx.update(|window, cx| execute_action_or_throw("toolwindow.find", None, window, cx)).expect("it runs");
        let find = workbench
            .read_with(cx, |w, _| w.tool_window_view(FIND_TOOL_WINDOW))
            .and_then(|view| view.downcast::<FindPanel>().ok())
            .expect("the Find window holds the panel");
        assert!(cx.update(|window, cx| find.focus_handle(cx).is_focused(window)), "the query has the keyboard");
        cx.simulate_input("needle");
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| find.read_with(cx, |find, _| find.status() == FindStatus::Done));
        // Case is ignored by default: four matches on three lines of two files.
        let mut results = find.read_with(cx, |find, _| find.results());
        results.sort();
        assert_eq!(results, [("src/a.txt".to_owned(), vec![1]), ("src/b.txt".to_owned(), vec![0, 2])]);
        assert_eq!(find.read_with(cx, |find, _| find.match_count()), 4);

        // Match Case searches again with it.
        find.update(cx, |find, cx| find.toggle(FindOption::MatchCase, cx));
        settle(cx, |cx| find.read_with(cx, |find, _| find.status() == FindStatus::Done && find.match_count() == 3));
        assert_eq!(find.read_with(cx, |find, _| find.results()), [("src/b.txt".to_owned(), vec![0, 2])]);

        // The same search again lists the same results once, not twice.
        find.update(cx, |find, cx| find.search(cx));
        settle(cx, |cx| find.read_with(cx, |find, _| find.status() == FindStatus::Done));
        assert_eq!(
            find.read_with(cx, |find, _| (find.results(), find.row_count())),
            (vec![("src/b.txt".to_owned(), vec![0, 2])], 3)
        );

        // Down selects the next matching line; Enter opens it with the caret on the match.
        cx.simulate_keystrokes("down enter");
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
        settle(cx, |cx| editor.read_with(cx, |editor, _| editor.caret_position() == (2, 0)));
        assert_eq!(editor.read_with(cx, |editor, _| editor.path().to_path_buf()), root.join("src/b.txt"));

        // A pattern that is not a valid expression says so and finds nothing.
        find.update(cx, |find, cx| {
            find.toggle(FindOption::Regex, cx);
            find.field().update(cx, |field, cx| field.set_text("(", cx));
            find.search(cx);
        });
        cx.run_until_parked();
        find.read_with(cx, |find, _| {
            assert_eq!(find.status(), FindStatus::Invalid);
            assert!(find.error().is_some() && find.results().is_empty());
        });
    }

    /// Types `text` into the Search Everywhere field and lets the query rest.
    fn type_query(cx: &mut VisualTestContext, text: &str) {
        cx.simulate_input(text);
        cx.executor().advance_clock(crate::everywhere::view::DEBOUNCE);
    }

    /// A project with a Rust file, a text file and a second Rust file, opened in a window.
    fn search_project(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &mut VisualTestContext) {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::create_dir(root.join("src")).expect("a folder");
        std::fs::write(
            root.join("src/alpha.rs"),
            "pub struct Widget;\n\npub fn make_widget() -> Widget {\n    Widget\n}\n",
        )
        .expect("the file is written");
        std::fs::write(root.join("src/beta.rs"), "pub fn other() {}\n").expect("the file is written");
        std::fs::write(root.join("notes.txt"), "first\n  a needle here\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        // The index lists the files once it has walked the project.
        settle(cx, |cx| {
            let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
            ide_host::ops::nav::find_files(&host, "w1", "alpha".to_owned(), 5).is_ok_and(|found| !found.is_empty())
        });
        (folder, root, workbench, cx)
    }

    fn popup(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<SearchEverywhere> {
        workbench.read_with(cx, |w, _| w.search_everywhere()).expect("Search Everywhere is open")
    }

    /// Go to File: the action opens Search Everywhere on Files with the keyboard in its query,
    /// the typed name finds the file, and Enter opens it and closes the popup. Escape closes it
    /// and gives the keyboard back to the editor.
    #[gpui_kit::test]
    async fn go_to_file_finds_a_file_by_name_and_opens_it(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        let _home = start(cx);
        let (_folder, root, workbench, cx) = search_project(cx);

        cx.update(|window, cx| execute_action_or_throw("navigate.gotoFile", None, window, cx)).expect("it runs");
        let everywhere = popup(&workbench, cx);
        assert_eq!(everywhere.read_with(cx, |popup, _| popup.tab()), SeTab::Files);
        assert!(cx.update(|window, cx| everywhere.focus_handle(cx).is_focused(window)), "the query has the keyboard");
        cx.run_until_parked();
        assert!(cx.update(|window, cx| crate::platform::context::window_context(window, cx).is_truthy("popupOpen")));

        type_query(cx, "alph");
        settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.lines() == ["src/alpha.rs"]));
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == root.join("src/alpha.rs")))
        });
        assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_none()), "choosing a result closes the popup");

        // Enter typed before the results are in opens the top result of the new query once it is final.
        cx.update(|window, cx| execute_action_or_throw("navigate.gotoFile", None, window, cx)).expect("it runs");
        cx.simulate_input("beta");
        cx.simulate_keystrokes("enter");
        assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_some()), "Enter waits for the results");
        cx.executor().advance_clock(crate::everywhere::view::DEBOUNCE);
        settle(cx, |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == root.join("src/beta.rs")))
        });

        // `name:line` opens the file with the caret on that line.
        cx.update(|window, cx| execute_action_or_throw("navigate.gotoFile", None, window, cx)).expect("it runs");
        let everywhere = popup(&workbench, cx);
        type_query(cx, "alpha:3");
        settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.lines() == ["src/alpha.rs"]));
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| {
            workbench.read_with(cx, |w, cx| {
                w.editor().is_some_and(|e| {
                    e.read(cx).path() == root.join("src/alpha.rs") && e.read(cx).caret_position().0 == 2
                })
            })
        });

        // Escape closes the popup; the keyboard is the editor's again.
        cx.update(|window, cx| execute_action_or_throw("navigate.searchEverywhere", None, window, cx))
            .expect("it runs");
        assert_eq!(popup(&workbench, cx).read_with(cx, |popup, _| popup.tab()), SeTab::All);
        cx.simulate_keystrokes("escape");
        assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_none()));
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
        assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
        cx.run_until_parked();
        assert!(!cx.update(|window, cx| crate::platform::context::window_context(window, cx).is_truthy("popupOpen")));
    }

    /// Find Action lists the actions matching the query and Enter runs the selected one; Search
    /// Text lists matching lines and opens one with the caret on the match; Tab moves along the
    /// tabs; `/` lists the scope commands and `/files x` moves to Files with `x`.
    #[gpui_kit::test]
    async fn search_everywhere_runs_actions_finds_text_and_switches_tabs(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        let _home = start(cx);
        cx.update(crate::theme::appearance::init);
        let (_folder, root, workbench, cx) = search_project(cx);

        cx.update(|window, cx| execute_action_or_throw("navigate.findAction", None, window, cx)).expect("it runs");
        let everywhere = popup(&workbench, cx);
        assert_eq!(everywhere.read_with(cx, |popup, _| popup.tab()), SeTab::Actions);
        let toggle = action_title(cx, "view.toggleTheme");
        type_query(cx, &toggle);
        settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.lines().first() == Some(&toggle)));
        assert!(!cx.update(|_, cx| cx.global::<theme::ActiveTheme>().is_dark()));
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| cx.update(|_, cx| cx.global::<theme::ActiveTheme>().is_dark()));
        assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_none()));

        cx.update(|window, cx| execute_action_or_throw("navigate.searchText", None, window, cx)).expect("it runs");
        let everywhere = popup(&workbench, cx);
        type_query(cx, "needle");
        settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.lines() == ["notes.txt:2 a needle here"]));
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| {
            workbench.read_with(cx, |w, cx| {
                w.editor().is_some_and(|e| {
                    e.read(cx).path() == root.join("notes.txt") && e.read(cx).caret_position() == (1, 4)
                })
            })
        });

        // Go to Symbol finds a function by name and opens its file with the caret on the name.
        cx.update(|window, cx| execute_action_or_throw("navigate.gotoSymbol", None, window, cx)).expect("it runs");
        let everywhere = popup(&workbench, cx);
        type_query(cx, "make_wid");
        settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.lines().len() == 1));
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| {
            workbench.read_with(cx, |w, cx| {
                w.editor().is_some_and(|e| {
                    e.read(cx).path() == root.join("src/alpha.rs") && e.read(cx).caret_position() == (2, 7)
                })
            })
        });

        // Tab and Shift+Tab move along the tabs; the action for another tab moves the open popup.
        cx.update(|window, cx| execute_action_or_throw("navigate.gotoFile", None, window, cx)).expect("it runs");
        let everywhere = popup(&workbench, cx);
        cx.simulate_keystrokes("tab");
        assert_eq!(everywhere.read_with(cx, |popup, _| popup.tab()), SeTab::Symbols);
        cx.simulate_keystrokes("shift-tab shift-tab");
        assert_eq!(everywhere.read_with(cx, |popup, _| popup.tab()), SeTab::Classes);
        cx.update(|window, cx| execute_action_or_throw("navigate.searchText", None, window, cx)).expect("it runs");
        assert_eq!(everywhere.read_with(cx, |popup, _| popup.tab()), SeTab::Text);
        assert_eq!(popup(&workbench, cx), everywhere, "the open popup is the one that moved");

        // `/` lists the scopes; `/files be` moves to Files and searches for `be`.
        cx.simulate_input("/");
        assert_eq!(
            everywhere.read_with(cx, |popup, _| popup.lines()),
            ["/classes", "/files", "/symbols", "/actions", "/text"]
        );
        type_query(cx, "files beta");
        assert_eq!(
            everywhere.read_with(cx, |popup, cx| (popup.tab(), popup.field().read(cx).text().to_owned())),
            (SeTab::Files, "beta".to_owned())
        );
        settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.lines() == ["src/beta.rs"]));
    }

    /// A short selection on one line of the editor is what Search Everywhere starts with, selected
    /// so that typing replaces it; a selection over several lines is not taken.
    #[gpui_kit::test]
    async fn search_everywhere_starts_with_the_editor_selection(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        let _home = start(cx);
        let (_folder, root, workbench, cx) = search_project(cx);
        workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("notes.txt"), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
        editor.update(cx, |editor, cx| editor.select(editor.offset_of(1, 4), editor.offset_of(1, 10), cx));
        cx.update(|window, cx| execute_action_or_throw("navigate.searchText", None, window, cx)).expect("it runs");
        let everywhere = popup(&workbench, cx);
        assert_eq!(everywhere.read_with(cx, |popup, cx| popup.field().read(cx).selected_text().to_owned()), "needle");
        settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.lines() == ["notes.txt:2 a needle here"]));
        cx.simulate_keystrokes("escape");

        editor.update(cx, |editor, cx| editor.select(0, editor.offset_of(1, 4), cx));
        cx.update(|window, cx| execute_action_or_throw("navigate.searchText", None, window, cx)).expect("it runs");
        assert_eq!(popup(&workbench, cx).read_with(cx, |popup, cx| popup.field().read(cx).text().to_owned()), "");
    }

    /// The title an action is listed under.
    fn action_title(cx: &mut VisualTestContext, id: &str) -> String {
        cx.update(|_, cx| {
            let action = cx.global::<ActionRegistry>().get(id).expect("the action is registered");
            action.meta.localized(&cx.default_global::<crate::platform::i18n::Localization>().0).title
        })
    }

    /// The selection moves by rows (round the ends, unless Shift is held), by groups of results
    /// and by pages; Show more adds rows and leaves the selection where it was; a click on a row
    /// opens it and a click beside the popup closes it.
    #[gpui_kit::test]
    async fn search_everywhere_moves_its_selection_shows_more_and_answers_the_mouse(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use gpui_kit::{Modifiers, point, px};
        let _home = start(cx);
        let (_folder, root, workbench, cx) = search_project(cx);
        for n in 0..10 {
            std::fs::write(root.join(format!("item{n}.txt")), "item text\n").expect("the file is written");
        }
        settle(cx, |cx| {
            let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
            ide_host::ops::nav::find_files(&host, "w1", "item".to_owned(), 50).is_ok_and(|found| found.len() == 10)
        });
        cx.update(|window, cx| execute_action_or_throw("navigate.searchEverywhere", None, window, cx))
            .expect("it runs");
        let everywhere = popup(&workbench, cx);
        type_query(cx, "item");
        // Six files, six matching lines, and a "more" row for each.
        settle(cx, |cx| everywhere.read_with(cx, |popup, _| !popup.is_loading() && popup.lines().len() == 14));
        let selected = |cx: &mut VisualTestContext| everywhere.read_with(cx, |popup, _| popup.selected());
        let line = |cx: &mut VisualTestContext, index: usize| {
            everywhere.read_with(cx, |popup, _| popup.lines()[index].clone())
        };
        assert_eq!(selected(cx).as_deref(), Some("item0.txt"));

        cx.simulate_keystrokes("down");
        assert_eq!(selected(cx).as_deref(), Some("item1.txt"));
        cx.simulate_keystrokes("up up");
        assert_eq!(
            (selected(cx), line(cx, 13)),
            (Some("more".to_owned()), "more".to_owned()),
            "Up at the top goes to the end"
        );
        cx.simulate_keystrokes("down");
        assert_eq!(selected(cx).as_deref(), Some("item0.txt"), "Down at the end goes to the top");
        cx.simulate_keystrokes("shift-up");
        assert_eq!(selected(cx).as_deref(), Some("item0.txt"), "with Shift the selection stops at the top");

        // The primary modifier with an arrow, and Page Down, go to the next group of results;
        // where there is none, Page Down moves by a page.
        cx.simulate_keystrokes("cmd-down");
        assert_eq!(selected(cx), Some(line(cx, 6)));
        assert!(line(cx, 6).ends_with(":1 item text"));
        cx.simulate_keystrokes("cmd-up");
        assert_eq!(selected(cx).as_deref(), Some("item0.txt"));
        cx.simulate_keystrokes("pagedown");
        assert_eq!(selected(cx), Some(line(cx, 6)));
        cx.simulate_keystrokes("up up pagedown");
        assert_eq!(selected(cx), Some(line(cx, 6)), "Page Down from the files goes to the lines");

        // Show more of the files: the four that were held back are listed, and the selection is
        // still where the row was, not back at the top.
        cx.simulate_keystrokes("up up up up up up down down down down down down down down down down down down");
        assert_eq!((selected(cx), line(cx, 12)), (Some("more".to_owned()), "more".to_owned()));
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| everywhere.read_with(cx, |popup, _| !popup.is_loading() && popup.lines().len() == 17));
        assert_eq!(
            everywhere.read_with(cx, |popup, _| popup.lines()[..10].to_vec()),
            (0..10).map(|n| format!("item{n}.txt")).collect::<Vec<_>>()
        );
        assert_eq!(selected(cx), Some(line(cx, 12)));

        // A click on a row opens it.
        let row = cx.debug_bounds("search-everywhere-row-3").expect("the fourth row is drawn");
        cx.simulate_click(row.center(), Modifiers::default());
        settle(cx, |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == root.join("item3.txt")))
        });
        assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_none()));

        // A click beside the popup closes it; one inside it does not.
        cx.update(|window, cx| execute_action_or_throw("navigate.gotoFile", None, window, cx)).expect("it runs");
        let everywhere = popup(&workbench, cx);
        type_query(cx, "item");
        settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.lines().len() == 10));
        let row = cx.debug_bounds("search-everywhere-row-0").expect("the first row is drawn");
        cx.simulate_click(point(row.left() + px(4.0), row.top() - px(40.0)), Modifiers::default());
        assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_some()), "a click on the tabs' row keeps it");
        cx.simulate_click(point(px(6.0), row.center().y), Modifiers::default());
        assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_none()));
    }

    /// Closing Search Everywhere gives the keyboard back to what had it, and an action chosen in
    /// it runs in that context: one that needs the terminal's focus runs when the popup was opened
    /// from the terminal. An action that shows a state leaves the popup open. A `/` command row
    /// moves to its tab.
    #[gpui_kit::test]
    async fn search_everywhere_gives_the_keyboard_back_and_runs_actions_where_it_was_opened(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use crate::terminal::panel::TerminalPanel;
        let _home = start(cx);
        let (_folder, _root, workbench, cx) = search_project(cx);
        cx.update(|window, cx| execute_action_or_throw("toolwindow.terminal", None, window, cx)).expect("it runs");
        let terminals = workbench
            .read_with(cx, |w, _| w.tool_window_view(TERMINAL_TOOL_WINDOW))
            .and_then(|view| view.downcast::<TerminalPanel>().ok())
            .expect("the Terminal window holds its tabs");
        let terminal = terminals.read_with(cx, |panel, _| panel.active_view().cloned()).expect("a first tab");
        cx.run_until_parked();
        assert!(cx.update(|window, cx| terminal.focus_handle(cx).is_focused(window)));

        // Escape: the keyboard is the terminal's again, not the editor's.
        cx.update(|window, cx| execute_action_or_throw("navigate.findAction", None, window, cx)).expect("it runs");
        assert!(!cx.update(|window, cx| terminal.focus_handle(cx).is_focused(window)));
        cx.simulate_keystrokes("escape");
        assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_none()));
        assert!(cx.update(|window, cx| terminal.focus_handle(cx).is_focused(window)));
        cx.run_until_parked();

        // Close Terminal Session needs the terminal's focus: it is listed as it was when the popup
        // opened, and runs once the popup has given the keyboard back.
        cx.update(|window, cx| execute_action_or_throw("navigate.findAction", None, window, cx)).expect("it runs");
        let everywhere = popup(&workbench, cx);
        let close = action_title(cx, "terminal.closeSession");
        type_query(cx, &close);
        settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.lines().first() == Some(&close)));
        assert_eq!(terminals.read_with(cx, |panel, _| panel.tab_titles().len()), 1);
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| terminals.read_with(cx, |panel, _| panel.tab_titles().is_empty()));

        // The IDE mode is a state: its action runs and the popup stays.
        let mode = workbench.read_with(cx, |w, cx| w.ide_mode(cx));
        cx.update(|window, cx| execute_action_or_throw("navigate.findAction", None, window, cx)).expect("it runs");
        let everywhere = popup(&workbench, cx);
        let toggle = action_title(cx, "ide.toggleMode");
        type_query(cx, &toggle);
        settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.lines().first() == Some(&toggle)));
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| workbench.read_with(cx, |w, cx| w.ide_mode(cx) != mode));
        assert_eq!(workbench.read_with(cx, |w, _| w.search_everywhere()), Some(everywhere.clone()));

        // `/fi` lists the Files scope; Enter on it moves there with an empty query.
        everywhere.update(cx, |popup, cx| popup.field().update(cx, |field, cx| field.select_all(cx)));
        cx.simulate_input("/fi");
        assert_eq!(everywhere.read_with(cx, |popup, _| popup.lines()), ["/files"]);
        cx.simulate_keystrokes("enter");
        assert_eq!(
            everywhere.read_with(cx, |popup, cx| (popup.tab(), popup.field().read(cx).text().to_owned())),
            (SeTab::Files, String::new())
        );
        cx.simulate_keystrokes("escape");

        // In a window with nothing open the popup opens and closes all the same.
        let (empty, cx) = cx.add_window_view(|window, cx| Workbench::new("w2", window, cx));
        cx.update(|window, cx| execute_action_or_throw("navigate.findAction", None, window, cx)).expect("it runs");
        assert!(empty.read_with(cx, |w, _| w.search_everywhere().is_some()));
        cx.simulate_keystrokes("escape");
        assert!(empty.read_with(cx, |w, _| w.search_everywhere().is_none()));
    }

    /// The menu bar follows the focused window: what cannot run there is greyed out, items show
    /// their action's shortcut, the window's tool windows are listed. A chosen item runs its
    /// action; a key that reaches an item's shortcut after the keymap declined it does not.
    #[gpui_kit::test]
    async fn the_menu_bar_follows_the_window_and_runs_chosen_items(cx: &mut TestAppContext) {
        use menu::{MenuBar, MenuCommand, MenuNode};
        fn item<'a>(nodes: &'a [MenuNode], wanted: &str) -> Option<&'a MenuNode> {
            nodes.iter().find_map(|node| match node {
                MenuNode::Submenu { children, .. } => item(children, wanted),
                MenuNode::Item { action, .. } if action == wanted => Some(node),
                _ => None,
            })
        }
        let shown = |cx: &mut VisualTestContext, wanted: &str| {
            cx.update(|window, cx| {
                menu::sync(window, cx);
                item(cx.global::<MenuBar>().shown().expect("a bar is installed"), wanted).cloned()
            })
        };
        let _home = start(cx);
        cx.update(crate::theme::appearance::init);
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.run_until_parked();
        let shortcut = cx.update(|_, cx| {
            let binding = cx.global::<input::Keymap>().0.primary_key_binding("navigate.gotoFile").map(str::to_owned);
            binding.and_then(|binding| menu::menu_accelerator(&binding)).expect("Go to File has a shortcut")
        });
        // Nothing is open: Go to File cannot run.
        assert!(matches!(shown(cx, "navigate.gotoFile"),
            Some(MenuNode::Item { enabled: false, shortcut: Some(shown), .. }) if shown == shortcut));

        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        cx.run_until_parked();
        assert!(matches!(shown(cx, "navigate.gotoFile"), Some(MenuNode::Item { enabled: true, .. })));
        for id in TOOL_WINDOWS {
            let wanted = format!("toolwindow.{id}");
            assert!(matches!(shown(cx, &wanted), Some(MenuNode::Item { enabled: true, .. })), "{wanted} is listed");
        }

        // An item chosen in the bar runs its action in the window.
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        assert!(!cx.update(|_, cx| cx.global::<theme::ActiveTheme>().is_dark()));
        cx.dispatch_action(MenuCommand { action: "view.toggleTheme".to_owned(), arg: None });
        cx.run_until_parked();
        assert!(cx.update(|_, cx| cx.global::<theme::ActiveTheme>().is_dark()));
        cx.dispatch_action(MenuCommand { action: "navigate.gotoFile".to_owned(), arg: None });
        cx.run_until_parked();
        assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_some()));
        cx.simulate_keystrokes("escape");

        // The keymap no longer binds the key the bar still shows for Go to File: pressing it is
        // not the keymap's doing, so the item does not run.
        cx.update(|_, cx| {
            let keymap = &mut cx.global_mut::<input::Keymap>().0;
            keymap.set_overrides(std::collections::BTreeMap::from([("navigate.gotoFile".to_owned(), Vec::new())]));
            assert_eq!(keymap.primary_key_binding("navigate.gotoFile"), None);
        });
        let stroke = menu::gpui_keystroke(&shortcut).expect("the shortcut is one GPUI can press");
        cx.simulate_keystrokes(&stroke);
        cx.run_until_parked();
        assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_none()));
        // Once the bar has followed the keymap it shows no shortcut for it.
        assert!(matches!(shown(cx, "navigate.gotoFile"), Some(MenuNode::Item { shortcut: None, .. })));
    }

    /// Projects open where "Open project in" says; the window that has a project comes to the
    /// front instead of opening it twice; windows are labelled, cycled and closed over.
    #[gpui_kit::test]
    async fn projects_open_in_this_window_a_new_one_or_the_one_that_has_them(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use windows::Target;
        let _home = start(cx);
        cx.update(gpui_kit::init);
        let folders: Vec<tempfile::TempDir> =
            (0..3).map(|_| tempfile::tempdir().expect("a temporary folder")).collect();
        let roots: Vec<PathBuf> = folders.iter().map(|f| f.path().canonicalize().expect("the folder exists")).collect();
        for root in &roots {
            std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        }
        let run = |cx: &mut TestAppContext,
                   window: gpui_kit::AnyWindowHandle,
                   id: &'static str,
                   arg: Option<&std::path::Path>| {
            let arg = arg.map(|path| serde_json::Value::String(path.to_string_lossy().into_owned()));
            window
                .update(cx, |_, window, cx| execute_action_or_throw(id, arg.as_ref(), window, cx))
                .expect("the window is open")
                .expect("the action runs");
            cx.run_until_parked();
        };
        let labels = |cx: &mut TestAppContext| -> Vec<String> {
            cx.update(|cx| windows::windows(cx).into_iter().map(|(label, _)| label).collect())
        };
        let window = |cx: &mut TestAppContext, label: &str| {
            cx.update(|cx| {
                windows::windows(cx).into_iter().find(|(known, _)| known == label).expect("the window is open").1
            })
        };
        let project = |cx: &mut TestAppContext, label: &str| -> Option<PathBuf> {
            let handle = window(cx, label);
            handle
                .update(cx, |_, window, cx| for_window(window, cx).and_then(|w| w.read(cx).project_root(cx)))
                .expect("the window is open")
        };
        let wait = |cx: &mut TestAppContext, done: &dyn Fn(&mut TestAppContext) -> bool| {
            for _ in 0..300 {
                cx.run_until_parked();
                if done(cx) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("the backend did not deliver within three seconds");
        };

        // A window that shows nothing takes the project.
        let main = cx.update(|cx| windows::open_window(Target::Empty, cx)).expect("a window opens");
        assert_eq!(labels(cx), ["main"]);
        run(cx, main, "file.openProjectPath", Some(&roots[0]));
        wait(cx, &|cx| project(cx, "main").as_deref() == Some(&roots[0]));

        // One with a project asks; New Window opens the next label.
        run(cx, main, "file.openProjectPath", Some(&roots[1]));
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("New Window");
        wait(cx, &|cx| labels(cx) == ["main", "project-1"] && project(cx, "project-1").as_deref() == Some(&roots[1]));
        assert_eq!(project(cx, "main").as_deref(), Some(&*roots[0]));

        // A project that a window has is not opened again: that window comes to the front.
        let second = window(cx, "project-1");
        run(cx, second, "file.openProjectPath", Some(&roots[0]));
        assert!(!cx.has_pending_prompt());
        assert_eq!(labels(cx).len(), 2);
        assert_eq!(cx.update(|cx| cx.active_window()), Some(main));

        // Cancel leaves everything as it is; This Window replaces the window's project.
        run(cx, second, "file.openProjectPath", Some(&roots[2]));
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(project(cx, "project-1").as_deref(), Some(&*roots[1]));
        run(cx, second, "file.openProjectPath", Some(&roots[2]));
        cx.simulate_prompt_answer("This Window");
        wait(cx, &|cx| project(cx, "project-1").as_deref() == Some(&roots[2]));
        assert_eq!(labels(cx).len(), 2);

        // With the answer stored, nothing is asked.
        cx.update(|cx| {
            use ide_workspace::settings::SettingsScope;
            crate::platform::settings::set("", SettingsScope::Global, "general.openProjectIn", &"same".into(), cx)
                .expect("the setting is stored");
        });
        run(cx, second, "file.openProjectPath", Some(&roots[1]));
        assert!(!cx.has_pending_prompt());
        wait(cx, &|cx| project(cx, "project-1").as_deref() == Some(&roots[1]));

        // Open… asks for a folder and opens what was picked the same way.
        run(cx, second, "file.openProject", None);
        assert!(cx.did_prompt_for_paths());
        let picked = roots[2].clone();
        cx.simulate_path_prompt_response(move |options| {
            assert!(options.directories && !options.files);
            Some(vec![picked])
        });
        wait(cx, &|cx| project(cx, "project-1").as_deref() == Some(&roots[2]));

        // Every project that was opened is a recent one, the last first.
        let recent: Vec<PathBuf> = cx.update(|cx| {
            let config = &cx.global::<Backend>().dirs.config;
            ide_workspace::RecentProjects::load(config).list().into_iter().map(|project| project.path.0).collect()
        });
        assert_eq!(recent, [roots[2].clone(), roots[1].clone(), roots[0].clone()]);

        // Next and Previous Project Window go round the windows in label order.
        run(cx, main, "window.new", None);
        assert_eq!(labels(cx), ["main", "project-1", "project-2"]);
        let third = window(cx, "project-2");
        run(cx, third, "window.next", None);
        assert_eq!(cx.update(|cx| cx.active_window()), Some(main));
        run(cx, main, "window.previous", None);
        assert_eq!(cx.update(|cx| cx.active_window()), Some(third));

        // Close Project leaves the window, showing nothing; it takes the next project itself.
        run(cx, second, "file.closeProject", None);
        assert_eq!(project(cx, "project-1"), None);
        assert!(cx.update(|cx| cx.global::<Backend>().host.project("project-1").is_err()));
        run(cx, second, "file.openRecentProject", Some(&roots[1]));
        assert!(!cx.has_pending_prompt());
        wait(cx, &|cx| project(cx, "project-1").as_deref() == Some(&roots[1]));
    }

    /// View › Appearance: each toggle flips its setting and shows as on or off; the status bar and
    /// the tool window bars go when theirs is off, and Compact Mode makes the rows lower. Maximize
    /// gives a tool window's side the whole frame, and the resize actions move its inner edge.
    #[gpui_kit::test]
    async fn appearance_toggles_hide_the_bars_and_tool_windows_maximize_and_resize(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        let _home = start(cx);
        cx.update(crate::theme::appearance::init);
        let (_folder, root, workbench, cx) = search_project(cx);
        workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("notes.txt"), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        let run = |cx: &mut VisualTestContext, id: &str| {
            cx.update(|window, cx| execute_action_or_throw(id, None, window, cx)).expect("it runs");
            cx.run_until_parked();
        };
        let on = |cx: &mut VisualTestContext, id: &str| {
            cx.update(|window, cx| cx.global::<ActionRegistry>().get(id).expect("registered").is_toggled(window, cx))
        };
        let appearance =
            |cx: &mut VisualTestContext| cx.update(|_, cx| crate::platform::settings::global(cx).appearance);

        assert_eq!(on(cx, "view.toggleStatusBar"), Some(true));
        assert!(workbench.read_with(cx, |w, cx| w.shows_status_bar(cx)));
        run(cx, "view.toggleStatusBar");
        assert_eq!((on(cx, "view.toggleStatusBar"), appearance(cx).show_status_bar), (Some(false), false));
        assert!(!workbench.read_with(cx, |w, cx| w.shows_status_bar(cx)));
        run(cx, "view.toggleStatusBar");
        assert!(workbench.read_with(cx, |w, cx| w.shows_status_bar(cx)));

        // The stripe's buttons are drawn only while the bars are on.
        assert!(cx.debug_bounds("stripe:project").is_some());
        run(cx, "view.toggleToolWindowBars");
        assert_eq!((on(cx, "view.toggleToolWindowBars"), appearance(cx).show_tool_window_bars), (Some(false), false));
        assert!(cx.debug_bounds("stripe:project").is_none());
        run(cx, "view.toggleToolWindowBars");
        assert!(cx.debug_bounds("stripe:project").is_some());

        let row = |cx: &mut VisualTestContext| cx.update(|_, cx| f32::from(theme::length(cx, "h-row")));
        let tall = row(cx);
        run(cx, "view.toggleCompactMode");
        assert_eq!(on(cx, "view.toggleCompactMode"), Some(true));
        assert!(row(cx) < tall, "compact rows are lower: {} < {tall}", row(cx));
        run(cx, "view.toggleCompactMode");
        assert_eq!(row(cx), tall);
        run(cx, "view.toggleToolbar");
        assert_eq!((on(cx, "view.toggleToolbar"), appearance(cx).show_main_toolbar), (Some(false), false));
        run(cx, "view.toggleToolbar");

        // The Project view has the keyboard: its side grows by a step to the right and shrinks
        // back to the left; maximized it covers the editor, and again restores it.
        run(cx, "toolwindow.project");
        let layout = |cx: &mut VisualTestContext| {
            workbench.read_with(cx, |w, _| {
                let layout = w.tool_windows().expect("a project is open");
                (layout.sizes().left, layout.maximized())
            })
        };
        assert!(
            cx.update(|window, cx| crate::platform::context::window_context(window, cx).is_truthy("toolWindowFocus"))
        );
        let (width, _) = layout(cx);
        run(cx, "toolwindow.resizeRight");
        assert!(layout(cx).0 > width);
        run(cx, "toolwindow.resizeLeft");
        assert_eq!(layout(cx).0, width);
        run(cx, "toolwindow.maximize");
        assert_eq!(layout(cx).1, Some(Anchor::Left));
        run(cx, "toolwindow.maximize");
        assert_eq!(layout(cx).1, None);
    }

    /// File Structure lists the symbols of the file being edited with the one at the caret
    /// selected; Enter on another puts the caret on it and closes the popup, with the keyboard
    /// back in the editor.
    #[gpui_kit::test]
    async fn file_structure_lists_the_editors_symbols_and_goes_to_the_chosen_one(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        let _home = start(cx);
        let (_folder, root, workbench, cx) = search_project(cx);
        std::fs::write(root.join("Makefile"), "CC = gcc\n\nbuild: main.o\n\t$(CC) -o app main.o\n\nall: build\n")
            .expect("the file is written");
        workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("Makefile"), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
        editor.update(cx, |editor, cx| {
            let at = editor.offset_of(3, 2);
            editor.select(at, at, cx);
        });

        cx.update(|window, cx| execute_action_or_throw("navigate.fileStructure", None, window, cx)).expect("it runs");
        let structure = workbench.read_with(cx, |w, _| w.file_structure()).expect("File Structure is open");
        settle(cx, |cx| structure.read_with(cx, |popup, _| !popup.is_loading() && !popup.lines().is_empty()));
        assert_eq!(structure.read_with(cx, |popup, _| popup.lines()), ["CC", "build", "all"]);
        assert_eq!(structure.read_with(cx, |popup, _| popup.selected()).as_deref(), Some("build"));

        cx.simulate_keystrokes("down enter");
        settle(cx, |cx| editor.read_with(cx, |editor, _| editor.caret_position() == (5, 0)));
        assert!(workbench.read_with(cx, |w, _| w.file_structure().is_none()));
        assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
    }

    /// Recent Files lists the project's files by when they were last shown and starts on the
    /// previous one, so the action and Enter go back and forth between two files; Delete closes
    /// the selected file's tab and leaves the popup open. The Switcher opens with Ctrl held, moves
    /// on every press and opens its selection when Ctrl is released.
    #[gpui_kit::test]
    async fn recent_files_and_the_switcher_go_between_the_editors(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use gpui_kit::Modifiers;
        let _home = start(cx);
        let (_folder, root, workbench, cx) = search_project(cx);
        let showing = |cx: &mut VisualTestContext| {
            workbench.read_with(cx, |w, cx| w.editor().map(|editor| editor.read(cx).path().to_path_buf()))
        };
        for name in ["src/alpha.rs", "src/beta.rs", "notes.txt"] {
            workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join(name), window, cx));
            settle(cx, |cx| showing(cx) == Some(root.join(name)));
        }
        let run = |cx: &mut VisualTestContext, id: &str| {
            cx.update(|window, cx| execute_action_or_throw(id, None, window, cx)).expect("it runs");
            cx.run_until_parked();
        };
        let recent =
            |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.recent_files()).expect("the popup is open");

        run(cx, "navigate.recentFiles");
        let popup = recent(cx);
        settle(cx, |cx| popup.read_with(cx, |popup, _| !popup.is_loading()));
        assert_eq!(popup.read_with(cx, |popup, _| popup.lines()), ["notes.txt", "src/beta.rs", "src/alpha.rs"]);
        let beta = root.join("src/beta.rs").to_string_lossy().into_owned();
        assert_eq!(popup.read_with(cx, |popup, _| popup.selected_file()), Some(beta));
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| showing(cx) == Some(root.join("src/beta.rs")));
        assert!(workbench.read_with(cx, |w, _| w.recent_files().is_none()));

        // The file that was showing is now the previous one: the same keys go back to it.
        run(cx, "navigate.recentFiles");
        let popup = recent(cx);
        settle(cx, |cx| popup.read_with(cx, |popup, _| !popup.is_loading()));
        assert_eq!(popup.read_with(cx, |popup, _| popup.lines()), ["src/beta.rs", "notes.txt", "src/alpha.rs"]);
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| showing(cx) == Some(root.join("notes.txt")));

        // Delete closes the tab of the selected file; the popup stays, with the keyboard.
        run(cx, "navigate.recentFiles");
        let popup = recent(cx);
        settle(cx, |cx| popup.read_with(cx, |popup, _| !popup.is_loading()));
        cx.simulate_keystrokes("delete");
        cx.run_until_parked();
        assert_eq!(popup.read_with(cx, |popup, _| popup.lines()), ["notes.txt", "src/alpha.rs"]);
        assert_eq!(workbench.read_with(cx, |w, _| w.tab_titles().len()), 2);
        assert!(cx.update(|window, cx| popup.focus_handle(cx).is_focused(window)));
        cx.simulate_keystrokes("escape");
        assert!(workbench.read_with(cx, |w, _| w.recent_files().is_none()));

        // The Switcher: Ctrl is down as it opens, on the editor used before this one; a second
        // press moves on, and letting go of Ctrl opens the selection.
        cx.simulate_modifiers_change(Modifiers { control: true, ..Modifiers::default() });
        run(cx, "navigate.switcher");
        let popup = recent(cx);
        settle(cx, |cx| popup.read_with(cx, |popup, _| !popup.is_loading()));
        assert!(popup.read_with(cx, |popup, _| popup.is_switcher()));
        assert_eq!(popup.read_with(cx, |popup, _| popup.lines()), ["notes.txt", "src/alpha.rs"]);
        let alpha = root.join("src/alpha.rs").to_string_lossy().into_owned();
        assert_eq!(popup.read_with(cx, |popup, _| popup.selected_file()), Some(alpha));
        run(cx, "navigate.switcher");
        assert_eq!(popup.read_with(cx, |popup, _| popup.selected_file()), None, "past the files come the tool windows");
        run(cx, "navigate.switcherBack");
        cx.simulate_modifiers_change(Modifiers::default());
        settle(cx, |cx| showing(cx) == Some(root.join("src/alpha.rs")));
        assert!(workbench.read_with(cx, |w, _| w.recent_files().is_none()));
    }

    /// The bar follows the focused window by itself: a change of what the window can do reaches
    /// it once the window has rested, and a draw that changes nothing asks for nothing.
    #[gpui_kit::test]
    async fn the_menu_bar_follows_the_focused_window_when_it_changes(cx: &mut TestAppContext) {
        use menu::{MenuBar, MenuNode};
        fn enabled(nodes: &[MenuNode], wanted: &str) -> Option<bool> {
            nodes.iter().find_map(|node| match node {
                MenuNode::Submenu { children, .. } => enabled(children, wanted),
                MenuNode::Item { action, enabled, .. } if action == wanted => Some(*enabled),
                _ => None,
            })
        }
        let go_to_file = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| cx.global::<MenuBar>().shown().and_then(|bar| enabled(bar, "navigate.gotoFile")))
        };
        let rest = |cx: &mut VisualTestContext| {
            cx.run_until_parked();
            cx.executor().advance_clock(menu::SYNC_DELAY);
            cx.run_until_parked();
        };
        let _home = start(cx);
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        rest(cx);
        assert_eq!(go_to_file(cx), Some(false), "nothing is open");

        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        rest(cx);
        assert_eq!(go_to_file(cx), Some(true), "the project made it possible");

        // What changed while the bar was waiting is followed by one more look; after that, drawing
        // again without a change leaves the bar alone: no new request is pending.
        rest(cx);
        assert!(!cx.update(|_, cx| cx.global::<MenuBar>().is_sync_pending()));
        workbench.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert!(!cx.update(|_, cx| cx.global::<MenuBar>().is_sync_pending()));
    }

    /// Split Right shows the file a second time beside itself, at the same place and with the
    /// keyboard; what is typed in one pane shows in the other. Tabs move and open across the
    /// panes, the keyboard goes from pane to pane, a click in a pane makes it the active one, and
    /// Unsplit takes a pane away again.
    #[gpui_kit::test]
    async fn the_editor_splits_into_panes_that_show_and_edit_the_same_file(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use gpui_kit::Modifiers;
        let _home = start(cx);
        let (_folder, root, workbench, cx) = search_project(cx);
        workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("src/alpha.rs"), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        let run = |cx: &mut VisualTestContext, id: &str| {
            cx.update(|window, cx| execute_action_or_throw(id, None, window, cx)).expect("it runs");
            cx.run_until_parked();
        };
        let panes = |cx: &mut VisualTestContext| -> Vec<(bool, Vec<String>)> {
            workbench.read_with(cx, |w, _| {
                let tabs = w.tabs().expect("a project is open");
                let ids: Vec<String> = tabs.pane_ids().into_iter().map(str::to_owned).collect();
                ids.iter()
                    .map(|id| (*id == tabs.active_pane, w.tab_titles_of(id).into_iter().map(|tab| tab.name).collect()))
                    .collect()
            })
        };
        let split = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| crate::platform::context::window_context(window, cx).is_truthy("editorSplit"))
        };
        let first = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
        first.update(cx, |editor, cx| {
            let at = editor.offset_of(2, 4);
            editor.select(at, at, cx);
        });
        assert!(!split(cx));

        run(cx, "editor.splitRight");
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some_and(|editor| *editor != first)));
        assert_eq!(panes(cx), [(false, vec!["alpha.rs".to_owned()]), (true, vec!["alpha.rs".to_owned()])]);
        let second = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("the new pane's editor");
        assert_eq!(second.read_with(cx, |editor, _| editor.caret_position()), (2, 4), "it opens where the first was");
        assert!(cx.update(|window, cx| second.focus_handle(cx).is_focused(window)));
        cx.run_until_parked();
        assert!(split(cx));

        // Both panes are views of one document.
        cx.simulate_input("xyz");
        settle(cx, |cx| first.read_with(cx, |editor, _| editor.text().contains("xyz")));
        assert_eq!(first.read_with(cx, |editor, _| editor.text()), second.read_with(cx, |editor, _| editor.text()));

        // Go to Next Splitter hands the keyboard to the other pane, and round again.
        run(cx, "editor.nextSplitter");
        assert!(cx.update(|window, cx| first.focus_handle(cx).is_focused(window)));
        assert_eq!(panes(cx).iter().map(|pane| pane.0).collect::<Vec<_>>(), [true, false]);
        run(cx, "editor.prevSplitter");
        assert!(cx.update(|window, cx| second.focus_handle(cx).is_focused(window)));

        // A file opened in the active pane moves to the other with Move to Opposite Group.
        workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("src/beta.rs"), window, cx));
        settle(cx, |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path().ends_with("beta.rs")))
        });
        assert_eq!(panes(cx)[1], (true, vec!["alpha.rs".to_owned(), "beta.rs".to_owned()]));
        run(cx, "editor.moveToOppositeGroup");
        assert_eq!(
            panes(cx),
            [(true, vec!["alpha.rs".to_owned(), "beta.rs".to_owned()]), (false, vec!["alpha.rs".to_owned()])]
        );

        // A click on the other pane's tab makes that pane the active one.
        let other_tab = workbench.read_with(cx, |w, _| {
            let tabs = w.tabs().expect("a project is open");
            let pane = tabs.pane(tabs.pane_ids()[1]).expect("the second pane");
            pane.tabs[0].id.clone()
        });
        let tab = cx.debug_bounds(Box::leak(format!("tab:{other_tab}").into_boxed_str())).expect("the tab is drawn");
        cx.simulate_click(tab.center(), Modifiers::default());
        assert_eq!(panes(cx).iter().map(|pane| pane.0).collect::<Vec<_>>(), [false, true]);

        // The splitter turns, and Unsplit closes the active pane.
        let stacked = |cx: &mut VisualTestContext| {
            workbench.read_with(cx, |w, _| matches!(&w.tabs().expect("tabs").layout, LayoutNode::Split { dir, .. } if *dir == SplitDir::Column))
        };
        assert!(!stacked(cx));
        run(cx, "editor.changeSplitOrientation");
        assert!(stacked(cx));
        run(cx, "editor.unsplit");
        assert_eq!(panes(cx), [(true, vec!["alpha.rs".to_owned(), "beta.rs".to_owned()])]);
        cx.run_until_parked();
        assert!(!split(cx));

        // Open in Opposite Group splits when there is no other pane; Unsplit All keeps this one.
        run(cx, "editor.openInOppositeGroup");
        settle(cx, |cx| panes(cx).len() == 2);
        run(cx, "editor.splitDown");
        settle(cx, |cx| panes(cx).len() == 3);
        run(cx, "editor.unsplitAll");
        assert_eq!(panes(cx).len(), 1);
    }

    /// A right click on a tab opens its menu at the pointer. Its rows close tabs around it, pin
    /// it and split the editor; what cannot be done is greyed out; Escape gives the keyboard back.
    #[gpui_kit::test]
    async fn a_tabs_menu_closes_pins_and_splits(cx: &mut TestAppContext) {
        use gpui_kit::{Modifiers, MouseButton};
        let _home = start(cx);
        let (_folder, root, workbench, cx) = search_project(cx);
        for name in ["src/alpha.rs", "src/beta.rs", "notes.txt"] {
            workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join(name), window, cx));
            settle(cx, |cx| {
                workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == root.join(name)))
            });
        }
        let names = |cx: &mut VisualTestContext| -> Vec<String> {
            workbench.read_with(cx, |w, _| w.tab_titles().into_iter().map(|tab| tab.name).collect())
        };
        let tab_id = |cx: &mut VisualTestContext, index: usize| {
            workbench.read_with(cx, |w, _| {
                let tabs = w.tabs().expect("a project is open");
                tabs.pane(&tabs.active_pane).expect("the active pane").tabs[index].id.clone()
            })
        };
        let right_click = |cx: &mut VisualTestContext, index: usize| {
            let id = tab_id(cx, index);
            let tab = cx.debug_bounds(Box::leak(format!("tab:{id}").into_boxed_str())).expect("the tab is drawn");
            cx.simulate_mouse_down(tab.center(), MouseButton::Right, Modifiers::default());
            cx.simulate_mouse_up(tab.center(), MouseButton::Right, Modifiers::default());
            workbench.read_with(cx, |w, _| w.open_menu::<TabCommand>()).expect("the tab's menu is open")
        };
        let labels = |menu: &Entity<PopupMenu<TabCommand>>, cx: &mut VisualTestContext| -> Vec<(String, bool)> {
            menu.read_with(cx, |menu, _| menu.rows().iter().map(|row| (row.label.clone(), row.disabled)).collect())
        };

        // The first tab has nothing to its left; one pane has no opposite group.
        let menu = right_click(cx, 0);
        let rows = labels(&menu, cx);
        assert_eq!(
            rows.iter().map(|(label, _)| label.as_str()).collect::<Vec<_>>(),
            [
                "Close",
                "Close Other Tabs",
                "Close Tabs to the Left",
                "Close Tabs to the Right",
                "Close All Tabs",
                "Close Unmodified Tabs",
                "Pin Tab",
                "Split and Move Right",
                "Split and Move Down",
                "Split Right",
                "Split Down",
            ]
        );
        assert_eq!(
            rows.iter().filter(|(_, disabled)| *disabled).map(|(label, _)| label.as_str()).collect::<Vec<_>>(),
            ["Close Tabs to the Left"]
        );
        assert!(cx.update(|window, cx| menu.focus_handle(cx).is_focused(window)));
        // Escape closes the menu and the editor has the keyboard again.
        cx.simulate_keystrokes("escape");
        assert!(workbench.read_with(cx, |w, _| w.open_menu::<TabCommand>().is_none()));
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
        assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));

        // Close Tabs to the Right, chosen with the keyboard, on the middle tab.
        right_click(cx, 1);
        cx.simulate_keystrokes("down down down enter");
        cx.run_until_parked();
        assert_eq!(names(cx), ["alpha.rs", "beta.rs"]);

        // Pin Tab pins it, and the menu then offers to unpin and to close all but the pinned.
        right_click(cx, 1);
        cx.simulate_keystrokes("p enter");
        cx.run_until_parked();
        let pinned = workbench.read_with(cx, |w, _| {
            let tabs = w.tabs().expect("a project is open");
            tabs.pane(&tabs.active_pane).expect("the active pane").tabs.iter().filter(|tab| tab.pinned).count()
        });
        assert_eq!(pinned, 1);
        let menu = right_click(cx, 0);
        let rows: Vec<String> = labels(&menu, cx).into_iter().map(|(label, _)| label).collect();
        assert!(rows.contains(&"Unpin Tab".to_owned()) && rows.contains(&"Close All but Pinned".to_owned()));
        cx.simulate_keystrokes("escape");

        // Split Right from the menu of a tab that is not the selected one: that tab is split.
        let menu = right_click(cx, 0);
        let first = names(cx)[0].clone();
        menu.update(cx, |_, cx| cx.emit(PopupMenuEvent::Chosen(Choice::Command(TabCommand::SplitRight))));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.tabs().is_some_and(|tabs| tabs.panes.len() == 2)));
        let split: Vec<Vec<String>> = workbench.read_with(cx, |w, _| {
            let tabs = w.tabs().expect("a project is open");
            tabs.pane_ids()
                .into_iter()
                .map(|id| w.tab_titles_of(id).into_iter().map(|tab| tab.name).collect())
                .collect()
        });
        assert_eq!(split[1], [first]);
        // With two panes the menu has the rows for the other group.
        let menu = right_click(cx, 0);
        assert!(labels(&menu, cx).iter().any(|(label, _)| label == "Move to Opposite Group"));
    }

    /// Go to Line puts the caret on the typed line and column. Bookmarks are toggled at the caret
    /// or at a given line, stored with the project, visited in order and by their digit, and
    /// listed in the Bookmarks popup, whose Enter opens one and whose Delete removes it.
    #[gpui_kit::test]
    async fn go_to_line_and_bookmarks_move_the_caret_through_the_project(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        let _home = start(cx);
        let (_folder, root, workbench, cx) = search_project(cx);
        let run = |cx: &mut VisualTestContext, id: &str, arg: Option<serde_json::Value>| {
            cx.update(|window, cx| execute_action_or_throw(id, arg.as_ref(), window, cx)).expect("it runs");
            cx.run_until_parked();
        };
        let at = |cx: &mut VisualTestContext| {
            workbench.read_with(cx, |w, cx| {
                w.editor().map(|editor| (editor.read(cx).path().to_path_buf(), editor.read(cx).caret_position()))
            })
        };
        let marks = |cx: &mut VisualTestContext| -> Vec<(String, u32, Option<char>)> {
            workbench.read_with(cx, |w, _| {
                let strip = |path: &str| path.rsplit('/').next().unwrap_or(path).to_owned();
                w.bookmarks().data().items.iter().map(|b| (strip(&b.path), b.line, b.mnemonic)).collect()
            })
        };
        let (alpha, beta) = (root.join("src/alpha.rs"), root.join("src/beta.rs"));
        workbench.update_in(cx, |w, window, cx| w.open_project_file(alpha.clone(), window, cx));
        settle(cx, |cx| at(cx).is_some());

        run(cx, "navigate.gotoLine", None);
        assert!(workbench.read_with(cx, |w, _| w.popup.is_some()));
        cx.simulate_input("3:5");
        cx.simulate_keystrokes("enter");
        assert_eq!(at(cx), Some((alpha.clone(), (2, 4))));
        assert!(workbench.read_with(cx, |w, _| w.popup.is_none()));

        // A bookmark at the caret, one on a line of a file that is not open, one with a digit.
        run(cx, "navigate.toggleBookmark", None);
        let other = serde_json::json!({ "path": beta.to_string_lossy(), "line": 0 });
        run(cx, "navigate.toggleBookmark", Some(other));
        assert_eq!(marks(cx), [("alpha.rs".to_owned(), 2, None), ("beta.rs".to_owned(), 0, None)]);
        let stored = cx.update(|_, cx| {
            let key = bookmarks_key(&root.to_string_lossy());
            cx.global::<UiStorage>().get(&key).map(str::to_owned)
        });
        assert!(stored.is_some_and(|stored| stored.contains("beta.rs")), "the bookmarks are stored with the project");

        // Next Bookmark goes on in file and line order and round again.
        run(cx, "navigate.nextBookmark", None);
        settle(cx, |cx| at(cx) == Some((beta.clone(), (0, 0))));
        run(cx, "navigate.toggleBookmark7", None);
        assert_eq!(marks(cx)[1], ("beta.rs".to_owned(), 0, Some('7')));
        run(cx, "navigate.nextBookmark", None);
        settle(cx, |cx| at(cx) == Some((alpha.clone(), (2, 0))));
        run(cx, "navigate.gotoBookmark7", None);
        settle(cx, |cx| at(cx) == Some((beta.clone(), (0, 0))));

        // The popup lists them; Delete removes the selected one, Enter opens the one left.
        run(cx, "navigate.showBookmarks", None);
        let popup = workbench
            .read_with(cx, |w, _| {
                w.popup.as_ref().and_then(|popup| popup.view.clone().downcast::<BookmarksPopup>().ok())
            })
            .expect("the Bookmarks popup is open");
        let listed = popup.read_with(cx, |popup, _| popup.lines().join("\n"));
        assert!(listed.contains("alpha.rs") && listed.contains("beta.rs"), "both files are listed:\n{listed}");
        cx.simulate_keystrokes("delete");
        cx.run_until_parked();
        assert_eq!(marks(cx).len(), 1, "what the popup removed is gone from the window's bookmarks");
        let left = marks(cx)[0].clone();
        cx.simulate_keystrokes("down up enter");
        settle(cx, |cx| at(cx).is_some_and(|(path, (line, _))| path.ends_with(&left.0) && line == left.1));
        assert!(workbench.read_with(cx, |w, _| w.popup.is_none()));

        // Toggling where a bookmark is removes it.
        run(cx, "navigate.toggleBookmark", None);
        assert!(marks(cx).is_empty());
    }

    /// Find in Files opens as a popup that searches as the query is typed; Enter opens the
    /// selected match and closes it. Open in Find Window hands the search to the Find tool
    /// window, and Replace in Files opens the same popup in its replace mode.
    #[gpui_kit::test]
    async fn find_in_files_opens_a_popup_that_searches_opens_and_hands_over(cx: &mut TestAppContext) {
        use crate::find::popup_view::SEARCH_DEBOUNCE;
        use crate::find::session::FindStatus;
        use crate::find::view::FindPanel;
        use crate::platform::actions::execute_action_or_throw;
        let _home = start(cx);
        let (_folder, root, workbench, cx) = search_project(cx);
        let run = |cx: &mut VisualTestContext, id: &str| {
            cx.update(|window, cx| execute_action_or_throw(id, None, window, cx)).expect("it runs");
            cx.run_until_parked();
        };
        let found = |popup: &Entity<FindPopup>, cx: &mut VisualTestContext| {
            for _ in 0..300 {
                cx.executor().advance_clock(SEARCH_DEBOUNCE);
                cx.run_until_parked();
                if popup.read_with(cx, |popup, _| popup.status() == FindStatus::Done) {
                    return popup.read_with(cx, |popup, _| popup.lines());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("the search did not finish within three seconds");
        };

        run(cx, "find.inPath");
        let popup = workbench.read_with(cx, |w, _| w.find_popup()).expect("Find in Files is open");
        assert!(!popup.read_with(cx, |popup, _| popup.is_replace()));
        assert!(cx.update(|window, cx| popup.focus_handle(cx).is_focused(window)), "the query has the keyboard");
        assert!(
            cx.update(|window, cx| crate::platform::context::window_context(window, cx).is_truthy("findPopupOpen"))
        );
        cx.simulate_input("needle");
        let lines = found(&popup, cx);
        assert_eq!(lines.len(), 1, "one line matches: {lines:?}");
        assert!(lines[0].contains("notes.txt") && lines[0].contains("a needle here"), "{lines:?}");
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| {
            workbench.read_with(cx, |w, cx| {
                w.editor().is_some_and(|e| {
                    e.read(cx).path() == root.join("notes.txt") && e.read(cx).caret_position() == (1, 4)
                })
            })
        });
        assert!(workbench.read_with(cx, |w, _| w.find_popup().is_none()));
        cx.run_until_parked();
        assert!(
            !cx.update(|window, cx| crate::platform::context::window_context(window, cx).is_truthy("findPopupOpen"))
        );

        // Open in Find Window: the popup goes and the tool window runs the same search.
        run(cx, "find.inPath");
        let popup = workbench.read_with(cx, |w, _| w.find_popup()).expect("Find in Files is open");
        popup.update(cx, |popup, cx| popup.set_pattern("widget", cx));
        found(&popup, cx);
        popup.update(cx, |popup, cx| popup.open_in_find_window(cx));
        cx.run_until_parked();
        assert!(workbench.read_with(cx, |w, _| w.find_popup().is_none() && w.is_tool_window_visible(FIND_TOOL_WINDOW)));
        let panel = workbench
            .read_with(cx, |w, _| w.tool_window_view(FIND_TOOL_WINDOW))
            .and_then(|view| view.downcast::<FindPanel>().ok())
            .expect("the Find window holds the panel");
        settle(cx, |cx| panel.read_with(cx, |panel, _| panel.status() == FindStatus::Done));
        assert_eq!(panel.read_with(cx, |panel, cx| panel.field().read(cx).text().to_owned()), "widget");
        assert_eq!(
            panel.read_with(cx, |panel, _| panel.results().into_iter().map(|(file, _)| file).collect::<Vec<_>>()),
            ["src/alpha.rs"]
        );

        // Replace in Files is the same popup with a replacement; Find in Files asked for while
        // it is open turns that popup back.
        run(cx, "find.replaceInPath");
        let popup = workbench.read_with(cx, |w, _| w.find_popup()).expect("Replace in Files is open");
        assert!(popup.read_with(cx, |popup, _| popup.is_replace()));
        run(cx, "find.inPath");
        assert_eq!(workbench.read_with(cx, |w, _| w.find_popup()), Some(popup.clone()));
        assert!(!popup.read_with(cx, |popup, _| popup.is_replace()));
    }

    /// Find opens a bar above the file's editor with the keyboard in it; typing selects the first
    /// occurrence, Enter and Find Next go on to the next ones, Replace shows the replacement and replaces
    /// the selected occurrence, and Escape closes the bar with the keyboard back in the editor. The bar
    /// belongs to its tab: another tab has none.
    #[gpui_kit::test]
    async fn find_opens_a_bar_over_the_tabs_editor_that_searches_replaces_and_closes(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use crate::platform::context::window_context;
        let _home = start(cx);
        let (_folder, root, workbench, cx) = search_project(cx);
        let run = |cx: &mut VisualTestContext, id: &str| {
            cx.update(|window, cx| execute_action_or_throw(id, None, window, cx)).expect("it runs");
            cx.run_until_parked();
        };
        let key =
            |cx: &mut VisualTestContext, key: &str| cx.update(|window, cx| window_context(window, cx).is_truthy(key));
        let open = |cx: &mut VisualTestContext, name: &str| {
            let path = root.join(name);
            workbench.update_in(cx, |w, window, cx| w.open_project_file(path.clone(), window, cx));
            settle(cx, |cx| {
                workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == path))
                    && cx.update(|window, cx| {
                        workbench.read(cx).editor().is_some_and(|e| e.focus_handle(cx).is_focused(window))
                    })
            });
            workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor")
        };
        let editor = open(cx, "src/alpha.rs");
        let selection = |cx: &mut VisualTestContext| editor.read_with(cx, |e, _| e.selection());

        run(cx, "find.inFile");
        let bar = workbench.read_with(cx, |w, _| w.find_bar()).expect("the bar is open");
        assert!(!bar.read_with(cx, |bar, _| bar.is_replace()));
        assert!(cx.update(|window, cx| bar.read(cx).find_field().focus_handle(cx).is_focused(window)));
        assert!(key(cx, "findBarOpen") && key(cx, "findBarFocus"));

        // "pub struct Widget;\n\npub fn make_widget() -> Widget {\n    Widget\n}\n"
        cx.simulate_input("widget");
        cx.run_until_parked();
        assert_eq!(selection(cx), 11..17);
        assert_eq!(bar.read_with(cx, |bar, _| bar.match_count()), 4);
        assert!(key(cx, "findBarHasResults"));
        cx.simulate_keystrokes("enter");
        assert_eq!(selection(cx), 32..38);
        run(cx, "find.next");
        assert_eq!(selection(cx), 44..50);
        run(cx, "find.previous");
        assert_eq!(selection(cx), 32..38);
        run(cx, "find.next");

        // Replace turns the same bar into the replace bar; Replace changes the selected occurrence.
        run(cx, "find.replaceInFile");
        assert_eq!(workbench.read_with(cx, |w, _| w.find_bar()), Some(bar.clone()));
        assert!(bar.read_with(cx, |bar, _| bar.is_replace()));
        bar.update(cx, |bar, cx| bar.set_replacement("gadget", cx));
        bar.update(cx, FindBar::replace_one);
        cx.run_until_parked();
        assert_eq!(
            editor.read_with(cx, |e, _| e.text()),
            "pub struct Widget;\n\npub fn make_widget() -> gadget {\n    Widget\n}\n"
        );

        // Another tab has no bar; coming back finds this one as it was.
        open(cx, "notes.txt");
        assert_eq!(workbench.read_with(cx, |w, _| w.find_bar()), None);
        assert!(!key(cx, "findBarOpen"));
        open(cx, "src/alpha.rs");
        assert_eq!(workbench.read_with(cx, |w, _| w.find_bar()), Some(bar.clone()));

        // Escape in the bar closes it and gives the editor the keyboard.
        run(cx, "find.inFile");
        assert!(cx.update(|window, cx| bar.read(cx).has_focus(window, cx)));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(workbench.read_with(cx, |w, _| w.find_bar()), None);
        assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
        assert!(!key(cx, "findBarOpen") && !key(cx, "findBarFocus"));

        // Open Results in Find Window hands the bar's search of its file to the Find tool window.
        run(cx, "find.inFile");
        let bar = workbench.read_with(cx, |w, _| w.find_bar()).expect("the bar is open");
        bar.update(cx, |bar, cx| bar.set_search("widget", cx));
        cx.run_until_parked();
        run(cx, "find.openInWindow");
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(FIND_TOOL_WINDOW)));
        let panel = workbench
            .read_with(cx, |w, _| w.tool_window_view(FIND_TOOL_WINDOW))
            .and_then(|view| view.downcast::<crate::find::view::FindPanel>().ok())
            .expect("the Find window holds the panel");
        settle(cx, |cx| panel.read_with(cx, |panel, _| panel.status() == crate::find::session::FindStatus::Done));
        assert_eq!(panel.read_with(cx, |panel, _| panel.results()), [("src/alpha.rs".to_owned(), vec![0, 2, 3])]);
    }

    /// The status bar's items answer a click: the position opens Go to Line, the line separator
    /// and the encoding open their menus, whose rows change the file. What the backend refuses
    /// is told in a notification.
    #[gpui_kit::test]
    async fn the_status_bars_items_open_go_to_line_and_their_menus(cx: &mut TestAppContext) {
        use gpui_kit::Modifiers;
        let _home = start(cx);
        let (_folder, root, workbench, cx) = search_project(cx);
        workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("notes.txt"), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
        let click = |cx: &mut VisualTestContext, item: &'static str| {
            cx.run_until_parked();
            let at = cx.debug_bounds(item).expect("the status bar item is drawn").center();
            cx.simulate_click(at, Modifiers::default());
        };
        let items = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, cx| w.status_items(cx));
        let menu = |cx: &mut VisualTestContext| {
            workbench.read_with(cx, |w, _| w.open_menu::<StatusCommand>()).expect("the item's menu is open")
        };
        assert_eq!(items(cx), ["1:1", "LF", "UTF-8"]);

        click(cx, "status:editor.caret");
        cx.simulate_input("2");
        cx.simulate_keystrokes("enter");
        assert_eq!(items(cx)[0], "2:1");

        // The file's own separator is marked; choosing another changes it and the file is unsaved.
        click(cx, "status:editor.lineEnding");
        let rows: Vec<(String, bool)> = menu(cx)
            .read_with(cx, |menu, _| menu.rows().iter().map(|row| (row.label.clone(), row.icon.is_some())).collect());
        assert_eq!(rows.iter().map(|(_, marked)| *marked).collect::<Vec<_>>(), [true, false, false]);
        assert!(rows[1].0.starts_with("CRLF"));
        cx.simulate_keystrokes("down enter");
        assert_eq!(items(cx)[1], "CRLF");
        assert!(editor.read_with(cx, |editor, _| editor.is_dirty()));

        // With unsaved changes the file cannot be reloaded in another encoding, but converted.
        click(cx, "status:editor.encoding");
        let rows: Vec<(String, bool)> =
            menu(cx).read_with(cx, |menu, _| menu.rows().iter().map(|row| (row.label.clone(), row.disabled)).collect());
        assert_eq!(rows[..2], [("Reload with Encoding".to_owned(), true), ("Convert to Encoding".to_owned(), false)]);
        // The first row cannot be chosen, so the selection starts on Convert; its list leaves out
        // the file's own encoding.
        cx.simulate_keystrokes("right");
        let offered: Vec<String> =
            menu(cx).read_with(cx, |menu, _| menu.rows().iter().map(|row| row.label.clone()).collect());
        assert!(!offered.contains(&"UTF-8".to_owned()) && offered[0] == "UTF-16LE", "{offered:?}");
        cx.simulate_keystrokes("enter");
        assert_eq!(items(cx)[2], "UTF-16LE");

        // A refusal by the backend is told.
        workbench.update(cx, |w, cx| w.run_status_command(StatusCommand::Reload("gbk".to_owned()), cx));
        let told = workbench.read_with(cx, |w, _| format!("{:?}", w.notifications()));
        assert!(told.contains("Cannot reload with encoding"), "{told}");
        assert_eq!(items(cx)[2], "UTF-16LE");
    }

    /// A raised notification shows as a balloon; closing the balloon keeps it in the history.
    /// The Notifications tool window lists the history, hides the balloons and marks everything
    /// read; Clear All empties it. "Don't Show Again" turns a group's balloons off in the settings.
    #[gpui_kit::test]
    async fn notifications_show_as_balloons_and_in_their_tool_window(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use gpui_kit::Modifiers;
        let _home = start(cx);
        let (_folder, _root, workbench, cx) = search_project(cx);
        let raise = |cx: &mut VisualTestContext, level: NotificationLevel, title: &str| {
            workbench.update(cx, |w, cx| w.notify(NotificationRequest::new(level, title), cx));
            cx.run_until_parked();
        };
        let balloons = |cx: &mut VisualTestContext| -> Vec<String> {
            workbench
                .read_with(cx, |w, cx| w.balloons.read(cx).shown().iter().map(|entry| entry.title.clone()).collect())
        };
        let click = |cx: &mut VisualTestContext, selector: String| {
            cx.run_until_parked();
            let at = cx.debug_bounds(Box::leak(selector.into_boxed_str())).expect("it is drawn").center();
            cx.simulate_click(at, Modifiers::default());
            cx.run_until_parked();
        };

        raise(cx, NotificationLevel::Warning, "Disk is almost full");
        raise(cx, NotificationLevel::Error, "Build failed");
        assert_eq!(balloons(cx), ["Disk is almost full", "Build failed"]);
        assert_eq!(workbench.read_with(cx, |w, _| w.notifications().unread_count()), 2);

        // The close button takes the balloon away; the notification stays in the history.
        let first = workbench.read_with(cx, |w, cx| w.balloons.read(cx).shown()[0].id);
        click(cx, format!("balloon-{first}-close"));
        assert_eq!(balloons(cx), ["Build failed"]);
        assert_eq!(workbench.read_with(cx, |w, _| w.notifications().items().len()), 2);

        // The tool window lists both, newest first; while it shows there are no balloons and
        // nothing is unread.
        cx.update(|window, cx| execute_action_or_throw("view.notifications", None, window, cx)).expect("it runs");
        cx.run_until_parked();
        let list = workbench
            .read_with(cx, |w, _| w.tool_window_view(NOTIFICATIONS_TOOL_WINDOW))
            .and_then(|view| view.downcast::<NotificationsView>().ok())
            .expect("the Notifications window holds the list");
        let listed: Vec<String> =
            list.read_with(cx, |list, _| list.entries().iter().map(|entry| entry.title.clone()).collect());
        assert_eq!(listed, ["Build failed", "Disk is almost full"]);
        assert!(balloons(cx).is_empty());
        assert_eq!(workbench.read_with(cx, |w, _| w.notifications().unread_count()), 0);

        click(cx, "notifications-clear-all".to_owned());
        assert!(list.read_with(cx, |list, _| list.entries().is_empty()));
        assert!(workbench.read_with(cx, |w, _| w.notifications().items().is_empty()));

        // With the window hidden again a new notification is a balloon; its Don't Show Again
        // stores the choice, and the next one of its group does not pop up.
        cx.update(|window, cx| execute_action_or_throw("toolwindow.notifications", None, window, cx)).expect("it runs");
        cx.run_until_parked();
        assert!(!workbench.read_with(cx, |w, _| w.is_tool_window_visible(NOTIFICATIONS_TOOL_WINDOW)));
        raise(cx, NotificationLevel::Info, "Indexing finished");
        assert_eq!(balloons(cx), ["Indexing finished"]);
        let entry = workbench.read_with(cx, |w, cx| w.balloons.read(cx).shown()[0].clone());
        let dont_show = entry
            .actions
            .iter()
            .position(|action| matches!(action, EntryAction::DontShowAgain { .. }))
            .expect("an information balloon offers it");
        click(cx, format!("balloon-{}-action-{dont_show}", entry.id));
        assert!(balloons(cx).is_empty());
        raise(cx, NotificationLevel::Info, "Indexing finished again");
        assert!(balloons(cx).is_empty(), "the group's balloons are off");
        assert_eq!(workbench.read_with(cx, |w, _| w.notifications().items().len()), 2, "they are still recorded");
    }

    /// A window with nothing open shows the Welcome screen. A project that was opened and closed
    /// is listed there; Enter or a click on its row opens it again, and removing it from the list
    /// forgets it.
    #[gpui_kit::test]
    async fn a_window_with_nothing_open_welcomes_with_the_recent_projects(cx: &mut TestAppContext) {
        use gpui_kit::Modifiers;
        use welcome_model::WelcomeCommand;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        let name = root.file_name().expect("a folder name").to_string_lossy().into_owned();
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        let welcome = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.welcome().cloned());
        let names = |cx: &mut VisualTestContext| -> Vec<String> {
            let view = welcome(cx).expect("the Welcome screen shows");
            view.read_with(cx, |view, _| view.shown_projects().into_iter().map(|project| project.name).collect())
        };
        let close = |cx: &mut VisualTestContext| {
            workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
            cx.run_until_parked();
        };
        let opened = |cx: &mut VisualTestContext| {
            settle(cx, |cx| workbench.read_with(cx, |w, cx| w.project_root(cx).as_deref() == Some(&*root)));
            assert!(welcome(cx).is_none(), "a project replaces the Welcome screen");
        };

        // Nothing has been opened yet: the page that offers to open one.
        cx.run_until_parked();
        let first = welcome(cx).expect("the Welcome screen shows");
        assert_eq!(first.read_with(cx, |view, _| view.empty_text()), Some("Welcome to OpenIDEA"));
        assert_eq!(
            first.read_with(cx, |view, _| view.version().map(str::to_owned)),
            Some(env!("CARGO_PKG_VERSION").into())
        );
        assert!(cx.debug_bounds("welcome-brand").is_some(), "the Welcome screen is drawn");

        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        opened(cx);

        // Closed, the project is the recent one; the keyboard is in the search, and Enter opens it.
        close(cx);
        assert_eq!(names(cx), [name]);
        let field = welcome(cx).expect("the Welcome screen shows").read_with(cx, |view, _| view.field().clone());
        let searching = |cx: &mut VisualTestContext| cx.update(|window, cx| field.focus_handle(cx).is_focused(window));
        assert!(searching(cx));
        // Recent Projects puts the keyboard back in the search.
        cx.update(|window, cx| window.blur(cx));
        assert!(!searching(cx));
        cx.update(|window, cx| {
            crate::platform::actions::execute_action_or_throw("welcome.recentProjects", None, window, cx)
        })
        .expect("it runs");
        assert!(searching(cx));
        cx.simulate_keystrokes("enter");
        opened(cx);

        // A click on its row opens it too.
        close(cx);
        let row = cx.debug_bounds("welcome-row-0").expect("the row is drawn").center();
        cx.simulate_click(row, Modifiers::default());
        opened(cx);

        // Removing it from the list, once confirmed, forgets it.
        close(cx);
        let view = welcome(cx).expect("the Welcome screen shows");
        let path = root.to_string_lossy().into_owned();
        view.update_in(cx, |view, window, cx| view.run(WelcomeCommand::AskRemove(path), window, cx));
        cx.simulate_prompt_answer("Remove");
        cx.run_until_parked();
        assert!(names(cx).is_empty());
        let config = cx.update(|_, cx| cx.global::<Backend>().dirs.config.clone());
        assert!(ide_workspace::RecentProjects::load(&config).list().is_empty(), "the project is forgotten");
        assert_eq!(
            welcome(cx).map(|view| view.read_with(cx, |view, _| view.empty_text())),
            Some(Some("Welcome to OpenIDEA"))
        );
    }

    /// Run › Run starts the selected configuration of the project: its output shows in the Run tool
    /// window and the session ends with the process's exit code; Run › Stop ends a long-running one.
    #[gpui_kit::test]
    async fn a_run_configuration_runs_into_the_run_tool_window_and_stop_ends_a_long_running_one(
        cx: &mut TestAppContext,
    ) {
        use crate::platform::actions::execute_action_or_throw;
        use crate::run::configs::blank_config;
        use crate::run::store::SessionStatus;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        let program = |id: &str, command: &str, arg: &str| ide_run::RunConfig {
            command: command.to_owned(),
            args: vec![arg.to_owned()],
            ..blank_config(id, id, "program")
        };
        let configs = [program("hello", "/bin/echo", "hello"), program("nap", "/bin/sleep", "30")];
        ide_run::save_user_configs(&root, &configs).expect("the configurations are saved");
        cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let driver = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("the project runs things");
        let store = driver.read_with(cx, |driver, _| driver.store().clone());
        // Processes and their timers: real time passes, and so does the executor's clock.
        let long = |cx: &mut VisualTestContext, what: &str, done: &dyn Fn(&mut VisualTestContext) -> bool| {
            for _ in 0..3000 {
                cx.executor().advance_clock(Duration::from_millis(20));
                cx.run_until_parked();
                if done(cx) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("{what} did not happen within thirty seconds");
        };
        long(cx, "listing the configurations", &|cx| store.read_with(cx, |store, _| store.loaded));
        let listed: Vec<String> =
            store.read_with(cx, |store, _| store.configs().iter().map(|c| c.id.clone()).collect());
        assert_eq!(listed, ["hello", "nap"]);
        assert!(!workbench.read_with(cx, |w, _| w.is_tool_window_visible(RUN_TOOL_WINDOW)));

        store.update(cx, |store, _| store.select_config(Some("hello")));
        cx.update(|window, cx| execute_action_or_throw("run.run", None, window, cx)).expect("it runs");
        let session = |cx: &mut VisualTestContext, name: &str| {
            store.read_with(cx, |store, _| store.sessions.iter().find(|s| s.config.name == name).cloned())
        };
        long(cx, "the exit of echo", &|cx| session(cx, "hello").is_some_and(|s| !s.is_live()));
        let hello = session(cx, "hello").expect("a tab");
        assert_eq!((hello.status, hello.exit_code, hello.stopped), (SessionStatus::Finished, Some(0), false));
        // Settings › Run › Show the Run tool window when a process starts.
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(RUN_TOOL_WINDOW)));
        let view = workbench
            .read_with(cx, |w, _| w.tool_window_view(RUN_TOOL_WINDOW))
            .and_then(|view| view.downcast::<RunView>().ok())
            .expect("the Run tool window holds its view");
        assert_eq!(view.read_with(cx, |view, cx| view.tab_titles(cx)), ["hello"]);
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert!(lines.iter().any(|line| line == "hello"), "{lines:?}");
        assert_eq!(view.read_with(cx, |view, cx| view.tab_icons(cx)), ["file-binary"]);

        // With the keyboard in the console's find bar the Run tool window is the active one: Rerun
        // restarts its tab.
        cx.update(|window, cx| {
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("cmd-f");
        workbench.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.find_open()));
        let active = cx.update(|window, cx| {
            let context = crate::platform::context::window_context(window, cx);
            matches!(context.get("activeToolWindow"), Some(crate::platform::context::ContextValue::Text(id)) if &**id == "run")
        });
        assert!(active, "the Run tool window is the active one");
        cx.update(|window, cx| execute_action_or_throw("run.rerun", None, window, cx)).expect("it reruns");
        let first = hello.started_at;
        long(cx, "the rerun of echo", &|cx| {
            session(cx, "hello").is_some_and(|s| s.started_at > first && s.status == SessionStatus::Finished)
        });
        assert_eq!(view.read_with(cx, |view, cx| view.tab_titles(cx)), ["hello"], "the same tab");
        cx.simulate_keystrokes("escape");

        store.update(cx, |store, _| store.select_config(Some("nap")));
        cx.update(|window, cx| execute_action_or_throw("run.run", None, window, cx)).expect("it runs");
        long(cx, "the start of sleep", &|cx| session(cx, "nap").is_some_and(|s| s.status == SessionStatus::Running));
        assert_eq!(view.read_with(cx, |view, cx| view.tab_icons(cx)), ["file-binary", "run"]);
        // Outside the Run tool window Stop reaches the only running process.
        cx.update(|window, cx| execute_action_or_throw("run.stop", None, window, cx)).expect("it runs");
        long(cx, "the end of sleep", &|cx| session(cx, "nap").is_some_and(|s| !s.is_live()));
        let nap = session(cx, "nap").expect("a tab");
        assert_eq!((nap.status, nap.stopped), (SessionStatus::Finished, true));
        assert_eq!(view.read_with(cx, |view, cx| view.tab_icons(cx)), ["file-binary", "file-binary"]);
    }

    #[test]
    fn titles_name_the_project_then_the_file() {
        let file = std::path::Path::new("/work/shop/src/main.rs");
        assert_eq!(window_title(Some("shop"), Some(file), false, None, "OpenIDEA"), "shop – main.rs");
        assert_eq!(window_title(Some("shop"), Some(file), true, None, "OpenIDEA"), "shop – /work/shop/src/main.rs");
        assert_eq!(window_title(Some("shop"), None, false, Some("me@box"), "OpenIDEA"), "shop [me@box]");
        assert_eq!(
            window_title(Some("shop"), Some(file), false, Some("me@box"), "OpenIDEA"),
            "shop [me@box] – main.rs"
        );
        assert_eq!(window_title(None, Some(file), false, None, "OpenIDEA"), "main.rs");
        assert_eq!(window_title(None, None, false, None, "OpenIDEA"), "OpenIDEA");
        assert_eq!(window_title(Some(""), None, false, None, "OpenIDEA"), "OpenIDEA");
    }

    #[gpui_kit::test]
    async fn a_file_that_cannot_open_is_a_message_in_the_window(cx: &mut TestAppContext) {
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        // A folder is not a file to edit.
        workbench.update_in(cx, |workbench, window, cx| workbench.open_file(folder.path().to_path_buf(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some() || w.failure().is_some()));
        workbench.read_with(cx, |w, _| {
            assert!(w.editor().is_none());
            assert!(w.failure().is_some_and(|message| message.contains("is a folder")), "{:?}", w.failure());
        });
    }
}
