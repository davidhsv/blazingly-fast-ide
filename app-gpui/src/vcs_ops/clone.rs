//! Get from VCS: the form of the Clone dialog (URL, directory, depth).

use regex::Regex;
use std::sync::LazyLock;

fn is_separator(c: char) -> bool {
    c == '/' || c == '\\'
}

/// Folder name git would pick for `url` (`https://h/o/repo.git` → `repo`, `git@h:o/r` → `r`).
pub fn clone_dir_name(url: &str) -> String {
    /// Runs of characters a folder name does not keep.
    static UNSAFE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^A-Za-z0-9_.\-]+").expect("valid pattern"));
    let url = url.trim();
    // A query or fragment runs to the end of the line.
    let cut = url.char_indices().find(|&(at, c)| matches!(c, '?' | '#') && !url[at..].contains(['\n', '\r']));
    let url = cut.map_or(url, |(at, _)| &url[..at]).trim_end_matches(is_separator);
    let stem = url.len().checked_sub(4).filter(|&at| url.get(at..).is_some_and(|end| end.eq_ignore_ascii_case(".git")));
    let url = stem.map_or(url, |at| &url[..at]).trim_end_matches(is_separator);
    let name = url.split(|c: char| is_separator(c) || c == ':').rfind(|part| !part.is_empty()).unwrap_or_default();
    UNSAFE.replace_all(name, "-").trim_start_matches(['.', '-']).to_owned()
}

/// Joins a parent directory and a child name with one separator.
pub fn join_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else if name.is_empty() {
        parent.to_owned()
    } else {
        format!("{}/{name}", parent.trim_end_matches(is_separator))
    }
}

/// The folder holding `path`.
pub fn parent_of(path: &str) -> &str {
    let path = path.trim_end_matches(is_separator);
    path.rfind(is_separator).map_or(path, |at| &path[..at])
}

fn is_absolute(dir: &str) -> bool {
    dir.starts_with('/') || matches!(dir.as_bytes(), [drive, b':', b'/' | b'\\', ..] if drive.is_ascii_alphabetic())
}

/// Validates the clone form; `None` when it can be submitted.
pub fn validate_clone(url: &str, dir: &str) -> Option<&'static str> {
    let url = url.trim();
    let dir = dir.trim();
    if url.is_empty() {
        Some("Enter a repository URL")
    } else if url.contains(|c: char| c.is_whitespace() || c == '\u{feff}') {
        Some("The URL must not contain spaces")
    } else if url.starts_with('-') {
        Some("Invalid repository URL")
    } else if dir.is_empty() {
        Some("Choose a directory")
    } else if !is_absolute(dir) {
        Some("The directory must be an absolute path")
    } else {
        None
    }
}

/// A valid `git clone --depth` value, or `None` while the field is invalid.
pub fn parse_clone_depth(value: &str) -> Option<u32> {
    let text = value.trim();
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u32>().ok().filter(|depth| *depth > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // src/features/vcs/__tests__/clone.test.ts › cloneDirName › derives the folder git would create
    #[test]
    fn derives_the_folder_git_would_create() {
        assert_eq!(clone_dir_name("https://github.com/user/repo.git"), "repo");
        assert_eq!(clone_dir_name("https://github.com/user/repo"), "repo");
        assert_eq!(clone_dir_name("https://github.com/user/repo/"), "repo");
        assert_eq!(clone_dir_name("git@github.com:user/my-lib.git"), "my-lib");
        assert_eq!(clone_dir_name("ssh://git@host:2222/group/sub/proj.git"), "proj");
        assert_eq!(clone_dir_name("/srv/git/local.git/"), "local");
        assert_eq!(clone_dir_name("https://host/a/b.git?ref=x"), "b");
        assert_eq!(clone_dir_name("  "), "");
    }

    // src/features/vcs/__tests__/clone.test.ts › joinPath / validateClone › joins with a single separator
    #[test]
    fn joins_with_a_single_separator() {
        assert_eq!(join_path("/Users/me/dev/", "repo"), "/Users/me/dev/repo");
        assert_eq!(join_path("", "repo"), "repo");
        assert_eq!(join_path("/x", ""), "/x");
    }

    // src/features/vcs/__tests__/clone.test.ts › joinPath / validateClone › requires a URL and an absolute directory
    #[test]
    fn requires_a_url_and_an_absolute_directory() {
        assert!(validate_clone("", "/a").unwrap().contains("URL"));
        assert!(validate_clone("--upload-pack=x", "/a").unwrap().contains("Invalid"));
        assert!(validate_clone("https://h/r a", "/a").unwrap().contains("spaces"));
        assert!(validate_clone("https://h/r", "").unwrap().to_lowercase().contains("directory"));
        assert!(validate_clone("https://h/r", "rel/dir").unwrap().contains("absolute"));
        assert_eq!(validate_clone("https://h/r", "/abs/r"), None);
    }

    // src/features/vcs/__tests__/clone.test.ts › parseCloneDepth › accepts positive Git depths only
    #[test]
    fn accepts_positive_git_depths_only() {
        assert_eq!(parse_clone_depth("1"), Some(1));
        assert_eq!(parse_clone_depth(" 25 "), Some(25));
        assert_eq!(parse_clone_depth("0"), None);
        assert_eq!(parse_clone_depth("1.5"), None);
        assert_eq!(parse_clone_depth("abc"), None);
        assert_eq!(parse_clone_depth("4294967296"), None);
    }

    // Not from a spec: the default clone directory is the parent of the last one used.
    #[test]
    fn finds_the_parent_folder() {
        assert_eq!(parent_of("/Users/me/dev/repo/"), "/Users/me/dev");
        assert_eq!(parent_of("C:\\dev\\repo"), "C:\\dev");
        assert_eq!(parent_of("repo"), "repo");
    }

    // Not from a spec: a Windows drive path is absolute, and a dotted name keeps its inner dots.
    #[test]
    fn accepts_drive_paths_and_cleans_odd_names() {
        assert_eq!(validate_clone("https://h/r", "C:\\dev\\r"), None);
        assert_eq!(clone_dir_name("https://h/o/.my repo!.GIT"), "my-repo-");
    }
}
