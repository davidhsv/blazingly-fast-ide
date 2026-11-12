//! IntelliJ editor schemes (`.icls`): importing one as a custom colour scheme, and exporting a
//! scheme with its edits. Unsupported keys are counted, never copied into tokens.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::OnceLock;

use super::color_scheme::{
    AttributeDef, AttributeOverrides, CLEAR_COLOR, ColorOverrides, CustomColorScheme, CustomColorSchemes, SchemeSource,
    THEME_DEFAULT_SCHEME, TextAttributesOverride, attributes, is_hex_color, list_schemes, resolved_attribute,
    resolved_scheme_tokens, validated_schemes,
};
use super::themes::{BuiltinThemeId, ThemeKind};

/// IntelliJ attribute keys and the editor attribute each one sets. Many language keys share the
/// editor's language-default tokens.
const DEFAULT_NAMES: &[(&str, &str)] = &[
    ("TEXT", "text"),
    ("DEFAULT_TEXT", "text"),
    ("DEFAULT_KEYWORD", "keyword"),
    ("DEFAULT_IDENTIFIER", "identifier"),
    ("DEFAULT_LOCAL_VARIABLE", "identifier"),
    ("DEFAULT_PARAMETER", "parameter"),
    ("DEFAULT_GLOBAL_VARIABLE", "globalVariable"),
    ("DEFAULT_CONSTANT", "constant"),
    ("DEFAULT_INSTANCE_FIELD", "field"),
    ("DEFAULT_STATIC_FIELD", "constant"),
    ("DEFAULT_FUNCTION_DECLARATION", "functionDeclaration"),
    ("DEFAULT_FUNCTION_CALL", "functionCall"),
    ("DEFAULT_PREDEFINED_SYMBOL", "predefined"),
    ("DEFAULT_CLASS_NAME", "className"),
    ("DEFAULT_INTERFACE_NAME", "className"),
    ("DEFAULT_NUMBER", "number"),
    ("DEFAULT_STRING", "string"),
    ("DEFAULT_VALID_STRING_ESCAPE", "escape"),
    ("DEFAULT_INVALID_STRING_ESCAPE", "invalid"),
    ("DEFAULT_LINE_COMMENT", "lineComment"),
    ("DEFAULT_BLOCK_COMMENT", "lineComment"),
    ("DEFAULT_DOC_COMMENT", "docComment"),
    ("DEFAULT_DOC_COMMENT_TAG", "docCommentTag"),
    ("DEFAULT_DOC_COMMENT_TAG_VALUE", "docCommentTagValue"),
    ("DEFAULT_DOC_MARKUP", "docCommentMarkup"),
    ("DEFAULT_METADATA", "metadata"),
    ("DEFAULT_OPERATION_SIGN", "operator"),
    ("DEFAULT_BRACES", "operator"),
    ("DEFAULT_BRACKETS", "operator"),
    ("DEFAULT_PARENTHS", "operator"),
    ("DEFAULT_DOT", "operator"),
    ("DEFAULT_COMMA", "operator"),
    ("DEFAULT_SEMICOLON", "operator"),
    ("DEFAULT_MARKUP_TAG", "markupTag"),
    ("DEFAULT_MARKUP_ATTRIBUTE", "markupAttribute"),
    ("BAD_CHARACTER", "invalid"),
    ("TODO_DEFAULT_ATTRIBUTES", "todo"),
    ("SEARCH_RESULT_ATTRIBUTES", "searchResult"),
    ("TEXT_SEARCH_RESULT_ATTRIBUTES", "searchResultCurrent"),
    ("WRITE_SEARCH_RESULT_ATTRIBUTES", "identifierUnderCaretWrite"),
    ("IDENTIFIER_UNDER_CARET_ATTRIBUTES", "identifierUnderCaret"),
    ("MATCHED_BRACE_ATTRIBUTES", "matchedBrace"),
    ("ERRORS_ATTRIBUTES", "error"),
    ("WARNING_ATTRIBUTES", "warning"),
    ("WEAK_WARNING_ATTRIBUTES", "weakWarning"),
    ("NOT_USED_ELEMENT_ATTRIBUTES", "unused"),
    ("FOLDED_TEXT_ATTRIBUTES", "folded"),
    ("INLAY_DEFAULT", "inlay"),
    ("PY_SELF_PARAMETER", "pySelf"),
    ("PY_KEYWORD_ARGUMENT", "pyKeywordArg"),
    ("PY_DOC_STRING", "pyDocstring"),
    ("PY_BUILTIN_NAME", "pyBuiltin"),
    ("PY_DECORATOR", "pyDecorator"),
    ("MARKDOWN_HEADER_LEVEL_1", "mdHeading"),
    ("MARKDOWN_CODE_SPAN", "mdCode"),
    ("MARKDOWN_LINK_TEXT", "mdLink"),
    ("REGEXP", "regexp"),
];

/// Keys that exist bare (a language default) and as `JAVA_…` / `KOTLIN_…`.
const LANGUAGE_KEYS: &[(&str, &str)] = &[
    ("KEYWORD", "keyword"),
    ("NUMBER", "number"),
    ("STRING", "string"),
    ("STRING_TEXT", "string"),
    ("VALID_STRING_ESCAPE", "escape"),
    ("LINE_COMMENT", "lineComment"),
    ("BLOCK_COMMENT", "lineComment"),
    ("DOC_COMMENT", "docComment"),
    ("CLASS", "className"),
    ("CLASS_NAME", "className"),
    ("INTERFACE", "className"),
    ("ENUM", "className"),
    ("TYPE_PARAMETER", "className"),
    ("ANNOTATION_NAME", "metadata"),
    ("ANNOTATION", "metadata"),
    ("LOCAL_VARIABLE", "identifier"),
    ("PARAMETER", "parameter"),
    ("INSTANCE_FIELD", "field"),
    ("STATIC_FIELD", "constant"),
    ("STATIC_FINAL_FIELD", "constant"),
    ("PROPERTY", "field"),
    ("BACKING_FIELD_VARIABLE", "field"),
    ("FUNCTION_DECLARATION", "functionDeclaration"),
    ("METHOD_DECLARATION", "functionDeclaration"),
    ("FUNCTION_CALL", "functionCall"),
    ("METHOD_CALL", "functionCall"),
    ("STATIC_METHOD", "functionCall"),
    ("CONSTRUCTOR_CALL", "className"),
    ("OPERATION_SIGN", "operator"),
    ("BRACES", "operator"),
    ("BRACKETS", "operator"),
    ("PARENTH", "operator"),
    ("PARENTHS", "operator"),
];

