//! Language surrounders for Surround With: the fixed `if` / `while` / `try` … wrappers IDEA lists
//! above the `$SELECTION$` live templates. Bodies use the live template syntax. Statement
//! surrounders wrap whole lines; expression surrounders wrap a selection within a line.

use super::live_builtins::TS_LANGS;
use super::live_model::{LiveTemplate, TemplateVariable};
use std::collections::BTreeSet;
use std::sync::LazyLock;

/// What a surrounder wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SurrounderKind {
    Statement,
    Expression,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surrounder {
    /// `<set>.<position in the set>`.
    pub id: String,
    pub description: String,
    pub body: String,
    pub variables: Vec<TemplateVariable>,
    pub kind: SurrounderKind,
}

/// A surrounder before its set numbers it.
struct Def {
    description: String,
    body: String,
    variables: Vec<TemplateVariable>,
    kind: SurrounderKind,
}

fn statement(description: &str, body: &str) -> Def {
    Def {
        description: description.to_owned(),
        body: body.to_owned(),
        variables: Vec::new(),
        kind: SurrounderKind::Statement,
    }
}

fn expression(description: &str, body: &str) -> Def {
    Def { kind: SurrounderKind::Expression, ..statement(description, body) }
}

impl Def {
    /// Gives the variable `name` a default value.
    fn with_default(mut self, name: &str, default_value: &str) -> Self {
        self.variables.push(TemplateVariable::new(name).default_value(default_value));
        self
    }
}

fn set(key: &str, defs: Vec<Def>) -> Vec<Surrounder> {
    defs.into_iter()
        .enumerate()
        .map(|(i, d)| Surrounder {
            id: format!("{key}.{i}"),
            description: d.description,
            body: d.body,
            variables: d.variables,
            kind: d.kind,
        })
        .collect()
}

/// `head { selection }`.
fn brace_block(description: &str, head: &str) -> Def {
    statement(description, &format!("{head} {{\n\t$SELECTION$\n}}$END$"))
}

const IF_ELSE_PARENS: &str = "if ($COND$) {\n\t$SELECTION$\n} else {\n\t$END$\n}";
const IF_ELSE_BARE: &str = "if $COND$ {\n\t$SELECTION$\n} else {\n\t$END$\n}";
const BLOCK: &str = "{\n\t$SELECTION$\n}$END$";
const TRY_FINALLY: &str = "try {\n\t$SELECTION$\n} finally {\n\t$END$\n}";

/// Region comments written `<comment>region`, or with a space after the comment sign.
fn region(comment: &str, spaced: bool) -> Def {
    let lead = if spaced { format!("{comment} ") } else { comment.to_owned() };
    statement(
        &format!("{lead}region...{lead}endregion Comments"),
        &format!("{lead}region $DESC$\n$SELECTION$\n{lead}endregion$END$"),
    )
    .with_default("DESC", "\"Description\"")
}

fn parens() -> Def {
    expression("(expr)", "($SELECTION$)$END$")
}

fn not_parens() -> Def {
    expression("!(expr)", "!($SELECTION$)$END$")
}

fn java() -> Vec<Surrounder> {
    set(
        "java",
        vec![
            brace_block("if", "if ($COND$)"),
            statement("if / else", IF_ELSE_PARENS),
            brace_block("while", "while ($COND$)"),
            statement("do / while", "do {\n\t$SELECTION$\n} while ($COND$);$END$"),
            brace_block("for", "for ($INIT$; $COND$; $UPDATE$)"),
            statement("try / catch", "try {\n\t$SELECTION$\n} catch ($EXC$ e) {\n\t$END$\n}")
                .with_default("EXC", "\"Exception\""),
            statement("try / finally", TRY_FINALLY),
            statement("try / catch / finally", "try {\n\t$SELECTION$\n} catch ($EXC$ e) {\n\t$END$\n} finally {\n}")
                .with_default("EXC", "\"Exception\""),
            brace_block("synchronized", "synchronized ($LOCK$)").with_default("LOCK", "\"this\""),
            statement(
                "Runnable",
                "Runnable $NAME$ = new Runnable() {\n\tpublic void run() {\n\t\t$SELECTION$\n\t}\n};$END$",
            )
            .with_default("NAME", "\"runnable\""),
            statement("{ }", BLOCK),
            region("//", false),
            parens(),
            expression("((Type)expr)", "(($TYPE$) $SELECTION$)$END$"),
            not_parens(),
        ],
    )
}

