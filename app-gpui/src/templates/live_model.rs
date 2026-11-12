//! Live templates (IDEA syntax) and their expansion to a snippet with linked fields.
//!
//! Body syntax: `$NAME$` variables (repeated occurrences are linked), `$END$` (the final caret),
//! `$SELECTION$` (the text being surrounded), `$$` (a literal dollar). Leading tabs in body lines
//! are indent units relative to the line the template is expanded on.
//!
//! A variable may carry an expression: string literals, references to other variables and calls
//! such as `className()`, `capitalize(fileNameWithoutExtension())`, `date("yyyy-MM-dd")`,
//! `suggestVariableName()` or `complete()`.

use chrono::{Datelike, Local, NaiveDateTime, Timelike};
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

/// A variable of a template body.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateVariable {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    /// Used when the expression is empty or yields nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    /// Don't stop at the variable when the expression produced a value.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip_if_defined: bool,
}

impl TemplateVariable {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_owned(), ..Self::default() }
    }

    pub fn expression(mut self, expression: &str) -> Self {
        self.expression = Some(expression.to_owned());
        self
    }

    pub fn default_value(mut self, default_value: &str) -> Self {
        self.default_value = Some(default_value.to_owned());
        self
    }

    pub fn skip_if_defined(mut self) -> Self {
        self.skip_if_defined = true;
        self
    }
}

/// Where a template expands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TemplateScope {
    /// Not inside strings and comments.
    #[default]
    Code,
    /// Everywhere.
    Any,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTemplate {
    /// Set on a read-only contribution, which is withdrawn with its plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_origin: Option<String>,
    pub id: String,
    /// Settings grouping, e.g. "Rust", "Surround".
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub abbrev: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub body: String,
    /// Language ids (`rust`, `typescript`…) or `any`.
    #[serde(default)]
    pub contexts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<TemplateVariable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Unset is [`TemplateScope::Code`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<TemplateScope>,
}

/// A piece of a template body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Text(String),
    Var(String),
    End,
    Selection,
}

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

fn is_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn parse_body(body: &str) -> Vec<Segment> {
    fn flush(text: &mut String, out: &mut Vec<Segment>) {
        if !text.is_empty() {
            out.push(Segment::Text(std::mem::take(text)));
        }
    }
    let chars: Vec<char> = body.chars().collect();
    let mut out = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '$' {
            text.push(c);
            i += 1;
            continue;
        }
        if chars.get(i + 1) == Some(&'$') {
            text.push('$');
            i += 2;
            continue;
        }
        let close = chars[i + 1..].iter().position(|&c| c == '$').map(|at| i + 1 + at);
        let name = close.map(|close| chars[i + 1..close].iter().collect::<String>());
        let name = name.filter(|name| is_variable_name(name));
        let (Some(close), Some(name)) = (close, name) else {
            text.push(c);
            i += 1;
            continue;
        };
        flush(&mut text, &mut out);
        out.push(match name.as_str() {
            "END" => Segment::End,
            "SELECTION" => Segment::Selection,
            _ => Segment::Var(name),
        });
        i = close + 1;
    }
    flush(&mut text, &mut out);
    out
}

/// Variable names in order of first appearance (without `END` and `SELECTION`).
pub fn body_variables(body: &str) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for segment in parse_body(body) {
        if let Segment::Var(name) = segment
            && !seen.contains(&name)
        {
            seen.push(name);
        }
    }
    seen
}

/// Whether the body wraps a selection (`$SELECTION$`).
pub fn uses_selection(body: &str) -> bool {
    parse_body(body).contains(&Segment::Selection)
}

// ---------------------------------------------------------------------------
// Expressions

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Str(String),
    Ref(String),
    Call { function: String, args: Vec<Expr> },
}

struct ExprParser {
    src: Vec<char>,
    at: usize,
}

impl ExprParser {
    fn peek(&self) -> Option<char> {
        self.src.get(self.at).copied()
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }

    fn string(&mut self, quote: char) -> Option<Expr> {
        let mut value = String::new();
        self.at += 1;
        loop {
            let c = self.peek()?;
            if c == quote {
                self.at += 1;
                return Some(Expr::Str(value));
            }
            // A backslash takes the next character as it is.
            if c == '\\' && self.at + 1 < self.src.len() {
                self.at += 1;
            }
            value.push(self.src[self.at]);
            self.at += 1;
        }
    }

