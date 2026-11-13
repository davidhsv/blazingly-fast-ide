//! Editor tabs that hold a view other than a text editor, and the Markdown layouts around a text
//! editor.
//!
//! A tab's input names its editor kind. `file` tabs get a text [`Editor`]; the kinds of
//! [`TAB_KINDS`] get a [`TabView`] built from the input by the kind's constructor: when the tab is
//! first shown, when the session restores it, when it opens. A view tab has no text editor, so
//! every text action is off there (`textEditorActive` is false) and its kind's context keys
//! (`imageViewerActive`, `imageViewerFocus`) say what is active instead.
//!
//! Opening a file asks the viewers first, as IDEA's file editor providers do: an image opens in the
//! image viewer; any other file not open as text yet is probed, and binary or very large content
//! turns its tab into a read-only hex view before a document is ever opened. Open as Text skips
//! the viewers. Markdown files stay text tabs: their editor is drawn inside a [`PreviewHost`]
//! with the Editor Only / Editor and Preview / Preview Only layouts in the tab strip, its preview
//! fed from the editor's edits once typing pauses ([`TEXT_DELAY_MS`]).

use super::{Content, Workbench, active_editor, for_window};
use crate::backend::Backend;
use crate::editor::tabs::{EditorInput, OpenTabOptions, Tab, TabsState};
use crate::editor::view::{Editor, EditorEvent};
use crate::features_b::viewers::file_kinds::{
    BINARY_KIND, IMAGE_ACTIONS, IMAGE_KIND, binary_input, image_viewer_for, is_markdown_path, wants_hex_view,
};
use crate::features_b::viewers::hex::PAGE_SIZE;
use crate::features_b::viewers::hex_view::{HexView, HexViewEvent};
use crate::features_b::viewers::image_view::{ImageView, ImageViewEvent, chessboard};
use crate::features_b::viewers::markdown_view::MarkdownViewEvent;
use crate::features_b::viewers::preview::{
    LiveSources, MODE_ACTIONS, MarkdownMode, MarkdownModes, PreviewImages, TEXT_DELAY_MS, markdown_preview_font_size,
};
use crate::features_b::viewers::preview_host::PreviewHost;
use crate::platform::actions::ActionRegistry;
use crate::platform::storage::UiStorage;
use crate::theme;
use gpui_kit::{
    AnyView, App, Context, Entity, FocusHandle, Focusable, Global, Render, Subscription, Task, WeakEntity, Window,
    prelude::*,
};
use ide_core::IdeResult;
use ide_host::Host;
use ide_workspace::DocumentInfo;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The editor kind of text documents.
pub const FILE_KIND: &str = "file";

/// How many bytes one read of the hex view asks for: one page.
const PAGE_LEN: u32 = PAGE_SIZE as u32;

/// Whether a view tab holds changes that closing it would lose.
pub type DirtyProbe = Rc<dyn Fn(&App) -> bool>;
/// Asked before a dirty view tab goes: resolves to whether it may close. A tab that may not comes
/// back with its view.
pub type CloseGuard = Rc<dyn Fn(&mut Window, &mut App) -> Task<bool>>;
/// Runs one of the view's actions; `false` when it is not one of them.
pub type RunAction = Rc<dyn Fn(&str, &mut App) -> bool>;
/// Builds the view of a tab from its input; `None` for an input it cannot show.
pub type BuildView = fn(&EditorInput, &mut Window, &mut Context<Workbench>) -> Option<TabView>;

/// The view of a tab that is not a text editor.
pub struct TabView {
    /// The editor kind ([`TabKind::id`]).
    pub kind: &'static str,
    /// What the tab shows; a tab that comes back after a refused close reopens with it.
    pub input: EditorInput,
    pub view: AnyView,
    /// Where the keyboard goes when the tab is shown.
    pub focus: FocusHandle,
    pub title: String,
    pub icon: String,
    /// Unset: the view never holds unsaved changes, and closing it asks nothing.
    pub dirty: Option<DirtyProbe>,
    pub close_guard: Option<CloseGuard>,
    /// The actions the view answers while its tab is active, and what runs them.
    pub actions: &'static [&'static str],
    pub run_action: Option<RunAction>,
    /// What the workbench listens to the view for, kept for as long as the view is.
    _subscriptions: Vec<Subscription>,
}

impl TabView {
    /// The tab view `view` of `input`, titled and iconed as its kind says.
    pub fn new<V: Focusable + Render>(kind: &'static str, input: EditorInput, view: &Entity<V>, cx: &App) -> Self {
        Self {
            kind,
            title: title_of(&input).unwrap_or_else(|| base_title(&input)),
            icon: icon_of(&input),
            input,
            view: view.clone().into(),
            focus: view.focus_handle(cx),
            dirty: None,
            close_guard: None,
            actions: &[],
            run_action: None,
            _subscriptions: Vec::new(),
        }
    }

