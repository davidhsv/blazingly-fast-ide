//! The Logcat tool window's model: the level filter, filter expressions in the shape Android
//! Studio accepts, and the history of recent filters.
//!
//! Filter expressions:
//!
//! ```text
//! tag:glide            the tag contains "glide"
//! message:boom         the message contains "boom" (also `msg:`)
//! pid:4242             the process (or thread) id is 4242
//! level:error          the level is error (or `level:warn,error`, names or letters)
//! -tag:chatty          "not": drops matching lines (also `!`)
//! a b                  both, implicitly and-ed (`&` works too)
//! a|b                  either, or-ed
//! (a|b) -c             grouping
//! message~:^Tile.*     a case-insensitive regular expression
//! message:"two words"  quoted values, single or double
//! ```
//!
//! A bare word matches the tag, the message, or `#pid`.

use ide_android::{LogcatLevel, LogcatLine};
use regex::Regex;

/// Severity order of the level filter, least severe first. `Unknown` sits last so lines logcat
/// prints outside its message format are never filtered away.
pub const LEVELS: [LogcatLevel; 7] = [
    LogcatLevel::Verbose,
    LogcatLevel::Debug,
    LogcatLevel::Info,
    LogcatLevel::Warn,
    LogcatLevel::Error,
    LogcatLevel::Fatal,
    LogcatLevel::Unknown,
];

/// The levels the filter offers as a minimum; `Unknown` always shows, so it is not one.
pub const FILTER_LEVELS: [LogcatLevel; 6] = [
    LogcatLevel::Verbose,
    LogcatLevel::Debug,
    LogcatLevel::Info,
    LogcatLevel::Warn,
    LogcatLevel::Error,
    LogcatLevel::Fatal,
];

/// The row style of a level.
pub fn level_class(level: LogcatLevel) -> &'static str {
    match level {
        LogcatLevel::Verbose => "levelV",
        LogcatLevel::Debug => "levelD",
        LogcatLevel::Info => "levelI",
        LogcatLevel::Warn => "levelW",
        LogcatLevel::Error => "levelE",
        LogcatLevel::Fatal => "levelF",
        LogcatLevel::Unknown => "levelU",
    }
}

fn rank(level: LogcatLevel) -> usize {
    LEVELS.iter().position(|l| *l == level).unwrap_or(0)
}

/// "11-04 12:52:39.123" → "12:52:39.123": the date is the same for the whole session.
pub fn short_time(time: &str) -> &str {
    match time.char_indices().nth(6) {
        Some((at, _)) if time.encode_utf16().count() > 6 => &time[at..],
        _ => time,
    }
}

/// Lines at or above `min` that the query accepts (an invalid query accepts every line).
pub fn filter_lines<'a>(lines: &'a [LogcatLine], min: LogcatLevel, query: &str) -> Vec<&'a LogcatLine> {
    let floor = rank(min);
    let filter = compile_query(query);
    lines.iter().filter(|line| rank(line.level) >= floor && filter.test(line)).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Tag,
    Message,
    Pid,
    Level,
    Text,
}

#[derive(Debug)]
enum Matcher {
    Contains(String),
    Pattern(Regex),
    Levels(Vec<LogcatLevel>),
}

#[derive(Debug)]
enum Node {
    Term { field: Field, value: String, matcher: Matcher },
    Not(Box<Node>),
    And(Vec<Node>),
    Or(Vec<Node>),
}

fn level_named(name: &str) -> Option<LogcatLevel> {
    Some(match name {
        "verbose" | "v" => LogcatLevel::Verbose,
        "debug" | "d" => LogcatLevel::Debug,
        "info" | "i" => LogcatLevel::Info,
        "warn" | "warning" | "w" => LogcatLevel::Warn,
        "error" | "e" => LogcatLevel::Error,
        "fatal" | "assert" | "a" | "f" => LogcatLevel::Fatal,
        "unknown" => LogcatLevel::Unknown,
        _ => return None,
    })
}

/// One word as a term; unknown `key:value` pairs stay plain text.
fn term(raw: &str) -> Result<Node, String> {
    let (field, value, regex) = match raw.find(':').filter(|&at| at > 0) {
        Some(at) => {
            let key = raw[..at].to_lowercase();
            let (key, regex) = match key.strip_suffix('~') {
                Some(key) => (key.to_owned(), true),
                None => (key, false),
            };
            let field = match key.as_str() {
                "tag" => Some(Field::Tag),
                "message" | "msg" => Some(Field::Message),
                "pid" => Some(Field::Pid),
                "level" => Some(Field::Level),
                _ => None,
            };
            match field {
                Some(field) => (field, raw[at + 1..].to_owned(), regex),
                None => (Field::Text, raw.to_owned(), false),
            }
        }
        None => (Field::Text, raw.to_owned(), false),
    };
    let matcher = if field == Field::Level {
        let mut chosen = Vec::new();
        for name in value.split(',').map(|part| part.trim().to_lowercase()) {
            let level = level_named(&name).ok_or_else(|| format!("Unknown level “{name}”"))?;
            chosen.push(level);
        }
        Matcher::Levels(chosen)
    } else if regex {
        Matcher::Pattern(Regex::new(&format!("(?i){value}")).map_err(|e| e.to_string())?)
    } else {
        Matcher::Contains(value.to_lowercase())
    };
    Ok(Node::Term { field, value, matcher })
}