fn kotlin() -> Vec<Surrounder> {
    set(
        "kotlin",
        vec![
            brace_block("if", "if ($COND$)"),
            statement("if / else", IF_ELSE_PARENS),
            brace_block("while", "while ($COND$)"),
            statement("do / while", "do {\n\t$SELECTION$\n} while ($COND$)$END$"),
            brace_block("for", "for ($ITEM$ in $ITER$)"),
            statement("try / catch", "try {\n\t$SELECTION$\n} catch (e: $EXC$) {\n\t$END$\n}")
                .with_default("EXC", "\"Exception\""),
            statement("try / finally", TRY_FINALLY),
            statement("try / catch / finally", "try {\n\t$SELECTION$\n} catch (e: $EXC$) {\n\t$END$\n} finally {\n}")
                .with_default("EXC", "\"Exception\""),
            statement("{ } (function literal)", BLOCK),
            brace_block("run { }", "run"),
            region("//", false),
            parens(),
            not_parens(),
        ],
    )
}

fn js() -> Vec<Surrounder> {
    set(
        "js",
        vec![
            brace_block("if", "if ($COND$)"),
            statement("if / else", IF_ELSE_PARENS),
            brace_block("while", "while ($COND$)"),
            statement("do / while", "do {\n\t$SELECTION$\n} while ($COND$);$END$"),
            brace_block("for", "for (let $I$ = 0; $I$ < $N$; $I$++)").with_default("I", "\"i\""),
            brace_block("for...in", "for (const $KEY$ in $OBJ$)").with_default("KEY", "\"key\""),
            brace_block("for...of", "for (const $ITEM$ of $ITER$)").with_default("ITEM", "\"item\""),
            statement("try / catch", "try {\n\t$SELECTION$\n} catch (e) {\n\t$END$\n}"),
            statement("try / finally", TRY_FINALLY),
            statement("try / catch / finally", "try {\n\t$SELECTION$\n} catch (e) {\n\t$END$\n} finally {\n}"),
            statement("{ }", BLOCK),
            brace_block("function", "function $NAME$()"),
            region("//", false),
            parens(),
            not_parens(),
        ],
    )
}

fn python() -> Vec<Surrounder> {
    set(
        "python",
        vec![
            statement("if", "if $COND$:\n\t$SELECTION$\n$END$"),
            statement("if / else", "if $COND$:\n\t$SELECTION$\nelse:\n\t$END$"),
            statement("while", "while $COND$:\n\t$SELECTION$\n$END$"),
            statement("for", "for $ITEM$ in $ITER$:\n\t$SELECTION$\n$END$"),
            statement("try / except", "try:\n\t$SELECTION$\nexcept $EXC$:\n\t$END$")
                .with_default("EXC", "\"Exception\""),
            statement("try / finally", "try:\n\t$SELECTION$\nfinally:\n\t$END$"),
            statement("try / except / finally", "try:\n\t$SELECTION$\nexcept $EXC$:\n\t$END$\nfinally:\n\tpass")
                .with_default("EXC", "\"Exception\""),
            statement("with", "with $EXPR$:\n\t$SELECTION$\n$END$"),
            statement("def", "def $NAME$():\n\t$SELECTION$\n$END$"),
            region("#", true),
            parens(),
            expression("not (expr)", "not ($SELECTION$)$END$"),
        ],
    )
}

fn go() -> Vec<Surrounder> {
    set(
        "go",
        vec![
            brace_block("if", "if $COND$"),
            statement("if / else", IF_ELSE_BARE),
            brace_block("for", "for $COND$"),
            statement("func", "func() {\n\t$SELECTION$\n}()$END$"),
            statement("{ }", BLOCK),
            region("//", true),
            parens(),
            not_parens(),
        ],
    )
}

