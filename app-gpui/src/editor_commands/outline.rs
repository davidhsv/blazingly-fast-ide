//! The outline symbols a position is the name of, for Copy Reference.

use crate::everywhere::outline::outline_path_at;
use ide_core::{OutlineItem, Pos};

/// The symbols enclosing `pos` down to the innermost one whose name (selection range) holds it,
/// outermost first; empty when `pos` is on no symbol's name.
pub fn declaration_chain(items: &[OutlineItem], pos: Pos) -> Vec<&OutlineItem> {
    let mut chain = outline_path_at(items, pos);
    let named = chain.iter().rposition(|item| item.selection_range.start <= pos && pos <= item.selection_range.end);
    chain.truncate(named.map_or(0, |index| index + 1));
    chain
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{Range, SymbolKind};

    fn range(at: [u32; 4]) -> Range {
        Range { start: Pos { line: at[0], col: at[1] }, end: Pos { line: at[2], col: at[3] } }
    }

    fn item(name: &str, extent: [u32; 4], selection: [u32; 4], children: Vec<OutlineItem>) -> OutlineItem {
        OutlineItem {
            name: name.to_owned(),
            detail: None,
            kind: SymbolKind::Class,
            range: range(extent),
            selection_range: range(selection),
            children,
            visibility: None,
        }
    }

    #[test]
    fn names_a_symbol_only_with_the_caret_on_its_name_like_idea() {
        // class Book {        line 0, name at columns 6-10
        //   void sum() {      line 1, name at columns 7-10
        //     total += 1;     line 2
        let outline =
            [item("Book", [0, 0, 4, 1], [0, 6, 0, 10], vec![item("sum", [1, 2, 3, 3], [1, 7, 1, 10], Vec::new())])];
        let names = |line: u32, col: u32| -> Vec<String> {
            declaration_chain(&outline, Pos { line, col }).into_iter().map(|item| item.name.clone()).collect()
        };
        assert_eq!(names(1, 8), ["Book", "sum"]);
        assert_eq!(names(1, 10), ["Book", "sum"]);
        assert_eq!(names(0, 6), ["Book"]);
        assert!(names(2, 6).is_empty());
        assert!(names(1, 2).is_empty());
        assert!(names(5, 0).is_empty());
    }
}
