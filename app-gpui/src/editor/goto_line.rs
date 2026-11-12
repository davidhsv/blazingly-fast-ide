//! Go to Line:Column: a field that takes `line[:column]`, both counted from 1, and asks the window to
//! put the caret of the editor there. It opens on the caret's position, selected, so that typing
//! replaces it. Enter and OK go to what is typed, Escape and Cancel give up.

use crate::platform::i18n::{Localization, Param};
use crate::platform::input;
use crate::platform::keymap::is_mac_host;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Render,
    SharedString, Subscription, Window, div, prelude::*, px,
};

const WIDTH: f32 = 340.0;
const BUTTON_HEIGHT: f32 = 28.0;
/// A typed number above this is taken as this: it lies past the end of any document and is still
/// safe to add to an offset in one.
const MAX_NUMBER: u32 = u32::MAX / 2;

/// What the popup asks of its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GotoLineEvent {
    /// Escape or Cancel.
    Dismissed,
    /// Close the popup and put the caret of the editor at the 0-based line and UTF-16 column. Both
    /// are as typed: a line or column past the end is the editor's to clamp.
    Go { line: u32, col: u32 },
}

pub struct GotoLine {
    field: Entity<TextField>,
    /// Where the caret was when the popup opened, as a 0-based line and UTF-16 column; `None` when
    /// no editor is open.
    position: Option<(u32, u32)>,
    /// The claim on Enter, ahead of the keymap.
    key_claim: u64,
    _field_events: Subscription,
}

impl EventEmitter<GotoLineEvent> for GotoLine {}

/// The number `digits` spell; `None` unless they are all ASCII digits.
fn number(digits: &str) -> Option<u32> {
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    // Digits that do not parse spell a number too large for the type.
    Some(digits.parse::<u32>().map_or(MAX_NUMBER, |number| number.min(MAX_NUMBER)))
}

/// The 0-based line and column that `text` names as `line`, `line:column` or `line,column`, both
/// counted from 1, with white space allowed around each part. The column is the first one when
/// only a line is given. `None` for anything else, and for a line or column 0.
pub fn parse_line_col(text: &str) -> Option<(u32, u32)> {
    let text = text.trim();
    let (line, col) = match text.split_once([':', ',']) {
        Some((line, col)) => (line.trim_end(), Some(col.trim_start())),
        None => (text, None),
    };
    let line = number(line)?;
    let col = match col {
        Some(col) => number(col)?,
        None => 1,
    };
    Some((line.checked_sub(1)?, col.checked_sub(1)?))
}