/// Keys of one language: Kotlin, Java docs, PHP and PyCharm's dotted ones.
const LANGUAGE_NAMES: &[(&str, &str)] = &[
    ("KOTLIN_MUTABLE_VARIABLE", "kotlin.identifier"),
    ("KOTLIN_PACKAGE_PROPERTY", "kotlin.field"),
    ("KOTLIN_SYNTHETIC_EXTENSION_PROPERTY", "kotlin.field"),
    ("KOTLIN_EXTENSION_FUNCTION_CALL", "kotlin.functionCall"),
    ("KOTLIN_NAMED_ARGUMENT", "kotlin.parameter"),
    ("KOTLIN_OBJECT", "kotlin.className"),
    ("JAVA_DOC_TAG", "java.docCommentTag"),
    ("DOC_COMMENT_TAG_VALUE", "java.docCommentTagValue"),
    ("JAVA_DOC_MARKUP", "java.docCommentMarkup"),
    ("KDOC_TAG_NAME", "kotlin.docCommentTag"),
    ("KDOC_LINK", "kotlin.docCommentTagValue"),
    ("CLASS_NAME_ATTRIBUTES", "className"),
    ("PHP_KEYWORD", "php.keyword"),
    ("PHP_VAR", "php.identifier"),
    ("PHP_PARAMETER", "php.parameter"),
    ("PHP_CLASS", "php.className"),
    ("PHP_CONSTANT", "php.constant"),
    ("PHP_INSTANCE_FIELD", "php.field"),
    ("PHP_STATIC_FIELD", "php.staticField"),
    ("PHP_FUNCTION", "php.functionDeclaration"),
    ("PHP_FUNCTION_CALL", "php.functionCall"),
    ("PHP_INSTANCE_METHOD", "php.functionCall"),
    ("PHP_PREDEFINED SYMBOL", "php.predefined"),
    ("PHP_NUMBER", "php.number"),
    ("PHP_STRING", "php.string"),
    ("PHP_ESCAPE_SEQUENCE", "php.escape"),
    ("PHP_COMMENT", "php.lineComment"),
    ("PHP_DOC_COMMENT_ID", "php.docComment"),
    ("PHP_DOC_TAG", "php.docCommentTag"),
    ("PHP_DOC_VAR", "php.docCommentTagValue"),
    ("PHP_MARKUP_ID", "php.docCommentMarkup"),
    ("PHP_ATTRIBUTE", "php.metadata"),
    ("PHP_OPERATION_SIGN", "php.operator"),
    ("PHP_BRACKETS", "php.brackets"),
    ("PHP_COMMA", "php.separator"),
    ("PHP_SEMICOLON", "php.separator"),
    ("PHP_TAG", "php.scriptTag"),
    ("PY.ANNOTATION", "pyAnnotation"),
    ("PY.PARENTHS", "pyParentheses"),
    ("PY.BRACKETS", "pyBrackets"),
    ("PY.BRACES", "pyBraces"),
    ("PY.KEYWORD", "keyword"),
    ("PY.FUNC_DEFINITION", "functionDeclaration"),
    ("PY.NESTED_FUNC_DEFINITION", "pyNestedFunction"),
    ("PY.METHOD_CALL", "pyMethodCall"),
    ("PY.DOC_COMMENT", "pyDocstring"),
    ("PY.BUILTIN_NAME", "pyBuiltin"),
    ("PY.DECORATOR", "pyDecorator"),
    ("PY.SELF_PARAMETER", "pySelf"),
    ("PY.KEYWORD_ARGUMENT", "pyKeywordArg"),
    ("PY.PREDEFINED_DEFINITION", "pyPredefinedDefinition"),
    ("PY.PREDEFINED_USAGE", "pyPredefinedUsage"),
    ("PY.STRING.B", "pyBytes"),
    ("PY.STRING.F", "pyFString"),
    ("PY.VALID_STRING_ESCAPE", "escape"),
    ("PY.OPERATION_SIGN", "operator"),
];

/// Every key an import understands with the attribute it sets, in a fixed order: an export
/// writes an attribute under the first key that names it.
fn attribute_names() -> &'static [(String, String)] {
    /// Sets `name`; a name set before keeps its place.
    fn assign(names: &mut Vec<(String, String)>, name: String, id: String) {
        match names.iter_mut().find(|(key, _)| *key == name) {
            Some(entry) => entry.1 = id,
            None => names.push((name, id)),
        }
    }
    static NAMES: OnceLock<Vec<(String, String)>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names = Vec::new();
        for &(name, id) in DEFAULT_NAMES {
            assign(&mut names, name.to_owned(), id.to_owned());
        }
        for &(name, id) in LANGUAGE_KEYS {
            assign(&mut names, format!("JAVA_{name}"), format!("java.{id}"));
            assign(&mut names, format!("KOTLIN_{name}"), format!("kotlin.{id}"));
            assign(&mut names, name.to_owned(), id.to_owned());
        }
        for &(name, id) in LANGUAGE_NAMES {
            assign(&mut names, name.to_owned(), id.to_owned());
        }
        // Editor attributes without a standard exchange name survive a round trip of our own.
        for attr in attributes() {
            if !names.iter().any(|(_, id)| *id == attr.id) {
                names.push((format!("OPENIDEA_{}", attr.id), attr.id.clone()));
            }
        }
        names
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorField {
    Foreground,
    Background,
}

impl ColorField {
    fn of(self, attribute: &TextAttributesOverride) -> Option<&str> {
        match self {
            Self::Foreground => attribute.foreground.as_deref(),
            Self::Background => attribute.background.as_deref(),
        }
    }

    fn set(self, attribute: &mut TextAttributesOverride, color: String) {
        match self {
            Self::Foreground => attribute.foreground = Some(color),
            Self::Background => attribute.background = Some(color),
        }
    }
}

/// IntelliJ `<colors>` keys: the editor attribute and the colour of it each one sets.
const COLOR_NAMES: &[(&str, &str, ColorField)] = &[
    ("CARET_COLOR", "caret", ColorField::Foreground),
    ("CARET_ROW_COLOR", "caretRow", ColorField::Background),
    ("SELECTION_BACKGROUND", "selection", ColorField::Background),
    ("INACTIVE_SELECTION_BACKGROUND", "selectionInactive", ColorField::Background),
    ("GUTTER_BACKGROUND", "gutter", ColorField::Background),
    ("LINE_NUMBERS_COLOR", "lineNumber", ColorField::Foreground),
    ("LINE_NUMBER_ON_CARET_ROW_COLOR", "lineNumberCaret", ColorField::Foreground),
    ("WHITESPACES", "whitespaces", ColorField::Foreground),
    ("INDENT_GUIDE", "indentGuide", ColorField::Foreground),
    ("RIGHT_MARGIN_COLOR", "rightMargin", ColorField::Foreground),
    ("METHOD_SEPARATORS_COLOR", "methodSeparator", ColorField::Foreground),
];

/// The effect of an IntelliJ `EFFECT_TYPE` code. The box (0) and faded text (6) have no
/// counterpart here; the bold and the bold dotted line (4, 5) become an underline.
fn imported_effect(code: &str) -> Option<&'static str> {
    match code {
        "1" | "4" | "5" => Some("underline"),
        "2" => Some("wave"),
        "3" => Some("strikeout"),
        _ => None,
    }
}

fn effect_code(effect: &str) -> Option<&'static str> {
    match effect {
        "underline" => Some("1"),
        "wave" => Some("2"),
        "strikeout" => Some("3"),
        _ => None,
    }
}

/// Bold and italic of an IntelliJ `FONT_TYPE` (bit 0: bold, bit 1: italic).
fn font_type(value: &str) -> Option<(bool, bool)> {
    match value {
        "0" => Some((false, false)),
        "1" => Some((true, false)),
        "2" => Some((false, true)),
        "3" => Some((true, true)),
        _ => None,
    }
}

/// An IntelliJ colour (up to six hex digits, leading zeros omitted) as `#rrggbb`.
fn color(value: Option<&str>) -> Option<String> {
    let value = value?;
    let valid = (1..=6).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_hexdigit());
    valid.then(|| format!("#{:0>6}", value.to_ascii_lowercase()))
}

// --- XML ---

/// One element of a parsed document. Text content is checked but not kept: schemes carry
/// everything in attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Element {
    name: String,
    attributes: Vec<(String, String)>,
    children: Vec<Element>,
}

impl Element {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    fn children_named<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a Element> {
        self.children.iter().filter(move |child| child.name == tag)
    }
}

/// Elements nested deeper than this are refused, so a hostile file cannot exhaust the stack
/// when its tree is dropped.
const MAX_DEPTH: usize = 256;

fn is_xml_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

/// Splits an element or attribute name off the front of `text`.
fn take_name(text: &str) -> Option<(&str, &str)> {
    let end = text.find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | ':' | '-' | '.'))).unwrap_or(text.len());
    let (name, rest) = text.split_at(end);
    name.starts_with(|c: char| c.is_alphabetic() || matches!(c, '_' | ':')).then_some((name, rest))
}

/// The character of `&#…;`: `code` is what follows the `#`.
fn char_reference(code: &str) -> Option<char> {
    let (digits, radix) = match code.strip_prefix('x') {
        Some(hex) => (hex, 16u32),
        None => (code, 10u32),
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let c = char::from_u32(u32::from_str_radix(digits, radix).ok()?)?;
    (matches!(c, '\t' | '\n' | '\r') || (c >= ' ' && !matches!(c, '\u{fffe}' | '\u{ffff}'))).then_some(c)
}

/// Appends literal text of an attribute value: tabs and line breaks become spaces.
fn push_normalized(out: &mut String, text: &str) {
    let mut previous = '\0';
    for c in text.chars() {
        match c {
            '\n' if previous == '\r' => {}
            '\t' | '\n' | '\r' => out.push(' '),
            c => out.push(c),
        }
        previous = c;
    }
}

/// An attribute value (or text) with its references resolved; `None` for a reference that is
/// not one of the five predefined entities or a character reference.
fn unescape(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        push_normalized(&mut out, &rest[..at]);
        let end = rest[at..].find(';')?;
        out.push(match &rest[at + 1..at + end] {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "apos" => '\'',
            "quot" => '"',
            reference => char_reference(reference.strip_prefix('#')?)?,
        });
        rest = &rest[at + end + 1..];
    }
    push_normalized(&mut out, rest);
    Some(out)
}

