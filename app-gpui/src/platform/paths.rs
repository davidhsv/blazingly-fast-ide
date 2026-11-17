//! Paths in the form the UI holds them.
//!
//! A path coming from a native API (the app data directory, a file dialog) is a native string:
//! `C:\a\b` on Windows. Everything the UI stores and compares (what the backend returns, an
//! editor's path, the project tree) is `/`-separated on every OS, so such a path has to be
//! converted before it is used.

use regex::Regex;
use std::borrow::Cow;
use std::sync::LazyLock;

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

/// Whether `path` starts as a network share does: two slashes or backslashes in any mix.
fn starts_as_share(path: &str) -> bool {
    matches!(path.as_bytes(), [b'/' | b'\\', b'/' | b'\\', ..])
}

/// `server/share` of a path on a network share, in lower case; `None` for any other path. The host
/// hands out a share's paths as `//?/UNC/server/share/…`, the dialogs as `//server/share/…`.
fn share_of(path: &str) -> Option<String> {
    static SHARE_NAME: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^[\\/]{2}(?:\?[\\/]UNC[\\/])?([^\\/]+)[\\/]([^\\/]+)").expect("the pattern is valid")
    });
    let found = SHARE_NAME.captures(path)?;
    let (server, share) = (found.get(1)?.as_str(), found.get(2)?.as_str());
    // Any other device path (`//?/C:/…`, `//./…`) names no server to compare.
    (server != "?" && server != ".").then(|| format!("{server}/{share}").to_lowercase())
}

/// Whether `path` is on a network share that `base` is not on. Windows reads `//server/share`,
/// `\\server\share` and mixes of the two as a share, and opening one signs in to its server with
/// the user's credentials: a path that text someone else wrote leads to is followed to no share but
/// the one that text is on. The check reads the text alone, so it is the same on every platform.
pub fn on_other_share(path: &str, base: &str) -> bool {
    if !starts_as_share(path) {
        return false;
    }
    match share_of(path) {
        Some(share) => share_of(base).as_deref() != Some(share.as_str()),
        None => true,
    }
}

/// [`on_other_share`] for `path` resolved against `base`, the folder or file of the text naming it:
/// also true when that text spells a share itself (`written`, decoded).
pub fn opens_other_share(path: &str, written: &str, base: &str) -> bool {
    starts_as_share(written) || on_other_share(path, base)
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

    // src/platform/paths.test.ts › onOtherShare › is true for a share the base is not on
    #[test]
    fn is_true_for_a_share_the_base_is_not_on() {
        for base in ["C:/repo/a.md", "", "//nas/work/a.md", "//evil/other/a.md", "//evil"] {
            assert!(on_other_share("//evil/share/a.png", base), "{base}");
            assert!(on_other_share(r"\\evil\share\a.png", base), "{base}");
        }
        // A server alone, and spellings with no server to compare.
        for path in ["//evil", "//evil/", r"/\\evil/share/a.png"] {
            assert!(on_other_share(path, "//evil/share/a.md"), "{path}");
        }
    }

    // src/platform/paths.test.ts › onOtherShare › reads the server and share of a device path
    #[test]
    fn reads_the_server_and_share_of_a_device_path() {
        let base = "//?/UNC/nas/work/app/a.md";
        assert!(on_other_share("//?/UNC/evil/share/a.png", base));
        assert!(on_other_share("//?/UNC/nas/private/a.png", base));
        assert!(on_other_share("//evil/share/a.png", base));
        assert!(on_other_share("//?/UNC", base));
        assert!(on_other_share("//?/C:/a.png", "//?/C:/a.md"));
        assert!(!on_other_share("//?/unc/NAS/Work/lib/a.png", base));
        assert!(!on_other_share("//nas/work/lib/a.png", base));
        assert!(!on_other_share("//?/UNC/nas/work/lib/a.png", "//nas/work/app/a.md"));
    }

    // src/platform/paths.test.ts › onOtherShare › is false on the base's own share and off any share
    #[test]
    fn is_false_on_the_bases_own_share_and_off_any_share() {
        assert!(!on_other_share("//NAS/Work/lib/a.png", "//nas/work/app/a.md"));
        assert!(!on_other_share(r"\\nas\work\lib\a.png", "//nas/work/app/a.md"));
        assert!(!on_other_share("C:/repo/a.png", "//nas/work/app/a.md"));
        assert!(!on_other_share("/abs/a.png", "C:/repo/a.md"));
    }

    // src/platform/paths.test.ts › opensOtherShare › is true for a share the text names itself, in any mix of
    // slashes
    #[test]
    fn is_true_for_a_share_the_text_names_itself_in_any_mix_of_slashes() {
        for written in ["//evil/share/a.png", r"\\evil\share\a.png", r"/\evil/share/a.png", r"\/evil/a.png"] {
            assert!(opens_other_share(written, written, "C:/repo"), "{written}");
            assert!(opens_other_share(written, written, "//nas/work"), "{written}");
        }
    }

    // src/platform/paths.test.ts › opensOtherShare › is true for a share that only appears once the path is
    // resolved
    #[test]
    fn is_true_for_a_share_that_only_appears_once_the_path_is_resolved() {
        assert!(opens_other_share(r"/\evil/share/a.png", r"\evil/share/a.png", ""));
        assert!(opens_other_share(r"/\evil/share/a.png", r"../\evil/share/a.png", "/guide.md"));
        assert!(opens_other_share("//evil/share/a.png", "a.png", "//nas/work"));
    }

    // src/platform/paths.test.ts › opensOtherShare › is false on a local drive and on the share the text is on
    #[test]
    fn is_false_on_a_local_drive_and_on_the_share_the_text_is_on() {
        assert!(!opens_other_share("C:/repo/docs/a.png", "docs/a.png", "C:/repo"));
        assert!(!opens_other_share("/abs/a.png", "/abs/a.png", "C:/repo"));
        assert!(!opens_other_share("//nas/work/docs/a.png", "docs/a.png", "//nas/work"));
    }
}
