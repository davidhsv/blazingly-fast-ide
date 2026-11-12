//! Semantic highlighting: the server's semantic tokens as styled ranges (`sem-*` classes over the
//! syntax highlighting), and semantic highlighting proper (IDEA's rainbow identifiers): locals and
//! parameters get a colour derived from their name, so one name keeps one colour in the file.
//!
//! Languages add their own classes to the generic ones (Python's static and built-in names,
//! Kotlin's mutable declarations, Go's constants and globals).

use crate::editor::buffer;
use ide_lsp::types::SemanticTokens;
use ropey::Rope;

/// One styled range (UTF-16 offsets); `class_name` is space-separated `cm-sem-*` classes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticRange {
    pub from: u32,
    pub to: u32,
    pub class_name: String,
}

/// Number of rainbow colours (IDEA's five semantic highlighting colours).
pub const RAINBOW_COLORS: u32 = 5;

/// The stable 1-based palette index of an identifier (FNV-1a over its UTF-16 units).
pub fn rainbow_index(name: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for unit in name.encode_utf16() {
        hash ^= u32::from(unit);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash % RAINBOW_COLORS + 1
}

fn type_class(token_type: &str) -> Option<&'static str> {
    Some(match token_type {
        "type" | "class" | "enum" | "interface" | "struct" | "typeAlias" | "builtinType" | "record" => "type",
        "selfTypeKeyword" => "keyword",
        "typeParameter" => "typeParameter",
        "function" | "method" | "annotationMember" => "function",
        "macro" => "macro",
        "decorator" | "annotation" | "attribute" | "lifetime" => "annotation",
        "recordComponent" | "property" => "property",
        "enumMember" => "constant",
        "parameter" => "parameter",
        "variable" => "variable",
        "namespace" => "namespace",
        "selfParameter" | "clsParameter" | "selfKeyword" => "self",
        _ => return None,
    })
}

fn modifier_class(modifier: &str) -> Option<&'static str> {
    Some(match modifier {
        "deprecated" => "deprecated",
        "static" => "static",
        "mutable" => "mutable",
        "declaration" | "definition" => "declaration",
        "defaultLibrary" => "builtin",
        "global" => "global",
        "selfParameter" => "self",
        "byteString" => "python-bytes",
        "escapeSequence" => "python-escape",
        "formatSpecifier" | "formatPlaceholder" => "python-format",
        _ => return None,
    })
}

/// Modifiers that make a variable anything but a local.
const NOT_LOCAL: [&str; 6] = ["static", "constant", "defaultLibrary", "global", "classScope", "fileScope"];

/// What a language adds to a server token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    Python,
    Kotlin,
    Go,
    Other,
}

impl Language {
    fn of(id: Option<&str>) -> Self {
        match id.map(str::to_ascii_lowercase).as_deref() {
            Some("python") => Self::Python,
            Some("kotlin") => Self::Kotlin,
            Some("go") => Self::Go,
            _ => Self::Other,
        }
    }

    fn token_classes(self, token_type: &str, modifiers: &[&str], classes: &mut Vec<String>) {
        let has = |name: &str| modifiers.contains(&name);
        let push = |classes: &mut Vec<String>, class: &str| classes.push(class.to_owned());
        match self {
            Self::Python => {
                if has("static") {
                    push(classes, "cm-sem-python-static");
                }
                if token_type == "method" {
                    push(classes, "cm-sem-python-method-call");
                }
                if has("defaultLibrary") {
                    push(classes, "cm-sem-python-builtin");
                }
            }
            Self::Kotlin => {
                if token_type == "operator" && has("declaration") {
                    push(classes, "cm-sem-function");
                }
                // The JetBrains Kotlin server says `modification` for mutable declarations and references.
                if matches!(token_type, "variable" | "property" | "parameter") && has("modification") {
                    push(classes, "cm-sem-mutable");
                }
                if token_type == "label" {
                    push(classes, "cm-sem-constant");
                }
            }
            Self::Go => {
                let constant = classes.iter().any(|class| class == "cm-sem-constant");
                if token_type == "variable" && !constant {
                    if has("readonly") {
                        push(classes, "cm-sem-constant");
                    } else if has("static") {
                        push(classes, "cm-sem-global");
                    }
                }
            }
            Self::Other => {}
        }
    }

    /// Whether a token counts as a local for rainbow colours; `None`: the generic rule.
    fn is_local(self, token_type: &str, modifiers: &[&str]) -> Option<bool> {
        match self {
            Self::Python => Some(token_type == "parameter"),
            Self::Go => {
                Some(token_type == "parameter" || (token_type == "variable" && !modifiers.contains(&"readonly")))
            }
            Self::Kotlin | Self::Other => None,
        }
    }
}

