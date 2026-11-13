//! Git Reset for the Git log's Reset Current Branch to Here…: the commit the current branch goes to
//! and the four modes of `git reset`, each with what it does to the files and the index. Mixed is
//! chosen first; Reset answers the chosen mode, Cancel and Escape answer nothing.
//!
//! The dialog runs nothing: the answer goes to the workbench, which resets the branch.

use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::theme;
use gpui_kit::{
    App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, Keystroke,
    MouseButton, Render, SharedString, Window, div, prelude::*, px,
};
use ide_vcs::ResetMode;

/// The ids of the dialog's buttons.
pub const RESET: &str = "reset";
pub const CANCEL: &str = "cancel";
const WIDTH: f32 = 520.0;

/// A mode of the dialog: its label and what it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResetModeOption {
    pub mode: ResetMode,
    pub label: &'static str,
    pub hint: &'static str,
}

/// The modes, in the order the dialog lists them.
pub const RESET_MODES: [ResetModeOption; 4] = [
    ResetModeOption {
        mode: ResetMode::Soft,
        label: "Soft",
        hint: "Files won't change, differences will be staged for commit.",
    },
    ResetModeOption {
        mode: ResetMode::Mixed,
        label: "Mixed",
        hint: "Files won't change, differences won't be staged.",
    },
    ResetModeOption {
        mode: ResetMode::Hard,
        label: "Hard",
        hint: "Files will be reverted to the state of the selected commit. Warning: any local changes will be lost.",
    },
    ResetModeOption {
        mode: ResetMode::Keep,
        label: "Keep",
        hint: "Files will be reverted to the state of the selected commit, but local changes will be kept intact.",
    },
];

/// The mode as `git reset` names it, for the success message ("Reset main to 1234abcd (hard)").
pub fn mode_name(mode: ResetMode) -> &'static str {
    match mode {
        ResetMode::Soft => "soft",
        ResetMode::Mixed => "mixed",
        ResetMode::Hard => "hard",
        ResetMode::Keep => "keep",
    }
}

/// What the dialog asks of the workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResetDialogEvent {
    /// The dialog is done: the mode to reset with, `None` when it was cancelled.
    Closed(Option<ResetMode>),
}

pub struct ResetDialog {
    /// The commit's short hash and subject.
    short_id: String,
    summary: String,
    mode: ResetMode,
    focus: FocusHandle,
    key_claim: u64,
}

impl EventEmitter<ResetDialogEvent> for ResetDialog {}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// A button: filled when `primary`, outlined otherwise.
fn button(cx: &App, label: String, primary: bool) -> Div {
    let face = div()
        .flex_none()
        .h(px(28.0))
        .min_w(px(72.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .whitespace_nowrap()
        .child(SharedString::from(label));
    if primary {
        let fill = theme::color(cx, "bg-button-primary");
        face.bg(fill).border_color(fill).text_color(theme::color(cx, "fg-button-primary"))
    } else {
        face.bg(theme::color(cx, "bg-button"))
            .border_color(theme::color(cx, "border-button"))
            .text_color(theme::color(cx, "fg-strong"))
    }
}

impl ResetDialog {
    /// The dialog for resetting the current branch to the commit `short_id` "`summary`".
    pub fn new(short_id: &str, summary: &str, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        Self { short_id: short_id.to_owned(), summary: summary.to_owned(), mode: ResetMode::Mixed, focus, key_claim }
    }

    /// The chosen mode.
    pub fn mode(&self) -> ResetMode {
        self.mode
    }

    /// The commit line under the explanation: its short hash and subject.
    pub fn commit_line(&self) -> String {
        format!("{} {}", self.short_id, self.summary)
    }

    /// The modes as listed, each with whether it is the chosen one.
    pub fn rows(&self) -> Vec<(ResetMode, bool)> {
        RESET_MODES.iter().map(|option| (option.mode, option.mode == self.mode)).collect()
    }

    pub fn set_mode(&mut self, mode: ResetMode, cx: &mut Context<Self>) {
        self.mode = mode;
        cx.notify();
    }

    /// Presses the button `id` ([`RESET`] or [`CANCEL`]).
    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        let answer = (id == RESET).then_some(self.mode);
        cx.emit(ResetDialogEvent::Closed(answer));
    }

    /// The mode above or below the chosen one (the arrow keys of a radio group).
    fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let at = RESET_MODES.iter().position(|option| option.mode == self.mode).unwrap_or(0);
        let next = if forward { (at + 1).min(RESET_MODES.len() - 1) } else { at.saturating_sub(1) };
        self.set_mode(RESET_MODES[next].mode, cx);
    }

    /// Enter resets, Escape cancels, the arrows move between the modes: the dialog's own keys,
    /// ahead of the keymap.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.press(RESET, cx),
            "escape" => self.press(CANCEL, cx),
            "down" => self.step(true, cx),
            "up" => self.step(false, cx),
            _ => return false,
        }
        true
    }
}