/// A start tag without its `<`: the element, whether it is empty (`/>`), and what follows.
fn start_tag(text: &str) -> Option<(Element, bool, &str)> {
    let (name, mut rest) = take_name(text)?;
    let mut element = Element { name: name.to_owned(), attributes: Vec::new(), children: Vec::new() };
    loop {
        let after = rest.trim_start_matches(is_xml_space);
        if let Some(after) = after.strip_prefix("/>") {
            return Some((element, true, after));
        }
        if let Some(after) = after.strip_prefix('>') {
            return Some((element, false, after));
        }
        // Attributes are separated by whitespace.
        if after.len() == rest.len() {
            return None;
        }
        let (key, after) = take_name(after)?;
        let after = after.trim_start_matches(is_xml_space).strip_prefix('=')?.trim_start_matches(is_xml_space);
        let quote = after.chars().next().filter(|c| matches!(c, '"' | '\''))?;
        let (value, after) = after[1..].split_once(quote)?;
        if value.contains('<') || element.attribute(key).is_some() {
            return None;
        }
        element.attributes.push((key.to_owned(), unescape(value)?));
        rest = after;
    }
}

/// Puts a finished element into its parent, or makes it the document's root.
fn close(element: Element, open: &mut [Element], root: &mut Option<Element>) {
    match open.last_mut() {
        Some(parent) => parent.children.push(element),
        None => *root = Some(element),
    }
}

/// Parses the XML that scheme files are written in: elements, attributes in `"` or `'`, the five
/// predefined entities and numeric character references, comments, CDATA sections, processing
/// instructions and the XML declaration. `None` when the text is not well formed in those
/// terms, has no or several root elements, or declares a document type.
///
/// Limits: no DTDs (any `<!DOCTYPE` is refused, so no custom entities), no namespace
/// processing (a prefixed name is just a name), names are letters, digits, `_`, `:`, `-` and
/// `.`, at most [`MAX_DEPTH`] nested elements, and the characters of text, comments and
/// attribute values are not checked against XML's character ranges.
fn parse_xml(xml: &str) -> Option<Element> {
    let mut rest = xml.strip_prefix('\u{feff}').unwrap_or(xml);
    let mut open: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;
    while let Some(at) = rest.find('<') {
        let (text, markup) = rest.split_at(at);
        if open.is_empty() {
            if !text.chars().all(is_xml_space) {
                return None;
            }
        } else {
            unescape(text)?;
        }
        rest = if let Some(comment) = markup.strip_prefix("<!--") {
            comment.split_once("-->")?.1
        } else if let Some(instruction) = markup.strip_prefix("<?") {
            instruction.split_once("?>")?.1
        } else if let Some(data) = markup.strip_prefix("<![CDATA[") {
            if open.is_empty() {
                return None;
            }
            data.split_once("]]>")?.1
        } else if let Some(end_tag) = markup.strip_prefix("</") {
            let (name, after) = take_name(end_tag)?;
            let element = open.pop().filter(|element| element.name == name)?;
            close(element, &mut open, &mut root);
            after.trim_start_matches(is_xml_space).strip_prefix('>')?
        } else if markup.starts_with("<!") {
            return None;
        } else {
            let (element, empty, after) = start_tag(&markup[1..])?;
            if (open.is_empty() && root.is_some()) || open.len() >= MAX_DEPTH {
                return None;
            }
            if empty {
                close(element, &mut open, &mut root);
            } else {
                open.push(element);
            }
            after
        };
    }
    if open.is_empty() && rest.chars().all(is_xml_space) { root } else { None }
}

// --- import ---

/// The file is not a colour scheme: not well-formed XML, or no named `<scheme>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidIcls;

impl fmt::Display for InvalidIcls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Invalid .icls color scheme")
    }
}

impl std::error::Error for InvalidIcls {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedIcls {
    pub scheme: CustomColorScheme,
    /// Values of the file the scheme took over.
    pub applied: usize,
    /// Values it has no counterpart for.
    pub skipped: usize,
}

/// The `<option>` entries of an `<attributes>` section in the order they apply. In a PyCharm
/// scheme the shared defaults come before the Python keys that refine them.
fn ordered_attributes(section: &Element) -> Vec<&Element> {
    let named = |entry: &Element, prefix: &str| entry.attribute("name").is_some_and(|name| name.starts_with(prefix));
    let entries: Vec<&Element> = section.children_named("option").collect();
    if !entries.iter().any(|entry| named(entry, "PY.")) {
        return entries;
    }
    let mut entries: Vec<&Element> =
        entries.into_iter().filter(|entry| entry.attribute("name") != Some("DEFAULT_BLOCK_COMMENT")).collect();
    entries.sort_by_key(|entry| {
        if named(entry, "PY.") {
            2u8
        } else if named(entry, "DEFAULT_") {
            1
        } else {
            0
        }
    });
    entries
}

/// The `<value>` of an entry, or of the first entry up its `baseAttributes` chain that has one.
fn inherited_value<'a>(entry: &'a Element, entries: &HashMap<Option<&'a str>, &'a Element>) -> Option<&'a Element> {
    let mut seen: Vec<&Element> = Vec::new();
    let mut current = Some(entry);
    while let Some(element) = current.filter(|element| !seen.iter().any(|visited| std::ptr::eq(*visited, *element))) {
        seen.push(element);
        if let Some(value) = element.children_named("value").next() {
            return Some(value);
        }
        current = entries.get(&element.attribute("baseAttributes")).copied();
    }
    None
}

/// Perceived brightness (0–255) of a `#rrggbb` colour.
fn luma(color: &str) -> f64 {
    let channel =
        |at: usize| color.get(at..at + 2).and_then(|hex| u8::from_str_radix(hex, 16).ok()).map_or(255.0, f64::from);
    channel(1) * 0.299 + channel(3) * 0.587 + channel(5) * 0.114
}

