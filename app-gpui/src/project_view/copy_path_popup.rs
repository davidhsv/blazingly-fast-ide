//! Copy Path/Reference… at the caret: the forms of the edited file's path (absolute, file name,
//! from the content root, from the source root, with the caret's line) and its reference, each
//! with the value it copies and the shortcut of the action that copies it straight away.
//!
//! The reference is resolved after the popup opens: its row waits as "Resolving reference…"
//! and cannot be chosen until [`CopyPathPopup::set_reference`] says what it is. Typing filters
//! the rows by label, the arrows move the selection, Enter or a click copies, Escape closes. The
//! popup copies nothing itself: the workbench puts the text on the clipboard.

use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::input;
use crate::platform::misc::copy_path::{PathKind, path_to_copy};
use crate::platform::project_roots::{OpenProject, RootFolder};
use crate::theme;
use crate::ui::matching::{fuzzy_match, index_runs};
use gpui_kit::{
    App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, MouseButton, Render,
    SharedString, Window, div, prelude::*, px,
};
use std::collections::BTreeMap;

/// The action that copies the reference, and the id of its row.
pub const REFERENCE: &str = "editor.copyReference";
/// The action that copies the path with the caret's line.
pub const PATH_WITH_LINE: &str = "editor.copyPathWithLineNumber";
const RESOLVING: &str = "Resolving reference…";
const UNRESOLVED: &str = "Cannot copy reference";
/// At most this many rows show before the list scrolls.
const VISIBLE_ROWS: usize = 12;
const ROW_HEIGHT: f32 = 24.0;

/// A row of the popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyPathItem {
    /// The action that copies this form, which is also the row's id.
    pub id: &'static str,
    /// English source text.
    pub label: &'static str,
    /// What choosing the row copies; none while it is not known.
    pub preview: Option<String>,
    /// The keymap's shortcut of the action.
    pub shortcut: Option<String>,
    /// Why the row cannot be chosen (English source text); none when it can.
    pub disabled: Option<&'static str>,
}

/// The rows for the file `path` with the caret on the 0-based `line`: every form of the path it
/// has (no source-root path outside the source roots), then the reference, still resolving.
/// `shortcuts` are the keymap's labels by action id.
pub fn copy_path_items(
    path: &str,
    line: u32,
    project: Option<OpenProject<'_>>,
    roots: &[RootFolder],
    shortcuts: &BTreeMap<String, String>,
) -> Vec<CopyPathItem> {
    let form = |kind: PathKind| path_to_copy(kind, path, project, roots);
    let with_line = form(PathKind::Relative).map(|relative| format!("{relative}:{}", line + 1));
    let flavours = [
        ("project.copyAbsolutePath", "Absolute Path", form(PathKind::Absolute)),
        ("project.copyFileName", "File Name", form(PathKind::Name)),
        ("project.copyRelativePath", "Path From Content Root", form(PathKind::Relative)),
        ("project.copySourceRootPath", "Path From Source Root", form(PathKind::Source)),
        (PATH_WITH_LINE, "Path with line number", with_line),
    ];
    let shortcut = |id: &str| shortcuts.get(id).cloned();
    let mut items: Vec<CopyPathItem> = flavours
        .into_iter()
        .filter_map(|(id, label, value)| {
            let preview = Some(value?);
            Some(CopyPathItem { id, label, preview, shortcut: shortcut(id), disabled: None })
        })
        .collect();
    items.push(CopyPathItem {
        id: REFERENCE,
        label: "Reference",
        preview: Some(RESOLVING.to_owned()),
        shortcut: shortcut(REFERENCE),
        disabled: Some(RESOLVING),
    });
    items
}

/// The hint shown once `text` is on the clipboard, in the UI language.
pub fn copied_hint(i18n: &mut I18n, action: &str, text: &str) -> String {
    let value = [Param::from(text)];
    if action == REFERENCE {
        i18n.tx_built(&["Copied reference: ", ""], &value)
    } else {
        i18n.tx_built(&["Copied ", ""], &value)
    }
}

/// What the popup asks of the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyPathEvent {
    /// Escape, or a click beside the popup.
    Dismissed,
    /// Close the popup and copy `text`, the value of the row of `action`, then show
    /// [`copied_hint`].
    Copy { action: &'static str, text: String },
}

