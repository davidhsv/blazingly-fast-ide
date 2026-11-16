# Code preview: integration notes

Worktree `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`. Nothing here was compiled. Every hunk below was checked by a script against the files as they are now: the first old line was found near the line given, and the anchor (two lines before) plus the old text occurs exactly once (`$S/dprev/hunks.py`, rerun it with `python3 -I $S/dprev/hunks.py <worktree> <out.md>` if the files moved; `mod.rs` and `workbench/code_insight.rs` changed while I wrote this).

## What the webview does, and what this changes (read this first)

The brief calls the preview "read-only". The webview's Find in Files and Bookmarks preview is not: `CodePreview` (`src/features/search/find/parts.tsx:105-242`) is a second editor on the document (editable unless the document is read-only), sharing edits, undo history and save with the tabs; the spec `codePreview.test.tsx` tests exactly that. Only the Find Usages pane (`SourcePreview`, `src/ui/SourcePreview.tsx`) and the Show Usages popup's `UsageCodePreview` (`UsagesPopup.tsx:251-299`) are read-only. The parity proofs say the same ("the preview as an editor on the document").

So the shared preview, `app-gpui/src/ui/code_preview.rs` (`CodePreview`), is a thin holder of an `Editor` (`editor::view::Editor`) on the backend document, as `workbench/code_insight.rs::preview_usage` and `workbench/todo_window.rs::preview_todo` already do, with a mode: `PreviewMode::Editable` (Find in Files, Bookmarks) or `PreviewMode::ReadOnly` (Find Usages). It debounces (80 ms), opens the document off the window's thread (`ide_host::ops::doc::open` through `Backend::with_host`), drops an answer the selection has left and closes the document reference it opened, marks the matches (`MarkLayer::Find`) and the line (a new `MarkKind::CurrentLine`), puts the caret on the line and centres it, and shows the empty text or the error in place. Row colours of Find Usages / Show Usages (`usagePreview.test.tsx`) are a separate model, `app-gpui/src/ui/code_preview/line_tokens.rs`.

Per frame: the preview's paint is the `Editor`'s, which shapes and paints only the visible rows (`editor/view.rs` prepaint, `visible = bounds.height / line_height + 1`), and iterates the marks of the layer per painted row. `CodePreview::render` iterates nothing. The usage rows iterate, per visible row, the tokens of that row's line only (`LineTokens::tokens` is a hash lookup). On the window's thread: `rope_snapshot` (a rope clone, O(log n)) and `Editor::with_text`; reading, highlighting and whole-file parsing run on the backend's runtime (`doc::open`, `doc_highlights`, `line_tokens::request` via `Host::blocking`).

Paste from History: its preview (`editor/paste_history_view/preview.rs`) shows a clipboard entry, not a document, so it should not move onto `CodePreview` (which needs a document). Its `line_runs` and `line_tokens::marked_runs` do the same slicing; a later change could share a `runs` helper. Left unchanged.

## (a) Hunks, in the order to apply them

Measured by reading, and the reason for the "Shared (unsaved previews)" hunks: natively, autosave (`Workbench::autosave`), Save All (`Workbench::save_all`) and the unsaved count the window's close question uses (`Workbench::unsaved_count`) cover only the tabs' editors (`Workbench::editors()`). A preview edit, or a Replace whose file the preview holds open (Replace leaves open documents unsaved, `crates/ide-host/src/ops/search/commands.rs:15-44`), would otherwise be lost silently when the window closes with no tab of the file. In the webview `DocumentManager.saveAll` covers every document (`src/editor/core/model/document.ts:801-806`) and the release of a document's last view autosaves it (`document.ts:758-769`); `hold_or_release` in `code_preview.rs` now autosaves the same way, and the three hunks count the held previews in Save All, autosave and the close question.

Land "Shared" first (the new files reference `MarkKind::CurrentLine`), then any of the places. The new files are declared by: `ui/mod.rs` (Shared hunk 1) for `ui/code_preview.rs`, which itself declares `line_tokens` and its `proofs26_code_preview` tests; `workbench/mod.rs` (Declarations) for `workbench/tests/proofs26_code_preview.rs`; `usages_view.rs` (Usages) for `code_insight/usages_view/tests/proofs26_usage_preview.rs`. `workbench/tests/proofs26_code_preview_pending.rs` stays undeclared, as the other `*_pending.rs` files are.

Where a hunk removes the last use of an import, the hunk list removes the import too (`Backend`, `Task`, `Path` in `bookmarks_popup.rs`; `utf16` in `usages_view.rs`; `trim_preview` in `usages_popup.rs`). Bookmarks hunk 14 leaves two blank lines: `cargo fmt` folds them.

### Shared

**app-gpui/src/ui/mod.rs**, hunk 1 (replace): declares the shared preview (and its `line_tokens` child module)

Anchor (the lines just before):

```rust
//! Logic behind the design-system primitives: matching, find-in-console, menu aiming.

```

Old lines:

```rust
pub mod console_find;
```

New lines:

```rust
pub mod code_preview;
pub mod console_find;
```

**app-gpui/src/editor/marks.rs**, hunk 1 (replace): the preview's line tint, drawn whether or not the preview has the keyboard

Anchor (the lines just before):

```rust
    /// A line with a disabled or muted breakpoint: only a hollow dot in the gutter.
    InactiveBreakpoint,
```

Old lines:

```rust
    /// The line where the debugger is suspended: its row is tinted.
    ExecutionLine,
```

New lines:

```rust
    /// The line where the debugger is suspended: its row is tinted.
    ExecutionLine,
    /// The line a code preview is on: its row is tinted in the current line's colour, focused or not.
    CurrentLine,
```

**app-gpui/src/editor/marks.rs**, hunk 2 (replace): a CurrentLine mark tints its row, never the text

Anchor (the lines just before):

```rust
    /// Tints the whole row of its line instead of the text.
    pub fn is_line(self) -> bool {
```

Old lines:

```rust
        matches!(self, Self::BreakpointLine | Self::UnverifiedBreakpoint | Self::ExecutionLine)
```

New lines:

```rust
        matches!(self, Self::BreakpointLine | Self::UnverifiedBreakpoint | Self::ExecutionLine | Self::CurrentLine)
```

**app-gpui/src/editor/view.rs**, hunk 1 (replace): its colour

Anchor (the lines just before):

```rust
            MarkKind::BreakpointLine | MarkKind::UnverifiedBreakpoint => theme::color(cx, "bg-breakpoint-line"),
            MarkKind::InactiveBreakpoint => theme::color(cx, "fg-gutter"),
```

Old lines:

```rust
            MarkKind::ExecutionLine => theme::color(cx, "bg-debug-line"),
```

New lines:

```rust
            MarkKind::ExecutionLine => theme::color(cx, "bg-debug-line"),
            MarkKind::CurrentLine => theme::color(cx, "bg-current-line"),
```

**app-gpui/src/editor/view.rs**, hunk 2 (replace): paints the tint (and records it in `painted_marks` for tests)

Anchor (the lines just before):

```rust
                    window.paint_quad(fill(row, theme::color(cx, "bg-current-line")));
                }
```

Old lines:

```rust
                // The breakpoint row is tinted, and the execution line goes over it.
                for kind in [MarkKind::BreakpointLine, MarkKind::UnverifiedBreakpoint, MarkKind::ExecutionLine] {
```

New lines:

```rust
                // The preview's line is tinted first; the breakpoint row goes over it, the execution line over
                // both.
                for kind in [
                    MarkKind::CurrentLine,
                    MarkKind::BreakpointLine,
                    MarkKind::UnverifiedBreakpoint,
                    MarkKind::ExecutionLine,
                ] {
```

### Find in Files

**app-gpui/src/find/popup_view.rs**, hunk 1 (insert after the anchor): imports

Anchor (the lines just before):

```rust
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
```

New lines:

```rust
use crate::ui::code_preview::{CodePreview, CodePreviewEvent, PreviewHighlight, PreviewMode, PreviewTarget};
```

**app-gpui/src/find/popup_view.rs**, hunk 2 (delete): the plain-text preview's keys go

Anchor (the lines just before):

```rust
}

```

Old lines:

```rust
/// Which text of a file the preview shows: the file as it was after `version` replacements.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewKey {
    path: String,
    version: u64,
}

struct FilePreview {
    key: PreviewKey,
    lines: Rc<Vec<String>>,
}

```

New lines:
(nothing)

**app-gpui/src/find/popup_view.rs**, hunk 3 (replace): the field

Anchor (the lines just before):

```rust
    /// Replace: the preview shows the replacement as a diff.
    preview_mode: bool,
```

Old lines:

```rust
    preview: Option<FilePreview>,
    /// The text the preview is waiting for, or shows.
    preview_wanted: Option<PreviewKey>,
    /// Why the wanted file cannot be shown.
    preview_error: Option<String>,
    /// The file and line the preview was last scrolled to.
    preview_shown: Option<(String, u32)>,
```

New lines:

```rust
    /// The selected result's file, in an editor on its document.
    code_preview: Entity<CodePreview>,
```

**app-gpui/src/find/popup_view.rs**, hunk 4 (replace)

Anchor (the lines just before):

```rust
    running: Option<ReplaceRun>,
    progress: Option<ReplaceProgress>,
```

Old lines:

```rust
    list_scroll: UniformListScrollHandle,
    preview_scroll: UniformListScrollHandle,
    diff_scroll: UniformListScrollHandle,
```

New lines:

```rust
    list_scroll: UniformListScrollHandle,
    diff_scroll: UniformListScrollHandle,
```

**app-gpui/src/find/popup_view.rs**, hunk 5 (replace)

Anchor (the lines just before):

```rust
    _masks_load: Option<Task<()>>,
    _browse: Option<Task<()>>,
```

Old lines:

```rust
    _field_events: Vec<Subscription>,
```

New lines:

```rust
    _field_events: Vec<Subscription>,
    _preview_events: Subscription,
```

**app-gpui/src/find/popup_view.rs**, hunk 6 (replace): creates the preview in `FindPopup::new`

Anchor (the lines just before):

```rust
            }
        })
```

Old lines:

```rust
        .detach();
        let mut popup = Self {
```

New lines:

