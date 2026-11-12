//! Editor colour schemes (Settings › Editor › Color Scheme). A scheme is the editor part of a
//! theme's tokens (`--syn-*`, editor surfaces, gutter, search, diagnostics), read from the token
//! stylesheet so the scheme list never drifts from the themes. Choosing a scheme other than the
//! theme's own, or editing an attribute, yields token overrides: for the root, and per language
//! for the editors of that language.
//!
//! Attribute ids are stable (they key `editor.colorOverrides[scheme][id]`). Token names are
//! written with their leading `--`, as in the stylesheet.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::themes::{
    BUILTIN_THEME_IDS, BUILTIN_THEMES, BuiltinThemeId, COLOR_THEME_ATTRIBUTE, CONTRAST_ATTRIBUTE,
    PLUGIN_THEME_ATTRIBUTE, THEME_ATTRIBUTE, ThemeKind,
};
use super::tokens::ThemeAttributes;

/// A built-in scheme is the editor part of the built-in theme with the same id.
pub type SchemeId = BuiltinThemeId;

/// The `editor.colorScheme` value that follows the IDE theme.
pub const THEME_DEFAULT_SCHEME: &str = "theme-default";

/// Editor › Color Scheme override of one text attribute, as stored in
/// `editor.colorOverrides[scheme][attribute]` and in a custom scheme's `attributes`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TextAttributesOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreground: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    /// `underline` | `wave` | `strikeout` | `none`; anything else draws no effect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect_color: Option<String>,
}

/// An imported editor colour scheme on top of a built-in base, as stored in
/// `editor.customSchemes[id]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomColorScheme {
    pub name: String,
    /// `light` | `dark`: the built-in scheme its attributes apply over.
    pub parent: String,
    pub attributes: AttributeOverrides,
}

/// Attribute id → override.
pub type AttributeOverrides = BTreeMap<String, TextAttributesOverride>;
/// `editor.customSchemes`: scheme id (`custom-…`) → scheme, in id order as the settings store
/// keeps them.
pub type CustomColorSchemes = BTreeMap<String, CustomColorScheme>;
/// `editor.colorOverrides`: scheme id → attribute id → override.
pub type ColorOverrides = BTreeMap<String, AttributeOverrides>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemeEntry {
    pub id: String,
    pub name: String,
}

