//! Evaluate Expression: a dialog that evaluates an expression in the selected frame of the suspended
//! session and shows its value as the Variables view does (`name = {type} value`).
//!
//! Enter evaluates (while the program is suspended), ⇧⌘Enter (⇧⌃Enter) adds the expression to the
//! watches, ↑ and ↓ walk the project's history of evaluated expressions, Escape closes. Add to Watches
//! adds it and closes.
//!
//! The dialog evaluates nothing itself: it asks the workbench through [`EvaluateDialogEvent`] and is
//! told the answer with [`EvaluateDialog::evaluated`]. The field is one line: an expression seeded
//! from a selection over several lines is joined with spaces.

use crate::app_views;
use crate::debugger::actions::Debugger;
use crate::debugger::state::SessionStatus;
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::platform::storage::UiStorage;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::ui::tooltip::WithTooltip as _;
use crate::vcs::prompt_dialogs::button;
use gpui_kit::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Pixels, Point,
    Render, SharedString, Subscription, Window, div, prelude::*,
};
use ide_dap::Variable;

/// The storage keys of the debugger's expression histories start with this.
const HISTORY_PREFIX: &str = "openidea.debugger.history.";
/// How many expressions a history keeps.
const HISTORY_MAX: usize = 50;
const WIDTH: f32 = 620.0;
/// How many of the history's expressions the History button's menu lists.
const HISTORY_MENU_MAX: usize = 20;

/// The ids of the dialog's buttons, for [`EvaluateDialog::press`].
pub const EVALUATE: &str = "evaluate";
pub const ADD_TO_WATCHES: &str = "addToWatches";
pub const CLOSE: &str = "close";

/// The storage key of the project's history of `kind` (`evaluate`, `condition`, `console`).
pub fn history_key(kind: &str, root: &str) -> String {
    format!("{HISTORY_PREFIX}{kind}:{root}")
}

/// The history stored under `key`, most recent first; a value that is not a list of texts is none.
pub fn read_history(storage: &UiStorage, key: &str) -> Vec<String> {
    let parsed = storage.get(key).and_then(|raw| serde_json::from_str::<Vec<serde_json::Value>>(raw).ok());
    let texts = |items: Vec<serde_json::Value>| items.into_iter().filter_map(|item| item.as_str().map(str::to_owned));
    parsed.map(|items| texts(items).collect()).unwrap_or_default()
}

/// `history` with `expression` (trimmed) first and nowhere else, kept to the last [`HISTORY_MAX`].
/// A blank expression leaves it as it is.
pub fn pushed_history(history: &[String], expression: &str) -> Vec<String> {
    let expression = expression.trim();
    if expression.is_empty() {
        return history.to_vec();
    }
    let rest = history.iter().filter(|known| known.as_str() != expression).cloned();
    std::iter::once(expression.to_owned()).chain(rest).take(HISTORY_MAX).collect()
}

/// `text` in the active locale.
fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// The text of a one-line field: line breaks become spaces.
fn one_line(text: &str) -> String {
    text.split(['\r', '\n']).filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ")
}

/// What an evaluation came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvaluateResult {
    Value { expression: String, value: String, type_name: Option<String> },
    Error { expression: String, message: String },
}

impl EvaluateResult {
    /// The result as the Variables view writes a row: `name = {type} value`, or `name = error`.
    pub fn line(&self) -> String {
        match self {
            Self::Value { expression, value, type_name } => {
                let type_name = type_name.as_ref().map(|t| format!("{{{t}}} ")).unwrap_or_default();
                format!("{expression} = {type_name}{}", one_line(value)).trim_end().to_owned()
            }
            Self::Error { expression, message } => format!("{expression} = {}", one_line(message)),
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }
}

/// What the dialog asks of the workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvaluateDialogEvent {
    /// Evaluate `expression` in the active session; answer with [`EvaluateDialog::evaluated`] and the
    /// same `request`.
    Evaluate { request: u64, expression: String },
    /// Add `expression` to the watches; `close`: and close the dialog.
    AddWatch { expression: String, close: bool },
    /// The History button was pressed at `position`: offer [`EvaluateDialog::history_menu`] there and
    /// answer the choice with [`EvaluateDialog::recall`].
    ShowHistory { position: Point<Pixels> },
    /// Close the dialog.
    Closed,
}

