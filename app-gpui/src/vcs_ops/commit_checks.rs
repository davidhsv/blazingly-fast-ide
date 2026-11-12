//! Before-commit checks: what they report, and file names Windows cannot check out.

use crate::vcs::changes::rel_to;

/// A failed before-commit check, shown in the commit panel while the Commit button turns into
/// "Commit Anyway".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitProblem {
    pub title: String,
    pub text: String,
    /// Label of the "Review" / "Show details" link, when the problem has one.
    pub details: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadNameKind {
    /// A device name Windows reserves (`CON`, `LPT1`…), with or without an extension.
    Reserved,
    /// A character Windows rejects in file names.
    Char,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadFileName {
    pub name: String,
    pub kind: BadNameKind,
}

fn is_reserved(stem: &str) -> bool {
    let stem = stem.to_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let numbered = stem.strip_prefix("COM").or_else(|| stem.strip_prefix("LPT"));
    numbered.is_some_and(|digit| matches!(digit.as_bytes(), [b'1'..=b'9']))
}

fn is_invalid_char(c: char) -> bool {
    matches!(c, '<' | '>' | ':' | '"' | '\\' | '|' | '?' | '*') || c <= '\u{1f}'
}

/// The first path segment below `root` that Windows rejects.
pub fn bad_file_name<S: AsRef<str>>(paths: &[S], root: &str) -> Option<BadFileName> {
    for path in paths {
        for name in rel_to(root, path.as_ref()).split('/') {
            let stem = match name.rfind('.') {
                Some(dot) if dot > 0 => &name[..dot],
                _ => name,
            };
            let kind = if is_reserved(stem) {
                BadNameKind::Reserved
            } else if name.contains(is_invalid_char) {
                BadNameKind::Char
            } else {
                continue;
            };
            return Some(BadFileName { name: name.to_owned(), kind });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/repo";

    fn bad(name: &str, kind: BadNameKind) -> Option<BadFileName> {
        Some(BadFileName { name: name.to_owned(), kind })
    }

    // src/features/vcs/__tests__/commitChecks.test.ts › badFileName › flags names reserved on Windows, with or without an extension
    #[test]
    fn flags_names_reserved_on_windows_with_or_without_an_extension() {
        assert_eq!(bad_file_name(&["/repo/src/con.ts"], ROOT), bad("con.ts", BadNameKind::Reserved));
        assert_eq!(bad_file_name(&["/repo/LPT1/a.txt"], ROOT), bad("LPT1", BadNameKind::Reserved));
    }

    // src/features/vcs/__tests__/commitChecks.test.ts › badFileName › flags characters Windows rejects
    #[test]
    fn flags_characters_windows_rejects() {
        assert_eq!(bad_file_name(&["/repo/docs/what?.md"], ROOT), bad("what?.md", BadNameKind::Char));
        assert_eq!(bad_file_name(&["/repo/a:b"], ROOT), bad("a:b", BadNameKind::Char));
    }

    // src/features/vcs/__tests__/commitChecks.test.ts › badFileName › accepts ordinary names, including ones that merely contain a reserved word
    #[test]
    fn accepts_ordinary_names_including_ones_that_merely_contain_a_reserved_word() {
        assert_eq!(bad_file_name(&["/repo/src/console.ts", "/repo/com10.txt", "/repo/.auxrc"], ROOT), None);
    }
}
