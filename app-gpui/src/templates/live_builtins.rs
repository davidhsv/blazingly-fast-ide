//! The built-in live templates: IDEA-style sets for the major languages, the SQL set of the query
//! console, the comment templates that work everywhere and the `$SELECTION$` templates Surround
//! With lists after the language surrounders.

use super::live_model::{LiveTemplate, TemplateScope, TemplateVariable};
use std::collections::BTreeSet;
use std::sync::LazyLock;

/// The languages sharing the JavaScript / TypeScript templates.
pub const TS_LANGS: [&str; 6] = ["typescript", "typescriptreact", "javascript", "javascriptreact", "vue", "svelte"];

/// A template before its group gives it an id, a group name and contexts.
struct Def {
    /// The id's last part when it is not the abbreviation.
    id: Option<&'static str>,
    abbrev: &'static str,
    description: String,
    body: String,
    variables: Vec<TemplateVariable>,
    scope: Option<TemplateScope>,
}

fn def(abbrev: &'static str, description: &str, body: &str) -> Def {
    let (description, body) = (description.to_owned(), body.to_owned());
    Def { id: None, abbrev, description, body, variables: Vec::new(), scope: None }
}

impl Def {
    fn vars(mut self, variables: Vec<TemplateVariable>) -> Self {
        self.variables = variables;
        self
    }

    fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// Expands inside strings and comments too.
    fn anywhere(mut self) -> Self {
        self.scope = Some(TemplateScope::Any);
        self
    }
}

/// A variable that is a plain stop.
fn var(name: &str) -> TemplateVariable {
    TemplateVariable::new(name)
}

/// A variable with a default value (an expression: a string literal is written with its quotes).
fn dflt(name: &str, default_value: &str) -> TemplateVariable {
    TemplateVariable::new(name).default_value(default_value)
}

/// A variable filled by an expression.
fn expr(name: &str, expression: &str) -> TemplateVariable {
    TemplateVariable::new(name).expression(expression)
}

/// A variable whose expression, when it has a value, is inserted without a stop.
fn fixed(name: &str, expression: &str) -> TemplateVariable {
    TemplateVariable::new(name).expression(expression).skip_if_defined()
}

fn group(name: &str, key: &str, contexts: &[&str], defs: Vec<Def>) -> Vec<LiveTemplate> {
    defs.into_iter()
        .map(|d| LiveTemplate {
            plugin_origin: None,
            id: format!("{key}.{}", d.id.unwrap_or(d.abbrev)),
            group: name.to_owned(),
            abbrev: d.abbrev.to_owned(),
            description: d.description,
            body: d.body,
            contexts: contexts.iter().map(|&context| context.to_owned()).collect(),
            variables: d.variables,
            enabled: None,
            scope: d.scope,
        })
        .collect()
}

fn rust() -> Vec<LiveTemplate> {
    group(
        "Rust",
        "rust",
        &["rust"],
        vec![
            def("fn", "Function", "fn $NAME$($PARAMS$) {\n\t$END$\n}"),
            def("pfn", "Public function", "pub fn $NAME$($PARAMS$) {\n\t$END$\n}"),
            def("afn", "Async function", "async fn $NAME$($PARAMS$) {\n\t$END$\n}"),
            def("fnr", "Function with return type", "fn $NAME$($PARAMS$) -> $RET$ {\n\t$END$\n}"),
            def("main", "main function", "fn main() {\n\t$END$\n}"),
            def("test", "Test function", "#[test]\nfn $NAME$() {\n\t$END$\n}").vars(vec![dflt("NAME", "\"it_works\"")]),
            def(
                "tmod",
                "Test module",
                "#[cfg(test)]\nmod tests {\n\tuse super::*;\n\n\t#[test]\n\tfn $NAME$() {\n\t\t$END$\n\t}\n}",
            )
            .vars(vec![dflt("NAME", "\"it_works\"")]),
            def("impl", "impl block", "impl $TYPE$ {\n\t$END$\n}"),
            def("implt", "Trait implementation", "impl $TRAIT$ for $TYPE$ {\n\t$END$\n}"),
            def("st", "Struct", "struct $NAME$ {\n\t$END$\n}"),
            def("en", "Enum", "enum $NAME$ {\n\t$END$\n}"),
            def("tr", "Trait", "trait $NAME$ {\n\t$END$\n}"),
            def("match", "match expression", "match $EXPR$ {\n\t$PAT$ => $END$,\n}"),
            def("ifl", "if let", "if let $PAT$ = $EXPR$ {\n\t$END$\n}"),
            def("whilel", "while let", "while let $PAT$ = $EXPR$ {\n\t$END$\n}"),
            def("for", "for loop", "for $ITEM$ in $ITER$ {\n\t$END$\n}"),
            def("loop", "loop", "loop {\n\t$END$\n}"),
            def("let", "let binding", "let $NAME$ = $END$;"),
            def("letm", "let mut binding", "let mut $NAME$ = $END$;"),
            def("derive", "#[derive(...)]", "#[derive($TRAITS$)]").vars(vec![dflt("TRAITS", "\"Debug, Clone\"")]),
            def("p", "println!", "println!(\"$END$\");"),
            def("pd", "println! with Debug", "println!(\"{:?}\", $END$);"),
            def("ep", "eprintln!", "eprintln!(\"$END$\");"),
            def("mod", "Module", "mod $NAME$ {\n\t$END$\n}"),
        ],
    )
}

