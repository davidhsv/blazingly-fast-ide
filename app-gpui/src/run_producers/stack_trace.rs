//! Code › Analyze Stack Trace or Thread Dump…: a pasted trace (prefilled from the clipboard) is browsed
//! in a console whose frames link to the sources, using the run console's link parser. Like IDEA, each
//! trace is a tab of the Run tool window, next to the run sessions.

use crate::find::utf16;
use crate::run::console::{ChunkKind, ConsoleModel, ConsoleOptions};
use crate::run::links::find_output_links;
use crate::run::store::RunStore;

/// A trace's tab in the Run tool window.
#[derive(Clone, Debug)]
pub struct StackTraceTab {
    pub id: String,
    pub title: String,
    pub model: ConsoleModel,
}

/// The analyzed traces, and the dialog a new one is pasted into.
#[derive(Clone, Debug, Default)]
pub struct StackTraces {
    pub tabs: Vec<StackTraceTab>,
    /// Text of the open "Analyze Stack Trace" dialog, or `None` when closed.
    pub dialog: Option<String>,
    opened: u64,
}

/// Normalises pasted text: line endings, trailing blanks, surrounding empty lines.
pub fn normalize_trace(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.split('\n').map(str::trim_end).collect();
    lines.join("\n").trim_matches('\n').to_string()
}

pattern!(TRACE_HEAD, r"(Exception|Error|panicked|Traceback|^Full thread dump)");
pattern!(FRAME, r"^at\s");
pattern!(
    EXCEPTION,
    r#"^(?:Exception in thread "[^"]*"\s+)?(?:[A-Za-z0-9_$]+\.)*([A-Za-z0-9_$]+(?:Exception|Error))(?-u:\b)"#
);

/// Tab title: the exception (or first) line, shortened.
pub fn trace_title(text: &str) -> String {
    let mut lines = text.split('\n').map(str::trim).filter(|line| !line.is_empty());
    let first = lines.clone().next().unwrap_or("Stack trace");
    let head = lines.find(|line| TRACE_HEAD.is_match(line) && !FRAME.is_match(line)).unwrap_or(first);
    // `java.lang.IllegalStateException: boom` is `IllegalStateException`.
    let title = EXCEPTION.captures(head).and_then(|found| found.get(1)).map_or(head, |name| name.as_str());
    if utf16::len(title) > 40 { format!("{}…", utf16::slice(title, 0, 39)) } else { title.to_string() }
}

/// Number of lines in `text` with a clickable source location.
pub fn count_frames(text: &str) -> usize {
    text.split('\n').filter(|line| find_output_links(line).iter().any(|link| link.target().is_some())).count()
}

impl StackTraces {
    /// Opens a tab with the trace as console lines and makes it the Run tool window's active tab.
    /// `None` for blank text.
    pub fn add(&mut self, raw: &str, runs: &mut RunStore) -> Option<String> {
        let text = normalize_trace(raw);
        if text.is_empty() {
            return None;
        }
        let mut model = ConsoleModel::new(ConsoleOptions::default());
        model.append(ChunkKind::Stdout, &text);
        model.flush();
        self.opened += 1;
        let id = format!("trace-{}", self.opened);
        self.tabs.push(StackTraceTab { id: id.clone(), title: trace_title(&text), model });
        runs.active_session = Some(id.clone());
        Some(id)
    }

    pub fn close(&mut self, id: &str) {
        self.tabs.retain(|tab| tab.id != id);
    }

    /// Opens the dialog, prefilled with the clipboard's text when that is a trace.
    pub fn open_dialog(&mut self, clipboard: &str) {
        let trace = count_frames(&normalize_trace(clipboard)) > 0;
        self.dialog = Some(if trace { clipboard.to_string() } else { String::new() });
    }

    /// The dialog's Analyze button: the trace's tab, with the dialog closed; `None` (the dialog stays)
    /// for blank text.
    pub fn analyze(&mut self, text: &str, runs: &mut RunStore) -> Option<String> {
        let id = self.add(text, runs)?;
        self.dialog = None;
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JVM: &str = concat!(
        "Exception in thread \"main\" java.lang.IllegalStateException: boom\r\n",
        "\tat com.acme.OrderController.list(OrderController.kt:22)   \r\n",
        "\tat com.acme.Main.main(Main.java:7)\r\n",
    );

    #[test]
    fn normalises_pasted_text() {
        let normal = concat!(
            "Exception in thread \"main\" java.lang.IllegalStateException: boom\n",
            "\tat com.acme.OrderController.list(OrderController.kt:22)\n\tat com.acme.Main.main(Main.java:7)",
        );
        assert_eq!(normalize_trace(&format!("\n\n{JVM}\n\n")), normal);
    }

    #[test]
    fn titles_tabs_by_the_exception() {
        assert_eq!(trace_title(&normalize_trace(JVM)), "IllegalStateException");
        let python = "Traceback (most recent call last):\n  File \"a.py\", line 3, in <module>";
        assert_eq!(trace_title(python), "Traceback (most recent call last):");
        let rust = "thread 'main' panicked at src/main.rs:4:5:\nboom";
        assert_eq!(trace_title(rust), "thread 'main' panicked at src/main.rs:4…");
    }

    #[test]
    fn counts_frames_with_source_links() {
        assert_eq!(count_frames(&normalize_trace(JVM)), 2);
        assert_eq!(count_frames("just some text"), 0);
        assert_eq!(count_frames("  File \"app/main.py\", line 12, in <module>"), 1);
    }

    #[test]
    fn opens_a_tab_per_trace_with_the_trace_as_console_lines() {
        let (mut traces, mut runs) = (StackTraces::default(), RunStore::new());
        assert_eq!(traces.add("   \n ", &mut runs), None);
        let id = traces.add(JVM, &mut runs).expect("a tab");
        assert_eq!(runs.active_session.as_deref(), Some(id.as_str()));
        let tab = traces.tabs.iter().find(|tab| tab.id == id).expect("the trace's tab");
        assert_eq!(tab.title, "IllegalStateException");
        assert_eq!(tab.model.lines().len(), 3);
        assert!(tab.model.lines()[1].text.contains("OrderController.kt:22"));
    }

    // Not from a spec: the dialog starts with the clipboard's trace only, and Analyze closes it.
    #[test]
    fn the_dialog_takes_a_trace_from_the_clipboard_and_closes_on_analyze() {
        let (mut traces, mut runs) = (StackTraces::default(), RunStore::new());
        traces.open_dialog("just some text");
        assert_eq!(traces.dialog.as_deref(), Some(""));
        traces.open_dialog(JVM);
        assert_eq!(traces.dialog.as_deref(), Some(JVM));
        assert_eq!(traces.analyze(" ", &mut runs), None);
        assert!(traces.dialog.is_some());
        let id = traces.analyze(JVM, &mut runs).expect("a tab");
        assert_eq!((traces.dialog.as_deref(), traces.tabs.len()), (None, 1));
        traces.close(&id);
        assert!(traces.tabs.is_empty());
    }
}