pub fn parse_icls(xml: &str) -> Result<ParsedIcls, InvalidIcls> {
    let root = parse_xml(xml).filter(|root| root.name == "scheme").ok_or(InvalidIcls)?;
    let name = root.attribute("name").map(str::trim).filter(|name| !name.is_empty()).ok_or(InvalidIcls)?;
    let entries: HashMap<Option<&str>, &Element> = root
        .children_named("attributes")
        .flat_map(|section| section.children_named("option"))
        .map(|entry| (entry.attribute("name"), entry))
        .collect();
    let mut imported = AttributeOverrides::new();
    let (mut applied, mut skipped) = (0usize, 0usize);
    for section in &root.children {
        if section.name == "colors" {
            for entry in section.children_named("option") {
                let target = COLOR_NAMES.iter().find(|(key, ..)| entry.attribute("name") == Some(*key));
                match (target, color(entry.attribute("value"))) {
                    (Some(&(_, id, field)), Some(color)) => {
                        field.set(imported.entry(id.to_owned()).or_default(), color);
                        applied += 1;
                    }
                    _ => skipped += 1,
                }
            }
        }
        if section.name != "attributes" {
            continue;
        }
        for entry in ordered_attributes(section) {
            let name = entry.attribute("name").unwrap_or("");
            let id = attribute_names().iter().find(|(key, _)| key == name).map(|(_, id)| id);
            let attr = id.and_then(|id| attributes().iter().find(|attr| attr.id == *id));
            let (Some(attr), Some(value)) = (attr, inherited_value(entry, &entries)) else {
                skipped += 1;
                continue;
            };
            let styled = attr.token.is_some();
            let mut merged = imported.get(&attr.id).cloned().unwrap_or_default();
            // An explicit TextAttributes value defaults to Font.PLAIN, not the last alias's style.
            if styled {
                (merged.bold, merged.italic) = (Some(false), Some(false));
            }
            if name.starts_with("PY.") {
                merged.effect = Some("none".to_owned());
            }
            // IntelliJ draws no effect without an effect colour.
            let drawn = value
                .children_named("option")
                .any(|o| o.attribute("name") == Some("EFFECT_COLOR") && color(o.attribute("value")).is_some());
            for option in value.children_named("option") {
                let raw = option.attribute("value");
                let done = match (option.attribute("name"), color(raw)) {
                    (Some("FOREGROUND"), Some(color)) if attr.fg.is_some() => {
                        merged.foreground = Some(color);
                        true
                    }
                    (Some("BACKGROUND"), Some(color)) if attr.bg.is_some() || styled => {
                        merged.background = Some(color);
                        true
                    }
                    (Some("EFFECT_COLOR"), Some(color)) if attr.effect.is_some() || styled => {
                        merged.effect_color = Some(color);
                        true
                    }
                    (Some("FONT_TYPE"), _) if styled => match raw.and_then(font_type) {
                        Some((bold, italic)) => {
                            (merged.bold, merged.italic) = (Some(bold), Some(italic));
                            true
                        }
                        None => false,
                    },
                    (Some("EFFECT_TYPE"), _) if drawn && (styled || attr.effect.is_some()) => {
                        let effect = raw.and_then(imported_effect);
                        if let Some(effect) = effect {
                            merged.effect = Some(effect.to_owned());
                        }
                        effect.is_some()
                    }
                    _ => false,
                };
                if done {
                    applied += 1;
                } else {
                    skipped += 1;
                }
            }
            // An entry that set nothing leaves no attribute behind.
            if merged != TextAttributesOverride::default() {
                imported.insert(attr.id.clone(), merged);
            }
        }
    }
    // The editor background decides light or dark; without one, the parent scheme's name does.
    let background = imported.get("text").and_then(|text| text.background.as_deref());
    let parent_name = root.attribute("parent_scheme").unwrap_or("").to_ascii_lowercase();
    let dark = match background {
        Some(background) => luma(background) < 128.0,
        None => parent_name.contains("darcula") || parent_name.contains("dark"),
    };
    let parent = if dark { ThemeKind::Dark } else { ThemeKind::Light };
    let scheme = CustomColorScheme { name: name.to_owned(), parent: parent.as_str().to_owned(), attributes: imported };
    Ok(ParsedIcls { scheme, applied, skipped })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedScheme {
    pub id: String,
    pub customs: CustomColorSchemes,
}

/// Adds `scheme` to `customs`. Name collisions include built-ins: unless `overwrite` replaces
/// the custom scheme of that name, the import is renamed `Name (2)`…; it never replaces a
/// built-in. A new scheme gets the id `custom-<unique>`: pass a fresh UUID.
pub fn import_scheme(
    customs: &CustomColorSchemes,
    scheme: CustomColorScheme,
    overwrite: bool,
    unique: &str,
) -> ImportedScheme {
    let existing = customs.iter().find(|(_, s)| s.name == scheme.name).map(|(id, _)| id.clone()).filter(|_| overwrite);
    let mut name = scheme.name.clone();
    if existing.is_none() {
        let names: HashSet<String> = list_schemes(customs).into_iter().map(|s| s.name).collect();
        let mut n = 2u32;
        while names.contains(&name) {
            name = format!("{} ({n})", scheme.name);
            n += 1;
        }
    }
    let id = existing.unwrap_or_else(|| format!("custom-{unique}"));
    let mut customs = customs.clone();
    customs.insert(id.clone(), CustomColorScheme { name, ..scheme });
    ImportedScheme { id, customs }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedScheme {
    pub customs: CustomColorSchemes,
    pub overrides: ColorOverrides,
    /// The `editor.colorScheme` setting afterwards: the theme's own if the deleted one was chosen.
    pub selected: String,
}

pub fn delete_scheme(
    customs: &CustomColorSchemes,
    overrides: &ColorOverrides,
    selected: &str,
    id: &str,
) -> DeletedScheme {
    let (mut customs, mut overrides) = (customs.clone(), overrides.clone());
    customs.remove(id);
    overrides.remove(id);
    let selected = if selected == id { THEME_DEFAULT_SCHEME } else { selected };
    DeletedScheme { customs, overrides, selected: selected.to_owned() }
}

// --- export ---

/// `rrggbb` of a `#rgb` or `#rrggbb` colour.
fn export_color(color: &str) -> String {
    let hex = color.strip_prefix('#').unwrap_or(color);
    if hex.len() == 3 { hex.chars().flat_map(|c| [c, c]).collect() } else { hex.to_owned() }
}

fn escape_xml(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;").replace('>', "&gt;")
}

/// `over`'s set fields over `base`.
fn overlaid(base: TextAttributesOverride, over: &TextAttributesOverride) -> TextAttributesOverride {
    TextAttributesOverride {
        foreground: over.foreground.clone().or(base.foreground),
        background: over.background.clone().or(base.background),
        bold: over.bold.or(base.bold),
        italic: over.italic.or(base.italic),
        effect: over.effect.clone().or(base.effect),
        effect_color: over.effect_color.clone().or(base.effect_color),
    }
}

/// The `.icls` text of scheme `id` as the editor shows it: its tokens, a custom scheme's own
/// attributes and the user's `overrides` of it.
pub fn export_icls(
    source: &SchemeSource,
    id: &str,
    customs: &CustomColorSchemes,
    overrides: &AttributeOverrides,
) -> String {
    let valid = validated_schemes(customs);
    let custom = valid.get(id);
    let name = list_schemes(customs).into_iter().find(|s| s.id == id).map_or_else(|| id.to_owned(), |s| s.name);
    let tokens = resolved_scheme_tokens(source, id, customs);
    let attribute = |id: &str| attributes().iter().find(|attr| attr.id == id);
    let values = |attr: &AttributeDef| {
        let hex = |token: &Option<String>| {
            token.as_deref().and_then(|token| tokens.get(token)).filter(|color| is_hex_color(color)).map(str::to_owned)
        };
        let mut v = TextAttributesOverride {
            foreground: hex(&attr.fg),
            background: hex(&attr.bg),
            effect_color: hex(&attr.effect),
            ..TextAttributesOverride::default()
        };
        if let Some(token) = attr.token.as_deref() {
            let bold = tokens.get(&format!("--syn-{token}-bold")).unwrap_or("");
            let heavy = bold.as_bytes().windows(3).any(|w| (b'6'..=b'9').contains(&w[0]) && w[1..] == *b"00");
            let italic = tokens.get(&format!("--syn-{token}-italic")).or_else(|| match token {
                "comment" => tokens.get("--syn-comment-style"),
                "doc-comment" | "docstring" => Some("italic"),
                _ if attr.italic_only && attr.default_italic != Some(false) => Some("italic"),
                _ => Some("normal"),
            });
            (v.bold, v.italic) = (Some(bold.contains("bold") || heavy), Some(italic == Some("italic")));
            let resolved = resolved_attribute(source, id, &attr.id, customs);
            (v.effect, v.effect_color) = (resolved.effect, resolved.effect_color);
        }
        let own = custom.and_then(|scheme| scheme.attributes.get(&attr.id));
        [own, overrides.get(&attr.id)].into_iter().flatten().fold(v, overlaid)
    };

    let mut colors = Vec::new();
    for &(key, id, field) in COLOR_NAMES {
        let v = attribute(id).map(values).unwrap_or_default();
        if let Some(color) = field.of(&v).filter(|color| is_hex_color(color)) {
            colors.push(format!("    <option name=\"{key}\" value=\"{}\" />", export_color(color)));
        }
    }
    let text = attribute("text").map(values).unwrap_or_default();
    let mut exported = Vec::new();
    for attr in attributes() {
        let edited = custom.is_some_and(|scheme| scheme.attributes.contains_key(&attr.id));
        if attr.language.is_some() && !edited && !overrides.contains_key(&attr.id) {
            continue;
        }
        let Some((key, _)) = attribute_names().iter().find(|(_, id)| *id == attr.id) else { continue };
        let mut v = values(attr);
        // A cleared effect colour hides the effect. One left unset takes the text's here, and IntelliJ would
        // draw none.
        let cleared = v.effect_color.as_deref() == Some(CLEAR_COLOR);
        let effect = v.effect.as_deref().filter(|_| !cleared).and_then(effect_code);
        if effect.is_some() && !v.effect_color.as_deref().is_some_and(is_hex_color) {
            v.effect_color = v.foreground.clone().or_else(|| text.foreground.clone());
        }
        let mut options = String::new();
        for (key, color) in
            [("FOREGROUND", &v.foreground), ("BACKGROUND", &v.background), ("EFFECT_COLOR", &v.effect_color)]
        {
            if let Some(color) = color.as_deref().filter(|color| is_hex_color(color)) {
                options += &format!("<option name=\"{key}\" value=\"{}\" />", export_color(color));
            }
        }
        if attr.token.is_some() {
            let font_type = u8::from(v.bold == Some(true)) + 2 * u8::from(v.italic == Some(true));
            options += &format!("<option name=\"FONT_TYPE\" value=\"{font_type}\" />");
        }
        if let Some(effect) = effect {
            options += &format!("<option name=\"EFFECT_TYPE\" value=\"{effect}\" />");
        }
        if !options.is_empty() {
            exported.push(format!("    <option name=\"{key}\"><value>{options}</value></option>"));
        }
    }
    let dark = match custom {
        Some(scheme) => scheme.parent == "dark",
        None => BuiltinThemeId::parse(id).is_none_or(|id| id.kind() == ThemeKind::Dark),
    };
    format!(
        "<scheme name=\"{}\" version=\"142\" parent_scheme=\"{}\">\n  <colors>\n{}\n  </colors>\n  <attributes>\n{}\n  </attributes>\n</scheme>\n",
        escape_xml(&name),
        if dark { "Darcula" } else { "Default" },
        colors.join("\n"),
        exported.join("\n"),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::super::color_scheme::{custom_schemes, effective_scheme, scheme_css, schemes};
    use super::super::tokens::{TOKENS_CSS, ThemeAttributes};
    use super::*;

    const FOREST: &str = include_str!("../../../src/platform/__fixtures__/colorSchemes/forest.icls");
    const DAYLIGHT: &str = include_str!("../../../src/platform/__fixtures__/colorSchemes/daylight.icls");
    const MONOKAI_PHP: &str = include_str!("../../../src/platform/__fixtures__/colorSchemes/monokai-php.icls");

    fn scheme(xml: &str) -> CustomColorScheme {
        parse_icls(xml).unwrap().scheme
    }

    fn attributes_of(xml: &str) -> AttributeOverrides {
        scheme(xml).attributes
    }

    fn one(id: &str, scheme: CustomColorScheme) -> CustomColorSchemes {
        BTreeMap::from([(id.to_owned(), scheme)])
    }

    fn forest() -> ImportedScheme {
        import_scheme(&BTreeMap::new(), scheme(FOREST), false, "forest")
    }

    fn text(value: &str) -> Option<String> {
        Some(value.to_owned())
    }

    fn styled(foreground: &str, bold: bool, italic: bool) -> TextAttributesOverride {
        TextAttributesOverride {
            foreground: text(foreground),
            bold: Some(bold),
            italic: Some(italic),
            ..Default::default()
        }
    }

    fn effect(effect: &str, color: Option<&str>) -> TextAttributesOverride {
        TextAttributesOverride { effect: text(effect), effect_color: color.map(str::to_owned), ..Default::default() }
    }

    fn overrides<const N: usize>(entries: [(&str, TextAttributesOverride); N]) -> AttributeOverrides {
        entries.into_iter().map(|(id, value)| (id.to_owned(), value)).collect()
    }

    fn foreground_of<'a>(attributes: &'a AttributeOverrides, id: &str) -> Option<&'a str> {
        attributes.get(id).and_then(|a| a.foreground.as_deref())
    }

    /// `expect(actual).toMatchObject(expected)`: every field `expected` sets has that value.
    fn assert_matches(actual: Option<&TextAttributesOverride>, expected: &TextAttributesOverride) {
        let actual = actual.expect("the attribute is set");
        let masked = TextAttributesOverride {
            foreground: actual.foreground.clone().filter(|_| expected.foreground.is_some()),
            background: actual.background.clone().filter(|_| expected.background.is_some()),
            bold: actual.bold.filter(|_| expected.bold.is_some()),
            italic: actual.italic.filter(|_| expected.italic.is_some()),
            effect: actual.effect.clone().filter(|_| expected.effect.is_some()),
            effect_color: actual.effect_color.clone().filter(|_| expected.effect_color.is_some()),
        };
        assert_eq!(&masked, expected);
    }

    /// The first `<option name="…">` below `element`, in document order.
    fn find_option<'a>(element: &'a Element, name: &str) -> Option<&'a Element> {
        element.children.iter().find_map(|child| {
            let found = child.name == "option" && child.attribute("name") == Some(name);
            if found { Some(child) } else { find_option(child, name) }
        })
    }

    /// `EFFECT_TYPE` and `EFFECT_COLOR` of the exported attribute `name`.
    fn exported_effect(xml: &str, name: &str) -> (Option<String>, Option<String>) {
        let document = parse_xml(xml).unwrap();
        let entry = find_option(&document, name);
        let value = |key: &str| {
            entry.and_then(|entry| find_option(entry, key)).and_then(|o| o.attribute("value")).map(str::to_owned)
        };
        (value("EFFECT_TYPE"), value("EFFECT_COLOR"))
    }

    // src/platform/icls.test.ts › .icls schemes › resolves explicit attribute inheritance independent of XML order
    // and stops cycles
    #[test]
    fn resolves_attribute_inheritance_independent_of_xml_order_and_stops_cycles() {
        let attributes = attributes_of(
            r#"<scheme name="Inherited"><attributes>
      <option name="PHP_PARAMETER" baseAttributes="PHP_VAR"/>
      <option name="PHP_VAR" baseAttributes="DEFAULT_IDENTIFIER"/>
      <option name="DEFAULT_IDENTIFIER"><value><option name="FOREGROUND" value="48d8ee"/></value></option>
      <option name="PHP_STRING" baseAttributes="PHP_NUMBER"/>
      <option name="PHP_NUMBER" baseAttributes="PHP_STRING"/>
      </attributes></scheme>"#,
        );
        assert_eq!(attributes.get("php.parameter"), Some(&styled("#48d8ee", false, false)));
        assert_eq!(attributes.get("php.identifier"), attributes.get("php.parameter"));
        assert_eq!(attributes.get("php.string"), None);
    }

    // src/platform/icls.test.ts › .icls schemes › maps doc comment tags, tag values and markup per language
    #[test]
    fn maps_doc_comment_tags_tag_values_and_markup_per_language() {
        let source = SchemeSource::new(TOKENS_CSS);
        let fg = |color: &str| format!(r#"<value><option name="FOREGROUND" value="{color}"/></value>"#);
        let scheme = scheme(&format!(
            r#"<scheme name="Docs" parent_scheme="Darcula"><attributes>
      <option name="DEFAULT_DOC_COMMENT_TAG">{}</option>
      <option name="DEFAULT_DOC_COMMENT_TAG_VALUE">{}</option>
      <option name="DEFAULT_DOC_MARKUP"><value><option name="BACKGROUND" value="e2ffe2"/></value></option>
      <option name="JAVA_DOC_TAG">{}</option>
      <option name="KDOC_LINK">{}</option>
      <option name="PHP_DOC_TAG">{}</option>
      <option name="PHP_DOC_VAR">{}</option>
      <option name="PHP_ATTRIBUTE">{}</option>
      </attributes></scheme>"#,
            fg("67a37c"),
            fg("abadb3"),
            fg("111111"),
            fg("222222"),
            fg("333333"),
            fg("444444"),
            fg("555555"),
        ));
        let attributes = &scheme.attributes;
        assert_eq!(foreground_of(attributes, "docCommentTag"), Some("#67a37c"));
        assert_eq!(foreground_of(attributes, "docCommentTagValue"), Some("#abadb3"));
        let markup = attributes.get("docCommentMarkup").and_then(|a| a.background.as_deref());
        assert_eq!(markup, Some("#e2ffe2"));
        assert_eq!(foreground_of(attributes, "java.docCommentTag"), Some("#111111"));
        assert_eq!(foreground_of(attributes, "kotlin.docCommentTagValue"), Some("#222222"));
        assert_eq!(foreground_of(attributes, "php.docCommentTag"), Some("#333333"));
        assert_eq!(foreground_of(attributes, "php.docCommentTagValue"), Some("#444444"));
        // PhpStorm's PHP 8 attributes, which PHP_DOC_TAG used to stand in for.
        assert_eq!(foreground_of(attributes, "php.metadata"), Some("#555555"));
        let customs = one("custom-docs", scheme.clone());
        let css = scheme_css(&source, Some("custom-docs"), None, None, &customs);
        assert!(css.contains("--syn-doc-comment-tag: #67a37c !important"));
        assert!(css.contains("--syn-doc-markup-bg: #e2ffe2 !important"));
        let imported = attributes_of(&export_icls(&source, "custom-docs", &customs, &BTreeMap::new()));
        for id in [
            "docCommentTag",
            "docCommentTagValue",
            "docCommentMarkup",
            "java.docCommentTag",
            "kotlin.docCommentTagValue",
            "php.docCommentTag",
            "php.docCommentTagValue",
        ] {
            assert_matches(imported.get(id), &attributes[id]);
        }
    }

    // src/platform/icls.test.ts › .icls schemes › preserves PHP colours and regular styles separately from Java and
    // language defaults
    #[test]
    fn preserves_php_colours_and_regular_styles_separately_from_java_and_language_defaults() {
        let source = SchemeSource::new(TOKENS_CSS);
        let scheme = scheme(MONOKAI_PHP);
        let attributes = &scheme.attributes;
        assert_eq!(scheme.parent, "dark");
        assert_eq!(attributes.get("php.identifier"), Some(&styled("#48d8ee", false, false)));
        assert_eq!(attributes.get("php.keyword"), Some(&styled("#f92672", false, false)));
        assert_eq!(attributes.get("php.string"), Some(&styled("#ffe792", false, false)));
        assert_eq!(attributes.get("php.docComment"), Some(&styled("#75715e", false, false)));
        assert_eq!(attributes.get("java.string"), Some(&styled("#ffe792", true, false)));
        assert_eq!(foreground_of(attributes, "keyword"), Some("#41abff"));
        let customs = one("custom-php", scheme.clone());
        let css = scheme_css(&source, Some("custom-php"), None, None, &customs);
        assert!(css.contains(r#".cm-editor[data-language="PHP"]"#));
        assert!(css.contains("--syn-variable: #48d8ee !important"));
        let round_trip = attributes_of(&export_icls(&source, "custom-php", &customs, &BTreeMap::new()));
        assert_eq!(round_trip.get("php.string"), attributes.get("php.string"));
        assert_eq!(round_trip.get("java.string"), attributes.get("java.string"));
    }

    // src/platform/icls.test.ts › .icls schemes › maps defaults, Java/Kotlin, editor surfaces, search and
    // diagnostics with counts
    #[test]
    fn maps_defaults_languages_editor_surfaces_search_and_diagnostics_with_counts() {
        let ParsedIcls { scheme, applied, skipped } = parse_icls(FOREST).unwrap();
        let attributes = &scheme.attributes;
        let background = |id: &str| attributes.get(id).and_then(|a| a.background.as_deref());
        assert_eq!(scheme.name, "Forest & Moss");
        assert_eq!(scheme.parent, "dark");
        assert_eq!(attributes.get("keyword"), Some(&styled("#aadd88", false, false)));
        assert_eq!(attributes.get("java.keyword"), Some(&styled("#99dd77", true, false)));
        assert_eq!(attributes.get("kotlin.functionDeclaration"), Some(&styled("#e0c080", false, true)));
        let surface =
            TextAttributesOverride { foreground: text("#d8ebd8"), background: text("#18281e"), ..Default::default() };
        assert_eq!(attributes.get("text"), Some(&surface));
        assert_eq!(foreground_of(attributes, "caret"), Some("#f0e080"));
        assert_eq!(background("selection"), Some("#406040"));
        assert_eq!(background("gutter"), Some("#142018"));
        assert_eq!(background("searchResult"), Some("#6a5530"));
        assert_eq!(attributes.get("error"), Some(&effect("wave", Some("#ff6655"))));
        assert_eq!((applied, skipped), (16, 2));
    }

    // src/platform/icls.test.ts › .icls schemes › rejects malformed XML without mutating schemes: %s
    #[test]
    fn rejects_malformed_xml() {
        for xml in [
            "",
            "<scheme",
            r#"<scheme name="A"><attributes></scheme>"#,
            "<scheme/>",
            r#"<html name="x"/>"#,
            r#"<!DOCTYPE scheme [<!ENTITY a "red">]><scheme name="x"/>"#,
        ] {
            assert_eq!(parse_icls(xml).unwrap_err().to_string(), "Invalid .icls color scheme", "{xml}");
        }
    }

    // src/platform/icls.test.ts › .icls schemes › imports IntelliJ effect type %s as %s
    #[test]
    fn imports_intellij_effect_types() {
        let cases = [
            ("0", None),
            ("1", Some("underline")),
            ("2", Some("wave")),
            ("3", Some("strikeout")),
            ("4", Some("underline")),
            ("5", Some("underline")),
            ("6", None),
        ];
        for (code, expected) in cases {
            let attributes = attributes_of(&format!(
                r#"<scheme name="Effects"><attributes>
      <option name="DEFAULT_KEYWORD"><value><option name="EFFECT_COLOR" value="aa1122"/><option name="EFFECT_TYPE" value="{code}"/></value></option>
    </attributes></scheme>"#
            ));
            assert_eq!(attributes.get("keyword").and_then(|a| a.effect.as_deref()), expected, "{code}");
        }
    }

    // src/platform/icls.test.ts › .icls schemes › imports no effect without an effect colour, which IntelliJ does
    // not draw
    #[test]
    fn imports_no_effect_without_an_effect_colour() {
        let attributes = attributes_of(
            r#"<scheme name="Effects"><attributes>
      <option name="DEFAULT_KEYWORD"><value><option name="FOREGROUND" value="112233"/><option name="EFFECT_TYPE" value="3"/></value></option>
    </attributes></scheme>"#,
        );
        assert_eq!(attributes.get("keyword"), Some(&styled("#112233", false, false)));
    }

    // src/platform/icls.test.ts › .icls schemes › exports effects with IntelliJ's codes and a colour IntelliJ draws
    // them in
    #[test]
    fn exports_effects_with_intellij_codes_and_a_colour_intellij_draws_them_in() {
        let source = SchemeSource::new(TOKENS_CSS);
        let ImportedScheme { id, customs } = forest();
        let edits =
            overrides([("keyword", effect("underline", None)), ("number", effect("strikeout", Some("#aa1122")))]);
        let xml = export_icls(&source, &id, &customs, &edits);
        assert_eq!(exported_effect(&xml, "DEFAULT_KEYWORD"), (text("1"), text("aadd88")));
        assert_eq!(exported_effect(&xml, "ERRORS_ATTRIBUTES"), (text("2"), text("ff6655")));
        assert_eq!(exported_effect(&xml, "DEFAULT_NUMBER"), (text("3"), text("aa1122")));
        let attributes = attributes_of(&xml);
        assert_matches(attributes.get("keyword"), &effect("underline", Some("#aadd88")));
        assert_eq!(attributes.get("error"), Some(&effect("wave", Some("#ff6655"))));
    }

    // src/platform/icls.test.ts › .icls schemes › exports no effect whose colour was cleared
    #[test]
    fn exports_no_effect_whose_colour_was_cleared() {
        let source = SchemeSource::new(TOKENS_CSS);
        let ImportedScheme { id, customs } = forest();
        let cleared = TextAttributesOverride { effect_color: text(CLEAR_COLOR), ..Default::default() };
        let xml = export_icls(&source, &id, &customs, &overrides([("error", cleared)]));
        assert_eq!(attributes_of(&xml).get("error"), None);
    }

    // src/platform/icls.test.ts › .icls schemes › round trips the Islands Light doc comment tag underline
    #[test]
    fn round_trips_the_islands_light_doc_comment_tag_underline() {
        let source = SchemeSource::new(TOKENS_CSS);
        let xml = export_icls(&source, "light", &BTreeMap::new(), &BTreeMap::new());
        assert_eq!(exported_effect(&xml, "DEFAULT_DOC_COMMENT_TAG"), (text("1"), text("999999")));
        let scheme = scheme(&xml);
        assert_matches(scheme.attributes.get("docCommentTag"), &effect("underline", Some("#999999")));
        let customs = one("custom-docs", scheme.clone());
        let imported = attributes_of(&export_icls(&source, "custom-docs", &customs, &BTreeMap::new()));
        assert_eq!(imported.get("docCommentTag"), scheme.attributes.get("docCommentTag"));
    }

    // src/platform/icls.test.ts › .icls schemes › round trips language effects and exports none without changing
    // shared state
    #[test]
    fn round_trips_language_effects_and_exports_none() {
        let source = SchemeSource::new(TOKENS_CSS);
        let customs = one(
            "custom-effects",
            scheme(
                r#"<scheme name="Effects"><attributes>
      <option name="JAVA_DOC_TAG"><value><option name="FOREGROUND" value="112233"/><option name="EFFECT_COLOR" value="abcdef"/><option name="EFFECT_TYPE" value="2"/></value></option>
    </attributes></scheme>"#,
            ),
        );
        let edits = overrides([
            ("keyword", effect("underline", None)),
            ("number", effect("none", Some("#ff0000"))),
            ("java.string", effect("strikeout", None)),
        ]);
        let xml = export_icls(&source, "custom-effects", &customs, &edits);
        let document = parse_xml(&xml).unwrap();
        let number = find_option(&document, "DEFAULT_NUMBER");
        assert_eq!(number.and_then(|number| find_option(number, "EFFECT_TYPE")), None);
        let imported = scheme(&xml);
        assert_matches(imported.attributes.get("java.docCommentTag"), &effect("wave", Some("#abcdef")));
        assert_matches(imported.attributes.get("java.string"), &effect("strikeout", None));
        assert_eq!(imported.attributes.get("number").and_then(|a| a.effect.as_deref()), None);
        let customs = one("custom-effects", imported.clone());
        let round_trip = attributes_of(&export_icls(&source, "custom-effects", &customs, &BTreeMap::new()));
        assert_eq!(round_trip.get("java.docCommentTag"), imported.attributes.get("java.docCommentTag"));
        assert_eq!(round_trip.get("java.string"), imported.attributes.get("java.string"));
    }

    // src/platform/icls.test.ts › .icls schemes › imports PyCharm dot keys after shared defaults without inherited
    // font effects
    #[test]
    fn imports_pycharm_dot_keys_after_shared_defaults_without_inherited_font_effects() {
        let source = SchemeSource::new(TOKENS_CSS);
        let xml = r#"<scheme name="Python"><attributes>
      <option name="PY.KEYWORD"><value><option name="FOREGROUND" value="112233"/></value></option>
      <option name="DEFAULT_KEYWORD"><value><option name="FOREGROUND" value="ff0000"/><option name="FONT_TYPE" value="1"/><option name="EFFECT_TYPE" value="1"/></value></option>
      <option name="JAVA_KEYWORD"><value><option name="FOREGROUND" value="00ff00"/></value></option>
      <option name="PY.NESTED_FUNC_DEFINITION"><value><option name="FOREGROUND" value="223344"/></value></option>
      <option name="PY.DOC_COMMENT"><value><option name="FOREGROUND" value="334455"/><option name="EFFECT_TYPE" value="1"/></value></option>
      <option name="PY.STRING.B"><value><option name="FOREGROUND" value="445566"/><option name="FONT_TYPE" value="1"/></value></option>
      <option name="PY.BUILTIN_NAME"><value><option name="FOREGROUND" value="abcd"/></value></option>
    </attributes></scheme>"#;
        let attributes = attributes_of(xml);
        let keyword = TextAttributesOverride { effect: text("none"), ..styled("#112233", false, false) };
        assert_eq!(attributes.get("keyword"), Some(&keyword));
        assert_eq!(foreground_of(&attributes, "pyNestedFunction"), Some("#223344"));
        assert_eq!(foreground_of(&attributes, "pyDocstring"), Some("#334455"));
        assert_eq!(attributes.get("pyDocstring").and_then(|a| a.effect.as_deref()), Some("none"));
        assert_eq!(foreground_of(&attributes, "pyBytes"), Some("#445566"));
        assert_eq!(attributes.get("pyBytes").and_then(|a| a.bold), Some(true));
        assert_eq!(foreground_of(&attributes, "pyBuiltin"), Some("#00abcd"));
        let imported = import_scheme(&BTreeMap::new(), scheme(xml), false, "python");
        let exported = export_icls(&source, &imported.id, &imported.customs, &BTreeMap::new());
        let round_trip = attributes_of(&exported);
        assert_eq!(foreground_of(&round_trip, "pyNestedFunction"), Some("#223344"));
        assert_eq!(foreground_of(&round_trip, "pyBytes"), Some("#445566"));
    }

    // src/platform/icls.test.ts › .icls schemes › round trips the Python identifier attributes independently
    #[test]
    fn round_trips_the_python_identifier_attributes_independently() {
        let source = SchemeSource::new(TOKENS_CSS);
        let names = [
            ("pyLocalVariable", "101010"),
            ("pyGlobalVariable", "202020"),
            ("pyInstanceAttribute", "303030"),
            ("pyClassAttribute", "404040"),
            ("pyConstant", "505050"),
            ("pyProperty", "606060"),
            ("pyTypeParameter", "707070"),
        ];
        let entry = |(id, value): &(&str, &str)| {
            format!(
                r#"<option name="OPENIDEA_{id}"><value><option name="FOREGROUND" value="{value}"/></value></option>"#
            )
        };
        let options: String = names.iter().map(entry).collect();
        let xml = format!(r#"<scheme name="Python identifiers"><attributes>{options}</attributes></scheme>"#);
        let ImportedScheme { id, customs } = import_scheme(&BTreeMap::new(), scheme(&xml), false, "python");
        let attributes = attributes_of(&export_icls(&source, &id, &customs, &BTreeMap::new()));
        for (name, value) in names {
            assert_eq!(foreground_of(&attributes, name), Some(format!("#{value}").as_str()), "{name}");
        }
    }

    // src/platform/icls.test.ts › .icls schemes › imports the Python f-string colour
    #[test]
    fn imports_the_python_f_string_colour() {
        let source = SchemeSource::new(TOKENS_CSS);
        let xml = concat!(
            r#"<scheme name="F"><attributes><option name="PY.STRING.F"><value>"#,
            r#"<option name="FOREGROUND" value="808080"/></value></option></attributes></scheme>"#,
        );
        let ImportedScheme { id, customs } = import_scheme(&BTreeMap::new(), scheme(xml), false, "f");
        let attributes = attributes_of(&export_icls(&source, &id, &customs, &BTreeMap::new()));
        assert_eq!(foreground_of(&attributes, "pyFString"), Some("#808080"));
    }

    // src/platform/icls.test.ts › .icls schemes › imports the global variable colour
    #[test]
    fn imports_the_global_variable_colour() {
        let xml = concat!(
            r#"<scheme name="G" parent_scheme="Darcula"><attributes><option name="DEFAULT_GLOBAL_VARIABLE"><value>"#,
            r#"<option name="FOREGROUND" value="9876aa"/><option name="FONT_TYPE" value="2"/></value></option>"#,
            "</attributes></scheme>",
        );
        assert_eq!(attributes_of(xml).get("globalVariable"), Some(&styled("#9876aa", false, true)));
    }

    // src/platform/icls.test.ts › .icls schemes › maps parent %s and background %s to %s
    #[test]
    fn maps_parent_scheme_and_background_to_light_or_dark() {
        let cases = [
            ("Darcula", "ffffff", "light"),
            ("Default", "000000", "dark"),
            ("Other", "101820", "dark"),
            ("Other", "fefefe", "light"),
            ("", "0", "dark"),
        ];
        for (parent, bg, expected) in cases {
            let xml = format!(
                r#"<scheme name="Test" parent_scheme="{parent}"><attributes><option name="TEXT"><value><option name="BACKGROUND" value="{bg}"/></value></option></attributes></scheme>"#
            );
            assert_eq!(scheme(&xml).parent, expected, "{parent} {bg}");
        }
    }

    // src/platform/icls.test.ts › .icls schemes › ignores CSS injection on import and from edited settings
    #[test]
    fn ignores_css_injection_on_import_and_from_edited_settings() {
        let source = SchemeSource::new(TOKENS_CSS);
        let ParsedIcls { scheme, applied, skipped } = parse_icls(
            r#"<scheme name="evil"><colors><option name="CARET_COLOR" value="fff;--bad:red"/></colors><attributes><option name="TEXT"><value><option name="BACKGROUND" value="fff}body{display:none"/></value></option></attributes></scheme>"#,
        )
        .unwrap();
        assert_eq!((applied, skipped), (0, 2));
        assert_eq!(scheme.attributes, BTreeMap::new());
        let customs = json!({
            "custom-test": {
                "name": "Test",
                "parent": "dark",
                "attributes": {
                    "keyword": {
                        "foreground": "red",
                        "background": "url(https://evil)",
                        "effectColor": "var(--bad)",
                        "effect": "wave"
                    }
                }
            }
        });
        let valid = custom_schemes(&customs);
        let keyword = valid.get("custom-test").and_then(|scheme| scheme.attributes.get("keyword"));
        assert_eq!(keyword, Some(&effect("wave", None)));
        let css = scheme_css(&source, Some("custom-test"), None, None, &valid);
        assert!(!["evil", "url(", "--bad"].iter().any(|bad| css.contains(*bad)), "{css}");
        let unsafe_edit = TextAttributesOverride { foreground: text("url(https://evil)"), ..Default::default() };
        let edits = BTreeMap::from([("dark".to_owned(), overrides([("keyword", unsafe_edit)]))]);
        assert!(!scheme_css(&source, Some("dark"), Some(&edits), None, &BTreeMap::new()).contains("evil"));
    }

    // src/platform/icls.test.ts › .icls schemes › keeps both, overwrites a custom, and never overwrites a built-in
    #[test]
    fn keeps_both_overwrites_a_custom_and_never_overwrites_a_builtin() {
        let parsed = scheme(FOREST);
        let name_of = |imported: &ImportedScheme| imported.customs.get(&imported.id).map(|scheme| scheme.name.clone());
        let first = import_scheme(&BTreeMap::new(), parsed.clone(), false, "first");
        let both = import_scheme(&first.customs, parsed.clone(), false, "second");
        assert_eq!(name_of(&both).as_deref(), Some("Forest & Moss (2)"));
        let third = import_scheme(&both.customs, parsed.clone(), false, "third");
        assert_eq!(name_of(&third).as_deref(), Some("Forest & Moss (3)"));
        let light = CustomColorScheme { parent: "light".to_owned(), ..parsed.clone() };
        let overwrite = import_scheme(&both.customs, light, true, "fourth");
        assert_eq!(overwrite.id, first.id);
        assert_eq!(overwrite.customs.get(&first.id).map(|scheme| scheme.parent.as_str()), Some("light"));
        let builtin_name = schemes()[0].name.clone();
        let builtin =
            import_scheme(&BTreeMap::new(), CustomColorScheme { name: builtin_name.clone(), ..parsed }, true, "fifth");
        assert_eq!(name_of(&builtin), Some(format!("{builtin_name} (2)")));
        assert!(!schemes().iter().any(|scheme| scheme.id == builtin.id));
    }

    // src/platform/icls.test.ts › .icls schemes › uses imported values under either theme, overrides them, and
    // falls back after deletion
    #[test]
    fn uses_imported_values_under_either_theme_overrides_them_and_falls_back_after_deletion() {
        let source = SchemeSource::new(TOKENS_CSS);
        let ImportedScheme { id, customs } = forest();
        let keyword = TextAttributesOverride { foreground: text("#abcdef"), ..Default::default() };
        let edits = BTreeMap::from([(id.clone(), overrides([("keyword", keyword)]))]);
        for theme in ["light", "dark"] {
            let root = ThemeAttributes::default().with("data-theme", theme);
            let css = scheme_css(&source, Some(id.as_str()), Some(&edits), Some(&root), &customs);
            assert!(css.contains("--bg-editor: #18281e !important"));
            assert!(css.find("--syn-keyword: #abcdef") > css.find("--syn-keyword: #aadd88"));
            assert!(css.contains(r#".cm-editor[data-language="Java"]"#));
            assert_eq!(effective_scheme(Some("theme-default"), Some(&root), &customs), theme);
            let deleted = delete_scheme(&customs, &edits, &id, &id);
            let nothing = DeletedScheme {
                customs: BTreeMap::new(),
                overrides: BTreeMap::new(),
                selected: "theme-default".to_owned(),
            };
            assert_eq!(deleted, nothing);
            assert_eq!(effective_scheme(Some(id.as_str()), Some(&root), &deleted.customs), theme);
            let (selected, edits_left) = (deleted.selected.as_str(), &deleted.overrides);
            assert_eq!(scheme_css(&source, Some(selected), Some(edits_left), Some(&root), &deleted.customs), "");
            assert_eq!(delete_scheme(&customs, &edits, "light", &id).selected, "light");
        }
    }

    // src/platform/icls.test.ts › .icls schemes › exports visible values and re-imports %s with overrides
    #[test]
    fn exports_visible_values_and_re_imports_them_with_overrides() {
        let source = SchemeSource::new(TOKENS_CSS);
        for fixture in [FOREST, DAYLIGHT] {
            let ImportedScheme { id, customs } = import_scheme(&BTreeMap::new(), scheme(fixture), false, "fixture");
            let keyword = styled("#123456", false, true);
            let error = TextAttributesOverride { effect_color: text("#aa1122"), ..Default::default() };
            let edits = overrides([("keyword", keyword.clone()), ("error", error)]);
            let imported = scheme(&export_icls(&source, &id, &customs, &edits));
            assert_eq!(imported.name, customs[&id].name);
            assert_eq!(imported.parent, customs[&id].parent);
            assert_eq!(imported.attributes.get("keyword"), Some(&keyword));
            assert_eq!(imported.attributes.get("text"), customs[&id].attributes.get("text"));
            let effect_color = imported.attributes.get("error").and_then(|a| a.effect_color.as_deref());
            assert_eq!(effect_color, Some("#aa1122"));
        }
    }

    // src/platform/icls.test.ts › .icls schemes › exports shorthand colors as full RGB
    #[test]
    fn exports_shorthand_colors_as_full_rgb() {
        let source = SchemeSource::new(TOKENS_CSS);
        let ImportedScheme { id, customs } = forest();
        let keyword = TextAttributesOverride { foreground: text("#abc"), ..Default::default() };
        let exported = export_icls(&source, &id, &customs, &overrides([("keyword", keyword)]));
        assert_eq!(foreground_of(&attributes_of(&exported), "keyword"), Some("#aabbcc"));
    }

    // src/platform/icls.test.ts › .icls schemes › round trips custom settings and lists them without changing
    // built-ins
    #[test]
    fn round_trips_custom_settings_and_lists_them_without_changing_builtins() {
        let ImportedScheme { customs, .. } = forest();
        let stored = serde_json::to_string(&customs).unwrap();
        assert_eq!(custom_schemes(&serde_json::from_str::<Value>(&stored).unwrap()), customs);
        let listed = list_schemes(&customs);
        assert_eq!(listed.last().map(|scheme| scheme.name.as_str()), Some("Forest & Moss"));
        assert_eq!(listed.len(), schemes().len() + 1);
    }

    /// Not from a spec: the XML subset scheme files are read with.
    #[test]
    fn parses_the_xml_subset_of_scheme_files() {
        let document = parse_xml(concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- a scheme -->\n",
            "<scheme name='A &amp; &#66;&#x43; &lt;&gt;&quot;&apos;' note=\"tab\there\">\n",
            "  <meta>text &amp; <![CDATA[<raw>]]></meta><option name=\"X\" value=\"1\" />\n</scheme>\n<!-- end -->\n",
        ))
        .unwrap();
        assert_eq!(document.attribute("name"), Some("A & BC <>\"'"));
        assert_eq!(document.attribute("note"), Some("tab here"));
        let children: Vec<&str> = document.children.iter().map(|child| child.name.as_str()).collect();
        assert_eq!(children, ["meta", "option"]);
        assert_eq!(document.children[1].attribute("value"), Some("1"));
        for malformed in [
            "<a><b></a></b>",
            "<a/><b/>",
            "<a b=\"1\" b=\"2\"/>",
            "<a b=\"1\"c=\"2\"/>",
            "<a b=1/>",
            "<a b=\"<\"/>",
            "<a>&unknown;</a>",
            "<a b=\"&#0;\"/>",
            "<a>&#x+41;</a>",
            "text<a/>",
            "<a/>text",
            "<a><!-- open</a>",
        ] {
            assert_eq!(parse_xml(malformed), None, "{malformed}");
        }
    }
}
