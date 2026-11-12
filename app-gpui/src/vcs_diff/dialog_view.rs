//! The diff window: a [`DiffView`] under its toolbar and the diff's title, with the files of the
//! diff's chain beside it when it has several, as the modal commit interface shows diffs in front of
//! the Commit dialog. Escape and the close button close it, and so does Jump to Source, so that the
//! editor it opens is not hidden behind the window. Moving past the last difference of a file, or
//! choosing another file, shows that file in the same window.
//!
//! [`DiffDialog`] reads no file. The window loads the texts of [`DiffDialog::data`] once it has made
//! the dialog and after every [`DiffDialogEvent::Load`], and hands them to [`DiffDialog::loaded`].

use super::diff_input::{DiffData, PaneSide, chain_neighbour, diff_key, diff_title, side_file, side_label, with_chain};
use super::diff_options::{DiffViewOptions, Side};
use super::doc::Direction;
use super::file_tree::{FileTreeNode, build_file_tree};
use super::toolbar_view::{DiffToolbar, DiffToolbarEvent};
use super::view::{DiffView, DiffViewEvent};
use crate::platform::input;
use crate::theme;
use gpui_kit::{
    App, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Render, SharedString,
    Subscription, Window, div, prelude::*, px,
};

/// Width of the files list.
const FILES_WIDTH: f32 = 220.0;

/// What the diff window asks of its window.
#[derive(Clone, Debug, PartialEq)]
pub enum DiffDialogEvent {
    /// Load both sides of this diff ([`super::diff_input::side_load`]) and call [`DiffDialog::loaded`].
    Load(Box<DiffData>),
    /// Look along the chain of `from` for the next file with changes in `direction`
    /// ([`super::diff_navigation::ChainSearch`]) and show it with [`DiffDialog::show`].
    FindFile {
        from: Box<DiffData>,
        direction: Direction,
    },
    /// What the viewer asks: an edit to apply, a file to open, its context menu, a reload, settings
    /// to store.
    Viewer(DiffViewEvent),
    /// Show a menu of the toolbar.
    Menu(DiffToolbarEvent),
    Close,
}

/// A row of the files list: a directory, or the chain's file `index`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRow {
    pub depth: usize,
    pub name: String,
    pub index: Option<usize>,
}

/// The directory every path is in.
fn common_dir<'a>(paths: impl Iterator<Item = &'a str>) -> String {
    let mut common: Option<Vec<&'a str>> = None;
    for path in paths {
        let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
        let parts: Vec<&str> = dir.split('/').collect();
        common = Some(match common {
            None => parts,
            Some(common) => common.iter().zip(&parts).take_while(|(a, b)| a == b).map(|(a, _)| *a).collect(),
        });
    }
    common.map(|parts| parts.join("/")).unwrap_or_default()
}

/// The files of `data`'s chain as a tree under the directory they share, directories first; none
/// for a diff of one file.
pub fn file_rows(data: &DiffData) -> Vec<FileRow> {
    fn flatten(nodes: &[FileTreeNode<usize>], depth: usize, out: &mut Vec<FileRow>) {
        for node in nodes {
            out.push(FileRow { depth, name: node.name.clone(), index: node.file });
            flatten(&node.children, depth + 1, out);
        }
    }
    let Some(chain) = data.chain.as_ref().filter(|chain| chain.len() > 1) else { return Vec::new() };
    let root = common_dir(chain.iter().map(|item| item.path.as_str()));
    let relative = |index: &usize| {
        let path = chain[*index].path.as_str();
        path.strip_prefix(root.as_str()).unwrap_or(path).trim_start_matches('/').to_owned()
    };
    let tree = build_file_tree(0..chain.len(), relative, |_| false);
    let mut out = Vec::new();
    flatten(&tree, 0, &mut out);
    out
}