    fn identifier(&mut self) -> Option<String> {
        let start = self.at;
        if !self.peek().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') {
            return None;
        }
        while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            self.at += 1;
        }
        Some(self.src[start..self.at].iter().collect())
    }

    fn parse(&mut self) -> Option<Expr> {
        self.skip_whitespace();
        if let Some(quote @ ('"' | '\'')) = self.peek() {
            return self.string(quote);
        }
        let name = self.identifier()?;
        self.skip_whitespace();
        if self.peek() != Some('(') {
            return Some(Expr::Ref(name));
        }
        self.at += 1;
        let mut args = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(')') {
            self.at += 1;
            return Some(Expr::Call { function: name, args });
        }
        loop {
            args.push(self.parse()?);
            self.skip_whitespace();
            match self.peek() {
                Some(',') => self.at += 1,
                Some(')') => {
                    self.at += 1;
                    return Some(Expr::Call { function: name, args });
                }
                _ => return None,
            }
        }
    }
}

/// The expression `src` spells, or `None` when it is not one.
pub fn parse_expression(src: &str) -> Option<Expr> {
    let mut parser = ExprParser { src: src.chars().collect(), at: 0 };
    let expr = parser.parse()?;
    parser.skip_whitespace();
    (parser.at == parser.src.len()).then_some(expr)
}

type TemplateFunction<'a> = Box<dyn Fn(&[String]) -> String + 'a>;

/// Functions available to variable expressions, by name; they take over the built-in ones of the
/// same name. A function returns `""` for what it does not know.
#[derive(Default)]
pub struct TemplateFunctions<'a> {
    functions: HashMap<String, TemplateFunction<'a>>,
}

impl<'a> TemplateFunctions<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: &str, function: impl Fn(&[String]) -> String + 'a) {
        self.functions.insert(name.to_owned(), Box::new(function));
    }

    pub fn with(mut self, name: &str, function: impl Fn(&[String]) -> String + 'a) -> Self {
        self.insert(name, function);
        self
    }

    /// The value of `name(args)`; `None` when there is no such function.
    pub fn call(&self, name: &str, args: &[String]) -> Option<String> {
        self.functions.get(name).map(|function| function(args))
    }
}

/// The words of an identifier or phrase: `loadUserProfile`, `HTTPServer`, `foo-bar baz`.
fn words(s: &str) -> Vec<String> {
    static LOWER_UPPER: LazyLock<Regex> = LazyLock::new(|| pattern(r"([a-z0-9])([A-Z])"));
    static UPPER_RUN: LazyLock<Regex> = LazyLock::new(|| pattern(r"([A-Z]+)([A-Z][a-z])"));
    let spaced = LOWER_UPPER.replace_all(s, "${1} ${2}");
    let spaced = UPPER_RUN.replace_all(&spaced, "${1} ${2}");
    spaced.split(|c: char| !c.is_ascii_alphanumeric()).filter(|word| !word.is_empty()).map(str::to_owned).collect()
}

/// `s` with its first character in upper case. A character outside the Basic Multilingual Plane
/// is left alone.
pub fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) if first.len_utf16() == 1 => first.to_uppercase().chain(chars).collect(),
        _ => s.to_owned(),
    }
}

/// `s` with its first character in lower case.
pub fn decapitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) if first.len_utf16() == 1 => first.to_lowercase().chain(chars).collect(),
        _ => s.to_owned(),
    }
}

pub fn camel_case(s: &str) -> String {
    words(s)
        .iter()
        .enumerate()
        .map(|(i, word)| if i == 0 { word.to_lowercase() } else { capitalize(&word.to_lowercase()) })
        .collect()
}

pub fn pascal_case(s: &str) -> String {
    capitalize(&camel_case(s))
}

fn lower_words(s: &str) -> Vec<String> {
    words(s).iter().map(|word| word.to_lowercase()).collect()
}

pub fn snake_case(s: &str) -> String {
    lower_words(s).join("_")
}

