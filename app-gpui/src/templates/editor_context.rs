//! Facts about the caret position used by template variables: the enclosing class and function
//! names, the comment tokens, whether the caret is inside a string or a comment. The syntax tree
//! comes first, text scanning is the fallback.
//!
//! The tree is an input: the chain of nodes around the position, innermost first, as
//! [`TreeNode`]s. Node kinds may be spelled `ClassDeclaration` or `class_declaration`; both read
//! the same here.

use super::live_model::{TemplateFunctions, camel_case, decapitalize, snake_case};
use regex::Regex;
use std::sync::LazyLock;

/// A direct child of a [`TreeNode`], with the text it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeChild {
    pub kind: String,
    pub text: String,
}

/// A syntax node around a position (UTF-16 offsets).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TreeNode {
    pub kind: String,
    pub from: u32,
    pub to: u32,
    /// The node is an error node of the parser.
    pub error: bool,
    /// The direct children, in order. Only needed on the nodes names are read from.
    pub children: Vec<TreeChild>,
}

impl TreeNode {
    pub fn new(kind: &str, from: u32, to: u32) -> Self {
        Self { kind: kind.to_owned(), from, to, ..Self::default() }
    }

    pub fn child(mut self, kind: &str, text: &str) -> Self {
        self.children.push(TreeChild { kind: kind.to_owned(), text: text.to_owned() });
        self
    }
}

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

/// `kind` in the spelling the patterns below are written for: `impl_item` reads `ImplItem`.
pub(crate) fn pascal_kind(kind: &str) -> String {
    if !kind.contains('_') && kind.starts_with(|c: char| c.is_ascii_uppercase()) {
        return kind.to_owned();
    }
    let mut out = String::with_capacity(kind.len());
    for word in kind.split('_') {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

/// Which enclosing declaration a name is asked of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    Class,
    Func,
}

static CLASS_NODE: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r"^(Class|Interface|Struct|Enum|Trait|Impl|Object|Record|Type)(Declaration|Definition|Item|Spec|Decl)?$")
});
static FUNC_NODE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^(Function|Method|Constructor|Fun)(Declaration|Definition|Item|Decl)$"));
static NAME_NODE: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        "^(VariableDefinition|VariableName|TypeIdentifier|TypeDefinition|Definition|DefName|BoundIdentifier\
         |PropertyDefinition|PropertyName|Identifier|Name|TypeName|FieldName)$",
    )
});
/// Where a declaration's header ends.
static BODY_NODE: LazyLock<Regex> = LazyLock::new(|| pattern("Body|Block|ParamList|Parameters|ArgList"));

fn node_name(node: &TreeNode, kind: NameKind) -> Option<&str> {
    let header = node.children.iter().take_while(|child| !BODY_NODE.is_match(&pascal_kind(&child.kind)));
    let named = header.filter(|child| NAME_NODE.is_match(&pascal_kind(&child.kind)));
    let mut names = named.map(|child| child.text.as_str());
    // `impl Trait for Type` names the type.
    if kind == NameKind::Class && pascal_kind(&node.kind).starts_with("Impl") { names.last() } else { names.next() }
}

