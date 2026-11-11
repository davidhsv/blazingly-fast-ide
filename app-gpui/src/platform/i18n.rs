//! Localisation.
//!
//! Catalogs are the webview UI's own files: flat JSON maps in `src/platform/locales/<locale>.json`
//! (keys namespaced by feature: `app.*`, `menu.*`, `vcs.*`…) and source-text catalogs in
//! `locales/text/<locale>.json` (English → translation). English is the fallback for missing
//! keys; unknown source text stays in English. Features may add messages of their own.
//!
//! Message syntax (ICU-lite):
//!   - `{name}` parameter (numbers are locale-formatted);
//!   - `{n, plural, =0 {none} one {# file} other {# files}}`: `=N` exact matches, then the
//!     locale's CLDR plural category, then `other`; `#` is the formatted number;
//!   - `{g, select, a {…} other {…}}`;
//!   - `{n, number}`.
//!
//! Branches nest. Unknown parameters are left as written.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

pub type Catalog = HashMap<String, String>;

pub const DEFAULT_LOCALE: &str = "en";

/// A UI language: `name` is written in the language itself, `tag` is its BCP 47 tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocaleInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub tag: &'static str,
}

/// The UI languages, in the order the settings offer them.
pub const LOCALES: [LocaleInfo; 7] = [
    LocaleInfo { id: "en", name: "English", tag: "en" },
    LocaleInfo { id: "zh", name: "简体中文 (Chinese, Simplified)", tag: "zh-CN" },
    LocaleInfo { id: "ko", name: "한국어 (Korean)", tag: "ko" },
    LocaleInfo { id: "fr", name: "Français (French)", tag: "fr" },
    LocaleInfo { id: "pl", name: "Polski (Polish)", tag: "pl" },
    LocaleInfo { id: "pt", name: "Português (Portuguese)", tag: "pt-BR" },
    LocaleInfo { id: "es", name: "Español (Spanish)", tag: "es-MX" },
];

macro_rules! embedded {
    ($dir:literal) => {
        [
            ("en", include_str!(concat!("../../../src/platform/locales/", $dir, "en.json"))),
            ("zh", include_str!(concat!("../../../src/platform/locales/", $dir, "zh.json"))),
            ("ko", include_str!(concat!("../../../src/platform/locales/", $dir, "ko.json"))),
            ("fr", include_str!(concat!("../../../src/platform/locales/", $dir, "fr.json"))),
            ("pl", include_str!(concat!("../../../src/platform/locales/", $dir, "pl.json"))),
            ("pt", include_str!(concat!("../../../src/platform/locales/", $dir, "pt.json"))),
            ("es", include_str!(concat!("../../../src/platform/locales/", $dir, "es.json"))),
        ]
    };
}

const MESSAGE_SOURCES: [(&str, &str); 7] = embedded!("");
const TEXT_SOURCES: [(&str, &str); 7] = embedded!("text/");

fn parsed(
    sources: &'static [(&str, &str); 7],
    cache: &'static [OnceLock<Catalog>; 7],
    locale: &str,
) -> &'static Catalog {
    static EMPTY: OnceLock<Catalog> = OnceLock::new();
    match sources.iter().position(|(id, _)| *id == locale) {
        Some(index) => cache[index].get_or_init(|| serde_json::from_str(sources[index].1).unwrap_or_default()),
        None => EMPTY.get_or_init(Catalog::new),
    }
}

/// The bundled message catalog of `locale` (empty for an unknown locale).
pub fn message_catalog(locale: &str) -> &'static Catalog {
    static CACHE: [OnceLock<Catalog>; 7] = [const { OnceLock::new() }; 7];
    parsed(&MESSAGE_SOURCES, &CACHE, locale)
}

/// The bundled source-text catalog of `locale`: English text → translation. English text is
/// shown as written, so its catalog is empty whatever `text/en.json` holds.
pub fn text_catalog(locale: &str) -> &'static Catalog {
    static CACHE: [OnceLock<Catalog>; 7] = [const { OnceLock::new() }; 7];
    parsed(&TEXT_SOURCES, &CACHE, if locale == DEFAULT_LOCALE { "" } else { locale })
}

/// Maps settings values / BCP 47 tags to a supported locale (`zh-CN` → `zh`, `ko-KR` → `ko`).
/// Empty, `system` and `auto` mean the operating system's language, given as `system`.
pub fn resolve_locale(input: Option<&str>, system: &str) -> &'static str {
    let tag = match input.map(str::trim) {
        None | Some("" | "system" | "auto") => system,
        Some(tag) => tag,
    };
    let base = tag.split(['-', '_']).next().unwrap_or_default().to_lowercase();
    LOCALES.iter().find(|l| l.id == base).map_or(DEFAULT_LOCALE, |l| l.id)
}

/// A message parameter.
#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    Text(String),
    Number(f64),
    Bool(bool),
}