impl Focusable for ResetDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ResetDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, strong, muted) = (theme::color(cx, "fg"), theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let accent = theme::color(cx, "bg-button-primary");
        let title = tx(cx, "Git Reset");
        let explanation = tx(
            cx,
            concat!(
                "This will reset the current branch head to the selected commit, and update the working tree ",
                "and the index according to the selected mode:"
            ),
        );
        let modes: Vec<_> = RESET_MODES
            .iter()
            .enumerate()
            .map(|(index, option)| {
                let chosen = option.mode == self.mode;
                let (label, hint) = (tx(cx, option.label), tx(cx, option.hint));
                let mode = option.mode;
                let dot = div()
                    .flex_none()
                    .mt(px(2.0))
                    .size(px(14.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_color(if chosen { accent } else { theme::color(cx, "border-checkbox") })
                    .bg(theme::color(cx, "bg-checkbox"))
                    .children(chosen.then(|| div().size(px(6.0)).rounded_full().bg(accent)));
                div()
                    .id(SharedString::from(format!("reset-mode-{index}")))
                    .debug_selector(move || format!("reset-mode-{index}"))
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    .child(dot)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().text_color(strong).child(SharedString::from(label)))
                            .child(div().text_color(muted).child(SharedString::from(hint))),
                    )
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.set_mode(mode, cx)))
            })
            .collect();
        let buttons: Vec<_> = [(RESET, "Reset", true), (CANCEL, "Cancel", false)]
            .into_iter()
            .map(|(id, label, primary)| {
                let label = tx(cx, label);
                button(cx, label, primary)
                    .id(SharedString::from(format!("reset-button-{id}")))
                    .debug_selector(move || format!("reset-button-{id}"))
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.press(id, cx)))
            })
            .collect();
        div()
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(fg)
            .track_focus(&self.focus)
            // A click on the dialog is not a click beside it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_color(strong)
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
                    .child(div().text_color(muted).child(SharedString::from(explanation)))
                    .child(div().text_color(strong).child(SharedString::from(self.commit_line())))
                    .children(modes),
            )
            .child(div().flex().justify_end().gap(px(8.0)).px(px(16.0)).py(px(12.0)).children(buttons))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Answers = Rc<RefCell<Vec<Option<ResetMode>>>>;

    fn open(cx: &mut TestAppContext) -> (Entity<ResetDialog>, Answers, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (dialog, cx) = cx.add_window_view(|_, cx| ResetDialog::new("1234abcd", "Fix the parser", cx));
        let answers: Answers = Rc::default();
        let log = Rc::clone(&answers);
        cx.update(|window, cx| {
            cx.subscribe(&dialog, move |_, event: &ResetDialogEvent, _| {
                let ResetDialogEvent::Closed(answer) = event;
                log.borrow_mut().push(*answer);
            })
            .detach();
            window.activate_window();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (dialog, answers, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    // Not from a spec: Mixed is chosen first, the arrows and a click choose another mode, and Enter
    // resets with the chosen one.
    #[gpui_kit::test]
    async fn mixed_is_chosen_first_and_reset_answers_the_chosen_mode(cx: &mut TestAppContext) {
        let (dialog, answers, cx) = open(cx);
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.commit_line()), "1234abcd Fix the parser");
        let chosen = |cx: &mut VisualTestContext| dialog.read_with(cx, |dialog, _| dialog.mode());
        assert_eq!(chosen(cx), ResetMode::Mixed);
        let modes: Vec<ResetMode> =
            dialog.read_with(cx, |dialog, _| dialog.rows().into_iter().map(|(mode, _)| mode).collect());
        assert_eq!(modes, [ResetMode::Soft, ResetMode::Mixed, ResetMode::Hard, ResetMode::Keep]);
        cx.simulate_keystrokes("down");
        assert_eq!(chosen(cx), ResetMode::Hard);
        click(cx, "reset-mode-3");
        assert_eq!(chosen(cx), ResetMode::Keep);
        cx.simulate_keystrokes("enter");
        assert_eq!(answers.borrow().as_slice(), [Some(ResetMode::Keep)]);
        assert_eq!(mode_name(ResetMode::Keep), "keep");
    }

    // Not from a spec: Cancel and Escape answer nothing.
    #[gpui_kit::test]
    async fn cancel_and_escape_answer_nothing(cx: &mut TestAppContext) {
        let (_dialog, answers, cx) = open(cx);
        click(cx, "reset-button-cancel");
        cx.simulate_keystrokes("escape");
        assert_eq!(answers.borrow().as_slice(), [None, None]);
    }
}
