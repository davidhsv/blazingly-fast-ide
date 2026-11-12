//! Which code actions Generate (⌘N), Implement Methods (⌃I) and Override Methods (⌃O) offer.

use ide_lsp::types::CodeAction;
use regex::Regex;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

/// Whether Generate lists the action. All actions are asked for: some servers offer
/// `source.addTest` without advertising that kind.
pub fn is_generate_action(action: &CodeAction) -> bool {
    static TITLE: LazyLock<Regex> = LazyLock::new(|| {
        pattern(
            r"(?i)^(generate|implement|override|declare missing methods\b|add (test for\b|methods to .+ implement\b|missing|derive|impl|getter|setter|constructor)|create (constructor|getter|setter))",
        )
    });
    action.kind.as_deref() == Some("source.addTest") || TITLE.is_match(&action.title)
}

/// The titles of Implement Methods actions.
pub fn is_implement_title(title: &str) -> bool {
    static TITLE: LazyLock<Regex> = LazyLock::new(|| {
        pattern(
            r"(?i)\bimplement\b|\b(add|generate) (missing|unimplemented|all missing)\b|missing (members|methods|trait items)",
        )
    });
    TITLE.is_match(title)
}

/// The titles of Override Methods actions.
pub fn is_override_title(title: &str) -> bool {
    static TITLE: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)\boverride\b"));
    TITLE.is_match(title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::ServerId;

    fn action(title: &str, kind: Option<&str>) -> CodeAction {
        CodeAction {
            title: title.into(),
            kind: kind.map(str::to_owned),
            is_preferred: false,
            disabled_reason: None,
            edit: None,
            command: None,
            data: None,
            server: ServerId(1),
        }
    }

    #[test]
    fn includes() {
        for title in [
            "Add test for Add",
            "Declare missing methods of I",
            "Add methods to T to implement an interface...",
            "Generate toString()",
            "Generate constructor",
            "Implement interface 'I'",
            "Override methods",
            "Add missing members",
            "Add derive",
            "Add impl",
            "Add getter",
            "Create constructor",
        ] {
            assert!(is_generate_action(&action(title, None)), "{title}");
        }
    }

    #[test]
    fn accepts_source_add_test_independently_of_its_title_and_advertised_kinds() {
        assert!(is_generate_action(&action("Localized test action", Some("source.addTest"))));
    }

    #[test]
    fn excludes() {
        for title in [
            "Browse documentation for type T",
            "Extract declarations to new file",
            "Add import",
            "Add test framework",
            "Add methods to documentation",
            "Declare variable",
            "Split package",
        ] {
            assert!(!is_generate_action(&action(title, None)), "{title}");
        }
    }

    #[test]
    fn implement_and_override_titles() {
        for (title, implement, override_) in [
            ("Implement missing members", true, false),
            ("Implement interface 'Runnable'", true, false),
            ("Add missing trait items", true, false),
            ("Override/Implement Methods...", true, true),
            ("Override toString()", false, true),
            ("Extract method", false, false),
            ("Add import", false, false),
        ] {
            assert_eq!((is_implement_title(title), is_override_title(title)), (implement, override_), "{title}");
        }
    }
}