/// The name of the class or function around the position: from `ancestors` (innermost first)
/// when one of them is such a declaration with a name, else from the last declaration in
/// `before`, the text before the position. Empty when there is none.
pub fn enclosing_name(ancestors: &[TreeNode], before: &str, kind: NameKind) -> String {
    static CLASS_TEXT: LazyLock<Regex> = LazyLock::new(|| {
        pattern(concat!(
            r"(?-u:\b)(?:class|interface|struct|enum|trait|object|record",
            r"|impl(?:<[^>\n]*>)?(?:\s+[A-Za-z0-9_:<>]+\s+for)?)",
            r"\s+([A-Za-z_$][A-Za-z0-9_$]*)",
        ))
    });
    static FUNC_TEXT: LazyLock<Regex> = LazyLock::new(|| {
        pattern(r"(?-u:\b)(?:fn|def|func|function|fun)\s+(?:\([^)\n]*\)\s*)?([A-Za-z_$][A-Za-z0-9_$]*)\s*[(<]")
    });
    let declaration = match kind {
        NameKind::Class => &CLASS_NODE,
        NameKind::Func => &FUNC_NODE,
    };
    let from_tree = ancestors
        .iter()
        .filter(|node| declaration.is_match(&pascal_kind(&node.kind)))
        .find_map(|node| node_name(node, kind).filter(|name| !name.is_empty()));
    if let Some(name) = from_tree {
        return name.to_owned();
    }
    let text = match kind {
        NameKind::Class => &CLASS_TEXT,
        NameKind::Func => &FUNC_TEXT,
    };
    let last = text.captures_iter(before).last().and_then(|found| found.get(1));
    last.map(|name| name.as_str().to_owned()).unwrap_or_default()
}

static COMMENT: LazyLock<Regex> = LazyLock::new(|| pattern("(?i)Comment"));
static STRING: LazyLock<Regex> = LazyLock::new(|| pattern("String|Literal"));

/// Whether `pos` is inside a comment or a string of the chain `ancestors` (innermost first).
/// Directly after a closed string the position is outside it.
pub fn in_comment_or_string(ancestors: &[TreeNode], pos: u32) -> bool {
    static COMMENT_OR_STRING: LazyLock<Regex> =
        LazyLock::new(|| pattern("(?i)Comment|String|Docstring|Template|Char(acter)?Literal|RawString"));
    static STRING_ANY_CASE: LazyLock<Regex> = LazyLock::new(|| pattern("(?i)String|Literal"));
    for node in ancestors {
        let kind = pascal_kind(&node.kind);
        if COMMENT_OR_STRING.is_match(&kind) && node.from < pos && (node.to > pos || !STRING.is_match(&kind)) {
            return !(STRING_ANY_CASE.is_match(&kind) && node.to == pos);
        }
    }
    false
}

/// Whether `pos` is inside a comment of the chain `ancestors`.
pub fn in_comment(ancestors: &[TreeNode], pos: u32) -> bool {
    ancestors.iter().any(|node| COMMENT.is_match(&node.kind) && node.from < pos)
}

/// How the language at a position writes comments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommentTokens {
    pub line: Option<String>,
    /// The opening and the closing token.
    pub block: Option<(String, String)>,
}

/// Method names that say nothing about what they return.
const STOP_WORDS: &[&str] = &[
    "get",
    "new",
    "iter",
    "into_iter",
    "iter_mut",
    "clone",
    "unwrap",
    "expect",
    "to_string",
    "to_owned",
    "as_ref",
    "collect",
    "await",
    "toString",
    "valueOf",
    "build",
    "of",
];

/// Names no variable can have in one of the languages.
const KEYWORDS: &[&str] = &[
    "if", "else", "for", "while", "do", "return", "class", "fn", "let", "const", "var", "val", "def", "func", "type",
    "match", "in", "is", "as", "new", "this", "self", "super", "import", "package", "struct", "enum", "impl", "trait",
    "loop", "break", "continue", "true", "false", "null", "nil", "None", "default", "switch", "case", "try", "catch",
];

