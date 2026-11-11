//! Search pieces shared by Find / Replace in the editor and Find in Files: the search context filter,
//! Preserve Case, the recent-search history and the user's custom (named) scopes, which are kept per project.
//!
//! Nothing here reads or writes storage: the history and the scopes are stored as JSON under the keys given
//! here, and the caller passes the stored text in and saves what comes back.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The search context filter: matches anywhere, only in, or outside comments and string literals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchContext {
    #[default]
    Any,
    InComments,
    InStringLiterals,
    ExceptComments,
    ExceptStringLiterals,
    ExceptCommentsAndStringLiterals,
}

/// Every search context with its label, in menu order.
pub const SEARCH_CONTEXTS: [(SearchContext, &str); 6] = [
    (SearchContext::Any, "Anywhere"),
    (SearchContext::InComments, "In Comments"),
    (SearchContext::InStringLiterals, "In String Literals"),
    (SearchContext::ExceptComments, "Except Comments"),
    (SearchContext::ExceptStringLiterals, "Except String Literals"),
    (SearchContext::ExceptCommentsAndStringLiterals, "Except Comments and String Literals"),
];

/// What kind of literal a match starts in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiteralKind {
    Comment,
    String,
}

/// Whether a match starting in `kind` (`None`: plain code) is kept; the backend's search filters the same way.
pub fn context_accepts(context: SearchContext, kind: Option<LiteralKind>) -> bool {
    match context {
        SearchContext::Any => true,
        SearchContext::InComments => kind == Some(LiteralKind::Comment),
        SearchContext::InStringLiterals => kind == Some(LiteralKind::String),
        SearchContext::ExceptComments => kind != Some(LiteralKind::Comment),
        SearchContext::ExceptStringLiterals => kind != Some(LiteralKind::String),
        SearchContext::ExceptCommentsAndStringLiterals => kind.is_none(),
    }
}

/// A character that is its own uppercase form and has a different lowercase one.
fn is_upper(c: char) -> bool {
    c.to_uppercase().eq([c]) && !c.to_lowercase().eq([c])
}

/// "Preserve case": the replacement takes the case pattern of the text it replaces. `FOO` upper-cases it,
/// `Foo` capitalises it and `foo` lower-cases its first letter; text without letters leaves it as typed.
pub fn preserve_case(found: &str, replacement: &str) -> String {
    let letters: Vec<char> = found.chars().filter(|c| !c.to_lowercase().eq(c.to_uppercase())).collect();
    let mut rest = replacement.chars();
    let (Some(&first), Some(head)) = (letters.first(), rest.next()) else { return replacement.to_owned() };
    if letters.len() > 1 && letters.iter().all(|&c| is_upper(c)) {
        return replacement.to_uppercase();
    }
    let head: String = if is_upper(first) { head.to_uppercase().collect() } else { head.to_lowercase().collect() };
    head + rest.as_str()
}

/// Which of the two recent-text lists an entry belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryKind {
    Find,
    Replace,
}

impl HistoryKind {
    /// Key the list is stored under.
    pub const fn storage_key(self) -> &'static str {
        match self {
            Self::Find => "openidea.search.findHistory.v1",
            Self::Replace => "openidea.search.replaceHistory.v1",
        }
    }
}

/// Most entries a history keeps.
pub const MAX_HISTORY: usize = 20;

/// Recent search (or replace) strings from their stored JSON, newest first; shared by the find bar and Find
/// in Files. Anything that is not a list reads as no history, and entries that are not strings are dropped.
pub fn parse_history(stored: &str) -> Vec<String> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(stored) else { return Vec::new() };
    items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect()
}

/// Puts `text` at the front of `history`, dropping its older occurrence and the entries beyond
/// [`MAX_HISTORY`]. Blank text is not remembered; returns whether the history changed and needs saving.
pub fn remember_search(history: &mut Vec<String>, text: &str) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    history.retain(|entry| entry != text);
    history.insert(0, text.to_owned());
    history.truncate(MAX_HISTORY);
    true
}

/// A user-defined scope: a name and an IDEA-style pattern (`file:src//*.ts&&!file:*test*`), matched by the
/// backend.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomScope {
    pub name: String,
    pub pattern: String,
}

/// Key the custom scopes of the project at `root` are stored under.
pub fn scopes_key(root: &str) -> String {
    format!("openidea.scopes.v1:{root}")
}

/// Custom scopes from their stored JSON. Anything that is not a list reads as no scopes; entries without a
/// text pattern or a name that is not blank are dropped.
pub fn parse_scopes(stored: &str) -> Vec<CustomScope> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(stored) else { return Vec::new() };
    items
        .iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?;
            let pattern = item.get("pattern")?.as_str()?;
            (!name.trim().is_empty()).then(|| CustomScope { name: name.to_owned(), pattern: pattern.to_owned() })
        })
        .collect()
}

