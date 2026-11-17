//! The Zig surrounders: blocks headed by Zig's parenthesised conditions, `for` with its capture,
//! `defer` and `test`, then the region comments and the two expression wrappers.

use super::{BLOCK, IF_ELSE_PARENS, Surrounder, brace_block, not_parens, parens, region, set, statement};

/// The Zig set, numbered in this order.
pub(super) fn zig() -> Vec<Surrounder> {
    set(
        "zig",
        vec![
            brace_block("if", "if ($COND$)"),
            statement("if / else", IF_ELSE_PARENS),
            brace_block("while", "while ($COND$)"),
            brace_block("for", "for ($ITER$) |$ITEM$|").with_default("ITEM", "\"item\""),
            brace_block("defer", "defer"),
            brace_block("test", "test \"$NAME$\"").with_default("NAME", "\"name\""),
            statement("{ }", BLOCK),
            region("//", false),
            parens(),
            not_parens(),
        ],
    )
}