fn is_identifier(part: &str) -> bool {
    let mut chars = part.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Variable name for an expression, IDEA-style: `getUserName()` → `userName`, `items.iter()` →
/// `items`, `new ArrayList<>()` → `arrayList`. `fallback` when the expression names nothing.
pub fn suggest_variable_name(expr: &str, language: &str, fallback: &str) -> String {
    static GENERICS: LazyLock<Regex> = LazyLock::new(|| pattern(r"<[^<>()]*>"));
    static CALLS: LazyLock<Regex> = LazyLock::new(|| pattern(r"\((?:[^()]|\([^()]*\))*\)|\[(?:[^\[\]])*\]"));
    static LINKS: LazyLock<Regex> = LazyLock::new(|| pattern(r"\.|::|->|\s+"));
    let snake = matches!(language, "rust" | "python");
    let cleaned = GENERICS.replace_all(expr, "");
    let cleaned = CALLS.replace_all(&cleaned, "");
    let cleaned: String = cleaned.chars().filter(|c| !matches!(c, '?' | '!' | '&' | '*')).collect();
    let parts: Vec<&str> = LINKS.split(&cleaned).filter(|part| is_identifier(part)).collect();
    let before_last = || parts.len().checked_sub(2).map_or("", |at| parts[at]);
    let mut word = parts.last().copied().unwrap_or_default();
    if word == "new" || word.is_empty() {
        word = before_last();
    }
    // An accessor names what it returns: `getName` is a `name`.
    for prefix in ["get", "is", "has", "to", "as"] {
        if let Some(rest) = word.strip_prefix(prefix)
            && rest.starts_with(|c: char| c.is_ascii_uppercase() || c == '_')
        {
            word = rest.strip_prefix('_').unwrap_or(rest);
            break;
        }
    }
    if STOP_WORDS.contains(&word) {
        word = before_last();
    }
    if word.is_empty() || word.starts_with(|c: char| c.is_ascii_digit()) {
        return fallback.to_owned();
    }
    let or_else = |name: String, other: &str| if name.is_empty() { other.to_owned() } else { name };
    let name = if snake {
        or_else(snake_case(word), fallback)
    } else if word.starts_with(|c: char| c.is_ascii_uppercase()) {
        decapitalize(word)
    } else {
        decapitalize(&or_else(camel_case(word), word))
    };
    if KEYWORDS.contains(&name.as_str()) { format!("{name}Value") } else { name }
}

/// What the variable functions of a template are bound to.
#[derive(Debug, Clone, Copy)]
pub struct TemplateEnvInput<'a> {
    /// The syntax nodes around the position, innermost first.
    pub ancestors: &'a [TreeNode],
    /// The text before the position (the last 20 000 UTF-16 units are plenty).
    pub before: &'a str,
    pub pos: u32,
    pub path: &'a str,
    pub language: &'a str,
    pub comment_tokens: &'a CommentTokens,
    pub clipboard: Option<&'a str>,
    pub user: Option<&'a str>,
    /// The postfix expression, for `suggestVariableName()` and `expressionText()`.
    pub expression: Option<&'a str>,
}

