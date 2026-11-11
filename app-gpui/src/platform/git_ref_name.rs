//! Branch names cleaned up while they are typed, following IntelliJ's
//! `GitRefNameValidator.cleanUpBranchNameOnTyping` (git4idea).

/// IntelliJ's advanced setting `git.branch.cleanup.symbol` (default `-`).
pub const BRANCH_CLEANUP_SYMBOL: char = '-';

/// A character git forbids anywhere in a ref name.
fn is_forbidden(c: char) -> bool {
    matches!(c, '~' | ':' | '^' | '?' | '*' | '"' | '[' | '\\')
}

/// A space or a control character (tabs included): what becomes the cleanup symbol.
fn is_gap(c: char) -> bool {
    c == ' ' || c.is_control()
}

fn is_separator(c: char) -> bool {
    c == BRANCH_CLEANUP_SYMBOL || is_gap(c)
}

/// Drops what git forbids and substitution cannot repair: a leading `.`, `-` or `/`, the
/// forbidden characters, `@{`, and the first character of `//`, `..` and `./`.
fn drop_forbidden(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut chars = name.chars().peekable();
    let mut at_start = true;
    while let Some(c) = chars.next() {
        let next = chars.peek().copied();
        let dropped = match c {
            '.' | '-' | '/' if at_start => true,
            '@' if next == Some('{') => {
                chars.next();
                true
            }
            '/' => next == Some('/'),
            '.' => matches!(next, Some('.' | '/')),
            _ => is_forbidden(c),
        };
        at_start = false;
        if !dropped {
            out.push(c);
        }
    }
    out
}

/// Turns every run of spaces and control characters into one cleanup symbol. The run also
/// absorbs the symbols mixed into or next to it, so pressing Space again after the substituted
/// one doesn't stack them (IntelliJ collapses only spaces typed in one go).
fn collapse_gaps(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(start) = rest.find(is_separator) {
        let end = rest[start..].find(|c: char| !is_separator(c)).map_or(rest.len(), |len| start + len);
        let run = &rest[start..end];
        out.push_str(&rest[..start]);
        if run.contains(is_gap) {
            out.push(BRANCH_CLEANUP_SYMBOL);
        } else {
            out.push_str(run);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn clean_once(name: &str) -> String {
    collapse_gaps(&drop_forbidden(name).replace("/.", "/"))
}

/// Makes `name` typable as a branch name: forbidden characters are dropped and spaces become the
/// cleanup symbol, so a space never reaches the validator. Runs to a fixed point because the
/// substitution can itself form a forbidden start (" x" → "-x" → "x").
pub fn clean_up_branch_name_on_typing(name: &str) -> String {
    let mut cleaned = name.to_owned();
    loop {
        let next = clean_once(&cleaned);
        if next == cleaned {
            return cleaned;
        }
        cleaned = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! cleans {
        ($name:ident, $input:expr, $expected:expr) => {
            #[test]
            fn $name() {
                assert_eq!(clean_up_branch_name_on_typing($input), $expected);
            }
        };
    }

    // The reported case: a space typed or pasted between words.
    // src/platform/gitRefName.test.ts › "feature x" → "feature-x"
    cleans!(space_between_two_words, "feature x", "feature-x");
    // src/platform/gitRefName.test.ts › "fix login redirect" → "fix-login-redirect"
    cleans!(spaces_between_three_words, "fix login redirect", "fix-login-redirect");

    // Repeated spaces collapse to one separator.
    // src/platform/gitRefName.test.ts › "a   b" → "a-b"
    cleans!(repeated_spaces, "a   b", "a-b");
    // src/platform/gitRefName.test.ts › "feature " → "feature-"
    cleans!(trailing_space, "feature ", "feature-");

    // Space pressed again after the substituted separator, and spaces around a typed one.
    // src/platform/gitRefName.test.ts › "feature- " → "feature-"
    cleans!(space_after_separator, "feature- ", "feature-");
    // src/platform/gitRefName.test.ts › "feature - x" → "feature-x"
    cleans!(spaces_around_separator, "feature - x", "feature-x");
    // src/platform/gitRefName.test.ts › "a-b" → "a-b"
    cleans!(single_separator, "a-b", "a-b");
    // src/platform/gitRefName.test.ts › "a--b" → "a--b"
    cleans!(double_separator, "a--b", "a--b");

    // A leading space would form a leading `-`, which git rejects: dropped.
    // src/platform/gitRefName.test.ts › " feature" → "feature"
    cleans!(leading_space, " feature", "feature");
    // src/platform/gitRefName.test.ts › "   " → ""
    cleans!(only_spaces, "   ", "");

    // Tabs and other control characters count as whitespace too.
    // src/platform/gitRefName.test.ts › "a\tb" → "a-b"
    cleans!(tab, "a\tb", "a-b");
    // src/platform/gitRefName.test.ts › "a\u007fb" → "a-b"
    cleans!(delete_character, "a\u{7f}b", "a-b");

    // Characters git forbids are dropped rather than substituted.
    // src/platform/gitRefName.test.ts › "a~b^c:d?e*f[g\\h" → "abcdefgh"
    cleans!(forbidden_characters, "a~b^c:d?e*f[g\\h", "abcdefgh");
    // src/platform/gitRefName.test.ts › "say \"hi\"" → "say-hi"
    cleans!(double_quotes, "say \"hi\"", "say-hi");
    // src/platform/gitRefName.test.ts › "a@{b" → "ab"
    cleans!(at_brace, "a@{b", "ab");
    // src/platform/gitRefName.test.ts › "a..b" → "a.b"
    cleans!(double_dot, "a..b", "a.b");
    // src/platform/gitRefName.test.ts › "a//b" → "a/b"
    cleans!(double_slash, "a//b", "a/b");
    // src/platform/gitRefName.test.ts › "/a" → "a"
    cleans!(leading_slash, "/a", "a");
    // src/platform/gitRefName.test.ts › ".a" → "a"
    cleans!(leading_dot, ".a", "a");
    // src/platform/gitRefName.test.ts › "-a" → "a"
    cleans!(leading_separator, "-a", "a");
    // src/platform/gitRefName.test.ts › "a/.b" → "a/b"
    cleans!(dot_after_slash, "a/.b", "a/b");
    // src/platform/gitRefName.test.ts › "a./b" → "a/b"
    cleans!(dot_before_slash, "a./b", "a/b");

    // Already valid names are untouched, including Unicode.
    // src/platform/gitRefName.test.ts › "feature/login-v2" → "feature/login-v2"
    cleans!(valid_nested_name, "feature/login-v2", "feature/login-v2");
    // src/platform/gitRefName.test.ts › "release/1.2.3" → "release/1.2.3"
    cleans!(valid_version_name, "release/1.2.3", "release/1.2.3");
    // src/platform/gitRefName.test.ts › "fix_bug" → "fix_bug"
    cleans!(valid_underscore_name, "fix_bug", "fix_bug");
    // src/platform/gitRefName.test.ts › "功能/登录" → "功能/登录"
    cleans!(valid_unicode_name, "功能/登录", "功能/登录");
    // src/platform/gitRefName.test.ts › "" → ""
    cleans!(empty_name, "", "");

    // src/platform/gitRefName.test.ts › is idempotent, so a cleaned field is stable under re-typing
    #[test]
    fn is_idempotent_so_a_cleaned_field_is_stable_under_retyping() {
        for name in ["a b", " a", "a  ..  b", "@{ x", "x/ /y", "- - a"] {
            let once = clean_up_branch_name_on_typing(name);
            assert_eq!(clean_up_branch_name_on_typing(&once), once, "{name:?}");
        }
    }
}
