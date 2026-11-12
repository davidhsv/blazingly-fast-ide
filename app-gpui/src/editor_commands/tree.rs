//! The syntax tree as the editing commands read it: named nodes with their offsets. The parser
//! that produced it does not matter; node names may be spelled `BlockComment` or `block_comment`,
//! the commands match both.

/// One syntax node (UTF-16 offsets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    pub name: String,
    pub from: u32,
    pub to: u32,
}

impl TreeNode {
    pub fn new(name: &str, from: u32, to: u32) -> Self {
        Self { name: name.to_owned(), from, to }
    }
}

/// A syntax tree: its nodes in document order, a parent before its children. The first node is
/// the top node and covers the document; an empty tree means the text is not parsed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyntaxTree {
    nodes: Vec<TreeNode>,
    parents: Vec<Option<usize>>,
}

impl SyntaxTree {
    /// A tree from its nodes in document order, a parent before its children: a node's parent is
    /// the closest node before it that contains it.
    pub fn new(nodes: Vec<TreeNode>) -> Self {
        let mut parents = Vec::with_capacity(nodes.len());
        let mut open: Vec<usize> = Vec::new();
        for (index, node) in nodes.iter().enumerate() {
            while open.last().is_some_and(|top| !(nodes[*top].from <= node.from && node.to <= nodes[*top].to)) {
                open.pop();
            }
            parents.push(open.last().copied());
            open.push(index);
        }
        Self { nodes, parents }
    }

    /// The text is not parsed.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn node(&self, index: usize) -> &TreeNode {
        &self.nodes[index]
    }

    pub fn name(&self, index: usize) -> &str {
        &self.nodes[index].name
    }

    pub fn parent(&self, index: usize) -> Option<usize> {
        self.parents.get(index).copied().flatten()
    }

    /// The direct children of a node, in document order.
    pub fn children(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        // A node's descendants follow it without a gap, each with a parent at or after the node.
        (index + 1..self.nodes.len())
            .take_while(move |child| self.parents[*child].is_some_and(|parent| parent >= index))
            .filter(move |child| self.parents[*child] == Some(index))
    }

    /// A node and its ancestors, innermost first.
    pub fn ancestors(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(Some(index), move |node| self.parent(*node))
    }

    /// Every node that touches `from..=to`, in document order.
    pub fn touching(&self, from: u32, to: u32) -> impl Iterator<Item = usize> + '_ {
        (0..self.nodes.len()).filter(move |index| self.nodes[*index].from <= to && self.nodes[*index].to >= from)
    }

    /// The innermost node around `pos`. With a negative `side` nodes that end at `pos` count,
    /// with a positive one nodes that start there; with zero the node must hold text on both
    /// sides of `pos`. The top node always counts. `None` for an empty tree.
    pub fn resolve_inner(&self, pos: u32, side: i8) -> Option<usize> {
        if self.nodes.is_empty() {
            return None;
        }
        let mut current = 0;
        loop {
            let inner = self.children(current).find(|child| {
                let node = &self.nodes[*child];
                let starts_before = if side > 0 { node.from <= pos } else { node.from < pos };
                let ends_after = if side < 0 { node.to >= pos } else { node.to > pos };
                starts_before && ends_after
            });
            match inner {
                Some(child) => current = child,
                None => return Some(current),
            }
        }
    }
}

/// `name` in the spelling the commands' patterns are written for: `block_comment` reads
/// `BlockComment`.
pub fn pascal_name(name: &str) -> String {
    if !name.contains('_') {
        return name.to_owned();
    }
    let mut out = String::with_capacity(name.len());
    for word in name.split('_') {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SyntaxTree {
        // f(a, "b")
        SyntaxTree::new(vec![
            TreeNode::new("Script", 0, 9),
            TreeNode::new("CallExpression", 0, 9),
            TreeNode::new("VariableName", 0, 1),
            TreeNode::new("ArgList", 1, 9),
            TreeNode::new("(", 1, 2),
            TreeNode::new("VariableName", 2, 3),
            TreeNode::new("String", 5, 8),
            TreeNode::new(")", 8, 9),
        ])
    }

    #[test]
    fn parents_and_children_follow_containment() {
        let tree = sample();
        assert_eq!(tree.parent(0), None);
        assert_eq!(tree.parent(1), Some(0));
        assert_eq!(tree.parent(6), Some(3));
        assert_eq!(tree.children(3).collect::<Vec<_>>(), [4, 5, 6, 7]);
        assert_eq!(tree.children(1).collect::<Vec<_>>(), [2, 3]);
        assert_eq!(tree.ancestors(6).collect::<Vec<_>>(), [6, 3, 1, 0]);
        assert_eq!(tree.touching(3, 5).collect::<Vec<_>>(), [0, 1, 3, 5, 6]);
    }

    #[test]
    fn the_innermost_node_depends_on_the_side() {
        let tree = sample();
        assert_eq!(tree.resolve_inner(5, 1).map(|node| tree.name(node)), Some("String"));
        assert_eq!(tree.resolve_inner(5, -1).map(|node| tree.name(node)), Some("ArgList"));
        assert_eq!(tree.resolve_inner(8, -1).map(|node| tree.name(node)), Some("String"));
        assert_eq!(tree.resolve_inner(8, 0).map(|node| tree.name(node)), Some("ArgList"));
        assert_eq!(tree.resolve_inner(1, -1).map(|node| tree.name(node)), Some("VariableName"));
        assert_eq!(SyntaxTree::default().resolve_inner(0, 1), None);
        assert!(SyntaxTree::default().is_empty());
    }

    #[test]
    fn node_names_read_the_same_in_both_spellings() {
        assert_eq!(pascal_name("block_comment"), "BlockComment");
        assert_eq!(pascal_name("BlockComment"), "BlockComment");
        assert_eq!(pascal_name("{"), "{");
    }
}
