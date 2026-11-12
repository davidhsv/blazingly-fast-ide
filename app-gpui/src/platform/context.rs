//! Context keys and `when` clauses.
//!
//! Components publish facts (`editorFocus`, `toolWindow.project.active`,
//! `debugSession.paused`…); actions and keybindings are enabled by a `when`
//! expression over those facts. Grammar:
//!
//! ```text
//! expr := or ; or := and ("||" and)* ; and := unary ("&&" unary)*
//! unary := "!" unary | "(" expr ")" | cmp ; cmp := key (("==" | "!=") value)?
//! ```

use gpui_kit::{App, Global, Window, WindowId};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

/// A fact's value. An absent key is falsy and equal to nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum ContextValue {
    Bool(bool),
    Number(f64),
    Text(Arc<str>),
}

impl ContextValue {
    /// Truthiness of a bare key (`editorFocus`): false, 0, NaN and "" are falsy.
    pub fn truthy(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Number(n) => *n != 0.0 && !n.is_nan(),
            Self::Text(s) => !s.is_empty(),
        }
    }
}

impl From<bool> for ContextValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}
impl From<f64> for ContextValue {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}
impl From<i32> for ContextValue {
    fn from(value: i32) -> Self {
        Self::Number(value.into())
    }
}
impl From<usize> for ContextValue {
    fn from(value: usize) -> Self {
        Self::Number(value as f64)
    }
}
impl From<&str> for ContextValue {
    fn from(value: &str) -> Self {
        Self::Text(value.into())
    }
}
impl From<String> for ContextValue {
    fn from(value: String) -> Self {
        Self::Text(value.into())
    }
}

/// The facts a `when` clause is evaluated against.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContextKeys(HashMap<Arc<str>, ContextValue>);

impl ContextKeys {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets `key`; returns whether the value changed.
    pub fn set(&mut self, key: &str, value: impl Into<ContextValue>) -> bool {
        let value = value.into();
        if self.0.get(key) == Some(&value) {
            return false;
        }
        self.0.insert(key.into(), value);
        true
    }

    /// Removes `key`; returns whether it was present.
    pub fn clear(&mut self, key: &str) -> bool {
        self.0.remove(key).is_some()
    }

    pub fn get(&self, key: &str) -> Option<&ContextValue> {
        self.0.get(key)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    pub fn is_truthy(&self, key: &str) -> bool {
        self.0.get(key).is_some_and(ContextValue::truthy)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &ContextValue)> {
        self.0.iter().map(|(k, v)| (&**k, v))
    }
}

impl<K: AsRef<str>, V: Into<ContextValue>> FromIterator<(K, V)> for ContextKeys {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Self(iter.into_iter().map(|(k, v)| (Arc::from(k.as_ref()), v.into())).collect())
    }
}

/// A malformed `when` clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhenError(pub String);

impl fmt::Display for WhenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for WhenError {}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Id(String),
    Str(String),
    Op(&'static str),
}

