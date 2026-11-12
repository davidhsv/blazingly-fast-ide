//! Code Style › Formatter: "Do not format" globs and formatter on/off markers in comments
//! (`@formatter:off` … `@formatter:on`). Reformat Code and format on save skip excluded files and
//! leave the text between markers untouched.
//!
//! Offsets are UTF-16 code units of the text.

use super::positions::{offset_to_pos, pos_to_offset};
use crate::editor::buffer;
use ide_core::editor_settings::CodeStyleSettings;
use ide_core::{Range, TextChange, TextEdit};
use regex::Regex;
use ropey::Rope;
use std::sync::LazyLock;

/// The Formatter options of the code style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatGuardOptions {
    pub do_not_format: Vec<String>,
    pub formatter_tags_enabled: bool,
    pub formatter_off_tag: String,
    pub formatter_on_tag: String,
    pub formatter_tags_regexp: bool,
}

impl From<&CodeStyleSettings> for FormatGuardOptions {
    fn from(style: &CodeStyleSettings) -> Self {
        Self {
            do_not_format: style.do_not_format.clone(),
            formatter_tags_enabled: style.formatter_tags_enabled,
            formatter_off_tag: style.formatter_off_tag.clone(),
            formatter_on_tag: style.formatter_on_tag.clone(),
            formatter_tags_regexp: style.formatter_tags_regexp,
        }
    }
}

fn glob_regex(glob: &str) -> Option<Regex> {
    let mut re = String::from("^");
    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '*' && chars.get(i + 1) == Some(&'*') {
            re.push_str(".*");
            i += 1;
            if chars.get(i + 1) == Some(&'/') {
                i += 1;
            }
        } else if c == '*' {
            re.push_str("[^/]*");
        } else if c == '?' {
            re.push_str("[^/]");
        } else {
            re.push_str(&regex::escape(&c.to_string()));
        }
        i += 1;
    }
    re.push('$');
    Regex::new(&re).ok()
}

/// Whether "Do not format" excludes `path`. A pattern without `/` matches the file name or any
/// folder name (`*.min.js`, `generated`); one with `/` matches the project-relative path or a
/// folder containing it (`src/gen/**`, `vendor/`).
pub fn format_excluded(path: &str, root: Option<&str>, globs: &[String]) -> bool {
    let rel = root.and_then(|root| path.strip_prefix(root)).and_then(|rest| rest.strip_prefix('/')).unwrap_or(path);
    let segments: Vec<&str> = rel.split('/').filter(|segment| !segment.is_empty()).collect();
    for raw in globs {
        let glob = raw.trim();
        let glob = glob.strip_prefix("./").unwrap_or(glob);
        if glob.is_empty() {
            continue;
        }
        if !glob.contains('/') {
            if glob_regex(glob).is_some_and(|re| segments.iter().any(|segment| re.is_match(segment))) {
                return true;
            }
            continue;
        }
        let Some(re) = glob_regex(glob.trim_start_matches('/').trim_end_matches('/')) else { continue };
        if (1..=segments.len()).rev().any(|i| re.is_match(&segments[..i].join("/"))) {
            return true;
        }
    }
    false
}

