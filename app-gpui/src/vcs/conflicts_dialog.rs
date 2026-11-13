//! The Conflicts dialog, as IntelliJ shows it once a merge, rebase, cherry-pick or unstash stops on
//! conflicts: the conflicted files (name in the conflict colour, folder beside it), Accept Yours and
//! Accept Theirs for the selected files, Merge… to resolve one of them in the merge editor, and
//! Close. Once every conflict is resolved the dialog concludes the stopped operation: Commit… for a
//! merge, Continue for the others; a stopped rebase, cherry-pick or revert whose last conflict is
//! resolved while the dialog is open goes on by itself, and the dialog closes.
//!
//! The dialog owns no backend: the workbench hands it the conflicted files with
//! [`ConflictsDialog::set_input`] whenever the repository is read again, and carries out what the
//! dialog asks through [`ConflictsDialogEvent`].

use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::input;
use crate::theme;
use crate::theme::icons::file_icon_name;
use crate::vcs::changes::{EffectiveKind, FileStatus, base_name, dir_name, effective_kind, rel_to, strip_slash};
use crate::vcs_ops::operations::{Control, InProgress, control_label, operation_title};
use gpui_kit::{
    App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, Keystroke,
    MouseButton, Render, SharedString, Window, div, prelude::*, px,
};

const WIDTH: f32 = 600.0;
const LIST_HEIGHT: f32 = 240.0;
const ROW_HEIGHT: f32 = 22.0;
const BUTTON_HEIGHT: f32 = 28.0;

/// What the dialog lists: the repository's conflicted files and the operation stopped on them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConflictsInput {
    /// The repository status; only its conflicted files are listed, in its order.
    pub files: Vec<FileStatus>,
    /// The project root the folders are shown relative to.
    pub root: String,
    /// The operation in progress, if any.
    pub operation: Option<InProgress>,
    /// The branch or commit the operation stands on, for the footer.
    pub head: Option<String>,
}

/// How the stopped operation is concluded once its conflicts are resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conclusion {
    /// A merge is concluded by committing (the Commit tool window).
    Commit,
    /// The other operations continue.
    Continue(InProgress),
}

/// What the dialog asks of the workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictsDialogEvent {
    /// Accept Yours (`theirs` false) or Accept Theirs for these files: write that side's version of
    /// each (delete the file when that side deleted it) and stage it.
    Accept { paths: Vec<String>, theirs: bool },
    /// Merge…: close the dialog and open the merge editor for this file. A stopped rebase,
    /// cherry-pick or revert still continues by itself once its last conflict is resolved.
    Merge { path: String },
    /// Commit… or Continue: close the dialog and conclude the operation.
    Conclude(Conclusion),
    /// Close the dialog. `continue_when_resolved` is the operation to continue by itself once its
    /// last conflict is resolved: `None` when the user closed the dialog (the operation stays
    /// stopped), the operation when the dialog closed because its last conflict was resolved.
    Closed { continue_when_resolved: Option<InProgress> },
}

/// A listed file as the dialog shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictRow {
    pub path: String,
    pub name: String,
    /// The folder, relative to the project root.
    pub dir: String,
    pub icon: &'static str,
    pub selected: bool,
    /// The row the keyboard is on.
    pub active: bool,
}

/// A button of the dialog, in the order they are drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictsButton {
    pub id: &'static str,
    /// English source text.
    pub label: String,
    /// The default button: filled, and pressed by Enter.
    pub primary: bool,
    pub enabled: bool,
}

pub const CONCLUDE: &str = "conclude";
pub const ACCEPT_YOURS: &str = "acceptYours";
pub const ACCEPT_THEIRS: &str = "acceptTheirs";
pub const MERGE: &str = "merge";
pub const CLOSE: &str = "close";

pub struct ConflictsDialog {
    focus: FocusHandle,
    input: ConflictsInput,
    /// The conflicted files of the input, stripped of a trailing `/`.
    paths: Vec<String>,
    /// The selected files, by path.
    selected: Vec<String>,
    /// The row the keyboard is on, and the end a Shift selection extends from.
    active: usize,
    anchor: usize,
    key_claim: u64,
}

