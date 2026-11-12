//! Parameter info (⌘P, and on its own after `(` and `,`): the signatures above the caret with the
//! active parameter in bold. While shown it follows the caret and closes when the server reports
//! no signature; ⇥ / ⇧⇥ move to the next / previous argument.

use crate::editor::buffer::{self, Assoc};
use crate::find::utf16;
use ide_core::TextChange;
use ide_lsp::types::{Markup, SignatureHelp};
use regex::Regex;
use std::sync::LazyLock;

/// A signature label split around the active parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureParts {
    pub before: String,
    pub param: String,
    pub after: String,
}

/// Splits `label` around the UTF-16 range of parameter `active`.
pub fn split_signature(label: &str, params: &[(u32, u32)], active: u32) -> SignatureParts {
    let Some(&(start, end)) = params.get(active as usize) else {
        return SignatureParts { before: label.to_owned(), param: String::new(), after: String::new() };
    };
    let len = utf16::len(label);
    let (start, end) = (start.min(len), end.min(len).max(start.min(len)));
    SignatureParts {
        before: utf16::slice(label, 0, start).to_owned(),
        param: utf16::slice(label, start, end).to_owned(),
        after: utf16::slice_from(label, end).to_owned(),
    }
}

/// Whether an overload can take an argument at `index` (varargs take any number).
pub fn accepts_parameter(label: &str, params: &[(u32, u32)], index: u32) -> bool {
    static VARIADIC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.\.\.|^\s*\*").expect("the pattern is valid"));
    if (index as usize) < params.len() {
        return true;
    }
    params.last().is_some_and(|&(start, end)| VARIADIC.is_match(utf16::slice(label, start, end)))
}

/// At most this many signatures show.
const MAX_ROWS: usize = 12;

/// A row of parameter info.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureRow {
    pub parts: SignatureParts,
    pub active: bool,
    /// The overload cannot take the argument at the caret.
    pub disabled: bool,
}

/// The rows of `help` (a window of [`MAX_ROWS`] around the active signature), and the active
/// signature's documentation when it is the only one.
pub fn signature_rows(help: &SignatureHelp) -> (Vec<SignatureRow>, Option<&Markup>) {
    let count = help.signatures.len();
    let active = help.active_signature as usize;
    let start = active.saturating_sub(6).min(count.saturating_sub(MAX_ROWS));
    let rows = help
        .signatures
        .iter()
        .enumerate()
        .skip(start)
        .take(MAX_ROWS)
        .map(|(i, signature)| SignatureRow {
            parts: split_signature(&signature.label, &signature.parameters, help.active_parameter),
            active: i == active,
            disabled: i != active && !accepts_parameter(&signature.label, &signature.parameters, help.active_parameter),
        })
        .collect();
    let doc = help
        .signatures
        .get(active)
        .filter(|_| count == 1)
        .and_then(|signature| signature.documentation.as_ref())
        .filter(|doc| !doc.value.trim().is_empty());
    (rows, doc)
}

/// Parameter info shown at an offset.
#[derive(Debug, Clone)]
pub struct ShownSignature {
    pub pos: u32,
    pub help: SignatureHelp,
}

/// What the editor does next for parameter info.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureFollowUp {
    None,
    /// Ask after the Show the parameter info popup delay (ms).
    AutoPopup(u64),
    /// Ask again after a short pause ([`REFRESH_MS`]).
    Refresh,
}

/// How long parameter info waits before following the caret.
pub const REFRESH_MS: u64 = 120;
pub const NO_PARAMETERS: &str = "No parameters";

/// The parameter info of one editor.
#[derive(Debug, Clone, Default)]
pub struct SignatureSession {
    shown: Option<ShownSignature>,
}

impl SignatureSession {
    pub fn shown(&self) -> Option<&ShownSignature> {
        self.shown.as_ref()
    }

