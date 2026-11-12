//! Completion items as rows of the lookup: what a row filters on and shows, which items are the
//! same, where an auto-import comes from, and the characters that ask for completion.

use crate::editor::buffer;
use ide_core::TextEdit;
use ide_lsp::types::{CompletionItem, CompletionKind};
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

/// The symbol badge (`symbol-<kind>` icon) of a completion kind.
pub fn completion_symbol_kind(kind: Option<CompletionKind>) -> &'static str {
    let Some(kind) = kind else { return "text" };
    match kind {
        CompletionKind::Text => "text",
        CompletionKind::Method => "method",
        CompletionKind::Function => "function",
        CompletionKind::Constructor => "constructor",
        CompletionKind::Field | CompletionKind::Event => "field",
        CompletionKind::Variable | CompletionKind::Reference => "variable",
        CompletionKind::Class => "class",
        CompletionKind::Interface => "interface",
        CompletionKind::Module => "module",
        CompletionKind::Property => "property",
        CompletionKind::Unit
        | CompletionKind::Value
        | CompletionKind::Color
        | CompletionKind::EnumMember
        | CompletionKind::Constant => "constant",
        CompletionKind::Enum => "enum",
        CompletionKind::Keyword | CompletionKind::Operator => "keyword",
        CompletionKind::Snippet => "snippet",
        CompletionKind::File => "file",
        CompletionKind::Folder => "package",
        CompletionKind::Struct => "struct",
        CompletionKind::TypeParameter => "type",
    }
}

/// How an item shows in the lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupEntry {
    /// What typing filters on: the item's filter text when it differs from the label.
    pub label: String,
    /// The label shown when it is not [`LookupEntry::label`].
    pub display_label: Option<String>,
    /// Shown right after the label (`(req: CreateOrderRequest)`).
    pub detail: Option<String>,
    /// The symbol badge, see [`completion_symbol_kind`].
    pub kind: &'static str,
    pub sort_text: Option<String>,
    /// Ranking adjustment: preselected items first, deprecated ones last.
    pub boost: Option<i32>,
    pub commit_characters: Vec<String>,
}

/// Whether a character continues an identifier.
pub fn is_word_char(c: char) -> bool {
    c.is_alphabetic() || c.is_numeric() || c == '_' || c == '$'
}

/// A filter text that is the label plus what the item inserts after it (`String(${0})`,
/// `List<>()`): typing a word never reaches that part, and matching on it would rank the exact
/// `String` like `StringBuilder`.
fn extends_label(item: &CompletionItem, filter: &str) -> bool {
    filter.strip_prefix(item.label.as_str()).and_then(|rest| rest.chars().next()).is_some_and(|c| !is_word_char(c))
}

/// The row of `item`.
pub fn lookup_entry(item: &CompletionItem) -> LookupEntry {
    let filter = item
        .filter_text
        .as_deref()
        .filter(|filter| !filter.is_empty() && *filter != item.label && !extends_label(item, filter));
    LookupEntry {
        label: filter.unwrap_or(&item.label).to_owned(),
        display_label: filter.map(|_| item.label.clone()),
        detail: item.label_detail.clone().filter(|detail| !detail.is_empty()),
        kind: completion_symbol_kind(item.kind),
        sort_text: item.sort_text.clone().filter(|sort| !sort.is_empty()),
        boost: if item.preselect {
            Some(99)
        } else if item.deprecated {
            Some(-20)
        } else {
            None
        },
        commit_characters: item.commit_characters.clone(),
    }
}

/// Items that look and insert the same, listed once (a server may repeat a class once per
/// classpath entry declaring it).
pub fn without_duplicates(items: Vec<CompletionItem>) -> Vec<CompletionItem> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| {
            let key = serde_json::to_string(&(
                item.server,
                &item.label,
                &item.label_detail,
                &item.detail,
                item.kind,
                &item.insert_text,
                &item.additional_edits,
            ))
            .unwrap_or_default();
            seen.insert(key)
        })
        .collect()
}