    /// The view answers `actions` while active, through `run`.
    pub fn with_actions(
        mut self,
        actions: &'static [&'static str],
        run: impl Fn(&str, &mut App) -> bool + 'static,
    ) -> Self {
        self.actions = actions;
        self.run_action = Some(Rc::new(run));
        self
    }

    /// Keeps `subscription` for as long as the view lives.
    pub fn with_subscription(mut self, subscription: Subscription) -> Self {
        self._subscriptions.push(subscription);
        self
    }

    pub fn is_dirty(&self, cx: &App) -> bool {
        self.dirty.as_ref().is_some_and(|dirty| dirty(cx))
    }

    /// The view as the entity it is, when it is a `V`.
    pub fn downcast<V: 'static>(&self) -> Option<Entity<V>> {
        self.view.clone().downcast().ok()
    }
}

/// What a tab holds once it has been shown: a text editor or another view.
#[derive(Clone, Copy)]
pub enum TabContent<'a> {
    Editor(&'a Entity<Editor>),
    View(&'a TabView),
}

/// An editor kind other than `file`: how its tabs are titled, whether the session restores them,
/// the context keys it publishes while active and focused, and how its view is built.
pub struct TabKind {
    pub id: &'static str,
    pub restorable: bool,
    pub active_key: Option<&'static str>,
    pub focus_key: Option<&'static str>,
    pub title: fn(&EditorInput) -> String,
    pub icon: fn(&EditorInput) -> String,
    pub build: BuildView,
}

/// The editor kinds whose tabs hold a [`TabView`]. A feature adds its kind here.
pub const TAB_KINDS: &[TabKind] = &[
    TabKind {
        id: IMAGE_KIND,
        restorable: false,
        active_key: Some("imageViewerActive"),
        focus_key: Some("imageViewerFocus"),
        title: base_title,
        icon: base_icon,
        build: build_image,
    },
    TabKind {
        id: BINARY_KIND,
        restorable: false,
        active_key: None,
        focus_key: None,
        title: base_title,
        icon: base_icon,
        build: build_hex,
    },
    TabKind {
        id: crate::plugin_views::log_tab::PLUGIN_LOG_KIND,
        restorable: false,
        active_key: None,
        focus_key: None,
        title: crate::plugin_views::log_tab::log_tab_title,
        icon: super::plugin_runtime::log_tab_icon,
        build: super::plugin_runtime::build_plugin_log_tab,
    },
];

pub fn kind(id: &str) -> Option<&'static TabKind> {
    TAB_KINDS.iter().find(|kind| kind.id == id)
}

/// Whether the session saves a tab that is not a file tab: its kind asks for it.
pub fn restorable(input: &EditorInput) -> bool {
    kind(&input.kind).is_some_and(|kind| kind.restorable)
}

/// The title of a tab of a registered kind; `None` for text tabs, titled by their file name.
pub fn title_of(input: &EditorInput) -> Option<String> {
    kind(&input.kind).map(|kind| (kind.title)(input))
}

/// The icon of a tab: its kind's, or that of its file name.
pub fn icon_of(input: &EditorInput) -> String {
    match kind(&input.kind) {
        Some(kind) => (kind.icon)(input),
        None => base_icon(input),
    }
}

/// The file name of the input's path, or of its key when it has none.
fn base_title(input: &EditorInput) -> String {
    let path = input.path.as_deref().unwrap_or(&input.key);
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

fn base_icon(input: &EditorInput) -> String {
    theme::icons::file_icon_name(&base_title(input)).to_owned()
}

/// The key of the hex view's tab of `path` ([`binary_input`]).
fn binary_key(path: &str) -> String {
    format!("{BINARY_KIND}:{path}")
}

/// A backend answer as the views take it: the value, or the message.
fn flatten<T>(result: anyhow::Result<IdeResult<T>>) -> Result<T, String> {
    result.map_err(|error| error.to_string()).and_then(|answer| answer.map_err(|error| error.message))
}

fn tab_mut<'a>(tabs: &'a mut TabsState, tab_id: &str) -> Option<&'a mut Tab> {
    tabs.panes.iter_mut().flat_map(|pane| pane.tabs.iter_mut()).find(|tab| tab.id == tab_id)
}

fn pane_of(tabs: &TabsState, tab_id: &str) -> Option<String> {
    tabs.panes.iter().find(|pane| pane.tab(tab_id).is_some()).map(|pane| pane.id.clone())
}