/// A comment opener before a marker on its line.
static COMMENT_BEFORE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(//|#|/\*|<!--|--|;|\*|%|\{-|\(\*|')").expect("the pattern is valid"));

fn marker_regex(tag: &str, regexp: bool) -> Option<Regex> {
    if tag.trim().is_empty() {
        return None;
    }
    Regex::new(&if regexp { tag.to_owned() } else { regex::escape(tag) }).ok()
}

/// The lines (byte offsets, the end before the line break) holding a marker inside a comment.
fn marker_lines(text: &str, re: &Regex) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut at = 0;
    while at <= text.len() {
        let Some(found) = re.find_at(text, at) else { break };
        if found.is_empty() {
            at = found.end() + text[found.end()..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        let start = text[..found.start()].rfind('\n').map_or(0, |nl| nl + 1);
        let end = text[found.end()..].find('\n').map_or(text.len(), |nl| found.end() + nl);
        if COMMENT_BEFORE.is_match(&text[start..found.start()]) && out.last().is_none_or(|last| last.0 != start) {
            out.push((start, end));
        }
        at = found.end().max(end);
    }
    out
}

/// Text the formatter must not touch: from the line of each off marker to the end of the line of
/// the next on marker (or the end of the text). UTF-16 offsets.
pub fn protected_regions(text: &str, options: &FormatGuardOptions) -> Vec<std::ops::Range<u32>> {
    if !options.formatter_tags_enabled {
        return Vec::new();
    }
    let Some(off) = marker_regex(&options.formatter_off_tag, options.formatter_tags_regexp) else {
        return Vec::new();
    };
    let offs = marker_lines(text, &off);
    let ons = marker_regex(&options.formatter_on_tag, options.formatter_tags_regexp)
        .map(|on| marker_lines(text, &on))
        .unwrap_or_default();
    let mut out = Vec::new();
    let mut pos = 0;
    for (start, _) in offs {
        if start < pos {
            continue;
        }
        let end = ons.iter().find(|(on, _)| *on > start).map(|(_, end)| *end);
        let to = end.unwrap_or(text.len());
        out.push(buffer::col_of_byte(text, start)..buffer::col_of_byte(text, to));
        pos = to;
        if end.is_none() {
            break;
        }
    }
    out
}

fn touches(change: &TextChange, region: &std::ops::Range<u32>) -> bool {
    if change.from == change.to {
        change.from > region.start && change.from < region.end
    } else {
        change.from < region.end && change.to > region.start
    }
}

fn utf16_slice(text: &str, from: u32, to: u32) -> &str {
    &text[buffer::byte_of_col(text, from)..buffer::byte_of_col(text, to)]
}

/// The change without its common prefix and suffix with the text it replaces; `None` when it
/// changes nothing.
fn narrow(text: &str, from: u32, to: u32, insert: &str) -> Option<TextChange> {
    let old = utf16_slice(text, from, to);
    if old == insert {
        return None;
    }
    let prefix: usize = old.chars().zip(insert.chars()).take_while(|(a, b)| a == b).map(|(c, _)| c.len_utf8()).sum();
    let (old_rest, insert_rest) = (&old[prefix..], &insert[prefix..]);
    let suffix: usize = old_rest
        .chars()
        .rev()
        .zip(insert_rest.chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum();
    let start = from + buffer::col_of_byte(old, prefix);
    let end = from + buffer::col_of_byte(old, old.len() - suffix);
    Some(TextChange { from: start, to: end, insert: insert_rest[..insert_rest.len() - suffix].to_owned() })
}

/// Drops formatting changes inside protected regions. A change replacing whole regions (a
/// formatter rewriting the file) is split around them, provided the formatted text keeps the
/// same markers; other overlapping changes are dropped.
pub fn guard_format_changes(text: &str, changes: &[TextChange], options: &FormatGuardOptions) -> Vec<TextChange> {
    let regions = protected_regions(text, options);
    if regions.is_empty() {
        return changes.to_vec();
    }
    let mut out = Vec::new();
    for raw in changes {
        let Some(change) = narrow(text, raw.from, raw.to, &raw.insert) else { continue };
        let hit: Vec<&std::ops::Range<u32>> = regions.iter().filter(|region| touches(&change, region)).collect();
        if hit.is_empty() {
            out.push(change);
            continue;
        }
        // Whole regions inside the change: map them onto the formatted text.
        if hit.iter().any(|region| region.start < change.from || region.end > change.to) {
            continue;
        }
        let local = protected_regions(&change.insert, options);
        if local.len() != hit.len() {
            continue;
        }
        let mut old_pos = change.from;
        let mut new_pos = 0;
        for i in 0..=hit.len() {
            let old_end = hit.get(i).map_or(change.to, |region| region.start);
            let new_end = local.get(i).map_or(utf16_len(&change.insert), |region| region.start);
            if let Some(piece) = narrow(text, old_pos, old_end, utf16_slice(&change.insert, new_pos, new_end)) {
                out.push(piece);
            }
            if let (Some(old), Some(new)) = (hit.get(i), local.get(i)) {
                old_pos = old.end;
                new_pos = new.end;
            }
        }
    }
    out
}

fn utf16_len(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

/// [`guard_format_changes`] for language-server edits against `text`.
pub fn guard_text_edits(text: &str, edits: &[TextEdit], options: &FormatGuardOptions) -> Vec<TextEdit> {
    if protected_regions(text, options).is_empty() {
        return edits.to_vec();
    }
    let doc = Rope::from_str(text);
    let mut changes: Vec<TextChange> = edits
        .iter()
        .map(|edit| TextChange {
            from: pos_to_offset(&doc, edit.range.start),
            to: pos_to_offset(&doc, edit.range.end),
            insert: edit.new_text.clone(),
        })
        .collect();
    changes.sort_by_key(|change| (change.from, change.to));
    guard_format_changes(text, &changes, options)
        .into_iter()
        .map(|change| TextEdit {
            range: Range::new(offset_to_pos(&doc, change.from), offset_to_pos(&doc, change.to)),
            new_text: change.insert,
        })
        .collect()
}

#[cfg(test)]
#[expect(clippy::single_range_in_vec_init, reason = "a single protected region is the case under test")]
mod tests {
    use super::*;
    use ide_core::Pos;

    fn on() -> FormatGuardOptions {
        FormatGuardOptions {
            do_not_format: Vec::new(),
            formatter_tags_enabled: true,
            formatter_off_tag: "@formatter:off".into(),
            formatter_on_tag: "@formatter:on".into(),
            formatter_tags_regexp: false,
        }
    }

    const LINES: [&str; 5] = ["a  =  1", "// @formatter:off", "b  =  2", "// @formatter:on", "c  =  3"];

    fn text() -> String {
        LINES.join("\n")
    }

    fn at(line: usize, col: u32) -> u32 {
        (LINES[..line].join("\n").len() + usize::from(line > 0)) as u32 + col
    }

    fn apply(text: &str, changes: &[TextChange]) -> String {
        let mut sorted = changes.to_vec();
        sorted.sort_by_key(|change| std::cmp::Reverse(change.from));
        let mut out = text.to_owned();
        for change in sorted {
            out.replace_range(change.from as usize..change.to as usize, &change.insert);
        }
        out
    }

    fn change(from: u32, to: u32, insert: &str) -> TextChange {
        TextChange { from, to, insert: insert.into() }
    }

    const EXPECTED: &str = "a = 1\n// @formatter:off\nb  =  2\n// @formatter:on\nc = 3";
    const FORMATTED: &str = "a = 1\n// @formatter:off\nb = 2\n// @formatter:on\nc = 3";

    #[test]
    fn finds_the_regions_between_markers_in_comments() {
        let text = text();
        assert_eq!(protected_regions(&text, &on()), [at(1, 0)..at(3, 16)]);
        assert!(protected_regions(&text, &FormatGuardOptions { formatter_tags_enabled: false, ..on() }).is_empty());
        assert!(protected_regions("s = \"@formatter:off\"\nx", &on()).is_empty());
        // Without an on marker the region runs to the end.
        assert_eq!(protected_regions("x\n# @formatter:off\ny", &on()), [2..20]);
        let regexp = FormatGuardOptions {
            formatter_off_tag: r"fmt:\s*off".into(),
            formatter_on_tag: r"fmt:\s*on".into(),
            formatter_tags_regexp: true,
            ..on()
        };
        assert_eq!(protected_regions("// fmt: off\nx\n// fmt:on", &regexp), [0..23]);
    }

    #[test]
    fn drops_edits_inside_a_region_and_keeps_the_others() {
        let text = text();
        let changes = [1, 3, 5].map(|line| change(at(line - 1, 1), at(line - 1, 3), " "));
        assert_eq!(
            apply(&text, &guard_format_changes(&text, &changes, &on())),
            "a =  1\n// @formatter:off\nb  =  2\n// @formatter:on\nc =  3"
        );
        let off = FormatGuardOptions { formatter_tags_enabled: false, ..on() };
        assert_eq!(guard_format_changes(&text, &changes, &off), changes);
    }

    #[test]
    fn splits_a_whole_file_replacement_around_the_regions() {
        let text = text();
        let whole = [change(0, text.len() as u32, FORMATTED)];
        assert_eq!(apply(&text, &guard_format_changes(&text, &whole, &on())), EXPECTED);
        let edit = TextEdit { range: Range::new(Pos::new(0, 0), Pos::new(4, 7)), new_text: FORMATTED.into() };
        let edits = guard_text_edits(&text, &[edit], &on());
        let as_changes: Vec<TextChange> = edits
            .iter()
            .map(|edit| {
                change(
                    at(edit.range.start.line as usize, edit.range.start.col),
                    at(edit.range.end.line as usize, edit.range.end.col),
                    &edit.new_text,
                )
            })
            .collect();
        assert_eq!(apply(&text, &as_changes), EXPECTED);
    }

    #[test]
    fn matches_do_not_format_globs_against_names_and_project_paths() {
        let globs: Vec<String> = ["*.min.js", "generated", "src/vendor/**", "docs/"].map(String::from).to_vec();
        assert!(format_excluded("/p/web/app.min.js", Some("/p"), &globs));
        assert!(format_excluded("/p/a/generated/x.ts", Some("/p"), &globs));
        assert!(format_excluded("/p/src/vendor/lib/x.ts", Some("/p"), &globs));
        assert!(format_excluded("/p/docs/a.md", Some("/p"), &globs));
        assert!(!format_excluded("/p/src/app.js", Some("/p"), &globs));
        assert!(!format_excluded("/p/other/docs.md", Some("/p"), &globs));
    }
}