pub struct EvaluateDialog {
    debugger: Entity<Debugger>,
    field: Entity<TextField>,
    /// The project's evaluated expressions, most recent first.
    history: Vec<String>,
    /// Where ↑ and ↓ are in the history; `None`: at the typed text.
    history_pos: Option<usize>,
    result: Option<EvaluateResult>,
    /// The evaluation asked last, with its expression; older answers are dropped.
    pending: Option<(u64, String)>,
    requests: u64,
    /// The field shows the placeholder of a suspended program.
    paused_placeholder: Option<bool>,
    code_font: SharedString,
    key_claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<EvaluateDialogEvent> for EvaluateDialog {}

impl EvaluateDialog {
    /// The dialog over `debugger`'s active session, its field holding `expression` (selected), with
    /// the project's `history`.
    pub fn new(
        debugger: Entity<Debugger>,
        expression: &str,
        history: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let field = cx.new(|cx| {
            let mut field = TextField::new(cx);
            field.set_text(&one_line(expression), cx);
            field.select_all(cx);
            field
        });
        let subscriptions = vec![
            cx.observe(&debugger, |dialog, _, cx| {
                dialog.sync_placeholder(cx);
                cx.notify();
            }),
            cx.subscribe_in(&field, window, |dialog: &mut Self, _, event: &TextFieldEvent, _, cx| match event {
                TextFieldEvent::Changed => {
                    dialog.history_pos = None;
                    cx.notify();
                }
                // Enter is claimed ahead of the field (see `claim_key`).
                TextFieldEvent::Submitted => {}
                TextFieldEvent::Cancelled => dialog.press(CLOSE, cx),
            }),
        ];
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            field.focus_handle(cx),
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        let mut dialog = Self {
            debugger,
            field,
            history,
            history_pos: None,
            result: None,
            pending: None,
            requests: 0,
            paused_placeholder: None,
            code_font: theme::installed_fonts::mono_family(cx),
            key_claim,
            _subscriptions: subscriptions,
        };
        dialog.sync_placeholder(cx);
        dialog
    }

    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    /// The expression in the field.
    pub fn expression(&self, cx: &App) -> String {
        self.field.read(cx).text().to_owned()
    }

    /// The project's evaluated expressions, most recent first.
    pub fn history(&self) -> &[String] {
        &self.history
    }

    pub fn result(&self) -> Option<&EvaluateResult> {
        self.result.as_ref()
    }

    /// The result as the Variables view writes a row, once there is one.
    pub fn result_line(&self) -> Option<String> {
        self.result.as_ref().map(EvaluateResult::line)
    }

    /// The active session is suspended: expressions can be evaluated.
    pub fn paused(&self, cx: &App) -> bool {
        let state = &self.debugger.read(cx).state;
        state.active_session().is_some_and(|session| session.status == SessionStatus::Paused)
    }

    /// Whether Evaluate can be pressed: suspended, and something is typed.
    pub fn can_evaluate(&self, cx: &App) -> bool {
        self.paused(cx) && !self.expression(cx).trim().is_empty()
    }

    /// What the result area says while there is no result, in English.
    pub fn note(&self, cx: &App) -> Option<&'static str> {
        if self.result.is_some() {
            return None;
        }
        Some(if self.paused(cx) {
            "Press Enter to evaluate"
        } else {
            "Evaluation is available while the program is suspended"
        })
    }

    /// The buttons: id, English label, whether they can be pressed.
    pub fn buttons(&self, cx: &App) -> Vec<(&'static str, &'static str, bool)> {
        let typed = !self.expression(cx).trim().is_empty();
        vec![
            (ADD_TO_WATCHES, "Add to Watches", typed),
            (EVALUATE, "Evaluate", self.can_evaluate(cx)),
            (CLOSE, "Close", true),
        ]
    }

