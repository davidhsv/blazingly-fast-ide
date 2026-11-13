//! Reading the editor's syntax colours the way they are painted: the colour token of each character,
//! from the highlights the backend's grammar gave.

use super::*;

/// Opens a project with the file `name` holding `text` and waits for its first highlights.
pub(super) async fn highlighted<'a>(
    cx: &'a mut TestAppContext,
    name: &str,
    text: &str,
) -> (Fixture, &'a mut VisualTestContext) {
    let (f, cx) = open(cx, &[(name, text)]).await;
    settle(cx, |cx| !f.editor.read_with(cx, |e, _| e.highlights().is_empty()));
    (f, cx)
}

/// The colour token painted at the UTF-16 `offset`; `None` for plain text.
pub(super) fn token_at(editor: &Editor, offset: u32) -> Option<&'static str> {
    editor.highlights().iter().find(|s| s.from <= offset && offset < s.to).map(|s| syntax_token(s.kind))
}

/// The UTF-16 offsets of the non-blank characters of the `nth` (from 0) occurrence of `needle`; a needle
/// written `before«inner»after` stands for `inner` where it sits between `before` and `after`.
fn characters(text: &str, needle: &str, nth: usize) -> Option<Vec<u32>> {
    let (whole, skip, needle) = match needle.split_once('«').zip(needle.split_once('»')) {
        Some(((before, rest), (_, after))) => {
            let inner = rest.split_once('»').map_or(rest, |(inner, _)| inner);
            (format!("{before}{inner}{after}"), before.len(), inner)
        }
        None => (needle.to_owned(), 0, needle),
    };
    let (at, _) = text.match_indices(whole.as_str()).nth(nth)?;
    let start = text[..at + skip].encode_utf16().count() as u32;
    let mut offsets = Vec::new();
    let mut offset = start;
    for c in needle.chars() {
        if !c.is_whitespace() {
            offsets.push(offset);
        }
        offset += c.len_utf16() as u32;
    }
    Some(offsets)
}

/// What the editor paints, as (text, token) runs, for failure messages.
pub(super) fn runs(editor: &Editor) -> Vec<(String, &'static str)> {
    let units: Vec<u16> = editor.text().encode_utf16().collect();
    let slice = |from: u32, to: u32| String::from_utf16_lossy(&units[from as usize..(to as usize).min(units.len())]);
    editor.highlights().iter().map(|s| (slice(s.from, s.to), syntax_token(s.kind))).collect()
}

/// The expectations a test collects, each failure as a line, so that one run reports them all.
#[derive(Default)]
pub(super) struct Expect(Vec<String>);

impl Expect {
    /// Every non-blank character of the `nth` occurrence of `needle` is painted `token` (`None`: plain).
    pub(super) fn painted(&mut self, editor: &Editor, needle: &str, nth: usize, token: Option<&str>) {
        let text = editor.text();
        let Some(offsets) = characters(&text, needle, nth) else {
            self.0.push(format!("{needle:?} #{nth} is not in the text"));
            return;
        };
        let got: Vec<Option<&str>> = offsets.iter().map(|&offset| token_at(editor, offset)).collect();
        if got.iter().any(|painted| *painted != token) {
            self.0.push(format!("{needle:?} #{nth}: expected {token:?}, painted {got:?}"));
        }
    }

    /// Some character of the `nth` occurrence of `needle` is painted `token`.
    pub(super) fn partly(&mut self, editor: &Editor, needle: &str, nth: usize, token: &str) {
        let text = editor.text();
        let offsets = characters(&text, needle, nth).unwrap_or_default();
        if !offsets.iter().any(|&offset| token_at(editor, offset) == Some(token)) {
            self.0.push(format!("{needle:?} #{nth}: no character painted {token}"));
        }
    }

    /// No character of the `nth` occurrence of `needle` is painted `token`.
    pub(super) fn never(&mut self, editor: &Editor, needle: &str, nth: usize, token: &str) {
        let text = editor.text();
        let offsets = characters(&text, needle, nth).unwrap_or_default();
        if offsets.iter().any(|&offset| token_at(editor, offset) == Some(token)) {
            self.0.push(format!("{needle:?} #{nth}: painted {token}"));
        }
    }

    /// Nothing in the text is painted `token`.
    pub(super) fn nowhere(&mut self, editor: &Editor, token: &str) {
        if let Some((text, _)) = runs(editor).into_iter().find(|(_, painted)| *painted == token) {
            self.0.push(format!("{text:?} is painted {token}"));
        }
    }

    /// The expectations that failed, each prefixed with `label`, then what the editor paints when any did.
    pub(super) fn failures(self, label: &str, editor: &Editor) -> Vec<String> {
        if self.0.is_empty() {
            return Vec::new();
        }
        let mut failures: Vec<String> = self.0.into_iter().map(|failure| format!("{label}: {failure}")).collect();
        failures.push(format!("{label}: painted {:?}", runs(editor)));
        failures
    }

    /// Panics with every expectation that failed and what the editor paints.
    #[track_caller]
    pub(super) fn check(self, editor: &Editor) {
        let failures = self.failures("expected", editor);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