fn ts() -> Vec<LiveTemplate> {
    group(
        "JavaScript / TypeScript",
        "ts",
        &TS_LANGS,
        vec![
            def("log", "console.log", "console.log($END$);"),
            def("logv", "console.log a value with its name", "console.log(\"$EXPR$:\", $EXPR$);"),
            def("fori", "Indexed for loop", "for (let $I$ = 0; $I$ < $ARR$.length; $I$++) {\n\t$END$\n}")
                .vars(vec![dflt("I", "\"i\"")]),
            def("iter", "for…of loop", "for (const $ITEM$ of $ITER$) {\n\t$END$\n}")
                .vars(vec![var("ITER"), expr("ITEM", "suggestVariableName()").default_value("\"item\"")]),
            def("itin", "for…in loop", "for (const $KEY$ in $OBJ$) {\n\t$END$\n}")
                .vars(vec![var("OBJ"), dflt("KEY", "\"key\"")]),
            def("arrow", "Arrow function", "($PARAMS$) => {\n\t$END$\n}"),
            def("fun", "Function", "function $NAME$($PARAMS$) {\n\t$END$\n}"),
            def("afun", "Async function", "async function $NAME$($PARAMS$) {\n\t$END$\n}"),
            def("imp", "import { … } from", "import { $NAMES$ } from \"$MODULE$\";$END$")
                .vars(vec![var("MODULE"), var("NAMES")]),
            def("impd", "import default", "import $NAME$ from \"$MODULE$\";$END$")
                .vars(vec![var("MODULE"), var("NAME")]),
            def("describe", "Test suite", "describe(\"$NAME$\", () => {\n\t$END$\n});"),
            def("it", "Test case", "it(\"$NAME$\", () => {\n\t$END$\n});"),
            def("ita", "Async test case", "it(\"$NAME$\", async () => {\n\t$END$\n});"),
            def("test", "Test case (test)", "test(\"$NAME$\", () => {\n\t$END$\n});"),
            def("cls", "Class", "class $NAME$ {\n\tconstructor($PARAMS$) {\n\t\t$END$\n\t}\n}"),
            def("try", "try / catch", "try {\n\t$END$\n} catch (e) {\n\t\n}"),
            def("if", "if statement", "if ($COND$) {\n\t$END$\n}"),
            def("ife", "if / else", "if ($COND$) {\n\t$END$\n} else {\n\t\n}"),
            def("const", "const declaration", "const $NAME$ = $END$;"),
            def("useState", "React useState hook", "const [$NAME$, set$SETTER$] = useState($INIT$);$END$"),
            def("useEffect", "React useEffect hook", "useEffect(() => {\n\t$END$\n}, [$DEPS$]);"),
        ],
    )
}

