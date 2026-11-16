//! The workbench's editor features that fall back to the document's syntax tree when the language
//! server or the backend's fold ranges have no answer: Context Info from the tree's blocks, and
//! Highlight Usages in File from the occurrences of a local in its scope.

use super::Workbench;
use super::editor_extras::NO_CONTEXT_INFO;
use super::editor_language::{This, call};
use crate::backend::Backend;
use crate::editor::structure::enclosing_headers;
use crate::editor::view::Editor;
use crate::platform::i18n::Localization;
use gpui_kit::{AsyncWindowContext, Context, Entity, WeakEntity};
use ide_core::{DocId, IdeError, IdeResult, Pos, Range};
use ide_syntax::{Locals, NodeAt};

/// What the syntax tree says at a line's start: `None` when the language has no grammar.
type LineNode = IdeResult<Option<NodeAt>>;

impl Workbench {
    /// View › Context Info without the backend's fold ranges: the header lines of the syntax tree's
    /// blocks around the caret's line, as the hint, once the backend answered. Answers whether the
    /// backend was asked; without a backend the caller says there is no context.
    pub(super) fn context_info_from_tree(&mut self, editor: &Entity<Editor>, max: u32, cx: &mut Context<Self>) -> bool {
        if !cx.has_global::<Backend>() {
            return false;
        }
        let (doc, language, version, line) = {
            let e = editor.read(cx);
            (e.doc(), e.language().clone(), e.version(), e.caret_position().0)
        };
        let label = self.label.clone();
        let work = cx.global::<Backend>().with_host(move |host| async move {
            if !ide_host::ops::syntax::syntax_support(&host, &label, language)?.tree {
                return Ok(None);
            }
            let start = Pos::new(line, 0);
            let range = Range { start, end: start };
            ide_host::ops::syntax::syntax_node_at(&host, &label, doc, range, None).await
        });
        let editor = editor.downgrade();
        cx.spawn(async move |this: WeakEntity<Workbench>, cx| {
            let answer: LineNode = work.await.unwrap_or_else(|error| Err(IdeError::internal(error.to_string())));
            let shown = this.update(cx, |workbench, cx| {
                workbench.context_info_answer(&editor, version, line, max, answer, cx);
            });
            if let Err(error) = shown {
                tracing::debug!("Context Info ended with its window: {error:#}");
            }
        })
        .detach();
        true
    }

    fn context_info_answer(
        &mut self,
        editor: &WeakEntity<Editor>,
        version: u32,
        line: u32,
        max: u32,
        answer: LineNode,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = editor.upgrade() else { return };
        // The caret or the text moved while the tree was read: its lines no longer apply.
        let moved = {
            let e = editor.read(cx);
            e.version() != version || e.caret_position().0 != line
        };
        if moved {
            return;
        }
        let at = answer.unwrap_or_else(|error| {
            tracing::debug!("no syntax tree for Context Info: {}", error.message);
            None
        });
        let text = {
            let e = editor.read(cx);
            let state = e.edit_state();
            let headers = enclosing_headers(&state, line, at.as_ref(), max);
            headers.into_iter().map(|header| e.line_text(header).trim_end().to_owned()).collect::<Vec<_>>().join("\n")
        };
        let text = if text.is_empty() { cx.default_global::<Localization>().0.tx(NO_CONTEXT_INFO) } else { text };
        self.show_hint(text, cx);
    }
}

/// Highlight Usages in File without a usable language server: the definition and the references
/// of the name at `pos` when the syntax tree resolves it to a local with certainty, in document
/// order. `None` when the language has no scope analysis or the name is not certainly a local:
/// anything wider is too noisy to guess.
pub(super) async fn local_usages(
    this: &This,
    editor: &Entity<Editor>,
    doc: DocId,
    pos: Pos,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<Option<Vec<Range>>> {
    let language = this.update(cx, |_, cx| editor.read(cx).language().clone())?;
    let answer = call(this, cx, move |host, label| async move {
        if !ide_host::ops::syntax::syntax_support(&host, &label, language)?.locals {
            return Ok(None);
        }
        ide_host::ops::syntax::syntax_locals(&host, &label, doc, pos, None).await.map(Some)
    })
    .await?;
    let symbol = match answer {
        Ok(Some(Locals::Local { symbol })) if symbol.certain => symbol,
        Ok(_) => return Ok(None),
        Err(error) => {
            tracing::debug!("no syntax fallback for the usages: {}", error.message);
            return Ok(None);
        }
    };
    let mut ranges = vec![symbol.definition];
    ranges.extend(symbol.references);
    ranges.sort_by_key(|range| range.start);
    Ok(Some(ranges))
}
