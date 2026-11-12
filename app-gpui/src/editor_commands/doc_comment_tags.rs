//! Doc comment tags, as IntelliJ colours them inside `/** … */`: a block tag at the start of a
//! line and an inline `{@tag`, the value some tags take (a `@param` name, a KDoc subject or
//! `[link]`, a PHPDoc `$variable`) and HTML markup. Only the comment nodes of the scanned range
//! are read; the colours come from the highlight style.

use super::EditState;
use super::tree::{SyntaxTree, pascal_name};
use crate::editor::buffer::col_of_byte;
use regex::Regex;
use std::sync::LazyLock;

/// How a range of a doc comment is coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocCommentTagKind {
    /// A block tag (`@param`) or an inline one (`{@link`, without the brace).
    Tag,
    /// What a tag names (`@param name`), or a KDoc `[link]`.
    Value,
    /// An HTML element's tag.
    Markup,
}

/// A coloured range of a doc comment, in UTF-16 offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocCommentTagRange {
    pub from: u32,
    pub to: u32,
    pub kind: DocCommentTagKind,
}

type ValueRule = (&'static LazyLock<Regex>, &'static LazyLock<Regex>);

/// What the doc comments of a language mark.
struct Dialect {
    /// Inline tags: `{@link …}`.
    inline: bool,
    /// HTML tags in the text.
    markup: bool,
    /// KDoc `[links]` in the text.
    links: bool,
    /// The tags taking a value, and the value in the rest of the line after one: the pattern's
    /// first group, which ends the match.
    value: Option<ValueRule>,
}

fn compile(pattern: &str) -> Regex {
    Regex::new(pattern).expect("the pattern is valid")
}

static JAVADOC_VALUE_TAGS: LazyLock<Regex> = LazyLock::new(|| compile(r"^@param$"));
static JAVADOC_VALUE: LazyLock<Regex> = LazyLock::new(|| compile(r"^\s+(<[A-Za-z0-9_]+>|[A-Za-z0-9_$]+)"));
static KDOC_VALUE_TAGS: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^@(?:param|property|throws|exception|see|sample)$"));
static KDOC_VALUE: LazyLock<Regex> = LazyLock::new(|| compile(r"^\s+(\[[^\]\n]+\]|[A-Za-z0-9_.]+)"));
static PHPDOC_VALUE_TAGS: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^@(?:[a-z]+-)?(?:param|var|property(?:-read|-write)?)$"));
static PHPDOC_VALUE: LazyLock<Regex> = LazyLock::new(|| compile(r"^.*?(?:\s|&|\.\.\.)(\$[A-Za-z_][A-Za-z0-9_]*)"));

/// Indentation and the leading `*` of a comment line.
static LEAD: LazyLock<Regex> = LazyLock::new(|| compile(r"^\s*\**\s*"));
static BLOCK_TAG: LazyLock<Regex> = LazyLock::new(|| compile(r"^@[A-Za-z][A-Za-z0-9_\\\-]*"));
static INLINE_TAG: LazyLock<Regex> = LazyLock::new(|| compile(r"\{@[A-Za-z][A-Za-z0-9_\-]*"));
/// A `[link]`; one followed by `(` or `[` is Markdown link text, which the caller skips.
static LINK: LazyLock<Regex> = LazyLock::new(|| compile(r"\[[^\[\]\n]+\]"));
/// Known HTML elements only, so generics such as `List<String>` stay text.
static MARKUP: LazyLock<Regex> = LazyLock::new(|| {
    compile(concat!(
        r"(?i)</?(?:a|abbr|b|big|blockquote|br|caption|cite|code|dd|del|details|div|dl|dt|em|h[1-6]|hr|i|img|",
        r"ins|kbd|li|ol|p|pre|q|s|samp|section|small|span|strong|sub|summary|sup|table|tbody|td|th|thead|tr|",
        r"tt|u|ul|var)(?-u:\b)[^<>]*>",
    ))
});