```rust
        .detach();
        let code_preview = cx.new(|cx| {
            CodePreview::new(&window_label, PreviewMode::Editable, "Select a result to preview", PREVIEW_DEBOUNCE, cx)
        });
        let preview_events = cx.subscribe(&code_preview, |popup: &mut FindPopup, _, event: &CodePreviewEvent, cx| {
            popup.code_preview_event(event, cx);
        });
        let mut popup = Self {
```

**app-gpui/src/find/popup_view.rs**, hunk 7 (replace)

Anchor (the lines just before):

```rust
            menu: None,
            preview_mode: false,
```

Old lines:

```rust
            preview: None,
            preview_wanted: None,
            preview_error: None,
            preview_shown: None,
```

New lines:

```rust
            code_preview,
```

**app-gpui/src/find/popup_view.rs**, hunk 8 (replace)

Anchor (the lines just before):

```rust
            running: None,
            progress: None,
```

Old lines:

```rust
            list_scroll: UniformListScrollHandle::new(),
            preview_scroll: UniformListScrollHandle::new(),
            diff_scroll: UniformListScrollHandle::new(),
```

New lines:

```rust
            list_scroll: UniformListScrollHandle::new(),
            diff_scroll: UniformListScrollHandle::new(),
```

**app-gpui/src/find/popup_view.rs**, hunk 9 (replace)

Anchor (the lines just before):

```rust
            _masks_load: None,
            _browse: None,
```

Old lines:

```rust
            _field_events: field_events,
```

New lines:

```rust
            _field_events: field_events,
            _preview_events: preview_events,
```

**app-gpui/src/find/popup_view.rs**, hunk 10 (replace): the accessors read the preview's editor (they take `cx` now)

Anchor (the lines just before):

```rust
    }

```

Old lines:

```rust
    /// The file the preview shows, once it is read.
    pub fn preview_path(&self) -> Option<&str> {
        self.shown_preview().map(|preview| preview.key.path.as_str())
    }

    /// The 0-based line the preview is on and its text.
    pub fn preview_line(&self) -> Option<(u32, String)> {
        let (preview, row) = (self.shown_preview()?, self.selected_row()?);
        Some((row.line, preview.lines.get(row.line as usize)?.clone()))
    }

    /// Every line of the previewed file.
    pub fn preview_lines(&self) -> Vec<String> {
        self.shown_preview().map(|preview| preview.lines.to_vec()).unwrap_or_default()
    }
```

New lines:

```rust
    /// The file the preview shows, once its editor is there.
    pub fn preview_path(&self, cx: &App) -> Option<String> {
        self.code_preview.read(cx).path().map(str::to_owned)
    }

    /// The 0-based line the preview is on and its text.
    pub fn preview_line(&self, cx: &App) -> Option<(u32, String)> {
        self.code_preview.read(cx).line(cx)
    }

    /// Every line of the previewed file, as its editor has it.
    pub fn preview_lines(&self, cx: &App) -> Vec<String> {
        self.code_preview.read(cx).lines(cx)
    }

    /// The preview of the selected result.
    pub fn code_preview(&self) -> &Entity<CodePreview> {
        &self.code_preview
    }
```

**app-gpui/src/find/popup_view.rs**, hunk 11 (delete)

Anchor (the lines just before):

```rust
    }

```

Old lines:

```rust
    /// The file preview, when it is the selected row's file as it is now.
    fn shown_preview(&self) -> Option<&FilePreview> {
        let row = self.selected_row()?;
        let key = PreviewKey { path: row.path.clone(), version: self.preview_version };
        self.preview.as_ref().filter(|preview| preview.key == key)
    }

```

New lines:
(nothing)

**app-gpui/src/find/popup_view.rs**, hunk 12 (replace): the file branch hands the row to the shared preview; `preview_loaded` and `reveal_preview_line` go

Anchor (the lines just before):

```rust
    }

```

Old lines:

```rust
    /// Brings the preview up to the selected row: its file is read once the selection has rested on it,
    /// and scrolled to the row's line; in the diff mode of Replace the row's replacement is asked for
    /// instead.
    fn sync_preview(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.selected_row().cloned() else {
            self._preview_task = None;
            self.preview_wanted = self.preview.as_ref().map(|preview| preview.key.clone());
            self.diff_wanted = self.diff.as_ref().map(|diff| diff.key.clone());
            return;
        };
        if self.shows_diff() {
            self.sync_diff(&row, cx);
            return;
        }
        let key = PreviewKey { path: row.path.clone(), version: self.preview_version };
        if self.preview.as_ref().is_some_and(|preview| preview.key == key) {
            self.reveal_preview_line(&row);
            return;
        }
        if self.preview_wanted.as_ref() == Some(&key) {
            return;
        }
        self.preview_wanted = Some(key.clone());
        self.diff_wanted = self.diff.as_ref().map(|diff| diff.key.clone());
        self._preview_task = Some(cx.spawn(async move |popup, cx| {
            cx.background_executor().timer(PREVIEW_DEBOUNCE).await;
            let read = popup.update(cx, |popup, cx| {
                let backend = cx.try_global::<Backend>()?;
                let (window, path) = (popup.window_label.clone(), PathBuf::from(&key.path));
                Some(
                    backend
                        .with_host(move |host| async move { ide_host::ops::fs::read_text(&host, &window, path).await }),
                )
            });
            let Ok(Some(read)) = read else { return };
            let text = match read.await {
                Ok(Ok(text)) => Ok(text),
                Ok(Err(error)) => Err(error.message),
                Err(error) => Err(error.to_string()),
            };
            let _ = popup.update(cx, |popup, cx| popup.preview_loaded(key, text, cx));
        }));
    }

    fn preview_loaded(&mut self, key: PreviewKey, text: Result<String, String>, cx: &mut Context<Self>) {
        if self.preview_wanted.as_ref() != Some(&key) {
            return;
        }
        self.preview_shown = None;
        match text {
            Ok(text) => {
                let lines = text.lines().map(str::to_owned).collect();
                self.preview = Some(FilePreview { key, lines: Rc::new(lines) });
                self.preview_error = None;
                if let Some(row) = self.selected_row().cloned() {
                    self.reveal_preview_line(&row);
                }
            }
            Err(message) => {
                self.preview = None;
                self.preview_error = Some(message);
            }
        }
        cx.notify();
    }

    /// Scrolls the preview to the row's line, in the middle, when it is not the line it was last put on.
    fn reveal_preview_line(&mut self, row: &MatchRow) {
        let at = (row.path.clone(), row.line);
        if self.preview_shown.as_ref() != Some(&at) {
            self.preview_scroll.scroll_to_item(row.line as usize, ScrollStrategy::Center);
            self.preview_shown = Some(at);
        }
    }
```

New lines:

```rust
    /// Brings the preview up to the selected row: its file, on the row's line with the file's matches marked,
    /// opened once the selection has rested on it; in the diff mode of Replace the row's replacement is asked
    /// for instead.
    fn sync_preview(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.selected_row().cloned() else {
            self._preview_task = None;
            self.diff_wanted = self.diff.as_ref().map(|diff| diff.key.clone());
            self.code_preview.update(cx, |preview, cx| preview.show(None, cx));
            return;
        };
        if self.shows_diff() {
            self.sync_diff(&row, cx);
            return;
        }
        self._preview_task = None;
        self.diff_wanted = self.diff.as_ref().map(|diff| diff.key.clone());
        let mut highlights: Vec<PreviewHighlight> =
            self.file_highlights().into_iter().map(|(line, ranges)| PreviewHighlight { line, ranges }).collect();
        highlights.sort_by_key(|highlight| highlight.line);
        let (path, line, version) = (row.path.clone(), row.line, self.preview_version);
        let target = PreviewTarget { path, line, highlights, version };
        self.code_preview.update(cx, |preview, cx| preview.show(Some(target), cx));
    }

    /// What the preview asks: Escape closes the popup, a failed save is told.
    fn code_preview_event(&mut self, event: &CodePreviewEvent, cx: &mut Context<Self>) {
        match event {
            CodePreviewEvent::Escape => cx.emit(FindPopupEvent::Dismissed),
            CodePreviewEvent::SaveFailed(message) => {
                let title = cx.default_global::<Localization>().0.t("app.unsaved.saveFailed", &[]);
                let failed = NotificationRequest::new(NotificationLevel::Error, title);
                cx.emit(FindPopupEvent::Notify(NotificationRequest { body: Some(message.clone()), ..failed }));
            }
        }
    }
```

**app-gpui/src/find/popup_view.rs**, hunk 13 (replace): `sync_diff` no longer resets the file preview's wish

Anchor (the lines just before):

```rust
            return;
        }
```

Old lines:

```rust
        self.diff_wanted = Some(key.clone());
        self.preview_wanted = self.preview.as_ref().map(|preview| preview.key.clone());
```

New lines:

```rust
        self.diff_wanted = Some(key.clone());
```

**app-gpui/src/find/popup_view.rs**, hunk 14 (replace): `render_preview` draws the shared preview (it draws its own empty text and errors)

Anchor (the lines just before):

```rust
                }
            }
```

Old lines:

```rust
        } else {
            match (self.shown_preview(), self.selected_row()) {
                (Some(preview), Some(row)) => {
                    let (lines, current_line) = (Rc::clone(&preview.lines), row.line);
                    let highlights = Rc::new(self.file_highlights());
                    let current = theme::color(cx, "bg-current-line");
                    let (mark, mark_current) =
                        (theme::color(cx, "bg-search-match"), theme::color(cx, "bg-search-match-current"));
                    let mark_fg = theme::color(cx, "fg-search-match");
                    uniform_list(
                        "find-popup-preview",
                        lines.len(),
                        cx.processor(move |_, range: Range<usize>, _, _| {
                            range
                                .map(|index| {
                                    let number = index as u32;
                                    let on_line = number == current_line;
                                    let ranges = highlights.get(&number).map(Vec::as_slice).unwrap_or_default();
                                    let pieces = segments(&lines[index], ranges);
                                    line_frame(number, None).when(on_line, |el| el.bg(current)).children(marked(
                                        pieces,
                                        if on_line { mark_current } else { mark },
                                        mark_fg,
                                    ))
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&self.preview_scroll)
                    .size_full()
                    .into_any_element()
                }
                _ => {
                    let note = match (&self.preview_error, self.selected_row()) {
                        (_, None) => tx(cx, "Select a result to preview"),
                        (Some(error), Some(_)) => error.clone(),
                        (None, Some(_)) => String::new(),
                    };
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(subtle)
                        .child(SharedString::from(note))
                        .into_any_element()
                }
            }
        };
```

