//! First-match lookup over an ordered list of file-name patterns (regular expressions).
//!
//! Most such patterns pin down the extension (`\.ya?ml$`) or the first character (`^README`) of the names
//! they match. The index reads those keys off each pattern's source, so a lookup only tests the patterns
//! filed under the name's own extension and first character, plus the few that pin down neither.

use std::collections::HashMap;

/// Most alternatives a pattern fragment may expand to before it counts as open-ended.
const MAX_STRINGS: usize = 64;
/// Characters with a meaning of their own in a regular expression.
const SPECIAL: &str = ".*+?{}()[]|^$\\";
/// Characters that stand for themselves after a backslash.
const ESCAPED_LITERALS: &str = ".-_/+()[]{}$^|?*\\";

struct Cursor<'a> {
    src: &'a [char],
    at: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<char> {
        self.src.get(self.at).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.src.get(self.at + 1).copied()
    }
}

/// Every concatenation of one string of `a` with one of `b`.
fn product(a: &[String], b: &[String]) -> Option<Vec<String>> {
    if a.len() * b.len() > MAX_STRINGS {
        return None;
    }
    Some(a.iter().flat_map(|x| b.iter().map(move |y| format!("{x}{y}"))).collect())
}

/// The characters of a `[...]` class; `None` for negated classes, ranges and anything else open-ended.
fn parse_class(c: &mut Cursor<'_>) -> Option<Vec<String>> {
    let mut chars = Vec::new();
    c.at += 1;
    while let Some(ch) = c.peek().filter(|&ch| ch != ']') {
        if ch == '^' || ch == '[' || (ch == '-' && !chars.is_empty() && c.peek_next() != Some(']')) {
            return None;
        }
        if ch == '\\' {
            let next = c.peek_next().filter(|&next| ESCAPED_LITERALS.contains(next))?;
            chars.push(next.to_string());
            c.at += 2;
            continue;
        }
        chars.push(ch.to_string());
        c.at += 1;
    }
    if c.peek() != Some(']') || chars.is_empty() {
        return None;
    }
    c.at += 1;
    Some(chars)
}

/// The strings one atom (a literal, an escape, a class or a group) can match.
fn parse_atom(c: &mut Cursor<'_>) -> Option<Vec<String>> {
    match c.peek()? {
        '(' => {
            c.at += if c.src[c.at..].starts_with(&['(', '?', ':']) { 3 } else { 1 };
            if c.peek() == Some('?') {
                return None;
            }
            let inner = parse_alternation(c)?;
            if c.peek() != Some(')') {
                return None;
            }
            c.at += 1;
            Some(inner)
        }
        '[' => parse_class(c),
        '\\' => {
            let next = c.peek_next().filter(|&next| ESCAPED_LITERALS.contains(next))?;
            c.at += 2;
            Some(vec![next.to_string()])
        }
        ch if SPECIAL.contains(ch) => None,
        ch => {
            c.at += 1;
            Some(vec![ch.to_string()])
        }
    }
}

fn parse_concat(c: &mut Cursor<'_>) -> Option<Vec<String>> {
    let mut out = vec![String::new()];
    while c.peek().is_some_and(|ch| ch != '|' && ch != ')') {
        let mut options = parse_atom(c)?;
        match c.peek() {
            Some('*' | '+' | '{') => return None,
            Some('?') => {
                options.push(String::new());
                c.at += 1;
            }
            _ => {}
        }
        out = product(&out, &options)?;
    }
    Some(out)
}

fn parse_alternation(c: &mut Cursor<'_>) -> Option<Vec<String>> {
    let mut out = Vec::new();
    loop {
        out.extend(parse_concat(c)?);
        if out.len() > MAX_STRINGS {
            return None;
        }
        if c.peek() != Some('|') {
            return Some(out);
        }
        c.at += 1;
    }
}

