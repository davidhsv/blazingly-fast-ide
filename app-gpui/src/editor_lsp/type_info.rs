//! View › Expression Type Info (⌃⇧P): the type of the symbol or expression at the caret, taken
//! from the signature block of the language server's hover.

use ide_lsp::types::Markup;
use regex::Regex;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

/// What Expression Type Info shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeInfo {
    /// The type, or the whole signature when no type can be split off.
    pub text: String,
    /// The fence language of the hover's code block, if any.
    pub language: Option<String>,
}

pub const NO_TYPE_INFO: &str = "No type information found";

/// The type of a variable, property or parameter signature; other signatures as they are.
pub fn type_from_signature(signature: &str) -> String {
    static SPACES: LazyLock<Regex> = LazyLock::new(|| pattern(r"\s+"));
    // `(local var) x: T`, `let mut x: T`, `(property) a.b?: T`, `x: T`.
    static COLON_TYPED: LazyLock<Regex> = LazyLock::new(|| {
        pattern(
            r"^(?:\([A-Za-z0-9_ ]+\)\s*)?(?:(?:let|const|var|val|mut|static|readonly|export|declare|public|private|protected|internal|final|ref)\s+)*[A-Za-z0-9_$.]+\??\s*:\s*([\s\S]+)$",
        )
    });
    // Go: `var x int`, `field x string`, `const x untyped int`.
    static GO_TYPED: LazyLock<Regex> =
        LazyLock::new(|| pattern(r"^(?:var|field|const)\s+[A-Za-z0-9_$]+\s+([\s\S]+?)(?:\s*=.*)?$"));
    static DEFAULT: LazyLock<Regex> = LazyLock::new(|| pattern(r"\s=\s[\s\S]*$"));
    let signature = SPACES.replace_all(signature, " ");
    let signature = signature.trim();
    if let Some(typed) = COLON_TYPED.captures(signature).and_then(|found| found.get(1)) {
        return DEFAULT.replace(typed.as_str(), "").trim().to_owned();
    }
    if let Some(typed) = GO_TYPED.captures(signature).and_then(|found| found.get(1)) {
        return typed.as_str().trim().to_owned();
    }
    signature.to_owned()
}

/// The type Expression Type Info shows for a hover: its first code block that is not a lone
/// module path, else an inline code span, else (plain text) its first line.
pub fn extract_type_info(markup: Option<&Markup>) -> Option<TypeInfo> {
    static FENCE: LazyLock<Regex> = LazyLock::new(|| pattern(r"```([A-Za-z0-9_+#-]*)[^\n]*\n([\s\S]*?)```"));
    static PATH_ONLY: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[A-Za-z0-9_$]+(?:(?:::|\.)[A-Za-z0-9_$]+)*$"));
    static INLINE: LazyLock<Regex> = LazyLock::new(|| pattern(r"`([^`]+)`"));
    let markup = markup.filter(|markup| !markup.value.trim().is_empty())?;
    if markup.kind != "markdown" {
        let first = markup.value.lines().find(|line| !line.trim().is_empty())?;
        return Some(TypeInfo { text: type_from_signature(first), language: None });
    }
    let blocks: Vec<(Option<String>, String)> = FENCE
        .captures_iter(&markup.value)
        .map(|found| {
            let language = found.get(1).map(|m| m.as_str()).filter(|l| !l.is_empty()).map(str::to_owned);
            (language, found.get(2).map_or("", |m| m.as_str()).trim().to_owned())
        })
        .filter(|(_, code)| !code.is_empty())
        .collect();
    if let Some((language, code)) = blocks.iter().find(|(_, code)| !PATH_ONLY.is_match(code)).or(blocks.first()) {
        return Some(TypeInfo { text: type_from_signature(code), language: language.clone() });
    }
    let inline = INLINE.captures(&markup.value).and_then(|found| found.get(1))?;
    Some(TypeInfo { text: type_from_signature(inline.as_str()), language: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(value: &str) -> Markup {
        Markup { kind: "markdown".into(), value: value.into() }
    }

    fn info(text: &str, language: Option<&str>) -> Option<TypeInfo> {
        Some(TypeInfo { text: text.into(), language: language.map(str::to_owned) })
    }

    #[test]
    fn splits_the_type_off_variable_signatures() {
        assert_eq!(type_from_signature("(local var) x: number"), "number");
        assert_eq!(type_from_signature("let mut v: Vec<i32>"), "Vec<i32>");
        assert_eq!(type_from_signature("(property) a.b?: string"), "string");
        assert_eq!(type_from_signature("const f: (a: number) => void"), "(a: number) => void");
        assert_eq!(type_from_signature("x: int = 3"), "int");
        assert_eq!(type_from_signature("var count int"), "int");
        assert_eq!(type_from_signature("fn add(a: i32, b: i32) -> i32"), "fn add(a: i32, b: i32) -> i32");
        assert_eq!(type_from_signature("String name"), "String name");
    }

    #[test]
    fn reads_the_signature_block_of_a_hover_skipping_module_paths() {
        let rust = md("```rust\ncrate::util\n```\n\n```rust\nlet n: usize\n```\n---\nDocs");
        assert_eq!(extract_type_info(Some(&rust)), info("usize", Some("rust")));
        let typescript = md("```typescript\nconst a: string[]\n```");
        assert_eq!(extract_type_info(Some(&typescript)), info("string[]", Some("typescript")));
        assert_eq!(extract_type_info(Some(&md("Just prose with `T` inline"))), info("T", None));
        let plain = Markup { kind: "plaintext".into(), value: "\nint x\nmore".into() };
        assert_eq!(extract_type_info(Some(&plain)), info("int x", None));
        assert_eq!(extract_type_info(Some(&md("   "))), None);
        assert_eq!(extract_type_info(None), None);
    }
}
