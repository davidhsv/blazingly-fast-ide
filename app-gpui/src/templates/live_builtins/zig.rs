//! The Zig live templates: functions, `main` and test blocks, the container types, the control
//! flow with Zig's parenthesised conditions and `|capture|`s, and `std.debug.print`.

use super::{def, dflt, group, var};
use crate::templates::live_model::LiveTemplate;

/// The "Zig" group, in the order the settings page lists it.
pub(super) fn zig() -> Vec<LiveTemplate> {
    group(
        "Zig",
        "zig",
        &["zig"],
        vec![
            def("fn", "Function", "fn $NAME$($PARAMS$) $RET$ {\n\t$END$\n}").vars(vec![dflt("RET", "\"void\"")]),
            def("pfn", "Public function", "pub fn $NAME$($PARAMS$) $RET$ {\n\t$END$\n}")
                .vars(vec![dflt("RET", "\"void\"")]),
            def("main", "main function", "pub fn main() !void {\n\t$END$\n}"),
            def("test", "Test block", "test \"$NAME$\" {\n\t$END$\n}").vars(vec![dflt("NAME", "\"it works\"")]),
            def("st", "Struct type", "pub const $NAME$ = struct {\n\t$END$\n};"),
            def("en", "Enum type", "pub const $NAME$ = enum {\n\t$END$\n};"),
            def("un", "Tagged union type", "pub const $NAME$ = union(enum) {\n\t$END$\n};"),
            def("if", "if statement", "if ($COND$) {\n\t$END$\n}"),
            def("ife", "if / else", "if ($COND$) {\n\t$END$\n} else {\n\t\n}"),
            def("iferr", "if result / error", "if ($EXPR$) |$VAL$| {\n\t$END$\n} else |$ERR$| {\n\t\n}")
                .vars(vec![dflt("VAL", "\"value\""), dflt("ERR", "\"err\"")]),
            def("while", "while loop", "while ($COND$) {\n\t$END$\n}"),
            def("for", "for loop", "for ($ITER$) |$ITEM$| {\n\t$END$\n}")
                .vars(vec![var("ITER"), dflt("ITEM", "\"item\"")]),
            def("defer", "defer block", "defer {\n\t$END$\n}"),
            def("const", "const declaration", "const $NAME$ = $END$;"),
            def("var", "var declaration", "var $NAME$ = $END$;"),
            def("try", "try expression", "try $END$;"),
            def("catch", "catch block", "$EXPR$ catch {\n\t$END$\n}"),
            def("sw", "switch expression", "switch ($EXPR$) {\n\t$END$\n}"),
            def("p", "std.debug.print", "std.debug.print(\"{any}\\n\", .{$END$});"),
        ],
    )
}