    /// An edit or a caret move. `typed` is whether the user typed or completed (the character
    /// before the caret is then `before`); `parameter_info` the auto-popup delay when Show the
    /// parameter info popup is on.
    pub fn update(
        &mut self,
        changes: &[TextChange],
        selection_set: bool,
        typed: Option<char>,
        parameter_info: Option<u64>,
    ) -> SignatureFollowUp {
        if let Some(shown) = &mut self.shown {
            shown.pos = buffer::map_offset(shown.pos, changes, Assoc::Before);
        }
        let typed_trigger = !changes.is_empty() && matches!(typed, Some('(' | ','));
        let active = self.shown.is_some();
        if typed_trigger && !active {
            return parameter_info.map_or(SignatureFollowUp::None, SignatureFollowUp::AutoPopup);
        }
        if typed_trigger || (active && (!changes.is_empty() || selection_set)) {
            return SignatureFollowUp::Refresh;
        }
        SignatureFollowUp::None
    }

    /// The server answered for the caret at `head`; `unchanged` is whether the document is the
    /// one asked about. Returns the hint to show.
    pub fn answer(
        &mut self,
        help: Option<SignatureHelp>,
        unchanged: bool,
        explicit: bool,
        head: u32,
        doc_len: u32,
    ) -> Option<&'static str> {
        if !unchanged && !explicit {
            return None;
        }
        match help.filter(|help| !help.signatures.is_empty()) {
            None => {
                self.shown = None;
                explicit.then_some(NO_PARAMETERS)
            }
            Some(help) => {
                self.shown = Some(ShownSignature { pos: head.min(doc_len), help });
                None
            }
        }
    }

    /// The request failed: parameter info closes.
    pub fn failed(&mut self) {
        self.shown = None;
    }

    /// Escape, or the editor lost focus; returns whether parameter info was shown.
    pub fn close(&mut self) -> bool {
        self.shown.take().is_some()
    }

    /// The caret was left inside the `()` of a completed call whose snippet does not say whether
    /// it takes arguments: parameter info when some signature has parameters, else the caret moves
    /// past `)`. `still_there` is whether the document and caret are as they were. Returns where
    /// the caret goes.
    pub fn parameter_info_or_leave(
        &mut self,
        help: Option<SignatureHelp>,
        still_there: bool,
        inside: u32,
    ) -> Option<u32> {
        if !still_there {
            return None;
        }
        match help.filter(|help| help.signatures.iter().any(|signature| !signature.parameters.is_empty())) {
            Some(help) => {
                self.shown = Some(ShownSignature { pos: inside, help });
                None
            }
            None => Some(inside + 1),
        }
    }
}

// ---------------------------------------------------------------------------
// ⇥ / ⇧⇥: next / previous argument while parameter info is shown

const OPEN: [u16; 3] = [b'(' as u16, b'[' as u16, b'{' as u16];
const CLOSE: [u16; 3] = [b')' as u16, b']' as u16, b'}' as u16];

fn is_quote(c: u16) -> bool {
    c == u16::from(b'"') || c == u16::from(b'\'') || c == u16::from(b'`')
}

fn is_ascii_word(c: Option<&u16>) -> bool {
    c.is_some_and(|&c| (c < 128 && (c as u8).is_ascii_alphanumeric()) || c == u16::from(b'_'))
}

fn is_space(c: u16) -> bool {
    char::from_u32(u32::from(c)).is_some_and(char::is_whitespace)
}

/// A quote that opens a string: not an apostrophe after a word character.
fn opens_string(text: &[u16], i: usize) -> bool {
    is_quote(text[i]) && !(text[i] == u16::from(b'\'') && is_ascii_word(i.checked_sub(1).and_then(|j| text.get(j))))
}

/// Skips a string literal starting at `i` (its quote); returns the index after it.
fn skip_string(text: &[u16], i: usize) -> usize {
    let quote = text[i];
    let mut j = i + 1;
    while j < text.len() && text[j] != quote {
        if text[j] == u16::from(b'\\') {
            j += 1;
        } else if text[j] == u16::from(b'\n') && quote != u16::from(b'`') {
            break;
        }
        j += 1;
    }
    j + 1
}