struct Parser<'a> {
    source: &'a str,
    index: usize,
}

impl Parser<'_> {
    /// Skips whitespace; the next character, if any.
    fn peek(&mut self) -> Option<char> {
        while let Some(c) = self.source[self.index..].chars().next().filter(|c| c.is_whitespace()) {
            self.index += c.len_utf8();
        }
        self.source[self.index..].chars().next()
    }

    fn bump(&mut self, c: char) {
        self.index += c.len_utf8();
    }

    fn parse(&mut self) -> Result<Option<Node>, String> {
        let node = self.or()?;
        match self.peek() {
            None => Ok(node),
            Some(c) => Err(format!("Unexpected “{c}”")),
        }
    }

    fn or(&mut self) -> Result<Option<Node>, String> {
        let mut nodes = Vec::new();
        if let Some(first) = self.and()? {
            nodes.push(first);
        }
        while self.peek() == Some('|') {
            self.bump('|');
            nodes.push(self.and()?.ok_or("Nothing to match after |")?);
        }
        Ok(match nodes.len() {
            0 => None,
            1 => nodes.pop(),
            _ => Some(Node::Or(nodes)),
        })
    }

    fn and(&mut self) -> Result<Option<Node>, String> {
        let mut nodes = Vec::new();
        loop {
            match self.peek() {
                None | Some('|' | ')') => break,
                Some('&') => self.bump('&'),
                Some(_) => nodes.push(self.unary()?),
            }
        }
        Ok(match nodes.len() {
            0 => None,
            1 => nodes.pop(),
            _ => Some(Node::And(nodes)),
        })
    }

    fn unary(&mut self) -> Result<Node, String> {
        let next = self.peek();
        match next {
            Some(c @ ('-' | '!')) => {
                self.bump(c);
                Ok(Node::Not(Box::new(self.unary()?)))
            }
            Some('(') => {
                self.bump('(');
                let node = self.or()?.ok_or("Empty parentheses")?;
                if self.peek() != Some(')') {
                    return Err("Missing )".to_owned());
                }
                self.bump(')');
                Ok(node)
            }
            _ => {
                let word = self.word();
                if word.is_empty() {
                    return Err(format!("Unexpected “{}”", next.map(String::from).unwrap_or_default()));
                }
                term(&word)
            }
        }
    }

    /// A run of characters up to whitespace or a separator, quotes removed.
    fn word(&mut self) -> String {
        let mut word = String::new();
        while let Some(c) = self.source[self.index..].chars().next() {
            if c.is_whitespace() || matches!(c, '|' | '&' | ')' | '(') {
                break;
            }
            self.bump(c);
            if c == '"' || c == '\'' {
                let rest = &self.source[self.index..];
                let end = rest.find(c).unwrap_or(rest.len());
                word.push_str(&rest[..end]);
                self.index += end;
                if self.index < self.source.len() {
                    self.bump(c);
                }
                continue;
            }
            word.push(c);
        }
        word
    }
}

/// `Number(value)` for an id: blank is 0, anything else must be a number.
fn number(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() { Some(0.0) } else { value.parse().ok() }
}

impl Node {
    fn test(&self, line: &LogcatLine) -> bool {
        match self {
            Self::Term { field, value, matcher } => {
                let hit = |text: &str| match matcher {
                    Matcher::Contains(v) if v.is_empty() => text.is_empty(),
                    Matcher::Contains(v) => text.to_lowercase().contains(v.as_str()),
                    Matcher::Pattern(re) => re.is_match(text),
                    Matcher::Levels(_) => false,
                };
                match field {
                    Field::Tag => hit(&line.tag),
                    Field::Message => hit(&line.message),
                    Field::Pid => number(value).is_some_and(|n| f64::from(line.pid) == n || f64::from(line.tid) == n),
                    Field::Level => matches!(matcher, Matcher::Levels(levels) if levels.contains(&line.level)),
                    Field::Text => {
                        let lower = value.to_lowercase();
                        let hash = lower.strip_prefix('#');
                        hit(&line.message)
                            || hit(&line.tag)
                            || hash.is_some_and(|h| line.pid.to_string() == h || line.tid.to_string() == h)
                    }
                }
            }
            Self::Not(node) => !node.test(line),
            Self::And(nodes) => nodes.iter().all(|n| n.test(line)),
            Self::Or(nodes) => nodes.iter().any(|n| n.test(line)),
        }
    }
}