/// The view tabs of a project window: the views of the tabs shown so far, and the Markdown hosts
/// of text tabs.
#[derive(Default)]
pub struct ViewTabs {
    views: HashMap<String, TabView>,
    markdown: HashMap<String, MarkdownTab>,
}

impl ViewTabs {
    /// Forgets what the tab `tab_id` showed besides its text editor: its view, its Markdown host.
    pub fn drop_tab(&mut self, tab_id: &str) {
        self.views.remove(tab_id);
        self.markdown.remove(tab_id);
    }

    /// The view tabs that hold unsaved changes.
    pub fn dirty_tabs(&self, cx: &App) -> Vec<String> {
        self.views.iter().filter(|(_, view)| view.is_dirty(cx)).map(|(id, _)| id.clone()).collect()
    }
}

/// The Markdown layouts around a text tab's editor and what feeds its preview.
struct MarkdownTab {
    host: Entity<PreviewHost>,
    feed: Entity<MarkdownFeed>,
    _events: Subscription,
}

/// What every Markdown editor of the app shares: the layout of each file, the live state the
/// editors publish and the local images the previews show.
#[derive(Clone)]
struct MarkdownShared {
    modes: Entity<MarkdownModes>,
    live: Entity<LiveSources>,
    images: Entity<PreviewImages>,
}

impl Global for MarkdownShared {}

fn markdown_shared(cx: &mut App) -> MarkdownShared {
    if let Some(shared) = cx.try_global::<MarkdownShared>() {
        return shared.clone();
    }
    let modes = MarkdownModes::load(cx.default_global::<UiStorage>());
    let shared = MarkdownShared {
        modes: cx.new(|_| modes),
        live: cx.new(|_| LiveSources::default()),
        images: cx.new(|_| PreviewImages::default()),
    };
    cx.set_global(shared.clone());
    shared
}

/// Publishes a Markdown editor's text, caret moves and scrolling to [`LiveSources`]. Edits wait
/// for typing to pause; the clock is the executor's, so tests move it.
struct MarkdownFeed {
    path: String,
    live: Entity<LiveSources>,
    editor: WeakEntity<Editor>,
    origin: Instant,
    top_line: u32,
    /// The pending publication of the latest edit; a newer edit replaces it.
    _publish: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl MarkdownFeed {
    fn new(path: String, editor: &Entity<Editor>, live: Entity<LiveSources>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(editor, |feed, editor, event: &EditorEvent, cx| feed.editor_event(&editor, event, cx)),
            cx.observe(editor, |feed, editor, cx| feed.editor_changed(&editor, cx)),
        ];
        let top_line = editor.read(cx).top_line(cx);
        Self {
            path,
            live,
            editor: editor.downgrade(),
            origin: cx.background_executor().now(),
            top_line,
            _publish: None,
            _subscriptions: subscriptions,
        }
    }

    fn now_ms(&self, cx: &App) -> u64 {
        let elapsed = cx.background_executor().now().saturating_duration_since(self.origin);
        u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
    }

    fn editor_event(&mut self, editor: &Entity<Editor>, event: &EditorEvent, cx: &mut Context<Self>) {
        let path = self.path.clone();
        match event {
            EditorEvent::Edited { .. } => {
                let (text, now) = (editor.read(cx).text(), self.now_ms(cx));
                self.live.update(cx, |live, _| live.edited(&path, text, now));
                self._publish = Some(cx.spawn(async move |feed, cx| {
                    cx.background_executor().timer(Duration::from_millis(TEXT_DELAY_MS)).await;
                    let _ = feed.update(cx, |feed, cx| feed.publish_due(cx));
                }));
            }
            EditorEvent::SelectionMoved => {
                let (line, _) = editor.read(cx).caret_position();
                self.live.update(cx, |live, cx| {
                    live.caret_moved(&path, line);
                    cx.notify();
                });
            }
            EditorEvent::Hovered { .. } | EditorEvent::GutterPressed { .. } => {}
        }
    }

    fn publish_due(&mut self, cx: &mut Context<Self>) {
        let now = self.now_ms(cx);
        self.live.update(cx, |live, cx| {
            if !live.due(now).is_empty() {
                cx.notify();
            }
        });
    }

    /// The editor redrew: a new first visible line is a scroll the preview follows side by side.
    fn editor_changed(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let top_line = editor.read(cx).top_line(cx);
        if top_line == self.top_line {
            return;
        }
        self.top_line = top_line;
        let path = self.path.clone();
        self.live.update(cx, |live, cx| {
            live.scrolled(&path, f64::from(top_line));
            cx.notify();
        });
    }