/// A footer button: filled when `primary`, outlined otherwise, flat and greyed when not `enabled`.
fn button(cx: &App, label: String, primary: bool, enabled: bool) -> Div {
    let face = div()
        .flex_none()
        .h(px(BUTTON_HEIGHT))
        .min_w(px(72.0))
        .px(px(14.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .child(SharedString::from(label));
    if !enabled {
        face.border_color(theme::color(cx, "border")).text_color(theme::color(cx, "fg-faint"))
    } else if primary {
        let fill = theme::color(cx, "bg-button-primary");
        face.bg(fill).border_color(fill).text_color(theme::color(cx, "fg-button-primary"))
    } else {
        face.bg(theme::color(cx, "bg-button"))
            .border_color(theme::color(cx, "border-button"))
            .text_color(theme::color(cx, "fg-strong"))
    }
}

impl GotoLine {
    /// The popup for an editor whose caret is at `position`, a 0-based line and UTF-16 column.
    /// Without an editor it says so and goes nowhere.
    pub fn new(position: Option<(u32, u32)>, cx: &mut Context<Self>) -> Self {
        let placeholder = cx.default_global::<Localization>().0.tx("line[:column]");
        let field = cx.new(|cx| {
            let mut field = TextField::new(cx).with_placeholder(placeholder);
            if let Some((line, col)) = position {
                field.set_text(&format!("{}:{}", u64::from(line) + 1, u64::from(col) + 1), cx);
                field.select_all(cx);
            }
            field
        });
        let events = cx.subscribe(&field, |_: &mut GotoLine, _, event: &TextFieldEvent, cx| match event {
            // What is typed decides how the field and OK are drawn.
            TextFieldEvent::Changed => cx.notify(),
            // Enter is the popup's own key: it is claimed before the field sees it.
            TextFieldEvent::Submitted => {}
            TextFieldEvent::Cancelled => cx.emit(GotoLineEvent::Dismissed),
        });
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            field.focus_handle(cx),
            move |keystroke, _, cx| claimant.update(cx, |popup, cx| popup.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|popup, cx| input::release_keys(popup.key_claim, cx)).detach();
        Self { field, position, key_claim, _field_events: events }
    }

    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    /// The 0-based line and column the typed text names; `None` while it names none.
    pub fn target(&self, cx: &App) -> Option<(u32, u32)> {
        parse_line_col(self.field.read(cx).text())
    }

    /// Something is typed that names no position: the field is drawn as wrong.
    pub fn is_invalid(&self, cx: &App) -> bool {
        !self.field.read(cx).text().is_empty() && self.target(cx).is_none()
    }

    /// Whether Enter and OK go anywhere: an editor is open and the typed text names a position.
    pub fn can_go(&self, cx: &App) -> bool {
        self.position.is_some() && self.target(cx).is_some()
    }

    /// The line under the field: where the caret is, or that there is no editor.
    pub fn hint(&self, cx: &mut App) -> String {
        let i18n = &mut cx.default_global::<Localization>().0;
        match self.position {
            Some((line, col)) => i18n.tx_built(
                &["Current position ", ":", ""],
                &[Param::from(f64::from(line) + 1.0), Param::from(f64::from(col) + 1.0)],
            ),
            None => i18n.tx("No active editor"),
        }
    }

    /// Enter and OK: asks the window to go to the typed position. Does nothing while the text names
    /// none or no editor is open; the popup stays for the text to be corrected.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if self.position.is_none() {
            return;
        }
        if let Some((line, col)) = self.target(cx) {
            cx.emit(GotoLineEvent::Go { line, col });
        }
    }

    /// Enter is the popup's key in every modifier combination while the keyboard is in its field:
    /// the keymap's bindings for it do not run. Returns whether `keystroke` is Enter.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.key != "enter" {
            return false;
        }
        self.submit(cx);
        true
    }
}

impl Focusable for GotoLine {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl Render for GotoLine {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let hint = self.hint(cx);
        let i18n = &cx.default_global::<Localization>().0;
        let (title, ok_label, cancel_label) = (i18n.tx("Go to Line:Column"), i18n.tx("OK"), i18n.tx("Cancel"));
        let (invalid, can_go) = (self.is_invalid(cx), self.can_go(cx));
        let field_border = theme::color(cx, if invalid { "danger" } else { "border" });