pub fn kebab_case(s: &str) -> String {
    lower_words(s).join("-")
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
/// Monday first.
const WEEKDAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

/// `d` in a `SimpleDateFormat`-style subset: `yyyy yy MMMM MMM MM M dd d HH H mm ss EEEE EEE` and
/// text in single quotes. Without a format: `yyyy-MM-dd`.
pub fn format_date(d: NaiveDateTime, format: Option<&str>) -> String {
    static TOKEN: LazyLock<Regex> =
        LazyLock::new(|| pattern(r"yyyy|yy|MMMM|MMM|MM|M|dd|d|HH|H|mm|ss|EEEE|EEE|'[^']*'"));
    let Some(format) = format.filter(|format| !format.is_empty()) else {
        return format!("{}-{:02}-{:02}", d.year(), d.month(), d.day());
    };
    let month = MONTHS[d.month0() as usize % 12];
    let weekday = WEEKDAYS[d.weekday().num_days_from_monday() as usize % 7];
    TOKEN
        .replace_all(format, |found: &Captures<'_>| {
            let token = &found[0];
            match token {
                "yyyy" => d.year().to_string(),
                "yy" => format!("{:02}", d.year().rem_euclid(100)),
                "MMMM" => month.to_owned(),
                "MMM" => month[..3].to_owned(),
                "MM" => format!("{:02}", d.month()),
                "M" => d.month().to_string(),
                "dd" => format!("{:02}", d.day()),
                "d" => d.day().to_string(),
                "HH" => format!("{:02}", d.hour()),
                "H" => d.hour().to_string(),
                "mm" => format!("{:02}", d.minute()),
                "ss" => format!("{:02}", d.second()),
                "EEEE" => weekday.to_owned(),
                "EEE" => weekday[..3].to_owned(),
                quoted => quoted.trim_matches('\'').to_owned(),
            }
        })
        .into_owned()
}

/// The built-in function `name` applied to `args`; `None` when there is no such function.
/// A function called without the text it works on yields `""`.
fn pure_function(name: &str, args: &[String], now: NaiveDateTime) -> Option<String> {
    let first = args.first().map(String::as_str);
    let on_text = |function: fn(&str) -> String| first.map(function).unwrap_or_default();
    Some(match name {
        "capitalize" => on_text(capitalize),
        "decapitalize" => on_text(decapitalize),
        "camelCase" => on_text(camel_case),
        "pascalCase" => on_text(pascal_case),
        "snakeCase" => on_text(snake_case),
        "lowercaseAndDash" | "kebabCase" => on_text(kebab_case),
        "capitalizeAndUnderscore" => first.map(|s| snake_case(s).to_uppercase()).unwrap_or_default(),
        "upperCase" => first.map(str::to_uppercase).unwrap_or_default(),
        "lowerCase" => first.map(str::to_lowercase).unwrap_or_default(),
        "concat" => args.concat(),
        "substringBefore" => match (first, args.get(1).filter(|delimiter| !delimiter.is_empty())) {
            (Some(s), Some(delimiter)) => s.split_once(delimiter.as_str()).map_or(s, |(before, _)| before).to_owned(),
            (Some(s), None) => s.to_owned(),
            (None, _) => String::new(),
        },
        "date" => format_date(now, first),
        "time" => format_date(now, Some(first.filter(|format| !format.is_empty()).unwrap_or("HH:mm"))),
        // Interactive helpers and IDEA's database macros have no value: the field stays a plain
        // tab stop, which the editor completes.
        "complete" | "completeSmart" | "dbObjectName" | "dbColumns" | "showParameterInfo" => String::new(),
        "enum" => first.unwrap_or_default().to_owned(),
        _ => return None,
    })
}

/// The value of the expression `src`: `""` when it does not parse or calls an unknown function.
/// `values` are the variables it may refer to, `now` the moment `date()` and `time()` format.
pub fn evaluate_expression(
    src: &str,
    fns: &TemplateFunctions<'_>,
    values: &BTreeMap<String, String>,
    now: NaiveDateTime,
) -> String {
    fn eval(expr: &Expr, fns: &TemplateFunctions<'_>, values: &BTreeMap<String, String>, now: NaiveDateTime) -> String {
        match expr {
            Expr::Str(value) => value.clone(),
            Expr::Ref(name) => values.get(name).cloned().unwrap_or_default(),
            Expr::Call { function, args } => {
                let args: Vec<String> = args.iter().map(|arg| eval(arg, fns, values, now)).collect();
                fns.call(function, &args).or_else(|| pure_function(function, &args, now)).unwrap_or_default()
            }
        }
    }
    parse_expression(src.trim()).map(|expr| eval(&expr, fns, values, now)).unwrap_or_default()
}