    /// Presses the button `id`, when it can be pressed.
    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        match id {
            EVALUATE => self.evaluate(cx),
            ADD_TO_WATCHES => self.add_watch(true, cx),
            CLOSE => cx.emit(EvaluateDialogEvent::Closed),
            _ => {}
        }
    }

    /// The History button's menu: the newest expressions, each on one line.
    pub fn history_menu(&self) -> Vec<(String, String)> {
        let row = |known: &String| (known.split_whitespace().collect::<Vec<_>>().join(" "), known.clone());
        self.history.iter().take(HISTORY_MENU_MAX).map(row).collect()
    }

    /// An expression chosen from the history goes into the field, in place of what was typed.
    pub fn recall(&mut self, expression: &str, cx: &mut Context<Self>) {
        self.history_pos = None;
        self.field.update(cx, |field, cx| {
            field.set_text(&one_line(expression), cx);
            field.select_all(cx);
        });
        cx.notify();
    }

    /// The answer to the evaluation `request`; an answer to an older one is dropped.
    pub fn evaluated(&mut self, request: u64, answer: Result<Variable, String>, cx: &mut Context<Self>) {
        let Some((_, expression)) = self.pending.take_if(|(pending, _)| *pending == request) else { return };
        self.result = Some(match answer {
            Ok(variable) => EvaluateResult::Value { expression, value: variable.value, type_name: variable.type_name },
            Err(message) => EvaluateResult::Error { expression, message },
        });
        cx.notify();
    }

    fn sync_placeholder(&mut self, cx: &mut Context<Self>) {
        let paused = self.paused(cx);
        if self.paused_placeholder == Some(paused) {
            return;
        }
        self.paused_placeholder = Some(paused);
        let text =
            if paused { "Expression to evaluate in the selected frame" } else { "Pause the program to evaluate" };
        let text = cx.default_global::<Localization>().0.tx(text);
        self.field.update(cx, |field, cx| field.set_placeholder(text, cx));
    }

    /// Evaluates the typed expression while the program is suspended; it goes first in the history.
    fn evaluate(&mut self, cx: &mut Context<Self>) {
        let expression = self.expression(cx).trim().to_owned();
        if expression.is_empty() || !self.paused(cx) {
            return;
        }
        self.history = pushed_history(&self.history, &expression);
        self.history_pos = None;
        self.requests += 1;
        self.pending = Some((self.requests, expression.clone()));
        cx.emit(EvaluateDialogEvent::Evaluate { request: self.requests, expression });
        cx.notify();
    }

    fn add_watch(&mut self, close: bool, cx: &mut Context<Self>) {
        let expression = self.expression(cx);
        if expression.trim().is_empty() {
            return;
        }
        cx.emit(EvaluateDialogEvent::AddWatch { expression, close });
    }

    /// ↑ (`older`) or ↓ through the history; at the newest end ↓ empties the field. False at either end.
    fn walk_history(&mut self, older: bool, cx: &mut Context<Self>) -> bool {
        let next = match (self.history_pos, older) {
            (None, true) => Some(0),
            (None, false) => return false,
            (Some(at), true) => Some(at + 1),
            (Some(0), false) => None,
            (Some(at), false) => Some(at - 1),
        };
        if next.is_some_and(|at| at >= self.history.len()) {
            return false;
        }
        self.history_pos = next;
        let text = next.map(|at| self.history[at].clone()).unwrap_or_default();
        self.field.update(cx, |field, cx| field.set_text(&text, cx));
        cx.notify();
        true
    }

    /// Enter, ⇧⌘Enter, Escape, ↑ and ↓ are the dialog's own, ahead of the keymap.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        match keystroke.key.as_str() {
            "enter" if modifiers.shift && (modifiers.platform || modifiers.control) => self.add_watch(false, cx),
            "enter" if !modifiers.shift && !modifiers.alt => self.evaluate(cx),
            "escape" if !modifiers.modified() => self.press(CLOSE, cx),
            "up" | "down" if !modifiers.shift && !modifiers.alt && !modifiers.platform => {
                return self.walk_history(keystroke.key == "up", cx);
            }
            _ => return false,
        }
        true
    }
}

