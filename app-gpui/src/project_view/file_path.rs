//! Navigate › File Path: the file and the folders above it, each to be shown in the OS file
//! manager.

use crate::project_tree_paths::{name_of, parent_of};

/// The title of the popup.
pub const FILE_PATH_TITLE: &str = "File Path";

/// Whether `path` is a drive: a letter and a colon, with or without the `/` after it.
fn is_drive(path: &str, with_slash: bool) -> bool {
    match path.as_bytes() {
        [letter, b':'] => letter.is_ascii_alphabetic(),
        [letter, b':', b'/'] => with_slash && letter.is_ascii_alphabetic(),
        _ => false,
    }
}

/// `path`, then each folder above it, up to the root of the file system or of the drive.
pub fn file_path_chain(path: &str) -> Vec<String> {
    let mut out = vec![path.to_owned()];
    let mut at = path.to_owned();
    while at != "/" && !is_drive(&at, true) {
        let parent = parent_of(&at);
        if parent == at {
            break;
        }
        // The folder above `C:/Users` is the drive's root, `C:/`.
        at = if is_drive(parent, false) { format!("{parent}/") } else { parent.to_owned() };
        out.push(at.clone());
    }
    out
}

/// A row of the popup; choosing it shows `path` in the file manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePathRow {
    pub path: String,
    pub label: String,
    /// The full path, for the folders.
    pub detail: Option<String>,
    pub icon: &'static str,
}

/// The rows of the popup for `path`: the file (or folder, with `is_dir`) itself first.
pub fn file_path_rows(path: &str, is_dir: bool) -> Vec<FilePathRow> {
    let chain = file_path_chain(path);
    let row = |(index, at): (usize, String)| {
        let name = Some(name_of(&at)).filter(|name| !name.is_empty());
        FilePathRow {
            label: name.unwrap_or(at.as_str()).to_owned(),
            detail: (index > 0).then(|| at.clone()),
            icon: if index == 0 && !is_dir { "file" } else { "folder" },
            path: at,
        }
    };
    chain.into_iter().enumerate().map(row).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // src/features/project-tree/filePath.test.ts › filePathChain › lists the file then every folder up to the root
    #[test]
    fn lists_the_file_then_every_folder_up_to_the_root() {
        let expected = ["/work/app/src/main.ts", "/work/app/src", "/work/app", "/work", "/"];
        assert_eq!(file_path_chain("/work/app/src/main.ts"), expected);
    }

    // src/features/project-tree/filePath.test.ts › filePathChain › stops at a Windows drive root
    #[test]
    fn stops_at_a_windows_drive_root() {
        assert_eq!(file_path_chain("C:/Users/me/a.txt"), ["C:/Users/me/a.txt", "C:/Users/me", "C:/Users", "C:/"]);
    }

    // Not from a spec: the popup lists the file by name and the folders with their paths.
    #[test]
    fn the_popup_lists_the_file_by_name_and_the_folders_with_their_paths() {
        let rows = file_path_rows("/work/main.ts", false);
        let shown: Vec<(&str, Option<&str>, &str)> =
            rows.iter().map(|row| (row.label.as_str(), row.detail.as_deref(), row.icon)).collect();
        assert_eq!(shown, [("main.ts", None, "file"), ("work", Some("/work"), "folder"), ("/", Some("/"), "folder")]);
        assert_eq!(file_path_rows("/work", true)[0].icon, "folder");
    }
}