fn python() -> Vec<LiveTemplate> {
    group(
        "Python",
        "py",
        &["python"],
        vec![
            def(
                "main",
                "main function with __main__ guard",
                "def main():\n\t$END$\n\n\nif __name__ == \"__main__\":\n\tmain()",
            ),
            def("ifmain", "if __name__ == '__main__'", "if __name__ == \"__main__\":\n\t$END$"),
            def("def", "Function", "def $NAME$($PARAMS$):\n\t$END$"),
            def("adef", "Async function", "async def $NAME$($PARAMS$):\n\t$END$"),
            def("defs", "Method", "def $NAME$(self$PARAMS$):\n\t$END$"),
            def("class", "Class", "class $NAME$:\n\tdef __init__(self$PARAMS$):\n\t\t$END$"),
            def("with", "with statement", "with $EXPR$ as $NAME$:\n\t$END$"),
            def("try", "try / except", "try:\n\t$END$\nexcept $EXC$ as e:\n\traise")
                .vars(vec![dflt("EXC", "\"Exception\"")]),
            def("for", "for loop", "for $ITEM$ in $ITER$:\n\t$END$"),
            def("fori", "for i in range", "for $I$ in range($N$):\n\t$END$").vars(vec![dflt("I", "\"i\""), var("N")]),
            def("prop", "Property", "@property\ndef $NAME$(self):\n\treturn self._$NAME$$END$"),
            def("p", "print", "print($END$)"),
            def("test", "pytest test function", "def test_$NAME$():\n\t$END$"),
        ],
    )
}

fn go() -> Vec<LiveTemplate> {
    group(
        "Go",
        "go",
        &["go"],
        vec![
            def("iferr", "if err != nil { return … }", "if err != nil {\n\treturn $RET$err\n}$END$"),
            def("fori", "Indexed for loop", "for $I$ := 0; $I$ < $N$; $I$++ {\n\t$END$\n}")
                .vars(vec![dflt("I", "\"i\""), var("N")]),
            def("forr", "for range loop", "for $K$, $V$ := range $X$ {\n\t$END$\n}").vars(vec![
                var("X"),
                dflt("K", "\"_\""),
                dflt("V", "\"v\""),
            ]),
            def("func", "Function", "func $NAME$($PARAMS$) $RET$ {\n\t$END$\n}"),
            def("meth", "Method", "func ($R$ $TYPE$) $NAME$($PARAMS$) $RET$ {\n\t$END$\n}"),
            def("main", "main function", "func main() {\n\t$END$\n}"),
            def("test", "Test function", "func Test$NAME$(t *testing.T) {\n\t$END$\n}"),
            def(
                "bench",
                "Benchmark function",
                "func Benchmark$NAME$(b *testing.B) {\n\tfor i := 0; i < b.N; i++ {\n\t\t$END$\n\t}\n}",
            ),
            def("st", "Struct type", "type $NAME$ struct {\n\t$END$\n}"),
            def("inf", "Interface type", "type $NAME$ interface {\n\t$END$\n}"),
            def("p", "fmt.Println", "fmt.Println($END$)"),
            def("pf", "fmt.Printf", "fmt.Printf(\"$FMT$\\n\", $END$)"),
            def("go", "goroutine", "go func() {\n\t$END$\n}()"),
        ],
    )
}

fn java() -> Vec<LiveTemplate> {
    group(
        "Java",
        "java",
        &["java"],
        vec![
            def("psvm", "main() method", "public static void main(String[] args) {\n\t$END$\n}"),
            def("main", "main() method", "public static void main(String[] args) {\n\t$END$\n}"),
            def("sout", "System.out.println", "System.out.println($END$);"),
            def("souf", "System.out.printf", "System.out.printf(\"$END$\");"),
            def("serr", "System.err.println", "System.err.println($END$);"),
            def("soutm", "Print class and method name", "System.out.println(\"$CLASS$.$METHOD$\");")
                .vars(vec![fixed("CLASS", "className()"), fixed("METHOD", "methodName()")]),
            def("fori", "Indexed for loop", "for (int $I$ = 0; $I$ < $N$; $I$++) {\n\t$END$\n}")
                .vars(vec![dflt("I", "\"i\""), var("N")]),
            def("iter", "Enhanced for loop", "for ($TYPE$ $ITEM$ : $ITER$) {\n\t$END$\n}").vars(vec![
                var("ITER"),
                dflt("TYPE", "\"var\""),
                expr("ITEM", "suggestVariableName()").default_value("\"item\""),
            ]),
            def("psf", "public static final", "public static final $END$"),
            def("psfs", "public static final String", "public static final String $END$"),
            def("prsf", "private static final", "private static final $END$"),
            def("ifn", "if (x == null)", "if ($VAR$ == null) {\n\t$END$\n}"),
            def("inn", "if (x != null)", "if ($VAR$ != null) {\n\t$END$\n}"),
            def("thr", "throw new", "throw new $END$"),
            def("try", "try / catch", "try {\n\t$END$\n} catch ($EXC$ e) {\n\tthrow new RuntimeException(e);\n}")
                .vars(vec![dflt("EXC", "\"Exception\"")]),
        ],
    )
}

