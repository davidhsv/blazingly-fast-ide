//! Edit › Copy Reference (⌥⇧⌘C): with the caret on a declaration's name, its fully qualified
//! name, built from the file's outline and the language's module conventions; anywhere else
//! `path:line`, as in IDEA.

use crate::editor::buffer::byte_of_col;
use ide_core::SymbolKind;
use regex::Regex;
use std::sync::LazyLock;

fn compile(pattern: &str) -> Regex {
    Regex::new(pattern).expect("the pattern is valid")
}

static IDENTIFIER: LazyLock<Regex> = LazyLock::new(|| compile(r"^[\p{L}_$][\p{L}\p{N}_$]*$"));
static WORD_CHAR: LazyLock<Regex> = LazyLock::new(|| compile(r"^[\p{L}\p{N}_$]"));
static RUST_IMPL: LazyLock<Regex> = LazyLock::new(|| compile(r"^impl(?:<[^>]*>)?\s+(?:.*?\s+for\s+)?(.+)$"));
static RUST_REFERENCE: LazyLock<Regex> = LazyLock::new(|| compile(r"^&(mut\s+)?"));
static JAVA_PACKAGE: LazyLock<Regex> = LazyLock::new(|| compile(r"(?m)^\s*package\s+([A-Za-z0-9_.]+)\s*;?"));
static KOTLIN_PACKAGE: LazyLock<Regex> = LazyLock::new(|| compile(r"(?m)^\s*package\s+([A-Za-z0-9_.]+)"));
static GO_PACKAGE: LazyLock<Regex> = LazyLock::new(|| compile(r"(?m)^\s*package\s+([A-Za-z0-9_]+)"));

/// What a reference is built from.
#[derive(Debug, Clone, Copy)]
pub struct ReferenceInput<'a> {
    /// The backend language id (`java`, `rust`…).
    pub language: &'a str,
    pub path: &'a str,
    /// The project root, for project-relative paths and module names.
    pub root: Option<&'a str>,
    /// The document's text.
    pub text: &'a str,
    /// The names of the outline symbols down to the one named at the caret, outermost first; empty
    /// off a name.
    pub chain: &'a [String],
    /// The 0-based caret line.
    pub line: u32,
}

/// The position to resolve: the start of a selected identifier, or the caret on an identifier
/// (UTF-16 offsets into `text`). `None` anywhere else.
pub fn reference_offset(text: &str, from: u32, to: u32) -> Option<u32> {
    let start = byte_of_col(text, from);
    if from != to {
        return IDENTIFIER.is_match(&text[start..byte_of_col(text, to)]).then_some(from);
    }
    WORD_CHAR.is_match(&text[start..]).then_some(from)
}

/// `path` relative to `root` when it is inside it.
pub fn relative_path<'a>(path: &'a str, root: Option<&str>) -> &'a str {
    root.filter(|root| !root.is_empty())
        .and_then(|root| path.strip_prefix(root))
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(path)
}

/// The file name without its extension.
fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[..dot],
        _ => name,
    }
}

/// The type an `impl` block is for: `impl Display for Foo<T>` → `Foo`, generic arguments and a
/// reference dropped.
pub fn rust_impl_type(name: &str) -> String {
    let target = RUST_IMPL.captures(name).and_then(|found| found.get(1)).map_or(name, |found| found.as_str());
    let target = target.trim();
    let bare = target.split('<').next().unwrap_or(target);
    RUST_REFERENCE.replace(bare, "").into_owned()
}

/// `crate::a::b` for `src/a/b.rs` (or `src/a/b/mod.rs`); `crate` for `lib.rs` and `main.rs`.
pub fn rust_module_path(path: &str) -> String {
    let relative = match path.rfind("/src/") {
        Some(src) => &path[src + 5..],
        None => path.rsplit('/').next().unwrap_or(path),
    };
    let mut parts: Vec<&str> = relative.strip_suffix(".rs").unwrap_or(relative).split('/').collect();
    let single = parts.len() == 1;
    if parts.last().is_some_and(|last| *last == "mod" || (single && (*last == "lib" || *last == "main"))) {
        parts.pop();
    }
    std::iter::once("crate").chain(parts).collect::<Vec<_>>().join("::")
}

