//! Language-server snippets as the editor's snippet templates.
//!
//! A server writes `$1`, `${1}`, `${1:default}` (nestable), `${1|a,b|}`, `$0`, `$VAR`,
//! `${VAR:default}`, transforms `${1/re/fmt/}` and backslash escapes for `$ } \`. The editor's
//! templates ([`crate::templates::snippet`]) know `${1}`, `${1:name}`, `${name}` and `${0}`; their
//! literal braces are escaped as `\{` / `\}`, and a field's name cannot contain braces or span
//! lines. Nested placeholders are therefore flattened into the text of their parent.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Text(String),
    Tab { n: u64, children: Option<Vec<Node>> },
    Choice { n: u64, options: Vec<String> },
    Var { name: String, children: Option<Vec<Node>> },
}

struct Parser {
    src: Vec<char>,
    i: usize,
}

impl Parser {
    fn at(&self, i: usize) -> Option<char> {
        self.src.get(i).copied()
    }

    fn sequence(&mut self, inside: bool) -> Vec<Node> {
        fn flush(text: &mut String, out: &mut Vec<Node>) {
            if !text.is_empty() {
                out.push(Node::Text(std::mem::take(text)));
            }
        }
        let mut out = Vec::new();
        let mut text = String::new();
        while let Some(c) = self.at(self.i) {
            if c == '\\'
                && let Some(next) = self.at(self.i + 1)
                && matches!(next, '$' | '}' | '\\')
            {
                text.push(next);
                self.i += 2;
                continue;
            }
            if inside && c == '}' {
                break;
            }
            if c == '$'
                && let Some(node) = self.dollar()
            {
                flush(&mut text, &mut out);
                out.push(node);
                continue;
            }
            text.push(c);
            self.i += 1;
        }
        flush(&mut text, &mut out);
        out
    }

    fn int(&mut self) -> Option<u64> {
        let start = self.i;
        while self.at(self.i).is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        if self.i == start {
            return None;
        }
        let digits: String = self.src[start..self.i].iter().collect();
        Some(digits.parse().unwrap_or(u64::MAX))
    }

    fn name(&mut self) -> Option<String> {
        let start = self.i;
        if !self.at(self.i).is_some_and(|c| c == '_' || c.is_ascii_alphabetic()) {
            return None;
        }
        while self.at(self.i).is_some_and(|c| c == '_' || c.is_ascii_alphanumeric()) {
            self.i += 1;
        }
        Some(self.src[start..self.i].iter().collect())
    }

    /// Skips a `/regex/format/options}` transform, leaving the position after the closing brace.
    fn skip_transform(&mut self) -> bool {
        self.i += 1;
        // 0 regex, 1 format, 2 options.
        let mut segment = 0;
        let mut depth = 0;
        while let Some(c) = self.at(self.i) {
            if c == '\\' {
                self.i += 2;
                continue;
            }
            if segment < 2 && c == '/' && depth == 0 {
                segment += 1;
            } else if segment == 1 && c == '{' {
                depth += 1;
            } else if segment == 1 && c == '}' && depth > 0 {
                depth -= 1;
            } else if segment == 2 && c == '}' {
                self.i += 1;
                return true;
            }
            self.i += 1;
        }
        false
    }

    fn choice(&mut self, n: u64) -> Option<Node> {
        let mut options = Vec::new();
        let mut current = String::new();
        while let Some(c) = self.at(self.i) {
            let next = self.at(self.i + 1);
            if c == '\\'
                && let Some(next) = next
                && matches!(next, '$' | '}' | '\\' | ',' | '|')
            {
                current.push(next);
                self.i += 2;
            } else if c == ',' {
                options.push(std::mem::take(&mut current));
                self.i += 1;
            } else if c == '|' && next == Some('}') {
                options.push(current);
                self.i += 2;
                return Some(Node::Choice { n, options });
            } else {
                current.push(c);
                self.i += 1;
            }
        }
        None
    }

    /// The children of a `${…:` placeholder up to its closing brace.
    fn children(&mut self) -> Option<Vec<Node>> {
        self.i += 1;
        let children = self.sequence(true);
        if self.at(self.i) != Some('}') {
            return None;
        }
        self.i += 1;
        Some(children)
    }