pub struct DiffDialog {
    data: DiffData,
    viewer: Entity<DiffView>,
    toolbar: Entity<DiffToolbar>,
    focus: FocusHandle,
    files: Vec<FileRow>,
    key_claims: Vec<u64>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DiffDialogEvent> for DiffDialog {}

impl DiffDialog {
    /// The window for `data`, its viewer under the stored `options` (or the viewer kind the diff
    /// asks for, which is not stored).
    pub fn new(data: DiffData, options: DiffViewOptions, cx: &mut Context<Self>) -> Self {
        let options = DiffViewOptions { mode: data.mode.unwrap_or(options.mode), ..options };
        let viewer = cx.new(|cx| DiffView::new("", "", options, cx));
        let shown = viewer.clone();
        let toolbar = cx.new(|cx| DiffToolbar::new(shown, cx));
        let subscriptions = vec![
            cx.subscribe(&viewer, |dialog, _, event: &DiffViewEvent, cx| dialog.viewer_event(event, cx)),
            cx.subscribe(&toolbar, |_, _, event: &DiffToolbarEvent, cx| cx.emit(DiffDialogEvent::Menu(event.clone()))),
        ];
        let focus = cx.focus_handle();
        let viewer_focus = viewer.read(cx).focus_handle(cx);
        let key_claims = [focus.clone(), viewer_focus]
            .into_iter()
            .map(|handle| {
                let dialog = cx.weak_entity();
                input::claim_keys(
                    handle,
                    move |keystroke, _, cx| dialog.update(cx, |dialog, cx| dialog.key(keystroke, cx)).unwrap_or(false),
                    cx,
                )
            })
            .collect();
        cx.on_release(|dialog, cx| {
            for claim in &dialog.key_claims {
                input::release_keys(*claim, cx);
            }
        })
        .detach();
        let files = file_rows(&data);
        let mut dialog = Self { data, viewer, toolbar, focus, files, key_claims, _subscriptions: subscriptions };
        dialog.set_neighbours(cx);
        dialog
    }

    /// The diff shown.
    pub fn data(&self) -> &DiffData {
        &self.data
    }

    pub fn viewer(&self) -> &Entity<DiffView> {
        &self.viewer
    }

    pub fn toolbar(&self) -> &Entity<DiffToolbar> {
        &self.toolbar
    }

    /// The files list; empty for a diff of one file.
    pub fn files(&self) -> &[FileRow] {
        &self.files
    }

    /// The window's title: the file and the revisions compared.
    pub fn title(&self) -> String {
        diff_title(&self.data)
    }

    fn set_neighbours(&mut self, cx: &mut Context<Self>) {
        let previous = chain_neighbour(&self.data, Direction::Previous).is_some();
        let next = chain_neighbour(&self.data, Direction::Next).is_some();
        self.viewer.update(cx, |viewer, cx| viewer.set_neighbours(previous, next, cx));
    }

    /// Shows another diff (another file of the chain) in the window, once its texts are loaded.
    pub fn show(&mut self, data: DiffData, cx: &mut Context<Self>) {
        self.files = file_rows(&data);
        self.data = data;
        self.set_neighbours(cx);
        cx.emit(DiffDialogEvent::Load(Box::new(self.data.clone())));
        cx.notify();
    }

    /// The texts of the shown diff arrived; `binary` when a side is not text. Reveals the line the
    /// diff asks for, or its first or last difference.
    pub fn loaded(&mut self, left: &str, right: &str, binary: bool, cx: &mut Context<Self>) {
        let data = self.data.clone();
        let has_source = side_file(&data.left, &data.path).is_some() || side_file(&data.right, &data.path).is_some();
        self.viewer.update(cx, |viewer, cx| {
            viewer.set_texts(left, right, cx);
            viewer.set_binary(binary, cx);
            viewer.set_labels(side_label(&data.left), side_label(&data.right), cx);
            viewer.set_has_source(has_source, cx);
            if let Some(line) = data.line {
                let side = if data.line_side == Some(PaneSide::Left) { Side::A } else { Side::B };
                viewer.reveal_line(side, line as usize, cx);
            } else if let Some(reveal) = data.reveal {
                viewer.reveal(reveal, cx);
            }
        });
        cx.notify();
    }

    /// Shows file `index` of the chain.
    pub fn select_file(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(chain) = self.data.chain.clone() else { return };
        if let Some(next) = with_chain(&chain, index).filter(|next| diff_key(next) != diff_key(&self.data)) {
            self.show(next, cx);
        }
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(DiffDialogEvent::Close);
    }

    fn viewer_event(&mut self, event: &DiffViewEvent, cx: &mut Context<Self>) {
        match event {
            DiffViewEvent::OpenFile(direction) => {
                cx.emit(DiffDialogEvent::FindFile { from: Box::new(self.data.clone()), direction: *direction });
            }
            DiffViewEvent::JumpToSource { .. } => {
                cx.emit(DiffDialogEvent::Viewer(event.clone()));
                self.close(cx);
            }
            other => cx.emit(DiffDialogEvent::Viewer(other.clone())),
        }
    }

    /// Escape closes the window.
    fn key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = &keystroke.modifiers;
        let plain = !modifiers.control && !modifiers.platform && !modifiers.alt && !modifiers.shift;
        if keystroke.key == "escape" && plain {
            self.close(cx);
            return true;
        }
        false
    }

