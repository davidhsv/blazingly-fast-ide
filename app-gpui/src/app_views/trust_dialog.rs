//! "Trust and Open Project?": asked before a project nobody decided about opens. Its three buttons
//! are actions (Preview in Restricted Mode, Don't Open, Trust Project), so the dialog only says
//! which one was pressed; closing it is Don't Open. "Trust projects in <parent>" trusts the
//! folder the project is in as well.

use super::{button, checkbox, footer, frame, path_line, title_row, tx};
use crate::platform::input;
use crate::theme;
use gpui_kit::{
    App, ClickEvent, Context, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Render, SharedString,
    Window, div, prelude::*, px,
};

/// Previews the project with its commands and tools disabled.
pub const PREVIEW_RESTRICTED: &str = "project.previewRestricted";
/// Leaves the project closed, deciding nothing.
pub const DONT_OPEN: &str = "project.dontOpen";
/// Trusts the project (and its folder, when asked) and opens it.
pub const TRUST: &str = "project.trust";

const WIDTH: f32 = 560.0;

/// The buttons in the order they are drawn: their action and label. The first one has the keyboard.
pub const BUTTONS: [(&str, &str); 3] =
    [(PREVIEW_RESTRICTED, "Preview in Restricted Mode"), (DONT_OPEN, "Don't Open"), (TRUST, "Trust Project")];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustDialogEvent {
    /// Run this action: one of [`BUTTONS`].
    Run(&'static str),
}

pub struct TrustDialog {
    /// The project's folder, as the trust store names it (links resolved).
    path: String,
    /// "Trust projects in <parent>".
    folder: bool,
    focus: FocusHandle,
    key_claim: u64,
}

impl EventEmitter<TrustDialogEvent> for TrustDialog {}

/// The folder `path` is in; empty for a root.
fn parent_of(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    match trimmed.rfind(['/', '\\']) {
        Some(0) => &trimmed[..1],
        Some(cut) => &trimmed[..cut],
        None => "",
    }
}

impl TrustDialog {
    pub fn new(path: String, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        Self { path, folder: false, focus, key_claim }
    }

    /// The project's folder.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The folder "Trust projects in" names.
    pub fn parent(&self) -> &str {
        parent_of(&self.path)
    }

    /// Whether "Trust projects in <parent>" is checked.
    pub fn folder(&self) -> bool {
        self.folder
    }

    fn toggle_folder(&mut self, cx: &mut Context<Self>) {
        self.folder = !self.folder;
        cx.notify();
    }

    fn run(&mut self, action: &'static str, cx: &mut Context<Self>) {
        cx.emit(TrustDialogEvent::Run(action));
    }

    /// Enter presses the button that has the keyboard (Preview in Restricted Mode); Escape is
    /// Don't Open.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.run(PREVIEW_RESTRICTED, cx),
            "escape" => self.run(DONT_OPEN, cx),
            _ => return false,
        }
        true
    }
}

impl Focusable for TrustDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TrustDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = tx(cx, "Trust and Open Project?");
        let paragraphs = [
            "If you don't trust the source, preview the project in Restricted Mode, where project commands and \
             tools can't run.",
            "Loading, running or building a project may execute potentially malicious code from its build scripts.",
        ]
        .map(|text| div().child(SharedString::from(tx(cx, text))));
        let folder_label = tx(cx, "Trust projects in");
        // A long parent folder is cut instead of pushing the buttons out of the dialog.
        let folder = checkbox(cx, folder_label, self.folder)
            .flex_shrink(1.0)
            .min_w_0()
            .overflow_hidden()
            .id("trust-folder")
            .debug_selector(|| "trust-folder".to_owned())
            .child(path_line(cx, parent_of(&self.path)))
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.toggle_folder(cx)));
        let buttons = BUTTONS
            .into_iter()
            .map(|(action, label)| {
                let label = tx(cx, label);
                button(cx, label, action == TRUST, true)
                    .id(SharedString::from(format!("trust-button-{action}")))
                    .debug_selector(move || format!("trust-button-{action}"))
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.run(action, cx)))
            })
            .collect();
        frame(cx, WIDTH)
            .track_focus(&self.focus)
            .child(title_row(cx, title))
            .child(
                div()
                    .px(px(16.0))
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .text_color(theme::color(cx, "fg"))
                    .child(path_line(cx, &self.path))
                    .children(paragraphs),
            )
            .child(footer(Some(folder), buttons))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Runs = Rc<RefCell<Vec<&'static str>>>;

    fn open<'a>(cx: &'a mut TestAppContext, path: &str) -> (Entity<TrustDialog>, Runs, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let path = path.to_owned();
        let (dialog, cx) = cx.add_window_view(move |window, cx| TrustDialog::new(path, window, cx));
        let runs: Runs = Rc::default();
        let log = Rc::clone(&runs);
        cx.update(|window, cx| {
            cx.subscribe(&dialog, move |_, event: &TrustDialogEvent, _| {
                let TrustDialogEvent::Run(action) = *event;
                log.borrow_mut().push(action);
            })
            .detach();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (dialog, runs, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    async fn each_button_runs_its_action_and_escape_is_dont_open(cx: &mut TestAppContext) {
        let (_dialog, runs, cx) = open(cx, "/projects/new");
        click(cx, "trust-button-project.previewRestricted");
        click(cx, "trust-button-project.dontOpen");
        click(cx, "trust-button-project.trust");
        cx.simulate_keystrokes("escape");
        cx.simulate_keystrokes("enter");
        assert_eq!(*runs.borrow(), [PREVIEW_RESTRICTED, DONT_OPEN, TRUST, DONT_OPEN, PREVIEW_RESTRICTED]);
    }

    #[gpui_kit::test]
    async fn the_checkbox_names_the_parent_folder_and_toggles(cx: &mut TestAppContext) {
        let (dialog, runs, cx) = open(cx, "/real/projects/new");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.parent().to_owned()), "/real/projects");
        assert!(!dialog.read_with(cx, |dialog, _| dialog.folder()));
        click(cx, "trust-folder");
        assert!(dialog.read_with(cx, |dialog, _| dialog.folder()));
        click(cx, "trust-folder");
        assert!(!dialog.read_with(cx, |dialog, _| dialog.folder()));
        assert!(runs.borrow().is_empty(), "the checkbox runs nothing");
    }

    #[gpui_kit::test]
    async fn a_long_parent_folder_is_cut_and_keeps_the_buttons_inside_the_dialog(cx: &mut TestAppContext) {
        let long = format!("/{}/new", ["a-folder-with-a-rather-long-name"; 12].join("/"));
        let (_dialog, runs, cx) = open(cx, &long);
        for selector in
            ["trust-button-project.previewRestricted", "trust-button-project.dontOpen", "trust-button-project.trust"]
        {
            let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
            assert!(
                bounds.right() <= px(WIDTH),
                "{selector} ends at {:?}, past the dialog's {WIDTH}px",
                bounds.right()
            );
        }
        click(cx, "trust-button-project.trust");
        assert_eq!(*runs.borrow(), [TRUST]);
    }

    #[test]
    fn the_parent_of_a_folder_at_the_root_is_the_root() {
        assert_eq!(parent_of("/new"), "/");
        assert_eq!(parent_of("/a/b/"), "/a");
        assert_eq!(parent_of("C:\\work\\p"), "C:\\work");
        assert_eq!(parent_of("p"), "");
    }
}
