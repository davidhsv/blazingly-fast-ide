//! An editor of an untitled buffer, and what changes for its document: Save As writes the buffer to
//! a file the user picked and every editor of it takes that file's path, language, encoding and
//! `.editorconfig`, keeping its text, carets and undo history (the document stays the same one in the
//! backend). Also the byte order mark of a UTF-8 document, which the encoding item's menu adds or
//! removes and the next save writes.

use super::save_cleanup::document_editors;
use super::*;
use crate::editor_model::untitled::is_untitled_path;

impl Editor {
    /// Whether the document is an untitled buffer: no file behind it until Save As gives it one.
    pub fn is_untitled(&self) -> bool {
        is_untitled_path(&self.path.to_string_lossy())
    }

    /// Whether Reload with Encoding can read the document's source again: false for an untitled
    /// buffer and for content a language server generated.
    pub fn can_reload_encoding(&self) -> bool {
        self.can_reload_encoding
    }

    /// The save cleanup of the document `editor` shows, through `editor` or another editor of the
    /// document that cleans up: an untitled buffer's explicit save runs it before the save dialog
    /// opens, as any save runs it before writing, so a cancelled dialog leaves the buffer cleaned.
    pub fn clean_up_document(editor: &Entity<Editor>, cx: &mut App) {
        let doc = editor.read(cx).doc;
        let cleaner = if editor.read(cx).cleans_on_save {
            Some(editor.clone())
        } else {
            document_editors(doc, cx).into_iter().find(|other| other.read(cx).cleans_on_save)
        };
        if let Some(cleaner) = cleaner {
            cleaner.update(cx, |cleaner, cx| cleaner.clean_up_before_save(cx));
        }
    }

    /// Writes the untitled buffer `editor` shows to `path` (Save As) through the backend, which
    /// creates missing folders, re-keys the document and detects its language from the new path.
    /// The answer is the document as the file it now is; [`Editor::retarget_document`] gives it to
    /// the editors.
    pub fn save_document_as(
        editor: &Entity<Editor>,
        path: PathBuf,
        cx: &mut App,
    ) -> impl Future<Output = anyhow::Result<IdeResult<DocumentInfo>>> + Send + 'static + use<> {
        let doc = editor.read(cx).doc;
        let (host, window) = (editor.read(cx).host.clone(), editor.read(cx).window_label.clone());
        cx.global::<Backend>().call(async move { ide_host::ops::doc::save_as(&host, &window, doc, path).await })
    }

    /// Save As made document `doc` the file `info` describes: every editor of it takes the file's
    /// path and what comes with it, and is clean.
    pub fn retarget_document(doc: DocId, info: &DocumentInfo, cx: &mut App) {
        for editor in document_editors(doc, cx) {
            editor.update(cx, |editor, cx| editor.retarget(info, cx));
        }
    }

    fn retarget(&mut self, info: &DocumentInfo, cx: &mut Context<Self>) {
        self.path = PathBuf::from(info.path.clone());
        self.read_only = info.read_only;
        self.large = info.large;
        self.line_ending = info.line_ending;
        (self.encoding, self.has_bom) = (info.encoding.clone(), info.has_bom);
        self.editorconfig = info.editorconfig.clone();
        self.can_reload_encoding = info.can_reload_encoding;
        self.dirty = info.dirty;
        if !self.dirty {
            self.modified.clear();
        }
        // The backend's own language event may come before or after this; a language already taken
        // changes nothing.
        self.change_language(info.language.clone(), cx);
        self.read_facts();
        cx.notify();
    }

    /// Adds or removes the byte order mark of a UTF-8 document; the document then has unsaved
    /// changes and the next save writes the mark as chosen. The backend refuses it for a read-only
    /// document and for any encoding but UTF-8. The task ends once the editor shows the answer.
    pub fn set_bom(&mut self, has_bom: bool, cx: &mut Context<Self>) -> Task<IdeResult<()>> {
        let (host, window, doc) = (self.host.clone(), self.window_label.clone(), self.doc);
        let changed = cx
            .global::<Backend>()
            .call(async move { ide_host::ops::doc::doc_set_bom(&host, &window, doc, has_bom).await });
        cx.spawn(async move |this, cx| {
            let answer = changed.await.unwrap_or_else(|error| Err(IdeError::internal(format!("{error:#}"))))?;
            this.update(cx, |editor, cx| {
                (editor.encoding, editor.has_bom) = (answer.encoding, answer.has_bom);
                editor.dirty = true;
                cx.notify();
            })
            .map_err(|error| IdeError::internal(error.to_string()))
        })
    }
}