/// The `(` of the innermost call around `pos` (UTF-16), scanning brackets outside strings and line
/// comments, at most `window` units back.
pub fn enclosing_paren(text: &str, pos: u32, window: u32) -> Option<u32> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let pos = (pos as usize).min(units.len());
    let mut stack: Vec<usize> = Vec::new();
    let mut i = pos.saturating_sub(window as usize);
    while i < pos {
        let c = units[i];
        if opens_string(&units, i) {
            i = skip_string(&units, i);
            continue;
        }
        if c == u16::from(b'/') && units.get(i + 1) == Some(&u16::from(b'/')) {
            while i < pos && units[i] != u16::from(b'\n') {
                i += 1;
            }
        } else if OPEN.contains(&c) {
            stack.push(i);
        } else if CLOSE.contains(&c) {
            stack.pop();
        }
        i += 1;
    }
    stack.iter().rev().find(|&&at| units[at] == u16::from(b'(')).map(|&at| at as u32)
}

/// The arguments (UTF-16 ranges, trimmed; an empty one collapses to where it would start) of the
/// call whose `(` is at `open`; none for `()`.
pub fn argument_spans(text: &str, open: u32) -> Vec<std::ops::Range<u32>> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut out: Vec<std::ops::Range<u32>> = Vec::new();
    let push = |out: &mut Vec<std::ops::Range<u32>>, start: usize, end: usize| {
        let (mut from, mut to) = (start, end);
        while from < to && is_space(units[from]) {
            from += 1;
        }
        while to > from && is_space(units[to - 1]) {
            to -= 1;
        }
        out.push(from as u32..to as u32);
    };
    let mut depth = 0;
    let mut arg_start = open as usize + 1;
    let mut i = arg_start;
    while i < units.len() {
        let c = units[i];
        if opens_string(&units, i) {
            i = skip_string(&units, i);
            continue;
        }
        if OPEN.contains(&c) {
            depth += 1;
        } else if CLOSE.contains(&c) {
            if depth == 0 {
                push(&mut out, arg_start, i);
                if let [only] = out.as_slice()
                    && only.is_empty()
                {
                    return Vec::new();
                }
                return out;
            }
            depth -= 1;
        } else if c == u16::from(b',') && depth == 0 {
            push(&mut out, arg_start, i);
            arg_start = i + 1;
        }
        i += 1;
    }
    out
}

/// Where ⇥ (`forward`) / ⇧⇥ puts the caret in the call around `pos`; `None` at the ends (the key
/// then does its usual thing).
pub fn argument_stop(text: &str, pos: u32, forward: bool) -> Option<u32> {
    let open = enclosing_paren(text, pos, 4000)?;
    let args = argument_spans(text, open);
    if args.is_empty() {
        return None;
    }
    let units: Vec<u16> = text.encode_utf16().collect();
    // The argument the caret is in: past an argument's separating comma it is that argument.
    let mut current = 0;
    for (i, arg) in args.iter().enumerate().skip(1) {
        let start = (arg.start as usize).min(units.len().saturating_sub(1));
        let comma = units[..=start].iter().rposition(|&c| c == u16::from(b','));
        if comma.is_none_or(|comma| pos as usize > comma) {
            current = i;
        }
    }
    let target = if forward { current.checked_add(1) } else { current.checked_sub(1) }?;
    args.get(target).map(|arg| arg.start)
}

/// What else may own ⇥ at the moment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TabOwners {
    pub read_only: bool,
    pub several_carets: bool,
    pub lookup_open: bool,
    /// A snippet or template field to move to in that direction.
    pub snippet_field: bool,
    pub inplace_rename: bool,
}

