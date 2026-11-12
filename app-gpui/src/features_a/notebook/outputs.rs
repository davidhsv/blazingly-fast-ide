//! Kernel output events applied to the notebook as shown while a batch runs. Transient protocol
//! state (a `clear_output(wait=True)` waiting for the output that consumes it) belongs to an
//! execution, never to the persisted MIME bundles; when local reduction cannot be exact (a gap in
//! revisions, a display update of another cell, a lost event) the snapshot is left alone and the
//! caller fetches the authoritative one.

use ide_host::notebook::NotebookOutputEvent;
use ide_notebook::NotebookSnapshot;
use serde_json::{Map, Value};
use std::collections::HashSet;

/// `value` as an object; anything else is an empty one.
pub fn object(value: &Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

/// The text of a MIME value: a string, or the strings of a list joined.
pub fn mime_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    }
}

const OUTPUT_KINDS: [&str; 4] = ["stream", "display_data", "execute_result", "error"];

/// What an event did to the snapshot.
#[derive(Clone, Debug)]
pub struct Reduction {
    /// The new snapshot; `None` when it is unchanged.
    pub snapshot: Option<NotebookSnapshot>,
    /// Fetch the authoritative snapshot.
    pub refresh: bool,
}

impl Reduction {
    fn unchanged(refresh: bool) -> Self {
        Self { snapshot: None, refresh }
    }
}

/// Applies the output events of one execution.
#[derive(Clone, Debug, Default)]
pub struct OutputReducer {
    waiting: HashSet<String>,
    recovering: bool,
}

impl OutputReducer {
    pub fn reset(&mut self) {
        self.waiting.clear();
        self.recovering = false;
    }

    /// An event was lost: it may have hidden a transient wait-clear, not just a revision.
    pub fn recover(&mut self) {
        self.recovering = true;
    }

    pub fn reduce(&mut self, snapshot: &NotebookSnapshot, event: &NotebookOutputEvent) -> Reduction {
        if event.id != snapshot.id {
            return Reduction::unchanged(false);
        }
        let content = object(&event.content);
        let wait = content.get("wait") == Some(&Value::Bool(true));
        let clear = event.kind == "clear_output";
        // A source acknowledgement may overtake a transient backend wait-clear.
        if clear && wait {
            self.waiting.insert(event.cell.clone());
        }
        if event.version <= snapshot.version {
            return Reduction::unchanged(false);
        }
        let produces_output = OUTPUT_KINDS.contains(&event.kind.as_str()) || event.kind == "update_display_data";
        // A full snapshot may already include the output that consumed this clear: reconcile
        // instead of clearing locally a second time.
        let deferred_clear = produces_output && self.waiting.remove(&event.cell);
        let refresh = self.recovering
            || deferred_clear
            || event.kind == "update_display_data"
            || event.version > snapshot.version + 1
            || event.changed_cells.iter().any(|id| *id != event.cell);
        if refresh {
            if clear && wait {
                self.waiting.insert(event.cell.clone());
            } else if produces_output || clear {
                self.waiting.remove(&event.cell);
            }
            return Reduction::unchanged(true);
        }
        let mut next = snapshot.clone();
        for cell in next.cells.iter_mut().filter(|cell| cell.id == event.cell) {
            if clear {
                if wait {
                    self.waiting.insert(cell.id.clone());
                } else {
                    self.waiting.remove(&cell.id);
                    cell.outputs.clear();
                }
            } else if event.kind == "execute_input" {
                cell.execution_count =
                    content.get("execution_count").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
            } else if OUTPUT_KINDS.contains(&event.kind.as_str()) {
                // Transient keys stay out of the shown copy too; persistence is the backend's.
                let mut body = content.clone();
                body.remove("transient");
                let last = cell.outputs.last().map(object).unwrap_or_default();
                let same_stream = event.kind == "stream"
                    && last.get("output_type").and_then(Value::as_str) == Some("stream")
                    && last.get("name") == body.get("name");
                if same_stream {
                    let mut merged = last.clone();
                    let text = mime_text(last.get("text")) + &mime_text(body.get("text"));
                    merged.insert("text".to_owned(), Value::String(text));
                    cell.outputs.pop();
                    cell.outputs.push(Value::Object(merged));
                } else {
                    body.insert("output_type".to_owned(), Value::String(event.kind.clone()));
                    cell.outputs.push(Value::Object(body));
                }
            }
        }
        next.version = event.version;
        next.dirty = event.dirty;
        Reduction { snapshot: Some(next), refresh: false }
    }
}