New lines:

```rust
        } else {
            self.code_preview.clone().into_any_element()
        };
```

### Find in Files (tests reading the accessors)

**app-gpui/src/find/popup_view.rs**, hunk 15 (replace)

Anchor (the lines just before):

```rust

        // The preview shows the selected match in its file, with the file's matches marked.
```

Old lines:

```rust
        settle(cx, |cx| popup.read_with(cx, |popup, _| popup.preview_path().is_some()));
```

New lines:

```rust
        settle(cx, |cx| popup.read_with(cx, |popup, cx| popup.preview_path(cx).is_some()));
```

**app-gpui/src/find/popup_view.rs**, hunk 16 (replace)

Anchor (the lines just before):

```rust
        // The preview shows the selected match in its file, with the file's matches marked.
        settle(cx, |cx| popup.read_with(cx, |popup, _| popup.preview_path().is_some()));
```

Old lines:

```rust
        popup.read_with(cx, |popup, _| {
```

New lines:

```rust
        popup.read_with(cx, |popup, cx| {
```

**app-gpui/src/find/popup_view.rs**, hunk 17 (replace)

Anchor (the lines just before):

```rust
        settle(cx, |cx| popup.read_with(cx, |popup, _| popup.preview_path().is_some()));
        popup.read_with(cx, |popup, _| {
```

Old lines:

```rust
            assert_eq!(popup.preview_path(), Some(&*path.to_string_lossy()));
```

New lines:

```rust
            assert_eq!(popup.preview_path(cx).as_deref(), Some(&*path.to_string_lossy()));
```

**app-gpui/src/find/popup_view.rs**, hunk 18 (replace)

Anchor (the lines just before):

```rust
        popup.read_with(cx, |popup, _| {
            assert_eq!(popup.preview_path(), Some(&*path.to_string_lossy()));
```

Old lines:

```rust
            assert_eq!(popup.preview_line(), Some((0, "needle 0".to_owned())));
```

New lines:

```rust
            assert_eq!(popup.preview_line(cx), Some((0, "needle 0".to_owned())));
```

**app-gpui/src/find/popup_view.rs**, hunk 19 (replace)

Anchor (the lines just before):

```rust
            assert_eq!(popup.preview_path(), Some(&*path.to_string_lossy()));
            assert_eq!(popup.preview_line(), Some((0, "needle 0".to_owned())));
```

Old lines:

```rust
            assert_eq!(popup.preview_lines().len(), 12);
```

New lines:

```rust
            assert_eq!(popup.preview_lines(cx).len(), 12);
```

**app-gpui/src/find/popup_view.rs**, hunk 20 (replace)

Anchor (the lines just before):

```rust
        cx.simulate_keystrokes("down");
        assert_eq!(selected(&popup, cx), row(1));
```

Old lines:

```rust
        assert_eq!(popup.read_with(cx, |popup, _| popup.preview_line()), Some((1, "needle 1".to_owned())));
```

New lines:

```rust
        assert_eq!(popup.read_with(cx, |popup, cx| popup.preview_line(cx)), Some((1, "needle 1".to_owned())));
```

**app-gpui/src/find/popup_view.rs**, hunk 21 (replace)

Anchor (the lines just before):

```rust
        assert_eq!(selected(&popup, cx).as_ref(), Some(&rows[b]));
        let shown = project.root.join("b.txt").to_string_lossy().into_owned();
```

Old lines:

```rust
        settle(cx, |cx| popup.read_with(cx, |popup, _| popup.preview_path() == Some(shown.as_str())));
```

New lines:

```rust
        settle(cx, |cx| popup.read_with(cx, |popup, cx| popup.preview_path(cx).as_deref() == Some(shown.as_str())));
```

**app-gpui/src/find/popup_view.rs**, hunk 22 (replace)

Anchor (the lines just before):

```rust
        let shown = project.root.join("b.txt").to_string_lossy().into_owned();
        settle(cx, |cx| popup.read_with(cx, |popup, _| popup.preview_path() == Some(shown.as_str())));
```

Old lines:

```rust
        popup.read_with(cx, |popup, _| {
```

New lines:

```rust
        popup.read_with(cx, |popup, cx| {
```

**app-gpui/src/find/popup_view.rs**, hunk 23 (replace)

Anchor (the lines just before):

```rust
        settle(cx, |cx| popup.read_with(cx, |popup, _| popup.preview_path() == Some(shown.as_str())));
        popup.read_with(cx, |popup, _| {
```

Old lines:

```rust
            assert_eq!(popup.preview_lines(), ["x", "needle b"]);
```

New lines:

```rust
            assert_eq!(popup.preview_lines(cx), ["x", "needle b"]);
```

**app-gpui/src/find/popup_view.rs**, hunk 24 (replace)

Anchor (the lines just before):

```rust
        popup.read_with(cx, |popup, _| {
            assert_eq!(popup.preview_lines(), ["x", "needle b"]);
```

Old lines:

```rust
            assert_eq!(popup.preview_line(), Some((1, "needle b".to_owned())));
```

New lines:

```rust
            assert_eq!(popup.preview_line(cx), Some((1, "needle b".to_owned())));
```

**app-gpui/src/find/popup_view.rs**, hunk 25 (replace)

Anchor (the lines just before):

```rust
        assert_eq!(last(&events), told("Replaced 2 occurrences in 1 file"));
        // The preview shows the file as it is now.
```

Old lines:

```rust
        settle(cx, |cx| popup.read_with(cx, |popup, _| popup.preview_line().is_some()));
```

New lines:

```rust
        settle(cx, |cx| popup.read_with(cx, |popup, cx| popup.preview_line(cx).is_some()));
```

**app-gpui/src/find/popup_view.rs**, hunk 26 (replace)

Anchor (the lines just before):

```rust
        // The preview shows the file as it is now.
        settle(cx, |cx| popup.read_with(cx, |popup, _| popup.preview_line().is_some()));
```

Old lines:

```rust
        assert_eq!(popup.read_with(cx, |popup, _| popup.preview_line()), Some((1, "qux two qux".to_owned())));
```

New lines:

```rust
        assert_eq!(popup.read_with(cx, |popup, cx| popup.preview_line(cx)), Some((1, "qux two qux".to_owned())));
```

**app-gpui/src/workbench/tests/proofs_editor_search_find.rs**, hunk 1 (replace)

Anchor (the lines just before):

```rust
    let notes = text_of(&root.join("notes.txt"));
    wait_long(cx, "the preview of notes.txt", |cx| {
```

Old lines:

```rust
        popup.read_with(cx, |popup, _| popup.preview_path() == Some(notes.as_str()))
```

New lines:

```rust
        popup.read_with(cx, |popup, cx| popup.preview_path(cx).as_deref() == Some(notes.as_str()))
```

**app-gpui/src/workbench/tests/proofs_editor_search_find.rs**, hunk 2 (replace)

Anchor (the lines just before):

```rust
        popup.read_with(cx, |popup, _| popup.preview_path() == Some(notes.as_str()))
    });
```

Old lines:

```rust
    popup.read_with(cx, |popup, _| {
```

New lines:

```rust
    popup.read_with(cx, |popup, cx| {
```

**app-gpui/src/workbench/tests/proofs_editor_search_find.rs**, hunk 3 (replace)

Anchor (the lines just before):

```rust
    });
    popup.read_with(cx, |popup, _| {
```

Old lines:

```rust
        assert_eq!(popup.preview_line(), Some((1, "  a needle here".to_owned())));
```

New lines:

```rust
        assert_eq!(popup.preview_line(cx), Some((1, "  a needle here".to_owned())));
```

**app-gpui/src/workbench/tests/proofs_editor_search_find.rs**, hunk 4 (replace)

Anchor (the lines just before):

```rust
    let path = text_of(&root.join("two.txt"));
    wait_long(cx, "the preview of two.txt", |cx| {
```

Old lines:

```rust
        popup.read_with(cx, |popup, _| popup.preview_path() == Some(path.as_str()) && popup.preview_line().is_some())
```

New lines:

```rust
        popup.read_with(cx, |popup, cx| {
            popup.preview_path(cx).as_deref() == Some(path.as_str()) && popup.preview_line(cx).is_some()
        })
```

**app-gpui/src/workbench/tests/proofs_editor_search_find.rs**, hunk 5 (replace)

Anchor (the lines just before):

```rust
        popup.read_with(cx, |popup, _| popup.preview_path() == Some(path.as_str()) && popup.preview_line().is_some())
    });
```

Old lines:

```rust
    popup.read_with(cx, |popup, _| {
```

New lines:

```rust
    popup.read_with(cx, |popup, cx| {
```

**app-gpui/src/workbench/tests/proofs_editor_search_find.rs**, hunk 6 (replace)

Anchor (the lines just before):

```rust
    });
    popup.read_with(cx, |popup, _| {
```

Old lines:

```rust
        assert_eq!(popup.preview_line(), Some((0, "needle one".to_owned())));
```

New lines:

```rust
        assert_eq!(popup.preview_line(cx), Some((0, "needle one".to_owned())));
```

**app-gpui/src/workbench/tests/proofs_editor_search_find.rs**, hunk 7 (replace)

Anchor (the lines just before):

```rust
    cx.simulate_keystrokes("down");
    wait_long(cx, "the preview to follow the selection", |cx| {
```

Old lines:

```rust
        popup.read_with(cx, |popup, _| popup.preview_line() == Some((2, "needle two needle".to_owned())))
```

New lines:

```rust
        popup.read_with(cx, |popup, cx| popup.preview_line(cx) == Some((2, "needle two needle".to_owned())))
```

**app-gpui/src/workbench/tests/proofs12_find_popup.rs**, hunk 1 (replace)

Anchor (the lines just before):

```rust
    popup.update(cx, |popup, cx| popup.set_replacement("item", cx));
    searched(&popup, cx);
```

Old lines:

```rust
    wait_long(cx, "the code preview", |cx| popup.read_with(cx, |popup, _| popup.preview_line().is_some()));
```

New lines:

```rust
    wait_long(cx, "the code preview", |cx| popup.read_with(cx, |popup, cx| popup.preview_line(cx).is_some()));
```