/// The doc comment dialect of a backend language id.
fn dialect(language: &str) -> Option<Dialect> {
    let jsdoc = Dialect { inline: true, markup: true, links: false, value: None };
    match language {
        "java" => Some(Dialect { value: Some((&JAVADOC_VALUE_TAGS, &JAVADOC_VALUE)), ..jsdoc }),
        // KDoc links the subject of some tags; a `[name]` anywhere in the text is a link too.
        "kotlin" => {
            Some(Dialect { inline: false, markup: false, links: true, value: Some((&KDOC_VALUE_TAGS, &KDOC_VALUE)) })
        }
        "javascript" | "javascriptreact" | "typescript" | "typescriptreact" => Some(jsdoc),
        "php" => Some(Dialect { value: Some((&PHPDOC_VALUE_TAGS, &PHPDOC_VALUE)), ..jsdoc }),
        _ => None,
    }
}

/// The language's doc comments have tags to colour.
pub fn has_doc_comment_tags(language: &str) -> bool {
    dialect(language).is_some()
}

/// `/**` opens a doc comment; `/**/` is an empty block comment.
fn opens_doc(head: &str) -> bool {
    head.starts_with("/**") && !head.starts_with("/**/")
}

fn is_comment(name: &str) -> bool {
    name.to_ascii_lowercase().contains("comment")
}

/// The byte range of the value a block tag takes in `rest`, the line after the tag.
fn value_of(rule: ValueRule, tag: &str, rest: &str) -> Option<(usize, usize)> {
    let (tags, value) = rule;
    if !tags.is_match(tag) {
        return None;
    }
    let group = value.captures(rest)?.get(1)?;
    Some((group.start(), group.end()))
}

/// The tag ranges of the lines of `comment` within `range`.
fn scan_comment(
    state: &EditState,
    dialect: &Dialect,
    comment: (u32, u32),
    range: (u32, u32),
    out: &mut Vec<DocCommentTagRange>,
) {
    let start = comment.0.max(range.0);
    let end = comment.1.min(range.1);
    if start > end {
        return;
    }
    for number in state.line_at(start).number..=state.line_at(end).number {
        let line = state.line(number);
        let from = line.from.max(comment.0);
        let full = state.slice(from, line.to.min(comment.1));
        let text = full.strip_suffix("*/").unwrap_or(&full);
        let at = |byte: usize| from + col_of_byte(text, byte);
        let mut push = |start: usize, end: usize, kind: DocCommentTagKind| {
            out.push(DocCommentTagRange { from: at(start), to: at(end), kind });
        };
        let skip = if from == comment.0 && opens_doc(text) { 3 } else { 0 };
        let rest = &text[skip..];
        let lead = skip + LEAD.find(rest).map_or(0, |found| found.end());
        if let Some(tag) = BLOCK_TAG.find(&text[lead..]) {
            let after = lead + tag.end();
            push(lead, after, DocCommentTagKind::Tag);
            let value = dialect.value.and_then(|rule| value_of(rule, tag.as_str(), &text[after..]));
            if let Some((value_from, value_to)) = value {
                push(after + value_from, after + value_to, DocCommentTagKind::Value);
            }
        }
        if dialect.inline {
            for found in INLINE_TAG.find_iter(rest) {
                push(skip + found.start() + 1, skip + found.end(), DocCommentTagKind::Tag);
            }
        }
        if dialect.links {
            for found in LINK.find_iter(rest).filter(|found| !rest[found.end()..].starts_with(['(', '['])) {
                push(skip + found.start(), skip + found.end(), DocCommentTagKind::Value);
            }
        }
        if dialect.markup {
            for found in MARKUP.find_iter(rest) {
                push(skip + found.start(), skip + found.end(), DocCommentTagKind::Markup);
            }
        }
    }
}

/// `ranges` sorted by start, without the ones overlapping one kept before them.
fn without_overlaps(mut ranges: Vec<DocCommentTagRange>) -> Vec<DocCommentTagRange> {
    ranges.sort_by_key(|range| range.from);
    let mut out: Vec<DocCommentTagRange> = Vec::with_capacity(ranges.len());
    for range in ranges {
        if out.last().is_none_or(|last| range.from >= last.to) {
            out.push(range);
        }
    }
    out
}