static IMPORT_SOURCE: LazyLock<[Regex; 4]> = LazyLock::new(|| {
    [
        // Python
        pattern(r"(?m)^\s*from\s+([A-Za-z0-9_.]+)\s+import\b"),
        // Java, Kotlin
        pattern(r"(?m)^\s*import\s+(?:static\s+)?([A-Za-z0-9_.]+?)\.[A-Za-z0-9_]+;?\s*$"),
        // Rust
        pattern(r"(?m)^\s*use\s+([A-Za-z0-9_:]+)::"),
        // JavaScript, TypeScript
        pattern(r#"\bfrom\s+["']([^"']+)["']"#),
    ]
});

/// Where an auto-import item comes from (`collections`, `java.util`): the module its import edit
/// names. Some servers say it nowhere else, so same-named items would look identical.
pub fn import_source(additional_edits: &[TextEdit]) -> Option<String> {
    additional_edits.iter().find_map(|edit| {
        IMPORT_SOURCE
            .iter()
            .find_map(|re| re.captures(&edit.new_text).and_then(|found| found.get(1)).map(|m| m.as_str().to_owned()))
    })
}

/// The right-aligned type column: the detail, with the import source when it lacks one.
pub fn type_column_text(detail: Option<&str>, additional_edits: &[TextEdit]) -> Option<String> {
    static SPACES: LazyLock<Regex> = LazyLock::new(|| pattern(r"\s+"));
    static AUTO_IMPORT: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^auto[- ]?import$"));
    let detail = detail.map(|detail| SPACES.replace_all(detail, " ").into_owned()).filter(|detail| !detail.is_empty());
    let Some(source) = import_source(additional_edits) else { return detail };
    match detail {
        Some(detail) if detail.contains(&source) => Some(detail),
        Some(detail) if !AUTO_IMPORT.is_match(&detail) => Some(format!("{detail} ({source})")),
        _ => Some(source),
    }
}

/// The end (UTF-16 column) of the identifier starting at column `pos` of `text`.
pub fn identifier_end(text: &str, pos: u32) -> u32 {
    let start = buffer::byte_of_col(text, pos);
    pos + text[start..].chars().take_while(|c| is_word_char(*c)).map(|c| c.len_utf16() as u32).sum::<u32>()
}

/// The trigger character for the text just before the caret, if any.
pub fn trigger_character(before: &str) -> Option<char> {
    let mut back = before.chars().rev();
    let last = back.next()?;
    let previous = back.next();
    match last {
        '.' => (previous != Some('.')).then_some('.'),
        ':' => (previous == Some(':')).then_some(':'),
        '>' => (previous == Some('-')).then_some('>'),
        '@' => Some('@'),
        _ => None,
    }
}

/// The language constructs whose documentation the editor writes itself: the server has no
/// declaration to show for them.
const PHP_CONSTRUCTS: [(&str, &str, &str); 2] = [
    (
        "empty",
        "empty(mixed $var): bool",
        "Returns true when the value is falsey or the variable does not exist. Checking an undefined variable does \
         not emit a warning.",
    ),
    (
        "isset",
        "isset(mixed $var, mixed ...$vars): bool",
        "Returns true only when every supplied variable exists and is not null. Checks stop at the first unset or \
         null variable.",
    ),
];

/// Markdown documentation of a language construct named `name` (PHP's `empty`, `isset`).
pub fn construct_documentation(language: Option<&str>, name: &str) -> Option<String> {
    if !language.is_some_and(|language| language.eq_ignore_ascii_case("php")) {
        return None;
    }
    let key = name.to_lowercase();
    let (_, signature, description) = PHP_CONSTRUCTS.iter().find(|(construct, _, _)| *construct == key)?;
    Some(
        [
            format!("```php\n{signature}\n```"),
            (*description).to_owned(),
            "PHP language construct; there is no source declaration to navigate to.".to_owned(),
            format!("[PHP manual](https://www.php.net/manual/en/function.{key}.php)"),
        ]
        .join("\n\n"),
    )
}

#[cfg(test)]
pub(crate) mod testing {
    use ide_core::ServerId;
    use ide_lsp::types::{CompletionItem, CompletionKind};

    /// A plain method item named `create`.
    pub(crate) fn item() -> CompletionItem {
        CompletionItem {
            label: "create".into(),
            label_detail: None,
            detail: None,
            kind: Some(CompletionKind::Method),
            documentation: None,
            sort_text: None,
            filter_text: None,
            preselect: false,
            deprecated: false,
            insert_text: "create".into(),
            is_snippet: false,
            replace_range: None,
            replace_range_tab: None,
            additional_edits: Vec::new(),
            commit_characters: Vec::new(),
            command: None,
            data: None,
            server: ServerId(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::item;
    use super::*;
    use ide_core::{Pos, Range, ServerId};

    fn edit(new_text: &str) -> Vec<TextEdit> {
        vec![TextEdit { range: Range::new(Pos::new(0, 0), Pos::new(0, 0)), new_text: new_text.into() }]
    }

    #[test]
    fn maps_label_detail_kind_and_sort_text() {
        let entry = lookup_entry(&CompletionItem {
            label_detail: Some("(req: CreateOrderRequest)".into()),
            detail: Some("Order".into()),
            sort_text: Some("0001".into()),
            commit_characters: vec!["(".into()],
            ..item()
        });
        assert_eq!(entry.label, "create");
        assert_eq!(entry.detail.as_deref(), Some("(req: CreateOrderRequest)"));
        assert_eq!(entry.kind, "method");
        assert_eq!(entry.sort_text.as_deref(), Some("0001"));
        assert_eq!(entry.commit_characters, ["("]);
        assert_eq!(entry.display_label, None);
        assert_eq!(entry.boost, None);
    }

    #[test]
    fn filters_on_filter_text_while_displaying_the_label() {
        let entry = lookup_entry(&CompletionItem {
            label: "@GetMapping".into(),
            filter_text: Some("GetMapping".into()),
            ..item()
        });
        assert_eq!(entry.label, "GetMapping");
        assert_eq!(entry.display_label.as_deref(), Some("@GetMapping"));
    }

    #[test]
    fn filters_on_the_label_when_filter_text_only_adds_the_call_or_body() {
        let cases = [("String", "String(${0})"), ("List", "List<>()"), ("Runnable", "Runnable() {\n\t${0}\n};")];
        for (label, filter_text) in cases {
            let entry =
                lookup_entry(&CompletionItem { label: label.into(), filter_text: Some(filter_text.into()), ..item() });
            assert_eq!(entry.label, label);
            assert_eq!(entry.display_label, None);
        }
        let entry =
            lookup_entry(&CompletionItem { label: "foo".into(), filter_text: Some("foo_bar".into()), ..item() });
        assert_eq!(entry.label, "foo_bar");
    }

    #[test]
    fn lists_identical_items_once() {
        let ctor = CompletionItem {
            label: "String()".into(),
            kind: Some(CompletionKind::Constructor),
            data: Some("1".into()),
            ..item()
        };
        let overload = CompletionItem {
            label: "String()".into(),
            kind: Some(CompletionKind::Constructor),
            label_detail: Some("(bytes: ByteArray)".into()),
            ..item()
        };
        let other_server = CompletionItem { server: ServerId(2), ..ctor.clone() };
        let out = without_duplicates(vec![
            ctor.clone(),
            CompletionItem { data: Some("2".into()), ..ctor.clone() },
            overload.clone(),
            other_server.clone(),
        ]);
        let json = |items: &[CompletionItem]| serde_json::to_value(items).expect("items serialize");
        assert_eq!(json(&out), json(&[ctor, overload, other_server]));
    }

    #[test]
    fn boosts_preselected_and_demotes_deprecated_items() {
        assert_eq!(lookup_entry(&CompletionItem { preselect: true, ..item() }).boost, Some(99));
        assert!(lookup_entry(&CompletionItem { deprecated: true, ..item() }).boost.is_some_and(|boost| boost < 0));
    }

    #[test]
    fn maps_completion_kinds_to_symbol_badges() {
        assert_eq!(completion_symbol_kind(Some(CompletionKind::Function)), "function");
        assert_eq!(completion_symbol_kind(Some(CompletionKind::EnumMember)), "constant");
        assert_eq!(completion_symbol_kind(Some(CompletionKind::TypeParameter)), "type");
        assert_eq!(completion_symbol_kind(None), "text");
    }

    #[test]
    fn detects_trigger_characters() {
        assert_eq!(trigger_character("a."), Some('.'));
        assert_eq!(trigger_character(".."), None);
        assert_eq!(trigger_character("a::"), Some(':'));
        assert_eq!(trigger_character("a:"), None);
        assert_eq!(trigger_character("a->"), Some('>'));
        assert_eq!(trigger_character("a>"), None);
        assert_eq!(trigger_character(" @"), Some('@'));
        assert_eq!(trigger_character("ab"), None);
    }

    #[test]
    fn finds_identifier_ends() {
        assert_eq!(identifier_end("foo.barBaz(1)", 4), 10);
        assert_eq!(identifier_end("x ", 1), 1);
    }

    #[test]
    fn names_the_module_an_auto_import_comes_from() {
        assert_eq!(
            type_column_text(Some("Auto-import"), &edit("\nfrom collections import OrderedDict")).as_deref(),
            Some("collections")
        );
        assert_eq!(
            type_column_text(Some("Map<K, V>"), &edit("import java.util.HashMap;\n")).as_deref(),
            Some("Map<K, V> (java.util)")
        );
        assert_eq!(
            type_column_text(None, &edit("use std::collections::HashMap;\n")).as_deref(),
            Some("std::collections")
        );
        assert_eq!(type_column_text(None, &edit("import { a } from \"./money\";\n")).as_deref(), Some("./money"));
    }

    #[test]
    fn keeps_a_detail_that_already_says_where() {
        assert_eq!(
            type_column_text(Some("java.util.ArrayList"), &edit("import java.util.ArrayList;\n")).as_deref(),
            Some("java.util.ArrayList")
        );
        assert_eq!(type_column_text(Some("fn()  "), &[]).as_deref(), Some("fn() "));
    }
}