**app-gpui/src/workbench/tests/proofs12_find_popup.rs**, hunk 2 (replace)

Anchor (the lines just before):

```rust
    let popup = find_popup(&workbench, cx);
    assert!(rows(&popup, cx).is_empty());
```

Old lines:

```rust
    assert!(popup.read_with(cx, |popup, _| popup.preview_path().is_none() && popup.preview_lines().is_empty()));
```

New lines:

```rust
    assert!(popup.read_with(cx, |popup, cx| popup.preview_path(cx).is_none() && popup.preview_lines(cx).is_empty()));
```

**app-gpui/src/workbench/tests/proofs12_find_popup.rs**, hunk 3 (replace)

Anchor (the lines just before):

```rust
    let first = selected(&popup, cx).expect("a row is selected");
    let file = text_of(&root.join(first.split_once(':').expect("a row names its file").0));
```

Old lines:

```rust
    wait_long(cx, "the preview", |cx| popup.read_with(cx, |popup, _| popup.preview_path() == Some(file.as_str())));
```

New lines:

```rust
    wait_long(cx, "the preview", |cx| {
        popup.read_with(cx, |popup, cx| popup.preview_path(cx).as_deref() == Some(file.as_str()))
    });
```

**app-gpui/src/workbench/tests/proofs12_find_popup.rs**, hunk 4 (replace)

Anchor (the lines just before):

```rust
    let file = text_of(&root.join(first.split_once(':').expect("a row names its file").0));
    wait_long(cx, "the preview", |cx| popup.read_with(cx, |popup, _| popup.preview_path() == Some(file.as_str())));
```

Old lines:

```rust
    assert!(!popup.read_with(cx, |popup, _| popup.preview_lines()).is_empty());
```

New lines:

```rust
    assert!(!popup.read_with(cx, |popup, cx| popup.preview_lines(cx)).is_empty());
```

### Bookmarks

**app-gpui/src/everywhere/bookmarks_popup.rs**, hunk 1 (delete): the popup reads no file itself any more

Anchor (the lines just before):

```rust
use super::bookmarks_view::{BookmarkNode, build_bookmark_tree, group_bookmarks};
use super::open::StorageWrite;
```

Old lines:

```rust
use crate::backend::Backend;
```

New lines:
(nothing)

**app-gpui/src/everywhere/bookmarks_popup.rs**, hunk 2 (replace)

Anchor (the lines just before):

```rust
use crate::platform::local_time::now_ms;
use crate::platform::virtual_docs::{basename, relative_path};
```

Old lines:

```rust
use crate::theme;
```

New lines:

```rust
use crate::theme;
use crate::ui::code_preview::{CodePreview, CodePreviewEvent, PREVIEW_DEBOUNCE, PreviewMode, PreviewTarget};
```

**app-gpui/src/everywhere/bookmarks_popup.rs**, hunk 3 (replace): `Task` goes, `Subscription` comes (rustfmt may re-flow the list)

Anchor (the lines just before):

```rust
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla, IntoElement,
```

Old lines:

```rust
    Keystroke, Render, ScrollStrategy, SharedString, Task, UniformListScrollHandle, Window, div, prelude::*, px,
```

New lines:

```rust
    Keystroke, Render, ScrollStrategy, SharedString, Subscription, UniformListScrollHandle, Window, div, prelude::*,
    px,
```

**app-gpui/src/everywhere/bookmarks_popup.rs**, hunk 4 (replace)

Anchor (the lines just before):

```rust
use std::cmp::Ordering;
use std::ops::Range;
```

Old lines:

```rust
use std::path::{Path, PathBuf};
```

New lines:

```rust
use std::path::PathBuf;
```

**app-gpui/src/everywhere/bookmarks_popup.rs**, hunk 5 (delete)

Anchor (the lines just before):

```rust
/// How many rows Page Up and Page Down move by: one less than the list shows.
const PAGE_ROWS: usize = 18;
```

Old lines:

```rust
/// The most that is read of a file that no editor has open, for the text beside the list.
const PREVIEW_BYTES: usize = 2 * 1024 * 1024;
```

New lines:
(nothing)

**app-gpui/src/everywhere/bookmarks_popup.rs**, hunk 6 (delete)

Anchor (the lines just before):

```rust
}

```

Old lines:

```rust
/// The lines of `text` as the popup draws them: tabs take four columns.
fn preview_lines(text: &str) -> Vec<SharedString> {
    text.lines().map(|line| SharedString::from(line.replace('\t', "    "))).collect()
}

/// The first font of the theme's `--font-mono` list that is installed.
fn code_font(cx: &App) -> SharedString {
    crate::theme::installed_fonts::mono_family(cx)
}

```

New lines:
(nothing)

**app-gpui/src/everywhere/bookmarks_popup.rs**, hunk 7 (replace)

Anchor (the lines just before):

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookmarksPopupEvent {
```

Old lines:

```rust
    /// Escape.
    Dismissed,
```

New lines:

```rust
    /// Escape.
    Dismissed,
    /// Saving from the preview failed, for this reason: the window tells it.
    SaveFailed(String),
```

**app-gpui/src/everywhere/bookmarks_popup.rs**, hunk 8 (delete)

Anchor (the lines just before):

```rust
}

```

Old lines:

```rust
/// The file whose text is shown beside the list.
struct Preview {
    path: String,
    /// The file's lines; `None` until they are read.
    lines: Option<Rc<Vec<SharedString>>>,
}

```

New lines:
(nothing)

**app-gpui/src/everywhere/bookmarks_popup.rs**, hunk 9 (replace)

Anchor (the lines just before):

```rust
    editing: Option<(String, u32)>,
    edit_field: Entity<TextField>,
```

Old lines:

```rust
    preview: Option<Preview>,
    code_font: SharedString,
    scroll: UniformListScrollHandle,
    preview_scroll: UniformListScrollHandle,
    /// The claims on the keys the popup handles itself, ahead of the keymap: in the list and in the
    /// description field.
    key_claims: [u64; 2],
    /// The question to the backend for the text of the file shown; a newer one takes its place.
    _preview_load: Option<Task<()>>,
```

New lines:

```rust
    /// The selected bookmark's file, in an editor on its document.
    code_preview: Entity<CodePreview>,
    scroll: UniformListScrollHandle,
    /// The claims on the keys the popup handles itself, ahead of the keymap: in the list and in the
    /// description field.
    key_claims: [u64; 2],
    _preview_events: Subscription,
```

**app-gpui/src/everywhere/bookmarks_popup.rs**, hunk 10 (replace)

Anchor (the lines just before):

```rust
        })
        .detach();
```

Old lines:

```rust
        let rows = bookmark_rows(store.data());
```

New lines:

```rust
        let code_preview = cx.new(|cx| {
            CodePreview::new(window_label, PreviewMode::Editable, "Select a result to preview", PREVIEW_DEBOUNCE, cx)
        });
        let preview_events = cx.subscribe(&code_preview, |_: &mut BookmarksPopup, _, event: &CodePreviewEvent, cx| {
            cx.emit(match event {
                CodePreviewEvent::Escape => BookmarksPopupEvent::Dismissed,
                CodePreviewEvent::SaveFailed(message) => BookmarksPopupEvent::SaveFailed(message.clone()),
            });
        });
        let rows = bookmark_rows(store.data());
```

**app-gpui/src/everywhere/bookmarks_popup.rs**, hunk 11 (replace)

Anchor (the lines just before):

```rust
            editing: None,
            edit_field,
```

Old lines:

```rust
            preview: None,
            code_font: code_font(cx),
            scroll: UniformListScrollHandle::new(),
            preview_scroll: UniformListScrollHandle::new(),
            key_claims: [list_claim, edit_claim],
            _preview_load: None,
```

New lines:

```rust
            code_preview,
            scroll: UniformListScrollHandle::new(),
            key_claims: [list_claim, edit_claim],
            _preview_events: preview_events,
```

**app-gpui/src/everywhere/bookmarks_popup.rs**, hunk 12 (replace)

Anchor (the lines just before):

```rust
    }

```

Old lines:

```rust
    /// The lines shown beside the list; none until the file is read.
    pub fn preview_text(&self) -> Vec<String> {
        let lines = self.preview.as_ref().and_then(|preview| preview.lines.as_ref());
        lines.map(|lines| lines.iter().map(ToString::to_string).collect()).unwrap_or_default()
    }
```

New lines:

```rust
    /// The lines shown beside the list, as the preview's editor has them; none until the file is open.
    pub fn preview_text(&self, cx: &App) -> Vec<String> {
        self.code_preview.read(cx).lines(cx)
    }

    /// The preview of the selected bookmark.
    pub fn code_preview(&self) -> &Entity<CodePreview> {
        &self.code_preview
    }
```

**app-gpui/src/everywhere/bookmarks_popup.rs**, hunk 13 (replace): `load_preview` and `preview_loaded` go

Anchor (the lines just before):

```rust
    }

```

Old lines:

```rust
    /// Shows the text of the selected bookmark's file beside the list, scrolled to its line. The text of
    /// another file is asked of the backend.
    fn sync_preview(&mut self, cx: &mut Context<Self>) {
        let Some((path, line)) = self.preview_target().map(|(path, line)| (path.to_owned(), line)) else {
            (self.preview, self._preview_load) = (None, None);
            return;
        };
        if self.preview.as_ref().is_none_or(|preview| preview.path != path) {
            self.preview = Some(Preview { path: path.clone(), lines: None });
            self.load_preview(path, cx);
        }
        self.preview_scroll.scroll_to_item(line.unwrap_or(0) as usize, ScrollStrategy::Center);
    }

    /// Asks the backend for the text of `path`: what an editor of the file shows when one is open, else
    /// what is on disk. Reading it opens no document and leaves the recent files as they are.
    fn load_preview(&mut self, path: String, cx: &mut Context<Self>) {
        self._preview_load = None;
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let (window, file) = (self.window_label.clone(), path.clone());
        let text = backend.with_host(move |host| async move {
            let project = host.project(&window)?;
            host.blocking(move || {
                let file = Path::new(&file);
                match project.documents.text_of(file) {
                    Some(text) => Ok(text),
                    None => project.documents.load_file_bounded(file, PREVIEW_BYTES).map(|loaded| loaded.text),
                }
            })
            .await
        });
        self._preview_load = Some(cx.spawn(async move |popup, cx| {
            let text = match text.await {
                Ok(Ok(text)) => text,
                Ok(Err(error)) => {
                    tracing::debug!("bookmarks: cannot read {path}: {}", error.message);
                    String::new()
                }
                Err(error) => {
                    tracing::debug!("bookmarks: cannot read {path}: {error}");
                    String::new()
                }
            };
            // The popup is gone when it was closed meanwhile.
            let _ = popup.update(cx, |popup, cx| popup.preview_loaded(&path, &text, cx));
        }));
    }

    /// The text of `path` arrived. It is dropped when the selection has moved to another file since.
    fn preview_loaded(&mut self, path: &str, text: &str, cx: &mut Context<Self>) {
        let line = self.preview_target().and_then(|(_, line)| line).unwrap_or(0);
        let Some(preview) = self.preview.as_mut().filter(|preview| preview.path == path) else { return };
        preview.lines = Some(Rc::new(preview_lines(text)));
        self.preview_scroll.scroll_to_item(line as usize, ScrollStrategy::Center);
        cx.notify();
    }
