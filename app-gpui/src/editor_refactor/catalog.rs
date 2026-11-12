//! IDEA's refactorings and how language-server code actions map onto them: `refactor.extract.*`
//! is Extract Method / Introduce Variable…, `refactor.inline*` Inline, `refactor.move*` Move,
//! signature rewrites Change Signature. Pull Members Up / Push Members Down come from the Java
//! extension, not from code actions.

use regex::Regex;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefactoringId {
    Rename,
    ChangeSignature,
    Move,
    SafeDelete,
    ExtractMethod,
    ExtractVariable,
    ExtractConstant,
    ExtractField,
    ExtractParameter,
    Inline,
    PullMembersUp,
    PushMembersDown,
}

/// One refactoring of the Refactor This list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refactoring {
    pub id: RefactoringId,
    /// Action id (keymaps, menu).
    pub action: &'static str,
    pub label: &'static str,
    /// The code action kind that can produce it; `None` for refactorings done without one.
    pub kind: Option<&'static str>,
    /// IDEA's reason when nothing applies at the caret, then with a selection.
    reasons: (&'static str, &'static str),
}

impl Refactoring {
    /// IDEA's reason when nothing applies at the caret (`selection`: or in the selection).
    pub fn reason(&self, selection: bool) -> &'static str {
        if selection { self.reasons.1 } else { self.reasons.0 }
    }
}

const AT_NAME: &str = "Caret should be positioned at the name of the element to be refactored";

const fn same(reason: &'static str) -> (&'static str, &'static str) {
    (reason, reason)
}

/// IDEA's Refactor This order.
pub const REFACTORINGS: [Refactoring; 12] = [
    Refactoring {
        id: RefactoringId::Rename,
        action: "refactor.rename",
        label: "Rename…",
        kind: None,
        reasons: same("Caret should be on a symbol"),
    },
    Refactoring {
        id: RefactoringId::ChangeSignature,
        action: "refactor.changeSignature",
        label: "Change Signature…",
        kind: Some("refactor.rewrite"),
        reasons: same("Caret should be positioned at the name of the function or method to be refactored"),
    },
    Refactoring {
        id: RefactoringId::Move,
        action: "refactor.move",
        label: "Move…",
        kind: Some("refactor.move"),
        reasons: same(AT_NAME),
    },
    Refactoring {
        id: RefactoringId::SafeDelete,
        action: "refactor.safeDelete",
        label: "Safe Delete…",
        kind: None,
        reasons: same("Caret should be on a symbol"),
    },
    Refactoring {
        id: RefactoringId::ExtractMethod,
        action: "refactor.extractMethod",
        label: "Extract Method…",
        kind: Some("refactor.extract"),
        reasons: ("Select the code to extract", "Selected block should represent a set of statements or an expression"),
    },
    Refactoring {
        id: RefactoringId::ExtractVariable,
        action: "refactor.extractVariable",
        label: "Introduce Variable…",
        kind: Some("refactor.extract"),
        reasons: ("Caret should be inside an expression", "Selected block should represent an expression"),
    },
    Refactoring {
        id: RefactoringId::ExtractConstant,
        action: "refactor.extractConstant",
        label: "Introduce Constant…",
        kind: Some("refactor.extract"),
        reasons: ("Caret should be inside an expression", "Selected block should represent an expression"),
    },
    Refactoring {
        id: RefactoringId::ExtractField,
        action: "refactor.extractField",
        label: "Introduce Field…",
        kind: Some("refactor.extract"),
        reasons: ("Caret should be inside an expression in a class", "Selected block should represent an expression"),
    },
    Refactoring {
        id: RefactoringId::ExtractParameter,
        action: "refactor.extractParameter",
        label: "Introduce Parameter…",
        kind: Some("refactor.extract"),
        reasons: (
            "Caret should be inside an expression in a function",
            "Selected block should represent an expression",
        ),
    },
    Refactoring {
        id: RefactoringId::Inline,
        action: "refactor.inline",
        label: "Inline…",
        kind: Some("refactor.inline"),
        reasons: same(AT_NAME),
    },
    Refactoring {
        id: RefactoringId::PullMembersUp,
        action: "refactor.pullMembersUp",
        label: "Pull Members Up…",
        kind: None,
        reasons: same("Caret should be inside a class that has members to pull up"),
    },
    Refactoring {
        id: RefactoringId::PushMembersDown,
        action: "refactor.pushMembersDown",
        label: "Push Members Down…",
        kind: None,
        reasons: same("Caret should be inside a class that has members to push down"),
    },
];

