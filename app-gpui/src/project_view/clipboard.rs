//! The OS clipboard's file list: what the file manager copies, and what the project view puts
//! there so the file manager can paste it back. The format is native (Windows `CF_HDROP`), so
//! the backend owns it; a call that cannot be made, or that fails, degrades to "no files".

/// A command of the backend's clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OsClipboardCall {
    /// The files the file manager copied; none when the clipboard holds no files.
    FileList,
    /// Whether the clipboard holds files: cheap, for the Paste item's enabled state.
    HasFiles,
    /// Puts the files on the clipboard; answers whether the OS took them.
    SetFileList(Vec<String>),
}

/// What a Copy / Cut in the tree hands to the OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OsClipboardWrite {
    /// The copied files, as the file list the file manager pastes.
    Files(Vec<String>),
    /// The copied paths as text, one per line.
    Text(String),
}

impl OsClipboardWrite {
    /// The hand-off of `paths`. Paths on a remote host are useful as text, but do not name files
    /// on the client.
    pub fn of(paths: &[String], remote: bool) -> Self {
        if remote { Self::Text(paths.join("\n")) } else { Self::Files(paths.to_vec()) }
    }

    /// The backend command of the hand-off: none for text, which goes to the UI's own clipboard,
    /// and none without files, which counts as not handed off.
    pub fn call(&self) -> Option<OsClipboardCall> {
        match self {
            Self::Files(paths) if !paths.is_empty() => Some(OsClipboardCall::SetFileList(paths.clone())),
            Self::Files(_) | Self::Text(_) => None,
        }
    }
}

/// Whether the OS took the file list. The internal clipboard still works without it, so a failed
/// hand-off is not an error.
pub fn files_handed_off<E>(answer: Result<bool, E>) -> bool {
    answer.unwrap_or(false)
}

/// Whether the copied text reached the clipboard.
pub fn text_handed_off<E>(answer: Result<(), E>) -> bool {
    answer.is_ok()
}

/// Whether the clipboard holds files, when asked for the Paste item: a failure is "none".
pub fn has_files<E>(answer: Result<bool, E>) -> bool {
    answer.unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: files are handed off as a file list, remote paths as text, and a failed
    // hand-off counts as not handed off.
    #[test]
    fn hands_files_to_the_os_and_remote_paths_as_text() {
        let paths = vec!["/p/a.txt".to_owned(), "/p/b.txt".to_owned()];
        let files = OsClipboardWrite::of(&paths, false);
        assert_eq!(files.call(), Some(OsClipboardCall::SetFileList(paths.clone())));
        let text = OsClipboardWrite::of(&paths, true);
        assert_eq!(text, OsClipboardWrite::Text("/p/a.txt\n/p/b.txt".to_owned()));
        assert_eq!(text.call(), None);
        assert_eq!(OsClipboardWrite::Files(Vec::new()).call(), None);
        assert!(files_handed_off::<String>(Ok(true)));
        assert!(!files_handed_off(Err("unavailable")));
        assert!(text_handed_off::<String>(Ok(())) && !text_handed_off(Err("unavailable")));
        assert!(has_files::<String>(Ok(true)) && !has_files(Err("busy")));
    }
}
