//! Parameter info: the signatures of the call at the caret, the active parameter in bold, the
//! active signature highlighted, the overloads that cannot take the argument at the caret
//! dimmed, and the documentation of a sole signature under them.
//!
//! It never takes the keyboard. While it shows, it claims Escape (it closes) and ⇥ / ⇧⇥ on the
//! editor's focus handle; ⇥ goes to the owner's handler, which moves to the next or previous
//! argument when nothing else owns the key ([`super::signature::move_to_argument`]).

use super::hover_view::{LinkHandler, code_font, keeps_focus, markup_element, popup_surface};
use super::markdown::{Block, blocks_text, markup_blocks};
use super::signature::{ShownSignature, SignatureRow, signature_rows};
use crate::platform::input;
use crate::theme;
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, Font, FontWeight, IntoElement, Keystroke, Render, StyledText, TextRun,
    Window, div, prelude::*, px,
};
use std::rc::Rc;

/// ⇥ (`true`) or ⇧⇥ (`false`) while parameter info shows: whether the owner moved the caret to
/// another argument and so used the key.
pub type ArgumentKeys = Rc<dyn Fn(bool, &mut Window, &mut App) -> bool>;

/// What parameter info asks of its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterInfoEvent {
    /// Escape closed it: [`super::signature::SignatureSession::close`].
    Closed,
}

/// Parameter info of one editor, following the owner's [`super::signature::SignatureSession`].
/// The owner draws it above the caret line at [`crate::editor::view::Editor::position_of`] of the
/// shown signature's `pos`, flipping below the line when there is no room above.
pub struct ParameterInfo {
    rows: Vec<SignatureRow>,
    doc: Vec<Block>,
    arguments: Option<ArgumentKeys>,
    on_link: LinkHandler,
    key_claim: u64,
}

impl EventEmitter<ParameterInfoEvent> for ParameterInfo {}

impl ParameterInfo {
    /// Hidden parameter info that claims its keys on `editor_focus`, the editor's focus handle.
    pub fn new(editor_focus: FocusHandle, cx: &mut Context<Self>) -> Self {
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            editor_focus,
            move |keystroke, window, cx| {
                claimant.update(cx, |info, cx| info.claim_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|info, cx| input::release_keys(info.key_claim, cx)).detach();
        Self {
            rows: Vec::new(),
            doc: Vec::new(),
            arguments: None,
            on_link: Rc::new(|_: &str, _: &mut Window, _: &mut App| {}),
            key_claim,
        }
    }

    /// What ⇥ and ⇧⇥ do while parameter info shows; without a handler they go to the editor.
    pub fn set_argument_keys(&mut self, keys: Option<ArgumentKeys>) {
        self.arguments = keys;
    }

    /// Shows the session's signature help, or hides parameter info when there is none.
    pub fn show(&mut self, shown: Option<&ShownSignature>, cx: &mut Context<Self>) {
        match shown {
            Some(shown) => {
                let (rows, doc) = signature_rows(&shown.help);
                self.doc = doc.map(markup_blocks).unwrap_or_default();
                self.rows = rows;
            }
            None => {
                self.rows.clear();
                self.doc.clear();
            }
        }
        cx.notify();
    }

    pub fn is_visible(&self) -> bool {
        !self.rows.is_empty()
    }

    /// The rows as text: the active parameter in `[ ]`, `> ` before the active signature, `- `
    /// before an overload that cannot take the argument.
    pub fn rows(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|row| {
                let mark = match (row.active, row.disabled) {
                    (true, _) => "> ",
                    (false, true) => "- ",
                    (false, false) => "",
                };
                let parts = &row.parts;
                let param = if parts.param.is_empty() { String::new() } else { format!("[{}]", parts.param) };
                format!("{mark}{}{param}{}", parts.before, parts.after)
            })
            .collect()
    }