/// Function names an expression calls (to fetch e.g. the clipboard only when it is needed).
pub fn called_functions(src: &str) -> Vec<String> {
    fn walk(expr: &Expr, out: &mut Vec<String>) {
        if let Expr::Call { function, args } = expr {
            out.push(function.clone());
            for arg in args {
                walk(arg, out);
            }
        }
    }
    let mut out = Vec::new();
    if let Some(expr) = parse_expression(src.trim()) {
        walk(&expr, &mut out);
    }
    out
}

// ---------------------------------------------------------------------------
// Expansion

/// What an expansion takes from the place it happens at.
#[derive(Default)]
pub struct ExpansionEnv<'a> {
    pub fns: TemplateFunctions<'a>,
    /// Text for `$SELECTION$`; its first line is expected to start at its indentation.
    pub selection: Option<String>,
    /// Indentation of the selection's first line in the document (stripped from later lines).
    pub selection_indent: Option<String>,
    /// Variables with fixed values (e.g. postfix `$EXPR$`); inserted as plain text.
    pub predefined: BTreeMap<String, String>,
    /// The moment `date()` and `time()` format; the local time when unset.
    pub now: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Expansion {
    /// The snippet template [`crate::templates::snippet`] applies.
    pub snippet: String,
    /// The first tab stop asks for completion (`complete()`).
    pub complete_first: bool,
    /// Per tab stop, in order: whether it asks for completion.
    pub completing: Vec<bool>,
    /// Plain-text result with the fields at their default values (for previews).
    pub preview: String,
}

/// `s` with its braces escaped, so the snippet parser takes them as text.
fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '{' | '}') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A field's default: snippet fields cannot hold braces or line breaks.
fn field_default(s: &str) -> String {
    let plain: String = s.chars().filter(|c| !matches!(c, '{' | '}')).collect();
    let first_line = plain.split('\n').next().unwrap_or_default();
    first_line.strip_suffix('\r').unwrap_or(first_line).to_owned()
}

fn common_prefix(a: &str, b: &str) -> String {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).map(|(x, _)| x).collect()
}

fn leading_blanks(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// The selection's lines without the indentation they share: the first line loses its own, the
/// others `indent` (or what they have in common when one of them is indented less).
fn dedent_selection(text: &str, indent: Option<&str>) -> Vec<String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<&str> = normalized.split('\n').collect();
    if lines.len() > 1 && lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    let rest: Vec<&str> = lines.iter().skip(1).copied().filter(|line| !line.trim().is_empty()).collect();
    let mut strip = indent.map(str::to_owned);
    if strip.as_deref().is_none_or(|strip| rest.iter().any(|line| !line.starts_with(strip))) {
        let common = rest.iter().map(|line| leading_blanks(line).to_owned()).reduce(|a, b| common_prefix(&a, &b));
        let common = common.unwrap_or_default();
        strip = Some(match strip {
            Some(strip) => common_prefix(&strip, &common),
            None => common,
        });
    }
    let strip = strip.unwrap_or_default();
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| match line.strip_prefix(strip.as_str()) {
            Some(rest) if i > 0 => rest.to_owned(),
            _ => line.trim_start().to_owned(),
        })
        .collect()
}

/// The default value as written when it is not an expression: without its quotes.
fn unquoted(default_value: &str) -> &str {
    let value = default_value.strip_prefix('"').unwrap_or(default_value);
    value.strip_suffix('"').unwrap_or(value)
}