/// The variable functions bound to an editor position.
pub fn template_functions(input: &TemplateEnvInput<'_>) -> TemplateFunctions<'static> {
    let file = input.path.rsplit('/').next().unwrap_or(input.path).to_owned();
    let stem = match file.rfind('.') {
        Some(dot) if dot > 0 => file[..dot].to_owned(),
        _ => file.clone(),
    };
    let tokens = input.comment_tokens;
    let commented = in_comment(input.ancestors, input.pos);
    let comment_start = match (&tokens.line, &tokens.block) {
        _ if commented => String::new(),
        (Some(line), _) if !line.is_empty() => format!("{line} "),
        (_, Some((open, _))) => format!("{open} "),
        _ => "// ".to_owned(),
    };
    let comment_end = match (&tokens.line, &tokens.block) {
        _ if commented => String::new(),
        (Some(line), _) if !line.is_empty() => String::new(),
        (_, Some((_, close))) => format!(" {close}"),
        _ => String::new(),
    };
    let line_comment = tokens.line.clone().unwrap_or_else(|| "//".to_owned());
    let expression = input.expression.unwrap_or_default();
    let values = [
        ("className", enclosing_name(input.ancestors, input.before, NameKind::Class)),
        ("methodName", enclosing_name(input.ancestors, input.before, NameKind::Func)),
        ("functionName", enclosing_name(input.ancestors, input.before, NameKind::Func)),
        ("fileName", file),
        ("fileNameWithoutExtension", stem),
        ("filePath", input.path.to_owned()),
        ("user", input.user.unwrap_or_default().to_owned()),
        ("clipboard", input.clipboard.unwrap_or_default().to_owned()),
        ("suggestVariableName", suggest_variable_name(expression, input.language, "")),
        ("expressionText", expression.to_owned()),
        ("commentStart", comment_start),
        ("commentEnd", comment_end),
        ("lineCommentStart", line_comment),
    ];
    let mut functions = TemplateFunctions::new();
    for (name, value) in values {
        functions.insert(name, move |_| value.clone());
    }
    functions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(ancestors: &[TreeNode], kind: NameKind) -> String {
        enclosing_name(ancestors, "", kind)
    }

    // editor context › finds enclosing class and method via the syntax tree
    //
    // The chains are written by hand, as a tree-sitter grammar shapes the spec's documents.
    #[test]
    fn finds_enclosing_class_and_method_via_the_syntax_tree() {
        // class Foo {\n  void bar() {\n    int x;\n  }\n}
        let java = [
            TreeNode::new("local_variable_declaration", 31, 37),
            TreeNode::new("block", 25, 41),
            TreeNode::new("method_declaration", 14, 41)
                .child("void_type", "void")
                .child("identifier", "bar")
                .child("formal_parameters", "()")
                .child("block", "{\n    int x;\n  }"),
            TreeNode::new("class_body", 10, 43),
            TreeNode::new("class_declaration", 0, 43)
                .child("class", "class")
                .child("identifier", "Foo")
                .child("class_body", "{…}"),
            TreeNode::new("program", 0, 43),
        ];
        assert_eq!(name(&java, NameKind::Class), "Foo");
        assert_eq!(name(&java, NameKind::Func), "bar");

        // struct A;\nimpl Display for A {\n    fn fmt(&self) {\n        x\n    }\n}
        let rust = [
            TreeNode::new("identifier", 57, 58),
            TreeNode::new("block", 47, 64),
            TreeNode::new("function_item", 33, 64)
                .child("fn", "fn")
                .child("identifier", "fmt")
                .child("parameters", "(&self)")
                .child("block", "{\n        x\n    }"),
            TreeNode::new("declaration_list", 29, 66),
            TreeNode::new("impl_item", 10, 66)
                .child("impl", "impl")
                .child("type_identifier", "Display")
                .child("for", "for")
                .child("type_identifier", "A")
                .child("declaration_list", "{…}"),
            TreeNode::new("source_file", 0, 66),
        ];
        assert_eq!(name(&rust, NameKind::Class), "A");
        assert_eq!(name(&rust, NameKind::Func), "fmt");

        // class Svc:\n    def run(self):\n        pass\n
        let python = [
            TreeNode::new("pass_statement", 38, 42),
            TreeNode::new("block", 38, 42),
            TreeNode::new("function_definition", 15, 42)
                .child("def", "def")
                .child("identifier", "run")
                .child("parameters", "(self)")
                .child("block", "pass"),
            TreeNode::new("block", 15, 42),
            TreeNode::new("class_definition", 0, 42)
                .child("class", "class")
                .child("identifier", "Svc")
                .child("block", "…"),
            TreeNode::new("module", 0, 43),
        ];
        assert_eq!(name(&python, NameKind::Class), "Svc");
        assert_eq!(name(&python, NameKind::Func), "run");

        // Node kinds spelled in PascalCase read the same.
        let lezer = [TreeNode::new("ClassDeclaration", 0, 9)
            .child("class", "class")
            .child("Definition", "Foo")
            .child("ClassBody", "{}")];
        assert_eq!(name(&lezer, NameKind::Class), "Foo");
    }

    // editor context › falls back to text scanning without a parser
    #[test]
    fn falls_back_to_text_scanning_without_a_parser() {
        let doc = "class Widget {\n  fun draw() {\n    x\n";
        assert_eq!(enclosing_name(&[], doc, NameKind::Class), "Widget");
        assert_eq!(enclosing_name(&[], doc, NameKind::Func), "draw");
    }

    // editor context › binds file and comment functions
    #[test]
    fn binds_file_and_comment_functions() {
        let tokens = CommentTokens { line: Some("//".to_owned()), block: Some(("/*".to_owned(), "*/".to_owned())) };
        let ancestors = [TreeNode::new("block", 10, 18), TreeNode::new("function_item", 0, 18)];
        let fns = template_functions(&TemplateEnvInput {
            ancestors: &ancestors,
            before: "fn main() {\n    ",
            pos: 16,
            path: "/p/src/main.rs",
            language: "rust",
            comment_tokens: &tokens,
            clipboard: None,
            user: Some("ann"),
            expression: None,
        });
        let call = |name: &str| fns.call(name, &[]);
        assert_eq!(call("fileName").as_deref(), Some("main.rs"));
        assert_eq!(call("fileNameWithoutExtension").as_deref(), Some("main"));
        assert_eq!(call("user").as_deref(), Some("ann"));
        assert_eq!(call("commentStart").as_deref(), Some("// "));
    }

    // editor context › suggests variable names
    #[test]
    fn suggests_variable_names() {
        assert_eq!(suggest_variable_name("user.getName()", "java", "value"), "name");
        assert_eq!(suggest_variable_name("new ArrayList<>()", "java", "value"), "arrayList");
        assert_eq!(suggest_variable_name("self.items.iter()", "rust", "value"), "items");
        assert_eq!(suggest_variable_name("loadUserProfile(id)", "python", "value"), "load_user_profile");
        assert_eq!(suggest_variable_name("fetchUsers()", "typescript", "value"), "fetchUsers");
        assert_eq!(suggest_variable_name("42", "typescript", "value"), "value");
    }

    // Not from a spec: a position inside a comment or an open string is not code; right after a
    // closed string it is, and comment functions yield nothing inside a comment.
    #[test]
    fn tells_comments_and_strings_from_code() {
        let comment = [TreeNode::new("line_comment", 0, 6), TreeNode::new("source_file", 0, 6)];
        assert!(in_comment_or_string(&comment, 6));
        assert!(in_comment(&comment, 6));
        assert!(!in_comment_or_string(&comment, 0));
        let string = [TreeNode::new("string_literal", 4, 9), TreeNode::new("source_file", 0, 12)];
        assert!(in_comment_or_string(&string, 6));
        assert!(!in_comment_or_string(&string, 9));
        assert!(!in_comment(&string, 6));
        assert!(!in_comment_or_string(&[TreeNode::new("Block", 0, 9)], 4));
        let tokens = CommentTokens { line: None, block: Some(("<!--".to_owned(), "-->".to_owned())) };
        let input = TemplateEnvInput {
            ancestors: &[],
            before: "",
            pos: 0,
            path: "index.html",
            language: "html",
            comment_tokens: &tokens,
            clipboard: Some("copied"),
            user: None,
            expression: Some("items.iter()"),
        };
        let fns = template_functions(&input);
        assert_eq!(fns.call("commentStart", &[]).as_deref(), Some("<!-- "));
        assert_eq!(fns.call("commentEnd", &[]).as_deref(), Some(" -->"));
        assert_eq!(fns.call("lineCommentStart", &[]).as_deref(), Some("//"));
        assert_eq!(fns.call("clipboard", &[]).as_deref(), Some("copied"));
        assert_eq!(fns.call("suggestVariableName", &[]).as_deref(), Some("items"));
        assert_eq!(fns.call("fileNameWithoutExtension", &[]).as_deref(), Some("index"));
        let inside = template_functions(&TemplateEnvInput { ancestors: &comment, pos: 3, ..input });
        assert_eq!(inside.call("commentStart", &[]).as_deref(), Some(""));
        assert_eq!(pascal_kind("raw_string_literal"), "RawStringLiteral");
        assert_eq!(suggest_variable_name("x.default()", "java", "value"), "defaultValue");
    }
}
