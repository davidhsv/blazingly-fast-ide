//! The scope editor's checks: what keeps a list of custom scopes from being saved, and the name a new scope
//! starts with.

use crate::platform::i18n::I18n;
use crate::platform::search::CustomScope;
use std::collections::HashSet;

/// The problem that keeps the list from being saved, or `None`.
pub fn scopes_problem(scopes: &[CustomScope]) -> Option<String> {
    let mut names = HashSet::new();
    for scope in scopes {
        let name = scope.name.trim();
        if name.is_empty() {
            return Some("Every scope needs a name".to_owned());
        }
        if !names.insert(name) {
            return Some(format!("Duplicate scope name '{name}'"));
        }
        if scope.pattern.trim().is_empty() {
            return Some(format!("Scope '{name}' has no pattern"));
        }
    }
    None
}

/// The first of "Unnamed", "Unnamed (2)", … that no scope in the list has.
pub fn new_scope_name(scopes: &[CustomScope], i18n: &I18n) -> String {
    let mut number = 1u32;
    loop {
        let name = if number == 1 { i18n.tx("Unnamed") } else { i18n.tx_template(&format!("Unnamed ({number})")) };
        if !scopes.iter().any(|scope| scope.name == name) {
            return name;
        }
        number += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(name: &str, pattern: &str) -> CustomScope {
        CustomScope { name: name.to_owned(), pattern: pattern.to_owned() }
    }

    // src/features/search/__tests__/findScopes.test.ts › Find in Files module and custom scopes › the scope editor
    // needs unique names and a pattern
    #[test]
    fn the_scope_editor_needs_unique_names_and_a_pattern() {
        assert_eq!(scopes_problem(&[scope("a", "*.ts")]), None);
        assert_eq!(scopes_problem(&[scope(" ", "*.ts")]).as_deref(), Some("Every scope needs a name"));
        assert_eq!(
            scopes_problem(&[scope("a", "*.ts"), scope("a ", "*.rs")]).as_deref(),
            Some("Duplicate scope name 'a'")
        );
        assert_eq!(scopes_problem(&[scope("a", "")]).as_deref(), Some("Scope 'a' has no pattern"));
    }

    // Not from a spec: a new scope takes the first unused default name.
    #[test]
    fn a_new_scope_takes_the_first_free_name() {
        let i18n = I18n::default();
        assert_eq!(new_scope_name(&[], &i18n), "Unnamed");
        assert_eq!(new_scope_name(&[scope("Unnamed", "x")], &i18n), "Unnamed (2)");
        assert_eq!(new_scope_name(&[scope("Unnamed", "x"), scope("Unnamed (2)", "y")], &i18n), "Unnamed (3)");
    }
}