/// The built-in schemes.
pub fn schemes() -> Vec<SchemeEntry> {
    BUILTIN_THEMES.iter().map(|t| SchemeEntry { id: t.id.as_str().to_owned(), name: t.label.to_owned() }).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeDef {
    pub id: String,
    pub name: String,
    /// Tree path below the root, e.g. `["Language Defaults"]`.
    pub path: Vec<String>,
    /// Token of the foreground / background / effect colour, if editable.
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub effect: Option<String>,
    /// Syntax token: bold / italic / background / effect come from `--syn-<token>-*`.
    pub token: Option<String>,
    /// A semantic modifier whose only configurable attribute is italic.
    pub italic_only: bool,
    pub default_bold: Option<bool>,
    pub default_italic: Option<bool>,
    pub default_effect: Option<&'static str>,
    /// Editor language name; unset attributes are language defaults.
    pub language: Option<&'static str>,
}

impl AttributeDef {
    fn new(id: &str, name: &str, path: &[&str]) -> Self {
        Self {
            id: id.to_owned(),
            name: name.to_owned(),
            path: path.iter().map(|p| (*p).to_owned()).collect(),
            fg: None,
            bg: None,
            effect: None,
            token: None,
            italic_only: false,
            default_bold: None,
            default_italic: None,
            default_effect: None,
            language: None,
        }
    }

    /// A syntax token coloured by `--syn-<token>`.
    fn syn(id: &str, name: &str, path: &[&str], token: &str) -> Self {
        Self { token: Some(token.to_owned()), ..Self::new(id, name, path) }.with_fg(&format!("--syn-{token}"))
    }

    /// A modifier of syntax token `token` that only turns italic on or off.
    fn italic_only(id: &str, name: &str, path: &[&str], token: &str) -> Self {
        Self { token: Some(token.to_owned()), italic_only: true, ..Self::new(id, name, path) }
    }

    fn with_fg(self, token: &str) -> Self {
        Self { fg: Some(token.to_owned()), ..self }
    }

    fn with_bg(self, token: &str) -> Self {
        Self { bg: Some(token.to_owned()), ..self }
    }

    fn with_effect(self, token: &str) -> Self {
        Self { effect: Some(token.to_owned()), ..self }
    }

    fn with_default_italic(self, italic: bool) -> Self {
        Self { default_italic: Some(italic), ..self }
    }
}

const LD: &str = "Language Defaults";

/// IDEA-style attribute tree: General, Language Defaults, then languages.
fn base_attributes() -> Vec<AttributeDef> {
    const TEXT: [&str; 2] = ["General", "Text"];
    const EDITOR: [&str; 2] = ["General", "Editor"];
    const CODE: [&str; 2] = ["General", "Code"];
    const SEARCH: [&str; 2] = ["General", "Search Results"];
    const ERRORS: [&str; 2] = ["General", "Errors and Warnings"];
    const DEBUGGER: [&str; 1] = ["Debugger"];
    const DEFAULTS: [&str; 1] = [LD];
    const PYTHON: [&str; 1] = ["Python"];
    const MARKDOWN: [&str; 1] = ["Markdown"];
    let plain = AttributeDef::new;
    let syn = AttributeDef::syn;
    let mut out = vec![
        plain("text", "Default text", &TEXT).with_fg("--fg-editor").with_bg("--bg-editor"),
        plain("caret", "Caret", &EDITOR).with_fg("--caret"),
        plain("caretRow", "Caret row", &EDITOR).with_bg("--bg-current-line"),
        plain("selection", "Selection background", &EDITOR).with_bg("--bg-editor-selection"),
        plain("selectionInactive", "Selection background (inactive)", &EDITOR)
            .with_bg("--bg-editor-selection-inactive"),
        plain("whitespaces", "Whitespaces", &EDITOR).with_fg("--fg-whitespace"),
        plain("indentGuide", "Indent guide", &EDITOR).with_fg("--border-indent-guide"),
        plain("rightMargin", "Hard wrap guide", &EDITOR).with_fg("--border-right-margin"),
        plain("methodSeparator", "Method separator color", &EDITOR).with_fg("--border-method-separator"),
        plain("lineNumber", "Line number", &CODE).with_fg("--fg-line-number"),
        plain("lineNumberCaret", "Line number on caret row", &CODE).with_fg("--fg-line-number-current"),
        plain("gutter", "Gutter background", &CODE).with_bg("--bg-editor-gutter"),
        plain("matchedBrace", "Matched brace", &CODE).with_bg("--bg-matched-brace"),
        plain("identifierUnderCaret", "Identifier under caret", &CODE).with_bg("--bg-identifier-under-caret"),
        plain("identifierUnderCaretWrite", "Identifier under caret (write)", &CODE)
            .with_bg("--bg-identifier-under-caret-write"),
        plain("inlay", "Inlay hints", &CODE).with_fg("--fg-inlay").with_bg("--bg-inlay"),
        plain("folded", "Folded text", &CODE).with_fg("--fg-fold-placeholder").with_bg("--bg-fold-placeholder"),
        plain("searchResult", "Search result", &SEARCH).with_bg("--bg-search-match"),
        plain("searchResultCurrent", "Current search result", &SEARCH).with_bg("--bg-search-match-current"),
        plain("error", "Error", &ERRORS).with_effect("--wave-error"),
        plain("warning", "Warning", &ERRORS).with_effect("--wave-warning"),
        plain("weakWarning", "Weak warning", &ERRORS).with_fg("--fg-weak-warning"),
        plain("unused", "Unused symbol", &ERRORS).with_fg("--fg-unused"),
        syn("invalid", "Bad character", &ERRORS, "invalid"),
        syn("todo", "TODO defaults", &CODE, "todo"),
        plain("inlineValue", "Inline debugger values", &DEBUGGER).with_fg("--syn-inline-value"),
        plain("breakpointLine", "Breakpoint line", &DEBUGGER).with_bg("--bg-breakpoint-line"),
        syn("keyword", "Keyword", &DEFAULTS, "keyword"),
        syn("identifier", "Identifiers › Local variable", &DEFAULTS, "variable"),
        syn("parameter", "Identifiers › Parameter", &DEFAULTS, "param"),
        syn("globalVariable", "Identifiers › Global variable", &DEFAULTS, "global"),
        syn("packageReference", "Identifiers › Package reference", &DEFAULTS, "namespace"),
        syn("constant", "Identifiers › Constant", &DEFAULTS, "constant").with_default_italic(true),
        syn("field", "Identifiers › Instance field", &DEFAULTS, "property"),
        AttributeDef::italic_only("staticSymbol", "Identifiers › Static symbol", &DEFAULTS, "static"),
        syn("functionDeclaration", "Identifiers › Function declaration", &DEFAULTS, "function"),
        syn("functionCall", "Identifiers › Function call", &DEFAULTS, "function-call"),
        syn("predefined", "Identifiers › Predefined symbol", &DEFAULTS, "builtin"),
        syn("className", "Classes › Class name", &DEFAULTS, "type"),
        syn("number", "Number", &DEFAULTS, "number"),
        syn("string", "String › String text", &DEFAULTS, "string"),
        syn("escape", "String › Valid escape sequence", &DEFAULTS, "escape"),
        syn("lineComment", "Comments › Line comment", &DEFAULTS, "comment"),
        syn("docComment", "Comments › Doc comment › Text", &DEFAULTS, "doc-comment").with_default_italic(true),
        syn("docCommentTag", "Comments › Doc comment › Tag", &DEFAULTS, "doc-comment-tag"),
        syn("docCommentTagValue", "Comments › Doc comment › Tag value", &DEFAULTS, "doc-comment-tag-value"),
        syn("docCommentMarkup", "Comments › Doc comment › Markup", &DEFAULTS, "doc-markup")
            .with_bg("--syn-doc-markup-bg"),
        syn("metadata", "Metadata", &DEFAULTS, "decorator"),
        syn("operator", "Braces and Operators", &DEFAULTS, "punct"),
        syn("markupTag", "Markup › Tag", &DEFAULTS, "tag"),
        syn("markupAttribute", "Markup › Attribute", &DEFAULTS, "attribute"),
    ];
    out.extend((1..=5u8).map(|n| {
        syn(&format!("semantic{n}"), &format!("Semantic highlighting › Color {n}"), &DEFAULTS, &format!("rainbow-{n}"))
    }));
    out.extend((1..=3u8).map(|n| {
        syn(&format!("bracket{n}"), &format!("Rainbow brackets › Level {n}"), &DEFAULTS, &format!("bracket-{n}"))
    }));
    out.extend([
        syn("pyMethodCall", "Method call", &PYTHON, "python-method-call"),
        syn("pyAnnotation", "Type annotation", &PYTHON, "python-annotation"),
        syn("pyParentheses", "Parentheses", &PYTHON, "python-parentheses"),
        syn("pyBrackets", "Brackets", &PYTHON, "python-brackets"),
        syn("pyBraces", "Braces", &PYTHON, "python-braces"),
        syn("pyNestedFunction", "Nested function definition", &PYTHON, "python-nested-function"),
        syn("pyBytes", "Byte string", &PYTHON, "python-bytes"),
        syn("pyFString", "F-string", &PYTHON, "python-fstring"),
        syn("pyPredefinedDefinition", "Predefined function definition", &PYTHON, "python-predefined-definition"),
        syn("pyPredefinedUsage", "Predefined name usage", &PYTHON, "python-predefined-usage"),
        syn("pyFormat", "Format specifier", &PYTHON, "python-format"),
        syn("pySelf", "Self parameter", &PYTHON, "self"),
        AttributeDef::italic_only("pyStaticSymbol", "Static symbol", &PYTHON, "python-static")
            .with_default_italic(false),
        syn("pyKeywordArg", "Keyword argument", &PYTHON, "keyword-arg"),
        syn("pyDocstring", "Docstring", &PYTHON, "docstring").with_default_italic(true),
        syn("pyBuiltin", "Built-in name", &PYTHON, "python-builtin"),
        syn("pyDecorator", "Decorator", &PYTHON, "annotation"),
        syn("pyLocalVariable", "Local variable", &PYTHON, "python-local-variable"),
        syn("pyGlobalVariable", "Global variable", &PYTHON, "python-global-variable"),
        syn("pyInstanceAttribute", "Instance attribute", &PYTHON, "python-instance-attribute"),
        syn("pyClassAttribute", "Class attribute", &PYTHON, "python-class-attribute"),
        syn("pyConstant", "Constant", &PYTHON, "python-constant"),
        syn("pyProperty", "Property", &PYTHON, "python-property"),
        syn("pyTypeParameter", "Type parameter", &PYTHON, "python-type-parameter"),
        AttributeDef { default_bold: Some(true), ..syn("mdHeading", "Heading", &MARKDOWN, "markup-heading") },
        syn("mdCode", "Code span", &MARKDOWN, "markup-code"),
        AttributeDef { default_effect: Some("underline"), ..syn("mdLink", "Link", &MARKDOWN, "link") },
        syn("regexp", "Regular expression", &["RegExp"], "regexp"),
    ]);
    out
}

/// Languages with a scheme page of their own: settings id prefix and editor language name.
const SCHEME_LANGUAGES: [(&str, &str); 4] = [("go", "Go"), ("java", "Java"), ("kotlin", "Kotlin"), ("php", "PHP")];

/// `semantic1`…, `bracket1`…: the numbered colours have no per-language variant.
fn is_numbered(id: &str) -> bool {
    ["semantic", "bracket"]
        .iter()
        .any(|prefix| id.strip_prefix(*prefix).is_some_and(|n| n.len() == 1 && n.bytes().all(|b| b.is_ascii_digit())))
}

/// Every editable attribute, in the order of the settings tree: the base attributes, each
/// language's variants of the language defaults, then PHP's own.
pub fn attributes() -> &'static [AttributeDef] {
    static ATTRIBUTES: OnceLock<Vec<AttributeDef>> = OnceLock::new();
    ATTRIBUTES.get_or_init(|| {
        let base = base_attributes();
        let mut all = base.clone();
        for (id, language) in SCHEME_LANGUAGES {
            let defaults = base.iter().filter(|a| a.path.first().is_some_and(|p| p == LD) && !is_numbered(&a.id));
            all.extend(defaults.map(|a| AttributeDef {
                id: format!("{id}.{}", a.id),
                path: std::iter::once(language.to_owned()).chain(a.path.iter().skip(1).cloned()).collect(),
                language: Some(language),
                ..a.clone()
            }));
        }
        let php = [
            ("brackets", "Brackets", "bracket"),
            ("separator", "Comma and semicolon", "separator"),
            ("scriptTag", "PHP tags", "processing-instruction"),
            ("staticField", "Static field", "static-property"),
        ];
        all.extend(php.map(|(id, name, token)| AttributeDef {
            language: Some("PHP"),
            ..AttributeDef::syn(&format!("php.{id}"), name, &["PHP"], token)
        }));
        all
    })
}

