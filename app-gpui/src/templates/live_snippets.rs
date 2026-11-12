//! Snippets contributed by plugins, as live templates: the numbered placeholders of a snippet body
//! (`$1`, `${2:default}`, `$0`) become linked template variables in the same tab order.

use super::live_model::{LiveTemplate, TemplateScope, TemplateVariable};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// What the ids of the templates made from plugin snippets start with.
pub const PLUGIN_SNIPPET_PREFIX: &str = "plugin-snippet:";

/// A snippet a plugin contributes for a language.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContributedSnippet {
    pub id: String,
    pub plugin_name: String,
    pub language: String,
    /// The abbreviations that expand the snippet.
    pub prefixes: Vec<String>,
    pub body: String,
    pub description: String,
}

/// `text` without the backslashes escaping `$`, `}` and `\`.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match chars.peek() {
            Some(&next @ ('$' | '}' | '\\')) if c == '\\' => {
                out.push(next);
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

/// One template per prefix of `snippet`, read-only and applicable everywhere in its language.
pub fn snippet_templates(snippet: &ContributedSnippet) -> Vec<LiveTemplate> {
    // `$1`, `${1}` or `${1:default}` at the start of the text.
    static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^\$(?:([0-9]+)|\{([0-9]+)(?::((?:\\.|[^}])*))?\})").expect("the pattern is valid")
    });
    let source = snippet.body.as_str();
    let mut variables: BTreeMap<u64, TemplateVariable> = BTreeMap::new();
    let mut body = String::new();
    let mut at = 0;
    while let Some(c) = source[at..].chars().next() {
        let rest = &source[at..];
        let next = rest[c.len_utf8()..].chars().next();
        if c == '\\'
            && let Some(escaped @ ('$' | '}' | '\\')) = next
        {
            body.push_str(if escaped == '$' { "$$" } else { &rest[1..2] });
            at += 2;
            continue;
        }
        let Some(found) = PLACEHOLDER.captures(rest) else {
            if c == '$' {
                body.push_str("$$");
            } else {
                body.push(c);
            }
            at += c.len_utf8();
            continue;
        };
        let digits = found.get(1).or_else(|| found.get(2)).map_or("", |m| m.as_str());
        let number = digits.parse::<u64>().unwrap_or(u64::MAX);
        if number == 0 {
            body.push_str("$END$");
        } else {
            let name = format!("P{number}");
            let value = found.get(3).map(|m| unescape(m.as_str()));
            if value.is_some() || !variables.contains_key(&number) {
                // A default is an expression: the text is written as a string literal.
                let default_value = value.map(|value| serde_json::Value::String(value).to_string());
                variables.insert(number, TemplateVariable { name: name.clone(), default_value, ..Default::default() });
            }
            body.push_str(&format!("${name}$"));
        }
        at += found.get(0).map_or(1, |m| m.end());
    }
    let variables: Vec<TemplateVariable> = variables.into_values().collect();
    snippet
        .prefixes
        .iter()
        .enumerate()
        .map(|(i, abbrev)| LiveTemplate {
            plugin_origin: Some(snippet.plugin_name.clone()),
            id: format!("{}:{i}", snippet.id),
            group: snippet.plugin_name.clone(),
            abbrev: abbrev.clone(),
            description: snippet.description.clone(),
            body: body.clone(),
            contexts: vec![snippet.language.clone()],
            variables: variables.clone(),
            enabled: None,
            scope: Some(TemplateScope::Any),
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod testing {
    use super::ContributedSnippet;

    /// The snippet the specs contribute.
    pub(crate) fn select_snippet() -> ContributedSnippet {
        ContributedSnippet {
            id: "plugin-snippet:p:s:0".to_owned(),
            plugin_name: "Plugin".to_owned(),
            language: "sql".to_owned(),
            prefixes: vec!["sel".to_owned(), "select".to_owned()],
            body: "${2:table} ${1:column} $1 $0 \\$".to_owned(),
            description: "Select rows".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::select_snippet;
    use super::*;
    use crate::templates::live_model::{ExpansionEnv, expand_template};

    // plugin snippets › preserves defaults, linked stops, numeric tab order and escaped dollars
    #[test]
    fn preserves_defaults_linked_stops_numeric_tab_order_and_escaped_dollars() {
        let templates = snippet_templates(&select_snippet());
        assert_eq!(templates.iter().map(|t| t.abbrev.as_str()).collect::<Vec<_>>(), ["sel", "select"]);
        assert_eq!(templates[0].variables.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["P1", "P2"]);
        let expanded = expand_template(&templates[0].body, &templates[0].variables, &ExpansionEnv::default());
        assert_eq!(expanded.preview, "table column column  $");
        assert_eq!(expanded.snippet, "${2:table} ${1:column} ${1:column} ${0} $");
    }

    // Not from a spec: a dollar that starts no placeholder is literal, escapes inside a default are
    // dropped, and an unclosed placeholder stays text.
    #[test]
    fn keeps_literal_dollars_and_unescapes_defaults() {
        let snippet = ContributedSnippet {
            body: r"cost: $x ${1:a\}b} ${2 \\ é".to_owned(),
            prefixes: vec!["c".to_owned()],
            ..select_snippet()
        };
        let template = snippet_templates(&snippet).remove(0);
        assert_eq!(template.body, r"cost: $$x $P1$ $${2 \ é");
        assert_eq!(template.variables[0].default_value.as_deref(), Some(r#""a}b""#));
        assert_eq!(template.id, "plugin-snippet:p:s:0:0");
        assert_eq!(template.plugin_origin.as_deref(), Some("Plugin"));
    }
}