    /// The tab closed: its latest text is published at once, for a preview that outlives it.
    fn closed(&mut self, cx: &mut Context<Self>) {
        self._publish = None;
        let Some(editor) = self.editor.upgrade() else { return };
        let text = editor.read(cx).text();
        let path = self.path.clone();
        self.live.update(cx, |live, cx| {
            live.closed(&path, &text);
            cx.notify();
        });
    }
}

/// What a file opens as: its text document, or the input of the viewer its content wants.
pub(super) enum TabFile {
    Text(DocumentInfo),
    Viewer(EditorInput),
}

/// Opens `path` as text; with `probe` its content is looked at first, and binary or very large
/// content wants the hex view instead, without a document. A probe that fails leaves the file
/// to the text editor, which says what is wrong with it.
pub(super) async fn open_tab_file(host: &Host, label: &str, path: PathBuf, probe: bool) -> IdeResult<TabFile> {
    if probe {
        match ide_host::ops::bytes::fs_probe(host, label, path.clone()).await {
            Ok(found) if wants_hex_view(&found) => {
                return Ok(TabFile::Viewer(binary_input(&path.to_string_lossy(), &found)));
            }
            Ok(_) => {}
            Err(error) => tracing::warn!("the file was not probed: {}", error.message),
        }
    }
    ide_host::ops::doc::open(host, label, path).await.map(TabFile::Text)
}

// ---------------------------------------------------------------------------------------------
// Kinds

fn build_image(input: &EditorInput, window: &mut Window, cx: &mut Context<Workbench>) -> Option<TabView> {
    if input.kind != IMAGE_KIND {
        return None;
    }
    let path = input.path.clone()?;
    let view = cx.new(|cx| ImageView::new(path, cx));
    let events =
        cx.subscribe_in(&view, window, |workbench, view, event: &ImageViewEvent, window, cx| match event.clone() {
            ImageViewEvent::Probe(path) => workbench.probe_image(view.clone(), path, cx),
            ImageViewEvent::OpenAsText(path) => workbench.open_as_text(PathBuf::from(path), window, cx),
        });
    view.update(cx, |view, cx| view.load(cx));
    let runner = view.clone();
    Some(
        TabView::new(IMAGE_KIND, input.clone(), &view, cx)
            .with_actions(&IMAGE_ACTIONS, move |id, cx| runner.update(cx, |view, cx| view.run_action(id, cx)))
            .with_subscription(events),
    )
}

fn build_hex(input: &EditorInput, window: &mut Window, cx: &mut Context<Workbench>) -> Option<TabView> {
    let (path, size, binary) = HexView::parts_of(input)?;
    let view = cx.new(|cx| HexView::new(path, size, binary, cx));
    let events =
        cx.subscribe_in(&view, window, |workbench, view, event: &HexViewEvent, window, cx| match event.clone() {
            HexViewEvent::ReadPage(page) => workbench.read_hex_page(view.clone(), page, cx),
            HexViewEvent::OpenAsText(path) => workbench.open_as_text(PathBuf::from(path), window, cx),
        });
    view.update(cx, |view, cx| view.load(cx));
    Some(TabView::new(BINARY_KIND, input.clone(), &view, cx).with_subscription(events))
}

// ---------------------------------------------------------------------------------------------
// Actions

pub(super) fn register_actions(registry: &mut ActionRegistry) {
    for id in IMAGE_ACTIONS {
        if id == "image.toggleChessboard" {
            registry.handle_toggle(id, |_, cx| chessboard(cx), move |_, window, cx| run_view_action(id, window, cx));
        } else {
            registry.handle(id, move |_, window, cx| run_view_action(id, window, cx));
        }
    }
    for (id, _, _) in MODE_ACTIONS {
        registry.handle(id, move |call, window, cx| {
            // The action's argument names the file, else it is the active editor's.
            let path = call.arg.and_then(serde_json::Value::as_str).map(str::to_owned).or_else(|| {
                active_editor(window, cx).map(|editor| editor.read(cx).path().to_string_lossy().into_owned())
            });
            let Some(path) = path else { return Ok(()) };
            let modes = markdown_shared(cx).modes;
            modes.update(cx, |modes, cx| {
                if modes.run_action(id, Some(&path), cx.default_global::<UiStorage>()) {
                    cx.notify();
                }
            });
            Ok(())
        });
    }
}