/// The scopes as they are saved: those with a blank name dropped, names and patterns trimmed.
pub fn clean_scopes(scopes: &[CustomScope]) -> Vec<CustomScope> {
    scopes
        .iter()
        .filter(|scope| !scope.name.trim().is_empty())
        .map(|scope| CustomScope { name: scope.name.trim().to_owned(), pattern: scope.pattern.trim().to_owned() })
        .collect()
}

/// The scope called `name`, if there is one.
pub fn find_custom_scope<'a>(scopes: &'a [CustomScope], name: &str) -> Option<&'a CustomScope> {
    scopes.iter().find(|scope| scope.name == name)
}

/// Action that opens the scope editor; its argument may name the scope to select (`{ "select": "<name>" }`).
pub const EDIT_SCOPES_ACTION: &str = "search.editScopes";

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(name: &str, pattern: &str) -> CustomScope {
        CustomScope { name: name.to_owned(), pattern: pattern.to_owned() }
    }

    // src/platform/search.test.ts › shared search pieces › search context accepts matches like the backend
    #[test]
    fn search_context_accepts_matches_like_the_backend() {
        assert!(context_accepts(SearchContext::Any, Some(LiteralKind::Comment)));
        assert!(context_accepts(SearchContext::InComments, Some(LiteralKind::Comment)));
        assert!(!context_accepts(SearchContext::InComments, None));
        assert!(context_accepts(SearchContext::ExceptStringLiterals, Some(LiteralKind::Comment)));
        assert!(!context_accepts(SearchContext::ExceptStringLiterals, Some(LiteralKind::String)));
        assert!(context_accepts(SearchContext::ExceptCommentsAndStringLiterals, None));
    }

    // src/platform/search.test.ts › shared search pieces › history is shared, newest first and unique
    #[test]
    fn history_is_newest_first_and_unique() {
        let mut history = Vec::new();
        assert!(remember_search(&mut history, "a"));
        assert!(remember_search(&mut history, "b\nc"));
        assert!(remember_search(&mut history, "a"));
        assert_eq!(history, ["a", "b\nc"]);
        assert_eq!(parse_history(&serde_json::to_string(&history).unwrap()), history);
    }

    // src/platform/search.test.ts › shared search pieces › custom scopes are saved per project
    #[test]
    fn custom_scopes_are_cleaned_for_saving_and_keyed_per_project() {
        let saved = clean_scopes(&[scope(" Sources ", " file:src//* "), scope("", "x")]);
        assert_eq!(saved, [scope("Sources", "file:src//*")]);
        assert_ne!(scopes_key("/p1"), scopes_key("/p2"));
        let loaded = parse_scopes(&serde_json::to_string(&saved).unwrap());
        assert_eq!(find_custom_scope(&loaded, "Sources").map(|s| s.pattern.as_str()), Some("file:src//*"));
        assert_eq!(find_custom_scope(&loaded, "Tests"), None);
    }

    // crates/ide-search/tests/replace.rs › preserve_case_rules
    #[test]
    fn preserve_case_rules() {
        assert_eq!(preserve_case("FOO", "bar"), "BAR");
        assert_eq!(preserve_case("Foo", "bar"), "Bar");
        assert_eq!(preserve_case("foo", "Bar"), "bar");
        assert_eq!(preserve_case("F", "bar"), "Bar");
        assert_eq!(preserve_case("123", "bar"), "bar");
        assert_eq!(preserve_case("Foo", ""), "");
    }

    // Blank text is not remembered and the list is capped, as the history's doc comments say.
    #[test]
    fn history_skips_blank_text_and_is_capped() {
        let mut history = vec!["a".to_owned()];
        assert!(!remember_search(&mut history, " \n"));
        assert_eq!(history, ["a"]);
        for n in 0..MAX_HISTORY + 5 {
            remember_search(&mut history, &n.to_string());
        }
        assert_eq!(history.len(), MAX_HISTORY);
        assert_eq!(history[0], (MAX_HISTORY + 4).to_string());
    }

    // Stored text is read leniently: whatever is malformed is dropped instead of failing the whole read.
    #[test]
    fn stored_text_is_read_leniently() {
        assert_eq!(parse_history(r#"["a", 1, null, "b"]"#), ["a", "b"]);
        assert!(parse_history("{}").is_empty());
        assert!(parse_history("").is_empty());
        let stored = r#"[{"name":"A","pattern":"p"},{"name":" ","pattern":"x"},{"name":1,"pattern":"x"},null,"x"]"#;
        assert_eq!(parse_scopes(stored), [scope("A", "p")]);
        assert!(parse_scopes("not json").is_empty());
    }

    // The context crosses to the backend under these names.
    #[test]
    fn search_context_serializes_in_camel_case() {
        let json = serde_json::to_string(&SearchContext::ExceptCommentsAndStringLiterals).unwrap();
        assert_eq!(json, r#""exceptCommentsAndStringLiterals""#);
        assert_eq!(serde_json::from_str::<SearchContext>(r#""inComments""#).unwrap(), SearchContext::InComments);
        assert_eq!(SearchContext::default(), SearchContext::Any);
    }
}