/// The tag, value and markup ranges in the doc comments between `from` and `to`, sorted, without
/// overlaps. A comment inside another one belongs to the outer one and is not read on its own.
pub fn doc_comment_tag_ranges(
    state: &EditState,
    tree: &SyntaxTree,
    language: &str,
    from: u32,
    to: u32,
) -> Vec<DocCommentTagRange> {
    let Some(dialect) = dialect(language) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for index in tree.touching(from, to) {
        let name = tree.name(index);
        if !is_comment(name) || tree.ancestors(index).skip(1).any(|outer| is_comment(tree.name(outer))) {
            continue;
        }
        let node = tree.node(index);
        let doc = if language == "kotlin" {
            name == "docComment" || pascal_name(name) == "DocComment"
        } else {
            opens_doc(&state.slice(node.from, node.from + 4))
        };
        if doc {
            scan_comment(state, &dialect, (node.from, node.to), (from, to), &mut found);
        }
    }
    without_overlaps(found)
}

/// The tag ranges of the visible ranges of the editor. A fold inside one line splits it into two
/// visible ranges that both scan that whole line; its tags count once.
pub fn visible_doc_comment_tags(
    state: &EditState,
    tree: &SyntaxTree,
    language: &str,
    visible: &[(u32, u32)],
) -> Vec<DocCommentTagRange> {
    let found =
        visible.iter().flat_map(|(from, to)| doc_comment_tag_ranges(state, tree, language, *from, *to)).collect();
    without_overlaps(found)
}

#[cfg(test)]
mod tests {
    use super::super::tree::TreeNode;
    use super::super::utf16_len;
    use super::DocCommentTagKind::{Markup, Tag, Value};
    use super::*;

    /// A tree of `doc`: a top node and a node for each `(name, snippet)`, each snippet searched
    /// from where the previous one starts.
    fn tree_of(doc: &str, nodes: &[(&str, &str)]) -> SyntaxTree {
        let mut found = vec![TreeNode::new("Program", 0, utf16_len(doc))];
        let mut after = 0;
        for (name, snippet) in nodes {
            let at = doc[after..].find(snippet).map(|at| at + after).expect("the snippet is in the document");
            let from = utf16_len(&doc[..at]);
            found.push(TreeNode::new(name, from, from + utf16_len(snippet)));
            after = at;
        }
        found.sort_by(|a, b| a.from.cmp(&b.from).then(b.to.cmp(&a.to)));
        SyntaxTree::new(found)
    }

    fn ranges(state: &EditState, tree: &SyntaxTree, language: &str, from: u32) -> Vec<(String, DocCommentTagKind)> {
        doc_comment_tag_ranges(state, tree, language, from, state.doc_len())
            .into_iter()
            .map(|range| (state.slice(range.from, range.to), range.kind))
            .collect()
    }

    fn expected(items: &[(&str, DocCommentTagKind)]) -> Vec<(String, DocCommentTagKind)> {
        items.iter().map(|(text, kind)| ((*text).to_owned(), *kind)).collect()
    }

    #[test]
    fn marks_javadoc_block_and_inline_tags_param_values_and_html_markup() {
        let doc_comment = [
            "/**",
            " * Sums {@link Math#addExact} values, mail dev@example.com.",
            " * <p>Second @param paragraph, List<String> stays text.</p>",
            " * @param <T> the type",
            " * @param left the left",
            " * @return the sum",
            " * @throws ArithmeticException on overflow",
            " */",
        ]
        .join("\n");
        let doc = [
            doc_comment.as_str(),
            "/* @param plain */",
            "// @param line",
            "class A { String s = \"/** @param x */\"; /**/ }",
            "/** @deprecated use {@link B} */",
        ]
        .join("\n");
        let tree = tree_of(
            &doc,
            &[
                ("BlockComment", doc_comment.as_str()),
                ("BlockComment", "/* @param plain */"),
                ("LineComment", "// @param line"),
                ("String", "\"/** @param x */\""),
                ("BlockComment", "/**/"),
                ("BlockComment", "/** @deprecated use {@link B} */"),
            ],
        );
        let state = EditState::new(&doc);
        assert_eq!(
            ranges(&state, &tree, "java", 0),
            expected(&[
                ("@link", Tag),
                ("<p>", Markup),
                ("</p>", Markup),
                ("@param", Tag),
                ("<T>", Value),
                ("@param", Tag),
                ("left", Value),
                ("@return", Tag),
                ("@throws", Tag),
                ("@deprecated", Tag),
                ("@link", Tag),
            ])
        );
    }

