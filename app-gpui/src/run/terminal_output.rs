//! Output of a run that emulates a terminal: raw terminal data kept (bounded) so a console shown
//! later replays it.

use crate::find::utf16;
use crate::run::console::ChunkKind;

/// Replay buffer bound, in UTF-16 code units (the terminal keeps its own scrollback).
const MAX_CHARS: usize = 2_000_000;

const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

/// What the terminal shows for a chunk: IDE messages dimmed and errors red, on their own lines.
pub fn terminal_text(kind: ChunkKind, text: &str) -> String {
    if kind == ChunkKind::Stdout {
        return text.to_string();
    }
    let text = text.replace("\r\n", "\n").replace('\n', "\r\n");
    let colour = if kind == ChunkKind::System { DIM } else { RED };
    format!("{colour}{text}{RESET}")
}

#[derive(Clone, Debug, Default)]
pub struct TerminalOutput {
    chunks: Vec<String>,
    size: usize,
}

impl TerminalOutput {
    /// Keeps the chunk for replay and returns the data a shown terminal writes now.
    pub fn append(&mut self, kind: ChunkKind, text: &str) -> Option<String> {
        let data = terminal_text(kind, text);
        if data.is_empty() {
            return None;
        }
        self.chunks.push(data.clone());
        self.size += utf16::len(&data) as usize;
        while self.size > MAX_CHARS && self.chunks.len() > 1 {
            self.size -= utf16::len(&self.chunks.remove(0)) as usize;
        }
        Some(data)
    }

    /// The output so far, for a terminal that is shown now.
    pub fn chunks(&self) -> &[String] {
        &self.chunks
    }

    pub fn clear(&mut self) {
        self.chunks.clear();
        self.size = 0;
    }

    pub fn text(&self) -> String {
        self.chunks.concat()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: process output passes through; IDE messages are dimmed with terminal line ends.
    #[test]
    fn styles_ide_messages_and_keeps_process_output_raw() {
        let mut output = TerminalOutput::default();
        assert_eq!(output.append(ChunkKind::Stdout, "a\n").as_deref(), Some("a\n"));
        let message = output.append(ChunkKind::System, "done\r\nbye\n");
        assert_eq!(message.as_deref(), Some("\x1b[2mdone\r\nbye\r\n\x1b[0m"));
        assert_eq!(output.append(ChunkKind::Stderr, "x").as_deref(), Some("\x1b[31mx\x1b[0m"));
        assert_eq!(output.append(ChunkKind::Stdout, ""), None);
        assert_eq!(output.chunks().len(), 3);
        output.clear();
        assert_eq!(output.text(), "");
    }
}
