//! Interactively Rebase from Here…: the commits from the chosen one to HEAD, oldest first as they
//! are applied, each with what the rebase does with it (Pick, Reword, Stop to Edit, Squash, Fixup,
//! Drop). `fixup!` and `squash!` commits start next to their targets. A row moves up and down; a
//! reworded commit gets its new message in an area of its own. Start Rebasing needs a plan that
//! can run and changes something; the problem of the plan shows beside the buttons.
//!
//! The dialog runs nothing: the plan goes to the workbench, which rebases.

use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::input;
use crate::theme;
use crate::ui::text_area::{TextArea, TextAreaEvent};
use crate::vcs_ops::rebase_plan::{RebaseRow, initial_rows, move_row, plan_is_noop, plan_problem, to_entries};
use gpui_kit::{
    App, AppContext, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement,
    Keystroke, MouseButton, Render, SharedString, Stateful, Subscription, Window, div, prelude::*, px,
};
use ide_vcs::{Commit, RebaseAction, RebaseEntry};
use std::collections::HashMap;

/// The ids of the dialog's buttons.
pub const START: &str = "start";
pub const CANCEL: &str = "cancel";
const WIDTH: f32 = 720.0;
const ACTION_WIDTH: f32 = 120.0;
/// Where the rows start below the top of the dialog, and the height of a row without a message area.
const CHOOSER_TOP: f32 = 64.0;
const ROW_HEIGHT: f32 = 30.0;

/// The actions a row can have, in the order the chooser lists them, with their labels.
pub const ACTIONS: [(RebaseAction, &str); 6] = [
    (RebaseAction::Pick, "Pick"),
    (RebaseAction::Reword, "Reword"),
    (RebaseAction::Edit, "Stop to Edit"),
    (RebaseAction::Squash, "Squash"),
    (RebaseAction::Fixup, "Fixup"),
    (RebaseAction::Drop, "Drop"),
];

fn action_label(action: RebaseAction) -> &'static str {
    ACTIONS.iter().find(|(known, _)| *known == action).map_or("Pick", |(_, label)| label)
}

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

/// The dialog's title: how many commits are rebased, onto which commit or from the root.
pub fn rebase_title(count: usize, onto: Option<&str>, i18n: &mut I18n) -> String {
    match onto {
        Some(onto) => {
            let strings: [&str; 3] =
                if count == 1 { ["Rebasing ", " commit onto ", ""] } else { ["Rebasing ", " commits onto ", ""] };
            i18n.tx_built(&strings, &[Param::from(count), Param::from(short(onto).as_str())])
        }
        None => {
            let strings: [&str; 2] = if count == 1 {
                ["Rebasing ", " commit from the root"]
            } else {
                ["Rebasing ", " commits from the root"]
            };
            i18n.tx_built(&strings, &[Param::from(count)])
        }
    }
}

/// What the dialog asks of the workbench.
#[derive(Clone, Debug)]
pub enum InteractiveRebaseEvent {
    /// The dialog is done: the plan to rebase with, oldest first; `None` when it was cancelled.
    Closed(Option<Vec<RebaseEntry>>),
}

/// A row as shown: the commit, its action and what stands beside it (its subject, or the new
/// message of a reworded commit).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RebaseRowLine {
    pub id: String,
    pub short_id: String,
    pub action: RebaseAction,
    pub text: String,
}

pub struct InteractiveRebaseDialog {
    /// The commits rebased, oldest first, as they were.
    commits: Vec<Commit>,
    /// The parent of the oldest commit; `None` rebases from the root.
    base: Option<String>,
    rows: Vec<RebaseRow>,
    /// The message areas of the reworded commits, by commit id.
    areas: HashMap<String, (Entity<TextArea>, Subscription)>,
    /// The row whose action chooser is open.
    choosing: Option<usize>,
    focus: FocusHandle,
    key_claim: u64,
}