    fn braced(&mut self) -> Option<Node> {
        if let Some(n) = self.int() {
            return match self.at(self.i) {
                Some('}') => {
                    self.i += 1;
                    Some(Node::Tab { n, children: None })
                }
                Some(':') => self.children().map(|children| Node::Tab { n, children: Some(children) }),
                Some('|') => {
                    self.i += 1;
                    self.choice(n)
                }
                Some('/') => self.skip_transform().then_some(Node::Tab { n, children: None }),
                _ => None,
            };
        }
        let name = self.name()?;
        match self.at(self.i) {
            Some('}') => {
                self.i += 1;
                Some(Node::Var { name, children: None })
            }
            Some(':') => self.children().map(|children| Node::Var { name, children: Some(children) }),
            Some('/') => self.skip_transform().then_some(Node::Var { name, children: None }),
            _ => None,
        }
    }

    /// The node a `$` starts; `None` (and the position unchanged) when it is plain text.
    fn dollar(&mut self) -> Option<Node> {
        let start = self.i;
        self.i += 1;
        let node = if self.at(self.i) == Some('{') {
            self.i += 1;
            self.braced()
        } else if let Some(n) = self.int() {
            Some(Node::Tab { n, children: None })
        } else {
            self.name().map(|name| Node::Var { name, children: None })
        };
        if node.is_none() {
            self.i = start;
        }
        node
    }
}

fn parse(src: &str) -> Vec<Node> {
    Parser { src: src.chars().collect(), i: 0 }.sequence(false)
}

/// Values of snippet variables by name; a name that is absent is unknown.
pub type SnippetVariables = HashMap<String, String>;

fn escape_text(text: &str) -> String {
    text.replace('{', "\\{").replace('}', "\\}")
}

/// Plain text of nodes (nested fields contribute their default text).
fn flatten(nodes: &[Node], vars: &SnippetVariables) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text(text) => out.push_str(text),
            Node::Tab { children, .. } => {
                if let Some(children) = children {
                    out.push_str(&flatten(children, vars));
                }
            }
            Node::Choice { options, .. } => out.push_str(options.first().map_or("", String::as_str)),
            Node::Var { name, children } => match (vars.get(name), children) {
                (Some(value), _) => out.push_str(value),
                (None, Some(children)) => out.push_str(&flatten(children, vars)),
                (None, None) => out.push_str(name),
            },
        }
    }
    out
}

fn field(name: &str, text: &str) -> String {
    if text.is_empty() {
        return format!("${{{name}}}");
    }
    // A field's name cannot contain braces or line breaks: keep the text, lose the selection.
    if text.contains(['{', '}', '\n', '\r']) {
        return format!("${{{name}}}{}", escape_text(text));
    }
    format!("${{{name}:{text}}}")
}

/// A server snippet as an editor snippet template.
pub fn lsp_snippet_to_template(snippet: &str, vars: &SnippetVariables) -> String {
    let mut out = String::new();
    for node in parse(snippet) {
        match node {
            Node::Text(text) => out.push_str(&escape_text(&text)),
            Node::Tab { n, children } => {
                out.push_str(&field(&n.to_string(), &children.map(|c| flatten(&c, vars)).unwrap_or_default()));
            }
            Node::Choice { n, options } => {
                out.push_str(&field(&n.to_string(), options.first().map_or("", String::as_str)));
            }
            Node::Var { name, children } => match (vars.get(&name), children) {
                (Some(value), _) => out.push_str(&escape_text(value)),
                (None, Some(children)) => out.push_str(&escape_text(&flatten(&children, vars))),
                // An unknown variable: its name is inserted as a placeholder.
                (None, None) => out.push_str(&field(&name, "")),
            },
        }
    }
    out
}

/// The snippet's text with every field replaced by its default (for previews and plain insertion).
pub fn lsp_snippet_to_plain(snippet: &str, vars: &SnippetVariables) -> String {
    flatten(&parse(snippet), vars)
}

