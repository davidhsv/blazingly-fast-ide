//! Fixtures of the refactoring tests: positions and ranges of needles in a text, locations,
//! outline items and backend errors.

use crate::editor_lsp::positions::offset_to_pos;
use ide_core::error::{ErrorKind, IdeError};
use ide_core::{Location, OutlineItem, Pos, Range, ServerId, SymbolKind};
use ide_lsp::types::CodeAction;
use ropey::Rope;

/// The UTF-16 offset of the first `needle` in `text` (from byte `after` on).
pub(crate) fn offset_of(text: &str, needle: &str) -> u32 {
    offset_after(text, needle, 0)
}

pub(crate) fn offset_after(text: &str, needle: &str, after: usize) -> u32 {
    let byte = after + text[after..].find(needle).expect("the needle is in the text");
    text[..byte].encode_utf16().count() as u32
}

/// The UTF-16 offset of the last `needle` in `text`.
pub(crate) fn last_offset_of(text: &str, needle: &str) -> u32 {
    let byte = text.rfind(needle).expect("the needle is in the text");
    text[..byte].encode_utf16().count() as u32
}

pub(crate) fn pos_of(text: &str, offset: u32) -> Pos {
    offset_to_pos(&Rope::from_str(text), offset)
}

pub(crate) fn range_of(text: &str, from: u32, to: u32) -> Range {
    Range::new(pos_of(text, from), pos_of(text, to))
}

pub(crate) fn location(path: &str, range: Range) -> Location {
    Location { path: path.into(), range, uri: None }
}

/// A location on `line`, columns 0 to 1.
pub(crate) fn at_line(path: &str, line: u32) -> Location {
    location(path, Range::new(Pos::new(line, 0), Pos::new(line, 1)))
}

/// An outline item spanning `decl` with the name `name` inside it.
pub(crate) fn item(text: &str, name: &str, decl: &str, kind: SymbolKind) -> OutlineItem {
    let from_byte = text.find(decl).expect("the declaration is in the text");
    let from = offset_of(text, decl);
    let at = offset_after(text, name, from_byte);
    let to = from + decl.encode_utf16().count() as u32;
    OutlineItem {
        name: name.to_owned(),
        detail: None,
        kind,
        range: range_of(text, from, to),
        selection_range: range_of(text, at, at + name.encode_utf16().count() as u32),
        children: Vec::new(),
        visibility: None,
    }
}

pub(crate) fn unavailable() -> IdeError {
    IdeError::new(ErrorKind::Unavailable, "no server")
}

pub(crate) fn cancelled() -> IdeError {
    IdeError::new(ErrorKind::Cancelled, "cancelled")
}

/// A code action of `kind` without an edit.
pub(crate) fn code_action(title: &str, kind: &str, disabled_reason: Option<&str>) -> CodeAction {
    CodeAction {
        title: title.to_owned(),
        kind: Some(kind.to_owned()),
        is_preferred: false,
        disabled_reason: disabled_reason.map(str::to_owned),
        edit: None,
        command: None,
        data: None,
        server: ServerId(1),
    }
}