impl EventEmitter<InteractiveRebaseEvent> for InteractiveRebaseDialog {}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// A button: filled when `primary`, outlined otherwise, greyed out when it cannot be pressed.
fn button(cx: &App, label: String, primary: bool, enabled: bool) -> Div {
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

impl InteractiveRebaseDialog {
    /// The dialog for rebasing `commits` (oldest first) from `base`.
    pub fn new(commits: Vec<Commit>, base: Option<String>, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        let rows = initial_rows(&commits);
        let mut dialog = Self { commits, base, rows, areas: HashMap::new(), choosing: None, focus, key_claim };
        dialog.sync_areas(cx);
        dialog
    }

    pub fn title(&self, cx: &mut App) -> String {
        rebase_title(self.commits.len(), self.base.as_deref(), &mut cx.default_global::<Localization>().0)
    }

    /// The rows from top to bottom: the order the commits are applied in.
    pub fn rows(&self) -> Vec<RebaseRowLine> {
        self.rows
            .iter()
            .map(|row| RebaseRowLine {
                id: row.commit.id.clone(),
                short_id: row.commit.short_id.clone(),
                action: row.action,
                text: if row.action == RebaseAction::Reword { row.message.clone() } else { row.commit.summary.clone() },
            })
            .collect()
    }

    /// The message area of a reworded row.
    pub fn message_area(&self, index: usize) -> Option<&Entity<TextArea>> {
        let row = self.rows.get(index)?;
        self.areas.get(&row.commit.id).map(|(area, _)| area)
    }

    /// Why the plan cannot run, in English.
    pub fn problem(&self) -> Option<&'static str> {
        plan_problem(&self.rows)
    }

    /// Start Rebasing can be pressed: the plan can run and changes something.
    pub fn can_start(&self) -> bool {
        self.problem().is_none() && !plan_is_noop(&self.rows, &self.commits)
    }

    /// The row whose action chooser is open.
    pub fn choosing(&self) -> Option<usize> {
        self.choosing
    }

    pub fn set_action(&mut self, index: usize, action: RebaseAction, cx: &mut Context<Self>) {
        self.choosing = None;
        let Some(row) = self.rows.get_mut(index) else { return };
        row.action = action;
        self.sync_areas(cx);
        cx.notify();
    }

    /// Moves row `index` by `delta` places, when it can move.
    pub fn move_row(&mut self, index: usize, delta: isize, cx: &mut Context<Self>) {
        self.choosing = None;
        self.rows = move_row(&self.rows, index, delta);
        cx.notify();
    }

    /// The new message of reworded row `index`.
    pub fn set_message(&mut self, index: usize, message: &str, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get_mut(index) else { return };
        row.message = message.to_owned();
        if let Some((area, _)) = self.areas.get(&row.commit.id) {
            area.update(cx, |area, cx| area.set_text(message, cx));
        }
        cx.notify();
    }

    /// Presses the button `id` ([`START`] or [`CANCEL`]); Start does nothing while it cannot be
    /// pressed.
    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        if id == START {
            if !self.can_start() {
                return;
            }
            cx.emit(InteractiveRebaseEvent::Closed(Some(to_entries(&self.rows))));
        } else {
            cx.emit(InteractiveRebaseEvent::Closed(None));
        }
    }

    fn toggle_chooser(&mut self, index: usize, cx: &mut Context<Self>) {
        self.choosing = if self.choosing == Some(index) { None } else { Some(index) };
        cx.notify();
    }

    /// Every reworded row has its message area, starting from the commit's message; the others have
    /// none.
    fn sync_areas(&mut self, cx: &mut Context<Self>) {
        let reworded: Vec<(String, String)> = self
            .rows
            .iter()
            .filter(|row| row.action == RebaseAction::Reword)
            .map(|row| (row.commit.id.clone(), row.message.clone()))
            .collect();
        self.areas.retain(|id, _| reworded.iter().any(|(kept, _)| kept == id));
        for (id, message) in reworded {
            if self.areas.contains_key(&id) {
                continue;
            }
            let area = cx.new(|cx| {
                let mut area = TextArea::new(cx);
                area.set_rows(2, 6, cx);
                area.set_text(&message, cx);
                area
            });
            let commit = id.clone();
            let events = cx.subscribe(&area, move |dialog: &mut Self, area, event: &TextAreaEvent, cx| {
                if *event != TextAreaEvent::Changed {
                    return;
                }
                let text = area.read(cx).text().to_owned();
                if let Some(row) = dialog.rows.iter_mut().find(|row| row.commit.id == commit) {
                    row.message = text;
                }
                cx.notify();
            });
            self.areas.insert(id, (area, events));
        }
    }

    /// Escape closes the open chooser, else cancels; Enter starts when it can. The dialog's own
    /// keys while it has the keyboard, ahead of the keymap.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "escape" if self.choosing.is_some() => {
                self.choosing = None;
                cx.notify();
            }
            "escape" => self.press(CANCEL, cx),
            "enter" => self.press(START, cx),
            _ => return false,
        }
        true
    }

    fn chooser(&self, index: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let (fg, hover) = (theme::color(cx, "fg"), theme::color(cx, "bg-tree-hover"));
        let options: Vec<_> = ACTIONS
            .iter()
            .enumerate()
            .map(|(option, (action, label))| {
                let action = *action;
                let label = tx(cx, label);
                div()
                    .id(SharedString::from(format!("rebase-action-option-{option}")))
                    .debug_selector(move || format!("rebase-action-option-{option}"))
                    .px(px(8.0))
                    .py(px(3.0))
                    .rounded(px(4.0))
                    .text_color(fg)
                    .hover(move |el| el.bg(hover))
                    .child(SharedString::from(label))
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.set_action(index, action, cx)))
            })
            .collect();
        // Below the row's chooser button: the title, the hint and the rows above it come first.
        let top = CHOOSER_TOP + (index as f32 + 1.0) * ROW_HEIGHT;
        div()
            .id("rebase-action-chooser")
            .absolute()
            .top(px(top))
            .left(px(16.0))
            // The rows below it take no click meant for it.
            .occlude()
            .w(px(ACTION_WIDTH))
            .flex()
            .flex_col()
            .p(px(4.0))
            .rounded(px(6.0))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .bg(theme::color(cx, "bg-popup"))
            .children(options)
    }
}