/// Refactorings of the class hierarchy, run by the Java extension.
pub fn is_hierarchy_refactoring(id: RefactoringId) -> bool {
    matches!(id, RefactoringId::PullMembersUp | RefactoringId::PushMembersDown)
}

pub fn refactoring(id: RefactoringId) -> &'static Refactoring {
    REFACTORINGS.iter().find(|r| r.id == id).unwrap_or(&REFACTORINGS[0])
}

/// The refactoring an action id runs.
pub fn refactoring_of_action(action: &str) -> Option<&'static Refactoring> {
    REFACTORINGS.iter().find(|r| r.action == action)
}

/// IDEA's hint text for a refactoring that cannot run (English source text).
pub fn cannot_perform(reason: &str) -> String {
    format!("Cannot perform refactoring. {}", reason.strip_suffix('.').unwrap_or(reason))
}

/// `kind` is `prefix` or one of its sub-kinds.
pub fn kind_is(kind: &str, prefix: &str) -> bool {
    kind == prefix || kind.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('.'))
}

static EXTRACT_TITLE: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^(extract|introduce)\b"));
static EXTRACT_TYPE: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r"(?i)^(extract|introduce)( (in)?to)?( an?)? (type\b|typedef|interface|struct|enum|trait|superclass|module)",
    )
});
static FUNCTION: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)function|method"));
static FIELD: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)field|member|property"));
static PARAMETER: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)parameter|argument"));
static ENCLOSING: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)enclosing scope"));
static CONSTANT: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)constant|static"));
static VARIABLE: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)variable|local|\blet\b"));
static INLINE_TITLE: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^inline\b"));
static MOVE_TITLE: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^move\b"));
static SIGNATURE_KIND: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)signature"));
static SIGNATURE_TITLE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?i)\bchange signature\b|\breorder (parameters|arguments)\b"));

/// The IDEA refactoring a server code action implements, or `None` ("Convert to arrow
/// function"). Some servers report every refactoring as plain `refactor`, so titles decide when
/// the kind is generic.
pub fn classify_action(kind: Option<&str>, title: &str) -> Option<RefactoringId> {
    let kind = kind.unwrap_or_default();
    if !kind_is(kind, "refactor") {
        return None;
    }
    let generic = kind == "refactor" || kind_is(kind, "refactor.rewrite");
    if kind_is(kind, "refactor.extract") || (generic && EXTRACT_TITLE.is_match(title)) {
        let sub = kind.strip_prefix("refactor.extract").unwrap_or_default();
        let is = |re: &Regex| re.is_match(sub) || re.is_match(title);
        if EXTRACT_TYPE.is_match(title) {
            return None;
        }
        if is(&FUNCTION) {
            return Some(RefactoringId::ExtractMethod);
        }
        if is(&FIELD) {
            return Some(RefactoringId::ExtractField);
        }
        if is(&PARAMETER) {
            return Some(RefactoringId::ExtractParameter);
        }
        // "Extract to constant in enclosing scope" declares a local: IDEA's Introduce Variable.
        if ENCLOSING.is_match(title) {
            return Some(RefactoringId::ExtractVariable);
        }
        if is(&CONSTANT) {
            return Some(RefactoringId::ExtractConstant);
        }
        if is(&VARIABLE) {
            return Some(RefactoringId::ExtractVariable);
        }
        return None;
    }
    if kind_is(kind, "refactor.inline") || (generic && INLINE_TITLE.is_match(title)) {
        return Some(RefactoringId::Inline);
    }
    if kind_is(kind, "refactor.move") || (generic && MOVE_TITLE.is_match(title)) {
        return Some(RefactoringId::Move);
    }
    if SIGNATURE_KIND.is_match(kind) || SIGNATURE_TITLE.is_match(title) {
        return Some(RefactoringId::ChangeSignature);
    }
    None
}