/// A row as shown: the item and its label in the UI language.
struct Row {
    item: CopyPathItem,
    label: String,
}

pub struct CopyPathPopup {
    title: String,
    rows: Vec<Row>,
    query: String,
    /// The selected row among the ones the query keeps.
    active: usize,
    focus: FocusHandle,
    key_claim: u64,
}

impl EventEmitter<CopyPathEvent> for CopyPathPopup {}

impl CopyPathPopup {
    /// The popup over `items` ([`copy_path_items`]).
    pub fn new(items: Vec<CopyPathItem>, cx: &mut Context<Self>) -> Self {
        let i18n = &cx.default_global::<Localization>().0;
        let title = i18n.tx("Copy Path/Reference…");
        let rows: Vec<Row> = items.into_iter().map(|item| Row { label: i18n.tx(item.label), item }).collect();
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |popup, cx| popup.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|popup, cx| input::release_keys(popup.key_claim, cx)).detach();
        let mut popup = Self { title, rows, query: String::new(), active: 0, focus, key_claim };
        popup.active = popup.first_enabled();
        popup
    }

    /// The rows the query keeps, with where it matched each label.
    fn filtered(&self) -> Vec<(usize, Option<Vec<usize>>)> {
        if self.query.is_empty() {
            return (0..self.rows.len()).map(|index| (index, None)).collect();
        }
        let matched = self.rows.iter().enumerate().map(|(index, row)| (index, fuzzy_match(&row.label, &self.query)));
        matched.filter_map(|(index, found)| found.map(|found| (index, Some(found)))).collect()
    }

    fn first_enabled(&self) -> usize {
        let filtered = self.filtered();
        filtered.iter().position(|(index, _)| self.rows[*index].item.disabled.is_none()).unwrap_or(0)
    }

    /// The rows the query keeps, as `label | preview | shortcut`.
    pub fn lines(&self) -> Vec<String> {
        self.filtered()
            .into_iter()
            .map(|(index, _)| {
                let row = &self.rows[index];
                let preview = row.item.preview.as_deref().unwrap_or_default();
                format!("{} | {preview} | {}", row.label, row.item.shortcut.as_deref().unwrap_or_default())
            })
            .collect()
    }

    /// The id of the selected row.
    pub fn selected(&self) -> Option<&'static str> {
        let filtered = self.filtered();
        let at = self.active.min(filtered.len().checked_sub(1)?);
        Some(self.rows[filtered[at].0].item.id)
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// The reference was resolved (`Some`) or could not be (`None`): its row can now be chosen,
    /// or says it cannot.
    pub fn set_reference(&mut self, reference: Option<String>, cx: &mut Context<Self>) {
        let Some(row) = self.rows.iter_mut().find(|row| row.item.id == REFERENCE) else { return };
        match reference {
            Some(reference) => (row.item.preview, row.item.disabled) = (Some(reference), None),
            None => (row.item.preview, row.item.disabled) = (None, Some(UNRESOLVED)),
        }
        cx.notify();
    }

    /// Copies the row at `index` of the filtered rows, unless it cannot be chosen.
    fn activate(&mut self, at: usize, cx: &mut Context<Self>) {
        let Some(&(index, _)) = self.filtered().get(at) else { return };
        let item = &self.rows[index].item;
        if item.disabled.is_some() {
            return;
        }
        let Some(text) = item.preview.clone() else { return };
        cx.emit(CopyPathEvent::Copy { action: item.id, text });
    }

    fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        self.query = query;
        self.active = 0;
        cx.notify();
    }

    /// The popup's keys, ahead of the keymap: the arrows, Enter, Escape, and typing the query.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt {
            return false;
        }
        let shown = self.filtered().len();
        match keystroke.key.as_str() {
            "up" => self.active = self.active.min(shown.saturating_sub(1)).saturating_sub(1),
            "down" => self.active = (self.active + 1).min(shown.saturating_sub(1)),
            "enter" => {
                let at = self.active.min(shown.saturating_sub(1));
                self.activate(at, cx);
                return true;
            }
            "escape" => {
                cx.emit(CopyPathEvent::Dismissed);
                return true;
            }
            "backspace" if !self.query.is_empty() => {
                let mut query = self.query.clone();
                query.pop();
                self.set_query(query, cx);
                return true;
            }
            _ => {
                // The character the key types, where the platform gives one; a space only once
                // something is typed.
                let typed = keystroke.key_char.as_deref().unwrap_or(match keystroke.key.as_str() {
                    "space" => " ",
                    key => key,
                });
                let mut chars = typed.chars();
                let (Some(c), None) = (chars.next(), chars.next()) else { return false };
                if c.is_control() || (c == ' ' && self.query.is_empty()) {
                    return false;
                }
                self.set_query(format!("{}{c}", self.query), cx);
                return true;
            }
        }
        cx.notify();
        true
    }

    fn label(cx: &App, label: &str, matched: Option<&[usize]>, color: gpui_kit::Hsla) -> Div {
        let accent = theme::color(cx, "accent");
        let mut parts: Vec<Div> = Vec::new();
        let mut at = 0;
        for run in index_runs(label, matched.unwrap_or_default()) {
            if run.start > at {
                parts.push(div().child(SharedString::from(label[at..run.start].to_owned())));
            }
            parts.push(div().text_color(accent).child(SharedString::from(label[run.clone()].to_owned())));
            at = run.end;
        }
        if at < label.len() {
            parts.push(div().child(SharedString::from(label[at..].to_owned())));
        }
        div().flex_none().flex().whitespace_nowrap().text_color(color).children(parts)
    }
}