    #[test]
    fn marks_kdoc_tags_their_subjects_and_links_also_when_the_comment_starts_above_the_range() {
        let doc_comment = [
            "/**",
            " * Adds [a] to b, see [Math.max] or [the docs](https://example.com).",
            " * @param a the a",
            " * @throws IllegalStateException never",
            " * @return the sum, {@link Foo} is not KDoc",
            " */",
        ]
        .join("\n");
        let doc = [doc_comment.as_str(), "fun f(a: Int) = a // @param x", "/* @param plain */"].join("\n");
        let tree = tree_of(
            &doc,
            &[("docComment", doc_comment.as_str()), ("comment", "// @param x"), ("comment", "/* @param plain */")],
        );
        let state = EditState::new(&doc);
        assert_eq!(
            ranges(&state, &tree, "kotlin", 0),
            expected(&[
                ("[a]", Value),
                ("[Math.max]", Value),
                ("@param", Tag),
                ("a", Value),
                ("@throws", Tag),
                ("IllegalStateException", Value),
                ("@return", Tag),
            ])
        );
        let from = utf16_len(&doc[..doc.find(" * @throws").expect("the throws line")]);
        assert_eq!(
            ranges(&state, &tree, "kotlin", from),
            expected(&[("@throws", Tag), ("IllegalStateException", Value), ("@return", Tag)])
        );
    }

    #[test]
    fn marks_jsdoc_tags_in_javascript_typescript_jsx_and_tsx() {
        let doc_comment = [
            "/**",
            " * Formats {@link Date} values for user@example.com.",
            " * @param {string} name the name",
            " * @returns {string}",
            " */",
        ]
        .join("\n");
        let doc = [doc_comment.as_str(), "/* @param plain */", "const s = '/** @param x */';"].join("\n");
        let tree = tree_of(
            &doc,
            &[
                ("BlockComment", doc_comment.as_str()),
                ("BlockComment", "/* @param plain */"),
                ("String", "'/** @param x */'"),
            ],
        );
        let state = EditState::new(&doc);
        for language in ["javascript", "typescript", "javascriptreact", "typescriptreact"] {
            assert_eq!(
                ranges(&state, &tree, language, 0),
                expected(&[("@link", Tag), ("@param", Tag), ("@returns", Tag)]),
                "{language}"
            );
        }
    }

    #[test]
    fn marks_phpdoc_tags_and_the_variable_of_param_and_var() {
        let doc_comment = [
            "/**",
            " * Finds a user, {@inheritDoc}",
            " * @param int|string $id the id",
            " * @param array<int, string> ...$rest",
            " * @var Foo&Bar $x",
            " * @return ?User",
            " */",
        ]
        .join("\n");
        let doc =
            ["<?php", doc_comment.as_str(), "function find($id) {} # @param $y", "/* @param int $z */"].join("\n");
        let tree = tree_of(
            &doc,
            &[
                ("BlockComment", doc_comment.as_str()),
                ("LineComment", "# @param $y"),
                ("BlockComment", "/* @param int $z */"),
            ],
        );
        let state = EditState::new(&doc);
        assert_eq!(
            ranges(&state, &tree, "php", 0),
            expected(&[
                ("@inheritDoc", Tag),
                ("@param", Tag),
                ("$id", Value),
                ("@param", Tag),
                ("$rest", Value),
                ("@var", Tag),
                ("$x", Value),
                ("@return", Tag),
            ])
        );
    }

