//! What a save cleans up before the text is written (Editor › General › Save Files, overridden by the
//! file's `.editorconfig` while its support is on): trailing spaces and tabs on the edited lines or on
//! all of them, but not under a caret of the document's editors when the caret line is kept, blank
//! lines at the end, and a missing final line break. The cleanup is one edit and one undo step of the
//! editor that saves; a code preview of the document leaves it to an editor tab of the document.

use super::*;
use crate::editor_model::document::{CleanupOptions, save_cleanup_changes};
use std::collections::BTreeSet;

impl Editor {
    /// Whether a save of the document cleans it up through this editor (false for a preview beside a
    /// list of results, whose carets keep no trailing spaces either).
    pub fn set_cleans_up_on_save(&mut self, cleans: bool) {
        self.cleans_on_save = cleans;
    }

    /// Saves the document `editor` shows: cleaned up first, through `editor` or, for a preview, through
    /// an editor tab of the document if one is open; then written by the backend, which reports the
    /// document clean when it is done.
    pub fn save_document(
        editor: &Entity<Editor>,
        cx: &mut App,
    ) -> impl Future<Output = anyhow::Result<IdeResult<()>>> + Send + 'static + use<> {
        let cleaner = if editor.read(cx).cleans_on_save {
            Some(editor.clone())
        } else {
            let doc = editor.read(cx).doc;
            document_editors(doc, cx).into_iter().find(|other| other.read(cx).cleans_on_save)
        };
        if let Some(cleaner) = cleaner {
            cleaner.update(cx, |cleaner, cx| cleaner.clean_up_before_save(cx));
        }
        editor.read(cx).write_as_is(cx)
    }

    /// Applies the cleanup to an unsaved, writable document. When the backend has moved on since the
    /// text was read, the text is refreshed and the cleanup computed again.
    fn clean_up_before_save(&mut self, cx: &mut Context<Self>) {
        if !self.dirty || self.read_only {
            return;
        }
        for _ in 0..2 {
            let changes = self.cleanup_changes(cx.entity_id(), cx);
            if changes.is_empty() {
                return;
            }
            // A caret maps to the side before what is inserted at it: one at the end stays in front of
            // the added line break.
            let selections: Vec<Selection> = self
                .selections
                .iter()
                .map(|&(anchor, head)| {
                    (
                        buffer::map_offset(anchor, &changes, Assoc::Before),
                        buffer::map_offset(head, &changes, Assoc::Before),
                    )
                })
                .collect();
            let applied =
                self.documents().and_then(|d| d.edit(self.doc, self.version, ChangeSet { changes: changes.clone() }));
            match applied {
                Ok(_) => {
                    self.dirty = true;
                    let base = self.version;
                    let main = self.main;
                    self.follow(
                        Some(&changes),
                        Some(EditKind::Other),
                        Some((selections, main)),
                        EditOrigin::SaveCleanup,
                        cx,
                    );
                    self.modified.add(&changes);
                    self.tell_other_editors(base, &changes, cx);
                    return;
                }
                Err(error) if error.kind == ErrorKind::OutOfSync => {
                    let followed = self.follow(None, Some(EditKind::Other), None, EditOrigin::External, cx);
                    self.modified.map(&followed);
                }
                Err(error) => {
                    tracing::warn!("cannot clean {} up before saving it: {}", self.path.display(), error.message);
                    return;
                }
            }
        }
    }

    /// The changes the save options ask for, given the ranges edited in every editor of the document
    /// since the last save and the carets of its editors that clean up.
    fn cleanup_changes(&self, this: gpui_kit::EntityId, cx: &App) -> Vec<TextChange> {
        let settings = ide_host::ops::project::settings(&self.host, &self.window_label);
        let general = &settings.editor.general;
        let editorconfig = self.editorconfig.as_ref().filter(|_| settings.editor.code_style.editorconfig);
        let mut modified = self.modified.clone();
        let mut protect_lines = BTreeSet::new();
        let mut caret_lines = |editor: &Editor| {
            for &(_, head) in &editor.selections {
                protect_lines.insert(buffer::line_col(&self.rope, head.min(buffer::len_utf16(&self.rope))).0);
            }
        };
        caret_lines(self);
        for other in document_editors(self.doc, cx) {
            if other.entity_id() == this {
                continue;
            }
            let other = other.read(cx);
            // An editor that has not caught up yet holds offsets of another text.
            if other.version != self.version {
                continue;
            }
            modified.all |= other.modified.all;
            modified.ranges.extend_from_slice(&other.modified.ranges);
            if other.cleans_on_save {
                caret_lines(other);
            }
        }
        let opts = CleanupOptions {
            trim: editorconfig
                .and_then(|ec| ec.trim_trailing_whitespace)
                .unwrap_or(settings.files.trim_trailing_whitespace),
            final_newline: editorconfig
                .and_then(|ec| ec.insert_final_newline)
                .unwrap_or(settings.files.ensure_final_newline),
            protect_lines: if general.keep_trailing_on_caret_line { protect_lines } else { BTreeSet::new() },
            all_lines: general.strip_trailing_scope == "all",
            trailing_blank_lines: general.remove_trailing_blank_lines,
        };
        save_cleanup_changes(&self.rope, &modified, &opts)
    }
}

/// The editors that show `doc`.
fn document_editors(doc: DocId, cx: &App) -> Vec<Entity<Editor>> {
    cx.try_global::<DocumentEditors>()
        .and_then(|shown| shown.0.get(&doc))
        .into_iter()
        .flatten()
        .filter_map(WeakEntity::upgrade)
        .collect()
}