```

New lines:

```rust
    /// Shows the selected bookmark's file beside the list in an editor on its document, on the bookmarked line
    /// (a bookmarked file from its top); a folder shows nothing.
    fn sync_preview(&mut self, cx: &mut Context<Self>) {
        let target = self.preview_target().map(|(path, line)| PreviewTarget {
            path: path.to_owned(),
            line: line.unwrap_or(0),
            highlights: Vec::new(),
            version: 0,
        });
        self.code_preview.update(cx, |preview, cx| preview.show(target, cx));
    }
```

**app-gpui/src/everywhere/bookmarks_popup.rs**, hunk 14 (delete): the hand-drawn line list goes (keep the blank line before it)

Anchor (the lines just before):

```rust
        };

```

Old lines:

```rust
        let marked = self.preview_target().and_then(|(_, line)| line).map(|line| line as usize);
        let shown = self.preview.as_ref().and_then(|preview| preview.lines.clone()).filter(|lines| !lines.is_empty());
        let (line_height, number_color) = (theme::length(cx, "lh-code"), theme::color(cx, "fg-line-number"));
        let marked_bg = theme::color(cx, "bg-current-line");
        let preview = match shown {
            Some(lines) => uniform_list(
                "bookmarks-preview",
                lines.len(),
                cx.processor(move |_, range: Range<usize>, _, _| {
                    range
                        .map(|index| {
                            div()
                                .id(index)
                                .h(line_height)
                                .w_full()
                                .flex()
                                .items_center()
                                .whitespace_nowrap()
                                .when(marked == Some(index), |el| el.bg(marked_bg))
                                .child(
                                    div()
                                        .flex_none()
                                        .w(px(52.0))
                                        .pr(px(12.0))
                                        .flex()
                                        .justify_end()
                                        .text_color(number_color)
                                        .child(SharedString::from((index + 1).to_string())),
                                )
                                .child(lines[index].clone())
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.preview_scroll)
            .size_full()
            .into_any_element(),
            None => div().size_full().into_any_element(),
        };
```

New lines:
(nothing)

**app-gpui/src/everywhere/bookmarks_popup.rs**, hunk 15 (replace): the pane holds the shared preview

Anchor (the lines just before):

```rust
                    .child(div().flex_none().w(px(LIST_WIDTH)).h_full().border_r_1().border_color(divider).child(list))
                    .child(
```

Old lines:

```rust
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_hidden()
                            .bg(theme::color(cx, "bg-editor"))
                            .font_family(self.code_font.clone())
                            .text_size(theme::length(cx, "fs-code"))
                            .text_color(theme::color(cx, "fg-editor"))
                            .child(preview),
```

New lines:

```rust
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_hidden()
                            .child(self.code_preview.clone()),
```

### Bookmarks (test replaced)

**app-gpui/src/everywhere/bookmarks_popup.rs**, hunk 16 (replace): its text injection (`preview_loaded`) no longer exists; the text an editor shows is proven by `workbench::tests::proofs26_code_preview::the_bookmarks_popup_previews_the_bookmarked_line_in_an_editor`

Anchor (the lines just before):

```rust
    }

```

Old lines:

```rust
    /// The text beside the list is the file of the selected bookmark, marked at its line; a folder has
    /// none, and text that arrives for a file the selection has left is dropped.
    #[gpui_kit::test]
    async fn the_text_beside_the_list_follows_the_selection(cx: &mut TestAppContext) {
        let (popup, _, cx) = open(cx, project());
        let preview = |cx: &mut VisualTestContext| {
            popup.read_with(cx, |popup, _| popup.preview_target().map(|(path, line)| (path.to_owned(), line)))
        };
        assert_eq!(preview(cx), None, "a folder has no text");
        cx.simulate_keystrokes("down");
        assert_eq!(preview(cx), Some(("/p/README.md".to_owned(), None)));
        cx.simulate_keystrokes("down");
        assert_eq!(preview(cx), Some(("/p/src/a.ts".to_owned(), Some(2))));
        assert!(popup.read_with(cx, |popup, _| popup.preview_text()).is_empty(), "nothing is read without a backend");

        popup.update(cx, |popup, cx| popup.preview_loaded("/p/README.md", "late", cx));
        assert!(popup.read_with(cx, |popup, _| popup.preview_text()).is_empty());
        popup.update(cx, |popup, cx| popup.preview_loaded("/p/src/a.ts", "one\n\ttwo\r\nalpha()\n", cx));
        assert_eq!(popup.read_with(cx, |popup, _| popup.preview_text()), ["one", "    two", "alpha()"]);
        cx.simulate_keystrokes("down");
        assert_eq!(preview(cx), Some(("/p/src/a.ts".to_owned(), Some(9))));
        assert_eq!(popup.read_with(cx, |popup, _| popup.preview_text()).len(), 3, "the same file stays shown");
        cx.simulate_keystrokes("down");
        assert_eq!(preview(cx), Some(("/p/src/b.ts".to_owned(), Some(4))));
        assert!(popup.read_with(cx, |popup, _| popup.preview_text()).is_empty());
    }
```

New lines:

```rust
    /// The text beside the list is the file of the selected bookmark, on its line; a folder has none.
    #[gpui_kit::test]
    async fn the_text_beside_the_list_follows_the_selection(cx: &mut TestAppContext) {
        let (popup, _, cx) = open(cx, project());
        let preview = |cx: &mut VisualTestContext| {
            popup.read_with(cx, |popup, _| popup.preview_target().map(|(path, line)| (path.to_owned(), line)))
        };
        assert_eq!(preview(cx), None, "a folder has no text");
        cx.simulate_keystrokes("down");
        assert_eq!(preview(cx), Some(("/p/README.md".to_owned(), None)));
        cx.simulate_keystrokes("down");
        assert_eq!(preview(cx), Some(("/p/src/a.ts".to_owned(), Some(2))));
        let text = |cx: &mut VisualTestContext| popup.read_with(cx, |popup, cx| popup.preview_text(cx));
        assert!(text(cx).is_empty(), "nothing is read without a backend");
        cx.simulate_keystrokes("down");
        assert_eq!(preview(cx), Some(("/p/src/a.ts".to_owned(), Some(9))));
        cx.simulate_keystrokes("down");
        assert_eq!(preview(cx), Some(("/p/src/b.ts".to_owned(), Some(4))));
        assert!(text(cx).is_empty());
    }
```

**app-gpui/src/workbench/mod.rs**, hunk 1 (replace): in `show_bookmarks`

Anchor (the lines just before):

```rust
        let events = cx.subscribe_in(&popup, window, |workbench, popup, event: &BookmarksPopupEvent, window, cx| {
            match event.clone() {
```

Old lines:

```rust
                BookmarksPopupEvent::Dismissed => workbench.close_popup(window, cx),
```

New lines:

```rust
                BookmarksPopupEvent::Dismissed => workbench.close_popup(window, cx),
                BookmarksPopupEvent::SaveFailed(failure) => workbench.report_save_failure(failure, cx),
```

### Usages

**app-gpui/src/workbench/code_insight.rs**, hunk 1 (replace)

Anchor (the lines just before):

```rust
    InspectionChange, InspectionCommand, SETTINGS_ACTION, inspection_settings_page, update_inspection,
};
```

Old lines:

```rust
use crate::platform_more::inspections::active_profile;
```

New lines:

```rust
use crate::platform_more::inspections::active_profile;
use crate::ui::code_preview::line_tokens::{self, LineTokens};
use crate::ui::code_preview::{CodePreview, PREVIEW_DEBOUNCE, PreviewHighlight, PreviewMode, PreviewTarget};
```

**app-gpui/src/workbench/code_insight.rs**, hunk 2 (replace): the two `usages_preview = None` resets stay as they are: they drop the pane's preview

Anchor (the lines just before):

```rust
    problems_preview: Option<Task<()>>,
    popup_preview: Option<Task<()>>,
```

Old lines:

```rust
    usages_preview: Option<Task<()>>,
```

New lines:

```rust
    /// The Find Usages window's preview pane, once a usage was previewed.
    usages_preview: Option<Entity<CodePreview>>,
```

**app-gpui/src/workbench/code_insight.rs**, hunk 3 (replace)

Anchor (the lines just before):

```rust
    }

```

Old lines:

```rust
    fn push_usages_tabs(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.usages_view() {
            view.update(cx, |view, cx| view.set_tabs(&self.code_insight.tabs, cx));
        }
    }
```

New lines:

```rust
    fn push_usages_tabs(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.usages_view() {
            view.update(cx, |view, cx| view.set_tabs(&self.code_insight.tabs, cx));
        }
        // The rows of the selected search are coloured once their files are parsed.
        let lines = self.code_insight.tabs.active().map(|search| line_tokens::group_sources(&search.groups));
        line_tokens::request(&self.label, lines.unwrap_or_default(), cx);
    }
```

**app-gpui/src/workbench/code_insight.rs**, hunk 4 (replace): in `reset_code_insight`

Anchor (the lines just before):

```rust
        insight.problems_preview = None;
        insight.popup_preview = None;
```

Old lines:

```rust
        insight.usages_preview = None;
```

New lines:

```rust
        insight.usages_preview = None;
        // The rows of another project are coloured from its own files.
        LineTokens::clear(cx);
```

**app-gpui/src/workbench/code_insight.rs**, hunk 5 (replace): the Show Usages popup's rows are coloured too (end of `carry_out_popup`)

Anchor (the lines just before):

```rust
            }
        }
```

Old lines:

```rust
        if let Some(view) = self.usages_popup() {
            let content = self.code_insight.popup.content().clone();
            view.update(cx, |view, cx| view.set_content(content, cx));
        }
```

New lines:

```rust
        if let Some(view) = self.usages_popup() {
            let content = self.code_insight.popup.content().clone();
            line_tokens::request(&self.label, line_tokens::group_sources(&content.groups), cx);
            view.update(cx, |view, cx| view.set_content(content, cx));
        }
```

**app-gpui/src/workbench/code_insight.rs**, hunk 6 (replace)

Anchor (the lines just before):

```rust
    }

```

Old lines:

```rust
    /// Shows the file of `usage` in the tool window's preview pane: a read-only editor with the
    /// caret on the usage.
    fn preview_usage(&mut self, usage: Usage, cx: &mut Context<Self>) {
        let (Some(view), Some(backend)) = (self.usages_view(), cx.try_global::<Backend>()) else { return };
        let label = self.label.clone();
        let path = PathBuf::from(&usage.path);
        let opened = {
            let label = label.clone();
            backend.with_host(move |host| async move { ide_host::ops::doc::open(&host, &label, path).await })
        };
        let task = cx.spawn(async move |workbench, cx| {
            let info = match opened.await {
                Ok(Ok(info)) => info,
                Ok(Err(error)) => return tracing::warn!("usages: cannot preview {}: {}", usage.path, error.message),
                Err(error) => return tracing::warn!("usages: cannot preview {}: {error}", usage.path),
            };
            // The window is gone when it was closed meanwhile.
            let _ = workbench.update(cx, |_, cx| {
                let host = cx.global::<Backend>().host.clone();
                let info = ide_workspace::DocumentInfo { read_only: true, ..info };
                let snapshot = host.project(&label).and_then(|project| project.documents.rope_snapshot(info.id));
                let (version, rope) = match snapshot {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        tracing::warn!("usages: cannot preview {}: {}", usage.path, error.message);
                        if let Err(error) = ide_host::ops::doc::close(&host, &label, info.id) {
                            tracing::debug!("usages: cannot close {}: {}", usage.path, error.message);
                        }
                        return;
                    }
                };
                // The editor holds the document it was opened for and closes it when it goes.
                let editor = cx.new(|cx| Editor::with_text(host, &label, &info, version, rope, cx));
                editor.update(cx, |editor, cx| {
                    let at = editor.offset_of(usage.line, usage.col);
                    editor.select(at, at, cx);
                    editor.scroll_to_line(usage.line.saturating_sub(5), cx);
                });
                view.update(cx, |view, cx| view.set_preview(&usage, editor.into(), cx));
            });
        });
        self.code_insight.usages_preview = Some(task);
    }
```

New lines:

```rust
    /// Shows the file of `usage` in the tool window's preview pane: a read-only editor on its document, on
    /// the usage's line, with every usage of the file marked.
    fn preview_usage(&mut self, usage: Usage, cx: &mut Context<Self>) {
        let Some(view) = self.usages_view() else { return };
        let label = self.label.clone();
        let preview = self.code_insight.usages_preview.get_or_insert_with(|| {
            let empty = "Select an item to preview";
            cx.new(|cx| CodePreview::new(&label, PreviewMode::ReadOnly, empty, PREVIEW_DEBOUNCE, cx))
        });
        let preview = preview.clone();
        let search = self.code_insight.tabs.active();
        let group = search.and_then(|search| search.groups.iter().find(|group| group.path == usage.path));
        let highlights = group
            .map(|group| {
                let marks = group.usages.iter();
                marks.map(|u| PreviewHighlight { line: u.line, ranges: vec![(u.col, u.end_col)] }).collect()
            })
            .unwrap_or_default();
        let target = PreviewTarget { path: usage.path.clone(), line: usage.line, highlights, version: 0 };
        preview.update(cx, |preview, cx| preview.show(Some(target), cx));
        view.update(cx, |view, cx| view.set_preview(&usage, preview.into(), cx));
    }
```

**app-gpui/src/code_insight/usages_view.rs**, hunk 1 (delete): only the old `marked_line` used it

Anchor (the lines just before):

```rust
};
use crate::find::store::StorageWrite;
```

Old lines:

```rust
use crate::find::utf16;
```

New lines:
(nothing)

**app-gpui/src/code_insight/usages_view.rs**, hunk 2 (replace)

Anchor (the lines just before):

```rust
use crate::platform::virtual_docs::{basename, relative_dir};
use crate::platform_more::occurrences::{Direction, OccurrenceStep};
```

Old lines:

```rust
use crate::theme;
```

New lines:

```rust
use crate::theme;
use crate::ui::code_preview::line_tokens::{LineTokens, usage_runs};
```

**app-gpui/src/code_insight/usages_view.rs**, hunk 3 (replace): in `UsagesView::new`

Anchor (the lines just before):

```rust
            cx,
        );
