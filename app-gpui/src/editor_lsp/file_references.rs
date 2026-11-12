//! File references while editing: the path being typed inside `href="…"`, `src="…"`, `url(…)`,
//! `@import "…"`, Markdown links and path-like string literals, which path completion lists the
//! files for.

use regex::Regex;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

/// The path typed at the caret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathContext {
    /// The path typed so far.
    pub prefix: String,
    /// Complete without an explicit request even while `prefix` does not look like a path yet.
    pub eager: bool,
}

/// Languages whose module specifiers the language server completes.
const JS_LIKE: [&str; 6] = ["javascript", "typescript", "javascriptreact", "typescriptreact", "vue", "svelte"];

/// Whether `prefix` already reads like a path (`./`, `../`, `a/b`).
pub fn path_like(prefix: &str) -> bool {
    prefix.starts_with("./") || prefix.starts_with("../") || prefix.contains('/')
}

/// The path being typed at the end of `before` (the line up to the caret), if any.
pub fn path_context(before: &str, language: &str) -> Option<PathContext> {
    static EAGER: LazyLock<[Regex; 4]> = LazyLock::new(|| {
        [
            pattern(r#"(?i)\b(?:href|src|srcset|action|formaction|poster|data|xlink:href)\s*=\s*["']?([^"'\s>]*)$"#),
            pattern(r#"\burl\(\s*["']?([^"')\s]*)$"#),
            pattern(r#"@(?:import|use|forward)\s+["']([^"']*)$"#),
            pattern(r"\]\(([^)\s]*)$"),
        ]
    });
    static STRING: LazyLock<Regex> = LazyLock::new(|| pattern(r#"(["'`])([^"'`\\]*)$"#));
    static MODULE_SPECIFIER: LazyLock<Regex> =
        LazyLock::new(|| pattern(r#"(?:\bimport\b|\bfrom\b|\brequire\s*\(|\bimport\s*\()\s*["'`][^"'`]*$"#));
    for re in EAGER.iter() {
        if let Some(found) = re.captures(before) {
            return Some(PathContext { prefix: found.get(1).map_or("", |m| m.as_str()).to_owned(), eager: true });
        }
    }
    let found = STRING.captures(before)?;
    let quote = found.get(1).map_or("", |m| m.as_str());
    // Inside a string only when the quote opens one (an odd count on the line).
    if before.matches(quote).count() % 2 == 0 {
        return None;
    }
    if JS_LIKE.contains(&language) && MODULE_SPECIFIER.is_match(before) {
        return None;
    }
    Some(PathContext { prefix: found.get(2).map_or("", |m| m.as_str()).to_owned(), eager: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(prefix: &str, eager: bool) -> Option<PathContext> {
        Some(PathContext { prefix: prefix.into(), eager })
    }

    #[test]
    fn completes_url_attributes_url_import_and_markdown_links_eagerly() {
        assert_eq!(path_context(r#"<link rel="stylesheet" href="css/"#, "html"), context("css/", true));
        assert_eq!(path_context("<img src=", "html"), context("", true));
        assert_eq!(path_context(".a { background: url(../img/lo", "css"), context("../img/lo", true));
        assert_eq!(path_context(r#"@import "the"#, "scss"), context("the", true));
        assert_eq!(path_context("See [docs](guide/", "markdown"), context("guide/", true));
    }

    #[test]
    fn completes_string_literals_only_once_they_look_like_paths() {
        assert_eq!(path_context(r#"open("data/x"#, "python"), context("data/x", false));
        assert!(path_like("data/x") && path_like("./") && path_like("../a"));
        assert!(!path_like("hello"));
        // Closed strings and module specifiers (the language server's job) are left alone.
        assert_eq!(path_context(r#"log("done") + x"#, "python"), None);
        assert_eq!(path_context(r#"import { a } from "./ut"#, "typescript"), None);
        assert_eq!(path_context(r#"const m = await import("./m"#, "javascript"), None);
        assert_eq!(path_context(r#"fetch("./data/"#, "typescript"), context("./data/", false));
    }
}