fn kotlin() -> Vec<LiveTemplate> {
    group(
        "Kotlin",
        "kt",
        &["kotlin"],
        vec![
            def("main", "main() function", "fun main() {\n\t$END$\n}"),
            def("maina", "main(args) function", "fun main(args: Array<String>) {\n\t$END$\n}"),
            def("fun", "Function", "fun $NAME$($PARAMS$) {\n\t$END$\n}"),
            def("fun1", "Function with return type", "fun $NAME$($PARAMS$): $RET$ {\n\t$END$\n}"),
            def("val", "val declaration", "val $NAME$ = $END$"),
            def("var", "var declaration", "var $NAME$ = $END$"),
            def("sout", "println", "println($END$)"),
            def("serr", "System.err.println", "System.err.println($END$)"),
            def("fori", "for (i in 0 until n)", "for ($I$ in 0 until $N$) {\n\t$END$\n}")
                .vars(vec![dflt("I", "\"i\""), var("N")]),
            def("iter", "for (item in items)", "for ($ITEM$ in $ITER$) {\n\t$END$\n}")
                .vars(vec![var("ITER"), expr("ITEM", "suggestVariableName()").default_value("\"item\"")]),
            def("ifn", "if (x == null)", "if ($VAR$ == null) {\n\t$END$\n}"),
            def("inn", "if (x != null)", "if ($VAR$ != null) {\n\t$END$\n}"),
            def("cls", "Class", "class $NAME$ {\n\t$END$\n}"),
            def("dcls", "Data class", "data class $NAME$($PARAMS$)$END$"),
            def("obj", "Object", "object $NAME$ {\n\t$END$\n}"),
            def("comp", "Companion object", "companion object {\n\t$END$\n}"),
            def("test", "JUnit test", "@Test\nfun $NAME$() {\n\t$END$\n}"),
        ],
    )
}

/// A PHP method template with the given modifiers.
fn php_method(abbrev: &'static str, modifiers: &str) -> Def {
    def(
        abbrev,
        &format!("{modifiers} function"),
        &format!("{modifiers} function $NAME$($PARAMETERS$)\n{{\n\t$END$\n}}"),
    )
}

/// PHP's `$` is written `$$` in a template body.
fn php() -> Vec<LiveTemplate> {
    group(
        "PHP",
        "php",
        &["php"],
        vec![
            def("fore", "foreach (iterable as value)", "foreach ($ITERABLE$ as $$$VALUE$) {\n\t$END$\n}")
                .vars(vec![var("ITERABLE"), dflt("VALUE", "\"value\"")]),
            def(
                "forek",
                "foreach (iterable as key => value)",
                "foreach ($ITERABLE$ as $$$KEY$ => $$$VALUE$) {\n\t$END$\n}",
            )
            .vars(vec![var("ITERABLE"), dflt("KEY", "\"key\""), dflt("VALUE", "\"value\"")]),
            php_method("pubf", "public"),
            php_method("prof", "protected"),
            php_method("prif", "private"),
            php_method("pubsf", "public static"),
            php_method("prosf", "protected static"),
            php_method("prisf", "private static"),
            def("thr", "throw new", "throw new $END$;"),
            def("eco", "echo", "echo \"$END$\";"),
            def("inc", "include", "include '$END$';"),
            def("inco", "include_once", "include_once '$END$';"),
            def("rqr", "require", "require '$END$';"),
            def("rqro", "require_once", "require_once '$END$';"),
        ],
    )
}

fn general() -> Vec<LiveTemplate> {
    let comment = |abbrev: &'static str, word: &str| {
        def(abbrev, &format!("{word} comment"), &format!("$C${word}: $END$$CE$"))
            .anywhere()
            .vars(vec![fixed("C", "commentStart()"), fixed("CE", "commentEnd()")])
    };
    group(
        "General",
        "any",
        &["any"],
        vec![
            comment("todo", "TODO"),
            comment("fixme", "FIXME"),
            comment("hack", "HACK"),
            def("date", "Current date", "$D$$END$").anywhere().vars(vec![fixed("D", "date()")]),
        ],
    )
}