impl EventEmitter<ConflictsDialogEvent> for ConflictsDialog {}

fn conflicted_paths(files: &[FileStatus]) -> Vec<String> {
    files
        .iter()
        .filter(|file| effective_kind(file) == Some(EffectiveKind::Conflicted))
        .map(|file| strip_slash(&file.path).to_owned())
        .collect()
}

/// A button: filled when `primary`, outlined otherwise, greyed out when it cannot be pressed.
fn button_face(cx: &App, label: String, primary: bool, enabled: bool) -> Div {
    let face = div()
        .flex_none()
        .h(px(BUTTON_HEIGHT))
        .min_w(px(72.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .whitespace_nowrap()
        .child(SharedString::from(label));
    let face = if primary {
        let fill = theme::color(cx, "bg-button-primary");
        face.bg(fill).border_color(fill).text_color(theme::color(cx, "fg-button-primary"))
    } else {
        face.bg(theme::color(cx, "bg-button"))
            .border_color(theme::color(cx, "border-button"))
            .text_color(theme::color(cx, "fg-strong"))
    };
    if enabled { face } else { face.opacity(0.5) }
}

/// "1 file with conflicts", "3 files with conflicts", or "All conflicts resolved".
pub fn heading(count: usize, i18n: &mut I18n) -> String {
    match count {
        0 => i18n.tx("All conflicts resolved"),
        1 => i18n.tx_built(&["", " file with conflicts"], &[Param::from(count)]),
        _ => i18n.tx_built(&["", " files with conflicts"], &[Param::from(count)]),
    }
}

impl ConflictsDialog {
    /// The dialog over `input`, the first file selected.
    pub fn new(start: ConflictsInput, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let weak = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| weak.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        let paths = conflicted_paths(&start.files);
        let selected = paths.first().cloned().into_iter().collect();
        Self { focus, input: start, paths, selected, active: 0, anchor: 0, key_claim }
    }

    /// The repository was read again. The selection keeps the files still conflicted; the last
    /// conflict resolved while a rebase, cherry-pick or revert is stopped closes the dialog, which
    /// then lets the operation continue.
    pub fn set_input(&mut self, input: ConflictsInput, cx: &mut Context<Self>) {
        let had_conflicts = !self.paths.is_empty();
        let active_path = self.paths.get(self.active).cloned();
        self.paths = conflicted_paths(&input.files);
        self.input = input;
        self.selected.retain(|path| self.paths.contains(path));
        self.active = active_path
            .and_then(|path| self.paths.iter().position(|p| *p == path))
            .unwrap_or(self.active)
            .min(self.paths.len().saturating_sub(1));
        self.anchor = self.anchor.min(self.paths.len().saturating_sub(1));
        if self.selected.is_empty()
            && let Some(path) = self.paths.get(self.active)
        {
            self.selected.push(path.clone());
        }
        cx.notify();
        if had_conflicts
            && self.paths.is_empty()
            && let Some(op) = self.continue_when_resolved()
        {
            cx.emit(ConflictsDialogEvent::Closed { continue_when_resolved: Some(op) });
        }
    }

    /// The stopped operation that goes on by itself once the conflicts resolved from this dialog
    /// (here or in the merge editor it opens) are all resolved: a rebase, cherry-pick or revert. A
    /// merge is concluded by committing.
    pub fn continue_when_resolved(&self) -> Option<InProgress> {
        self.input.operation.filter(|op| *op != InProgress::Merge)
    }

    /// What concludes the operation once no conflict is left; `None` while some are, or outside an
    /// operation.
    pub fn conclusion(&self) -> Option<Conclusion> {
        let op = self.input.operation?;
        if !self.paths.is_empty() {
            return None;
        }
        Some(if op == InProgress::Merge { Conclusion::Commit } else { Conclusion::Continue(op) })
    }

    pub fn rows(&self) -> Vec<ConflictRow> {
        self.paths
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let name = base_name(path).to_owned();
                ConflictRow {
                    path: path.clone(),
                    icon: file_icon_name(&name),
                    dir: dir_name(rel_to(&self.input.root, path)).to_owned(),
                    name,
                    selected: self.selected.contains(path),
                    active: index == self.active,
                }
            })
            .collect()
    }

    /// The selected files, in the order they are listed.
    pub fn selected(&self) -> Vec<String> {
        self.paths.iter().filter(|path| self.selected.contains(path)).cloned().collect()
    }

    /// The file Merge… opens: the row the keyboard is on when it is selected, else the first selected.
    fn merge_target(&self) -> Option<String> {
        match self.paths.get(self.active) {
            Some(path) if self.selected.contains(path) => Some(path.clone()),
            _ => self.selected().into_iter().next(),
        }
    }

    /// The buttons: Commit… / Continue once everything is resolved, Accept Yours, Accept Theirs,
    /// Merge… (the default until then) and Close.
    pub fn buttons(&self) -> Vec<ConflictsButton> {
        let conclusion = self.conclusion();
        let any = !self.selected.is_empty();
        let mut out = Vec::with_capacity(5);
        if let Some(conclusion) = conclusion {
            let label = match conclusion {
                Conclusion::Commit => "Commit…".to_owned(),
                Conclusion::Continue(op) => control_label(op, Control::Continue),
            };
            out.push(ConflictsButton { id: CONCLUDE, label, primary: true, enabled: true });
        }
        let button =
            |id, label: &str, primary, enabled| ConflictsButton { id, label: label.to_owned(), primary, enabled };
        out.push(button(ACCEPT_YOURS, "Accept Yours", false, any));
        out.push(button(ACCEPT_THEIRS, "Accept Theirs", false, any));
        out.push(button(MERGE, "Merge…", conclusion.is_none(), any));
        out.push(button(CLOSE, "Close", false, true));
        out
    }

    /// Presses the button `id`, when it can be pressed.
    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.buttons().iter().any(|button| button.id == id && button.enabled) {
            return;
        }
        let event = match id {
            CONCLUDE => match self.conclusion() {
                Some(conclusion) => ConflictsDialogEvent::Conclude(conclusion),
                None => return,
            },
            ACCEPT_YOURS | ACCEPT_THEIRS => {
                ConflictsDialogEvent::Accept { paths: self.selected(), theirs: id == ACCEPT_THEIRS }
            }
            MERGE => match self.merge_target() {
                Some(path) => ConflictsDialogEvent::Merge { path },
                None => return,
            },
            CLOSE => ConflictsDialogEvent::Closed { continue_when_resolved: None },
            _ => return,
        };
        cx.emit(event);
    }

    /// Enter presses the default button.
    fn press_default(&mut self, cx: &mut Context<Self>) {
        if let Some(button) = self.buttons().into_iter().find(|button| button.primary) {
            self.press(button.id, cx);
        }
    }

    /// Selects `index` alone, or adds and removes it (`toggle`), or the rows from the anchor to it
    /// (`extend`).
    fn select(&mut self, index: usize, extend: bool, toggle: bool, cx: &mut Context<Self>) {
        let Some(path) = self.paths.get(index).cloned() else { return };
        if extend {
            let (from, to) = (self.anchor.min(index), self.anchor.max(index));
            self.selected = self.paths[from..=to].to_vec();
        } else if toggle {
            if self.selected.contains(&path) {
                self.selected.retain(|selected| *selected != path);
            } else {
                self.selected.push(path);
            }
            self.anchor = index;
        } else {
            self.selected = vec![path];
            self.anchor = index;
        }
        self.active = index;
        cx.notify();
    }

    fn step(&mut self, delta: isize, extend: bool, cx: &mut Context<Self>) {
        let Some(last) = self.paths.len().checked_sub(1) else { return };
        let to = self.active.saturating_add_signed(delta).min(last);
        self.select(to, extend, false, cx);
    }

    fn select_all(&mut self, cx: &mut Context<Self>) {
        self.selected = self.paths.clone();
        cx.notify();
    }

    fn row_clicked(&mut self, index: usize, event: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let modifiers = event.modifiers();
        if event.click_count() >= 2 && !modifiers.shift && !modifiers.secondary(cx) {
            self.select(index, false, false, cx);
            self.press(MERGE, cx);
            return;
        }
        self.select(index, modifiers.shift, modifiers.secondary(cx), cx);
    }

    /// Up / Down (Shift extends), Home / End, ⌘A, Enter (the default button) and Escape (Close) are
    /// the dialog's own, ahead of the keymap.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = &keystroke.modifiers;
        let primary = modifiers.secondary(cx);
        let plain = !modifiers.control && !modifiers.platform && !modifiers.alt;
        match keystroke.key.as_str() {
            "up" if plain => self.step(-1, modifiers.shift, cx),
            "down" if plain => self.step(1, modifiers.shift, cx),
            "home" if plain => self.select(0, modifiers.shift, false, cx),
            "end" if plain => {
                let last = self.paths.len().saturating_sub(1);
                self.select(last, modifiers.shift, false, cx);
            }
            "a" if primary && !modifiers.shift && !modifiers.alt => self.select_all(cx),
            "enter" if !modifiers.modified() => self.press_default(cx),
            "escape" if !modifiers.modified() => self.press(CLOSE, cx),
            _ => return false,
        }
        true
    }
}