/// Converts a template to a snippet with linked fields and a final caret.
pub fn expand_template(body: &str, variables: &[TemplateVariable], env: &ExpansionEnv<'_>) -> Expansion {
    // Expressions whose field is filled through completion.
    static COMPLETING: LazyLock<Regex> =
        LazyLock::new(|| pattern(r"(?-u:\b)(complete|completeSmart|dbObjectName|dbColumns)\(\)"));
    let segments = parse_body(body);
    let now = env.now.unwrap_or_else(|| Local::now().naive_local());
    let in_body = |name: &str| segments.iter().any(|segment| matches!(segment, Segment::Var(var) if var == name));

    // Tab order: declared variables first (IDEA's order), then by appearance.
    let mut order: Vec<&str> = variables.iter().map(|v| v.name.as_str()).filter(|name| in_body(name)).collect();
    for segment in &segments {
        if let Segment::Var(name) = segment
            && !order.contains(&name.as_str())
        {
            order.push(name);
        }
    }

    let mut values = env.predefined.clone();
    // Stop numbers by variable, in tab order.
    let mut stops: Vec<(&str, usize)> = Vec::new();
    let mut completing = Vec::new();
    for name in order {
        if env.predefined.contains_key(name) {
            continue;
        }
        // The last declaration of a name is the one in effect.
        let declared = variables.iter().rev().find(|v| v.name == name);
        let expression = declared.and_then(|v| v.expression.as_deref()).filter(|expression| !expression.is_empty());
        let mut value = expression.map(|e| evaluate_expression(e, &env.fns, &values, now)).unwrap_or_default();
        if value.is_empty()
            && let Some(default_value) = declared.and_then(|v| v.default_value.as_deref()).filter(|d| !d.is_empty())
        {
            value = evaluate_expression(default_value, &env.fns, &values, now);
            if value.is_empty() {
                value = unquoted(default_value).to_owned();
            }
        }
        let skip = declared.is_some_and(|v| v.skip_if_defined) && !value.is_empty();
        values.insert(name.to_owned(), value);
        if skip {
            continue;
        }
        let number = stops.len() + 1;
        match stops.iter_mut().find(|(stop, _)| *stop == name) {
            Some(stop) => stop.1 = number,
            None => stops.push((name, number)),
        }
        completing.push(expression.is_some_and(|expression| COMPLETING.is_match(expression)));
    }

    let mut snippet = String::new();
    let mut preview = String::new();
    let mut has_end = false;
    for segment in &segments {
        match segment {
            Segment::Text(text) => {
                snippet.push_str(&escape_text(text));
                preview.push_str(text);
            }
            Segment::End => {
                if !has_end {
                    snippet.push_str("${0}");
                }
                has_end = true;
            }
            Segment::Selection => {
                let line_start = snippet.rfind('\n').map_or(0, |at| at + 1);
                let tabs = "\t".repeat(snippet[line_start..].chars().take_while(|&c| c == '\t').count());
                let selection = env.selection.as_deref().unwrap_or_default();
                let lines = dedent_selection(selection, env.selection_indent.as_deref());
                let joined = lines
                    .iter()
                    .enumerate()
                    .map(|(i, line)| if i == 0 { line.clone() } else { format!("{tabs}{line}") })
                    .collect::<Vec<_>>()
                    .join("\n");
                snippet.push_str(&escape_text(&joined));
                preview.push_str(&joined);
            }
            Segment::Var(name) => {
                let value = values.get(name).map(String::as_str).unwrap_or_default();
                match stops.iter().find(|(stop, _)| stop == name) {
                    None => snippet.push_str(&escape_text(value)),
                    Some((_, number)) => {
                        let default = field_default(value);
                        if default.is_empty() {
                            snippet.push_str(&format!("${{{number}}}"));
                        } else {
                            snippet.push_str(&format!("${{{number}:{default}}}"));
                        }
                    }
                }
                preview.push_str(value);
            }
        }
    }
    if !has_end {
        snippet.push_str("${0}");
    }
    Expansion { snippet, complete_first: completing.first().copied().unwrap_or(false), completing, preview }
}

/// What narrows the templates offered at a place.
#[derive(Debug, Clone, Copy, Default)]
pub struct TemplateFilter<'a> {
    /// The place is inside a string or a comment: only templates of scope `any` apply.
    pub in_comment_or_string: bool,
    /// Finer contexts the editor reports at the caret (`sql-statement`…).
    pub contexts: &'a [String],
}