/// The dotted Python module path relative to the project: a leading `src/` dropped, `__init__`
/// folded into its package.
pub fn python_module_path(path: &str, root: Option<&str>) -> String {
    let relative = relative_path(path, root);
    let relative = relative.strip_suffix(".py").or_else(|| relative.strip_suffix(".pyi")).unwrap_or(relative);
    let mut parts: Vec<&str> = relative.split('/').collect();
    if let Some(src) = parts.iter().rposition(|part| *part == "src")
        && src + 1 < parts.len()
    {
        parts = parts.split_off(src + 1);
    }
    if parts.last() == Some(&"__init__") {
        parts.pop();
    }
    parts.join(".")
}

/// A callable's bare name: a language server may bake the signature into it
/// (`method(int, String)`), and IDEA's reference is the bare name.
pub fn declared_name(name: &str, kind: SymbolKind) -> &str {
    if !matches!(kind, SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor) {
        return name;
    }
    match name.find('(') {
        Some(paren) if paren > 0 => name[..paren].trim(),
        _ => name,
    }
}

fn declared<'a>(text: &'a str, pattern: &Regex) -> Option<&'a str> {
    pattern.captures(text).and_then(|found| found.get(1)).map(|found| found.as_str())
}

/// `pkg.Outer.Inner#member` (the last symbol is the member unless the caret is on a type's name),
/// as IDEA's Copy Reference writes it.
fn jvm_reference(package: Option<&str>, chain: &[&str]) -> String {
    let Some((first, rest)) = chain.split_first() else {
        return package.unwrap_or_default().to_owned();
    };
    let parts = [package.unwrap_or_default(), *first];
    let type_path = parts.into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(".");
    match rest.split_last() {
        Some((member, between)) => {
            let owner: Vec<&str> = std::iter::once(type_path.as_str()).chain(between.iter().copied()).collect();
            let owner = owner.join(".");
            format!("{owner}#{member}")
        }
        None => type_path,
    }
}

