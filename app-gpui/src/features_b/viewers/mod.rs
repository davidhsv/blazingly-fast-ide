//! Non-text editors and the Markdown preview: images open in an image viewer, binary and very
//! large files in a read-only hex view, and Markdown files get IDEA's Editor / Editor and Preview /
//! Preview layouts around the text editor, with a live preview of blocks and styled runs.

pub mod file_kinds;
pub mod font_zoom;
pub mod hex;
pub mod image_zoom;
pub mod mermaid;
pub mod preview;
pub mod render;
