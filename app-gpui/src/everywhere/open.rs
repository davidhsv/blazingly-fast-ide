//! Pieces the choosers share: paths relative to the project, how symbols are named, values saved to the UI
//! storage and text measured in UTF-16 code units.

use ide_core::SymbolKind;
use serde::Serialize;
use serde_json::Value;

/// A value to save in the UI storage: `value` is the JSON text to keep under `key`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct StorageWrite {
    pub key: String,
    pub value: String,
}

/// The write that saves `value` under `key`.
pub fn storage_write(key: &str, value: &impl Serialize) -> StorageWrite {
    // Strings, numbers, lists and maps keyed by strings always serialize.
    let value = serde_json::to_string(value).expect("plain data serializes to JSON");
    StorageWrite { key: key.to_owned(), value }
}

/// A stored value parsed as JSON; `None` when nothing is stored or the text is not JSON.
pub fn stored_json(stored: Option<&str>) -> Option<Value> {
    stored.filter(|text| !text.is_empty()).and_then(|text| serde_json::from_str(text).ok())
}

/// Directory part of a project-relative path ("" for the root).
pub fn rel_dir(rel: &str) -> &str {
    rel.rfind('/').map_or("", |slash| &rel[..slash])
}

/// `path` relative to `root` when it lies below it, else `path` itself.
pub fn rel_to_root<'a>(root: Option<&str>, path: &'a str) -> &'a str {
    root.filter(|root| !root.is_empty())
        .and_then(|root| path.strip_prefix(root))
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(path)
}

/// Display name of a symbol, IDEA style: callables get `()`.
pub fn symbol_display_name(name: &str, kind: SymbolKind) -> String {
    let callable = matches!(kind, SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor);
    if callable && !name.ends_with(')') { format!("{name}()") } else { name.to_owned() }
}

/// The name of a symbol kind as the backend serializes it (`typeAlias`). Keys and stored filters are built
/// from it.
pub const fn kind_name(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::File => "file",
        SymbolKind::Module => "module",
        SymbolKind::Namespace => "namespace",
        SymbolKind::Package => "package",
        SymbolKind::Class => "class",
        SymbolKind::Interface => "interface",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::EnumMember => "enumMember",
        SymbolKind::Trait => "trait",
        SymbolKind::Function => "function",
        SymbolKind::Method => "method",
        SymbolKind::Constructor => "constructor",
        SymbolKind::Field => "field",
        SymbolKind::Property => "property",
        SymbolKind::Variable => "variable",
        SymbolKind::Constant => "constant",
        SymbolKind::TypeParameter => "typeParameter",
        SymbolKind::TypeAlias => "typeAlias",
        SymbolKind::Macro => "macro",
        SymbolKind::Event => "event",
        SymbolKind::Operator => "operator",
        SymbolKind::Key => "key",
        SymbolKind::Unknown => "unknown",
    }
}

/// Length of `text` in UTF-16 code units.
pub fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Byte offset of the character `units` UTF-16 code units into `text`; the end of the text when it is
/// shorter. An offset inside a surrogate pair counts as the character after it.
pub fn byte_at_utf16(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (offset, c) in text.char_indices() {
        if seen >= units {
            return offset;
        }
        seen += c.len_utf16();
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: the directory of a relative path and paths below, at and outside the root.
    #[test]
    fn relative_paths_and_their_directories() {
        assert_eq!(rel_dir("src/orders/Order.kt"), "src/orders");
        assert_eq!(rel_dir("Order.kt"), "");
        assert_eq!(rel_to_root(Some("/p"), "/p/src/a.rs"), "src/a.rs");
        assert_eq!(rel_to_root(Some("/p"), "/pother/a.rs"), "/pother/a.rs");
        assert_eq!(rel_to_root(Some("/p"), "/p"), "/p");
        assert_eq!(rel_to_root(Some(""), "/p/a.rs"), "/p/a.rs");
        assert_eq!(rel_to_root(None, "/p/a.rs"), "/p/a.rs");
    }

    // Not from a spec: callables get `()` once.
    #[test]
    fn callables_are_shown_with_parentheses() {
        assert_eq!(symbol_display_name("run", SymbolKind::Function), "run()");
        assert_eq!(symbol_display_name("eat(String)", SymbolKind::Method), "eat(String)");
        assert_eq!(symbol_display_name("Order", SymbolKind::Class), "Order");
    }

    // Not from a spec: kind names are the ones the backend serializes.
    #[test]
    fn kind_names_match_the_serialized_ones() {
        let kinds = [
            SymbolKind::File,
            SymbolKind::Module,
            SymbolKind::Namespace,
            SymbolKind::Package,
            SymbolKind::Class,
            SymbolKind::Interface,
            SymbolKind::Struct,
            SymbolKind::Enum,
            SymbolKind::EnumMember,
            SymbolKind::Trait,
            SymbolKind::Function,
            SymbolKind::Method,
            SymbolKind::Constructor,
            SymbolKind::Field,
            SymbolKind::Property,
            SymbolKind::Variable,
            SymbolKind::Constant,
            SymbolKind::TypeParameter,
            SymbolKind::TypeAlias,
            SymbolKind::Macro,
            SymbolKind::Event,
            SymbolKind::Operator,
            SymbolKind::Key,
            SymbolKind::Unknown,
        ];
        for kind in kinds {
            assert_eq!(serde_json::to_value(kind).unwrap(), Value::String(kind_name(kind).to_owned()));
        }
    }

    // Not from a spec: UTF-16 lengths and offsets on text with characters outside the BMP.
    #[test]
    fn utf16_offsets_map_to_character_boundaries() {
        let text = "a😀é";
        assert_eq!(utf16_len(text), 4);
        assert_eq!(byte_at_utf16(text, 0), 0);
        assert_eq!(byte_at_utf16(text, 1), 1);
        assert_eq!(byte_at_utf16(text, 2), 5);
        assert_eq!(byte_at_utf16(text, 3), 5);
        assert_eq!(byte_at_utf16(text, 4), text.len());
        assert_eq!(byte_at_utf16(text, 9), text.len());
    }

    // Not from a spec: stored text that is missing, empty or not JSON reads as nothing.
    #[test]
    fn stored_json_is_read_leniently() {
        assert_eq!(stored_json(Some("[1]")), Some(serde_json::json!([1])));
        assert_eq!(stored_json(Some("")), None);
        assert_eq!(stored_json(Some("{oops")), None);
        assert_eq!(stored_json(None), None);
    }
}