/// The `$SELECTION$` templates Surround With offers after the language surrounders.
fn surround() -> Vec<LiveTemplate> {
    let mut out = group(
        "Surround",
        "surround.html",
        &["html", "xml", "vue", "svelte", "markdown"],
        vec![
            def("T", "Surround with <tag></tag>", "<$TAG$>$SELECTION$</$TAG$>$END$")
                .id("tag")
                .vars(vec![dflt("TAG", "\"div\"")]),
        ],
    );
    out.extend(group(
        "Surround",
        "surround.java",
        &["java"],
        vec![
            def(
                "C",
                "Surround with Callable",
                "java.util.concurrent.Callable<$RET$> callable = new java.util.concurrent.Callable<$RET$>() {\n\
                 \tpublic $RET$ call() throws Exception {\n\t\t$SELECTION$\n\t\t$END$\n\t}\n};",
            )
            .id("callable")
            .vars(vec![dflt("RET", "\"Object\"")]),
            def(
                "RL",
                "Surround with ReadWriteLock.readLock",
                "$LOCK$.readLock().lock();\ntry {\n\t$SELECTION$\n} finally {\n\t$LOCK$.readLock().unlock();\n}$END$",
            )
            .id("readLock")
            .vars(vec![var("LOCK")]),
            def(
                "WL",
                "Surround with ReadWriteLock.writeLock",
                "$LOCK$.writeLock().lock();\ntry {\n\t$SELECTION$\n} finally {\n\t$LOCK$.writeLock().unlock();\n}$END$",
            )
            .id("writeLock")
            .vars(vec![var("LOCK")]),
            def("I", "Iterate Iterable or array", "for ($ELEMENT_TYPE$ $VAR$ : $SELECTION$) {\n\t$END$\n}")
                .id("iterate")
                .vars(vec![
                    dflt("ELEMENT_TYPE", "\"var\""),
                    expr("VAR", "suggestVariableName()").default_value("\"item\""),
                ]),
        ],
    ));
    out
}

/// IDEA's SQL set, written as its formatter lays the statements out. The contexts come from the
/// query console: `sql-statement` where a statement starts, `sql-query` where a query may (also
/// subqueries), `sql-code` anywhere in SQL code. `dbObjectName()` and `dbColumns()` fields open
/// the console's completion.
fn sql() -> Vec<LiveTemplate> {
    let table = || expr("table", "dbObjectName()");
    let alias = || expr("alias", "complete()").default_value("\"alias\"");
    let mut out = group(
        "SQL",
        "sql",
        &["sql-query"],
        vec![
            def("sel", "select all rows from a table", "select *\nfrom $table$$END$;").vars(vec![table()]),
            def("selw", "select specific rows from a table", "select *\nfrom $table$ $alias$\nwhere $alias$.$END$;")
                .vars(vec![expr("table", "complete()"), alias()]),
            def(
                "selc",
                "select the number of specific rows in a table",
                "select count(*)\nfrom $table$ $alias$\nwhere $alias$.$END$;",
            )
            .vars(vec![expr("table", "complete()"), alias()]),
            def("lim", "select a limited number of rows from a table", "select *\nfrom $table$\nlimit $size$$END$;")
                .vars(vec![table(), dflt("size", "\"100\"")]),
        ],
    );
    out.extend(group(
        "SQL",
        "sql",
        &["sql-statement"],
        vec![
            def("ins", "insert rows into a table", "insert into $table$ ($columns$)\nvalues ($END$);")
                .vars(vec![table(), expr("columns", "dbColumns()")]),
            def("upd", "update values in a table", "update $table$\nset $col$ = $value$\nwhere $END$;").vars(vec![
                table(),
                expr("col", "complete()"),
                var("value"),
            ]),
            def("del", "delete specific rows from a table", "delete\nfrom $table$\nwhere $END$;").vars(vec![table()]),
            def("tab", "new table definition", "create table $table$\n(\n    $col$ $type$ $null$$END$\n);").vars(vec![
                dflt("table", "\"new_table\""),
                dflt("col", "\"col\""),
                dflt("type", "\"int\""),
                dflt("null", "\"not null\""),
            ]),
            def("view", "new view definition", "create view $view$ as\nselect *\nfrom $table$$END$;")
                .vars(vec![dflt("view", "\"new_view\""), table()]),
        ],
    ));
    out.extend(group(
        "SQL",
        "sql",
        &["sql-code"],
        vec![def("col", "new column definition", "$col$ $type$ $null$$END$").vars(vec![
            dflt("col", "\"col\""),
            dflt("type", "\"int\""),
            dflt("null", "\"not null\""),
        ])],
    ));
    out
}