fn rust() -> Vec<Surrounder> {
    set(
        "rust",
        vec![
            brace_block("if", "if $COND$"),
            statement("if / else", IF_ELSE_BARE),
            brace_block("if let", "if let $PAT$ = $EXPR$"),
            brace_block("while", "while $COND$"),
            brace_block("loop", "loop"),
            brace_block("for", "for $ITEM$ in $ITER$"),
            statement("{ }", BLOCK),
            brace_block("unsafe { }", "unsafe"),
            region("//", true),
            parens(),
            not_parens(),
        ],
    )
}

/// The C-family languages without a set of their own.
fn c_like() -> Vec<Surrounder> {
    set(
        "c",
        vec![
            brace_block("if", "if ($COND$)"),
            statement("if / else", IF_ELSE_PARENS),
            brace_block("while", "while ($COND$)"),
            statement("do / while", "do {\n\t$SELECTION$\n} while ($COND$);$END$"),
            statement("{ }", BLOCK),
            parens(),
            not_parens(),
        ],
    )
}

const C_LIKE_LANGS: [&str; 8] = ["c", "cpp", "csharp", "swift", "scala", "dart", "php", "objective-c"];

/// Every surrounder of `language`, statement and expression ones in the order they are listed.
fn all_for(language: &str) -> &'static [Surrounder] {
    static JAVA: LazyLock<Vec<Surrounder>> = LazyLock::new(java);
    static KOTLIN: LazyLock<Vec<Surrounder>> = LazyLock::new(kotlin);
    static PYTHON: LazyLock<Vec<Surrounder>> = LazyLock::new(python);
    static GO: LazyLock<Vec<Surrounder>> = LazyLock::new(go);
    static RUST: LazyLock<Vec<Surrounder>> = LazyLock::new(rust);
    static JS: LazyLock<Vec<Surrounder>> = LazyLock::new(js);
    static C_LIKE: LazyLock<Vec<Surrounder>> = LazyLock::new(c_like);
    match language {
        "java" => JAVA.as_slice(),
        "kotlin" => KOTLIN.as_slice(),
        "python" => PYTHON.as_slice(),
        "go" => GO.as_slice(),
        "rust" => RUST.as_slice(),
        _ if TS_LANGS.contains(&language) => JS.as_slice(),
        _ if C_LIKE_LANGS.contains(&language) => C_LIKE.as_slice(),
        _ => &[],
    }
}

/// What Surround With is about to wrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurroundSelection {
    /// The text covers whole lines (or is the statement / line at a caret).
    pub lines: bool,
    /// The text is a non-empty part of a single line.
    pub inline: bool,
}

/// The surrounders of `language` that fit the selection: statement ones, then expression ones.
pub fn surrounders_for(language: &str, selection: SurroundSelection) -> Vec<&'static Surrounder> {
    let all = all_for(language);
    let of_kind = |kind: SurrounderKind, wanted: bool| all.iter().filter(move |s| wanted && s.kind == kind);
    of_kind(SurrounderKind::Statement, selection.lines || !selection.inline)
        .chain(of_kind(SurrounderKind::Expression, selection.inline))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurroundEntryKind {
    Surrounder,
    Template,
}

/// One entry of the Surround With popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurroundEntry {
    pub kind: SurroundEntryKind,
    pub id: String,
    /// The character that picks the entry.
    pub mnemonic: Option<String>,
    /// For templates: where in the abbreviation (UTF-16) the mnemonic is underlined.
    pub mnemonic_index: Option<u32>,
}

