//! Which files open in which viewer: images in the image viewer, binary and very large files in a
//! read-only hex view, Markdown files in the text editor with a preview beside it. The hex view
//! asks the backend for a [`FileProbe`]; probes of one file share the request in flight and are
//! never cached, so a file that changed is probed again.

use crate::editor::tabs::EditorInput;
use ide_vfs::FileProbe;
use std::collections::HashMap;

pub const IMAGE_KIND: &str = "image";
pub const BINARY_KIND: &str = "binary";

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "ico", "bmp", "avif"];
const MARKDOWN_EXTENSIONS: &[&str] = &["md", "markdown", "mdown", "mkd", "mkdn", "mdwn"];

/// The lower-case extension of `path`'s file name; none for a dotfile like `.png`.
fn extension(path: &str) -> String {
    let name = &path[path.rfind('/').map_or(0, |slash| slash + 1)..];
    match name.rfind('.') {
        Some(dot) if dot > 0 => name[dot + 1..].to_lowercase(),
        _ => String::new(),
    }
}

pub fn is_image_path(path: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&extension(path).as_str())
}

pub fn is_svg_path(path: &str) -> bool {
    extension(path) == "svg"
}

pub fn is_markdown_path(path: &str) -> bool {
    MARKDOWN_EXTENSIONS.contains(&extension(path).as_str())
}

pub fn image_input(path: &str) -> EditorInput {
    EditorInput { kind: IMAGE_KIND.to_owned(), key: format!("image:{path}"), path: Some(path.to_owned()), data: None }
}

pub fn binary_input(path: &str, probe: &FileProbe) -> EditorInput {
    EditorInput {
        kind: BINARY_KIND.to_owned(),
        key: format!("binary:{path}"),
        path: Some(path.to_owned()),
        // `binary` is false for huge text files shown in the hex view (they may still open as text).
        data: Some(serde_json::json!({ "size": probe.size, "binary": probe.binary })),
    }
}

/// Binary content, or past the size where the text editor turns read-only.
pub fn wants_hex_view(probe: &FileProbe) -> bool {
    probe.binary || probe.too_large
}

/// The image viewer's input for `path`, unless a text editor wrapper (a plugin's editor for
/// `*.diagram.svg`, say) claims the file: `wrapped` tells whether one does.
pub fn image_viewer_for(path: &str, wrapped: impl Fn(&str) -> bool) -> Option<EditorInput> {
    (is_image_path(path) && !wrapped(path)).then(|| image_input(path))
}

/// `bytes` for people: `512 B`, `1.5 KB`, `300 MB`.
pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 { format!("{value:.0} {}", UNITS[unit]) } else { format!("{value:.1} {}", UNITS[unit]) }
}

/// The ids of the image viewer's actions (category Image, while an image viewer is active).
pub const IMAGE_ACTIONS: [&str; 5] =
    ["image.zoomIn", "image.zoomOut", "image.actualSize", "image.fitToWindow", "image.toggleChessboard"];

/// Who asked to open a file, so each gets its answer.
pub type Waiter = u64;

/// The hex view's probes in flight, by path.
#[derive(Debug, Default)]
pub struct FileProbes {
    pending: HashMap<String, Vec<Waiter>>,
}

impl FileProbes {
    /// `waiter` asks which viewer opens `path`: `true` when the caller must probe the file now
    /// (no probe of it is in flight), `false` when it joins the probe in flight.
    pub fn probe(&mut self, path: &str, waiter: Waiter) -> bool {
        let waiters = self.pending.entry(path.to_owned()).or_default();
        waiters.push(waiter);
        waiters.len() == 1
    }