// --- scheme base values ---

/// Editor tokens a scheme sets besides the attribute tokens: the ones derived from them.
const DERIVED_SCHEME_VARS: [&str; 13] = [
    "--syn-comment-style",
    "--syn-doc-comment-tag-effect",
    "--editor-error-decoration",
    "--editor-warning-decoration",
    "--editor-warning-background",
    "--border-gutter",
    "--border-indent-guide-active",
    "--fg-fold-marker",
    "--bg-editor-inline",
    "--fg-completion-match",
    "--fg-search-match",
    "--matched-brace-weight",
    "--bg-matched-tag",
];

/// Tokens a scheme sets: every attribute token plus the editor tokens derived from them.
fn scheme_var_names() -> HashSet<&'static str> {
    attributes()
        .iter()
        .flat_map(|a| [&a.fg, &a.bg, &a.effect])
        .flatten()
        .map(String::as_str)
        .chain(DERIVED_SCHEME_VARS)
        .collect()
}

/// Token values by name, in the order they were first set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Vars {
    entries: Vec<(String, String)>,
    index: HashMap<String, usize>,
}

impl Vars {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.index.get(name).map(|at| self.entries[*at].1.as_str())
    }

    pub fn has(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    /// Sets `name`; a name set before keeps its place.
    pub fn set(&mut self, name: &str, value: &str) {
        match self.index.get(name).copied() {
            Some(at) => self.entries[at].1 = value.to_owned(),
            None => {
                self.index.insert(name.to_owned(), self.entries.len());
                self.entries.push((name.to_owned(), value.to_owned()));
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(name, value)| (name.as_str(), value.as_str()))
    }
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The length of the custom property name (`--` and at least one more name character) that
/// `text` starts with.
fn property_name_len(text: &str) -> Option<usize> {
    let len = text.find(|c: char| !is_word(c) && c != '-').unwrap_or(text.len());
    (text.starts_with("--") && len > 2).then_some(len)
}

struct Block {
    selectors: Vec<String>,
    decls: Vars,
}

/// `--name: value;` at the start of `text`: the name, the value and the length of it all.
fn declaration(text: &str) -> Option<(&str, &str, usize)> {
    let (name, rest) = text.split_at(property_name_len(text)?);
    let value = rest.trim_start().strip_prefix(':')?;
    let end = value.find(';').filter(|end| *end > 0)?;
    Some((name, value[..end].trim(), text.len() - value.len() + end + 1))
}

/// The custom properties a block declares. A declaration needs its closing `;`.
fn declarations(body: &str) -> Vars {
    let mut decls = Vars::default();
    let mut at = 0;
    while let Some(found) = body[at..].find("--") {
        at += found;
        match declaration(&body[at..]) {
            Some((name, value, len)) => {
                decls.set(name, value);
                at += len;
            }
            None => at += 1,
        }
    }
    decls
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        let Some(end) = rest[start + 2..].find("*/") else { break };
        out.push_str(&rest[..start]);
        rest = &rest[start + 2 + end + 2..];
    }
    out.push_str(rest);
    out
}

/// Top-level `selector { decls }` blocks of a stylesheet (at-rules skipped).
fn blocks(css: &str) -> Vec<Block> {
    let text = strip_comments(css);
    let mut out = Vec::new();
    let (mut depth, mut start, mut selector) = (0i32, 0usize, "");
    for (i, ch) in text.char_indices() {
        match ch {
            '{' => {
                if depth == 0 {
                    selector = text[start..i].trim();
                    start = i + 1;
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if !selector.starts_with('@') {
                        out.push(Block {
                            selectors: selector.split(',').map(|s| s.trim().to_owned()).collect(),
                            decls: declarations(&text[start..i]),
                        });
                    }
                    start = i + 1;
                }
            }
            _ => {}
        }
    }
    out
}

/// The selectors whose blocks make up a theme, besides `:root`.
fn theme_selectors(id: SchemeId) -> Vec<String> {
    match id {
        BuiltinThemeId::Light => vec![r#":root[data-theme="light"]"#.to_owned()],
        BuiltinThemeId::DarkHc => vec![
            r#":root[data-theme="dark-hc"]"#.to_owned(),
            r#":root[data-theme="dark"][data-contrast="high"]"#.to_owned(),
        ],
        BuiltinThemeId::Dark => vec![r#":root[data-theme="dark"]"#.to_owned()],
        palette => vec![
            format!(r#":root[data-theme="{}"]"#, palette.kind().as_str()),
            ":root[data-color-theme]".to_owned(),
            format!(r#":root[data-color-theme="{}"]"#, palette.as_str()),
        ],
    }
}

/// `var(--name)` or `var(--name, fallback)` at the start of `text`: the name, the fallback and
/// the length of the reference. The fallback may hold one level of parentheses.
fn var_reference(text: &str) -> Option<(&str, Option<&str>, usize)> {
    let body = text.strip_prefix("var(")?;
    let (name, after) = body.split_at(property_name_len(body)?);
    if after.starts_with(')') {
        return Some((name, None, text.len() - after.len() + 1));
    }
    let fallback = after.trim_start().strip_prefix(',')?.trim_start();
    let paren = |text: &str| text.find(['(', ')']);
    let mut end = paren(fallback)?;
    if fallback[end..].starts_with('(') {
        end += 1 + paren(&fallback[end + 1..])?;
        if !fallback[end..].starts_with(')') {
            return None;
        }
        end += 1 + paren(&fallback[end + 1..])?;
        if !fallback[end..].starts_with(')') {
            return None;
        }
    }
    Some((name, Some(&fallback[..end]), text.len() - fallback.len() + end + 1))
}

/// Substitutes the `var()` references of `value`. A reference to an unknown token without a
/// fallback stays as written, so a token set at run time still applies.
fn resolve_vars(value: &str, vars: &Vars, depth: u8) -> String {
    if depth > 8 {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("var(") {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some((name, fallback, len)) = var_reference(rest) else {
            out.push('v');
            rest = &rest[1..];
            continue;
        };
        match (vars.get(name), fallback) {
            (Some(value), _) => out.push_str(&resolve_vars(value, vars, depth + 1)),
            (None, Some(fallback)) => out.push_str(&resolve_vars(fallback.trim(), vars, depth + 1)),
            (None, None) => out.push_str(&rest[..len]),
        }
        rest = &rest[len..];
    }
    out.push_str(rest);
    out
}

fn merged_tokens(blocks: &[Block], id: SchemeId) -> Vars {
    let own = theme_selectors(id);
    let mut merged = Vars::default();
    let root = blocks.iter().filter(|b| b.selectors.iter().any(|s| s.as_str() == ":root"));
    let theme = blocks.iter().filter(|b| b.selectors.iter().any(|s| own.contains(s)));
    for (name, value) in root.chain(theme).flat_map(|b| b.decls.iter()) {
        merged.set(name, value);
    }
    let mut out = Vars::default();
    for (name, value) in merged.iter() {
        out.set(name, &resolve_vars(value, &merged, 0));
    }
    out
}

/// All tokens of theme `id` in stylesheet `css`, with `var()` references resolved (so they
/// survive under another theme).
pub fn scheme_tokens(id: SchemeId, css: &str) -> Vars {
    merged_tokens(&blocks(css), id)
}

/// The resolved tokens of every built-in scheme of one token stylesheet.
#[derive(Debug, Clone)]
pub struct SchemeSource {
    /// Indexed by scheme id, in the order of [`BUILTIN_THEME_IDS`].
    tokens: [Vars; 7],
}

impl SchemeSource {
    pub fn new(css: &str) -> Self {
        let blocks = blocks(css);
        Self { tokens: BUILTIN_THEME_IDS.map(|id| merged_tokens(&blocks, id)) }
    }

    /// All tokens of a theme with `var()` references resolved.
    pub fn tokens(&self, id: SchemeId) -> &Vars {
        &self.tokens[id as usize]
    }
}

/// Scheme belonging to the active IDE theme (`data-color-theme` / `data-theme` /
/// `data-contrast` of `root`); without a root, the default theme's.
pub fn theme_scheme(root: Option<&ThemeAttributes>) -> SchemeId {
    let Some(root) = root else { return BuiltinThemeId::Light };
    if let Some(palette) = root.get(COLOR_THEME_ATTRIBUTE).and_then(BuiltinThemeId::parse) {
        return palette;
    }
    match (root.get(THEME_ATTRIBUTE), root.get(CONTRAST_ATTRIBUTE)) {
        (Some("light"), _) => BuiltinThemeId::Light,
        (Some("dark-hc"), _) | (_, Some("high")) => BuiltinThemeId::DarkHc,
        _ => BuiltinThemeId::Dark,
    }
}

/// The scheme the editor shows for `setting` under the current theme.
pub fn effective_scheme(setting: Option<&str>, root: Option<&ThemeAttributes>, customs: &CustomColorSchemes) -> String {
    match setting {
        Some(setting) if BuiltinThemeId::parse(setting).is_some() || customs.contains_key(setting) => {
            setting.to_owned()
        }
        _ => theme_scheme(root).as_str().to_owned(),
    }
}

/// Light or dark: a built-in scheme's theme kind, a custom scheme's parent (`None`: unknown).
pub fn scheme_kind(id: Option<&str>, customs: &CustomColorSchemes) -> Option<ThemeKind> {
    let id = id?;
    match BuiltinThemeId::parse(id) {
        Some(builtin) => Some(builtin.kind()),
        None => validated_schemes(customs).get(id).and_then(|scheme| ThemeKind::parse(&scheme.parent)),
    }
}

/// The scheme setting once the IDE theme has turned `kind`, or `None` to keep it: a chosen
/// scheme of the other kind gives way to the theme's own, so a light theme doesn't keep a dark
/// editor.
pub fn scheme_after_theme_change(
    setting: Option<&str>,
    kind: ThemeKind,
    customs: &CustomColorSchemes,
) -> Option<&'static str> {
    scheme_kind(setting, customs).filter(|current| *current != kind).map(|_| THEME_DEFAULT_SCHEME)
}

/// An explicit Bold / regular attribute: the editor's configured bold or main weight.
pub fn bold_weight(bold: bool) -> &'static str {
    if bold { "var(--fw-code-bold, 700)" } else { "var(--fw-code, 400)" }
}

/// The `text-decoration` of an effect drawn in `color`; `None` for an unknown effect.
fn effect_css(effect: &str, color: &str) -> Option<String> {
    match effect {
        "underline" => Some(format!("underline {color}")),
        "wave" => Some(format!("underline wavy {color}")),
        "strikeout" => Some(format!("line-through {color}")),
        "none" => Some("none".to_owned()),
        _ => None,
    }
}

/// Explicitly removes a scheme colour; unlike an unset field, this must survive as an override.
pub const CLEAR_COLOR: &str = "transparent";

/// `#rgb` or `#rrggbb`.
pub fn is_hex_color(value: &str) -> bool {
    value.strip_prefix('#').is_some_and(|hex| matches!(hex.len(), 3 | 6) && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn set_or_cleared(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|value| is_hex_color(value) || *value == CLEAR_COLOR)
}

/// Declarations for the overrides of one scheme: of the attributes of `language`, or of the
/// attributes shared by all languages.
pub fn override_declarations(overrides: Option<&AttributeOverrides>, language: Option<&str>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(overrides) = overrides else { return out };
    for attr in attributes().iter().filter(|attr| attr.language == language) {
        let Some(o) = overrides.get(&attr.id) else { continue };
        let (foreground, background) = (set_or_cleared(&o.foreground), set_or_cleared(&o.background));
        let effect_color = set_or_cleared(&o.effect_color);
        if let (Some(color), Some(token)) = (foreground, &attr.fg) {
            out.push((token.clone(), color.to_owned()));
        }
        if let (Some(color), Some(token)) = (background, &attr.bg) {
            out.push((token.clone(), color.to_owned()));
        }
        if let (Some(color), Some(token)) = (effect_color, &attr.effect) {
            out.push((token.clone(), color.to_owned()));
            if attr.id == "error" {
                out.push(("--editor-error-decoration".to_owned(), format!("underline wavy {color}")));
            }
            if attr.id == "warning" {
                out.push(("--editor-warning-decoration".to_owned(), format!("underline wavy {color}")));
            }
        }
        let Some(token) = &attr.token else { continue };
        if let Some(bold) = o.bold {
            out.push((format!("--syn-{token}-bold"), bold_weight(bold).to_owned()));
        }
        if let Some(italic) = o.italic {
            let style = if italic { "italic" } else { "normal" };
            out.push((format!("--syn-{token}-italic"), style.to_owned()));
            if token == "comment" {
                out.push(("--syn-comment-style".to_owned(), style.to_owned()));
            }
        }
        if let Some(color) = background {
            out.push((format!("--syn-{token}-bg"), color.to_owned()));
        }
        if let Some(effect) = o.effect.as_deref().filter(|effect| !effect.is_empty()) {
            let css = effect_css(effect, effect_color.unwrap_or("currentColor"));
            out.push((format!("--syn-{token}-effect"), css.unwrap_or_else(|| "none".to_owned())));
        }
    }
    out
}

/// `custom-` and at least one more id character.
fn is_custom_scheme_id(id: &str) -> bool {
    id.strip_prefix("custom-").is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| is_word(c) || c == '-'))
}

/// The schemes of `customs` that are usable, reduced to what the editor understands: ids of the
/// `custom-…` form, a name, a light / dark parent, known attributes, hex colours and known
/// effects. Settings files are user editable, so persisted imports are validated too.
pub fn validated_schemes(customs: &CustomColorSchemes) -> CustomColorSchemes {
    let hex = |value: &Option<String>| value.clone().filter(|value| is_hex_color(value));
    let usable = customs.iter().filter(|(id, scheme)| {
        is_custom_scheme_id(id) && !scheme.name.trim().is_empty() && ThemeKind::parse(&scheme.parent).is_some()
    });
    usable
        .map(|(id, scheme)| {
            let known = attributes().iter().filter_map(|attr| {
                let a = scheme.attributes.get(&attr.id)?;
                let safe = TextAttributesOverride {
                    foreground: hex(&a.foreground),
                    background: hex(&a.background),
                    bold: a.bold,
                    italic: a.italic,
                    effect: a.effect.clone().filter(|effect| effect_css(effect, "").is_some()),
                    effect_color: hex(&a.effect_color),
                };
                Some((attr.id.clone(), safe))
            });
            let (name, parent) = (scheme.name.clone(), scheme.parent.clone());
            (id.clone(), CustomColorScheme { name, parent, attributes: known.collect() })
        })
        .collect()
}

/// An override as written in JSON: fields of another type are dropped.
fn lenient_override(raw: &Map<String, Value>) -> TextAttributesOverride {
    let text = |key: &str| raw.get(key).and_then(Value::as_str).map(str::to_owned);
    let flag = |key: &str| raw.get(key).and_then(Value::as_bool);
    TextAttributesOverride {
        foreground: text("foreground"),
        background: text("background"),
        bold: flag("bold"),
        italic: flag("italic"),
        effect: text("effect"),
        effect_color: text("effectColor"),
    }
}

/// The usable custom schemes of the `editor.customSchemes` setting as stored, whatever its
/// shape: entries that are not schemes are dropped ([`validated_schemes`]).
pub fn custom_schemes(raw: &Value) -> CustomColorSchemes {
    let Value::Object(raw) = raw else { return CustomColorSchemes::new() };
    let schemes = raw.iter().filter_map(|(id, value)| {
        let scheme = value.as_object()?;
        let text = |key: &str| scheme.get(key).and_then(Value::as_str).map(str::to_owned);
        let (name, parent) = (text("name")?, text("parent")?);
        let attributes = scheme.get("attributes").and_then(Value::as_object).into_iter().flatten();
        let attributes = attributes.filter_map(|(id, a)| Some((id.clone(), lenient_override(a.as_object()?))));
        Some((id.clone(), CustomColorScheme { name, parent, attributes: attributes.collect() }))
    });
    let schemes: CustomColorSchemes = schemes.collect();
    validated_schemes(&schemes)
}

/// The schemes a picker lists: the built-in ones, then the custom ones.
pub fn list_schemes(customs: &CustomColorSchemes) -> Vec<SchemeEntry> {
    let custom = validated_schemes(customs).into_iter().map(|(id, scheme)| SchemeEntry { id, name: scheme.name });
    schemes().into_iter().chain(custom).collect()
}

/// The tokens of scheme `id`: a built-in scheme's, or a custom scheme's attributes over its
/// parent's.
pub fn resolved_scheme_tokens(source: &SchemeSource, id: &str, customs: &CustomColorSchemes) -> Vars {
    let valid = validated_schemes(customs);
    let custom = valid.get(id);
    let parent = custom.and_then(|scheme| ThemeKind::parse(&scheme.parent));
    let base = BuiltinThemeId::parse(id).unwrap_or(match parent {
        Some(ThemeKind::Dark) => BuiltinThemeId::Dark,
        _ => BuiltinThemeId::Light,
    });
    let mut tokens = source.tokens(base).clone();
    for (name, value) in override_declarations(custom.map(|scheme| &scheme.attributes), None) {
        tokens.set(&name, &value);
    }
    tokens
}

/// The first `#rgb` / `#rrggbb` colour in a CSS value, lowercased.
fn hex_in(value: &str) -> Option<String> {
    value.match_indices('#').find_map(|(at, _)| {
        let rest = &value[at + 1..];
        [3usize, 6].into_iter().find_map(|len| {
            let hex = rest.get(..len).filter(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()))?;
            (!rest[len..].starts_with(is_word)).then(|| format!("#{}", hex.to_ascii_lowercase()))
        })
    })
}

fn effect_from_css(value: Option<&str>) -> Option<&'static str> {
    let value = value?.to_ascii_lowercase();
    [("line-through", "strikeout"), ("wavy", "wave"), ("underline", "underline"), ("none", "none")]
        .into_iter()
        .find_map(|(keyword, effect)| value.contains(keyword).then_some(effect))
}

/// Whether a CSS value names a font weight in `hundreds` (`600`…`900` for bold).
fn has_weight(value: &str, hundreds: std::ops::RangeInclusive<u8>) -> bool {
    value.as_bytes().windows(3).any(|w| hundreds.contains(&w[0]) && w[1] == b'0' && w[2] == b'0')
}

fn bold_from_css(value: Option<&str>) -> Option<bool> {
    let value = value?.to_ascii_lowercase();
    if value.contains("bold") || has_weight(&value, b'6'..=b'9') {
        Some(true)
    } else if value.contains("normal") || has_weight(&value, b'1'..=b'5') {
        Some(false)
    } else {
        None
    }
}

fn italic_from_css(value: Option<&str>) -> Option<bool> {
    let value = value?.to_ascii_lowercase();
    if value.contains("italic") || value.contains("oblique") {
        Some(true)
    } else if value.contains("normal") {
        Some(false)
    } else {
        None
    }
}

/// The attributes an editor shows for a scheme before user overrides: imported values where the
/// scheme sets them, otherwise the theme token and the editor's built-in defaults.
pub fn resolved_attribute(
    source: &SchemeSource,
    id: &str,
    attr_id: &str,
    customs: &CustomColorSchemes,
) -> TextAttributesOverride {
    let mut out = TextAttributesOverride::default();
    let Some(attr) = attributes().iter().find(|attr| attr.id == attr_id) else { return out };
    let valid = validated_schemes(customs);
    let custom = valid.get(id);
    let own = custom.and_then(|scheme| scheme.attributes.get(&attr.id));
    let mut vars = resolved_scheme_tokens(source, id, &valid);
    if let Some(language) = attr.language {
        for (name, value) in override_declarations(custom.map(|scheme| &scheme.attributes), Some(language)) {
            vars.set(&name, &value);
        }
    }

    let token = attr.token.as_deref();
    let var = |name: &Option<String>| name.as_deref().and_then(|name| vars.get(name)).map(str::to_owned);
    let syn = |suffix: &str| token.and_then(|token| vars.get(&format!("--syn-{token}-{suffix}"))).map(str::to_owned);
    let hex = |value: Option<String>| value.filter(|value| is_hex_color(value)).map(|value| value.to_lowercase());
    let background = if attr.bg.is_some() { var(&attr.bg) } else { syn("bg") };
    let effect_css = if attr.effect.is_some() { var(&attr.effect) } else { syn("effect") };
    let effect = own
        .and_then(|own| own.effect.clone())
        .or_else(|| effect_from_css(effect_css.as_deref()).map(str::to_owned))
        .or_else(|| attr.default_effect.map(str::to_owned));

    out.foreground = hex(var(&attr.fg));
    out.background = hex(background);
    out.effect_color = hex(own.and_then(|own| own.effect_color.clone())).or_else(|| hex_in(effect_css.as_deref()?));
    if let Some(token) = token {
        if !attr.italic_only {
            let bold = own.and_then(|own| own.bold).or_else(|| bold_from_css(syn("bold").as_deref()));
            out.bold = Some(bold.or(attr.default_bold).unwrap_or(false));
        }
        let comment_style = || vars.get("--syn-comment-style").filter(|_| token == "comment").map(str::to_owned);
        let italic_css = syn("italic").or_else(comment_style);
        let italic = own.and_then(|own| own.italic).or_else(|| italic_from_css(italic_css.as_deref()));
        out.italic = Some(italic.or(attr.default_italic).unwrap_or(false));
    }
    out.effect = effect.filter(|effect| !effect.is_empty());
    out
}

/// The token overrides of a scheme choice: what the editor draws with instead of the theme's
/// own editor tokens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemeStyle {
    /// Tokens to set on the root, in order (a later entry for the same token wins).
    pub root: Vec<(String, String)>,
    /// Tokens to set for the editors of one language (its editor language name), over the root's.
    pub languages: Vec<(&'static str, Vec<(String, String)>)>,
}

fn css_rule(selector: &str, declarations: &[(String, String)]) -> String {
    let body: Vec<String> = declarations.iter().map(|(name, value)| format!("  {name}: {value} !important;")).collect();
    format!("{selector} {{\n{}\n}}\n", body.join("\n"))
}

impl SchemeStyle {
    /// The overrides as stylesheet text; empty when there are none.
    pub fn to_css(&self) -> String {
        let root = if self.root.is_empty() { String::new() } else { css_rule(":root", &self.root) };
        self.languages.iter().fold(root, |css, (language, declarations)| {
            css + &css_rule(&format!(r#".cm-editor[data-language="{language}"]"#), declarations)
        })
    }
}

/// Only what can be written into a declaration without ending it.
fn safe_declarations(declarations: Vec<(String, String)>) -> Vec<(String, String)> {
    let safe = |(name, value): &(String, String)| {
        property_name_len(name) == Some(name.len()) && !value.contains([';', '{', '}', '<', '>'])
    };
    declarations.into_iter().filter(safe).collect()
}

/// The overrides for the chosen scheme and its edits; empty when the theme's own scheme is used
/// unchanged. `root` carries the attributes of the theme in effect (`None`: no theme applied).
pub fn scheme_style(
    source: &SchemeSource,
    setting: Option<&str>,
    overrides: Option<&ColorOverrides>,
    root: Option<&ThemeAttributes>,
    customs: &CustomColorSchemes,
) -> SchemeStyle {
    let valid = validated_schemes(customs);
    let id = effective_scheme(setting, root, &valid);
    let edits = overrides.and_then(|overrides| overrides.get(&id));
    let mut decls = Vec::new();
    // A plugin theme brings its own editor colours: an explicit scheme replaces them even on the same base.
    let plugin_theme = root.and_then(|root| root.get(PLUGIN_THEME_ATTRIBUTE)).is_some_and(|theme| !theme.is_empty());
    if id != theme_scheme(root).as_str() || (plugin_theme && setting == Some(id.as_str())) {
        let tokens = resolved_scheme_tokens(source, &id, &valid);
        let scheme_vars = scheme_var_names();
        let editor = tokens.iter().filter(|(name, _)| scheme_vars.contains(*name) || name.starts_with("--syn-"));
        decls.extend(editor.map(|(name, value)| (name.to_owned(), value.to_owned())));
        // The editor surface follows the scheme, not the island colour of the UI theme.
        let surface = tokens.get("--bg-editor").or_else(|| tokens.get("--bg-island")).filter(|bg| !bg.is_empty());
        if let Some(bg) = surface {
            decls.push(("--bg-editor".to_owned(), bg.to_owned()));
            decls.push(("--bg-editor-gutter".to_owned(), tokens.get("--bg-editor-gutter").unwrap_or(bg).to_owned()));
        }
    }
    decls.extend(override_declarations(edits, None));
    let own = valid.get(&id).map(|scheme| &scheme.attributes);
    let languages = SCHEME_LANGUAGES.into_iter().filter_map(|(_, language)| {
        let mut decls = override_declarations(own, Some(language));
        decls.extend(override_declarations(edits, Some(language)));
        let decls = safe_declarations(decls);
        (!decls.is_empty()).then_some((language, decls))
    });
    SchemeStyle { languages: languages.collect(), root: safe_declarations(decls) }
}

/// Stylesheet text for the chosen scheme and its overrides ("" when the theme's own is used
/// unchanged).
pub fn scheme_css(
    source: &SchemeSource,
    setting: Option<&str>,
    overrides: Option<&ColorOverrides>,
    root: Option<&ThemeAttributes>,
    customs: &CustomColorSchemes,
) -> String {
    scheme_style(source, setting, overrides, root, customs).to_css()
}

#[cfg(test)]
mod tests {
    use super::super::tokens::TOKENS_CSS;
    use super::*;

    fn foreground(color: &str) -> TextAttributesOverride {
        TextAttributesOverride { foreground: Some(color.to_owned()), ..Default::default() }
    }

    fn edit(scheme: &str, attribute: &str, value: TextAttributesOverride) -> ColorOverrides {
        BTreeMap::from([(scheme.to_owned(), BTreeMap::from([(attribute.to_owned(), value)]))])
    }

    fn custom(id: &str, name: &str, parent: &str, attributes: AttributeOverrides) -> CustomColorSchemes {
        let scheme = CustomColorScheme { name: name.to_owned(), parent: parent.to_owned(), attributes };
        BTreeMap::from([(id.to_owned(), scheme)])
    }

    fn dark_root() -> ThemeAttributes {
        ThemeAttributes::default().with(THEME_ATTRIBUTE, "dark")
    }

    // src/platform/colorScheme.test.ts › color schemes › reads resolved scheme tokens from tokens.css
    #[test]
    fn reads_resolved_scheme_tokens_from_the_stylesheet() {
        let source = SchemeSource::new(TOKENS_CSS);
        let (light, dark) = (source.tokens(BuiltinThemeId::Light), source.tokens(BuiltinThemeId::Dark));
        assert_eq!(light.get("--syn-keyword"), Some("#0033b3"));
        assert_eq!(dark.get("--syn-keyword"), Some("#cf8e6d"));
        assert!(light.get("--bg-editor").is_some_and(|value| !value.contains("var(")));
        for (name, value) in [
            ("--syn-python-builtin", "#8888c6"),
            ("--syn-python-predefined-definition", "#b200b2"),
            ("--syn-python-predefined-usage", "#b200b2"),
            ("--syn-python-instance-attribute", "#bcbec4"),
            ("--syn-python-fstring", "#a5c261"),
            ("--syn-python-bytes", "#a5c261"),
        ] {
            assert_eq!(dark.get(name), Some(value), "{name}");
        }
    }

    // src/platform/colorScheme.test.ts › color schemes › inherits new doc tokens for older %s custom schemes
    #[test]
    fn inherits_new_doc_tokens_for_older_custom_schemes() {
        let source = SchemeSource::new(TOKENS_CSS);
        for parent in ["light", "dark"] {
            let customs = custom("custom-old", "Old", parent, BTreeMap::new());
            for id in ["docCommentTag", "docCommentTagValue", "docCommentMarkup"] {
                assert_eq!(
                    resolved_attribute(&source, "custom-old", id, &customs),
                    resolved_attribute(&source, parent, id, &BTreeMap::new()),
                    "{parent} {id}"
                );
            }
        }
    }

    // src/platform/colorScheme.test.ts › color schemes › gives PHP variables and function calls their own colours,
    // as PhpStorm (#1532)
    #[test]
    fn gives_php_variables_and_function_calls_their_own_colours() {
        let source = SchemeSource::new(TOKENS_CSS);
        let (light, dark) = (source.tokens(BuiltinThemeId::Light), source.tokens(BuiltinThemeId::Dark));
        assert_eq!(dark.get("--syn-php-variable"), Some("#c77dbb"));
        assert_eq!(dark.get("--syn-php-function-call"), Some("#56a8f5"));
        assert_eq!(light.get("--syn-php-variable"), Some("#871094"));
        assert_eq!(light.get("--syn-php-function-call"), Some("#00627a"));
        let php_rule = TOKENS_CSS.split_once(r#".cm-editor[data-language="PHP"] {"#).map(|(_, body)| body.trim_start());
        assert!(php_rule.is_some_and(|body| body.starts_with("--syn-variable: var(--syn-php-variable);")));
        // An imported PHP_VAR still wins: language overrides are emitted for the same element, !important.
        let edits = edit("dark", "php.identifier", foreground("#48d8ee"));
        let root = ThemeAttributes::default();
        let imported = scheme_css(&source, Some("theme-default"), Some(&edits), Some(&root), &BTreeMap::new());
        assert!(imported.contains(".cm-editor[data-language=\"PHP\"] {\n  --syn-variable: #48d8ee !important;"));
    }

    // src/platform/colorScheme.test.ts › color schemes › offers a Go scheme page and scopes its overrides to Go
    // editors
    #[test]
    fn offers_a_go_scheme_page_and_scopes_its_overrides_to_go_editors() {
        let source = SchemeSource::new(TOKENS_CSS);
        let go_constant = attributes().iter().find(|a| a.id == "go.constant");
        assert_eq!(go_constant.map(|a| a.path.clone()), Some(vec!["Go".to_owned()]));
        let edits = edit("dark", "go.constant", foreground("#224466"));
        let css = scheme_css(&source, Some("dark"), Some(&edits), Some(&ThemeAttributes::default()), &BTreeMap::new());
        assert!(css.contains(r#".cm-editor[data-language="Go"]"#));
        assert!(css.contains("--syn-constant: #224466 !important"));
    }

    // src/platform/colorScheme.test.ts › color schemes › resolves scheme attribute state and editor defaults
    #[test]
    fn resolves_scheme_attribute_state_and_editor_defaults() {
        let source = SchemeSource::new(TOKENS_CSS);
        let none = BTreeMap::new();
        let light = resolved_attribute(&source, "light", "lineComment", &none);
        assert_eq!((light.foreground.as_deref(), light.italic), (Some("#8c8c8c"), Some(true)));
        let dark = resolved_attribute(&source, "dark", "lineComment", &none);
        assert_eq!((dark.foreground.as_deref(), dark.italic), (Some("#7a7e85"), Some(false)));
        assert_eq!(resolved_attribute(&source, "dark", "mdHeading", &none).bold, Some(true));

        let todo = TextAttributesOverride {
            foreground: Some("#139839".to_owned()),
            background: Some("#101010".to_owned()),
            bold: Some(true),
            italic: Some(false),
            effect: Some("wave".to_owned()),
            effect_color: None,
        };
        let customs = custom("custom-test", "Test", "dark", BTreeMap::from([("todo".to_owned(), todo.clone())]));
        let resolved = resolved_attribute(&source, "custom-test", "todo", &customs);
        assert_eq!(TextAttributesOverride { effect_color: None, ..resolved }, todo);
    }

    // src/platform/colorScheme.test.ts › color schemes › resolves package references from the editor foreground
    // and emits their overrides
    #[test]
    fn resolves_package_references_from_the_editor_foreground_and_emits_their_overrides() {
        let source = SchemeSource::new(TOKENS_CSS);
        let none = BTreeMap::new();
        for theme in [BuiltinThemeId::Light, BuiltinThemeId::Dark, BuiltinThemeId::DarkHc] {
            let resolved = resolved_attribute(&source, theme.as_str(), "packageReference", &none);
            assert_eq!(resolved.foreground.as_deref(), source.tokens(theme).get("--fg-editor"), "{theme}");
        }
        let package = TextAttributesOverride { italic: Some(true), ..foreground("#446688") };
        let edits = edit("dark", "packageReference", package);
        let css = scheme_css(&source, Some("dark"), Some(&edits), Some(&ThemeAttributes::default()), &none);
        assert!(css.contains("--syn-namespace: #446688 !important"));
        assert!(css.contains("--syn-namespace-italic: italic !important"));
    }

    // src/platform/colorScheme.test.ts › color schemes › emits scheme and override declarations
    #[test]
    fn emits_scheme_and_override_declarations() {
        let source = SchemeSource::new(TOKENS_CSS);
        let (root, none) = (ThemeAttributes::light(), BTreeMap::new());
        let css = |setting: &str, edits: Option<&ColorOverrides>| {
            scheme_css(&source, Some(setting), edits, Some(&root), &none)
        };
        let edited = |attribute: &str, value: TextAttributesOverride| {
            css("theme-default", Some(&edit("light", attribute, value)))
        };
        let italic = |italic: bool| TextAttributesOverride { italic: Some(italic), ..Default::default() };
        assert_eq!(css("theme-default", None), "");
        assert!(css("dark", None).contains("--syn-keyword: #cf8e6d !important"));
        let over = edited("keyword", TextAttributesOverride { bold: Some(true), ..foreground("#ff0000") });
        assert!(over.contains("--syn-keyword: #ff0000 !important"));
        // Explicit Bold / regular follow Typography Settings' bold / main weight.
        assert!(over.contains("--syn-keyword-bold: var(--fw-code-bold, 700) !important"));
        let regular = edited("keyword", TextAttributesOverride { bold: Some(false), ..Default::default() });
        assert!(regular.contains("--syn-keyword-bold: var(--fw-code, 400) !important"));
        let transparent = TextAttributesOverride { background: Some(CLEAR_COLOR.to_owned()), ..Default::default() };
        let cleared = edited("keyword", transparent);
        assert!(cleared.contains("--syn-keyword-bg: transparent !important"));
        let upright = edited("staticSymbol", italic(false));
        assert!(upright.contains("--syn-static-italic: normal !important"));
        let python_italic = edited("pyStaticSymbol", italic(true));
        assert!(python_italic.contains("--syn-python-static-italic: italic !important"));
    }

    // src/platform/colorScheme.test.ts › color schemes › applies an explicit scheme over a plugin theme of the
    // same base
    #[test]
    fn applies_an_explicit_scheme_over_a_plugin_theme_of_the_same_base() {
        let source = SchemeSource::new(TOKENS_CSS);
        let none = BTreeMap::new();
        let plugin = dark_root().with(PLUGIN_THEME_ATTRIBUTE, "theme-one/one-dark");
        // "Theme default" keeps the plugin theme's own editor colours.
        assert_eq!(scheme_css(&source, Some("theme-default"), None, Some(&plugin), &none), "");
        assert!(
            scheme_css(&source, Some("dark"), None, Some(&plugin), &none).contains("--syn-keyword: #cf8e6d !important")
        );
        assert_eq!(scheme_css(&source, Some("dark"), None, Some(&dark_root()), &none), "");
    }

    // src/platform/colorScheme.test.ts › color schemes › re-applies the scheme when a plugin theme of the same
    // base is selected
    #[test]
    fn re_applies_the_scheme_when_a_plugin_theme_of_the_same_base_is_selected() {
        let source = SchemeSource::new(TOKENS_CSS);
        let (none, edits) = (BTreeMap::new(), BTreeMap::new());
        let style = |root: &ThemeAttributes| scheme_style(&source, Some("dark"), Some(&edits), Some(root), &none);
        assert_eq!(style(&dark_root()), SchemeStyle::default());
        let selected = style(&dark_root().with(PLUGIN_THEME_ATTRIBUTE, "theme-one/one-dark"));
        assert!(selected.root.contains(&("--syn-keyword".to_owned(), "#cf8e6d".to_owned())));
    }

    // src/platform/colorScheme.test.ts › color schemes › switches a chosen scheme of the other kind back to the
    // theme's own when the theme turns light or dark
    #[test]
    fn switches_a_chosen_scheme_of_the_other_kind_back_to_the_themes_own() {
        let none = BTreeMap::new();
        assert_eq!(scheme_after_theme_change(Some("one-dark"), ThemeKind::Light, &none), Some("theme-default"));
        assert_eq!(scheme_after_theme_change(Some("solarized-light"), ThemeKind::Light, &none), None);
        assert_eq!(scheme_after_theme_change(Some("theme-default"), ThemeKind::Light, &none), None);
        let customs = custom("custom-night", "Night", "dark", BTreeMap::new());
        assert_eq!(scheme_after_theme_change(Some("custom-night"), ThemeKind::Light, &customs), Some("theme-default"));
        assert_eq!(scheme_after_theme_change(Some("custom-night"), ThemeKind::Dark, &customs), None);
    }

    // src/platform/colorScheme.test.ts › color schemes › keeps editor weights out of the schemes
    #[test]
    fn keeps_editor_weights_out_of_the_schemes() {
        let source = SchemeSource::new(TOKENS_CSS);
        for id in [BuiltinThemeId::Light, BuiltinThemeId::Dark, BuiltinThemeId::DarkHc] {
            assert!(!source.tokens(id).has("--fw-code"), "{id}");
            assert!(!source.tokens(id).has("--syn-keyword-bold"), "{id}");
        }
        // Dark schemes bold matched braces with the configured bold weight, even under the light theme.
        assert_eq!(source.tokens(BuiltinThemeId::Dark).get("--matched-brace-weight"), Some("var(--fw-code-bold)"));
        assert_eq!(source.tokens(BuiltinThemeId::Light).get("--matched-brace-weight"), Some("inherit"));
        let css = scheme_css(&source, Some("dark"), None, Some(&ThemeAttributes::light()), &BTreeMap::new());
        assert!(css.contains("--matched-brace-weight: var(--fw-code-bold) !important"));
    }
}