/// The popup's entries with IDEA's mnemonics: surrounders get 1–9, 0, then A–Z in order; each
/// template the first character of its abbreviation not taken yet (none when all are).
pub fn surround_entries(surrounders: &[&Surrounder], templates: &[&LiveTemplate]) -> Vec<SurroundEntry> {
    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<SurroundEntry> = Vec::with_capacity(surrounders.len() + templates.len());
    for (i, surrounder) in surrounders.iter().enumerate() {
        let mnemonic = match i {
            0..=8 => (i + 1).to_string(),
            9 => "0".to_owned(),
            _ => char::from_u32(b'A' as u32 + (i - 10) as u32).map(String::from).unwrap_or_default(),
        };
        used.insert(mnemonic.to_uppercase());
        out.push(SurroundEntry {
            kind: SurroundEntryKind::Surrounder,
            id: surrounder.id.clone(),
            mnemonic: Some(mnemonic),
            mnemonic_index: None,
        });
    }
    for template in templates {
        let mut index = 0u32;
        let mut found = None;
        for c in template.abbrev.chars() {
            if used.insert(c.to_uppercase().collect()) {
                found = Some((c.to_string(), index));
                break;
            }
            index += c.len_utf16() as u32;
        }
        out.push(SurroundEntry {
            kind: SurroundEntryKind::Template,
            id: template.id.clone(),
            mnemonic: found.as_ref().map(|(mnemonic, _)| mnemonic.clone()),
            mnemonic_index: found.map(|(_, index)| index),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINES: SurroundSelection = SurroundSelection { lines: true, inline: false };

    fn template(id: &str, abbrev: &str) -> LiveTemplate {
        LiveTemplate {
            id: id.to_owned(),
            abbrev: abbrev.to_owned(),
            group: "User".to_owned(),
            body: "$SELECTION$".to_owned(),
            contexts: vec!["java".to_owned()],
            ..LiveTemplate::default()
        }
    }

    // surround with entries › numbers surrounders 1-9, 0, then letters
    #[test]
    fn numbers_surrounders_1_to_9_0_then_letters() {
        let java = surrounders_for("java", LINES);
        assert!(java.len() > 10);
        let entries = surround_entries(&java, &[]);
        let mnemonics: Vec<&str> = entries.iter().take(12).filter_map(|e| e.mnemonic.as_deref()).collect();
        assert_eq!(mnemonics, ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "A", "B"]);
    }

    // surround with entries › gives templates the first abbreviation character not taken yet
    #[test]
    fn gives_templates_the_first_abbreviation_character_not_taken_yet() {
        // 1-9, 0, A
        let surrounders: Vec<&Surrounder> = surrounders_for("java", LINES).into_iter().take(11).collect();
        let templates = [template("t1", "T"), template("t2", "at"), template("t3", "ta")];
        let entries = surround_entries(&surrounders, &templates.iter().collect::<Vec<_>>());
        let given: Vec<(Option<&str>, Option<u32>)> =
            entries.iter().skip(11).map(|e| (e.mnemonic.as_deref(), e.mnemonic_index)).collect();
        assert_eq!(given, [(Some("T"), Some(0)), (None, None), (None, None)]);
    }

    // surround with entries › filters statement and expression surrounders by the selection
    #[test]
    fn filters_statement_and_expression_surrounders_by_the_selection() {
        let kinds = |lines: bool, inline: bool| -> BTreeSet<SurrounderKind> {
            surrounders_for("typescript", SurroundSelection { lines, inline }).iter().map(|s| s.kind).collect()
        };
        assert_eq!(kinds(true, false), BTreeSet::from([SurrounderKind::Statement]));
        assert_eq!(kinds(false, true), BTreeSet::from([SurrounderKind::Expression]));
        assert_eq!(kinds(true, true), BTreeSet::from([SurrounderKind::Statement, SurrounderKind::Expression]));
        assert!(surrounders_for("plaintext", LINES).is_empty());
    }

    // Not from a spec: ids number a set's surrounders, and a selection that is neither whole lines
    // nor inline (several partial lines) is offered the statement surrounders.
    #[test]
    fn ids_number_the_set_and_partial_lines_get_statement_surrounders() {
        let java = surrounders_for("java", SurroundSelection { lines: false, inline: false });
        assert_eq!(java.first().map(|s| s.id.as_str()), Some("java.0"));
        assert!(java.iter().all(|s| s.kind == SurrounderKind::Statement));
        assert_eq!(java.get(8).map(|s| s.body.as_str()), Some("synchronized ($LOCK$) {\n\t$SELECTION$\n}$END$"));
        let php = surrounders_for("php", SurroundSelection { lines: false, inline: true });
        assert_eq!(php.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["c.5", "c.6"]);
        let region = surrounders_for("python", LINES).into_iter().find(|s| s.id == "python.9").expect("the region");
        assert_eq!(region.description, "# region...# endregion Comments");
        assert_eq!(region.body, "# region $DESC$\n$SELECTION$\n# endregion$END$");
    }
}
