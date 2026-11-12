//! Text for "Copy Value": a structured value (array, object, map) is copied with its contents, not
//! its summary.

use ide_dap::Variable;
use std::collections::HashSet;

/// How much of a structured value is copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValueTextLimits {
    /// Levels of children below the value.
    pub depth: usize,
    /// Children in total.
    pub entries: usize,
}

pub const VALUE_TEXT_LIMITS: ValueTextLimits = ValueTextLimits { depth: 8, entries: 2000 };

struct Walk<'a> {
    lines: Vec<String>,
    budget: usize,
    /// References on the way from the root to the value being listed: one of them again is a cycle.
    path: HashSet<i64>,
    limits: ValueTextLimits,
    fetch_children: &'a mut dyn FnMut(i64) -> Vec<Variable>,
}

impl Walk<'_> {
    fn children(&mut self, reference: i64, depth: usize) {
        let pad = "  ".repeat(depth);
        if depth > self.limits.depth {
            self.lines.push(format!("{pad}…"));
            return;
        }
        for child in (self.fetch_children)(reference) {
            if self.budget == 0 {
                self.lines.push(format!("{pad}…"));
                return;
            }
            self.budget -= 1;
            self.lines.push(format!("{pad}{}: {}", child.name, child.value));
            let nested = child.variables_reference;
            if nested > 0 && self.path.insert(nested) {
                self.children(nested, depth + 1);
                self.path.remove(&nested);
            }
        }
    }
}

/// `value` alone for a scalar (`reference` not above 0); for an expandable value the summary line
/// followed by its children, one per line and indented by depth. Cycles and anything past the
/// limits end in an ellipsis line. `fetch_children` lists the children of a variables reference.
pub fn value_text(
    value: &str,
    reference: i64,
    fetch_children: &mut dyn FnMut(i64) -> Vec<Variable>,
    limits: ValueTextLimits,
) -> String {
    if reference <= 0 {
        return value.to_owned();
    }
    let mut walk = Walk {
        lines: vec![value.to_owned()],
        budget: limits.entries,
        path: HashSet::from([reference]),
        limits,
        fetch_children,
    };
    walk.children(reference, 1);
    walk.lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn v(name: &str, value: &str, variables_reference: i64) -> Variable {
        Variable {
            name: name.to_owned(),
            value: value.to_owned(),
            type_name: None,
            variables_reference,
            evaluate_name: None,
        }
    }

    fn text(value: &str, reference: i64, tree: &[(i64, Vec<Variable>)], limits: ValueTextLimits) -> String {
        let tree: HashMap<i64, Vec<Variable>> = tree.iter().cloned().collect();
        value_text(value, reference, &mut |reference| tree.get(&reference).cloned().unwrap_or_default(), limits)
    }

    #[test]
    fn returns_a_scalar_as_is() {
        assert_eq!(text("42", 0, &[], VALUE_TEXT_LIMITS), "42");
    }

    #[test]
    fn copies_a_php_array_with_its_nested_elements() {
        let tree = [
            (1, vec![v("name", "\"Ann\"", 0), v("tags", "array(2)", 2)]),
            (2, vec![v("0", "\"a\"", 0), v("1", "\"b\"", 0)]),
        ];
        assert_eq!(
            text("array(2)", 1, &tree, VALUE_TEXT_LIMITS),
            ["array(2)", "  name: \"Ann\"", "  tags: array(2)", "    0: \"a\"", "    1: \"b\""].join("\n"),
        );
    }

    #[test]
    fn stops_at_a_cycle() {
        let tree = [(1, vec![v("self", "Node", 1)])];
        assert_eq!(text("Node", 1, &tree, VALUE_TEXT_LIMITS), "Node\n  self: Node");
    }

    #[test]
    fn truncates_past_the_entry_limit() {
        let tree = [(1, vec![v("0", "a", 0), v("1", "b", 0), v("2", "c", 0)])];
        let limits = ValueTextLimits { depth: 8, entries: 2 };
        assert_eq!(text("arr", 1, &tree, limits), "arr\n  0: a\n  1: b\n  …");
    }

    #[test]
    fn truncates_past_the_depth_limit() {
        let tree = [(1, vec![v("a", "x", 2)]), (2, vec![v("b", "y", 3)]), (3, vec![v("c", "z", 0)])];
        let limits = ValueTextLimits { depth: 2, entries: 100 };
        assert_eq!(text("r", 1, &tree, limits), "r\n  a: x\n    b: y\n      …");
    }
}
