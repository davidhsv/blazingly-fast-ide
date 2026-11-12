//! Editor settings that decide per file: soft-wrap file patterns, and the indentation a document
//! gets (`.editorconfig`, then the file's detected indents, then Code Style).

/// Whether `pattern` (`*` any run, `?` one character) matches all of `name`, ignoring case.
fn wildcard(pattern: &[char], name: &[char]) -> bool {
    let (mut p, mut n, mut star, mut mark) = (0, 0, None, 0);
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            mark = n;
            p += 1;
        } else if let Some(at) = star {
            p = at + 1;
            mark += 1;
            n = mark;
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

fn folded(text: &str) -> Vec<char> {
    text.chars().flat_map(char::to_lowercase).collect()
}

/// Soft-wrap files (`editor.general.softWrapFiles`): `*.md; *.txt` patterns separated by `;` or
/// `,`, matched against the file name.
pub fn matches_file_patterns(patterns: &str, path: &str) -> bool {
    let name = folded(path.rsplit_once('/').map_or(path, |(_, name)| name));
    patterns
        .split([';', ','])
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty())
        .any(|pattern| wildcard(&folded(pattern), &name))
}

/// A file's own indentation: tabs, or spaces of `size` columns (unknown when no step is clear).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectedIndent {
    pub insert_spaces: bool,
    pub size: Option<u32>,
}

/// How many lines detection reads.
const DETECT_LINES: usize = 4000;

/// The indentation the first lines of `text` use; `None` with fewer than two non-blank lines or
/// no indented one. The step is the most frequent increase of 2–8 spaces, the smaller on a tie.
pub fn detect_indent(text: &str) -> Option<DetectedIndent> {
    let (mut tabs, mut spaces, mut lines) = (0usize, 0usize, 0usize);
    let mut deltas: Vec<(u32, usize)> = Vec::new();
    let mut previous = 0usize;
    for line in text.split('\n').take(DETECT_LINES) {
        if line.trim().is_empty() {
            continue;
        }
        lines += 1;
        let leading = &line[..line.len() - line.trim_start_matches([' ', '\t']).len()];
        if leading.starts_with('\t') {
            tabs += 1;
        } else if leading.len() >= 2 {
            spaces += 1;
        }
        if !leading.contains('\t') {
            if let Some(delta) = leading.len().checked_sub(previous).filter(|delta| (2..=8).contains(delta)) {
                let delta = delta as u32;
                match deltas.iter_mut().find(|(step, _)| *step == delta) {
                    Some((_, count)) => *count += 1,
                    None => deltas.push((delta, 1)),
                }
            }
            previous = leading.len();
        }
    }
    if lines < 2 || (tabs == 0 && spaces == 0) {
        return None;
    }
    if tabs > spaces {
        return Some(DetectedIndent { insert_spaces: false, size: None });
    }
    let size = deltas.iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0))).map(|(step, _)| *step);
    Some(DetectedIndent { insert_spaces: true, size })
}

/// The Code Style override of a language (`editor.codeStyle.languages.<id>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LanguageStyle {
    pub tab_size: Option<u32>,
    pub use_tab: Option<bool>,
    pub indent: Option<u32>,
}

/// The `.editorconfig` properties of a file that concern indentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EditorConfigIndent {
    pub indent_size: Option<u32>,
    pub tab_width: Option<u32>,
    /// `indent_style = space` (`Some(true)`) or `tab`.
    pub indent_with_spaces: Option<bool>,
}

/// Where a document's indentation comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IndentSources {
    /// `(tab size, insert spaces)` of the language definition, else of the editor settings.
    pub fallback: (u32, bool),
    pub style: Option<LanguageStyle>,
    /// `editor.codeStyle.detectIndents`.
    pub detect_indents: bool,
    /// `editor.codeStyle.editorconfig`.
    pub editorconfig: bool,
}

/// A document's indentation; `indent_size` when an indent level differs from a tab stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveIndent {
    pub tab_size: u32,
    pub insert_spaces: bool,
    pub indent_size: Option<u32>,
}

