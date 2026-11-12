//! Path and symbol naming shared by the search result models.

use ide_core::SymbolKind;

/// Directory part of a project-relative path ("" for the root).
pub fn rel_dir(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// The name a symbol is listed under: callables get `()`.
pub fn symbol_display_name(name: &str, kind: SymbolKind) -> String {
    let callable = matches!(kind, SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor);
    if callable && !name.ends_with(')') { format!("{name}()") } else { name.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: the directory of a relative path, and which symbol kinds are shown as calls.
    #[test]
    fn names_directories_and_callables() {
        assert_eq!(rel_dir("src/main/App.kt"), "src/main");
        assert_eq!(rel_dir("App.kt"), "");
        assert_eq!(symbol_display_name("run", SymbolKind::Method), "run()");
        assert_eq!(symbol_display_name("run()", SymbolKind::Function), "run()");
        assert_eq!(symbol_display_name("Order", SymbolKind::Class), "Order");
    }
}