/// The reference Copy Reference copies.
pub fn qualified_reference(input: &ReferenceInput<'_>) -> String {
    let relative = relative_path(input.path, input.root);
    let chain: Vec<&str> = input.chain.iter().map(String::as_str).filter(|name| !name.is_empty()).collect();
    if chain.is_empty() {
        return format!("{relative}:{}", input.line + 1);
    }
    match input.language {
        "java" | "groovy" => jvm_reference(declared(input.text, &JAVA_PACKAGE), &chain),
        "kotlin" | "scala" => jvm_reference(declared(input.text, &KOTLIN_PACKAGE), &chain),
        "csharp" => chain.join("."),
        "rust" => {
            let named = |name: &&str| if name.starts_with("impl") { rust_impl_type(name) } else { (*name).to_owned() };
            let names = chain.iter().map(named);
            let path: Vec<String> = std::iter::once(rust_module_path(input.path)).chain(names).collect();
            path.join("::")
        }
        "python" => std::iter::once(python_module_path(input.path, input.root))
            .chain(chain.iter().map(|name| (*name).to_owned()))
            .collect::<Vec<_>>()
            .join("."),
        "go" => {
            let package = declared(input.text, &GO_PACKAGE).unwrap_or_else(|| stem(input.path));
            std::iter::once(package).chain(chain.iter().copied()).collect::<Vec<_>>().join(".")
        }
        "cpp" | "c" | "objective-cpp" | "php" => chain.join("::"),
        _ => format!("{relative}:{}", chain.join(".")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(language: &str, path: &str, text: &str, chain: &[&str]) -> String {
        reference_in(Some("/p"), language, path, text, chain)
    }

    fn reference_in(root: Option<&str>, language: &str, path: &str, text: &str, chain: &[&str]) -> String {
        let chain: Vec<String> = chain.iter().map(|name| (*name).to_owned()).collect();
        qualified_reference(&ReferenceInput { language, path, root, text, chain: &chain, line: 4 })
    }

    #[test]
    fn qualifies_java_members_with_the_package() {
        let text = "package com.acme.util;\n\nclass Strings {}";
        let trim = reference("java", "/p/src/Strings.java", text, &["Strings", "trim"]);
        assert_eq!(trim, "com.acme.util.Strings#trim");
        assert_eq!(reference("java", "/p/src/Strings.java", text, &["Strings"]), "com.acme.util.Strings");
        assert_eq!(reference("java", "/p/src/A.java", "", &["A", "B", "m"]), "A.B#m");
    }

    #[test]
    fn qualifies_kotlin_and_scala_members_like_idea() {
        let text = "package com.acme.util";
        assert_eq!(reference("kotlin", "/p/src/Svc.kt", text, &["Svc", "get"]), "com.acme.util.Svc#get");
        assert_eq!(reference("scala", "/p/src/Svc.scala", text, &["Svc", "get"]), "com.acme.util.Svc#get");
    }

    #[test]
    fn strips_a_language_servers_signature_from_a_callables_name() {
        assert_eq!(declared_name("getUserById(int)", SymbolKind::Method), "getUserById");
        assert_eq!(declared_name("UserService(int)", SymbolKind::Constructor), "UserService");
        assert_eq!(declared_name("run()", SymbolKind::Function), "run");
        // No signature, or a kind that is not callable: the name is kept as it is.
        assert_eq!(declared_name("trim", SymbolKind::Method), "trim");
        assert_eq!(declared_name("Strings", SymbolKind::Class), "Strings");
    }

    #[test]
    fn builds_rust_paths_from_the_module_tree() {
        assert_eq!(rust_module_path("/p/crates/x/src/lib.rs"), "crate");
        assert_eq!(rust_module_path("/p/src/a/b.rs"), "crate::a::b");
        assert_eq!(rust_module_path("/p/src/a/mod.rs"), "crate::a");
        assert_eq!(rust_impl_type("impl<T> fmt::Display for Foo<T>"), "Foo");
        assert_eq!(rust_impl_type("impl Bar"), "Bar");
        let get = reference("rust", "/p/src/net/http.rs", "", &["impl Client", "get"]);
        assert_eq!(get, "crate::net::http::Client::get");
    }

    #[test]
    fn uses_dotted_module_paths_for_python_and_package_names_for_go() {
        assert_eq!(python_module_path("/p/src/pkg/mod.py", Some("/p")), "pkg.mod");
        assert_eq!(
            python_module_path("/p/services/live/src/coordination_layer/db/key_scope.py", Some("/p")),
            "coordination_layer.db.key_scope"
        );
        assert_eq!(python_module_path("/p/pkg/__init__.py", Some("/p")), "pkg");
        assert_eq!(reference("python", "/p/pkg/mod.py", "", &["C", "m"]), "pkg.mod.C.m");
        assert_eq!(reference("go", "/p/x.go", "package server\n", &["Run"]), "server.Run");
    }

    #[test]
    fn falls_back_to_path_line() {
        assert_eq!(reference("python", "/p/a.py", "", &[]), "a.py:5");
        assert_eq!(reference("rust", "/p/src/a.rs", "", &[]), "src/a.rs:5");
        assert_eq!(reference("java", "/p/src/A.java", "", &[]), "src/A.java:5");
        assert_eq!(reference("typescript", "/p/a.ts", "", &["C", "m"]), "a.ts:C.m");
        assert_eq!(reference_in(None, "markdown", "/x/a.md", "", &[]), "/x/a.md:5");
        assert_eq!(reference("python", "/p/pkg/mod.py", "", &[]), "pkg/mod.py:5");
    }

    #[test]
    fn only_resolves_a_selected_identifier_or_an_identifier_under_the_caret() {
        assert_eq!(reference_offset("  load_scoped(x)", 2, 13), Some(2));
        assert_eq!(reference_offset("  load_scoped(x)", 5, 5), Some(5));
        assert_eq!(reference_offset("  load_scoped(x)", 0, 0), None);
        assert_eq!(reference_offset("  load_scoped(x)", 2, 16), None);
    }
}