/// `.editorconfig` first (when its support is on), then the detected indents (when detection is
/// on), then Code Style: the language's override over its definition and the defaults.
pub fn effective_indent(
    sources: &IndentSources,
    editorconfig: Option<&EditorConfigIndent>,
    detected: Option<&DetectedIndent>,
) -> EffectiveIndent {
    let style = sources.style.unwrap_or_default();
    let tab_size = style.tab_size.unwrap_or(sources.fallback.0);
    let mut base = EffectiveIndent {
        tab_size,
        insert_spaces: style.use_tab.map_or(sources.fallback.1, |use_tab| !use_tab),
        indent_size: style.indent.filter(|indent| *indent != tab_size),
    };
    if let Some(detected) = detected.filter(|_| sources.detect_indents) {
        base = if detected.insert_spaces {
            let size = detected.size.or(base.indent_size).unwrap_or(base.tab_size);
            EffectiveIndent { tab_size: size, insert_spaces: true, indent_size: None }
        } else {
            EffectiveIndent { tab_size: base.tab_size, insert_spaces: false, indent_size: None }
        };
    }
    match editorconfig.filter(|_| sources.editorconfig) {
        None => base,
        Some(config) => EffectiveIndent {
            tab_size: config.indent_size.or(config.tab_width).unwrap_or(base.tab_size),
            insert_spaces: config.indent_with_spaces.unwrap_or(base.insert_spaces),
            indent_size: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_lsp::completion::matches_case;
    use crate::editor_model::document::{CleanupOptions, ModifiedRanges, save_cleanup_changes};
    use ropey::Rope;

    #[test]
    fn matches_soft_wrap_file_patterns() {
        assert!(matches_file_patterns("*.md; *.txt", "/p/README.md"));
        assert!(!matches_file_patterns("*.md; *.txt", "/p/main.rs"));
        assert!(matches_file_patterns("*", "/p/main.rs"));
        assert!(matches_file_patterns("readme.?D", "/p/README.md"));
    }

    #[test]
    fn detects_file_indents_and_layers_code_style_overrides() {
        assert_eq!(detect_indent("a\n  b\n    c\n  d\n"), Some(DetectedIndent { insert_spaces: true, size: Some(2) }));
        assert_eq!(detect_indent("a\n\tb\n\t\tc\n"), Some(DetectedIndent { insert_spaces: false, size: None }));
        assert_eq!(detect_indent("plain\ntext\n"), None);
        let python = IndentSources {
            fallback: (4, true),
            style: Some(LanguageStyle { tab_size: Some(8), use_tab: Some(true), indent: None }),
            detect_indents: true,
            editorconfig: false,
        };
        let tabs = EffectiveIndent { tab_size: 8, insert_spaces: false, indent_size: None };
        assert_eq!(effective_indent(&python, None, None), tabs);
        let config = EditorConfigIndent { indent_size: Some(2), tab_width: None, indent_with_spaces: Some(true) };
        assert_eq!(effective_indent(&python, Some(&config), None), tabs);
        let detected = DetectedIndent { insert_spaces: true, size: Some(2) };
        let spaces = EffectiveIndent { tab_size: 2, insert_spaces: true, indent_size: None };
        assert_eq!(effective_indent(&python, None, Some(&detected)), spaces);
    }

    #[test]
    fn removes_trailing_blank_lines_and_strips_all_lines_on_request() {
        let mut rope = Rope::from_str("a  \nb\n\n\n");
        let opts = CleanupOptions {
            trim: true,
            final_newline: true,
            all_lines: true,
            trailing_blank_lines: true,
            ..CleanupOptions::default()
        };
        let changes = save_cleanup_changes(&rope, &ModifiedRanges::default(), &opts);
        for change in changes.iter().rev() {
            let (from, to) = (rope.utf16_cu_to_char(change.from as usize), rope.utf16_cu_to_char(change.to as usize));
            rope.remove(from..to);
            rope.insert(from, &change.insert);
        }
        assert_eq!(rope.to_string(), "a\nb\n");
    }

    #[test]
    fn matches_completion_case() {
        assert!(!matches_case("String", "s", false));
        assert!(matches_case("string", "s", false));
        assert!(matches_case("toString", "tS", true));
        assert!(!matches_case("tostring", "tS", true));
    }
}