/// The styled ranges of absolute `[line, col, len, type, modifiers]` tokens. Plain variables and
/// unknown types get no class; with `rainbow`, locals and parameters also get
/// `cm-sem-rainbow-<n>`.
pub fn semantic_ranges(
    doc: &Rope,
    tokens: &SemanticTokens,
    rainbow: bool,
    language: Option<&str>,
) -> Vec<SemanticRange> {
    let language = Language::of(language);
    // Servers with a `local` modifier tell module-level variables apart.
    let marks_locals = tokens.token_modifiers.iter().any(|modifier| modifier == "local");
    let line_count = buffer::line_count(doc);
    let mut out = Vec::new();
    for &[line, col, len, type_index, bits] in tokens.data.as_chunks::<5>().0 {
        if line >= line_count || len == 0 {
            continue;
        }
        let range = buffer::line_range(doc, line);
        let from = range.start + col.min(range.end - range.start);
        let to = from.saturating_add(len).min(range.end);
        if from >= to {
            continue;
        }
        let token_type = tokens.token_types.get(type_index as usize).map_or("", String::as_str);
        let modifiers: Vec<&str> = tokens
            .token_modifiers
            .iter()
            .enumerate()
            .filter(|(bit, _)| *bit < 32 && bits & (1u32 << *bit) != 0)
            .map(|(_, name)| name.as_str())
            .collect();
        let has = |name: &str| modifiers.contains(&name);
        let mut base = type_class(token_type);
        // A constructor comes as a class with a `constructor` modifier; IDEA colours it as a method.
        if has("constructor") {
            base = Some("function");
        }
        let constant = has("constant") || (token_type == "variable" && has("readonly") && has("static"));
        if constant && matches!(token_type, "variable" | "property") {
            base = Some("constant");
        }
        let mut classes: Vec<String> = Vec::new();
        if let Some(base) = base.filter(|base| *base != "variable") {
            classes.push(format!("cm-sem-{base}"));
        }
        classes.extend(modifiers.iter().filter_map(|name| modifier_class(name)).map(|class| format!("cm-sem-{class}")));
        language.token_classes(token_type, &modifiers, &mut classes);
        let local = rainbow
            && !constant
            && !has("selfParameter")
            && !modifiers.iter().any(|name| NOT_LOCAL.contains(name))
            && language.is_local(token_type, &modifiers).unwrap_or_else(|| {
                matches!(token_type, "variable" | "parameter")
                    && (token_type == "parameter" || !marks_locals || has("local"))
            });
        if classes.is_empty() && !local {
            continue;
        }
        if local {
            classes.push(format!("cm-sem-rainbow-{}", rainbow_index(&buffer::slice(doc, from, to))));
        }
        out.push(SemanticRange { from, to, class_name: classes.join(" ") });
    }
    out.sort_by_key(|range| (range.from, range.to));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn tokens(types: &[&str], modifiers: &[&str], data: &[u32]) -> SemanticTokens {
        SemanticTokens {
            data: data.to_vec(),
            token_types: types.iter().map(|t| (*t).to_owned()).collect(),
            token_modifiers: modifiers.iter().map(|m| (*m).to_owned()).collect(),
        }
    }

    fn range(from: u32, to: u32, class_name: &str) -> SemanticRange {
        SemanticRange { from, to, class_name: class_name.into() }
    }

    fn doc() -> Rope {
        Rope::from_str("let mut count = Foo::new();\nconst MAX: u32 = 1;")
    }

    fn first_class(input: &SemanticTokens, language: &str) -> String {
        semantic_ranges(&doc(), input, false, Some(language)).first().map(|r| r.class_name.clone()).unwrap_or_default()
    }

    #[test]
    fn maps_types_and_modifiers_to_classes_and_skips_plain_variables_and_unknown_types() {
        let input = tokens(
            &["variable", "struct", "function", "keyword"],
            &["mutable", "constant", "static", "deprecated"],
            &[
                0, 8, 5, 0, 0b0001, 0, 16, 3, 1, 0, 0, 21, 3, 2, 0b1000, 1, 6, 3, 0, 0b0010, 0, 0, 3, 3, 0, 9, 0, 1, 1,
                0,
            ],
        );
        assert_eq!(
            semantic_ranges(&doc(), &input, false, None),
            [
                range(8, 13, "cm-sem-mutable"),
                range(16, 19, "cm-sem-type"),
                range(21, 24, "cm-sem-function cm-sem-deprecated"),
                range(34, 37, "cm-sem-constant"),
            ]
        );
    }

    #[test]
    fn applies_the_python_specific_static_style_without_changing_other_languages() {
        let input = tokens(&["property"], &["static"], &[0, 0, 5, 0, 1]);
        assert_eq!(first_class(&input, "python"), "cm-sem-property cm-sem-static cm-sem-python-static");
        assert_eq!(first_class(&input, "java"), "cm-sem-property cm-sem-static");
    }

    #[test]
    fn routes_python_library_symbols_through_its_built_in_name_colour() {
        let input = tokens(&["function"], &["defaultLibrary"], &[0, 0, 3, 0, 1]);
        assert_eq!(first_class(&input, "python"), "cm-sem-function cm-sem-builtin cm-sem-python-builtin");
        assert_eq!(first_class(&input, "java"), "cm-sem-function cm-sem-builtin");
    }

    #[test]
    fn ignores_type_and_modifier_names_that_only_exist_on_object_prototype() {
        let input = tokens(&["method", "toString"], &["valueOf"], &[0, 0, 3, 0, 1, 0, 4, 3, 1, 1]);
        assert_eq!(semantic_ranges(&doc(), &input, false, None), [range(0, 3, "cm-sem-function")]);
    }

    #[test]
    fn marks_globals() {
        let input = tokens(&["variable", "function"], &["global"], &[0, 0, 3, 0, 1, 0, 4, 3, 1, 1]);
        assert_eq!(
            semantic_ranges(&doc(), &input, false, None),
            [range(0, 3, "cm-sem-global"), range(4, 7, "cm-sem-function cm-sem-global")]
        );
    }

    #[test]
    fn colours_a_constructor_class_and_constructor_as_a_function() {
        let input = tokens(&["class"], &["constructor", "declaration"], &[0, 0, 3, 0, 0b11]);
        assert_eq!(first_class(&input, "java"), "cm-sem-function cm-sem-declaration");
    }

    #[test]
    fn hashes_names_to_a_stable_palette_index() {
        assert_eq!(rainbow_index("count"), rainbow_index("count"));
        let all: Vec<u32> = ["a", "b", "count", "total", "index", "value", "name", "item", "result", "i", "j", "k"]
            .into_iter()
            .map(rainbow_index)
            .collect();
        assert!(all.iter().all(|n| (1..=RAINBOW_COLORS).contains(n)));
        assert!(all.iter().collect::<HashSet<_>>().len() > 2);
    }

    fn rainbow_of(range: &SemanticRange) -> Option<u32> {
        range.class_name.split(' ').find_map(|class| class.strip_prefix("cm-sem-rainbow-")?.parse().ok())
    }

    #[test]
    fn colours_locals_and_parameters_by_name_only_when_enabled() {
        let doc = Rope::from_str("fn f(total, self) { let count = total; S; obj.field; count + total }");
        let data: Vec<u32> = [
            [0, 5, 5, 1, 0b10],
            [0, 12, 4, 3, 0],
            [0, 24, 5, 0, 0b10],
            [0, 32, 5, 1, 0],
            [0, 39, 1, 0, 0b01],
            [0, 42, 3, 0, 0],
            [0, 46, 5, 2, 0],
            [0, 53, 5, 0, 0],
            [0, 61, 5, 1, 0],
        ]
        .concat();
        let input = tokens(&["variable", "parameter", "property", "selfParameter"], &["static", "declaration"], &data);
        assert!(semantic_ranges(&doc, &input, false, None).iter().all(|r| rainbow_of(r).is_none()));
        let ranges = semantic_ranges(&doc, &input, true, None);
        let mut by_text: HashMap<String, HashSet<Option<u32>>> = HashMap::new();
        for r in &ranges {
            by_text.entry(buffer::slice(&doc, r.from, r.to)).or_default().insert(rainbow_of(r));
        }
        for name in ["total", "count", "obj"] {
            assert_eq!(by_text[name], HashSet::from([Some(rainbow_index(name))]), "{name}");
        }
        // Parameters keep their own class too; self, statics and fields get no rainbow colour.
        let total = ranges.iter().find(|r| r.from == 5).map(|r| r.class_name.clone());
        assert_eq!(
            total,
            Some(format!("cm-sem-parameter cm-sem-declaration cm-sem-rainbow-{}", rainbow_index("total")))
        );
        for name in ["self", "S", "field"] {
            assert_eq!(by_text[name], HashSet::from([None]), "{name}");
        }
    }

    #[test]
    fn leaves_module_level_variables_plain_when_the_server_marks_locals() {
        let doc = Rope::from_str("let top = 1; function f(p) { let x = top + p; }");
        let input = tokens(
            &["variable", "parameter"],
            &["declaration", "local"],
            &[0, 4, 3, 0, 0b01, 0, 24, 1, 1, 0b11, 0, 33, 1, 0, 0b11, 0, 37, 3, 0, 0, 0, 43, 1, 1, 0b10],
        );
        let rainbow: Vec<String> = semantic_ranges(&doc, &input, true, None)
            .iter()
            .filter(|r| r.class_name.contains("rainbow"))
            .map(|r| buffer::slice(&doc, r.from, r.to))
            .collect();
        assert_eq!(rainbow, ["p", "x", "p"]);
    }
}