/// Templates applicable to a language at a place.
pub fn applicable_templates<'t>(
    templates: &'t [LiveTemplate],
    language: &str,
    filter: TemplateFilter<'_>,
) -> Vec<&'t LiveTemplate> {
    templates
        .iter()
        .filter(|t| {
            t.enabled != Some(false)
                && t.contexts.iter().any(|c| c == "any" || c == language || filter.contexts.contains(c))
                && !(filter.in_comment_or_string && t.scope.unwrap_or_default() == TemplateScope::Code)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::snippet::testing::applied;
    use chrono::NaiveDate;

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(year, month, day)
            .and_then(|date| date.and_hms_opt(hour, minute, second))
            .expect("a valid date")
    }

    fn now() -> NaiveDateTime {
        at(2026, 1, 5, 9, 7, 3)
    }

    fn text(v: &str) -> Segment {
        Segment::Text(v.to_owned())
    }

    fn expand(body: &str, variables: &[TemplateVariable], env: &ExpansionEnv<'_>) -> Expansion {
        expand_template(body, variables, env)
    }

    // template parsing › splits variables, END, SELECTION and $$
    #[test]
    fn splits_variables_end_selection_and_double_dollar() {
        assert_eq!(
            parse_body("a $X$ $$ $END$ $SELECTION$ $ b"),
            [
                text("a "),
                Segment::Var("X".to_owned()),
                text(" $ "),
                Segment::End,
                text(" "),
                Segment::Selection,
                text(" $ b"),
            ]
        );
        assert_eq!(body_variables("for ($I$ = 0; $I$ < $N$; $I$++) $END$"), ["I", "N"]);
    }

    // expressions › evaluates calls, literals and references
    #[test]
    fn evaluates_calls_literals_and_references() {
        let fns = TemplateFunctions::new().with("className", |_| "UserService".to_owned());
        let none = TemplateFunctions::new();
        let no_values = BTreeMap::new();
        assert_eq!(evaluate_expression("className()", &fns, &no_values, now()), "UserService");
        assert_eq!(evaluate_expression(r#"capitalize("abc")"#, &none, &no_values, now()), "Abc");
        assert_eq!(evaluate_expression("snakeCase(className())", &fns, &no_values, now()), "user_service");
        let values = BTreeMap::from([("NAME".to_owned(), "foo-bar".to_owned())]);
        assert_eq!(evaluate_expression("camelCase(NAME)", &none, &values, now()), "fooBar");
        assert_eq!(evaluate_expression("unknownFn()", &none, &no_values, now()), "");
        assert_eq!(evaluate_expression("broken(", &none, &no_values, now()), "");
    }

    // expressions › formats dates
    #[test]
    fn formats_dates() {
        let d = at(2026, 1, 5, 9, 7, 3);
        assert_eq!(format_date(d, None), "2026-01-05");
        assert_eq!(format_date(d, Some("dd.MM.yy HH:mm:ss")), "05.01.26 09:07:03");
    }

    // expansion › numbers stops, links repeats and ends at $END$
    #[test]
    fn numbers_stops_links_repeats_and_ends_at_end() {
        let e = expand(
            "for (let $I$ = 0; $I$ < $N$; $I$++) {\n\t$END$\n}",
            &[TemplateVariable::new("I").default_value("\"i\"")],
            &ExpansionEnv::default(),
        );
        assert_eq!(e.snippet, "for (let ${1:i} = 0; ${1:i} < ${2}; ${1:i}++) \\{\n\t${0}\n\\}");
        let (text, selection) = applied("", &e.snippet, 0, 0);
        assert_eq!(text, "for (let i = 0; i < ; i++) {\n  \n}");
        // All occurrences of the first field are selected together (linked editing).
        assert_eq!(selection, [(9, 10), (16, 17), (22, 23)]);
    }

    // expansion › appends the final caret when $END$ is missing
    #[test]
    fn appends_the_final_caret_when_end_is_missing() {
        assert_eq!(expand("console.log()", &[], &ExpansionEnv::default()).snippet, "console.log()${0}");
    }

    // expansion › inlines skipIfDefined values and keeps empty ones as stops
    #[test]
    fn inlines_skip_if_defined_values_and_keeps_empty_ones_as_stops() {
        let fns =
            TemplateFunctions::new().with("className", |_| "Foo".to_owned()).with("methodName", |_| String::new());
        let e = expand(
            r#"System.out.println("$C$.$M$");"#,
            &[
                TemplateVariable::new("C").expression("className()").skip_if_defined(),
                TemplateVariable::new("M").expression("methodName()").skip_if_defined(),
            ],
            &ExpansionEnv { fns, ..ExpansionEnv::default() },
        );
        assert_eq!(e.snippet, r#"System.out.println("Foo.${1}");${0}"#);
    }

    // expansion › flags complete() on the first stop
    #[test]
    fn flags_complete_on_the_first_stop() {
        let variables = [TemplateVariable::new("T").expression("complete()")];
        assert!(expand("$T$ x", &variables, &ExpansionEnv::default()).complete_first);
    }

    // expansion › flags completing stops in tab order
    #[test]
    fn flags_completing_stops_in_tab_order() {
        let e = expand(
            "update $table$ set $col$ = $value$ where $END$",
            &[
                TemplateVariable::new("table").expression("dbObjectName()"),
                TemplateVariable::new("col").expression("complete()"),
                TemplateVariable::new("value"),
            ],
            &ExpansionEnv::default(),
        );
        assert_eq!(e.completing, [true, true, false]);
    }

    // expansion › indents $SELECTION$ lines relative to the template
    #[test]
    fn indents_selection_lines_relative_to_the_template() {
        let env = ExpansionEnv {
            selection: Some("foo();\n    if (x) {\n        bar();\n    }".to_owned()),
            selection_indent: Some("    ".to_owned()),
            ..ExpansionEnv::default()
        };
        let e = expand("if ($C$) {\n\t$SELECTION$\n}$END$", &[], &env);
        let doc = "    foo();";
        let (text, _) = applied(doc, &e.snippet, 4, doc.len() as u32);
        assert_eq!(text, "    if () {\n      foo();\n      if (x) {\n          bar();\n      }\n    }");
    }

    // expansion › escapes braces from selections and predefined values
    #[test]
    fn escapes_braces_from_selections_and_predefined_values() {
        let env = ExpansionEnv {
            predefined: BTreeMap::from([("EXPR".to_owned(), "{a: 1}".to_owned())]),
            ..ExpansionEnv::default()
        };
        let e = expand("($EXPR$)$END$", &[], &env);
        assert_eq!(applied("", &e.snippet, 0, 0).0, "({a: 1})");
    }

    // Not from a spec: the built-in text functions, a function called without its text, and the
    // functions an expression calls.
    #[test]
    fn built_in_functions_change_case_join_and_cut() {
        let none = TemplateFunctions::new();
        let eval = |src: &str| evaluate_expression(src, &none, &BTreeMap::new(), now());
        assert_eq!(eval(r#"pascalCase("load user-profile")"#), "LoadUserProfile");
        assert_eq!(eval(r#"capitalizeAndUnderscore("HTTPServer")"#), "HTTP_SERVER");
        assert_eq!(eval(r#"lowercaseAndDash("UserService")"#), "user-service");
        assert_eq!(eval(r#"concat("a", 'b', decapitalize("Cd"))"#), "abcd");
        assert_eq!(eval(r#"substringBefore("main.rs", ".")"#), "main");
        assert_eq!(eval(r#"substringBefore("main", ".")"#), "main");
        assert_eq!(eval("capitalize()"), "");
        assert_eq!(eval("date()"), "2026-01-05");
        assert_eq!(eval("time()"), "09:07");
        assert_eq!(eval(r#"date("EEE, d MMMM yyyy 'at' H")"#), "Mon, 5 January 2026 at 9");
        assert_eq!(called_functions("capitalize(clipboard())"), ["capitalize", "clipboard"]);
        assert!(called_functions("NAME").is_empty());
    }

    // Not from a spec: a selection indented less than the insertion line keeps its relative indentation.
    #[test]
    fn a_selection_is_dedented_by_what_its_lines_share() {
        assert_eq!(dedent_selection("a\n      b\n    c\n", Some("        ")), ["a", "  b", "c"]);
        assert_eq!(dedent_selection("  a\r\n  b", None), ["a", "b"]);
        assert!(uses_selection("($SELECTION$)") && !uses_selection("$END$"));
    }
}