/// Whether servers advertising `advertised` (`codeActionProvider.codeActionKinds`) could offer
/// `kind`: a listed kind equal to it, a parent of it or a child of it. Unknown (`None`) means
/// maybe.
pub fn kind_advertised(advertised: Option<&[String]>, kind: &str) -> bool {
    advertised.is_none_or(|kinds| kinds.iter().any(|k| !k.is_empty() && (kind_is(kind, k) || kind_is(k, kind))))
}

/// The display name of a language id when the language list is not at hand: `rust` is `Rust`.
pub fn fallback_language_name(id: &str) -> String {
    let known = match id {
        "typescript" => "TypeScript",
        "javascript" => "JavaScript",
        "typescriptreact" => "TypeScript JSX",
        "javascriptreact" => "JavaScript JSX",
        "cpp" => "C++",
        "csharp" => "C#",
        "php" => "PHP",
        "css" => "CSS",
        "html" => "HTML",
        "json" => "JSON",
        "yaml" => "YAML",
        "toml" => "TOML",
        "sql" => "SQL",
        _ => {
            let mut chars = id.chars();
            return chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default();
        }
    };
    known.to_owned()
}

/// The display name of a language: the backend's language list's, else the fallback.
pub fn language_name(id: &str, languages: &[ide_language::LanguageInfo]) -> String {
    languages
        .iter()
        .find(|language| language.id.as_str() == id)
        .map_or_else(|| fallback_language_name(id), |language| language.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_server_kinds_and_titles_to_idea_refactorings() {
        use RefactoringId::*;
        let cases: [(&str, &str, Option<RefactoringId>); 22] = [
            ("Extract to function in module scope", "refactor.extract.function", Some(ExtractMethod)),
            ("Extract into function", "refactor.extract", Some(ExtractMethod)),
            ("Extract into variable", "refactor.extract", Some(ExtractVariable)),
            ("Extract to constant in enclosing scope", "refactor.extract.constant", Some(ExtractVariable)),
            ("Extract to constant in module scope", "refactor.extract.constant", Some(ExtractConstant)),
            ("Extract field", "refactor.extract.field", Some(ExtractField)),
            ("Introduce parameter", "refactor.extract.parameter", Some(ExtractParameter)),
            ("Inline variable", "refactor.inline", Some(Inline)),
            ("Move to a new file", "refactor.move.newFile", Some(Move)),
            ("Change signature", "refactor.changeSignature", Some(ChangeSignature)),
            ("Convert parameters to destructured object", "refactor.rewrite.parameters.toDestructured", None),
            ("Extract type alias", "refactor.extract.type", None),
            ("Remove unused", "quickfix", None),
            // Some servers report plain `refactor` for everything.
            ("Extract to inner function in function 'formatMoney'", "refactor", Some(ExtractMethod)),
            ("Extract to constant in enclosing scope", "refactor", Some(ExtractVariable)),
            ("Extract to type alias", "refactor", None),
            ("Extract type as type alias", "refactor.extract", None),
            ("Extract struct from enum variant", "refactor.extract", None),
            ("Move to a new file", "refactor", Some(Move)),
            ("Inline variable", "refactor", Some(Inline)),
            ("Convert to template string", "refactor", None),
            ("Remove unused", "", None),
        ];
        for (title, kind, expected) in cases {
            let kind = (!kind.is_empty()).then_some(kind);
            assert_eq!(classify_action(kind, title), expected, "{title} ({kind:?})");
        }
    }

    #[test]
    fn knows_which_kinds_an_advertised_list_can_produce() {
        let list = |kinds: &[&str]| kinds.iter().map(|kind| (*kind).to_owned()).collect::<Vec<_>>();
        assert!(kind_advertised(None, "refactor.move"));
        assert!(!kind_advertised(Some(list(&["quickfix", "refactor.extract"]).as_slice()), "refactor.move"));
        assert!(!kind_advertised(Some(list(&["", "quickfix"]).as_slice()), "refactor.move"));
        assert!(kind_advertised(Some(list(&["refactor"]).as_slice()), "refactor.move"));
        assert!(kind_advertised(Some(list(&["refactor.move.newFile"]).as_slice()), "refactor.move"));
    }

    #[test]
    fn formats_idea_s_hint() {
        assert_eq!(
            cannot_perform("Caret should be on a symbol."),
            "Cannot perform refactoring. Caret should be on a symbol"
        );
    }
}