#[cfg(test)]
pub(super) mod fixtures {
    use ide_core::IpcPath;
    use ide_host::notebook::NotebookOutputEvent;
    use ide_notebook::{NotebookCell, NotebookSnapshot};
    use serde_json::{Value, json};
    use std::path::PathBuf;

    pub fn notebook(source: &str, outputs: Vec<Value>, execution_count: Option<u32>) -> NotebookSnapshot {
        NotebookSnapshot {
            id: "document-a".to_owned(),
            path: IpcPath(PathBuf::from("/p/a.ipynb")),
            version: 1,
            dirty: false,
            can_undo: false,
            can_redo: false,
            nbformat: 4,
            nbformat_minor: 5,
            metadata: json!({}),
            cells: vec![NotebookCell {
                id: "cell-a".to_owned(),
                cell_type: "code".to_owned(),
                source: source.to_owned(),
                metadata: json!({}),
                outputs,
                execution_count,
                attachments: json!({}),
            }],
        }
    }

    pub fn event(kind: &str, version: u32, content: Value) -> NotebookOutputEvent {
        NotebookOutputEvent {
            id: "document-a".to_owned(),
            path: IpcPath(PathBuf::from("/p/a.ipynb")),
            cell: "cell-a".to_owned(),
            msg_id: "request-a".to_owned(),
            kind: kind.to_owned(),
            content,
            version,
            dirty: true,
            changed_cells: vec!["cell-a".to_owned()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{event, notebook};
    use super::*;
    use serde_json::json;

    fn stream(name: &str, text: &str) -> Value {
        json!({ "output_type": "stream", "name": name, "text": text })
    }

    fn original() -> NotebookSnapshot {
        notebook("print('old')", vec![stream("stdout", "old\n")], Some(1))
    }

    fn with_outputs(base: &NotebookSnapshot, version: u32, outputs: Vec<Value>) -> NotebookSnapshot {
        let mut next = base.clone();
        next.version = version;
        next.cells[0].outputs = outputs;
        next
    }

    fn reduce(reducer: &mut OutputReducer, state: &NotebookSnapshot, e: &NotebookOutputEvent) -> NotebookSnapshot {
        reducer.reduce(state, e).snapshot.unwrap_or_else(|| state.clone())
    }

    #[test]
    fn defers_wait_clear_without_changing_bytes_and_reconciles_its_consuming_output_exactly_once() {
        let mut reducer = OutputReducer::default();
        let original = original();
        let waiting = reduce(&mut reducer, &original, &event("clear_output", 1, json!({ "wait": true })));
        assert_eq!(waiting.cells[0].outputs, original.cells[0].outputs);
        let consumed = reducer.reduce(&waiting, &event("stream", 2, json!({ "name": "stdout", "text": "new" })));
        assert!(consumed.refresh);
        assert!(consumed.snapshot.is_none());
        let mut recovered = with_outputs(&original, 2, vec![stream("stdout", "new")]);
        recovered.dirty = true;
        let more = event("stream", 3, json!({ "name": "stdout", "text": " output\n" }));
        let second = reduce(&mut reducer, &recovered, &more);
        assert_eq!(second.cells[0].outputs, [stream("stdout", "new output\n")]);
        assert!(second.dirty);
    }

    #[test]
    fn coalesces_only_adjacent_streams_of_the_same_name_preserving_stderr_order() {
        let mut reducer = OutputReducer::default();
        let mut state = original();
        state = reduce(&mut reducer, &state, &event("clear_output", 2, json!({ "wait": false })));
        state = reduce(&mut reducer, &state, &event("stream", 3, json!({ "name": "stderr", "text": "warning\n" })));
        state = reduce(&mut reducer, &state, &event("stream", 4, json!({ "name": "stdout", "text": "result\n" })));
        assert_eq!(state.cells[0].outputs, [stream("stderr", "warning\n"), stream("stdout", "result\n")]);
    }

    #[test]
    fn requires_authoritative_reconciliation_for_cross_cell_displays_without_duplicating_an_output() {
        let mut reducer = OutputReducer::default();
        let state = original();
        let mut update = event(
            "update_display_data",
            2,
            json!({ "data": { "text/plain": "updated" }, "transient": { "display_id": "display-a" } }),
        );
        update.changed_cells = vec!["cell-a".to_owned(), "earlier-cell".to_owned()];
        let result = reducer.reduce(&state, &update);
        assert!(result.refresh);
        assert!(result.snapshot.is_none());
    }

    #[test]
    fn does_not_clear_twice_after_recovering_a_stream_revision_gap() {
        let mut reducer = OutputReducer::default();
        let state = original();
        reducer.reduce(&state, &event("clear_output", 1, json!({ "wait": true })));
        let gap = event("stream", 4, json!({ "name": "stdout", "text": "recovered" }));
        assert!(reducer.reduce(&state, &gap).refresh);
        let state = with_outputs(&state, 4, vec![stream("stdout", "recovered")]);
        let state = reduce(&mut reducer, &state, &event("stream", 5, json!({ "name": "stdout", "text": "+next" })));
        assert_eq!(state.cells[0].outputs, [stream("stdout", "recovered+next")]);
    }

    #[test]
    fn does_not_erase_already_reconciled_output_when_a_snapshot_overtakes_a_deferred_clear() {
        let mut reducer = OutputReducer::default();
        let original = original();
        reducer.reduce(&original, &event("clear_output", 1, json!({ "wait": true })));
        let recovered = with_outputs(&original, 2, vec![stream("stdout", "new")]);
        let next = reducer.reduce(&recovered, &event("stream", 3, json!({ "name": "stdout", "text": "+next" })));
        assert!(next.refresh);
        assert!(next.snapshot.is_none());
    }

    #[test]
    fn repairs_an_overtaken_clear_and_a_lost_transient_clear_rather_than_appending_to_old_output() {
        let mut reducer = OutputReducer::default();
        let mut acknowledged = original();
        acknowledged.version = 2;
        reducer.reduce(&acknowledged, &event("clear_output", 1, json!({ "wait": true })));
        let replacement = event("stream", 3, json!({ "name": "stdout", "text": "replacement" }));
        assert!(reducer.reduce(&acknowledged, &replacement).refresh);
        reducer.reset();
        reducer.recover();
        assert!(reducer.reduce(&acknowledged, &replacement).refresh);
    }

    #[test]
    fn changes_only_the_execution_count_on_execute_input_matching_the_backend_commit() {
        let original = original();
        let input = event("execute_input", 2, json!({ "execution_count": 2 }));
        let next = reduce(&mut OutputReducer::default(), &original, &input);
        assert_eq!(next.cells[0].execution_count, Some(2));
        assert_eq!(next.cells[0].outputs, original.cells[0].outputs);
    }

    #[test]
    fn drops_old_incarnations_and_duplicate_revisions_and_removes_transient_display_ids() {
        let mut reducer = OutputReducer::default();
        let state = original();
        let mut wrong = event("stream", 2, json!({ "text": "stale" }));
        wrong.id = "old-document".to_owned();
        let stale = reducer.reduce(&state, &wrong);
        assert!(stale.snapshot.is_none() && !stale.refresh);
        let display = event(
            "display_data",
            2,
            json!({ "data": { "text/plain": "hello" }, "metadata": {}, "transient": { "display_id": "temporary" } }),
        );
        let result = reduce(&mut reducer, &state, &display);
        assert_eq!(
            result.cells[0].outputs.last(),
            Some(&json!({ "output_type": "display_data", "data": { "text/plain": "hello" }, "metadata": {} }))
        );
        assert!(reducer.reduce(&result, &display).snapshot.is_none());
    }
}