/// The local date and time the `CURRENT_…` variables read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SnippetClock {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// The standard snippet variables for the file at `path` (`/`-separated), `extra` overriding them.
pub fn snippet_variables(path: &str, now: SnippetClock, extra: &[(&str, &str)]) -> SnippetVariables {
    let slash = path.rfind('/');
    let name = slash.map_or(path, |slash| &path[slash + 1..]);
    let base = match name.rfind('.') {
        Some(dot) if dot > 0 => &name[..dot],
        _ => name,
    };
    let directory = match slash {
        Some(slash) if slash > 0 => &path[..slash],
        _ => "",
    };
    let year = now.year.to_string();
    let short_year = year[year.len().saturating_sub(2)..].to_owned();
    let pad = |value: u32| format!("{value:02}");
    let mut vars: SnippetVariables = [
        ("TM_FILENAME", name.to_owned()),
        ("TM_FILENAME_BASE", base.to_owned()),
        ("TM_DIRECTORY", directory.to_owned()),
        ("TM_FILEPATH", path.to_owned()),
        ("TM_SELECTED_TEXT", String::new()),
        ("CLIPBOARD", String::new()),
        ("CURRENT_YEAR", year),
        ("CURRENT_YEAR_SHORT", short_year),
        ("CURRENT_MONTH", pad(now.month)),
        ("CURRENT_DATE", pad(now.day)),
        ("CURRENT_HOUR", pad(now.hour)),
        ("CURRENT_MINUTE", pad(now.minute)),
        ("CURRENT_SECOND", pad(now.second)),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect();
    vars.extend(extra.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())));
    vars
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::snippet::testing::applied;

    fn template(snippet: &str) -> String {
        lsp_snippet_to_template(snippet, &SnippetVariables::new())
    }

    /// The text and the selection after `template` went into an empty document.
    fn expand(template: &str) -> (String, Vec<(u32, u32)>) {
        applied("", template, 0, 0)
    }

    #[test]
    fn converts_tab_stops_placeholders_and_the_final_position() {
        assert_eq!(template("foo($1, ${2:bar})$0"), "foo(${1}, ${2:bar})${0}");
        assert_eq!(template("${1}"), "${1}");
    }

    #[test]
    fn escapes_literal_braces_for_the_template() {
        let t = template("fn main() {\n\t$0\n}");
        assert_eq!(t, "fn main() \\{\n\t${0}\n\\}");
        assert_eq!(expand(&t).0, "fn main() {\n  \n}");
    }

    #[test]
    fn handles_lsp_escapes() {
        assert_eq!(template("cost \\$5 \\} \\\\"), "cost $5 \\} \\");
        assert_eq!(expand(&template("a \\${1}")).0, "a ${1}");
    }

    #[test]
    fn flattens_nested_placeholders_into_the_outer_field() {
        assert_eq!(template("${1:foo(${2:bar})}"), "${1:foo(bar)}");
    }

    #[test]
    fn uses_the_first_choice() {
        assert_eq!(template("${1|public,private|} x"), "${1:public} x");
    }

    #[test]
    fn resolves_variables_defaults_and_unknown_names() {
        let vars = snippet_variables("/p/src/Main.kt", SnippetClock::default(), &[]);
        assert_eq!(lsp_snippet_to_template("class $TM_FILENAME_BASE", &vars), "class Main");
        assert_eq!(lsp_snippet_to_template("${NOPE:dflt}", &vars), "dflt");
        assert_eq!(lsp_snippet_to_template("$NOPE", &vars), "${NOPE}");
        assert_eq!(vars["TM_DIRECTORY"], "/p/src");
    }

    #[test]
    fn ignores_transforms() {
        assert_eq!(template("${1/(.*)/${1:/upcase}/}x"), "${1}x");
    }

    #[test]
    fn keeps_placeholder_text_whose_braces_a_field_cannot_name() {
        assert_eq!(template("${1:{}}"), "${1}\\{\\}");
    }

    #[test]
    fn treats_a_lone_dollar_as_text() {
        assert_eq!(template("$ x $"), "$ x $");
    }

    #[test]
    fn selects_the_first_field_when_instantiated() {
        let (text, selection) = expand(&template("call(${1:arg}, $2)$0"));
        assert_eq!(text, "call(arg, )");
        assert_eq!(selection, [(5, 8)]);
    }

    #[test]
    fn produces_plain_text_previews() {
        assert_eq!(lsp_snippet_to_plain("foo(${1:a}, ${2|x,y|})$0", &SnippetVariables::new()), "foo(a, x)");
    }
}