```

Old lines:

```rust
        cx.on_release(|view, cx| input::release_keys(view.key_claim, cx)).detach();
```

New lines:

```rust
        cx.on_release(|view, cx| input::release_keys(view.key_claim, cx)).detach();
        // The rows are drawn again once their files' colours are known.
        cx.observe_global::<LineTokens>(|_, cx| cx.notify()).detach();
```

**app-gpui/src/code_insight/usages_view.rs**, hunk 4 (replace)

Anchor (the lines just before):

```rust
}

```

Old lines:

```rust
/// A usage's line, trimmed, with the usage on the search-match colour.
fn marked_line(cx: &App, usage: &Usage) -> Div {
    let preview = trim_preview(usage);
    let parts = [
        (utf16::slice(&preview.text, 0, preview.from).to_owned(), false),
        (utf16::slice(&preview.text, preview.from, preview.to).to_owned(), true),
        (utf16::slice_from(&preview.text, preview.to).to_owned(), false),
    ];
    let match_bg = theme::color(cx, "bg-search-match");
    div().flex().min_w_0().overflow_hidden().whitespace_nowrap().children(
        parts.into_iter().filter(|(text, _)| !text.is_empty()).map(|(text, marked)| {
            let part = div().child(SharedString::from(text));
            if marked { part.bg(match_bg).rounded(px(2.0)) } else { part }
        }),
    )
}
```

New lines:

```rust
/// A usage's line, trimmed, in its file's colours, with the usage on the search-match colours.
fn marked_line(cx: &App, usage: &Usage) -> Div {
    let match_bg = theme::color(cx, "bg-search-match");
    div().flex().min_w_0().overflow_hidden().whitespace_nowrap().children(usage_runs(usage, cx).into_iter().map(
        |(text, color, marked)| {
            let part = div().text_color(color).child(SharedString::from(text));
            if marked { part.bg(match_bg).rounded(px(2.0)) } else { part }
        },
    ))
}
```

**app-gpui/src/code_insight/usages_view.rs**, hunk 5 (replace): declares the row-colour proofs in its `mod tests`

Anchor (the lines just before):

```rust
    use ide_lsp::types::References;
    use std::cell::RefCell;
```

Old lines:

```rust
    use std::path::PathBuf;
```

New lines:

```rust
    use std::path::PathBuf;

    mod proofs26_usage_preview;
```

**app-gpui/src/code_insight/usages_popup.rs**, hunk 1 (replace)

Anchor (the lines just before):

```rust
use super::usages::{
    AccessButton, ShowUsageResults, Usage, UsageAccessOptions, UsageScope, filter_usage_access, filter_usages,
```

Old lines:

```rust
    supports_usage_access, trim_preview, usage_count,
```

New lines:

```rust
    supports_usage_access, usage_count,
```

**app-gpui/src/code_insight/usages_popup.rs**, hunk 2 (replace)

Anchor (the lines just before):

```rust
use crate::platform::input;
use crate::platform::virtual_docs::basename;
```

Old lines:

```rust
use crate::theme;
```

New lines:

```rust
use crate::theme;
use crate::ui::code_preview::line_tokens::{LineTokens, usage_runs};
```

**app-gpui/src/code_insight/usages_popup.rs**, hunk 3 (replace): in `UsagesPopup::new`

Anchor (the lines just before):

```rust
            cx,
        );
```

Old lines:

```rust
        cx.on_release(|popup, cx| input::release_keys(popup.key_claim, cx)).detach();
```

New lines:

```rust
        cx.on_release(|popup, cx| input::release_keys(popup.key_claim, cx)).detach();
        // The rows are drawn again once their files' colours are known.
        cx.observe_global::<LineTokens>(|_, cx| cx.notify()).detach();
```

**app-gpui/src/code_insight/usages_popup.rs**, hunk 4 (replace)

Anchor (the lines just before):

```rust
}