    fn render_files(&self, cx: &mut Context<Self>) -> Div {
        let shown = diff_key(&self.data);
        let chain = self.data.chain.as_deref().unwrap_or_default();
        let mut list = div()
            .debug_selector(|| "diff-files".to_owned())
            .w(px(FILES_WIDTH))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .py(px(4.0))
            .overflow_hidden()
            .border_r_1()
            .border_color(theme::color(cx, "border"))
            .text_size(theme::length(cx, "fs-ui"));
        for (position, row) in self.files.iter().enumerate() {
            let selected = row.index.and_then(|index| chain.get(index)).is_some_and(|item| diff_key(item) == shown);
            let icon = if row.index.is_some() { "diff" } else { "folder" };
            let entry = div()
                .id(SharedString::from(format!("diff-file-row-{position}")))
                .h(theme::length(cx, "h-row"))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(4.0))
                .pl(px(8.0 + 12.0 * row.depth as f32))
                .pr(px(8.0))
                .whitespace_nowrap()
                .overflow_hidden()
                .text_color(theme::color(cx, if row.index.is_some() { "fg" } else { "fg-secondary" }))
                .when(selected, |el| el.bg(theme::color(cx, "bg-selected")))
                .child(theme::icon(cx, icon, 14.0, theme::color(cx, "fg-muted")))
                .child(SharedString::from(row.name.clone()));
            let entry = match row.index {
                Some(index) => {
                    let selector = format!("diff-file-{index}");
                    entry
                        .debug_selector(move || selector.clone())
                        .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                        .on_click(cx.listener(move |dialog, _, _, cx| dialog.select_file(index, cx)))
                }
                None => entry,
            };
            list = list.child(entry);
        }
        list
    }
}