impl From<&str> for Param {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}
impl From<String> for Param {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}
impl From<f64> for Param {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}
impl From<i32> for Param {
    fn from(value: i32) -> Self {
        Self::Number(value.into())
    }
}
impl From<usize> for Param {
    fn from(value: usize) -> Self {
        Self::Number(value as f64)
    }
}
impl From<bool> for Param {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

pub type Params<'a> = &'a [(&'a str, Param)];

fn param<'a>(params: Params<'a>, name: &str) -> Option<&'a Param> {
    params.iter().rev().find(|(n, _)| *n == name).map(|(_, v)| v)
}

// ---------------------------------------------------------------------------
// Numbers and plural categories per locale (CLDR, as `Intl` applies them).

struct NumberStyle {
    group: &'static str,
    decimal: &'static str,
    /// Digits the leading group needs before a separator is used (es, pl: `1234` but `12 345`).
    minimum_grouping_digits: usize,
}

fn number_style(locale: &str) -> NumberStyle {
    match locale {
        "fr" => NumberStyle { group: "\u{202f}", decimal: ",", minimum_grouping_digits: 1 },
        "pl" => NumberStyle { group: "\u{a0}", decimal: ",", minimum_grouping_digits: 2 },
        "pt" => NumberStyle { group: ".", decimal: ",", minimum_grouping_digits: 1 },
        "es" => NumberStyle { group: ".", decimal: ",", minimum_grouping_digits: 2 },
        _ => NumberStyle { group: ",", decimal: ".", minimum_grouping_digits: 1 },
    }
}

/// The digits of `n` with at most three fraction digits, as `Intl.NumberFormat` rounds by default.
fn decimal_digits(n: f64) -> (String, String) {
    let rounded = format!("{:.3}", n.abs());
    let (integer, fraction) = rounded.split_once('.').unwrap_or((&rounded, ""));
    (integer.to_owned(), fraction.trim_end_matches('0').to_owned())
}

/// `n` as the locale writes numbers: `12,345.5` (en), `12 345,5` (fr, pl), `12.345,5` (pt, es).
pub fn format_number(n: f64, locale: &str) -> String {
    if n.is_nan() {
        return "NaN".to_owned();
    }
    if n.is_infinite() {
        return if n > 0.0 { "∞".to_owned() } else { "-∞".to_owned() };
    }
    let style = number_style(locale);
    let (integer, fraction) = decimal_digits(n);
    let mut out = String::new();
    // A negative number keeps its sign even when it rounds to zero, as `Intl` writes it ("-0").
    if n < 0.0 {
        out.push('-');
    }
    if integer.len() >= 3 + style.minimum_grouping_digits {
        let lead = integer.len() % 3;
        if lead > 0 {
            out.push_str(&integer[..lead]);
        }
        for (i, chunk) in integer.as_bytes()[lead..].chunks(3).enumerate() {
            if i > 0 || lead > 0 {
                out.push_str(style.group);
            }
            out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        }
    } else {
        out.push_str(&integer);
    }
    if !fraction.is_empty() {
        out.push_str(style.decimal);
        out.push_str(&fraction);
    }
    out
}

/// The CLDR cardinal plural category of `n` in `locale`.
pub fn plural_category(n: f64, locale: &str) -> &'static str {
    if !n.is_finite() {
        return "other";
    }
    let (integer, fraction) = decimal_digits(n);
    // CLDR operands: i = integer digits, v = number of visible fraction digits.
    let i: u128 = integer.parse().unwrap_or(0);
    let v = fraction.len();
    let whole = v == 0;
    let millions = whole && i != 0 && i % 1_000_000 == 0;
    match locale {
        "zh" | "ko" => "other",
        "fr" | "pt" => {
            if i == 0 || i == 1 {
                "one"
            } else if millions {
                "many"
            } else {
                "other"
            }
        }
        "es" => {
            if whole && i == 1 {
                "one"
            } else if millions {
                "many"
            } else {
                "other"
            }
        }
        "pl" => {
            let (d, h) = (i % 10, i % 100);
            if whole && i == 1 {
                "one"
            } else if whole && (2..=4).contains(&d) && !(12..=14).contains(&h) {
                "few"
            } else if whole && (d <= 1 || (5..=9).contains(&d) || (12..=14).contains(&h)) {
                "many"
            } else {
                "other"
            }
        }
        _ => {
            if whole && i == 1 {
                "one"
            } else {
                "other"
            }
        }
    }
}

fn display(value: &Param, locale: &str) -> String {
    match value {
        Param::Number(n) => format_number(*n, locale),
        Param::Text(s) => s.clone(),
        Param::Bool(b) => b.to_string(),
    }
}

/// The number a parameter stands for in a plural (`Number(value)` in the webview).
fn numeric(value: &Param) -> f64 {
    match value {
        Param::Number(n) => *n,
        Param::Bool(b) => f64::from(u8::from(*b)),
        Param::Text(s) => {
            let s = s.trim();
            if s.is_empty() { 0.0 } else { s.parse().unwrap_or(f64::NAN) }
        }
    }
}

// ---------------------------------------------------------------------------
// ICU-lite formatter

struct SyntaxError;

struct Formatter<'a> {
    src: Vec<char>,
    at: usize,
    params: Params<'a>,
    locale: &'a str,
}