impl Focusable for ConflictsDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ConflictsDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let i18n = &mut cx.default_global::<Localization>().0;
        let title = i18n.tx("Conflicts");
        let heading = heading(self.paths.len(), i18n);
        let empty = i18n.tx("No conflicts");
        let hint = self.input.operation.map(|op| operation_title(op, self.input.head.as_deref(), i18n));
        let (small, row_height) = (theme::length(cx, "fs-small"), px(ROW_HEIGHT));
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let rows: Vec<_> = self
            .rows()
            .into_iter()
            .enumerate()
            .map(|(index, row)| {
                let selector = format!("conflicts-row-{index}");
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .flex_none()
                    .h(row_height)
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .when(row.selected, |el| el.bg(selected_bg))
                    .when(!row.selected, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
                    .child(theme::icon(cx, row.icon, 14.0, theme::color(cx, "fg-muted")))
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .text_color(theme::color(cx, "vcs-conflict"))
                            .child(SharedString::from(row.name)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .text_color(theme::color(cx, "fg-subtle"))
                            .text_size(small)
                            .child(SharedString::from(row.dir)),
                    )
                    .on_click(cx.listener(move |dialog, event: &ClickEvent, window, cx| {
                        dialog.row_clicked(index, event, window, cx)
                    }))
            })
            .collect();
        let list = div()
            .id("conflicts-list")
            .debug_selector(|| "conflicts-list".to_owned())
            .flex_1()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .border_1()
            .border_color(theme::color(cx, "border"))
            .rounded(theme::length(cx, "radius-small"))
            .when(rows.is_empty(), |el| {
                el.items_center()
                    .justify_center()
                    .text_color(theme::color(cx, "fg-subtle"))
                    .child(SharedString::from(empty))
            })
            .children(rows);
        let buttons: Vec<_> = self
            .buttons()
            .into_iter()
            .map(|button| {
                let label = cx.default_global::<Localization>().0.tx(&button.label);
                let id = button.id;
                button_face(cx, label, button.primary, button.enabled)
                    .id(SharedString::from(format!("conflicts-button-{id}")))
                    .debug_selector(move || format!("conflicts-button-{id}"))
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.press(id, cx)))
            })
            .collect();
        div()
            .debug_selector(|| "conflicts-dialog".to_owned())
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .track_focus(&self.focus)
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
                    .pb(px(4.0))
                    .text_color(theme::color(cx, "fg-strong"))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .debug_selector(|| "conflicts-heading".to_owned())
                    .px(px(14.0))
                    .pt(px(10.0))
                    .pb(px(4.0))
                    .text_color(theme::color(cx, "fg-subtle"))
                    .text_size(small)
                    .child(SharedString::from(heading)),
            )
            .child(div().h(px(LIST_HEIGHT)).flex().flex_col().px(px(14.0)).pb(px(14.0)).child(list))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(16.0))
                    .py(px(12.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(theme::color(cx, "fg-subtle"))
                            .text_size(small)
                            .children(hint.map(SharedString::from)),
                    )
                    .children(buttons),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs::changes::ChangeKind;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<ConflictsDialogEvent>>>;

    fn file(rel: &str, unstaged: ChangeKind) -> FileStatus {
        FileStatus {
            path: format!("/p/{rel}"),
            rel_path: rel.to_owned(),
            staged: None,
            unstaged: Some(unstaged),
            renamed_from: None,
        }
    }

    fn input(files: &[&str], operation: Option<InProgress>) -> ConflictsInput {
        let mut status: Vec<FileStatus> = files.iter().map(|rel| file(rel, ChangeKind::Conflicted)).collect();
        status.push(file("src/clean.ts", ChangeKind::Modified));
        ConflictsInput { files: status, root: "/p".to_owned(), operation, head: Some("main".to_owned()) }
    }

    fn open<'a>(
        cx: &'a mut TestAppContext,
        start: ConflictsInput,
    ) -> (Entity<ConflictsDialog>, Events, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (dialog, cx) = cx.add_window_view(move |_, cx| ConflictsDialog::new(start, cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&dialog, move |_, event: &ConflictsDialogEvent, _| seen.borrow_mut().push(event.clone()))
                .detach();
            window.activate_window();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (dialog, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str, modifiers: Modifiers) {
        let centre = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
        cx.simulate_click(centre, modifiers);
        cx.run_until_parked();
    }

    fn selected(dialog: &Entity<ConflictsDialog>, cx: &mut VisualTestContext) -> Vec<String> {
        dialog.read_with(cx, |dialog, _| dialog.selected())
    }

    fn enabled(dialog: &Entity<ConflictsDialog>, cx: &mut VisualTestContext) -> Vec<(&'static str, bool, bool)> {
        dialog.read_with(cx, |dialog, _| {
            dialog.buttons().into_iter().map(|button| (button.id, button.enabled, button.primary)).collect()
        })
    }

    // Not from a spec: the conflicted files are listed with their folder, the first one selected,
    // and Accept Yours / Accept Theirs / Merge… act on the selection.
    #[gpui_kit::test]
    async fn lists_the_conflicted_files_and_accepts_a_side_for_the_selected_ones(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, input(&["src/a.ts", "lib/b.ts", "c.md"], Some(InProgress::Merge)));
        let rows = dialog.read_with(cx, |dialog, _| dialog.rows());
        let shown: Vec<(&str, &str, bool)> =
            rows.iter().map(|row| (row.name.as_str(), row.dir.as_str(), row.selected)).collect();
        assert_eq!(shown, [("a.ts", "src", true), ("b.ts", "lib", false), ("c.md", "", false)]);
        assert_eq!(heading(rows.len(), &mut I18n::default()), "3 files with conflicts");
        assert_eq!(
            enabled(&dialog, cx),
            [(ACCEPT_YOURS, true, false), (ACCEPT_THEIRS, true, false), (MERGE, true, true), (CLOSE, true, false)]
        );

        // ⌘-click adds a file, Shift-click selects the run from the anchor.
        let primary = cx.update(|_, cx| Modifiers::secondary_key(cx));
        click(cx, "conflicts-row-2", primary);
        assert_eq!(selected(&dialog, cx), ["/p/src/a.ts", "/p/c.md"]);
        click(cx, "conflicts-row-1", Modifiers::default());
        click(cx, "conflicts-row-2", Modifiers::shift());
        assert_eq!(selected(&dialog, cx), ["/p/lib/b.ts", "/p/c.md"]);
        let paths = vec!["/p/lib/b.ts".to_owned(), "/p/c.md".to_owned()];
        click(cx, "conflicts-button-acceptTheirs", Modifiers::default());
        let accepted = ConflictsDialogEvent::Accept { paths: paths.clone(), theirs: true };
        assert_eq!(events.borrow().last(), Some(&accepted));
        click(cx, "conflicts-button-acceptYours", Modifiers::default());
        assert_eq!(events.borrow().last(), Some(&ConflictsDialogEvent::Accept { paths, theirs: false }));
    }

    // Not from a spec: Enter and a double click open the merge editor for the file; the keyboard
    // moves and extends the selection; Escape closes the dialog without continuing the operation.
    #[gpui_kit::test]
    async fn opens_the_merge_editor_on_enter_and_on_a_double_click(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, input(&["a.ts", "b.ts", "c.ts"], Some(InProgress::Rebase)));
        cx.simulate_keystrokes("down");
        assert_eq!(selected(&dialog, cx), ["/p/b.ts"]);
        cx.simulate_keystrokes("enter");
        assert_eq!(events.borrow().last(), Some(&ConflictsDialogEvent::Merge { path: "/p/b.ts".to_owned() }));
        cx.simulate_keystrokes("shift-down");
        assert_eq!(selected(&dialog, cx), ["/p/b.ts", "/p/c.ts"]);
        cx.simulate_keystrokes("up");
        assert_eq!(selected(&dialog, cx), ["/p/b.ts"]);

        let row = cx.debug_bounds("conflicts-row-0").expect("the row is drawn").center();
        cx.simulate_event(gpui_kit::MouseDownEvent {
            position: row,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 2,
            first_mouse: false,
        });
        cx.simulate_event(gpui_kit::MouseUpEvent {
            position: row,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 2,
        });
        cx.run_until_parked();
        assert_eq!(events.borrow().last(), Some(&ConflictsDialogEvent::Merge { path: "/p/a.ts".to_owned() }));
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.continue_when_resolved()), Some(InProgress::Rebase));

        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().last(), Some(&ConflictsDialogEvent::Closed { continue_when_resolved: None }));
    }

    // Not from a spec: resolving the last conflict of a stopped rebase closes the dialog and lets the
    // rebase continue; a merge stays open with Commit… as its default button.
    #[gpui_kit::test]
    async fn closes_once_the_last_conflict_is_resolved_or_offers_to_conclude(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, input(&["a.ts", "b.ts"], Some(InProgress::Rebase)));
        dialog.update(cx, |dialog, cx| dialog.set_input(input(&["b.ts"], Some(InProgress::Rebase)), cx));
        assert!(events.borrow().is_empty());
        assert_eq!(selected(&dialog, cx), ["/p/b.ts"]);
        dialog.update(cx, |dialog, cx| dialog.set_input(input(&[], Some(InProgress::Rebase)), cx));
        let closed = ConflictsDialogEvent::Closed { continue_when_resolved: Some(InProgress::Rebase) };
        assert_eq!(*events.borrow(), [closed]);

        let (dialog, events, cx) = open(cx, input(&["a.ts"], Some(InProgress::Merge)));
        dialog.update(cx, |dialog, cx| dialog.set_input(input(&[], Some(InProgress::Merge)), cx));
        cx.run_until_parked();
        assert!(events.borrow().is_empty(), "a merge is concluded by committing, not by closing");
        let heading = heading(0, &mut I18n::default());
        assert_eq!(heading, "All conflicts resolved");
        assert_eq!(
            enabled(&dialog, cx),
            [
                (CONCLUDE, true, true),
                (ACCEPT_YOURS, false, false),
                (ACCEPT_THEIRS, false, false),
                (MERGE, false, false),
                (CLOSE, true, false)
            ]
        );
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.buttons()[0].label.clone()), "Commit…");
        cx.simulate_keystrokes("enter");
        assert_eq!(*events.borrow(), [ConflictsDialogEvent::Conclude(Conclusion::Commit)]);
        // Pressing a disabled button does nothing.
        dialog.update(cx, |dialog, cx| dialog.press(MERGE, cx));
        assert_eq!(events.borrow().len(), 1);
    }

    // Not from a spec: a stopped cherry-pick is concluded with Continue Cherry-Pick once nothing is
    // left, and the footer names the operation.
    #[test]
    fn names_the_continue_button_and_the_operation() {
        assert_eq!(control_label(InProgress::CherryPick, Control::Continue), "Continue Cherry-Pick");
        assert_eq!(heading(1, &mut I18n::default()), "1 file with conflicts");
        assert_eq!(operation_title(InProgress::Rebase, Some("main"), &mut I18n::default()), "Rebasing main");
    }
}