    /// The probe of `path` answered: the hex view's input for each waiter, or `None` (the file
    /// opens as text, or the probe failed and the next open probes again).
    pub fn answered(&mut self, path: &str, answer: Result<&FileProbe, &str>) -> Vec<(Waiter, Option<EditorInput>)> {
        let waiters = self.pending.remove(path).unwrap_or_default();
        if let Err(error) = answer {
            tracing::warn!("probing {path} failed: {error}");
        }
        let input = answer.ok().filter(|probe| wants_hex_view(probe)).map(|probe| binary_input(path, probe));
        waiters.into_iter().map(|waiter| (waiter, input.clone())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(size: u64, binary: bool, too_large: bool) -> FileProbe {
        FileProbe { size, binary, too_large }
    }

    fn kind(input: &Option<EditorInput>) -> &str {
        input.as_ref().map_or("file", |input| input.kind.as_str())
    }

    #[test]
    fn routes_by_extension() {
        assert!(is_image_path("/p/a.PNG"));
        assert!(is_image_path("/p/icon.svg"));
        assert!(!is_image_path("/p/png"));
        assert!(!is_image_path("/p/.png"));
        assert!(is_svg_path("/p/x.svg"));
        assert!(is_markdown_path("/p/README.md"));
        assert!(!is_markdown_path("/p/notes.txt"));
    }

    #[test]
    fn builds_viewer_inputs_keyed_apart_from_text_tabs() {
        let image = image_input("/a/b.png");
        assert_eq!(
            (image.kind.as_str(), image.key.as_str(), image.path.as_deref()),
            ("image", "image:/a/b.png", Some("/a/b.png"))
        );
        let input = binary_input("/a/b.bin", &probe(10, true, false));
        assert_eq!(input.key, "binary:/a/b.bin");
        assert_eq!(input.data, Some(serde_json::json!({ "size": 10, "binary": true })));
    }

    #[test]
    fn uses_the_hex_view_for_binary_or_huge_files() {
        assert!(wants_hex_view(&probe(1, true, false)));
        assert!(wants_hex_view(&probe(1, false, true)));
        assert!(!wants_hex_view(&probe(1, false, false)));
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(300 * 1024 * 1024), "300 MB");
    }

    #[test]
    fn lets_a_text_editor_wrapper_take_precedence_over_the_image_viewer() {
        let wrapper = |path: &str| path.ends_with(".diagram.svg");
        assert_eq!(image_viewer_for("/p/icon.svg", wrapper).map(|input| input.kind), Some("image".to_owned()));
        assert_eq!(image_viewer_for("/p/arch.diagram.svg", wrapper), None);
    }

    #[test]
    fn shares_an_in_flight_probe_without_caching_its_result() {
        for binary in [false, true] {
            let mut probes = FileProbes::default();
            assert!(probes.probe("/p/a.ts", 1));
            assert!(!probes.probe("/p/a.ts", 2));
            let answers = probes.answered("/p/a.ts", Ok(&probe(12, binary, false)));
            let kinds: Vec<&str> = answers.iter().map(|(_, input)| kind(input)).collect();
            assert_eq!(kinds, if binary { ["binary", "binary"] } else { ["file", "file"] });
            assert_eq!(answers.iter().map(|(waiter, _)| *waiter).collect::<Vec<_>>(), [1, 2]);
            // The next open probes again and sees the file as it is now.
            assert!(probes.probe("/p/a.ts", 3));
            let again = probes.answered("/p/a.ts", Ok(&probe(20, !binary, false)));
            assert_eq!(kind(&again[0].1), if binary { "file" } else { "binary" });
        }
    }

    #[test]
    fn retries_a_failed_probe() {
        let mut probes = FileProbes::default();
        assert!(probes.probe("/p/a.ts", 1));
        assert_eq!(probes.answered("/p/a.ts", Err("read failed")), [(1, None)]);
        assert!(probes.probe("/p/a.ts", 2));
        let answers = probes.answered("/p/a.ts", Ok(&probe(20, true, false)));
        assert_eq!(kind(&answers[0].1), "binary");
    }

    #[test]
    fn probes_different_files_independently() {
        let mut probes = FileProbes::default();
        assert!(probes.probe("/p/a.ts", 1));
        assert!(probes.probe("/p/b.ts", 2));
        assert_eq!(probes.answered("/p/a.ts", Ok(&probe(20, false, false))), [(1, None)]);
        assert_eq!(probes.answered("/p/b.ts", Ok(&probe(20, false, false))), [(2, None)]);
    }
}