/// Next / Previous Parameter: where the caret goes, while parameter info is shown and nothing
/// else owns ⇥.
pub fn move_to_argument(shown: bool, owners: TabOwners, text: &str, head: u32, forward: bool) -> Option<u32> {
    if !shown || owners != TabOwners::default() {
        return None;
    }
    argument_stop(text, head, forward).filter(|to| *to != head)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_lsp::types::Signature;

    fn help(label: &str, parameters: Vec<(u32, u32)>) -> SignatureHelp {
        SignatureHelp {
            signatures: vec![Signature { label: label.into(), documentation: None, parameters }],
            active_signature: 0,
            active_parameter: 0,
        }
    }

    #[test]
    fn splits_around_the_active_parameter() {
        let label = "fn create(req: Request, user: Long) -> Order";
        assert_eq!(
            split_signature(label, &[(10, 22), (24, 34)], 1),
            SignatureParts {
                before: "fn create(req: Request, ".into(),
                param: "user: Long".into(),
                after: ") -> Order".into(),
            }
        );
        assert_eq!(split_signature(label, &[], 0).param, "");
    }

    #[test]
    fn opens_after_typing_the_parenthesis_even_when_the_closing_one_is_auto_inserted() {
        let mut session = SignatureSession::default();
        // `(` typed after `f`, `)` inserted with it: "f()" with the caret between them.
        let typed = [TextChange { from: 1, to: 1, insert: "()".into() }];
        assert_eq!(session.update(&typed, true, Some('('), Some(1000)), SignatureFollowUp::AutoPopup(1000));
        assert_eq!(session.answer(Some(help("f(a: number)", vec![(2, 11)])), true, false, 2, 3), None);
        let shown = session.shown().map(|shown| signature_rows(&shown.help).0);
        assert_eq!(shown.map(|rows| rows[0].parts.param.clone()).as_deref(), Some("a: number"));
    }

    #[test]
    fn rejects_overloads_with_too_few_parameters_unless_the_last_is_variadic() {
        assert!(!accepts_parameter("println() : void", &[], 0));
        assert!(accepts_parameter("println(Object x) : void", &[(8, 16)], 0));
        assert!(!accepts_parameter("println(Object x) : void", &[(8, 16)], 1));
        assert!(accepts_parameter("format(String f, Object... a)", &[(7, 15), (17, 28)], 4));
    }

    fn slices(text: &str, open: u32) -> Vec<String> {
        argument_spans(text, open).into_iter().map(|span| utf16::slice(text, span.start, span.end).to_owned()).collect()
    }

    #[test]
    fn splits_arguments_at_top_level_commas_outside_strings_and_nested_brackets() {
        assert_eq!(slices(r#"f(a, g(b, c), "x, y", [1, 2])"#, 1), ["a", "g(b, c)", "\"x, y\"", "[1, 2]"]);
        assert!(argument_spans("f()", 1).is_empty());
    }

    /// The text with `|` at the caret after ⇥ / ⇧⇥ from the `|` of `source`.
    fn stop(source: &str, forward: bool) -> Option<String> {
        let at = source.find('|').expect("the source marks the caret");
        let text = source.replacen('|', "", 1);
        let to = argument_stop(&text, utf16::len(&source[..at]), forward)?;
        Some(format!("{}|{}", utf16::slice(&text, 0, to), utf16::slice_from(&text, to)))
    }

    #[test]
    fn moves_to_the_start_of_the_next_or_previous_argument_not_past_the_ends() {
        assert_eq!(stop("f(a|b, c)", true).as_deref(), Some("f(ab, |c)"));
        assert_eq!(stop("f(ab, c|)", false).as_deref(), Some("f(|ab, c)"));
        assert_eq!(stop("f(ab,| c)", true), None);
        assert_eq!(stop("f(ab, c|)", true), None);
        assert_eq!(stop("f(a, |)", false).as_deref(), Some("f(|a, )"));
        assert_eq!(stop("f(a, g(|x, y), z)", true).as_deref(), Some("f(a, g(x, |y), z)"));
        assert_eq!(stop(r#"f("(", |b)"#, false).as_deref(), Some(r#"f(|"(", b)"#));
    }

    #[test]
    fn tab_moves_while_parameter_info_is_shown_and_is_left_alone_otherwise() {
        let owners = TabOwners::default();
        assert_eq!(move_to_argument(false, owners, "f(a, b)", 3, true), None);
        assert_eq!(move_to_argument(true, owners, "f(a, b)", 3, true), Some(5));
        let lookup = TabOwners { lookup_open: true, ..owners };
        assert_eq!(move_to_argument(true, lookup, "f(a, b)", 3, true), None);
    }
}
