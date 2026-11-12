//! Frame list presentation: library frames collapse into "… N library frames hidden".

use crate::debugger::effects::path_text;
use ide_dap::StackFrame;
use regex::Regex;
use std::sync::LazyLock;

static LIBRARY_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"/(?:node_modules|site-packages|dist-packages|\.cargo/registry|\.rustup|rustc/[0-9a-f]+",
        r"|go/pkg/mod|\.m2/repository|\.gradle/caches)/|^/(?:usr|opt|Library|System)/|<node_internals>",
    ))
    .expect("the library path pattern is valid")
});

/// The frame's source path as the UI holds it; `None` for a frame without a source.
pub fn frame_path(frame: &StackFrame) -> Option<String> {
    frame.path.as_ref().map(path_text).filter(|path| !path.is_empty())
}

/// Whether the adapter marks the frame as one to step over when reading a stack.
fn is_subtle(frame: &StackFrame) -> bool {
    matches!(frame.presentation_hint.as_deref(), Some("subtle" | "deemphasize"))
}

/// Whether a frame runs code outside the project `root`: a dependency, the runtime, generated code.
pub fn is_library_frame(frame: &StackFrame, root: Option<&str>) -> bool {
    if is_subtle(frame) {
        return true;
    }
    let Some(path) = frame_path(frame) else { return true };
    if LIBRARY_PATH.is_match(&path) {
        return true;
    }
    root.filter(|root| !root.is_empty())
        .is_some_and(|root| !path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/')))
}

/// A row of the frame list.
#[derive(Debug, Clone)]
pub enum FrameItem<'a> {
    Frame {
        frame: &'a StackFrame,
        library: bool,
    },
    /// A run of collapsed library frames; `key` identifies the row.
    Hidden {
        count: usize,
        key: String,
    },
}

/// Groups runs of library frames. The selected frame and the first frame are always shown;
/// `show_all` disables collapsing.
pub fn group_frames<'a>(
    frames: &'a [StackFrame],
    root: Option<&str>,
    selected: Option<i64>,
    show_all: bool,
) -> Vec<FrameItem<'a>> {
    fn flush<'a>(run: &mut Vec<&'a StackFrame>, out: &mut Vec<FrameItem<'a>>) {
        match run.as_slice() {
            [] => {}
            [frame] => out.push(FrameItem::Frame { frame, library: true }),
            [first, ..] => out.push(FrameItem::Hidden { count: run.len(), key: format!("hidden:{}", first.id) }),
        }
        run.clear();
    }
    let mut out = Vec::new();
    let mut run = Vec::new();
    for (i, frame) in frames.iter().enumerate() {
        let library = is_library_frame(frame, root);
        if !library || show_all || i == 0 || Some(frame.id) == selected {
            flush(&mut run, &mut out);
            out.push(FrameItem::Frame { frame, library });
        } else {
            run.push(frame);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// `create:22`: the frame's name and its 1-based line.
pub fn frame_label(frame: &StackFrame) -> String {
    if frame_path(frame).is_some() { format!("{}:{}", frame.name, frame.line + 1) } else { frame.name.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debugger::effects::ipc;

    const ROOT: Option<&str> = Some("/proj");

    fn frame(id: i64, path: Option<&str>, hint: Option<&str>, line: u32) -> StackFrame {
        StackFrame {
            id,
            name: format!("f{id}"),
            path: path.map(ipc),
            line,
            column: 0,
            presentation_hint: hint.map(str::to_owned),
        }
    }

    fn shape(items: &[FrameItem]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                FrameItem::Frame { frame, .. } => frame.id.to_string(),
                FrameItem::Hidden { count, .. } => format!("hidden{count}"),
            })
            .collect()
    }

    #[test]
    fn detects_library_frames() {
        assert!(!is_library_frame(&frame(1, Some("/proj/src/a.py"), None, 9), ROOT));
        assert!(is_library_frame(&frame(1, Some("/proj/node_modules/x/i.js"), None, 9), ROOT));
        assert!(is_library_frame(&frame(1, Some("/usr/lib/python3/x.py"), None, 9), ROOT));
        assert!(is_library_frame(&frame(1, None, None, 9), ROOT));
        assert!(is_library_frame(&frame(1, Some("/proj/a.py"), Some("subtle"), 9), ROOT));
        assert!(is_library_frame(&frame(1, Some("/elsewhere/a.py"), None, 9), ROOT));
    }

    #[test]
    fn collapses_runs_of_library_frames_but_keeps_the_top_and_selected_frames() {
        let frames = [
            frame(1, Some("/proj/a.py"), None, 9),
            frame(2, None, None, 9),
            frame(3, None, None, 9),
            frame(4, None, None, 9),
            frame(5, Some("/proj/b.py"), None, 9),
            frame(6, None, None, 9),
        ];
        assert_eq!(shape(&group_frames(&frames, ROOT, Some(1), false)), ["1", "hidden3", "5", "6"]);
        assert_eq!(shape(&group_frames(&frames, ROOT, Some(3), false)), ["1", "2", "3", "4", "5", "6"]);
        assert_eq!(group_frames(&frames, ROOT, Some(1), true).len(), 6);
    }

    #[test]
    fn labels_frames_with_1_based_lines() {
        assert_eq!(frame_label(&frame(1, Some("/proj/a.py"), None, 21)), "f1:22");
        assert_eq!(frame_label(&frame(1, None, None, 9)), "f1");
    }
}
