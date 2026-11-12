//! PHP language constructs (`empty`, `isset`) have no source declaration: hover, Go to
//! Declaration and Quick Definition take their documentation from here, without a server.

use crate::editor::buffer;
use crate::editor_commands::tree::{SyntaxTree, pascal_name};
use ropey::Rope;

struct Construct {
    name: &'static str,
    signature: &'static str,
    description: &'static str,
}

const CONSTRUCTS: [Construct; 2] = [
    Construct {
        name: "empty",
        signature: "empty(mixed $var): bool",
        description: "Returns true when the value is falsey or the variable does not exist. Checking an undefined \
                      variable does not emit a warning.",
    },
    Construct {
        name: "isset",
        signature: "isset(mixed $var, mixed ...$vars): bool",
        description: "Returns true only when every supplied variable exists and is not null. Checks stop at the \
                      first unset or null variable.",
    },
];

fn is_php(language: Option<&str>) -> bool {
    language.is_some_and(|language| language.eq_ignore_ascii_case("php"))
}

/// The Markdown documentation of construct `name` (any case) in a PHP document.
pub fn php_construct_documentation(language: Option<&str>, name: &str) -> Option<String> {
    if !is_php(language) {
        return None;
    }
    let key = name.to_ascii_lowercase();
    let entry = CONSTRUCTS.iter().find(|construct| construct.name == key)?;
    Some(
        [
            format!("```php\n{}\n```", entry.signature),
            entry.description.to_owned(),
            "PHP language construct; there is no source declaration to navigate to.".to_owned(),
            format!("[PHP manual](https://www.php.net/manual/en/function.{key}.php)"),
        ]
        .join("\n\n"),
    )
}

/// A construct documented at `pos` (either edge of its name counts): only the name of an
/// unqualified call, never a method, a variable, a string, a comment or HTML text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstructAt {
    pub from: u32,
    pub to: u32,
    pub markdown: String,
}

pub fn php_construct_at(tree: &SyntaxTree, doc: &Rope, pos: u32, language: Option<&str>) -> Option<ConstructAt> {
    if !is_php(language) {
        return None;
    }
    for side in [-1, 1] {
        let Some(node) = tree.resolve_inner(pos, side) else { continue };
        let Some(parent) = tree.parent(node) else { continue };
        let (name, call) = (tree.node(node), tree.node(parent));
        if pascal_name(&name.name) != "Name" || pascal_name(&call.name) != "CallExpression" || call.from != name.from {
            continue;
        }
        if let Some(markdown) = php_construct_documentation(language, &buffer::slice(doc, name.from, name.to)) {
            return Some(ConstructAt { from: name.from, to: name.to, markdown });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_commands::tree::TreeNode;

    /// `<?php NAME($value);` as the PHP grammar parses it.
    fn call(name: &str) -> (Rope, SyntaxTree) {
        let text = format!("<?php {name}($value);");
        let end = 6 + name.len() as u32;
        let tree = SyntaxTree::new(vec![
            TreeNode::new("Template", 0, text.len() as u32),
            TreeNode::new("ExpressionStatement", 6, end + 9),
            TreeNode::new("CallExpression", 6, end + 8),
            TreeNode::new("Name", 6, end),
            TreeNode::new("ArgList", end, end + 8),
            TreeNode::new("VariableName", end + 1, end + 7),
        ]);
        (Rope::from_str(&text), tree)
    }

    #[test]
    fn documents_a_construct_at_either_edge_of_its_name() {
        for name in ["empty", "isset", "EMPTY"] {
            let (doc, tree) = call(name);
            let end = 6 + name.len() as u32;
            for pos in [6, 8, end] {
                let found = php_construct_at(&tree, &doc, pos, Some("php")).expect("documented");
                assert_eq!((found.from, found.to), (6, end), "{name} at {pos}");
                assert!(found.markdown.contains("PHP language construct"));
                assert!(found.markdown.contains(&format!("function.{}.php", name.to_ascii_lowercase())));
            }
        }
    }

    #[test]
    fn does_not_hijack_other_occurrences() {
        // Each text with the node holding `empty` and that node's parent.
        type Nodes<'a> = &'a [(&'a str, u32, u32)];
        let cases: [(&str, Nodes); 6] = [
            ("<?php $empty = \"empty($x)\";", &[("Assign", 6, 26), ("VariableName", 6, 12), ("String", 15, 26)]),
            ("<?php // empty($x)", &[("LineComment", 6, 18)]),
            ("<?php $obj->empty();", &[("CallExpression", 6, 19), ("MemberExpression", 6, 17), ("Name", 12, 17)]),
            ("<?php Foo::empty();", &[("CallExpression", 6, 18), ("ScopedExpression", 6, 16), ("Name", 11, 16)]),
            ("<?php Foo\\empty();", &[("CallExpression", 6, 17), ("QualifiedName", 6, 15), ("Name", 10, 15)]),
            ("<?php function empty() {}", &[("FunctionDefinition", 6, 25), ("Name", 15, 20)]),
        ];
        for (text, nodes) in cases {
            let mut all = vec![TreeNode::new("Template", 0, text.len() as u32)];
            all.extend(nodes.iter().map(|(name, from, to)| TreeNode::new(name, *from, *to)));
            let tree = SyntaxTree::new(all);
            let doc = Rope::from_str(text);
            for (at, _) in text.match_indices("empty") {
                assert_eq!(php_construct_at(&tree, &doc, at as u32 + 2, Some("php")), None, "{text}");
            }
        }
    }

    #[test]
    fn does_not_attach_php_docs_to_other_languages_or_ordinary_functions() {
        let (doc, tree) = call("empty");
        assert_eq!(php_construct_at(&tree, &doc, 8, Some("javascript")), None);
        assert_eq!(php_construct_documentation(Some("PHP"), "strtoupper"), None);
        assert_eq!(php_construct_documentation(Some("php"), "constructor"), None);
    }

    #[test]
    fn serves_hover_declaration_and_quick_definition_without_a_running_server() {
        let (doc, tree) = call("empty");
        let found = php_construct_at(&tree, &doc, 8, Some("php")).expect("documented");
        assert!(found.markdown.contains("empty(mixed $var): bool"));
        assert!(found.markdown.contains("no source declaration"));
    }
}