impl Focusable for CopyPathPopup {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for CopyPathPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, muted) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let faint = theme::color(cx, "fg-faint");
        let filtered = self.filtered();
        let active = self.active.min(filtered.len().saturating_sub(1));
        // Sized from the content, as wide as the longest row needs.
        let chars = self.rows.iter().map(|row| {
            row.label.chars().count()
                + row.item.preview.as_deref().map_or(0, |preview| preview.chars().count())
                + row.item.shortcut.as_deref().map_or(0, |shortcut| shortcut.chars().count())
        });
        let chars = chars.chain(std::iter::once(self.title.chars().count())).max().unwrap_or_default();
        let width = (chars as f32 * 7.0 + 90.0).clamp(320.0, 720.0);
        let rows: Vec<_> = filtered
            .iter()
            .enumerate()
            .map(|(at, (index, matched))| {
                let row = &self.rows[*index];
                let disabled = row.item.disabled.is_some();
                // A row that waits shows why; a path is shown as it is.
                let i18n = &cx.default_global::<Localization>().0;
                let preview = match (&row.item.preview, row.item.disabled) {
                    (Some(preview), Some(reason)) if preview == reason => Some(i18n.tx(reason)),
                    (Some(preview), _) => Some(preview.clone()),
                    (None, reason) => reason.map(|reason| i18n.tx(reason)),
                };
                let preview = preview.map(|preview| {
                    let text = SharedString::from(preview);
                    div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_color(muted).child(text)
                });
                let shortcut = row.item.shortcut.clone().map(|shortcut| {
                    div().flex_none().ml(px(12.0)).text_color(muted).child(SharedString::from(shortcut))
                });
                div()
                    .id(at)
                    .debug_selector(move || format!("copy-path-row-{at}"))
                    .h(px(ROW_HEIGHT))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .when(at == active, |el| el.bg(theme::color(cx, "bg-tree-selected")))
                    .child(Self::label(cx, &row.label, matched.as_deref(), if disabled { faint } else { fg }))
                    .children(preview)
                    .children(shortcut)
                    .on_hover(cx.listener(move |popup: &mut Self, hovered: &bool, _, cx| {
                        if *hovered && popup.active != at {
                            popup.active = at;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |popup, _: &ClickEvent, _, cx| popup.activate(at, cx)))
            })
            .collect();
        let empty = rows.is_empty().then(|| {
            let text = cx.default_global::<Localization>().0.tx("Nothing matches");
            div().px(px(10.0)).h(px(ROW_HEIGHT)).flex().items_center().text_color(faint).child(SharedString::from(text))
        });
        let query = (!self.query.is_empty())
            .then(|| div().text_color(theme::color(cx, "accent")).child(SharedString::from(self.query.clone())));
        let height = filtered.len().clamp(1, VISIBLE_ROWS) as f32 * ROW_HEIGHT;
        div()
            .w(px(width))
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .track_focus(&self.focus)
            // A click in the popup is not a click beside it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(10.0))
                    .h(theme::length(cx, "h-row"))
                    .text_color(muted)
                    .child(SharedString::from(self.title.clone()))
                    .children(query),
            )
            .child(div().id("copy-path-list").h(px(height)).overflow_y_scroll().children(rows).children(empty))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::project_roots::FolderRole;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<CopyPathEvent>>>;

    const PATH: &str = "/p/src/A.java";

    fn project() -> Option<OpenProject<'static>> {
        Some(OpenProject { root: "/p", name: "p", workspace: None })
    }

    fn source_root(path: &str) -> Vec<RootFolder> {
        vec![RootFolder { path: path.to_owned(), kind: FolderRole::Source, detected_from: None }]
    }

    fn shortcuts() -> BTreeMap<String, String> {
        [("project.copyFileName", "⌥⌘N"), ("project.copyAbsolutePath", "⇧⌘C")]
            .map(|(id, label)| (id.to_owned(), label.to_owned()))
            .into()
    }

    /// The popup for line 2 of `A.java` in `/p`, whose `src` is a source root.
    fn items() -> Vec<CopyPathItem> {
        copy_path_items(PATH, 1, project(), &source_root("src"), &shortcuts())
    }

    fn open(
        cx: &mut TestAppContext,
        items: Vec<CopyPathItem>,
    ) -> (Entity<CopyPathPopup>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            // Each `init` adds a keystroke interceptor: a second one would see every key twice.
            if !cx.has_global::<input::Keymap>() {
                input::init(cx);
            }
        });
        let (popup, cx) = cx.add_window_view(move |_, cx| CopyPathPopup::new(items, cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&popup, move |_, event: &CopyPathEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = popup.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (popup, events, cx)
    }

    fn select(popup: &Entity<CopyPathPopup>, id: &str, cx: &mut VisualTestContext) {
        for _ in 0..8 {
            if popup.read_with(cx, |popup, _| popup.selected()) == Some(id) {
                return;
            }
            cx.simulate_keystrokes("down");
        }
        panic!("{id} is not a row");
    }

    // src/editor/core/copyPathPopup.test.tsx › editor Copy Path/Reference chooser › opens at the caret without
    // copying, with every preview and assigned shortcut
    #[gpui_kit::test]
    async fn opens_at_the_caret_without_copying_with_every_preview_and_assigned_shortcut(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, items());
        assert!(events.borrow().is_empty());
        let shown: Vec<(&str, Option<String>)> = items().into_iter().map(|item| (item.label, item.preview)).collect();
        let expected = [
            ("Absolute Path", Some(PATH.to_owned())),
            ("File Name", Some("A.java".to_owned())),
            ("Path From Content Root", Some("src/A.java".to_owned())),
            ("Path From Source Root", Some("A.java".to_owned())),
            ("Path with line number", Some("src/A.java:2".to_owned())),
            ("Reference", Some(RESOLVING.to_owned())),
        ];
        assert_eq!(shown, expected);
        let lines = popup.read_with(cx, |popup, _| popup.lines());
        assert_eq!(lines[1], "File Name | A.java | ⌥⌘N");
        for item in items() {
            assert_eq!(item.shortcut, shortcuts().get(item.id).cloned(), "{}", item.id);
        }
        // The reference cannot be copied while it resolves.
        select(&popup, REFERENCE, cx);
        cx.simulate_keystrokes("enter");
        assert!(events.borrow().is_empty());
        popup.update(cx, |popup, cx| popup.set_reference(Some("com.acme.A".to_owned()), cx));
        assert_eq!(popup.read_with(cx, |popup, _| popup.lines())[5], "Reference | com.acme.A | ");
    }

    // src/editor/core/copyPathPopup.test.tsx › editor Copy Path/Reference chooser › copies the preview through %s
    // and confirms it
    #[gpui_kit::test]
    async fn copies_the_preview_through_each_action_and_confirms_it(cx: &mut TestAppContext) {
        let ids = [
            "project.copyAbsolutePath",
            "project.copyFileName",
            "project.copyRelativePath",
            "project.copySourceRootPath",
            PATH_WITH_LINE,
            REFERENCE,
        ];
        for id in ids {
            let (popup, events, visual) = open(cx, items());
            popup.update(visual, |popup, cx| popup.set_reference(Some("com.acme.A".to_owned()), cx));
            select(&popup, id, visual);
            visual.simulate_keystrokes("enter");
            let preview = items().into_iter().find(|item| item.id == id).and_then(|item| item.preview);
            let text = if id == REFERENCE { "com.acme.A".to_owned() } else { preview.expect("a preview") };
            let hint = copied_hint(&mut I18n::default(), id, &text);
            assert_eq!(*events.borrow(), [CopyPathEvent::Copy { action: id, text: text.clone() }]);
            let expected = if id == REFERENCE { format!("Copied reference: {text}") } else { format!("Copied {text}") };
            assert_eq!(hint, expected);
        }
    }

    // src/editor/core/copyPathPopup.test.tsx › editor Copy Path/Reference chooser › omits source paths outside
    // known roots and supports loose files
    #[test]
    fn omits_source_paths_outside_known_roots_and_supports_loose_files() {
        let outside = copy_path_items(PATH, 1, project(), &source_root("other"), &BTreeMap::new());
        assert!(!outside.iter().any(|item| item.id == "project.copySourceRootPath"));
        let loose = copy_path_items(PATH, 1, None, &[], &BTreeMap::new());
        let preview = |id: &str| loose.iter().find(|item| item.id == id).and_then(|item| item.preview.clone());
        assert_eq!(preview(PATH_WITH_LINE).as_deref(), Some("/p/src/A.java:2"));
        assert_eq!(preview("project.copyRelativePath").as_deref(), Some(PATH));
    }

    // src/editor/core/copyPathPopup.test.tsx › editor Copy Path/Reference chooser › copies the reference preview
    // even if the caret moves afterwards
    #[gpui_kit::test]
    async fn copies_the_reference_preview_even_if_the_caret_moves_afterwards(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, items());
        popup.update(cx, |popup, cx| popup.set_reference(Some("com.acme.A".to_owned()), cx));
        cx.run_until_parked();
        let last = popup.read_with(cx, |popup, _| popup.lines().len()) - 1;
        assert_eq!(last, 5);
        let bounds = cx.debug_bounds("copy-path-row-5").expect("the reference row is drawn");
        cx.simulate_click(bounds.center(), Modifiers::default());
        assert_eq!(*events.borrow(), [CopyPathEvent::Copy { action: REFERENCE, text: "com.acme.A".to_owned() }]);
    }

    // src/editor/core/copyPathPopup.test.tsx › editor Copy Path/Reference chooser › supports keyboard selection and
    // Escape through the existing list popup
    #[gpui_kit::test]
    async fn supports_keyboard_selection_and_escape_through_the_existing_list_popup(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, items());
        assert_eq!(popup.read_with(cx, |popup, _| popup.lines().len()), 6);
        cx.simulate_keystrokes("down enter");
        let copied = CopyPathEvent::Copy { action: "project.copyFileName", text: "A.java".to_owned() };
        assert_eq!(*events.borrow(), [copied]);
        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().last(), Some(&CopyPathEvent::Dismissed));
        // Typing filters by label; Backspace takes the last character back.
        cx.simulate_keystrokes("r e f");
        assert_eq!(popup.read_with(cx, |popup, _| popup.query().to_owned()), "ref");
        assert_eq!(popup.read_with(cx, |popup, _| popup.selected()), Some(REFERENCE));
        cx.simulate_keystrokes("backspace backspace backspace");
        assert_eq!(popup.read_with(cx, |popup, _| popup.lines().len()), 6);
    }

    // Not from a spec: a reference that cannot be resolved stays listed and cannot be copied.
    #[gpui_kit::test]
    async fn an_unresolved_reference_cannot_be_copied(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, items());
        popup.update(cx, |popup, cx| popup.set_reference(None, cx));
        select(&popup, REFERENCE, cx);
        cx.simulate_keystrokes("enter");
        assert!(events.borrow().is_empty());
        assert_eq!(popup.read_with(cx, |popup, _| popup.lines())[5], "Reference |  | ");
    }
}