        let ok = button(cx, ok_label, true, can_go)
            .id("ok")
            .debug_selector(|| "goto-line-ok".to_owned())
            .on_click(cx.listener(|popup, _: &ClickEvent, _, cx| popup.submit(cx)))
            .into_any_element();
        let cancel = button(cx, cancel_label, false, true)
            .id("cancel")
            .debug_selector(|| "goto-line-cancel".to_owned())
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(GotoLineEvent::Dismissed)))
            .into_any_element();
        // The action that confirms comes last on macOS and first elsewhere.
        let buttons = if is_mac_host() { [cancel, ok] } else { [ok, cancel] };

        div()
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .rounded(px(10.0))
            .overflow_hidden()
            .text_size(theme::length(cx, "fs-ui"))
            .child(
                div()
                    .flex_none()
                    .h(px(32.0))
                    .flex()
                    .items_center()
                    .px(px(16.0))
                    .text_color(theme::color(cx, "fg-strong"))
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .px(px(16.0))
                    .pt(px(8.0))
                    .child(
                        div()
                            .w_full()
                            .px(px(8.0))
                            .rounded(theme::length(cx, "radius-small"))
                            .border_1()
                            .border_color(field_border)
                            .bg(theme::color(cx, "bg-input"))
                            .child(self.field.clone()),
                    )
                    .child(
                        div()
                            .text_color(theme::color(cx, "fg-subtle"))
                            .text_size(theme::length(cx, "fs-small"))
                            .child(SharedString::from(hint)),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.0))
                    .px(px(16.0))
                    .py(px(12.0))
                    .children(buttons),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<GotoLineEvent>>>;

    /// The popup in a window of its own, with the keyboard, and what it asks of the window.
    fn open(
        cx: &mut TestAppContext,
        position: Option<(u32, u32)>,
    ) -> (Entity<GotoLine>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (popup, cx) = cx.add_window_view(move |_, cx| GotoLine::new(position, cx));
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&popup, move |_, event: &GotoLineEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = popup.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (popup, events, cx)
    }

    fn typed(popup: &Entity<GotoLine>, cx: &mut VisualTestContext) -> String {
        popup.read_with(cx, |popup, cx| popup.field().read(cx).text().to_owned())
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
    }

    // Not from a spec: a line, a line and a column after `:` or `,`, white space around the parts,
    // and everything that names no position.
    #[test]
    fn reads_a_line_and_an_optional_column_counted_from_one() {
        assert_eq!(parse_line_col("12"), Some((11, 0)));
        assert_eq!(parse_line_col("12:5"), Some((11, 4)));
        assert_eq!(parse_line_col("12,5"), Some((11, 4)));
        assert_eq!(parse_line_col("1:1"), Some((0, 0)));
        assert_eq!(parse_line_col("  7  "), Some((6, 0)));
        assert_eq!(parse_line_col(" 7 : 3 "), Some((6, 2)));
        assert_eq!(parse_line_col("7 ,3"), Some((6, 2)));
        assert_eq!(parse_line_col("007:02"), Some((6, 1)));

        for wrong in ["", "   ", "abc", "12:", "12,", ":5", "12:x", "1 2", "12:3 4", "12:5:6", "12;5", "12.5"] {
            assert_eq!(parse_line_col(wrong), None, "{wrong:?} names no position");
        }
        for signed in ["-3", "+3", "3:-1", "1e3", "0x10"] {
            assert_eq!(parse_line_col(signed), None, "{signed:?} is not a plain number");
        }
        assert_eq!(parse_line_col("0"), None, "lines are counted from 1");
        assert_eq!(parse_line_col("0:4"), None);
        assert_eq!(parse_line_col("4:0"), None, "columns are counted from 1");
    }

    // Not from a spec: a number of any length is a position far past the end, never an overflow.
    #[test]
    fn a_number_too_large_to_hold_is_a_position_past_the_end() {
        let far = MAX_NUMBER - 1;
        assert_eq!(parse_line_col("99999999999999999999"), Some((far, 0)));
        assert_eq!(parse_line_col("3:99999999999999999999"), Some((2, far)));
        assert_eq!(parse_line_col("4294967295:4294967295"), Some((far, far)));
        assert!(far.checked_add(far).is_some(), "a position this far still adds to an offset");
    }

    /// The field starts on the caret's position, selected, over a line that names it.
    #[gpui_kit::test]
    async fn opens_on_the_position_of_the_caret(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, Some((2, 4)));
        assert_eq!(typed(&popup, cx), "3:5");
        assert_eq!(popup.read_with(cx, |popup, cx| popup.field().read(cx).selected_text().to_owned()), "3:5");
        assert_eq!(popup.update(cx, |popup, cx| popup.hint(cx)), "Current position 3:5");
        assert!(cx.update(|window, cx| popup.focus_handle(cx).is_focused(window)), "the field has the keyboard");
        assert!(popup.read_with(cx, |popup, cx| popup.can_go(cx) && !popup.is_invalid(cx)));
        assert_eq!(popup.read_with(cx, |popup, cx| popup.target(cx)), Some((2, 4)));
        assert!(events.borrow().is_empty());
    }

    /// Typing replaces the position shown; Enter asks for the typed line and column, from 0.
    #[gpui_kit::test]
    async fn enter_goes_to_the_typed_line_and_column(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, Some((2, 4)));
        cx.simulate_input("12:7");
        assert_eq!(typed(&popup, cx), "12:7");
        cx.simulate_keystrokes("enter");
        assert_eq!(*events.borrow(), [GotoLineEvent::Go { line: 11, col: 6 }]);
    }

    /// A line alone goes to its first column, and Enter on the position the popup opened with goes
    /// to where the caret is.
    #[gpui_kit::test]
    async fn a_line_alone_goes_to_its_first_column(cx: &mut TestAppContext) {
        let (_popup, events, cx) = open(cx, Some((2, 4)));
        cx.simulate_keystrokes("enter");
        assert_eq!(*events.borrow(), [GotoLineEvent::Go { line: 2, col: 4 }]);
        cx.simulate_input("40");
        cx.simulate_keystrokes("enter");
        assert_eq!(events.borrow().last(), Some(&GotoLineEvent::Go { line: 39, col: 0 }));
        cx.simulate_keystrokes("backspace backspace");
        cx.simulate_input("8,2");
        cx.simulate_keystrokes("enter");
        assert_eq!(events.borrow().last(), Some(&GotoLineEvent::Go { line: 7, col: 1 }));
    }

    /// Text that names no position is marked as wrong and Enter leaves the popup open; an empty
    /// field is not wrong, and goes nowhere either.
    #[gpui_kit::test]
    async fn text_that_names_no_position_goes_nowhere(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, Some((0, 0)));
        cx.simulate_input("12:");
        assert!(popup.read_with(cx, |popup, cx| popup.is_invalid(cx) && !popup.can_go(cx)));
        cx.simulate_keystrokes("enter");
        assert!(events.borrow().is_empty());

        cx.simulate_input("x");
        assert!(popup.read_with(cx, |popup, cx| popup.is_invalid(cx)));
        cx.simulate_keystrokes("backspace backspace backspace backspace");
        assert_eq!(typed(&popup, cx), "");
        assert!(popup.read_with(cx, |popup, cx| !popup.is_invalid(cx) && !popup.can_go(cx)));
        cx.simulate_keystrokes("enter");
        assert!(events.borrow().is_empty());

        cx.simulate_input("0");
        assert!(popup.read_with(cx, |popup, cx| popup.is_invalid(cx)), "there is no line 0");
        click(cx, "goto-line-ok");
        assert!(events.borrow().is_empty(), "OK is off while the text names no position");
    }

    #[gpui_kit::test]
    async fn escape_dismisses_the_popup(cx: &mut TestAppContext) {
        let (_popup, events, cx) = open(cx, Some((2, 4)));
        cx.simulate_keystrokes("escape");
        assert_eq!(*events.borrow(), [GotoLineEvent::Dismissed]);
    }

    /// OK goes to the typed position and Cancel gives up.
    #[gpui_kit::test]
    async fn the_buttons_confirm_and_cancel(cx: &mut TestAppContext) {
        let (_popup, events, cx) = open(cx, Some((2, 4)));
        cx.simulate_input("9:2");
        click(cx, "goto-line-ok");
        assert_eq!(*events.borrow(), [GotoLineEvent::Go { line: 8, col: 1 }]);
        click(cx, "goto-line-cancel");
        assert_eq!(events.borrow().last(), Some(&GotoLineEvent::Dismissed));
    }

    /// Without an editor the field is empty, the popup says so and a typed position goes nowhere.
    #[gpui_kit::test]
    async fn without_an_editor_there_is_nowhere_to_go(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, None);
        assert_eq!(typed(&popup, cx), "");
        assert_eq!(popup.update(cx, |popup, cx| popup.hint(cx)), "No active editor");
        cx.simulate_input("3:1");
        assert_eq!(popup.read_with(cx, |popup, cx| popup.target(cx)), Some((2, 0)));
        assert!(popup.read_with(cx, |popup, cx| !popup.can_go(cx) && !popup.is_invalid(cx)));
        cx.simulate_keystrokes("enter");
        assert!(events.borrow().is_empty());
        cx.simulate_keystrokes("escape");
        assert_eq!(*events.borrow(), [GotoLineEvent::Dismissed]);
        click(cx, "goto-line-ok");
        assert_eq!(events.borrow().len(), 1, "OK is off without an editor");
    }

    // Not from a spec: every design token the popup draws with is defined, as the kind it is used as.
    #[test]
    fn the_design_tokens_the_popup_uses_exist() {
        let theme = theme::ActiveTheme::default().theme;
        let colors = [
            "fg-strong",
            "fg-subtle",
            "fg-faint",
            "fg-button-primary",
            "bg-popup",
            "bg-input",
            "bg-button",
            "bg-button-primary",
            "border",
            "border-button",
            "danger",
        ];
        for name in colors {
            assert!(theme.color(name).is_some(), "--{name} is a colour");
        }
        for name in ["fs-ui", "fs-small", "radius-small"] {
            assert!(theme.px(name).is_some(), "--{name} is a length");
        }
    }
}