    #[test]
    fn rejects_non_doc_lookalikes() {
        for language in ["java", "kotlin", "typescript", "php"] {
            let unfinished = "/** dev@example.com\n * @return unfinished";
            let doc = [
                if language == "php" { "<?php" } else { "" },
                "@Override class A {}",
                "// @Override {@link X}",
                "/* @Override {@link X} */",
                "/* plain\n * @param falsePositive\n */",
                "const s = \"/** @param string */\";",
                "/**/",
                unfinished,
            ]
            .join("\n");
            let (line, block, doc_block) = match language {
                "kotlin" => ("comment", "comment", "docComment"),
                _ => ("LineComment", "BlockComment", "BlockComment"),
            };
            let mut nodes = vec![
                (line, "// @Override {@link X}"),
                (block, "/* @Override {@link X} */"),
                (block, "/* plain\n * @param falsePositive\n */"),
                ("String", "\"/** @param string */\""),
                (block, "/**/"),
            ];
            // Java and PHP do not parse an unterminated block comment as a comment.
            let terminated_only = language == "java" || language == "php";
            if !terminated_only {
                nodes.push((doc_block, unfinished));
            }
            let state = EditState::new(&doc);
            let found = ranges(&state, &tree_of(&doc, &nodes), language, 0);
            let wanted = if terminated_only { Vec::new() } else { expected(&[("@return", Tag)]) };
            assert_eq!(found, wanted, "{language}");
        }
    }

    #[test]
    fn keeps_tags_deep_inside_a_long_doc_comment() {
        let body = std::iter::repeat_n(" * @param item description", 19997);
        let lines: Vec<&str> = std::iter::once("/**").chain(body).chain([" */", ""]).collect();
        let doc = lines.join("\n");
        let state = EditState::new(&doc);
        for (language, name) in [("java", "BlockComment"), ("kotlin", "docComment")] {
            let tree = tree_of(&doc, &[(name, doc.trim_end())]);
            let (from, to) = (state.line(18999).from, state.line(19000).to);
            let tags = doc_comment_tag_ranges(&state, &tree, language, from, to);
            assert_eq!(tags.iter().filter(|range| range.kind == Tag).count(), 2, "{language}");
        }
    }

    #[test]
    fn keeps_kotlin_nested_comments_in_their_outer_comments_dialect() {
        let doc = concat!(
            "/* plain\n/** @param hidden */\n * @return hidden\n */\n",
            "/** docs\n/* nested */\n * @return shown\n */"
        );
        let tree = tree_of(
            doc,
            &[
                ("comment", "/* plain\n/** @param hidden */\n * @return hidden\n */"),
                ("docComment", "/** @param hidden */"),
                ("docComment", "/** docs\n/* nested */\n * @return shown\n */"),
                ("comment", "/* nested */"),
            ],
        );
        assert_eq!(ranges(&EditState::new(doc), &tree, "kotlin", 0), expected(&[("@return", Tag)]));
    }

    #[test]
    fn gives_other_languages_nothing() {
        let doc = "/** @param x */\nfn f() {}";
        let tree = tree_of(doc, &[("block_comment", "/** @param x */")]);
        let state = EditState::new(doc);
        for language in ["rust", "PluginLanguage", "constructor", "toString"] {
            assert!(ranges(&state, &tree, language, 0).is_empty(), "{language}");
            assert!(!has_doc_comment_tags(language), "{language}");
        }
    }

    #[test]
    fn keeps_the_tags_of_a_line_that_a_fold_splits_into_two_visible_ranges() {
        let doc = "/** See {@link A} and some text long enough to fold away, then {@link B}. */\nclass A {}";
        let comment = "/** See {@link A} and some text long enough to fold away, then {@link B}. */";
        let tree = tree_of(doc, &[("BlockComment", comment)]);
        let state = EditState::new(doc);
        let fold = (doc.find("some").expect("some") as u32, doc.find(", then").expect("then") as u32);
        let visible = [(0, fold.0), (fold.1, state.doc_len())];
        let tags: Vec<String> = visible_doc_comment_tags(&state, &tree, "java", &visible)
            .into_iter()
            .filter(|range| range.kind == Tag)
            .map(|range| state.slice(range.from, range.to))
            .collect();
        assert_eq!(tags, ["@link", "@link"]);
    }
}
