//! The Zig postfix templates. Conditions are parenthesised as Zig requires; `.nn` unwraps an
//! optional into a capture (`if (expr) |item|`), and a statement ends with `;`.

use super::{PostfixTemplate, expr, item, name, set, stmt};

/// The Zig set, in the order the settings page lists it.
pub(super) fn zig() -> Vec<PostfixTemplate> {
    set(
        "zig",
        &["zig"],
        vec![
            stmt("if", "if (expr)", "if ($EXPR$) {\n\t$END$\n}"),
            expr("not", "!expr", "!$EXPR$$END$").wrap(),
            stmt("const", "const name = expr;", "const $NAME$ = $EXPR$;$END$").var(name()),
            stmt("var", "var name = expr;", "var $NAME$ = $EXPR$;$END$").var(name()),
            stmt("return", "return expr;", "return $EXPR$;$END$"),
            stmt("try", "try expr;", "try $EXPR$;$END$"),
            stmt("defer", "defer expr;", "defer $EXPR$;$END$"),
            stmt("nn", "if (expr) |item|", "if ($EXPR$) |$ITEM$| {\n\t$END$\n}").var(item()),
            stmt("null", "if (expr == null)", "if ($EXPR$ == null) {\n\t$END$\n}"),
            stmt("switch", "switch (expr)", "switch ($EXPR$) {\n\t$END$\n}"),
            stmt("print", "std.debug.print(\"{any}\\n\", .{expr})", "std.debug.print(\"{any}\\n\", .{$EXPR$});$END$"),
            expr("par", "(expr)", "($EXPR$)$END$"),
        ],
    )
}
