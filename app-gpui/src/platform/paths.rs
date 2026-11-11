//! Paths in the form the UI holds them.
//!
//! A path coming from a native API (the app data directory, a file dialog) is a native string:
//! `C:\a\b` on Windows. Everything the UI stores and compares (what the backend returns, an
//! editor's path, the project tree) is `/`-separated on every OS, so such a path has to be
//! converted before it is used.

use std::borrow::Cow;

/// Whether `path` starts with a drive: a letter, a colon and a backslash (`C:\`).
fn starts_with_drive(path: &str) -> bool {
    matches!(path.as_bytes(), [letter, b':', b'\\', ..] if letter.is_ascii_alphabetic())
}

/// `p` in the form the UI holds paths: `C:\a\b` and `\\?\C:\a\b` become `C:/a/b`,
/// `\\server\share` becomes `//server/share`. A path that already is in that form, or one that
/// is not a Windows path at all, is returned as is.
pub fn ipc_path(p: &str) -> String {
    let path: Cow<'_, str> = match p.strip_prefix(r"\\?\") {
        Some(verbatim) => match verbatim.strip_prefix(r"UNC\") {
            Some(share) => Cow::Owned(format!(r"\\{share}")),
            None => Cow::Borrowed(verbatim),
        },
        None => Cow::Borrowed(p),
    };
    if starts_with_drive(&path) || path.starts_with(r"\\") { path.replace('\\', "/") } else { path.into_owned() }
}

/// True when `path` equals `ancestor` or lies below it.
pub fn is_same_or_inside(path: &str, ancestor: &str) -> bool {
    let (path, ancestor) = (ipc_path(path), ipc_path(ancestor));
    path == ancestor
        || path.strip_prefix(ancestor.as_str()).is_some_and(|rest| ancestor.ends_with('/') || rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const APPDATA: &str = r"C:\Users\me\AppData\Roaming\ai.l0ng.openidea";

    // src/platform/paths.test.ts › slashes Windows drive paths, plain or verbatim
    #[test]
    fn slashes_windows_drive_paths_plain_or_verbatim() {
        assert_eq!(ipc_path(APPDATA), "C:/Users/me/AppData/Roaming/ai.l0ng.openidea");
        assert_eq!(ipc_path(r"\\?\C:\Users\me"), "C:/Users/me");
    }

    // src/platform/paths.test.ts › slashes network shares
    #[test]
    fn slashes_network_shares() {
        assert_eq!(ipc_path(r"\\server\share\dir"), "//server/share/dir");
        assert_eq!(ipc_path(r"\\?\UNC\server\share\dir"), "//server/share/dir");
    }

    // src/platform/paths.test.ts › leaves UI paths and POSIX paths alone
    #[test]
    fn leaves_ui_paths_and_posix_paths_alone() {
        assert_eq!(ipc_path("C:/Users/me"), "C:/Users/me");
        assert_eq!(ipc_path("/home/me/notes"), "/home/me/notes");
        // A backslash is a legal character in a POSIX file name.
        assert_eq!(ipc_path(r"/home/me/a\b.txt"), r"/home/me/a\b.txt");
    }
}