impl Focusable for EvaluateDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl Render for EvaluateDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, muted) = (theme::color(cx, "fg"), theme::color(cx, "fg-muted"));
        let title = tx(cx, "Evaluate");
        let label = tx(cx, "Expression");
        let result = match (&self.result, self.note(cx)) {
            (Some(result), _) => {
                let color = if result.is_error() { theme::color(cx, "danger") } else { fg };
                div()
                    .font_family(self.code_font.clone())
                    .text_color(color)
                    .whitespace_nowrap()
                    .debug_selector(|| "evaluate-result".to_owned())
                    .child(SharedString::from(result.line()))
            }
            (None, Some(note)) => div().text_color(muted).child(SharedString::from(tx(cx, note))),
            (None, None) => div(),
        };
        let buttons: Vec<_> = self
            .buttons(cx)
            .into_iter()
            .map(|(id, label, enabled)| {
                let label = tx(cx, label);
                button(cx, label, id == EVALUATE, false, enabled)
                    .id(SharedString::from(format!("evaluate-button-{id}")))
                    .debug_selector(move || format!("evaluate-button-{id}"))
                    .when(enabled, |el| {
                        el.on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.press(id, cx)))
                    })
            })
            .collect();
        let mut buttons = buttons.into_iter();
        let watches = buttons.next();
        let has_history = !self.history.is_empty();
        let history = div()
            .id("evaluate-history")
            .debug_selector(|| "evaluate-history".to_owned())
            .flex_none()
            .size(theme::zoomed(cx, 22.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .when(has_history, |el| el.hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover"))))
            .when(!has_history, |el| el.opacity(0.5))
            .child(theme::icon(cx, "history", 16.0, muted))
            .with_tooltip("History")
            .when(has_history, |el| {
                el.on_click(cx.listener(|_, event: &ClickEvent, _, cx| {
                    cx.emit(EvaluateDialogEvent::ShowHistory { position: event.position() });
                }))
            });
        app_views::frame(cx, WIDTH)
            .child(app_views::title_row(cx, title))
            .child(
                div()
                    .px(theme::zoomed(cx, 16.0))
                    .py(theme::zoomed(cx, 4.0))
                    .flex()
                    .flex_col()
                    .gap(theme::zoomed(cx, 6.0))
                    .child(div().text_color(muted).child(SharedString::from(label)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::zoomed(cx, 4.0))
                            .child(
                                app_views::field_frame(cx, false)
                                    .flex_1()
                                    .min_w_0()
                                    .font_family(self.code_font.clone())
                                    .child(div().flex_1().min_w_0().child(self.field.clone())),
                            )
                            .child(history),
                    )
                    .child(
                        div()
                            .min_h(theme::zoomed(cx, 120.0))
                            .p(theme::zoomed(cx, 8.0))
                            .rounded(theme::length(cx, "radius-small"))
                            .border_1()
                            .border_color(theme::color(cx, "border"))
                            .bg(theme::color(cx, "bg-editor"))
                            .overflow_hidden()
                            .child(result),
                    ),
            )
            .child(
                div()
                    .px(theme::zoomed(cx, 16.0))
                    .pt(theme::zoomed(cx, 10.0))
                    .pb(theme::zoomed(cx, 14.0))
                    .flex()
                    .items_center()
                    .gap(theme::zoomed(cx, 8.0))
                    .children(watches)
                    .child(div().flex_1())
                    .children(buttons),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debugger::state::DebugSession;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use ide_core::DebugSessionId;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<EvaluateDialogEvent>>>;

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
    }

    /// The dialog on a debugger whose only session is suspended when `paused`, with the keyboard in
    /// its field, and the events it sends.
    fn open<'a>(
        cx: &'a mut TestAppContext,
        paused: bool,
        expression: &'static str,
        history: &[&str],
    ) -> (Entity<EvaluateDialog>, Events, &'a mut VisualTestContext) {
        init(cx);
        let history: Vec<String> = history.iter().map(|known| (*known).to_owned()).collect();
        let mut model = Debugger::default();
        model.state.adopt_session(DebugSessionId(1), 0.0, |session: &mut DebugSession| {
            session.name = "main".to_owned();
            if paused {
                session.status = SessionStatus::Paused;
            }
        });
        model.state.take_effects();
        let debugger = cx.update(|cx| cx.new(|_| model));
        let (dialog, cx) =
            cx.add_window_view(move |window, cx| EvaluateDialog::new(debugger, expression, history, window, cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            let record = move |_: Entity<EvaluateDialog>, event: &EvaluateDialogEvent, _: &mut App| {
                log.borrow_mut().push(event.clone());
            };
            cx.subscribe(&dialog, record).detach();
            window.activate_window();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (dialog, events, cx)
    }

    fn text(dialog: &Entity<EvaluateDialog>, cx: &mut VisualTestContext) -> String {
        dialog.read_with(cx, |dialog, cx| dialog.expression(cx))
    }

    // Not from a spec: the history keeps the newest expression first, once, and at most fifty.
    #[test]
    fn the_history_keeps_the_newest_expression_first_and_at_most_fifty() {
        let older: Vec<String> = (0..60).map(|n| format!("e{n}")).collect();
        let pushed = pushed_history(&older, "  e3 ");
        assert_eq!(pushed.len(), HISTORY_MAX);
        assert_eq!(&pushed[..3], ["e3", "e0", "e1"]);
        assert_eq!(pushed_history(&older[..2], "   "), older[..2]);
        let mut storage = UiStorage::default();
        let key = history_key("evaluate", "/p");
        assert_eq!(key, "openidea.debugger.history.evaluate:/p");
        storage.set(&key, "[\"a\", 3, \"b\"]".to_owned());
        assert_eq!(read_history(&storage, &key), ["a", "b"]);
        storage.set(&key, "not json".to_owned());
        assert!(read_history(&storage, &key).is_empty());
    }

    // Not from a spec: a result reads as a row of the Variables view.
    #[test]
    fn a_result_reads_as_a_row_of_the_variables_view() {
        let value = EvaluateResult::Value {
            expression: "total".to_owned(),
            value: "12\n".to_owned(),
            type_name: Some("int".to_owned()),
        };
        assert_eq!(value.line(), "total = {int} 12");
        let error = EvaluateResult::Error { expression: "x".to_owned(), message: "name 'x' is not defined".to_owned() };
        assert_eq!(error.line(), "x = name 'x' is not defined");
        assert!(error.is_error() && !value.is_error());
    }

    // Not from a spec: Enter asks for the evaluation of the selected-in text and shows its answer; an
    // older answer is dropped.
    #[gpui_kit::test]
    async fn enter_evaluates_the_expression_and_shows_its_value(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, true, "order.items", &[]);
        assert_eq!(text(&dialog, cx), "order.items");
        assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.note(cx)), Some("Press Enter to evaluate"));
        cx.simulate_keystrokes("enter");
        let asked = events.borrow().clone();
        assert_eq!(asked, [EvaluateDialogEvent::Evaluate { request: 1, expression: "order.items".to_owned() }]);
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.history().to_vec()), ["order.items"]);
        let variable = Variable {
            name: "order.items".to_owned(),
            value: "[1, 2]".to_owned(),
            type_name: Some("list".to_owned()),
            variables_reference: 0,
            evaluate_name: None,
        };
        dialog.update(cx, |dialog, cx| dialog.evaluated(7, Ok(variable.clone()), cx));
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.result_line()), None, "an older answer is dropped");
        dialog.update(cx, |dialog, cx| dialog.evaluated(1, Ok(variable), cx));
        let shown = dialog.read_with(cx, |dialog, _| dialog.result_line());
        assert_eq!(shown.as_deref(), Some("order.items = {list} [1, 2]"));
        assert!(cx.debug_bounds("evaluate-result").is_some(), "the result is drawn");
    }

    // Not from a spec: nothing is evaluated while the program runs; Evaluate is disabled and the
    // field says why.
    #[gpui_kit::test]
    async fn nothing_is_evaluated_while_the_program_runs(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, false, "total", &[]);
        cx.simulate_keystrokes("enter");
        assert!(events.borrow().is_empty());
        let buttons = dialog.read_with(cx, |dialog, cx| dialog.buttons(cx));
        assert_eq!(buttons[1], (EVALUATE, "Evaluate", false));
        let placeholder = dialog.read_with(cx, |dialog, cx| dialog.field().read(cx).placeholder().to_owned());
        assert_eq!(placeholder, "Pause the program to evaluate");
        let note = dialog.read_with(cx, |dialog, cx| dialog.note(cx));
        assert_eq!(note, Some("Evaluation is available while the program is suspended"));
    }

    // Not from a spec: ↑ and ↓ walk the history, ↓ past the newest empties the field.
    #[gpui_kit::test]
    async fn up_and_down_walk_the_history(cx: &mut TestAppContext) {
        let (dialog, _, cx) = open(cx, true, "", &["b", "a"]);
        cx.simulate_keystrokes("up");
        assert_eq!(text(&dialog, cx), "b");
        cx.simulate_keystrokes("up");
        assert_eq!(text(&dialog, cx), "a");
        cx.simulate_keystrokes("up");
        assert_eq!(text(&dialog, cx), "a", "the oldest is the end");
        cx.simulate_keystrokes("down");
        assert_eq!(text(&dialog, cx), "b");
        cx.simulate_keystrokes("down");
        assert_eq!(text(&dialog, cx), "");
    }

    // Not from a spec: ⇧⌘Enter adds a watch and keeps the dialog, Add to Watches adds it and closes,
    // Escape closes.
    #[gpui_kit::test]
    async fn add_to_watches_and_escape(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, true, "total", &[]);
        cx.simulate_keystrokes("cmd-shift-enter");
        let click = cx.debug_bounds("evaluate-button-addToWatches").expect("the button is drawn");
        cx.simulate_click(click.center(), Modifiers::default());
        cx.simulate_keystrokes("escape");
        let watch = |close| EvaluateDialogEvent::AddWatch { expression: "total".to_owned(), close };
        assert_eq!(*events.borrow(), [watch(false), watch(true), EvaluateDialogEvent::Closed]);
        drop(dialog);
    }

    // Not from a spec: the History button is greyed out without a history and asks for its menu
    // otherwise; the menu's rows are the newest expressions on one line each, and a chosen one replaces
    // what was typed.
    #[gpui_kit::test]
    async fn the_history_button_offers_the_newest_expressions_and_recalls_one(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, true, "x", &[]);
        let button = cx.debug_bounds("evaluate-history").expect("the History button is drawn");
        cx.simulate_click(button.center(), Modifiers::default());
        assert!(events.borrow().is_empty(), "nothing to recall");

        let (dialog, events, cx) = open(cx, true, "x", &["a  +\n b", "c"]);
        let button = cx.debug_bounds("evaluate-history").expect("the History button is drawn");
        cx.simulate_click(button.center(), Modifiers::default());
        assert!(matches!(events.borrow().as_slice(), [EvaluateDialogEvent::ShowHistory { .. }]));
        let menu = dialog.read_with(cx, |dialog, _| dialog.history_menu());
        assert_eq!(menu, [("a + b".to_owned(), "a  +\n b".to_owned()), ("c".to_owned(), "c".to_owned())]);
        dialog.update(cx, |dialog, cx| dialog.recall("c", cx));
        assert_eq!(text(&dialog, cx), "c");
    }
}