/// A compiled filter expression.
#[derive(Debug)]
pub struct LogFilter {
    node: Option<Node>,
    /// Why an invalid query was ignored.
    pub error: Option<String>,
}

impl LogFilter {
    /// Whether a line passes; every line passes when the query is empty or invalid.
    pub fn test(&self, line: &LogcatLine) -> bool {
        self.node.as_ref().is_none_or(|node| node.test(line))
    }
}

/// Parses and compiles a query.
pub fn compile_query(query: &str) -> LogFilter {
    let source = query.trim();
    if source.is_empty() {
        return LogFilter { node: None, error: None };
    }
    match (Parser { source, index: 0 }).parse() {
        Ok(node) => LogFilter { node, error: None },
        Err(error) => LogFilter { node: None, error: Some(error) },
    }
}

/// Where the filter history is stored.
pub const FILTER_HISTORY_KEY: &str = "openidea.logcat.filters";
const MAX_FILTERS: usize = 20;

/// The recent filters, newest first, from the stored value (anything but a list of strings
/// reads as none).
pub fn load_filters(stored: Option<&str>) -> Vec<String> {
    let Some(serde_json::Value::Array(items)) = stored.and_then(|raw| serde_json::from_str(raw).ok()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(serde_json::Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .take(MAX_FILTERS)
        .map(str::to_owned)
        .collect()
}

/// Puts `query` first (an empty one is not worth remembering): the new list and the value to
/// store under [`FILTER_HISTORY_KEY`].
pub fn remember_filter(stored: Option<&str>, query: &str) -> (Vec<String>, String) {
    let text = query.trim();
    let mut list: Vec<String> = load_filters(stored).into_iter().filter(|q| q != text).collect();
    if !text.is_empty() {
        list.insert(0, text.to_owned());
        list.truncate(MAX_FILTERS);
    }
    let stored = serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_owned());
    (list, stored)
}

/// Forgets every filter: the value to store.
pub fn forget_filters() -> String {
    "[]".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(level: LogcatLevel, tag: &str, message: &str, pid: i32) -> LogcatLine {
        LogcatLine {
            time: "11-04 12:52:39.123".to_owned(),
            pid,
            tid: pid + 1,
            level,
            tag: tag.to_owned(),
            message: message.to_owned(),
        }
    }

    fn lines() -> Vec<LogcatLine> {
        vec![
            line(LogcatLevel::Verbose, "chatty", "starting up", 100),
            line(LogcatLevel::Info, "ActivityManager", "Displayed com.example/.Main", 100),
            line(LogcatLevel::Warn, "Glide", "Failed to load bitmap", 100),
            line(LogcatLevel::Error, "flutter", "IllegalStateException: boom", 4242),
            line(LogcatLevel::Unknown, "", "--------- beginning of main", 100),
        ]
    }

    fn tags_at(min: LogcatLevel, query: &str) -> Vec<String> {
        filter_lines(&lines(), min, query).into_iter().map(|l| l.tag.clone()).collect()
    }

    fn tags(query: &str) -> Vec<String> {
        tags_at(LogcatLevel::Verbose, query)
    }

    fn count(min: LogcatLevel, query: &str) -> usize {
        filter_lines(&lines(), min, query).len()
    }

    #[test]
    fn keeps_everything_at_the_least_severe_level() {
        assert_eq!(count(LogcatLevel::Verbose, ""), 5);
    }

    #[test]
    fn drops_lines_below_the_chosen_level() {
        assert_eq!(tags_at(LogcatLevel::Warn, ""), ["Glide", "flutter", ""]);
        assert_eq!(tags_at(LogcatLevel::Error, ""), ["flutter", ""]);
    }

    #[test]
    fn matches_the_tag_the_message_and_pid() {
        assert_eq!(count(LogcatLevel::Verbose, "glide"), 1);
        assert_eq!(count(LogcatLevel::Verbose, "bitmap"), 1);
        assert_eq!(tags("#4242"), ["flutter"]);
    }

    #[test]
    fn combines_level_and_text() {
        assert_eq!(count(LogcatLevel::Warn, "boom"), 1);
        assert_eq!(count(LogcatLevel::Error, "bitmap"), 0);
    }

    #[test]
    fn drops_what_a_negated_term_matches() {
        assert_eq!(tags("-message:boom"), ["chatty", "ActivityManager", "Glide", ""]);
        assert_eq!(tags("-tag:glide -tag:flutter"), ["chatty", "ActivityManager", ""]);
    }

    #[test]
    fn ands_adjacent_terms_and_ors_them_with_a_bar() {
        assert_eq!(tags("tag:glide message:bitmap"), ["Glide"]);
        assert_eq!(tags("tag:glide|tag:flutter"), ["Glide", "flutter"]);
        assert_eq!(tags("tag:glide | tag:flutter"), ["Glide", "flutter"]);
    }

    #[test]
    fn groups_so_a_negation_can_apply_to_either_term() {
        assert_eq!(tags("(tag:glide|tag:flutter) -level:error"), ["Glide"]);
        assert_eq!(tags("-(tag:glide|tag:flutter)"), ["chatty", "ActivityManager", ""]);
    }

    #[test]
    fn matches_a_level_by_name_letter_or_list() {
        assert_eq!(tags("level:error"), ["flutter"]);
        assert_eq!(tags("level:w,e"), ["Glide", "flutter"]);
        assert_eq!(tags("level:ERROR"), ["flutter"]);
    }

    #[test]
    fn matches_pid_and_tid_and_still_hash_pid() {
        assert_eq!(tags("pid:4242"), ["flutter"]);
        assert_eq!(tags("pid:4243"), ["flutter"]);
        assert_eq!(tags("#4242"), ["flutter"]);
    }

    #[test]
    fn takes_a_regular_expression_after_tilde() {
        assert_eq!(tags("message~:^Displayed"), ["ActivityManager"]);
        assert_eq!(tags("tag~:^ACTIVITY"), ["ActivityManager"]);
    }

    #[test]
    fn reads_a_quoted_value_including_an_empty_one() {
        assert!(tags("message:\"IllegalState boom\"").is_empty());
        assert_eq!(tags("message:'IllegalStateException: boom'"), ["flutter"]);
        assert_eq!(tags("tag:\"\""), [""]);
    }

    #[test]
    fn treats_an_unknown_key_as_plain_text() {
        assert!(tags("package:com.example").is_empty());
    }

    #[test]
    fn ignores_an_invalid_query_and_says_why() {
        assert!(compile_query("tag:((").error.is_some());
        assert!(compile_query("message~:(").error.is_some());
        assert!(compile_query("").error.is_none());
        assert_eq!(tags("tag:((").len(), 5);
    }

    #[test]
    fn orders_levels_least_severe_first_with_unknown_last_so_it_is_never_hidden() {
        assert_eq!(LEVELS[0], LogcatLevel::Verbose);
        assert_eq!(LEVELS.last(), Some(&LogcatLevel::Unknown));
        assert!(rank(LogcatLevel::Error) > rank(LogcatLevel::Warn));
        assert_eq!(level_class(LogcatLevel::Error), "levelE");
        assert!(!FILTER_LEVELS.contains(&LogcatLevel::Unknown));
    }

    #[test]
    fn shortens_a_logcat_timestamp_to_the_time_of_day() {
        assert_eq!(short_time("11-04 12:52:39.123"), "12:52:39.123");
        assert_eq!(short_time("12:52"), "12:52");
    }

    #[test]
    fn keeps_the_newest_first_without_duplicates() {
        let (_, stored) = remember_filter(None, "tag:aa");
        let (_, stored) = remember_filter(Some(&stored), "tag:bb");
        assert_eq!(load_filters(Some(&stored)), ["tag:bb", "tag:aa"]);
        assert_eq!(remember_filter(Some(&stored), "  tag:aa ").0, ["tag:aa", "tag:bb"]);
    }

    #[test]
    fn takes_up_down_order_from_the_list_and_ignores_an_empty_filter() {
        let (list, mut stored) = remember_filter(Some(&forget_filters()), "   ");
        assert!(list.is_empty());
        for i in 0..30 {
            stored = remember_filter(Some(&stored), &format!("tag:{i}")).1;
        }
        let list = load_filters(Some(&stored));
        assert_eq!(list.len(), 20);
        assert_eq!(list[0], "tag:29");
        assert_eq!(list.last().map(String::as_str), Some("tag:10"));
    }

    #[test]
    fn survives_storage_content_that_is_not_a_list() {
        assert!(load_filters(Some("{not json")).is_empty());
        assert_eq!(load_filters(Some(r#"[1, "", "tag:x"]"#)), ["tag:x"]);
    }

    #[test]
    fn works_when_storage_is_unavailable() {
        assert!(load_filters(None).is_empty());
        assert_eq!(remember_filter(None, "tag:aa").0, ["tag:aa"]);
    }
}
