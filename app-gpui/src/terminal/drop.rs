//! Files dropped from the OS onto a terminal: the path is pasted at the prompt (shell-quoted when
//! needed), separated by spaces, instead of the window-wide drop (open in the editor, "Open
//! project in" for a folder). Images are treated the same way: showing the path is the expected
//! behaviour.

use crate::platform::command_line::{format_args, quote_path};
use crate::platform::misc::platform_info::Os;

/// Characters `cmd.exe` and PowerShell read as syntax in an unquoted word.
const WINDOWS_METACHARACTERS: [char; 7] = ['&', '|', '<', '>', '(', ')', '^'];

/// Shell-quoted paths for the prompt; the user presses Enter to execute. `os`: the system the
/// app runs on.
pub fn dropped_paths_text<S: AsRef<str>>(paths: &[S], os: Os) -> String {
    if os != Os::Windows {
        return format_args(paths);
    }
    let quoted = |path: &str| {
        if path.contains(WINDOWS_METACHARACTERS) { format!("\"{path}\"") } else { quote_path(path) }
    };
    paths.iter().map(|path| quoted(path.as_ref())).collect::<Vec<_>>().join(" ")
}

/// What a drop of `paths` pastes into a terminal, or `None` when the drop is not the terminal's:
/// nothing was dropped, the pointer is not `over_terminal`, or the window is remote (local paths
/// mean nothing to its shell).
pub fn terminal_drop_text<S: AsRef<str>>(
    paths: &[S],
    os: Os,
    remote_window: bool,
    over_terminal: bool,
) -> Option<String> {
    if paths.is_empty() || remote_window || !over_terminal {
        return None;
    }
    Some(dropped_paths_text(paths, os))
}

#[cfg(test)]
mod tests {
    use super::*;

    // src/features/terminal/drop.test.ts › terminal drop › quotes POSIX paths and joins files with spaces on %s
    #[test]
    fn quotes_posix_paths_and_joins_files_with_spaces() {
        for os in [Os::Macos, Os::Linux] {
            assert_eq!(dropped_paths_text(&["/plain/file.ts"], os), "/plain/file.ts");
            let several = ["/a b/c.png", "/plain", "C:/dir/e f"];
            assert_eq!(dropped_paths_text(&several, os), "'/a b/c.png' /plain 'C:/dir/e f'");
            assert_eq!(dropped_paths_text(&["/it's here"], os), r"'/it'\''s here'");
        }
    }

    // src/features/terminal/drop.test.ts › terminal drop › uses double quotes for Windows paths with spaces or shell
    // metacharacters
    #[test]
    fn uses_double_quotes_for_windows_paths_with_spaces_or_shell_metacharacters() {
        assert_eq!(
            dropped_paths_text(&[r"C:\plain\file.ts", r"C:\a b\c.png", r"C:\it's here\"], Os::Windows),
            r#"C:\plain\file.ts "C:\a b\c.png" "C:\it's here\""#,
        );
        assert_eq!(dropped_paths_text(&[r"C:\a&b\c.png"], Os::Windows), r#""C:\a&b\c.png""#);
    }

    // src/features/terminal/drop.test.ts › terminal drop › pastes only into the host under the cursor without
    // executing, regardless of registration order
    #[test]
    fn pastes_only_into_the_terminal_under_the_cursor_without_executing() {
        assert_eq!(terminal_drop_text(&["/a b", "/plain"], Os::Macos, false, true).as_deref(), Some("'/a b' /plain"));
        assert_eq!(terminal_drop_text(&["/a b", "/plain"], Os::Macos, false, false), None);
        assert_eq!(terminal_drop_text(&["/first"], Os::Macos, false, true).as_deref(), Some("/first"));
        assert_eq!(terminal_drop_text::<&str>(&[], Os::Macos, false, true), None);
        assert_eq!(terminal_drop_text(&["/img/pic.png"], Os::Macos, false, false), None);
    }

    // src/features/terminal/drop.test.ts › terminal drop › leaves local path drops unhandled in a remote window
    #[test]
    fn leaves_local_path_drops_unhandled_in_a_remote_window() {
        assert_eq!(terminal_drop_text(&["/local/file"], Os::Macos, true, true), None);
    }
}