impl Formatter<'_> {
    fn peek(&self) -> Option<char> {
        self.src.get(self.at).copied()
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }

    fn read_until(&mut self, stops: &[char]) -> String {
        let start = self.at;
        while self.peek().is_some_and(|c| !stops.contains(&c)) {
            self.at += 1;
        }
        self.src[start..self.at].iter().collect()
    }

    fn slice(&self, start: usize) -> String {
        self.src[start..self.at].iter().collect()
    }

    /// Text until an unmatched `}` (left unconsumed) or the end.
    fn text(&mut self, hash: Option<&str>) -> Result<String, SyntaxError> {
        let mut out = String::new();
        while let Some(c) = self.peek() {
            match (c, hash) {
                ('}', _) => return Ok(out),
                ('{', _) => out.push_str(&self.argument(hash)?),
                ('#', Some(hash)) => {
                    out.push_str(hash);
                    self.at += 1;
                }
                _ => {
                    out.push(c);
                    self.at += 1;
                }
            }
        }
        Ok(out)
    }

    fn argument(&mut self, hash: Option<&str>) -> Result<String, SyntaxError> {
        let start = self.at;
        self.at += 1;
        self.skip_whitespace();
        let name = self.read_until(&[',', '}']).trim().to_owned();
        let value = param(self.params, &name);
        if self.peek() == Some('}') {
            self.at += 1;
            return Ok(value.map_or_else(|| self.slice(start), |v| display(v, self.locale)));
        }
        if self.peek() != Some(',') {
            return Err(SyntaxError);
        }
        self.at += 1;
        self.skip_whitespace();
        let kind = self.read_until(&[',', '}']).trim().to_owned();
        if self.peek() == Some('}') {
            self.at += 1;
            return Ok(value.map_or_else(|| self.slice(start), |v| display(v, self.locale)));
        }
        if self.peek().is_none() {
            return Err(SyntaxError);
        }
        self.at += 1; // ','
        if kind != "plural" && kind != "select" {
            return Err(SyntaxError);
        }
        let n = value.map_or(f64::NAN, numeric);
        let plural_hash = (kind == "plural").then(|| match value {
            Some(_) if n.is_finite() => format_number(n, self.locale),
            Some(v) => display(v, self.locale),
            None => "undefined".to_owned(),
        });
        let branch_hash = plural_hash.as_deref().or(hash);
        let mut options: Vec<(String, String)> = Vec::new();
        loop {
            self.skip_whitespace();
            match self.peek() {
                None => return Err(SyntaxError),
                Some('}') => {
                    self.at += 1;
                    break;
                }
                Some(_) => {}
            }
            let selector = self.read_until(&['{', ' ', '\t', '\n', '\r', '}']).trim().to_owned();
            self.skip_whitespace();
            if self.peek() != Some('{') || selector.is_empty() {
                return Err(SyntaxError);
            }
            self.at += 1;
            let body = self.text(branch_hash)?;
            if self.peek() != Some('}') {
                return Err(SyntaxError);
            }
            self.at += 1;
            // A repeated selector replaces the earlier one.
            options.retain(|(s, _)| *s != selector);
            options.push((selector, body));
        }
        let option = |selector: &str| options.iter().find(|(s, _)| s == selector).map(|(_, body)| body.clone());
        let other = || option("other").unwrap_or_default();
        let Some(value) = value else { return Ok(other()) };
        if kind == "plural" {
            let exact = if n.is_finite() { option(&format!("={}", js_number(n))) } else { None };
            let category = if n.is_finite() { plural_category(n, self.locale) } else { "other" };
            return Ok(exact.or_else(|| option(category)).unwrap_or_else(other));
        }
        let key = match value {
            Param::Number(n) => js_number(*n),
            other => display(other, self.locale),
        };
        Ok(option(&key).unwrap_or_else(other))
    }
}

/// A number as JavaScript's `String(n)` writes it, for `=N` and `select` keys.
fn js_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e21 { format!("{n:.0}") } else { n.to_string() }
}

fn format_strict(message: &str, params: Params, locale: &str) -> Result<String, SyntaxError> {
    let mut f = Formatter { src: message.chars().collect(), at: 0, params, locale };
    let mut out = String::new();
    while f.at < f.src.len() {
        out.push_str(&f.text(None)?);
        // A stray `}`.
        if let Some(c) = f.peek() {
            out.push(c);
            f.at += 1;
        }
    }
    Ok(out)
}

/// Formats an ICU-lite message. Malformed messages are returned unformatted.
pub fn format_message(message: &str, params: Params, locale: &str) -> String {
    if !message.contains('{') {
        return message.to_owned();
    }
    format_strict(message, params, locale).unwrap_or_else(|_| message.to_owned())
}

/// Whether `message` is well-formed ICU-lite (used by catalog tests).
pub fn is_valid_message(message: &str) -> bool {
    format_strict(message, &[], DEFAULT_LOCALE).is_ok()
}

// ---------------------------------------------------------------------------
// Source-text templates

/// A source text split at its `{name}` placeholders: `"Run '{name}'"` → `["Run '", "'"]`, `["name"]`.
fn split_placeholders(source: &str) -> (Vec<String>, Vec<String>) {
    let (mut literals, mut names) = (vec![String::new()], Vec::new());
    let mut rest = source;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let end = after.find('}');
        let name = end
            .map(|end| &after[..end])
            .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_alphanumeric() || c == '_'));
        match (name, end) {
            (Some(name), Some(end)) => {
                literals.last_mut().expect("starts with one literal").push_str(&rest[..open]);
                names.push(name.to_owned());
                literals.push(String::new());
                rest = &after[end + 1..];
            }
            _ => {
                literals.last_mut().expect("starts with one literal").push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    literals.last_mut().expect("starts with one literal").push_str(rest);
    (literals, names)
}

struct Template {
    literals: Vec<String>,
    names: Vec<String>,
    translation: String,
    /// Length of the literal text: the most specific template is tried first.
    literal: usize,
}

impl Template {
    /// The placeholder values when `text` has this template's shape. Each placeholder takes
    /// the shortest non-empty run without line breaks that lets the rest match.
    fn captures(&self, text: &str) -> Option<Vec<String>> {
        fn go<'t>(literals: &[String], text: &'t str, out: &mut Vec<&'t str>) -> bool {
            let Some((next, rest)) = literals.split_first() else { return text.is_empty() };
            let mut end = 0;
            for c in text.chars() {
                if matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                    return false;
                }
                end += c.len_utf8();
                if text[end..].starts_with(next.as_str()) {
                    out.push(&text[..end]);
                    if go(rest, &text[end + next.len()..], out) {
                        return true;
                    }
                    out.pop();
                }
            }
            false
        }
        let (first, rest) = self.literals.split_first()?;
        let mut out = Vec::new();
        go(rest, text.strip_prefix(first.as_str())?, &mut out).then(|| out.into_iter().map(str::to_owned).collect())
    }
}