impl Focusable for InteractiveRebaseDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for InteractiveRebaseDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, strong, muted) = (theme::color(cx, "fg"), theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let title = self.title(cx);
        let hint = tx(cx, "Commits are applied from top to bottom.");
        let last = self.rows.len().saturating_sub(1);
        let lines = self.rows();
        let rows: Vec<_> = lines
            .into_iter()
            .enumerate()
            .map(|(index, line)| {
                let label = tx(cx, action_label(line.action));
                let action = div().flex_none().w(px(ACTION_WIDTH)).child(
                    div()
                        .id(SharedString::from(format!("rebase-action-{index}")))
                        .debug_selector(move || format!("rebase-action-{index}"))
                        .h(px(26.0))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .rounded(theme::length(cx, "radius-small"))
                        .border_1()
                        .border_color(theme::color(cx, "border"))
                        .bg(theme::color(cx, "bg-input"))
                        .child(SharedString::from(label))
                        .child(theme::icon(cx, "chevron-down", 12.0, muted))
                        .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.toggle_chooser(index, cx))),
                );
                let subject = match self.areas.get(&line.id) {
                    Some((area, _)) if line.action == RebaseAction::Reword => div()
                        .flex_1()
                        .min_w_0()
                        .p(px(4.0))
                        .rounded(theme::length(cx, "radius-small"))
                        .border_1()
                        .border_color(theme::color(cx, "border"))
                        .bg(theme::color(cx, "bg-input"))
                        .child(area.clone()),
                    _ => div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .when(line.action == RebaseAction::Drop, |el| el.line_through().text_color(muted))
                        .child(SharedString::from(line.text.clone())),
                };
                let arrow =
                    |icon: &'static str, selector: String, enabled: bool, delta: isize, cx: &mut Context<Self>| {
                        let color = if enabled { fg } else { muted };
                        div()
                            .id(SharedString::from(selector.clone()))
                            .debug_selector(move || selector.clone())
                            .flex_none()
                            .size(px(22.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(!enabled, |el| el.opacity(0.5))
                            .child(theme::icon(cx, icon, 12.0, color))
                            .when(enabled, |el| {
                                el.on_click(
                                    cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.move_row(index, delta, cx)),
                                )
                            })
                    };
                let up = arrow("arrow-up", format!("rebase-up-{index}"), index > 0, -1, cx);
                let down = arrow("arrow-down", format!("rebase-down-{index}"), index < last, 1, cx);
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    .py(px(2.0))
                    .child(action)
                    .child(
                        div()
                            .flex_none()
                            .w(px(72.0))
                            .pt(px(4.0))
                            .text_color(muted)
                            .child(SharedString::from(line.short_id)),
                    )
                    .child(subject)
                    .child(up)
                    .child(down)
            })
            .collect();
        let problem = self.problem().map(|problem| tx(cx, problem));
        let can_start = self.can_start();
        let buttons: Vec<_> = [(START, "Start Rebasing", true), (CANCEL, "Cancel", false)]
            .into_iter()
            .map(|(id, label, primary)| {
                let label = tx(cx, label);
                button(cx, label, primary, id != START || can_start)
                    .id(SharedString::from(format!("rebase-button-{id}")))
                    .debug_selector(move || format!("rebase-button-{id}"))
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.press(id, cx)))
            })
            .collect();
        let chooser = self.choosing.map(|index| self.chooser(index, cx));
        div()
            .relative()
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
            .child(div().px(px(16.0)).pb(px(6.0)).text_color(muted).child(SharedString::from(hint)))
            .child(div().px(px(16.0)).pb(px(12.0)).flex().flex_col().children(rows))
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
                            .text_color(theme::color(cx, "danger"))
                            .children(problem.map(SharedString::from)),
                    )
                    .children(buttons),
            )
            // Drawn last, over the rows below the one it is for.
            .children(chooser)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Answers = Rc<RefCell<Vec<Option<Vec<RebaseEntry>>>>>;

    fn commit(id: &str, parents: &[&str], summary: &str) -> Commit {
        let wire = serde_json::json!({
            "id": id, "shortId": short(id), "parents": parents, "author": "Ada", "email": "ada@example.test",
            "timeMs": 0.0, "summary": summary, "message": format!("{summary}\n"), "refs": [],
        });
        serde_json::from_value(wire).expect("a commit in its wire form")
    }

    fn open(cx: &mut TestAppContext) -> (Entity<InteractiveRebaseDialog>, Answers, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let commits = vec![
            commit("a1a1a1a1a1", &["base0000"], "Add parser"),
            commit("b2b2b2b2b2", &["a1a1a1a1a1"], "Add lexer"),
            commit("c3c3c3c3c3", &["b2b2b2b2b2"], "fixup! Add parser"),
        ];
        let (dialog, cx) =
            cx.add_window_view(move |_, cx| InteractiveRebaseDialog::new(commits, Some("base0000aaaa".to_owned()), cx));
        let answers: Answers = Rc::default();
        let log = Rc::clone(&answers);
        cx.update(|window, cx| {
            cx.subscribe(&dialog, move |_, event: &InteractiveRebaseEvent, _| {
                let InteractiveRebaseEvent::Closed(answer) = event;
                log.borrow_mut().push(answer.clone());
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

    fn shape(dialog: &Entity<InteractiveRebaseDialog>, cx: &mut VisualTestContext) -> Vec<(String, RebaseAction)> {
        dialog.read_with(cx, |dialog, _| dialog.rows().into_iter().map(|row| (row.text, row.action)).collect())
    }

    // Not from a spec: the commits are listed oldest first with a fixup! commit under its target,
    // the title says how many are rebased onto what, and an unchanged plan cannot start.
    #[gpui_kit::test]
    async fn lists_the_commits_oldest_first_with_fixups_under_their_targets(cx: &mut TestAppContext) {
        let (dialog, _answers, cx) = open(cx);
        assert_eq!(dialog.update(cx, |dialog, cx| dialog.title(cx)), "Rebasing 3 commits onto base0000");
        assert_eq!(
            shape(&dialog, cx),
            [
                ("Add parser".to_owned(), RebaseAction::Pick),
                ("fixup! Add parser".to_owned(), RebaseAction::Fixup),
                ("Add lexer".to_owned(), RebaseAction::Pick),
            ]
        );
        assert!(dialog.read_with(cx, |dialog, _| dialog.can_start()), "the autosquashed fixup is a change");
        // The fixup picked and moved back to where it was: nothing would change.
        dialog.update(cx, |dialog, cx| {
            dialog.set_action(1, RebaseAction::Pick, cx);
            dialog.move_row(1, 1, cx);
        });
        assert!(!dialog.read_with(cx, |dialog, _| dialog.can_start()), "nothing would change");
    }

    // Not from a spec: the chooser sets a row's action, the arrows reorder, a reword gets its
    // message area, a squash first is refused, and Start answers the plan.
    #[gpui_kit::test]
    async fn the_chooser_and_the_arrows_make_the_plan_that_start_answers(cx: &mut TestAppContext) {
        let (dialog, answers, cx) = open(cx);
        click(cx, "rebase-action-0");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.choosing()), Some(0));
        click(cx, "rebase-action-option-3");
        assert_eq!(shape(&dialog, cx)[0].1, RebaseAction::Squash);
        assert_eq!(
            dialog.read_with(cx, |dialog, _| dialog.problem()),
            Some("The first commit cannot be squashed into a previous one")
        );
        assert!(!dialog.read_with(cx, |dialog, _| dialog.can_start()));
        click(cx, "rebase-down-0");
        assert_eq!(shape(&dialog, cx)[0].0, "fixup! Add parser");
        dialog.update(cx, |dialog, cx| {
            dialog.set_action(0, RebaseAction::Drop, cx);
            dialog.set_action(1, RebaseAction::Pick, cx);
            dialog.set_action(2, RebaseAction::Reword, cx);
        });
        assert!(dialog.read_with(cx, |dialog, _| dialog.message_area(2).is_some()), "a reword edits its message");
        dialog.update(cx, |dialog, cx| dialog.set_message(2, "Add the lexer", cx));
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.problem()), None);
        click(cx, "rebase-button-start");
        let answers = answers.borrow();
        let plan: Vec<(String, RebaseAction, Option<String>)> = answers[0]
            .as_ref()
            .expect("the plan is answered")
            .iter()
            .map(|entry| (entry.commit.clone(), entry.action, entry.message.clone()))
            .collect();
        assert_eq!(
            plan,
            [
                ("c3c3c3c3c3".to_owned(), RebaseAction::Drop, None),
                ("a1a1a1a1a1".to_owned(), RebaseAction::Pick, None),
                ("b2b2b2b2b2".to_owned(), RebaseAction::Reword, Some("Add the lexer".to_owned())),
            ]
        );
    }
}