impl Focusable for DiffDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for DiffDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = theme::color(cx, "fg-muted");
        let header = div()
            .h(theme::length(cx, "h-header"))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(12.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(
                div()
                    .debug_selector(|| "diff-dialog-title".to_owned())
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(theme::color(cx, "fg-strong"))
                    .child(SharedString::from(self.title())),
            )
            .child(
                div()
                    .id("diff-dialog-close")
                    .debug_selector(|| "diff-dialog-close".to_owned())
                    .size(px(24.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme::length(cx, "radius-small"))
                    .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .child(theme::icon(cx, "close", 14.0, muted))
                    .on_click(cx.listener(|dialog, _, _, cx| dialog.close(cx))),
            );
        let body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .when(!self.files.is_empty(), |el| el.child(self.render_files(cx)))
            .child(div().flex_1().min_w_0().h_full().child(self.viewer.clone()));
        div()
            .debug_selector(|| "diff-dialog".to_owned())
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .bg(theme::color(cx, "bg-app"))
            .text_size(theme::length(cx, "fs-ui"))
            .child(header)
            .child(self.toolbar.clone())
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs_diff::diff_input::{DiffSide, Reveal};
    use crate::vcs_diff::toolbar_view::DiffCommand;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<DiffDialogEvent>>>;

    fn diff(path: &str) -> DiffData {
        DiffData::new(path, DiffSide::rev("HEAD"), DiffSide::worktree())
    }

    fn chain(index: usize) -> DiffData {
        let items = [diff("/repo/src/a.ts"), diff("/repo/src/lib/b.ts"), diff("/repo/README.md")];
        with_chain(&items, index).expect("the item exists")
    }

    /// The window for `data` with its texts loaded, and what it asked of its window.
    fn open<'a>(
        cx: &'a mut TestAppContext,
        data: DiffData,
        left: &str,
        right: &str,
    ) -> (Entity<DiffDialog>, Events, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (dialog, cx) = cx.add_window_view(|_, cx| DiffDialog::new(data, DiffViewOptions::default(), cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&dialog, move |_, event: &DiffDialogEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        let (left, right) = (left.to_owned(), right.to_owned());
        dialog.update(cx, |dialog, cx| dialog.loaded(&left, &right, false, cx));
        cx.run_until_parked();
        (dialog, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let centre = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
        cx.simulate_click(centre, Modifiers::default());
        cx.run_until_parked();
    }

    // src/features/vcs/__tests__/diffDialog.test.tsx › Commit dialog diff window (#1863) › keeps the changes tree
    // mounted underneath and closes on Esc
    #[gpui_kit::test]
    async fn keeps_the_changes_tree_mounted_underneath_and_closes_on_esc(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, diff("/repo/a.ts"), "a\n", "b\n");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.title()), "a.ts (HEAD ↔ Your version)");
        assert!(cx.debug_bounds("diff-toolbar").is_some() && cx.debug_bounds("diff-pane-right").is_some());
        // A diff of one file has no files list.
        assert!(cx.debug_bounds("diff-files").is_none());
        let viewer = dialog.read_with(cx, |dialog, _| dialog.viewer().clone());
        cx.update(|window, cx| {
            let focus = viewer.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().last(), Some(&DiffDialogEvent::Close));
        events.borrow_mut().clear();
        click(cx, "diff-dialog-close");
        assert_eq!(*events.borrow(), [DiffDialogEvent::Close]);
    }

    // src/features/vcs/__tests__/diffDialog.test.tsx › Commit dialog diff window (#1863) › moves along the chain
    // inside the dialog instead of opening editor tabs
    #[gpui_kit::test]
    async fn moves_along_the_chain_inside_the_dialog_instead_of_opening_editor_tabs(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, chain(0), "same\nold\n", "same\nnew\n");
        let viewer = dialog.read_with(cx, |dialog, _| dialog.viewer().clone());
        for _ in 0..3 {
            viewer.update(cx, |viewer, cx| viewer.next_difference(cx));
        }
        let asked = events.borrow().iter().find_map(|event| match event {
            DiffDialogEvent::FindFile { from, direction } => Some((from.path.clone(), *direction)),
            _ => None,
        });
        assert_eq!(asked, Some(("/repo/src/a.ts".to_owned(), Direction::Next)));
        // The window shows the file the search found, in place.
        let next = chain_neighbour(&chain(0), Direction::Next).expect("the chain goes on");
        dialog.update(cx, |dialog, cx| dialog.show(next.clone(), cx));
        assert_eq!(events.borrow().last(), Some(&DiffDialogEvent::Load(Box::new(next.clone()))));
        dialog.update(cx, |dialog, cx| dialog.loaded("x\n", "y\n", false, cx));
        assert_eq!(next.reveal, Some(Reveal::First));
        assert_eq!(viewer.read_with(cx, |viewer, _| viewer.current_difference()), Some(0));
    }

    // src/features/vcs/__tests__/diffDialog.test.tsx › Commit dialog diff window (#1863) › closes on Jump to Source
    // so the editor it opens is not hidden behind it
    #[gpui_kit::test]
    async fn closes_on_jump_to_source_so_the_editor_it_opens_is_not_hidden_behind_it(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, diff("/repo/a.ts"), "a\n", "b\n");
        let viewer = dialog.read_with(cx, |dialog, _| dialog.viewer().clone());
        let offered = viewer.read_with(cx, |viewer, cx| {
            viewer.toolbar(cx).iter().any(|item| item.button().is_some_and(|button| button.id == "diff-jump-to-source"))
        });
        assert!(offered, "the working copy is a file to jump to");
        click(cx, "diff-jump-to-source");
        let events = events.borrow();
        assert!(matches!(events.first(), Some(DiffDialogEvent::Viewer(DiffViewEvent::JumpToSource { .. }))));
        assert_eq!(events.last(), Some(&DiffDialogEvent::Close));
    }

    // The files of the chain are listed under the directory they share; choosing one shows it in the window.
    #[gpui_kit::test]
    async fn lists_the_files_of_the_chain_and_shows_the_one_chosen(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, chain(0), "a\n", "b\n");
        let rows: Vec<(usize, String, Option<usize>)> = dialog
            .read_with(cx, |dialog, _| dialog.files().to_vec())
            .into_iter()
            .map(|row| (row.depth, row.name, row.index))
            .collect();
        assert_eq!(
            rows,
            [
                (0, "src".to_owned(), None),
                (1, "lib".to_owned(), None),
                (2, "b.ts".to_owned(), Some(1)),
                (1, "a.ts".to_owned(), Some(0)),
                (0, "README.md".to_owned(), Some(2)),
            ]
        );
        assert!(cx.debug_bounds("diff-files").is_some());
        click(cx, "diff-file-2");
        let shown = dialog.read_with(cx, |dialog, _| dialog.data().path.clone());
        assert_eq!(shown, "/repo/README.md");
        assert!(matches!(events.borrow().last(), Some(DiffDialogEvent::Load(data)) if data.path == "/repo/README.md"));
        // The viewer's events reach the window.
        let viewer = dialog.read_with(cx, |dialog, _| dialog.viewer().clone());
        viewer.update(cx, |viewer, cx| viewer.run_command(DiffCommand::Refresh, cx));
        assert_eq!(events.borrow().last(), Some(&DiffDialogEvent::Viewer(DiffViewEvent::Refresh)));
    }

    // Not from a spec: the directory shared by every path.
    #[test]
    fn finds_the_directory_every_path_is_in() {
        assert_eq!(common_dir(["/repo/src/a.ts", "/repo/src/lib/b.ts"].into_iter()), "/repo/src");
        assert_eq!(common_dir(["/repo/a.ts", "/other/b.ts"].into_iter()), "");
        assert_eq!(common_dir(std::iter::empty()), "");
    }
}