struct Shapes {
    /// Source-text templates by their literal parts: `"Run '{name}'"` under `"Run '{}'"`.
    by_shape: HashMap<String, (Vec<String>, String)>,
    templates: Vec<Template>,
}

fn shapes(locale: &str) -> &'static Shapes {
    static CACHE: [OnceLock<Shapes>; 7] = [const { OnceLock::new() }; 7];
    let index = LOCALES.iter().position(|l| l.id == locale).unwrap_or(0);
    CACHE[index].get_or_init(|| {
        let mut by_shape = HashMap::new();
        let mut templates = Vec::new();
        for (source, translation) in text_catalog(LOCALES[index].id) {
            let (literals, names) = split_placeholders(source);
            if names.is_empty() {
                continue;
            }
            by_shape.insert(literals.join("{}"), (names.clone(), translation.clone()));
            let literal = literals.iter().map(|l| l.chars().count()).sum();
            templates.push(Template { literals, names, translation: translation.clone(), literal });
        }
        // Most specific first: `Language server '{name}' is not installed` before `{name} is not installed`.
        templates.sort_by(|a, b| b.literal.cmp(&a.literal).then_with(|| a.translation.cmp(&b.translation)));
        Shapes { by_shape, templates }
    })
}

/// Text built from a template: its literal parts and the interpolated values.
#[derive(Debug, Clone, PartialEq)]
struct Built {
    strings: Vec<String>,
    values: Vec<Param>,
}

const MAX_BUILT: usize = 10_000;

/// Text that templates produced (in English and translated), so that it can be translated again
/// after a locale change. Least recently built entries are dropped first.
#[derive(Debug, Default)]
struct BuiltTexts {
    entries: HashMap<String, (Built, u64)>,
    order: BTreeMap<u64, String>,
    next: u64,
}

impl BuiltTexts {
    fn remember(&mut self, text: String, built: Built) {
        if let Some((_, sequence)) = self.entries.remove(&text) {
            self.order.remove(&sequence);
        }
        self.order.insert(self.next, text.clone());
        self.entries.insert(text, (built, self.next));
        self.next += 1;
        if self.entries.len() > MAX_BUILT
            && let Some((_, oldest)) = self.order.pop_first()
        {
            self.entries.remove(&oldest);
        }
    }

    fn get(&self, text: &str) -> Option<&Built> {
        self.entries.get(text).map(|(built, _)| built)
    }
}

/// The active locale and the messages features registered.
#[derive(Debug)]
pub struct I18n {
    locale: &'static str,
    /// Feature-registered messages, per locale; later registrations win.
    extra: HashMap<String, Catalog>,
    built: BuiltTexts,
}

/// The process-wide locale state.
#[derive(Default)]
pub struct Localization(pub I18n);

impl gpui_kit::Global for Localization {}

impl Default for I18n {
    fn default() -> Self {
        Self { locale: DEFAULT_LOCALE, extra: HashMap::new(), built: BuiltTexts::default() }
    }
}

fn interleave(strings: &[String], parts: &[String]) -> String {
    let mut out = String::new();
    for (i, s) in strings.iter().enumerate() {
        if i > 0 {
            out.push_str(parts.get(i - 1).map_or("", String::as_str));
        }
        out.push_str(s);
    }
    out
}