/// Every built-in template, in the order the settings list them.
pub fn builtin_templates() -> &'static [LiveTemplate] {
    static TEMPLATES: LazyLock<Vec<LiveTemplate>> = LazyLock::new(|| {
        [rust(), ts(), python(), go(), java(), kotlin(), php(), sql(), general(), surround()].concat()
    });
    &TEMPLATES
}

/// Whether `id` is a built-in template's.
pub fn is_builtin_id(id: &str) -> bool {
    static IDS: LazyLock<BTreeSet<&'static str>> =
        LazyLock::new(|| builtin_templates().iter().map(|t| t.id.as_str()).collect());
    IDS.contains(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::live_model::{ExpansionEnv, TemplateFilter, applicable_templates, expand_template};
    use crate::templates::snippet::testing::applied;

    fn abbrevs(language: &str, filter: TemplateFilter<'_>) -> Vec<&'static str> {
        applicable_templates(builtin_templates(), language, filter).iter().map(|t| t.abbrev.as_str()).collect()
    }

    // expansion › filters by context and scope
    #[test]
    fn filters_by_context_and_scope() {
        let ts = abbrevs("typescript", TemplateFilter::default());
        assert!(ts.contains(&"log"));
        assert!(ts.contains(&"todo"));
        assert!(!ts.contains(&"psvm"));
        let in_comment =
            abbrevs("typescript", TemplateFilter { in_comment_or_string: true, ..TemplateFilter::default() });
        assert!(in_comment.contains(&"todo"));
        assert!(!in_comment.contains(&"log"));
    }

    // expansion › ships the requested built-ins
    #[test]
    fn ships_the_requested_built_ins() {
        let has = |language: &str, abbrev: &str| abbrevs(language, TemplateFilter::default()).contains(&abbrev);
        for a in ["fn", "test", "impl", "match", "ifl", "derive"] {
            assert!(has("rust", a), "rust {a}");
        }
        for a in ["log", "fori", "iter", "arrow", "imp", "describe", "it"] {
            assert!(has("typescriptreact", a), "typescriptreact {a}");
        }
        for a in ["main", "def", "class", "ifmain", "with", "try"] {
            assert!(has("python", a), "python {a}");
        }
        for a in ["iferr", "fori", "func", "test"] {
            assert!(has("go", a), "go {a}");
        }
        for a in ["psvm", "sout", "fori", "iter", "main"] {
            assert!(has("java", a), "java {a}");
        }
        for a in ["main", "fun", "val", "sout"] {
            assert!(has("kotlin", a), "kotlin {a}");
        }
        for a in ["todo", "fixme"] {
            assert!(has("go", a), "go {a}");
        }
        for a in ["fore", "forek", "pubf", "prisf", "thr", "eco", "rqro"] {
            assert!(has("php", a), "php {a}");
        }
        assert!(!has("java", "fore"));
    }

    // expansion › writes PHP variables with their $
    #[test]
    fn writes_php_variables_with_their_dollar() {
        let expanded = |abbrev: &str| {
            let id = format!("php.{abbrev}");
            let template = builtin_templates().iter().find(|t| t.id == id).expect("a PHP template");
            let expansion = expand_template(&template.body, &template.variables, &ExpansionEnv::default());
            applied("", &expansion.snippet, 0, 0)
        };
        let (text, selection) = expanded("fore");
        assert_eq!(text, "foreach ( as $value) {\n  \n}");
        // The caret starts on the iterable.
        assert_eq!(selection.first(), Some(&(9, 9)));
        assert_eq!(expanded("forek").0, "foreach ( as $key => $value) {\n  \n}");
        assert_eq!(expanded("prosf").0, "protected static function ()\n{\n  \n}");
    }

    // Not from a spec: ids are unique, built from the group key and the abbreviation or the given id.
    #[test]
    fn ids_are_unique_and_keyed_by_group() {
        let ids: BTreeSet<&str> = builtin_templates().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids.len(), builtin_templates().len());
        assert!(is_builtin_id("rust.fn") && is_builtin_id("surround.java.readLock") && is_builtin_id("sql.col"));
        assert!(!is_builtin_id("user.x"));
    }
}
