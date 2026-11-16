//! The notebook editor of `.ipynb` files: a tab kind whose view shows the notebook's cells, edits
//! them through the backend's notebook document and runs code cells in the notebook's kernel.
//!
//! - [`model`]: the notebook of one file, shared by its tabs, and the calls it wants made;
//! - [`view`] and [`cells`]: the editor drawn from it;
//! - [`output_items`], [`ansi`], [`markdown_cell`]: what outputs and Markdown cells show, from the
//!   notebook's own data only.

pub mod ansi;
pub mod cells;
pub mod markdown_cell;
pub mod model;
pub mod output_items;
pub mod view;

use crate::editor::tabs::EditorInput;

/// The editor kind of notebook tabs.
pub const NOTEBOOK_KIND: &str = "notebook";

/// Whether `path` names a notebook file (`.ipynb`, any case).
pub fn is_notebook_path(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("ipynb"))
}

/// The tab input of the notebook at the canonical path `path`.
pub fn notebook_input(path: &str) -> EditorInput {
    EditorInput {
        kind: NOTEBOOK_KIND.to_owned(),
        key: format!("notebook:{path}"),
        path: Some(path.to_owned()),
        data: None,
    }
}

/// A notebook tab is titled by its file name.
pub fn notebook_title(input: &EditorInput) -> String {
    let path = input.path.as_deref().unwrap_or(&input.key);
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned()
}

pub fn notebook_icon(_: &EditorInput) -> String {
    "file".to_owned()
}

#[cfg(test)]
mod tests {
    mod proofs24_notebook_units;
    mod proofs24_notebook_view;

    use super::*;

    #[test]
    fn notebook_files_open_as_notebook_tabs_titled_by_their_name() {
        assert!(is_notebook_path("/p/Analysis.IPYNB") && is_notebook_path("/p/a.ipynb"));
        assert!(!is_notebook_path("/p/a.ipynb.json") && !is_notebook_path("/p/ipynb"));
        let input = notebook_input("/p/work/a.ipynb");
        assert_eq!((input.kind.as_str(), input.key.as_str()), (NOTEBOOK_KIND, "notebook:/p/work/a.ipynb"));
        assert_eq!(notebook_title(&input), "a.ipynb");
        assert_eq!(notebook_icon(&input), "file");
    }
}