impl I18n {
    pub fn locale(&self) -> &'static str {
        self.locale
    }

    pub fn locale_info(&self) -> LocaleInfo {
        LOCALES.iter().copied().find(|l| l.id == self.locale).unwrap_or(LOCALES[0])
    }

    /// Switches the UI locale. Unsupported tags fall back to English. Returns whether it changed.
    pub fn set_locale(&mut self, input: Option<&str>, system: &str) -> bool {
        let locale = resolve_locale(input, system);
        let changed = locale != self.locale;
        self.locale = locale;
        changed
    }

    /// Adds messages for a locale (e.g. a feature's own strings). Later registrations win.
    pub fn register_messages(&mut self, locale: &str, messages: &[(&str, &str)]) {
        let catalog = self.extra.entry(locale.to_owned()).or_default();
        for (key, message) in messages {
            catalog.insert((*key).to_owned(), (*message).to_owned());
        }
    }

    /// Removes messages registered with [`I18n::register_messages`] that still have these values.
    pub fn unregister_messages(&mut self, locale: &str, messages: &[(&str, &str)]) {
        if let Some(catalog) = self.extra.get_mut(locale) {
            for (key, message) in messages {
                if catalog.get(*key).map(String::as_str) == Some(*message) {
                    catalog.remove(*key);
                }
            }
        }
    }

    fn message_in(&self, locale: &str, key: &str) -> Option<&str> {
        self.extra.get(locale).and_then(|c| c.get(key)).or_else(|| message_catalog(locale).get(key)).map(String::as_str)
    }

    /// The raw message for `key` in the active locale, falling back to English.
    pub fn lookup(&self, key: &str) -> Option<&str> {
        self.message_in(self.locale, key).or_else(|| self.message_in(DEFAULT_LOCALE, key))
    }

    /// Translates `key`; returns the key itself when no catalog has it.
    pub fn t(&self, key: &str, params: Params) -> String {
        match self.lookup(key) {
            Some(message) => format_message(message, params, self.locale),
            None => key.to_owned(),
        }
    }

    fn texts(&self) -> &'static Catalog {
        text_catalog(self.locale)
    }

    fn format_built(&self, built: &Built) -> String {
        let texts = self.texts();
        let parts: Vec<String> = built
            .values
            .iter()
            .map(|v| match v {
                Param::Text(s) => texts.get(s).cloned().or_else(|| self.rebuild(s)).unwrap_or_else(|| s.clone()),
                Param::Number(n) => js_number(*n),
                Param::Bool(b) => b.to_string(),
            })
            .collect();
        match shapes(self.locale).by_shape.get(&built.strings.join("{}")) {
            None => interleave(&built.strings, &parts),
            Some((names, translation)) => {
                let params: Vec<(&str, Param)> =
                    names.iter().zip(&parts).map(|(n, p)| (n.as_str(), Param::Text(p.clone()))).collect();
                format_message(translation, &params, DEFAULT_LOCALE)
            }
        }
    }

    /// Text that a template built, in the active locale; `None` for any other text.
    fn rebuild(&self, text: &str) -> Option<String> {
        self.built.get(text).map(|built| self.format_built(built))
    }

    /// Translates English UI text through the source-text catalog; unknown text is returned as is.
    pub fn tx(&self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }
        self.texts().get(text).cloned().or_else(|| self.rebuild(text)).unwrap_or_else(|| text.to_owned())
    }

    /// Translates text built at run time: `strings` are the literal parts around `values`
    /// (`["Remove data source '", "'?"]` looks up `"Remove data source '{name}'?"`, whatever the
    /// placeholder is called). Values that are text are translated when the catalog has them.
    /// The result is remembered, so [`I18n::tx`] translates it again after a locale change.
    pub fn tx_built(&mut self, strings: &[&str], values: &[Param]) -> String {
        if values.is_empty() {
            return self.tx(strings.first().copied().unwrap_or_default());
        }
        let built = Built { strings: strings.iter().map(|s| (*s).to_owned()).collect(), values: values.to_vec() };
        let english_parts: Vec<String> = built
            .values
            .iter()
            .map(|v| match v {
                Param::Text(s) => s.clone(),
                Param::Number(n) => js_number(*n),
                Param::Bool(b) => b.to_string(),
            })
            .collect();
        let english = interleave(&built.strings, &english_parts);
        self.built.remember(english.clone(), built.clone());
        let text = self.format_built(&built);
        if text != english {
            self.built.remember(text.clone(), built);
        }
        text
    }

    /// [`I18n::tx`] for text built elsewhere (notification titles): also matches source-text
    /// templates such as `"{name} failed"`, translating the substituted parts as text.
    pub fn tx_template(&self, text: &str) -> String {
        let texts = self.texts();
        if let Some(exact) = texts.get(text).cloned().or_else(|| self.rebuild(text)) {
            return exact;
        }
        for template in &shapes(self.locale).templates {
            let Some(captures) = template.captures(text) else { continue };
            let params: Vec<(&str, Param)> = template
                .names
                .iter()
                .zip(&captures)
                .map(|(name, value)| {
                    (name.as_str(), Param::Text(texts.get(value).cloned().unwrap_or_else(|| value.clone())))
                })
                .collect();
            return format_message(&template.translation, &params, DEFAULT_LOCALE);
        }
        text.to_owned()
    }

    /// All keys with `prefix`, across the active catalog and the English fallback.
    pub fn keys_with_prefix(&self, prefix: &str) -> Vec<String> {
        let mut keys: Vec<String> = [DEFAULT_LOCALE, self.locale]
            .into_iter()
            .flat_map(|locale| {
                message_catalog(locale).keys().chain(self.extra.get(locale).into_iter().flat_map(|c| c.keys()))
            })
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        keys.sort_unstable();
        keys.dedup();
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(message: &str, params: Params) -> String {
        format_message(message, params, "en")
    }

    fn with_locale(tag: &str) -> I18n {
        let mut i18n = I18n::default();
        i18n.set_locale(Some(tag), "en");
        i18n
    }

    // src/platform/i18n.test.ts › formatMessage › substitutes parameters and leaves unknown ones
    #[test]
    fn substitutes_parameters_and_leaves_unknown_ones() {
        assert_eq!(fmt("Open {name}", &[("name", "openidea".into())]), "Open openidea");
        assert_eq!(fmt("Open {name} in {where}", &[("name", "openidea".into())]), "Open openidea in {where}");
        assert_eq!(fmt("plain text", &[]), "plain text");
    }

    // src/platform/i18n.test.ts › formatMessage › formats numbers per locale
    #[test]
    fn formats_numbers_per_locale() {
        assert_eq!(fmt("{n} files", &[("n", 12345.into())]), "12,345 files");
        assert_eq!(fmt("{n, number}", &[("n", 1234.5.into())]), "1,234.5");
        assert_eq!(format_number(1_000_000.0, "fr"), "1\u{202f}000\u{202f}000");
        assert_eq!(format_number(1234.0, "pl"), "1234");
        assert_eq!(format_number(12345.5, "pl"), "12\u{a0}345,5");
        assert_eq!(format_number(1234.0, "es"), "1234");
        assert_eq!(format_number(12345.0, "es"), "12.345");
        assert_eq!(format_number(1234.5678, "pt"), "1.234,568");
        assert_eq!(format_number(-0.5, "en"), "-0.5");
        assert_eq!(format_number(-0.0001, "en"), "-0");
        assert_eq!(format_number(999.0, "en"), "999");
    }

    // src/platform/i18n.test.ts › formatMessage › selects plural categories, exact matches first
    #[test]
    fn selects_plural_categories_exact_matches_first() {
        let m = "{count, plural, =0 {No files} one {# file} other {# files}}";
        assert_eq!(fmt(m, &[("count", 0.into())]), "No files");
        assert_eq!(fmt(m, &[("count", 1.into())]), "1 file");
        assert_eq!(fmt(m, &[("count", 2.into())]), "2 files");
        assert_eq!(fmt(m, &[("count", 1000.into())]), "1,000 files");
    }

    // src/platform/i18n.test.ts › formatMessage › uses CLDR rules of the locale
    #[test]
    fn uses_cldr_rules_of_the_locale() {
        let m = "{count, plural, one {# one} other {# other}}";
        assert_eq!(format_message(m, &[("count", 1.into())], "zh"), "1 other");
        assert_eq!(format_message(m, &[("count", 1.into())], "en"), "1 one");
        // French zero is singular; `many` falls back to `other`.
        let m = "{count, plural, one {# fichier} other {# fichiers}}";
        assert_eq!(format_message(m, &[("count", 0.into())], "fr"), "0 fichier");
        assert_eq!(format_message(m, &[("count", 2.into())], "fr"), "2 fichiers");
        assert_eq!(format_message(m, &[("count", 1_000_000.into())], "fr"), "1\u{202f}000\u{202f}000 fichiers");
        for (n, category) in
            [(1.0, "one"), (2.0, "few"), (5.0, "many"), (12.0, "many"), (22.0, "few"), (1.5, "other"), (0.0, "many")]
        {
            assert_eq!(plural_category(n, "pl"), category, "pl {n}");
        }
        assert_eq!(plural_category(1.5, "fr"), "one");
        assert_eq!(plural_category(1.5, "en"), "other");
        assert_eq!(plural_category(1.0, "es"), "one");
        assert_eq!(plural_category(0.0, "es"), "other");
        assert_eq!(plural_category(2_000_000.0, "es"), "many");
    }

    // src/platform/i18n.test.ts › formatMessage › supports select and nesting
    #[test]
    fn supports_select_and_nesting() {
        let m = "{os, select, macos {Finder} windows {Explorer} other {Files}}";
        assert_eq!(fmt(m, &[("os", "macos".into())]), "Finder");
        assert_eq!(fmt(m, &[("os", "linux".into())]), "Files");
        let nested = "{count, plural, one {{who} has # file} other {{who} has # files}}";
        assert_eq!(fmt(nested, &[("count", 3.into()), ("who", "Ann".into())]), "Ann has 3 files");
        let deep = "{os, select, macos {{count, plural, one {# window} other {# windows}}} other {#}}";
        assert_eq!(fmt(deep, &[("os", "macos".into()), ("count", 2.into())]), "2 windows");
        assert_eq!(fmt("{on, select, true {yes} other {no}}", &[("on", true.into())]), "yes");
    }

    // src/platform/i18n.test.ts › formatMessage › falls back to `other` when the parameter is missing
    #[test]
    fn falls_back_to_other_when_the_parameter_is_missing() {
        assert_eq!(fmt("{n, plural, one {one} other {many}}", &[]), "many");
    }

    // src/platform/i18n.test.ts › formatMessage › returns malformed messages unformatted
    #[test]
    fn returns_malformed_messages_unformatted() {
        assert_eq!(fmt("{n, plural, one {x}", &[("n", 1.into())]), "{n, plural, one {x}");
        assert_eq!(fmt("{n, weird, a {b}}", &[("n", 1.into())]), "{n, weird, a {b}}");
        assert!(!is_valid_message("{n, plural, one {x}"));
        assert!(is_valid_message("{n, plural, one {x} other {y}}"));
        assert_eq!(fmt("a } b", &[]), "a } b");
    }

    // src/platform/i18n.test.ts › t / locales › resolves locale tags
    #[test]
    fn resolves_locale_tags() {
        for (tag, locale) in [
            ("zh-CN", "zh"),
            ("zh_Hans", "zh"),
            ("ko-KR", "ko"),
            ("pt-BR", "pt"),
            ("pt_PT", "pt"),
            ("EN-us", "en"),
            ("fr", "fr"),
            ("fr-CA", "fr"),
            ("pl_PL", "pl"),
            ("pl-PL", "pl"),
            ("es-MX", "es"),
            ("es_MX", "es"),
            ("xx", "en"),
        ] {
            assert_eq!(resolve_locale(Some(tag), "en"), locale, "{tag}");
        }
        assert_eq!(resolve_locale(None, "ko-KR"), "ko");
        assert_eq!(resolve_locale(Some("system"), "fr-FR"), "fr");
        assert_eq!(resolve_locale(Some(" auto "), "xx"), "en");
    }

    // src/platform/i18n.test.ts › translates with English fallback, then the key itself
    #[test]
    fn translates_with_english_fallback_then_the_key_itself() {
        let mut i18n = I18n::default();
        assert_eq!(i18n.t("common.cancel", &[]), "Cancel");
        assert!(i18n.set_locale(Some("zh"), "en"));
        assert_eq!(i18n.locale(), "zh");
        assert_eq!(i18n.t("common.cancel", &[]), "取消");
        i18n.register_messages("en", &[("test.onlyEnglish", "Only English")]);
        assert_eq!(i18n.t("test.onlyEnglish", &[]), "Only English");
        assert_eq!(i18n.t("test.missing.key", &[]), "test.missing.key");
        // Unsupported locales fall back to English.
        i18n.set_locale(Some("xx"), "en");
        assert_eq!(i18n.locale(), "en");
    }

    // src/platform/i18n.test.ts › applies params and plurals in the active locale (+ per-locale catalogs)
    #[test]
    fn applies_params_and_plurals_in_the_active_locale() {
        let en = I18n::default();
        assert_eq!(
            en.t("app.unsaved.body", &[("count", 1.into()), ("project", "openidea".into())]),
            "1 file has unsaved changes in openidea. Save before closing?"
        );
        assert_eq!(with_locale("zh-CN").t("app.keymap.count", &[("count", 1.into())]), "1 个快捷键");
        let pt = with_locale("pt-BR");
        assert_eq!(pt.locale(), "pt");
        assert_eq!(pt.t("common.cancel", &[]), "Cancelar");
        assert_eq!(pt.t("app.keymap.count", &[("count", 1.into())]), "1 atalho");
        assert_eq!(pt.t("app.keymap.count", &[("count", 5.into())]), "5 atalhos");
        let pl = with_locale("pl-PL");
        assert_eq!(pl.t("common.cancel", &[]), "Anuluj");
        assert_eq!(pl.t("app.keymap.count", &[("count", 1.into())]), "1 skrót");
        assert_eq!(pl.t("app.keymap.count", &[("count", 5.into())]), "5 skrótów");
        let es = with_locale("es-MX");
        assert_eq!(es.t("common.cancel", &[]), "Cancelar");
        assert_eq!(es.t("app.keymap.count", &[("count", 1.into())]), "1 atajo");
        assert_eq!(es.t("app.keymap.count", &[("count", 5.into())]), "5 atajos");
        let ko = with_locale("ko-KR");
        assert_eq!(ko.t("common.cancel", &[]), "취소");
        assert_eq!(ko.tx("Cancel"), "취소");
        let fr = with_locale("fr-FR");
        assert_eq!(fr.t("common.cancel", &[]), "Annuler");
        assert_eq!(fr.t("app.keymap.count", &[("count", 0.into())]), "0 raccourci");
        assert_eq!(fr.tx("Sync with OS"), "Synchroniser avec le système");
        assert_eq!(fr.tx_template("ruff is not installed"), "ruff n'est pas installé");
    }

    // src/platform/i18n.test.ts › registerMessages adds and removes feature strings
    #[test]
    fn register_messages_adds_and_removes_feature_strings() {
        let mut i18n = I18n::default();
        i18n.register_messages("zh", &[("feature.hello", "你好")]);
        i18n.register_messages("en", &[("feature.hello", "Hello")]);
        assert_eq!(i18n.t("feature.hello", &[]), "Hello");
        i18n.set_locale(Some("zh"), "en");
        assert_eq!(i18n.t("feature.hello", &[]), "你好");
        i18n.unregister_messages("zh", &[("feature.hello", "你好")]);
        assert_eq!(i18n.t("feature.hello", &[]), "Hello");
    }

    // src/platform/i18n.test.ts › translates source text through the locale's text catalog
    #[test]
    fn translates_source_text_through_the_locales_text_catalog() {
        let mut i18n = I18n::default();
        assert_eq!(i18n.tx("Sync with OS"), "Sync with OS");
        i18n.set_locale(Some("zh"), "en");
        assert_eq!(i18n.tx("Sync with OS"), "与操作系统同步");
        assert_eq!(i18n.tx("Some text nobody translated"), "Some text nobody translated");
        assert_eq!(i18n.tx(""), "");
        // A template without values is the same text as a string.
        assert_eq!(i18n.tx_built(&["Sync with OS"], &[]), "与操作系统同步");
    }

    // src/platform/i18n.test.ts › matches source-text templates, translating the substituted text
    #[test]
    fn matches_source_text_templates_translating_the_substituted_text() {
        assert_eq!(I18n::default().tx_template("Push failed"), "Push failed");
        let zh = with_locale("zh");
        assert_eq!(zh.tx_template("ruff is not installed"), "ruff 未安装");
        assert_eq!(zh.tx_template("Language server 'gopls' is not installed"), "语言服务器 'gopls' 未安装");
        assert_eq!(zh.tx_template("Retry failed"), format!("{}失败", zh.tx("Retry")));
        assert_eq!(zh.tx_template("Sync with OS"), "与操作系统同步");
        assert_eq!(zh.tx_template("Something else entirely"), "Something else entirely");
        assert_eq!(
            zh.tx_template("The PHP language server can't rename this symbol; renaming only the plugin's indexed names would leave it half renamed"),
            "语言服务器无法重命名此符号（PHP）；仅重命名插件索引的名称会导致重命名不完整"
        );
        assert_eq!(
            zh.tx_template("The Ruby language server can't rename this symbol; renaming only the plugin's indexed names would leave it half renamed"),
            "语言服务器无法重命名此符号（Ruby 语言）；仅重命名插件索引的名称会导致重命名不完整"
        );
    }

    // src/platform/i18n.test.ts › translates text built by the tx tag, again after the locale changes
    #[test]
    fn translates_built_text_again_after_the_locale_changes() {
        let mut i18n = I18n::default();
        let built = i18n.tx_built(&["Run '", "'"], &["orders".into()]);
        assert_eq!(built, "Run 'orders'");
        assert_eq!(i18n.tx_built(&["Unknown ", " text"], &["orders".into()]), "Unknown orders text");
        i18n.set_locale(Some("zh"), "en");
        // Built before the locale changed: passing it through `tx` translates it.
        assert_eq!(i18n.tx(&built), "运行 'orders'");
        assert_eq!(i18n.tx_template(&built), "运行 'orders'");
        let zh = i18n.tx_built(&["Run '", "'"], &["orders".into()]);
        assert_eq!(zh, "运行 'orders'");
        // Substituted text is translated when the catalog has it.
        assert_eq!(i18n.tx_built(&["Run '", "'"], &["Retry".into()]), format!("运行 '{}'", i18n.tx("Retry")));
        // Values are inserted as they are, braces included.
        assert_eq!(
            i18n.tx_built(&["Drop ", "? This cannot be undone."], &["stash@{0}".into()]),
            "是否丢弃 stash@{0}？此操作无法撤消。"
        );
        i18n.set_locale(Some("en"), "en");
        assert_eq!(i18n.tx(&zh), "Run 'orders'");
        assert_eq!(with_locale("fr").tx_built(&["Run '", "'"], &["orders".into()]), "Exécuter 'orders'");
    }

    // src/platform/i18n.test.ts › lists keys by prefix
    #[test]
    fn lists_keys_by_prefix() {
        let keys = I18n::default().keys_with_prefix("menu.");
        assert!(keys.iter().any(|k| k == "menu.file.save"));
        assert!(keys.iter().all(|k| k.starts_with("menu.")));
    }

    // src/platform/i18n.test.ts › catalogs
    #[test]
    fn catalogs_are_complete_and_well_formed() {
        let en = message_catalog("en");
        assert!(en.len() > 300);
        for locale in LOCALES {
            let messages = message_catalog(locale.id);
            assert!(!messages.is_empty(), "{} has a message catalog", locale.id);
            for (key, message) in messages {
                assert!(is_valid_message(message), "{}:{key} is malformed", locale.id);
                assert!(en.contains_key(key), "{}:{key} is not an English key", locale.id);
            }
            for (source, text) in text_catalog(locale.id) {
                assert!(!text.trim().is_empty(), "{}:{source} is empty", locale.id);
                let mut expected = split_placeholders(source).1;
                let mut actual = split_placeholders(text).1;
                expected.sort();
                actual.sort();
                assert_eq!(actual, expected, "{}:{source} keeps its placeholders", locale.id);
            }
        }
        // Messages use the same parameters across locales.
        let parameters = |message: &str| {
            let mut names = Vec::new();
            let mut rest = message;
            while let Some(open) = rest.find('{') {
                rest = &rest[open + 1..];
                let name: String = rest.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                let after = rest.trim_start()[name.len()..].trim_start();
                if name.starts_with(|c: char| c.is_ascii_lowercase()) && after.starts_with([',', '}']) {
                    names.push(name);
                }
            }
            names.sort();
            names
        };
        for locale in LOCALES.iter().filter(|l| l.id != "en") {
            for (key, message) in message_catalog(locale.id) {
                assert_eq!(
                    parameters(message),
                    parameters(&en[key]),
                    "{}:{key} uses the English parameters",
                    locale.id
                );
            }
        }
        // Written in another script, so a text equal to its English source was not translated.
        const SAME_AS_ENGLISH: [&str; 25] = [
            "Docker",
            "Shell",
            "Java",
            "Python",
            "C/C++",
            "C#",
            "Go",
            "Rust",
            "Kotlin",
            "PHP",
            "SQL",
            "JavaScript",
            "TypeScript",
            "HTML",
            "CSS",
            "JSON",
            "YAML",
            "XML",
            "Maven",
            "Gradle",
            "npm",
            "pnpm",
            "Yarn",
            "Cargo",
            "CMake",
        ];
        for locale in ["zh", "ko"] {
            for (source, text) in text_catalog(locale) {
                assert!(
                    text != source || SAME_AS_ENGLISH.contains(&source.as_str()),
                    "{locale}:{source} is not translated"
                );
            }
        }
        // Chinese must have every key; the other locales show English for keys they lack.
        let zh = message_catalog("zh");
        assert_eq!(zh.len(), en.len());
        assert!(!text_catalog("zh").is_empty() && text_catalog("en").is_empty());
    }

    /// The number formats and plural categories match what the webview's `Intl` answers
    /// (`assets/gen/intl.json`, written by `docs/gpui-port/gen-data.mjs` from Node's `Intl`).
    #[test]
    fn numbers_and_plurals_match_intl() {
        #[derive(serde::Deserialize)]
        struct Row {
            n: f64,
            text: String,
            plural: String,
        }
        let table: HashMap<String, Vec<Row>> =
            serde_json::from_str(include_str!("../../assets/gen/intl.json")).expect("the table parses");
        assert_eq!(table.len(), LOCALES.len());
        let mut failures = Vec::new();
        for (locale, rows) in &table {
            for row in rows {
                if format_number(row.n, locale) != row.text {
                    failures.push(format!(
                        "{locale}: {} formats as {:?}, Intl says {:?}",
                        row.n,
                        format_number(row.n, locale),
                        row.text
                    ));
                }
                if plural_category(row.n, locale) != row.plural {
                    failures.push(format!(
                        "{locale}: {} is {:?}, Intl says {:?}",
                        row.n,
                        plural_category(row.n, locale),
                        row.plural
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn built_texts_drop_the_least_recently_built() {
        let mut built = BuiltTexts::default();
        for i in 0..MAX_BUILT + 5 {
            built.remember(format!("t{i}"), Built { strings: vec![format!("t{i}")], values: vec![] });
        }
        assert_eq!(built.entries.len(), MAX_BUILT);
        assert!(built.get("t0").is_none() && built.get("t4").is_none() && built.get("t5").is_some());
        // Building a text again makes it the most recent.
        built.remember("t5".to_owned(), Built { strings: vec!["t5".to_owned()], values: vec![] });
        built.remember("fresh".to_owned(), Built { strings: vec![], values: vec![] });
        assert!(built.get("t5").is_some() && built.get("t6").is_none());
    }
}