    /// The text of the sole signature's documentation.
    pub fn doc_text(&self) -> Option<String> {
        (!self.doc.is_empty()).then(|| blocks_text(&self.doc))
    }

    fn claim_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        if !self.is_visible() || modifiers.platform || modifiers.control || modifiers.alt {
            return false;
        }
        match keystroke.key.as_str() {
            "escape" if !modifiers.shift => {
                self.show(None, cx);
                cx.emit(ParameterInfoEvent::Closed);
                true
            }
            "tab" => match &self.arguments {
                Some(arguments) => Rc::clone(arguments)(!modifiers.shift, window, cx),
                None => false,
            },
            _ => false,
        }
    }
}

/// A signature as runs: the active parameter bold and strong.
fn signature_text(row: &SignatureRow, base: &Font, cx: &App) -> StyledText {
    let parts = &row.parts;
    let text = format!("{}{}{}", parts.before, parts.param, parts.after);
    let color = theme::color(cx, if row.active { "fg-strong" } else { "fg-subtle" });
    let run = |len: usize, bold: bool| TextRun {
        len,
        font: Font { weight: if bold { FontWeight::BOLD } else { base.weight }, ..base.clone() },
        color: if bold { theme::color(cx, "fg-strong") } else { color },
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let runs: Vec<TextRun> = [(parts.before.len(), false), (parts.param.len(), true), (parts.after.len(), false)]
        .into_iter()
        .filter(|(len, _)| *len > 0)
        .map(|(len, bold)| run(len, bold))
        .collect();
    if text.is_empty() {
        return StyledText::new(" ").with_runs(vec![run(1, false)]);
    }
    StyledText::new(text).with_runs(runs)
}

impl Render for ParameterInfo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_visible() {
            return div();
        }
        let base = code_font(cx);
        let active_bg = theme::color(cx, "bg-identifier-under-caret");
        let rows = self.rows.iter().enumerate().map(|(index, row)| {
            div()
                .debug_selector(move || format!("signature-row-{index}"))
                .mx(px(-10.0))
                .px(px(10.0))
                .when(row.active, |el| el.bg(active_bg))
                .when(row.disabled, |el| el.opacity(0.55))
                .child(signature_text(row, &base, cx))
        });
        let rows: Vec<_> = rows.collect();
        let doc = (!self.doc.is_empty()).then(|| {
            div()
                .id("signature-doc")
                .mt(px(6.0))
                .pt(px(6.0))
                .border_t_1()
                .border_color(theme::color(cx, "border-popup"))
                .max_h(px(160.0))
                .overflow_y_scroll()
                .child(markup_element(&self.doc, "signature-doc", &self.on_link, cx))
        });
        keeps_focus(
            popup_surface(cx)
                .debug_selector(|| "parameter-info".to_owned())
                .max_w(px(640.0))
                .px(px(10.0))
                .py(px(6.0))
                .flex()
                .flex_col()
                .font_family(base.family.clone())
                .text_size(theme::length(cx, "fs-code"))
                .line_height(px(20.0))
                .children(rows)
                .children(doc),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::signature::SignatureSession;
    use super::*;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use ide_core::TextChange;
    use ide_lsp::types::{Markup, Signature, SignatureHelp};
    use std::cell::{Cell, RefCell};

    struct Host {
        focus: FocusHandle,
        info: Entity<ParameterInfo>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().track_focus(&self.focus).size_full().child(self.info.clone())
        }
    }

    type Events = Rc<RefCell<Vec<ParameterInfoEvent>>>;

    fn open(cx: &mut TestAppContext) -> (Entity<ParameterInfo>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (host, cx) = cx.add_window_view(|_, cx| {
            let focus = cx.focus_handle();
            let info = cx.new(|cx| ParameterInfo::new(focus.clone(), cx));
            Host { focus, info }
        });
        let info = host.read_with(cx, |host, _| host.info.clone());
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&info, move |_, event: &ParameterInfoEvent, _| seen.borrow_mut().push(*event)).detach();
            window.activate_window();
            let focus = host.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (info, events, cx)
    }

    fn signature(label: &str, parameters: Vec<(u32, u32)>, doc: Option<&str>) -> Signature {
        let documentation = doc.map(|value| Markup { kind: "markdown".into(), value: value.into() });
        Signature { label: label.into(), documentation, parameters }
    }

    /// The session after `(` was typed and the server answered with `help`.
    fn session(help: SignatureHelp) -> SignatureSession {
        let mut session = SignatureSession::default();
        session.update(&[TextChange { from: 1, to: 1, insert: "()".into() }], true, Some('('), Some(0));
        session.answer(Some(help), true, false, 2, 3);
        session
    }

    #[gpui_kit::test]
    async fn shows_each_signature_with_the_active_parameter_in_bold(cx: &mut TestAppContext) {
        let (info, _, cx) = open(cx);
        let help = SignatureHelp {
            signatures: vec![
                signature("println()", Vec::new(), None),
                signature("println(Object x, int n)", vec![(8, 16), (18, 23)], None),
                signature("println(String s)", vec![(8, 16)], None),
            ],
            active_signature: 1,
            active_parameter: 1,
        };
        let session = session(help);
        info.update(cx, |info, cx| info.show(session.shown(), cx));
        cx.run_until_parked();
        assert_eq!(
            info.read_with(cx, |info, _| info.rows()),
            ["- println()", "> println(Object x, [int n])", "- println(String s)"]
        );
        assert!(cx.debug_bounds("signature-row-2").is_some());
        assert_eq!(info.read_with(cx, |info, _| info.doc_text()), None, "several signatures show no documentation");
    }

    #[gpui_kit::test]
    async fn shows_the_documentation_of_a_sole_signature(cx: &mut TestAppContext) {
        let (info, _, cx) = open(cx);
        let help = SignatureHelp {
            signatures: vec![signature("f(a: number)", vec![(2, 11)], Some("Doubles `a`."))],
            active_signature: 0,
            active_parameter: 0,
        };
        info.update(cx, |info, cx| info.show(session(help).shown(), cx));
        cx.run_until_parked();
        assert_eq!(info.read_with(cx, |info, _| info.rows()), ["> f([a: number])"]);
        assert_eq!(info.read_with(cx, |info, _| info.doc_text()).as_deref(), Some("Doubles a."));
    }

    #[gpui_kit::test]
    async fn escape_closes_it_and_tab_goes_to_the_owner_while_it_shows(cx: &mut TestAppContext) {
        let (info, events, cx) = open(cx);
        let moves: Rc<Cell<(u32, u32)>> = Rc::default();
        let seen = Rc::clone(&moves);
        let keys: ArgumentKeys = Rc::new(move |forward, _, _| {
            let (next, previous) = seen.get();
            seen.set(if forward { (next + 1, previous) } else { (next, previous + 1) });
            true
        });
        let help = SignatureHelp {
            signatures: vec![signature("f(a, b)", vec![(2, 3), (5, 6)], None)],
            active_signature: 0,
            active_parameter: 0,
        };
        info.update(cx, |info, cx| {
            info.set_argument_keys(Some(keys));
            info.show(session(help).shown(), cx);
        });
        cx.simulate_keystrokes("tab shift-tab tab");
        assert_eq!(moves.get(), (2, 1));
        cx.simulate_keystrokes("escape");
        assert_eq!(*events.borrow(), [ParameterInfoEvent::Closed]);
        assert!(!info.read_with(cx, |info, _| info.is_visible()));
        assert!(cx.debug_bounds("parameter-info").is_none());
        // Hidden, it takes no key.
        cx.simulate_keystrokes("tab escape");
        assert_eq!(moves.get(), (2, 1));
        assert_eq!(events.borrow().len(), 1);
    }
}