/// Every string `src` can match, when it is a small finite set.
fn finite_strings(src: &[char]) -> Option<Vec<String>> {
    let mut c = Cursor { src, at: 0 };
    let out = parse_alternation(&mut c)?;
    (c.at == src.len()).then_some(out)
}

struct TopLevel {
    /// The pattern has a `|` outside every group.
    alternation: bool,
    /// Index of the backslash of the last `\.` outside every group and class.
    last_dot: Option<usize>,
}

fn scan_top_level(src: &[char]) -> TopLevel {
    let mut top = TopLevel { alternation: false, last_dot: None };
    let mut depth = 0i32;
    let mut in_class = false;
    let mut i = 0;
    while let Some(&ch) = src.get(i) {
        match ch {
            '\\' => {
                if !in_class && depth == 0 && src.get(i + 1) == Some(&'.') {
                    top.last_dot = Some(i);
                }
                i += 1;
            }
            ']' if in_class => in_class = false,
            _ if in_class => {}
            '[' => in_class = true,
            '(' => depth += 1,
            ')' => depth -= 1,
            '|' if depth == 0 => top.alternation = true,
            _ => {}
        }
        i += 1;
    }
    top
}

/// The items in first-seen order, each once.
fn unique<T: PartialEq>(items: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut out = Vec::new();
    for item in items {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

/// The extensions (text after the last `.`) of every name the pattern `src` can match, when the pattern
/// ends in `\.` followed by a small finite set of endings and `$`; `None` when it does not pin them down.
pub fn extension_keys(src: &str) -> Option<Vec<String>> {
    if !src.ends_with('$') || src.ends_with("\\$") {
        return None;
    }
    let chars: Vec<char> = src.chars().collect();
    let TopLevel { alternation: false, last_dot: Some(dot) } = scan_top_level(&chars) else { return None };
    let tail = finite_strings(chars.get(dot + 2..chars.len() - 1)?)?;
    Some(unique(tail.iter().map(|s| s.rsplit_once('.').map_or(s.as_str(), |(_, ext)| ext).to_owned())))
}

/// The characters every match of the pattern `src` can start with, when the pattern is anchored with `^`
/// and opens with a small finite set of alternatives; `None` when it does not pin them down.
pub fn first_char_keys(src: &str) -> Option<Vec<char>> {
    let chars: Vec<char> = src.chars().collect();
    if chars.first() != Some(&'^') || scan_top_level(&chars).alternation {
        return None;
    }
    let mut c = Cursor { src: &chars, at: 1 };
    let atom = parse_atom(&mut c)?;
    if matches!(c.peek(), Some('?' | '*' | '+' | '{')) {
        return None;
    }
    let firsts = atom.iter().map(|s| s.chars().next()).collect::<Option<Vec<char>>>()?;
    Some(unique(firsts))
}

/// A regular expression in an ordered pattern list.
pub trait IndexedPattern {
    /// Source text of the regular expression.
    fn source(&self) -> &str;
    /// Whether the pattern is tested against the full path instead of the file name.
    fn path(&self) -> bool;
    /// Position in the list; among the patterns that match, the lowest wins.
    fn order(&self) -> usize;
    /// Whether the regular expression matches `subject`.
    fn is_match(&self, subject: &str) -> bool;
}

/// The text `pattern` is tested against: the path for path patterns (`None` when there is no path), the
/// file name otherwise.
pub fn subject<'a, T: IndexedPattern>(pattern: &T, name: &'a str, path: Option<&'a str>) -> Option<&'a str> {
    if pattern.path() { path } else { Some(name) }
}

/// Patterns filed by the extension or first character they require. `patterns` must be in ascending
/// [`IndexedPattern::order`].
pub struct PatternIndex<T> {
    patterns: Vec<T>,
    /// Positions in `patterns` by lowercased extension.
    by_ext: HashMap<String, Vec<usize>>,
    /// Positions in `patterns` by lowercased first character.
    by_first: HashMap<String, Vec<usize>>,
    /// Positions of the patterns that pin down neither.
    generic: Vec<usize>,
}

impl<T: IndexedPattern> PatternIndex<T> {
    /// Files each pattern under the keys read off its source.
    pub fn new(patterns: Vec<T>) -> Self {
        let mut by_ext: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_first: HashMap<String, Vec<usize>> = HashMap::new();
        let mut generic = Vec::new();
        for (at, p) in patterns.iter().enumerate() {
            if let Some(ext) = extension_keys(p.source()) {
                for key in unique(ext.iter().map(|k| k.to_lowercase())) {
                    by_ext.entry(key).or_default().push(at);
                }
                continue;
            }
            let first = if p.path() { None } else { first_char_keys(p.source()) };
            let Some(first) = first else {
                generic.push(at);
                continue;
            };
            for key in unique(first.iter().map(|k| k.to_lowercase().collect::<String>())) {
                by_first.entry(key).or_default().push(at);
            }
        }
        Self { patterns, by_ext, by_first, generic }
    }

    /// The first pattern, by order, whose regular expression matches the file `name` (or its `path`, for
    /// path patterns).
    pub fn find(&self, name: &str, path: Option<&str>) -> Option<&T> {
        self.find_by(name, path, |p, name, path| subject(p, name, path).is_some_and(|s| p.is_match(s)))
    }

    /// The first pattern, by order, that `accept` takes for the file `name` at `path`. `accept` is only
    /// asked about patterns whose keys fit the name.
    pub fn find_by(
        &self,
        name: &str,
        path: Option<&str>,
        accept: impl Fn(&T, &str, Option<&str>) -> bool,
    ) -> Option<&T> {
        let ext = name.rsplit_once('.').and_then(|(_, ext)| self.by_ext.get(&ext.to_lowercase()));
        let first = name.chars().next().and_then(|c| self.by_first.get(&c.to_lowercase().collect::<String>()));
        let lists: [&[usize]; 3] = [
            self.generic.as_slice(),
            ext.map(Vec::as_slice).unwrap_or_default(),
            first.map(Vec::as_slice).unwrap_or_default(),
        ];
        let mut heads = [0usize; 3];
        loop {
            let next = (0..lists.len()).filter_map(|list| lists[list].get(heads[list]).map(|&at| (list, at)));
            let (list, at) = next.min_by_key(|&(_, at)| self.patterns[at].order())?;
            heads[list] += 1;
            let pattern = &self.patterns[at];
            if accept(pattern, name, path) {
                return Some(pattern);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext(src: &str) -> Option<String> {
        extension_keys(src).map(|mut keys| {
            keys.sort();
            keys.join(",")
        })
    }

    fn first(src: &str) -> Option<String> {
        first_char_keys(src).map(|mut keys| {
            keys.sort_unstable();
            keys.into_iter().collect()
        })
    }

    // src/ui/patternIndex.test.ts › pattern index keys › derives the extensions a pattern can end with
    #[test]
    fn derives_the_extensions_a_pattern_can_end_with() {
        assert_eq!(ext("\\.ya?ml$").as_deref(), Some("yaml,yml"));
        assert_eq!(ext(".*\\.(es|edql)$").as_deref(), Some("edql,es"));
        assert_eq!(ext("\\.[cm]?js$").as_deref(), Some("cjs,js,mjs"));
        assert_eq!(ext("\\.(tar\\.gz|tgz)$").as_deref(), Some("gz,tgz"));
        assert_eq!(extension_keys("^Cargo\\.toml$"), Some(vec!["toml".to_owned()]));
        assert_eq!(extension_keys("/(tests?)/.*\\.java$"), Some(vec!["java".to_owned()]));
        assert_eq!(extension_keys("\\.js"), None);
        assert_eq!(extension_keys("\\.(md|[a-z]+)$"), None);
        assert_eq!(extension_keys("a\\.md$|b\\.txt$"), None);
        assert_eq!(extension_keys("(architecture)(\\.(md|txt))?$"), None);
    }

    // src/ui/patternIndex.test.ts › pattern index keys › derives the characters an anchored pattern can start
    // with
    #[test]
    fn derives_the_characters_an_anchored_pattern_can_start_with() {
        assert_eq!(first_char_keys("^README"), Some(vec!['R']));
        assert_eq!(first("^(README|readme)(\\.md)?$").as_deref(), Some("Rr"));
        assert_eq!(first("^[Mm]akefile$").as_deref(), Some("Mm"));
        assert_eq!(first_char_keys("^\\.babelrc$"), Some(vec!['.']));
        assert_eq!(first_char_keys("^\\.?kitchen"), None);
        assert_eq!(first_char_keys("README"), None);
        assert_eq!(first_char_keys("^a|b"), None);
    }

    /// A pattern with a predicate that accepts exactly what its regular expression matches.
    struct Fixture {
        source: &'static str,
        order: usize,
        test: fn(&str) -> bool,
    }

    impl IndexedPattern for Fixture {
        fn source(&self) -> &str {
            self.source
        }

        fn path(&self) -> bool {
            self.source.starts_with('/')
        }

        fn order(&self) -> usize {
            self.order
        }

        fn is_match(&self, subject: &str) -> bool {
            (self.test)(subject)
        }
    }

    fn fixture(source: &'static str, test: fn(&str) -> bool) -> Fixture {
        Fixture { source, order: 0, test }
    }

    fn fixtures() -> Vec<Fixture> {
        let mut all = vec![
            fixture("^README", |s: &str| s.starts_with("README")),
            fixture("\\.ya?ml$", |s: &str| s.ends_with(".yml") || s.ends_with(".yaml")),
            fixture("(main|ci)\\.ya?ml$", |s: &str| {
                ["main.yml", "main.yaml", "ci.yml", "ci.yaml"].iter().any(|end| s.ends_with(*end))
            }),
            fixture("\\.md$", |s: &str| s.ends_with(".md")),
            fixture("^\\.github", |s: &str| s.starts_with(".github")),
            fixture("/tests?/.*\\.java$", |s: &str| {
                s.strip_suffix(".java").is_some_and(|head| head.contains("/test/") || head.contains("/tests/"))
            }),
            fixture("\\.java$", |s: &str| s.ends_with(".java")),
            fixture("^(a|b)", |s: &str| s.starts_with(['a', 'b'])),
            fixture("x", |s: &str| s.contains('x')),
            fixture("^cargo\\.toml$", |s: &str| s.eq_ignore_ascii_case("cargo.toml")),
            fixture("\\.TXT$", |s: &str| s.to_ascii_lowercase().ends_with(".txt")),
        ];
        for (order, pattern) in all.iter_mut().enumerate() {
            pattern.order = order;
        }
        all
    }

    fn linear(patterns: &[Fixture], name: &str, path: Option<&str>) -> Option<usize> {
        patterns.iter().find(|p| subject(*p, name, path).is_some_and(|s| p.is_match(s))).map(|p| p.order)
    }

    // src/ui/patternIndex.test.ts › pattern index › finds the same first match as a linear scan
    #[test]
    fn finds_the_same_first_match_as_a_linear_scan() {
        let index = PatternIndex::new(fixtures());
        let patterns = fixtures();
        let names = [
            "README.md",
            "ci.yml",
            "a.yaml",
            "x.md",
            ".github",
            "Foo.java",
            "b.txt",
            "box",
            "nothing.rs",
            "md",
            "Cargo.toml",
            "NOTES.txt",
        ];
        for name in names {
            for path in [None, Some(format!("/p/tests/{name}")), Some(format!("/p/src/{name}"))] {
                let path = path.as_deref();
                let found = index.find(name, path).map(|p| p.order);
                assert_eq!(found, linear(&patterns, name, path), "{name} {path:?}");
            }
        }
    }
}