/// Runs `id` on the view of the active tab, when that view answers it.
fn run_view_action(id: &str, window: &mut Window, cx: &mut App) -> anyhow::Result<()> {
    let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
    let run = workbench
        .read(cx)
        .active_tab_view()
        .filter(|view| view.actions.contains(&id))
        .and_then(|view| view.run_action.clone())
        .ok_or_else(|| anyhow::anyhow!("no view of the active tab answers {id}"))?;
    run(id, cx);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The workbench's side

impl Workbench {
    /// What the tab `tab_id` holds, once it has been shown.
    pub(super) fn tab_content(&self, tab_id: &str) -> Option<TabContent<'_>> {
        let Content::Project { editors, view_tabs, .. } = &self.content else { return None };
        match view_tabs.views.get(tab_id) {
            Some(view) => Some(TabContent::View(view)),
            None => editors.get(tab_id).map(TabContent::Editor),
        }
    }

    /// The view of the tab `tab_id`, when it holds one.
    pub(super) fn tab_view(&self, tab_id: &str) -> Option<&TabView> {
        match self.tab_content(tab_id)? {
            TabContent::View(view) => Some(view),
            TabContent::Editor(_) => None,
        }
    }

    /// The view of the active tab, or of the single-file window.
    pub(super) fn active_tab_view(&self) -> Option<&TabView> {
        match &self.content {
            Content::Project { tabs, .. } => {
                let id = tabs.pane(&tabs.active_pane)?.active.as_deref()?;
                self.tab_view(id)
            }
            Content::Viewer(view) => Some(view),
            _ => None,
        }
    }

    /// The Markdown layouts around the editor of the tab `tab_id`, when it has them.
    pub(super) fn markdown_host(&self, tab_id: &str) -> Option<Entity<PreviewHost>> {
        let Content::Project { view_tabs, .. } = &self.content else { return None };
        view_tabs.markdown.get(tab_id).map(|tab| tab.host.clone())
    }

    /// The Markdown layouts of the active tab.
    pub(super) fn active_markdown_host(&self) -> Option<Entity<PreviewHost>> {
        let tabs = self.tabs()?;
        let id = tabs.pane(&tabs.active_pane)?.active.as_deref()?;
        self.markdown_host(id)
    }

    /// Where the keyboard goes when the active tab is shown, when it is not its text editor: the
    /// view of a view tab, the preview of a Markdown file in Preview Only.
    pub(super) fn active_focus_target(&self, cx: &App) -> Option<FocusHandle> {
        if let Some(view) = self.active_tab_view() {
            return Some(view.focus.clone());
        }
        let host = self.active_markdown_host()?;
        let host = host.read(cx);
        if host.mode() != MarkdownMode::Preview {
            return None;
        }
        host.preview().map(|preview| preview.focus_handle(cx))
    }

    /// Sets the context keys of the view kinds: on for the kind of the active tab's view, its
    /// focus key while that view has the keyboard, off for every other kind.
    pub(super) fn publish_view_context(&self, window: &mut Window, cx: &mut App) {
        use crate::platform::context::set_context;
        let active = self.active_tab_view().map(|view| (view.kind, view.focus.contains_focused(window, cx)));
        for kind in TAB_KINDS {
            let on = active.is_some_and(|(active, _)| active == kind.id);
            if let Some(key) = kind.active_key {
                set_context(window, cx, key, on);
            }
            if let Some(key) = kind.focus_key {
                set_context(window, cx, key, on && active.is_some_and(|(_, focused)| focused));
            }
        }
    }

    /// Opens a file of the project in a tab of the active pane; a file that already has a tab
    /// there is brought to the front instead. A file that is not open as text opens in its viewer:
    /// an image in the image viewer, binary or very large content (once the probe says so) in the
    /// hex view.
    pub fn open_project_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tabs) = self.tabs() else { return };
        let key = path.to_string_lossy().into_owned();
        let pane = tabs.active_pane.clone();
        if tabs.find_tab(&key, Some(&pane)).is_none() {
            let viewer = image_viewer_for(&key, |_| false)
                .or_else(|| tabs.find_tab(&binary_key(&key), Some(&pane)).map(|(_, tab)| tab.input.clone()));
            if let Some(input) = viewer {
                return self.open_view_tab(input, window, cx);
            }
        }
        self.open_text_tab(path, true, window, cx);
    }

    /// Open as Text: the file opens in a text editor, past the viewers.
    pub fn open_project_file_as_text(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_text_tab(path, false, window, cx);
    }

    /// Open as Text from a viewer, in a project or in a single-file window.
    pub(super) fn open_as_text(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.content, Content::Project { .. }) {
            self.open_project_file_as_text(path, window, cx);
        } else if matches!(self.content, Content::Viewer(_)) {
            self.open_loose_file(path, true, window, cx);
        }
    }

    /// Opens (or brings to the front) the tab of `input` in the active pane; its view is built
    /// when it shows.
    pub(super) fn open_view_tab(&mut self, input: EditorInput, window: &mut Window, cx: &mut Context<Self>) {
        self.with_tabs(window, cx, |tabs, pane| tabs.open_tab(pane, input, OpenTabOptions::default()).is_some());
    }

    /// Opens the text tab of `path`, or brings it to the front. With `probe` a new tab gets its
    /// document only once the probe tells that the file is text; binary content turns it into
    /// the hex view.
    fn open_text_tab(&mut self, path: PathBuf, probe: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Content::Project { tabs, editors, .. } = &mut self.content else { return };
        let key = path.to_string_lossy().into_owned();
        let pane = tabs.active_pane.clone();
        let existing = tabs.find_tab(&key, Some(&pane)).map(|(_, tab)| tab.id.clone());
        let fresh = existing.is_none();
        let tab_id = match existing {
            Some(id) => {
                tabs.activate_tab(&pane, &id);
                id
            }
            None => {
                let input = EditorInput { kind: FILE_KIND.to_owned(), key: key.clone(), path: Some(key), data: None };
                let Some(id) = tabs.open_tab(&pane, input, OpenTabOptions::default()) else { return };
                id
            }
        };
        // The tab counts as loading before the window looks at it, so it is opened once, probed.
        if !editors.contains_key(&tab_id) {
            self.load_tab_probed(tab_id, path, true, probe && fresh, window, cx);
        }
        self.tabs_changed(true, window, cx);
    }

    /// The probe of the text tab `tab_id` found content for a viewer: the tab becomes that viewer,
    /// or goes when its pane has a tab of that viewer already, which comes to the front.
    pub(super) fn tab_shows_viewer(&mut self, tab_id: &str, input: EditorInput) {
        let Content::Project { tabs, .. } = &mut self.content else { return };
        let Some(pane) = pane_of(tabs, tab_id) else { return };
        let shown = tabs
            .pane(&pane)
            .and_then(|open| open.tabs.iter().find(|tab| tab.input.key == input.key))
            .map(|tab| tab.id.clone());
        match shown {
            Some(shown) => {
                tabs.close_tab(&pane, tab_id);
                tabs.activate_tab(&pane, &shown);
            }
            None => {
                if let Some(tab) = tab_mut(tabs, tab_id) {
                    tab.input = input;
                }
            }
        }
    }

    /// Builds the view of the tab `tab_id` from its input. An input its kind cannot show closes
    /// the tab: a tab without a view would be a dead end.
    pub(super) fn build_view_tab(
        &mut self,
        tab_id: String,
        input: EditorInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(kind) = kind(&input.kind) else { return };
        match (kind.build)(&input, window, cx) {
            Some(view) => self.install_view(tab_id, view, window, cx),
            None => {
                tracing::error!("cannot show the {} tab {}", input.kind, input.key);
                if let Content::Project { tabs, .. } = &mut self.content
                    && let Some(pane) = pane_of(tabs, &tab_id)
                {
                    tabs.close_tab(&pane, &tab_id);
                }
                cx.defer_in(window, |workbench, window, cx| workbench.tabs_changed(false, window, cx));
            }
        }
    }

    /// Gives the tab `tab_id` its view; the view taking the keyboard makes its pane the active one.
    fn install_view(&mut self, tab_id: String, view: TabView, window: &mut Window, cx: &mut Context<Self>) {
        let focused = tab_id.clone();
        let took_focus = cx.on_focus_in(&view.focus, window, move |workbench, window, cx| {
            workbench.editor_took_focus(&focused, window, cx);
        });
        if let Content::Project { view_tabs, editor_focus, .. } = &mut self.content {
            view_tabs.views.insert(tab_id.clone(), view);
            editor_focus.insert(tab_id, took_focus);
        }
    }

    /// Releases the views and Markdown hosts of closed tabs. A view with unsaved changes asks its
    /// close guard first, and its tab comes back when the guard says no.
    pub(super) fn release_closed_views(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Content::Project { tabs, view_tabs, editor_focus, .. } = &mut self.content else { return };
        let open: HashSet<&str> =
            tabs.panes.iter().flat_map(|pane| pane.tabs.iter().map(|tab| tab.id.as_str())).collect();
        let closed = |ids: Vec<&String>| -> Vec<String> {
            ids.into_iter().filter(|id| !open.contains(id.as_str())).cloned().collect()
        };
        let closed_hosts = closed(view_tabs.markdown.keys().collect());
        let closed_views = closed(view_tabs.views.keys().collect());
        for id in closed_hosts {
            if let Some(tab) = view_tabs.markdown.remove(&id) {
                tab.feed.update(cx, |feed, cx| feed.closed(cx));
            }
        }
        let mut guarded = Vec::new();
        for id in closed_views {
            editor_focus.remove(&id);
            let Some(view) = view_tabs.views.remove(&id) else { continue };
            if view.is_dirty(cx)
                && let Some(guard) = view.close_guard.clone()
            {
                guarded.push((view, guard));
            }
        }
        for (view, guard) in guarded {
            let decision = guard(window, cx);
            cx.spawn_in(window, async move |workbench, cx| {
                if decision.await {
                    return;
                }
                let _ = workbench.update_in(cx, |workbench, window, cx| workbench.restore_view(view, window, cx));
            })
            .detach();
        }
    }

    /// A view tab whose close was refused comes back with its view.
    fn restore_view(&mut self, view: TabView, window: &mut Window, cx: &mut Context<Self>) {
        let Content::Project { tabs, .. } = &mut self.content else { return };
        let pane = tabs.active_pane.clone();
        let Some(id) = tabs.open_tab(&pane, view.input.clone(), OpenTabOptions::default()) else { return };
        self.install_view(id, view, window, cx);
        self.tabs_changed(true, window, cx);
    }

    /// Puts the Markdown layouts around the editor of the text tab `tab_id`, when its file is a
    /// Markdown file; the layout buttons go to the tab strip.
    pub(super) fn wrap_markdown(
        &mut self,
        tab_id: &str,
        editor: &Entity<Editor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = editor.read(cx).path().to_string_lossy().into_owned();
        if !is_markdown_path(&path) {
            return;
        }
        let shared = markdown_shared(cx);
        let text = editor.read(cx).text();
        shared.live.update(cx, |live, cx| {
            live.opened(&path, &text, 0.0);
            cx.notify();
        });
        let feed_path = path.clone();
        let live = shared.live.clone();
        let feed = cx.new(|cx| MarkdownFeed::new(feed_path, editor, live, cx));
        let settings = serde_json::to_value(crate::platform::settings::effective(&self.label, cx)).ok();
        let font_size = markdown_preview_font_size(settings.as_ref());
        let editor_view: AnyView = editor.clone().into();
        let host = cx.new(|cx| {
            let mut host = PreviewHost::new(
                path,
                editor_view,
                shared.modes.clone(),
                shared.live.clone(),
                shared.images.clone(),
                font_size,
                cx,
            );
            host.set_actions_in_tabs(true, cx);
            host
        });
        let images = shared.images.clone();
        let events = cx.subscribe_in(&host, window, move |workbench, _, event: &MarkdownViewEvent, window, cx| {
            workbench.markdown_event(event.clone(), &images, window, cx);
        });
        host.update(cx, |host, cx| host.load(cx));
        if let Content::Project { view_tabs, .. } = &mut self.content {
            view_tabs.markdown.insert(tab_id.to_owned(), MarkdownTab { host, feed, _events: events });
        }
    }

    /// Carries out what a Markdown preview asks for.
    fn markdown_event(
        &mut self,
        event: MarkdownViewEvent,
        images: &Entity<PreviewImages>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            MarkdownViewEvent::OpenUrl(url) => cx.open_url(&url),
            MarkdownViewEvent::OpenFile { path, line: Some(line) } => {
                self.open_project_file_at(PathBuf::from(path), line, 0, window, cx);
            }
            MarkdownViewEvent::OpenFile { path, line: None } => self.open_project_file(PathBuf::from(path), window, cx),
            MarkdownViewEvent::ReadImage(path) => self.read_preview_image(path, images.clone(), cx),
            // The preview keeps its own scroll offset; Structure does not follow the preview.
            MarkdownViewEvent::ShownLine(_) | MarkdownViewEvent::Scrolled(_) => {}
        }
    }

    /// Reads a local image of a preview as a `data:` URL for every preview that shows it.
    fn read_preview_image(&mut self, path: String, images: Entity<PreviewImages>, cx: &mut Context<Self>) {
        let label = self.label.clone();
        let file = PathBuf::from(&path);
        let read = cx
            .global::<Backend>()
            .with_host(move |host| async move { ide_host::ops::bytes::fs_read_image(&host, &label, file).await });
        cx.spawn(async move |_, cx| {
            let answer = flatten(read.await);
            images.update(cx, |images, cx| {
                images.read(&path, answer);
                cx.notify();
            });
        })
        .detach();
    }

    /// Answers the image viewer's probe with the file's size.
    fn probe_image(&mut self, view: Entity<ImageView>, path: String, cx: &mut Context<Self>) {
        let label = self.label.clone();
        let probe = cx.global::<Backend>().with_host(move |host| async move {
            ide_host::ops::bytes::fs_probe(&host, &label, PathBuf::from(path)).await
        });
        cx.spawn(async move |_, cx| {
            let size = flatten(probe.await).map(|probe| probe.size);
            view.update(cx, |view, cx| view.set_file_size(size, cx));
        })
        .detach();
    }

    /// Answers the hex view's request for page `page`.
    fn read_hex_page(&mut self, view: Entity<HexView>, page: u64, cx: &mut Context<Self>) {
        let label = self.label.clone();
        let path = PathBuf::from(view.read(cx).path());
        let offset = page.saturating_mul(PAGE_SIZE);
        let read = cx.global::<Backend>().with_host(move |host| async move {
            ide_host::ops::bytes::fs_read_range(&host, &label, path, offset, PAGE_LEN).await
        });
        cx.spawn(async move |_, cx| {
            let answer = flatten(read.await);
            view.update(cx, |view, cx| view.page_loaded(page, answer, cx));
        })
        .detach();
    }

    /// Opens a loose file for editing (LightEdit: no project, no tool windows): a text editor, or
    /// its viewer unless `as_text`.
    pub fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_loose_file(path, false, window, cx);
    }

    fn open_loose_file(&mut self, path: PathBuf, as_text: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.content = Content::Opening(path.clone());
        self.light_find_bar = None;
        self.sync_welcome(window, cx);
        cx.notify();
        let label = self.label.clone();
        let opened = cx.global::<Backend>().with_host(move |host| async move {
            let (blocking_host, blocking_label, file) = (host.clone(), label.clone(), path.clone());
            tokio::task::spawn_blocking(move || blocking_host.open_light(&blocking_label, &file).map(|_| ()))
                .await
                .map_err(|e| ide_core::IdeError::internal(e.to_string()))??;
            let file = path.canonicalize().unwrap_or(path);
            if !as_text && let Some(input) = image_viewer_for(&file.to_string_lossy(), |_| false) {
                return Ok(TabFile::Viewer(input));
            }
            open_tab_file(&host, &label, file, !as_text).await
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let result = opened.await;
            let _ = workbench.update_in(cx, |workbench, window, cx| {
                let host = cx.global::<Backend>().host.clone();
                let label = workbench.label.clone();
                workbench.content = match flatten(result) {
                    // The document's text is read before the view exists, so a failure is a
                    // message in the window instead of an editor without a document.
                    Ok(TabFile::Text(info)) => {
                        let text = host.project(&label).map_err(|error| error.message).and_then(|project| {
                            project.documents.rope_snapshot(info.id).map_err(|error| error.message)
                        });
                        match text {
                            Ok((version, rope)) => {
                                let editor = cx.new(|cx| Editor::with_text(host, &label, &info, version, rope, cx));
                                workbench.install_intelligence(&editor, window, cx);
                                let focus = editor.focus_handle(cx);
                                window.focus(&focus, cx);
                                workbench.observe_editor(&editor, cx);
                                Content::Editor(editor)
                            }
                            Err(message) => Content::Failed(message.into()),
                        }
                    }
                    Ok(TabFile::Viewer(input)) => {
                        let built = kind(&input.kind).and_then(|viewer| (viewer.build)(&input, window, cx));
                        match built {
                            Some(view) => {
                                window.focus(&view.focus, cx);
                                Content::Viewer(Box::new(view))
                            }
                            None => Content::Failed(input.key.into()),
                        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features_b::viewers::file_kinds::image_input;
    use ide_vfs::FileProbe;

    #[test]
    fn viewer_tabs_are_titled_by_their_file_and_not_restored_with_the_session() {
        let image = image_input("/p/img/logo.png");
        let probe = FileProbe { size: 3, binary: true, too_large: false };
        let hex = binary_input("/p/data.bin", &probe);
        assert_eq!(title_of(&image).as_deref(), Some("logo.png"));
        assert_eq!(title_of(&hex).as_deref(), Some("data.bin"));
        assert_eq!(icon_of(&image), theme::icons::file_icon_name("logo.png"));
        assert!(!restorable(&image) && !restorable(&hex));
        let path = "/p/a.md".to_owned();
        let text = EditorInput { kind: FILE_KIND.to_owned(), key: path.clone(), path: Some(path), data: None };
        assert_eq!(title_of(&text), None);
        assert!(!restorable(&text));
        assert_eq!(icon_of(&text), theme::icons::file_icon_name("a.md"));
        assert_eq!(binary_key("/p/data.bin"), hex.key);
    }

    #[test]
    fn every_kind_is_registered_once() {
        let ids: HashSet<&str> = TAB_KINDS.iter().map(|kind| kind.id).collect();
        assert_eq!(ids.len(), TAB_KINDS.len());
        assert!(!ids.contains(FILE_KIND));
        assert!(kind(IMAGE_KIND).is_some_and(|kind| kind.active_key == Some("imageViewerActive")));
        assert!(kind("diff").is_none());
    }
}