```

Old lines:

```rust
/// A usage's line, trimmed, with the usage on the search-result colour.
fn marked_line(cx: &App, usage: &Usage) -> Div {
    let preview = trim_preview(usage);
    let parts = [
        (utf16::slice(&preview.text, 0, preview.from).to_owned(), false),
        (utf16::slice(&preview.text, preview.from, preview.to).to_owned(), true),
        (utf16::slice_from(&preview.text, preview.to).to_owned(), false),
    ];
    let match_bg = theme::color(cx, "bg-text-search-result");
    div().flex().min_w_0().overflow_hidden().whitespace_nowrap().children(
        parts.into_iter().filter(|(text, _)| !text.is_empty()).map(|(text, marked)| {
            let part = div().child(SharedString::from(text));
            if marked { part.bg(match_bg) } else { part }
        }),
    )
}
```

New lines:

```rust
/// A usage's line, trimmed, in its file's colours, with the usage on the search-result colour.
fn marked_line(cx: &App, usage: &Usage) -> Div {
    let match_bg = theme::color(cx, "bg-text-search-result");
    div().flex().min_w_0().overflow_hidden().whitespace_nowrap().children(usage_runs(usage, cx).into_iter().map(
        |(text, color, marked)| {
            let part = div().text_color(color).child(SharedString::from(text));
            if marked { part.bg(match_bg) } else { part }
        },
    ))
}
```

### Shared (unsaved previews)

**app-gpui/src/workbench/mod.rs**, hunk 2 (replace): `unsaved_count`: the question on closing the window counts the previews held with unsaved edits, each document once

Anchor (the lines just before):

```rust
    /// How many open documents have changes that are not on disk.
    pub fn unsaved_count(&self, cx: &App) -> usize {
```

Old lines:

```rust
        self.editors().iter().filter(|editor| editor.read(cx).is_dirty()).count()
```

New lines:

```rust
        let held = crate::ui::code_preview::held_editors(&self.label, cx);
        let dirty = self.editors().into_iter().chain(held).filter(|editor| editor.read(cx).is_dirty());
        // A document a tab and a held preview both show counts once.
        dirty.map(|editor| editor.read(cx).doc()).collect::<std::collections::HashSet<_>>().len()
```

**app-gpui/src/workbench/mod.rs**, hunk 3 (replace): `autosave`: autosave saves them too

Anchor (the lines just before):

```rust
        let editors = match editor {
            Some(editor) => vec![editor.clone()],
```

Old lines:

```rust
            None => self.editors(),
```

New lines:

```rust
            None => {
                let mut editors = self.editors();
                editors.extend(crate::ui::code_preview::held_editors(&self.label, cx));
                editors
            }
```

**app-gpui/src/workbench/mod.rs**, hunk 4 (replace): `save_all`: Save All saves them too

Anchor (the lines just before):

```rust
    fn save_all(&self, cx: &App) -> impl Future<Output = Result<(), String>> + 'static + use<> {
        let saving: Vec<_> = self
```

Old lines:

```rust
            .editors()
            .iter()
```

New lines:

```rust
            .editors()
            .iter()
            .chain(&crate::ui::code_preview::held_editors(&self.label, cx))
```

### Declarations

**app-gpui/src/workbench/mod.rs**, hunk 5 (replace): in `workbench::tests`; `proofs26_code_preview_pending.rs` stays undeclared

Anchor (the lines just before):

```rust
    mod proofs21_shell;
    mod proofs22_settings_units;
```

Old lines:

```rust
    mod proofs23_e2e_terminal_groups;
```

New lines:

```rust
    mod proofs23_e2e_terminal_groups;
    mod proofs26_code_preview;
```

## (b) Parity rows

Test paths: `ui::code_preview::proofs26_code_preview::…` (plain and view tests of the shared module), `workbench::tests::proofs26_code_preview::…` (end-to-end), `code_insight::usages_view::tests::proofs26_usage_preview::…` (the rows in the view), `workbench::tests::proofs26_code_preview_pending::…` (undeclared).

| Key                                                       | Expected                           | Proving tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | What the preview gap was / what stays missing                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| --------------------------------------------------------- | ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `unit:src/features/search/__tests__/codePreview.test.tsx` | partial, 4 of 8 cases (+1 in part) | 1 "marks matches (the current line's as current) and the current line": `ui::code_preview::proofs26_code_preview::marks_matches_the_current_lines_as_current_and_the_current_line`. 2 "shows the file editable in an editor view with the matches highlighted": `workbench::tests::proofs26_code_preview::shows_the_file_editable_in_an_editor_view_with_the_matches_highlighted`. 4 "keeps dirty text and undo when the preview closes and reopens without a tab": `workbench::tests::proofs26_code_preview::keeps_dirty_text_and_undo_when_the_preview_closes_and_reopens_without_a_tab`. 8 "Enter inserts a newline; editing keys stay out of the results; Escape bubbles": `workbench::tests::proofs26_code_preview::enter_inserts_a_newline_editing_keys_stay_out_of_the_results_escape_bubbles`. 7 "keeps %s documents read-only", in part (the `file` kind): `workbench::tests::proofs26_code_preview::keeps_file_documents_read_only` | Before: a plain line list. Missing: 3 "shares edits, document undo and save with a tab", 5 "hands undo history to a newly opened tab and back when the tab closes", 6 "tracks tab edits and history while the dirty preview is unmounted" need an undo history kept per document (each `Editor` has its own `History`); written in `proofs26_code_preview_pending.rs` as `shares_edits_document_undo_and_save_with_a_tab`, `hands_undo_history_to_a_newly_opened_tab_and_back_when_the_tab_closes`, `tracks_tab_edits_and_history_while_the_dirty_preview_is_unmounted`. Case 7: `library` (needs library roots), `archive` (needs a zip in the fixture), `binary` (Find in Files skips binary files, so no result reaches the preview) and `large` (a 5 MB file opens writable natively: `ide-workspace/src/documents.rs:399` makes only unobserved or unwritable documents read-only) are not proven |
| `unit:src/features/code-insight/usagePreview.test.tsx`    | done, 3 of 3                       | "colours the usage line with its file as context": `code_insight::usages_view::tests::proofs26_usage_preview::colours_the_usage_line_with_its_file_as_context`. "keeps comment colours outside the match and the search foreground inside it": `code_insight::usages_view::tests::proofs26_usage_preview::keeps_comment_colours_outside_the_match_and_the_search_foreground_inside_it`. "shows a plain count for unclassified searches after a Java access mode was selected": `code_insight::usages_view::tests::shows_a_plain_count_for_unclassified_searches_after_a_java_access_mode_was_selected` (exists)                                                                                                                                                                                                                                                                                                                               | Before: plain rows. Hypothesis: the two new ones count as done although they build the view with hand-filled searches and fill `LineTokens` with `LineTokens::store` (the spec mocks the file read the same way); the backend read itself (`line_tokens::request`) is not exercised by them                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `point.registerOverlay:search.findInFiles`                | partial                            | `shows_the_file_editable_in_an_editor_view_with_the_matches_highlighted`, `an_edit_in_the_preview_reaches_the_open_tab_and_the_save_shortcut_writes_it`, `a_result_passed_over_before_its_file_opens_is_dropped`, `enter_inserts_a_newline_editing_keys_stay_out_of_the_results_escape_bubbles` (all `workbench::tests::proofs26_code_preview::`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | The preview gap ("the preview as an editor on the document") is closed but for the shared undo. Other gaps stay: queries of several lines, syntax colours in result rows, resizing and the remembered size, the scope editor, opening to the side, the selection end of an opened match                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `action:find.inPath`                                      | partial                            | same as above (they run `find.inPath` through the registry)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | as above                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `action:find.replaceInPath`                               | partial                            | none new                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | The preview of Replace is the same `CodePreview`; no new test runs `find.replaceInPath`. Other gaps as for find.inPath. See (e): the existing Replace tests may now fail                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `point.registerOverlay:search.bookmarks`                  | done                               | `workbench::tests::proofs26_code_preview::the_bookmarks_popup_previews_the_bookmarked_line_in_an_editor`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | The proof's only "Missing" was the preview ("the file's plain text, not an editor on the document"); it is now an editor on the document. The shared undo between editors is counted on the codePreview unit row                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `action:navigate.showBookmarks`                           | done                               | same                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | The proof's only gap was the preview ("not a second editor on the document")                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `point.registerToolWindow:usages`                         | partial                            | `workbench::tests::proofs26_code_preview::the_usages_preview_marks_every_usage_of_the_file_read_only`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | It refers to `find.usages`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `action:find.usages`                                      | partial                            | same, plus the row-colour tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | "no preview colours" is closed (rows coloured, pane coloured by the editor). Stays: "the UI's own syntax fallback is not exercised"                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `feature:search`                                          | partial                            | all of the above                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | The preview editor exists now in Find in Files, Bookmarks and Find Usages; the Find tool window's preview (`FindToolWindow.tsx:504`) and Search Everywhere's (`SearchEverywhere.tsx:607`) still are not built on it, and the Structure and Bookmarks tool windows stay missing                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |

## (c) Behaviours of the webview's previews

`src/features/search/find/parts.tsx` (`CodePreview`, `previewDecorations`):

- 84-86: match, current match and active-line decorations. Built: `MarkKind::FindOccurrence`, `CurrentFindOccurrence`, new `CurrentLine`.
- 87, 176: read-only documents are not editable. Built: `DocumentInfo::read_only` makes the editor read-only.
- 90-103: the line marked when inside the text; columns clamped to the line; lines past the end skipped; ranges on the current line are current. Built: `preview_marks`. An empty current line gets no tint natively (`Editor::set_marks` drops empty ranges and `buffer::line_range` of an empty line is empty).
- 105-108: a second editor sharing the document and its save pipeline; read-only documents stay selectable. Built (shared document through `DocumentEditors`); the shared undo history (`sharedHistory: true`, 139) is not.
- 114-119, 208: `version` reopens the file. Built: `PreviewTarget::version` (Find in Files passes `preview_version`).
- 136, 197: 80 ms debounce. Built: `PREVIEW_DEBOUNCE`.
- 138, `preview.ts:9-24`: a dirty preview's attachment is held and taken again. Built: `HeldPreviews`, keyed by window label and path.
- `preview.ts:26-32`: held previews are released on save or close. Built in part: a held preview whose document was saved is dropped at the next `take_held`, not on the save itself.
- `preview.ts:34-38`: held previews are released when the project changes. Built differently: `take_held` drops a held editor whose document the window's project no longer has open (`documents.by_path(path) != editor.doc()`), and `held_editors` keeps counting it until then.
- 140-149, 198-207: a late answer releases its attachment. Built: a stale `doc::open` answer closes its document; a request overtaken during the debounce never opens it.
- 150-151: a failed save is notified ("app.unsaved.saveFailed", body the error). Built for Find in Files (`FindPopupEvent::Notify`) and Bookmarks (`BookmarksPopupEvent::SaveFailed` → `Workbench::report_save_failure`).
- 155: a held editor state is reused. Built: the held `Editor` is shown again.
- 162: the selection is drawn. Built (the editor's).
- 164-166: Mod-Z, Mod-Shift-Z, Mod-Y. Built: the editor handles ⌘Z/⌘⇧Z; `CodePreview::key_down` handles ⌘Y.
- 167-173: Mod-S saves. Built: `CodePreview::key_down`.
- 174: the editor's keymap without Escape. Built: the escape handler hands Escape to the owner before the editor collapses a selection.
- 182-188: keys bound to editor-text actions go to the preview. Built differently: `input::keep_keymap_out` keeps every IDE shortcut out while the preview editor has the keyboard (the webview keeps out only the `editorTextFocus` ones), so ⇧⌘F and the like do nothing there.
- 192-196, 228: the error text in place. Built: `CodePreview::note`.
- 211-221: a new file or line scrolls the line to the middle; new highlights only repaint. Built: `apply_target` (`revealed`), centred on the next frame (`scroll_caret_to_center`).
- 228: "Select a result to preview". Built.
- 230-236: `role="region"`, `aria-label="Preview"`, keys other than Escape stop at the preview. Keys built (the editor stops what it handles, the shield keeps the keymap out); the accessibility labels are left out (no accessibility tree in this crate).
- 238: the editor is hidden while it shows another file. Built (`current()` is none: the pane is blank).
- `src/editor/core/cm/preview.ts:30-41`: the editor's font, theme, line numbers, special characters, highlighting, no folding, selectable read-only text. Built through `Editor` (the user's font, size and line height; theme tokens), except "no folding": the editor's fold chevrons show in the preview.

`src/ui/SourcePreview.tsx` (Find Usages pane):

- 26-27: ±300 lines. Not needed: the editor holds the document and paints only the visible rows.
- 29-30, 95: no colours above 500 000 characters. Different: the editor drops highlighting for `large` documents (5 MB, `ide-vfs` `LARGE_FILE_THRESHOLD`).
- 66-84: debounced load, open buffer else disk. Built (80 ms; `doc::open` returns the open document).
- 147-152: "Select an item to preview". Built.
- 153: the error. Built.
- 189: selection kept inside the pane. Built (the editor's own selection and ⌘A).
- 192-196: centred line. Built.
- 198: one entry per line, the last usage of a line wins. Different: every usage is marked.
- 207, 53-56 of its CSS: current line background and the current line number in `fg-muted`. The tint is built; the line number's colour is not.
- CSS `.match` (accent-soft, fg-strong). Different: the editor's search-match colours (`bg-search-match`).

`src/features/code-insight/UsagesView.tsx`:

- 51-61 (`UsagePreview`): the trimmed line coloured with its file as context, the usage in a `mark` without token spans. Built: `line_tokens::usage_runs`, drawn by `marked_line` in `usages_view.rs` and `usages_popup.rs`.
- 405-427 (`UsagesPreview`): the usage, or a file's first usage, with every usage of the file. Built (`preview_usage`).

`src/editor/lineTokens.tsx`: 23-38 cache of the texts of 40 files (not needed: no text or parse is kept, only each asked line's tokens, until `LineTokens::clear` on a project change), 40-44 cleared for a new Find in Files search (the native Find result rows are not coloured, so nothing to clear), 55-58 and 86 parsed again when the open document changes (different: a line is known by its text too, so a changed line is coloured again, but an unchanged line whose context changed, say a comment opened above it, keeps its old colours), 46-47 and 109-116 whole-file parse up to 500 000 (built: `MAX_PARSE`), 80-107 the line alone when the file no longer holds it (built), 83 the open editor's text first (built: `Project::text_of`).

`src/features/code-insight/UsagesPopup.tsx:251-299` (`UsageCodePreview`): read-only editor, 60 ms, one mark and its line, centred. Left out: the native popup keeps its plain window of lines (`usages_popup.rs::preview_pane`); changing it means a new `PreviewWanted` payload (the usage, not the path) and a pane `AnyView`, which breaks the popup's existing tests. `POPUP_PREVIEW_DEBOUNCE` (60 ms) is in place for it.

`FindInFiles.tsx:825-837`: the diff in place of the preview in Replace's Preview mode. Built (unchanged native diff). `BookmarkPopups.tsx:277-279`: line `?? 0`, no highlights. Built (a bookmarked file is previewed on its first line, tinted, as the webview does).

## (d) Hypotheses (not settled by compiling)

Settled by reading, so not hypotheses: gpui's `simulate_input` types one keystroke per character (`gpui-pre-0.3.12/src/app/test_context.rs:639-650`), and continuous typing is one undo group (`editor/history.rs`, its test `continuous_typing_is_one_group_until_the_caret_moves_or_time_passes`), so the tests type without `\n` and press Enter apart; `FindPopup::new` searches its argument's query at once (`popup.sync_query(true, cx)`); `App::default_global` pushes `NotifyGlobalObservers` (gpui `app.rs:2566-2568`), so `LineTokens::store` redraws the rows; the IPC `doc_open` (`src-tauri/src/commands/doc.rs:17-18`) is the same `ide_host::ops::doc::open`, which marks the file recent.

1. `Editor::with_text` for a second editor of an open document behaves as for todo/usages previews: edits typed in the preview reach the tab through `DocumentEditors` (read in `editor/view.rs`), and the preview's `Editor` gets `DocDirty` events. Settle: `an_edit_in_the_preview_reaches_the_open_tab_and_the_save_shortcut_writes_it`.
2. `input::keep_keymap_out` lets keys reach the focused editor's `on_key_down` (read in `platform/input.rs`: the shield only skips the keymap). Settle: `enter_inserts_…`.
3. A bubbled ⌘S/⌘Y reaches `CodePreview`'s `on_key_down`: the editor returns from `key_down` without `stop_propagation` for them (read). Settle: the save test.
4. `cx.simulate_input` types into the preview editor once it is focused and painted inside the popup. Settle: the dirty-text test.
5. A click into the preview editor keeps the keyboard there inside the popups (the rules warn a parent `track_focus` can take it back). Not tested; check in the real window.
6. `window.on_next_frame` runs after the first frame laid the editor out, so `scroll_caret_to_center` has a height. Settle: look at the window; no test asserts the centred line.
7. The webview's preview marks the file recent too: its `attachDocument` calling `docOpen` was not read. Settle: read `src/editor/core/model/attachment.ts` and the backend of `DocumentManager.acquire`.
8. `Syntax::highlights` gives `/** … */` the `Comment` or `DocComment` kind for TypeScript, and `const` the `Keyword` kind. Settle: `ui::code_preview::proofs26_code_preview::a_line_inside_a_block_comment_is_coloured_as_a_comment` and the usage row tests.
9. `ide_host::ops::doc::open` starts no language server in the tests (Light mode, as the comment of `go_to_declaration_without_a_server_finds_a_local_through_the_syntax_tree` says). Settle: `window_lsp_report` in a failing wait lists servers.
10. `AnyView::downcast::<CodePreview>` works on the pane the workbench hands the usages view. Settle: the usages test.
11. The default keymap gives no other meaning to Enter, Backspace or Escape in the find popup when its preview has the keyboard (the shield makes the keymap irrelevant). Settle: `enter_inserts_…`.

## (e) Tests most likely to fail

1. Existing Replace tests that read the disk after Replace with the preview showing that file: `find::popup_view::tests::replace_replaces_the_selected_row_and_moves_on` (`project.read("a.txt")` at the lines quoting `"qux one\nfoo two foo\nbar\n"` and `"qux one\nqux two qux\nbar\n"`) and the neighbours that call `project.read`. Measured by reading `crates/ide-host/src/ops/search/commands.rs:15-44`: Replace saves only documents it opened itself; a document the preview holds open is replaced in place and left unsaved. The webview has the same flow (its preview attaches the document too). Fix either way and say which: read the document's text in those tests (`documents.text_of`), or have Replace All save documents only a preview holds.
2. `a_result_passed_over_before_its_file_opens_is_dropped`: it relies on the search ending before the first preview's 80 ms are over on the virtual clock; `searched` advances 150 ms per round, so the first file may already be open. If it flakes, drive the selection before the search's last round, or drop it.
3. `keeps_dirty_text_and_undo_when_the_preview_closes_and_reopens_without_a_tab`: needs the popup's release to release its `CodePreview` (so `on_release` holds the editor); it turns autosave off first, as the spec does.
4. `shows_the_file_editable_…`: `painted_color_at(1, 0)` waits on the editor's own highlights (`doc_highlights` off-thread) and on `painted_marks(CurrentLine)` (needs Shared hunk 5).
5. `the_bookmarks_popup_previews_the_bookmarked_line_in_an_editor`: the row order of the popup (it walks down at most four rows to the bookmark).
6. `everywhere::bookmarks_popup::tests::the_text_beside_the_list_follows_the_selection` is replaced by a Bookmarks hunk (its text injection is gone); the other popup view tests run without a backend and now show a blank pane where they showed nothing either.

## (f) What a look at the real window must check

- Find in Files and Bookmarks: the preview is in the editor's font and size from Settings › Editor, with line numbers, keyword colours, the selected line tinted in light and dark themes, current matches outlined, the line centred; typing edits the file and the tab shows it; ⌘S saves; Escape closes the popup; a click places the caret and keeps the keyboard in the preview.
- The tint when the preview is focused: the editor also paints `bg-current-line` on the caret line, so the tinted line may look darker than unfocused (two layers of the same colour).
- Fold chevrons in the preview gutter (the webview has none).
- Find Usages: the pane is read-only, every usage of the file marked, the usage's line tinted; the rows are coloured (keywords, comments), the usage in the search colours, struck rows still dimmed.
- Show Usages popup rows coloured the same way.
- A large file (over 5 MB) previews without colours and without freezing.

## (g) Found in the existing code and in the instructions

- The instructions call the preview read-only; the webview's Find in Files and Bookmarks preview is an editable second editor (`parts.tsx:105-108`, spec cases 2-6 and 8). Only the Find Usages pane and the Show Usages popup's preview are read-only.
- `src/platform/locales/text/en.json` has neither "Select a result to preview" nor "Select an item to preview" (the other catalogs do); the English text is the key, so `I18n::tx` shows it as written.
- `workbench/code_insight.rs::preview_usage` (replaced by the Usages hunk) and `workbench/todo_window.rs::preview_todo` (unchanged) drop their task when a newer selection replaces it, but `Backend::call` spawns the `doc::open` on the backend runtime, which runs on: the document reference that answer opened is never closed. `CodePreview::open` closes it. The TODO pane is a candidate for `CodePreview` (`PreviewMode::ReadOnly`).
- Native autosave, Save All and the unsaved count cover only tab editors (see the head of (a)).
- Replace in Files leaves documents that are open (a preview counts) replaced and unsaved (`ops/search/commands.rs:15-44`); see (e) 1.