fn tokenize(src: &str) -> Result<Vec<Token>, WhenError> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(c) = rest.chars().next() {
        if c.is_whitespace() {
            rest = &rest[c.len_utf8()..];
        } else if let Some(op) = ["&&", "||", "==", "!="].into_iter().find(|op| rest.starts_with(op)) {
            out.push(Token::Op(op));
            rest = &rest[2..];
        } else if let Some(op) = ["!", "(", ")"].into_iter().find(|op| rest.starts_with(op)) {
            out.push(Token::Op(op));
            rest = &rest[1..];
        } else if c == '\'' || c == '"' {
            let body = &rest[1..];
            let end = body.find(c).ok_or_else(|| WhenError(format!("unterminated string in when clause: {src}")))?;
            out.push(Token::Str(body[..end].to_owned()));
            rest = &body[end + 1..];
        } else {
            let len = rest
                .char_indices()
                .find(|(_, ch)| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-' | ':' | '/')))
                .map_or(rest.len(), |(i, _)| i);
            if len == 0 {
                return Err(WhenError(format!("unexpected '{c}' in when clause: {src}")));
            }
            out.push(Token::Id(rest[..len].to_owned()));
            rest = &rest[len..];
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Or(Box<Node>, Box<Node>),
    And(Box<Node>, Box<Node>),
    Not(Box<Node>),
    Key(Arc<str>),
    Compare { key: Arc<str>, value: ContextValue, negate: bool },
}

impl Node {
    fn eval(&self, ctx: &ContextKeys) -> bool {
        match self {
            Self::Or(l, r) => l.eval(ctx) || r.eval(ctx),
            Self::And(l, r) => l.eval(ctx) && r.eval(ctx),
            Self::Not(inner) => !inner.eval(ctx),
            Self::Key(key) => ctx.is_truthy(key),
            Self::Compare { key, value, negate } => (ctx.get(key) == Some(value)) != *negate,
        }
    }

    fn keys(&self, out: &mut Vec<Arc<str>>) {
        match self {
            Self::Or(l, r) | Self::And(l, r) => {
                l.keys(out);
                r.keys(out);
            }
            Self::Not(inner) => inner.keys(out),
            Self::Key(key) | Self::Compare { key, .. } => out.push(key.clone()),
        }
    }
}

struct Parser<'a> {
    tokens: Vec<Token>,
    at: usize,
    src: &'a str,
}

impl Parser<'_> {
    fn eat(&mut self, op: &str) -> bool {
        if matches!(self.tokens.get(self.at), Some(Token::Op(o)) if *o == op) {
            self.at += 1;
            return true;
        }
        false
    }

    fn or(&mut self) -> Result<Node, WhenError> {
        let mut left = self.and()?;
        while self.eat("||") {
            left = Node::Or(Box::new(left), Box::new(self.and()?));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Node, WhenError> {
        let mut left = self.unary()?;
        while self.eat("&&") {
            left = Node::And(Box::new(left), Box::new(self.unary()?));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Node, WhenError> {
        if self.eat("!") {
            return Ok(Node::Not(Box::new(self.unary()?)));
        }
        if self.eat("(") {
            let inner = self.or()?;
            if !self.eat(")") {
                return Err(WhenError(format!("missing ')' in when clause: {}", self.src)));
            }
            return Ok(inner);
        }
        let key: Arc<str> = match self.tokens.get(self.at) {
            Some(Token::Id(v) | Token::Str(v)) => v.as_str().into(),
            _ => return Err(WhenError(format!("expected key in when clause: {}", self.src))),
        };
        self.at += 1;
        let negate = if self.eat("==") {
            false
        } else if self.eat("!=") {
            true
        } else {
            return Ok(Node::Key(key));
        };
        let value = match self.tokens.get(self.at) {
            Some(Token::Str(v)) => ContextValue::Text(v.as_str().into()),
            Some(Token::Id(v)) => match v.as_str() {
                "true" => ContextValue::Bool(true),
                "false" => ContextValue::Bool(false),
                raw => match raw.parse::<f64>() {
                    Ok(n) if n.is_finite() => ContextValue::Number(n),
                    _ => ContextValue::Text(raw.into()),
                },
            },
            _ => return Err(WhenError(format!("expected value in when clause: {}", self.src))),
        };
        self.at += 1;
        Ok(Node::Compare { key, value, negate })
    }
}

/// A compiled `when` clause. The empty clause is always true.
#[derive(Debug, Clone, PartialEq)]
pub struct When(Option<Arc<Node>>);

impl When {
    pub const ALWAYS: When = When(None);

    pub fn parse(expr: &str) -> Result<Self, WhenError> {
        if expr.trim().is_empty() {
            return Ok(Self(None));
        }
        let mut parser = Parser { tokens: tokenize(expr)?, at: 0, src: expr };
        let node = parser.or()?;
        if parser.at != parser.tokens.len() {
            return Err(WhenError(format!("unexpected trailing tokens in when clause: {expr}")));
        }
        Ok(Self(Some(Arc::new(node))))
    }

    pub fn eval(&self, ctx: &ContextKeys) -> bool {
        self.0.as_ref().is_none_or(|node| node.eval(ctx))
    }

    /// The context keys the clause reads, in source order.
    pub fn keys(&self) -> Vec<Arc<str>> {
        let mut out = Vec::new();
        if let Some(node) = &self.0 {
            node.keys(&mut out);
        }
        out
    }
}

fn cache() -> &'static RwLock<HashMap<String, Result<When, WhenError>>> {
    static CACHE: OnceLock<RwLock<HashMap<String, Result<When, WhenError>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Compiles (and caches) a when clause.
pub fn compile_when(expr: &str) -> Result<When, WhenError> {
    if expr.trim().is_empty() {
        return Ok(When::ALWAYS);
    }
    if let Some(hit) = cache().read().get(expr) {
        return hit.clone();
    }
    let compiled = When::parse(expr);
    cache().write().insert(expr.to_owned(), compiled.clone());
    compiled
}

/// Evaluates `expr` against `ctx`; `None` (no clause) is true.
pub fn evaluate_when(expr: Option<&str>, ctx: &ContextKeys) -> Result<bool, WhenError> {
    match expr {
        None => Ok(true),
        Some(expr) => Ok(compile_when(expr)?.eval(ctx)),
    }
}

/// Each window's facts. The webview UI keeps them in a per-webview store; here one process
/// serves every window, so they are kept per window.
#[derive(Default)]
pub struct ContextStore {
    windows: HashMap<WindowId, Rc<ContextKeys>>,
    /// How many times a fact of any window changed: what follows the facts compares it.
    changes: u64,
}

impl Global for ContextStore {}

/// The context keys of `window`.
pub fn window_context(window: &Window, cx: &App) -> Rc<ContextKeys> {
    cx.try_global::<ContextStore>()
        .and_then(|store| store.windows.get(&window.window_handle().window_id()).cloned())
        .unwrap_or_default()
}

/// Publishes a fact for `window`; the window is redrawn when the value changed.
pub fn set_context(window: &mut Window, cx: &mut App, key: &str, value: impl Into<ContextValue>) {
    let id = window.window_handle().window_id();
    let store = cx.default_global::<ContextStore>();
    if Rc::make_mut(store.windows.entry(id).or_default()).set(key, value) {
        store.changes += 1;
        window.refresh();
    }
}

/// A count that grows whenever a fact of any window changes.
pub fn context_changes(cx: &App) -> u64 {
    cx.try_global::<ContextStore>().map_or(0, |store| store.changes)
}

/// Withdraws a fact of `window`.
pub fn clear_context(window: &mut Window, cx: &mut App, key: &str) {
    let id = window.window_handle().window_id();
    let cleared = cx
        .default_global::<ContextStore>()
        .windows
        .get_mut(&id)
        .is_some_and(|keys| keys.contains(key) && Rc::make_mut(keys).clear(key));
    if cleared {
        cx.default_global::<ContextStore>().changes += 1;
        window.refresh();
    }
}

/// Forgets a closed window's facts.
pub fn drop_window_context(window: WindowId, cx: &mut App) {
    cx.default_global::<ContextStore>().windows.remove(&window);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ContextKeys {
        let mut ctx = ContextKeys::new();
        ctx.set("editorFocus", true);
        ctx.set("lang", "rust");
        ctx.set("count", 2);
        ctx.set("debug", false);
        ctx
    }

    // src/platform/context.test.ts › when clauses
    #[test]
    fn when_clauses() {
        for (expr, expected) in [
            ("", true),
            ("editorFocus", true),
            ("!editorFocus", false),
            ("editorFocus && debug", false),
            ("editorFocus || debug", true),
            ("lang == rust", true),
            ("lang == 'rust'", true),
            ("lang != rust", false),
            ("count == 2", true),
            ("!(debug || !editorFocus)", true),
            ("missing", false),
        ] {
            assert_eq!(evaluate_when(Some(expr), &ctx()), Ok(expected), "{expr}");
        }
    }

    // src/platform/context.test.ts › rejects malformed clauses
    #[test]
    fn rejects_malformed_clauses() {
        assert!(evaluate_when(Some("a &&"), &ctx()).is_err());
        assert!(evaluate_when(Some("(a"), &ctx()).is_err());
        assert!(evaluate_when(Some("a b"), &ctx()).is_err());
        assert!(evaluate_when(Some("a == 'x"), &ctx()).is_err());
        assert!(evaluate_when(Some("a == )"), &ctx()).is_err());
    }

    // src/platform/context.test.ts › clearContext
    #[test]
    fn clear_deletes_the_key_and_keeps_the_others() {
        let mut ctx = ContextKeys::new();
        assert!(ctx.set("a.x", 1));
        assert!(ctx.set("a.y", 2));
        assert!(ctx.clear("a.x"));
        assert!(!ctx.contains("a.x"));
        assert_eq!(ctx.get("a.y"), Some(&ContextValue::Number(2.0)));
        let before = ctx.clone();
        assert!(!ctx.clear("a.never"));
        assert_eq!(ctx, before);
    }

    #[test]
    fn setting_the_same_value_is_not_a_change() {
        let mut ctx = ContextKeys::new();
        assert!(ctx.set("k", "v"));
        assert!(!ctx.set("k", "v"));
        assert!(ctx.set("k", true));
    }

    #[test]
    fn comparison_is_strict_about_types() {
        let mut ctx = ContextKeys::new();
        ctx.set("n", "2");
        ctx.set("b", "true");
        assert_eq!(evaluate_when(Some("n == 2"), &ctx), Ok(false));
        assert_eq!(evaluate_when(Some("n == '2'"), &ctx), Ok(true));
        assert_eq!(evaluate_when(Some("b == true"), &ctx), Ok(false));
        assert_eq!(evaluate_when(Some("absent != x"), &ctx), Ok(true));
    }

    #[test]
    fn keys_lists_what_a_clause_reads() {
        let when = compile_when("editorFocus && !(lang == rust || toolWindow.project.active)").unwrap();
        let keys: Vec<String> = when.keys().iter().map(|k| k.to_string()).collect();
        assert_eq!(keys, ["editorFocus", "lang", "toolWindow.project.active"]);
    }
}
