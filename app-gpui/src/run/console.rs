//! Console line model for run output: splits streamed chunks into lines, parses ANSI SGR colours (one
//! stateful parser per stream), handles `\r` overwrites (progress bars) and caps the buffer.
//!
//! Lengths are UTF-16 code units, as the console's columns and find matches are.

use crate::editor::buffer::byte_of_col;
use crate::find::utf16;
use crate::run::ansi::{AnsiStream, Span};
use ide_run::OutputChunk;

/// Which stream a chunk of output came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChunkKind {
    Stdout,
    Stderr,
    /// A message from the IDE (command line echo, exit code).
    System,
}

impl ChunkKind {
    /// The stream and the text of a backend chunk.
    pub fn of(chunk: &OutputChunk) -> (Self, &str) {
        match chunk {
            OutputChunk::Stdout { text } => (Self::Stdout, text),
            OutputChunk::Stderr { text } => (Self::Stderr, text),
            OutputChunk::System { text } => (Self::System, text),
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Stdout => 0,
            Self::Stderr => 1,
            Self::System => 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleLine {
    /// Monotonic id (stable across trimming).
    pub id: u64,
    pub kind: ChunkKind,
    pub spans: Vec<Span>,
    /// Plain text without escape sequences.
    pub text: String,
}

pub const MAX_LINES: usize = 50_000;
/// Very long lines are broken up so a single minified blob can't stall rendering.
pub const MAX_LINE_LENGTH: u32 = 8_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsoleOptions {
    pub max_lines: usize,
    /// Cycle buffer: total characters kept (Settings › Console); unlimited when absent.
    pub max_chars: Option<usize>,
    /// Keep every completed line for [`ConsoleModel::take_completed`] (e.g. to feed a test parser).
    pub report_lines: bool,
}

impl Default for ConsoleOptions {
    fn default() -> Self {
        Self { max_lines: MAX_LINES, max_chars: None, report_lines: false }
    }
}

/// A line that was completed: its plain text and stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletedLine {
    pub text: String,
    pub kind: ChunkKind,
}

#[derive(Clone, Debug)]
pub struct ConsoleModel {
    lines: Vec<ConsoleLine>,
    /// Total lines ever dropped from the front.
    dropped: usize,
    next_id: u64,
    /// The last line is still being written.
    open: bool,
    /// Length of the open line.
    open_units: u32,
    pending_cr: bool,
    parsers: [AnsiStream; 3],
    max_lines: usize,
    max_chars: Option<usize>,
    /// Characters currently buffered (line text + one per line break).
    chars: usize,
    report_lines: bool,
    completed: Vec<CompletedLine>,
}

impl Default for ConsoleModel {
    fn default() -> Self {
        Self::new(ConsoleOptions::default())
    }
}

impl ConsoleModel {
    pub fn new(options: ConsoleOptions) -> Self {
        Self {
            lines: Vec::new(),
            dropped: 0,
            next_id: 1,
            open: false,
            open_units: 0,
            pending_cr: false,
            parsers: Default::default(),
            max_lines: options.max_lines.max(1),
            max_chars: options.max_chars,
            chars: 0,
            report_lines: options.report_lines,
            completed: Vec::new(),
        }
    }

    pub fn lines(&self) -> &[ConsoleLine] {
        &self.lines
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn size(&self) -> usize {
        self.lines.len()
    }

    /// The last line while it is still being written (no newline yet), e.g. a prompt.
    pub fn open_line(&self) -> Option<&ConsoleLine> {
        self.lines.last().filter(|_| self.open)
    }

    /// Whole buffer as plain text (copy / export).
    pub fn text(&self) -> String {
        self.lines.iter().map(|line| line.text.as_str()).collect::<Vec<_>>().join("\n")
    }

    /// The lines completed since the last call, in order (empty unless `report_lines` is on).
    pub fn take_completed(&mut self) -> Vec<CompletedLine> {
        std::mem::take(&mut self.completed)
    }

    /// Changes the cycle buffer size.
    pub fn set_max_chars(&mut self, max_chars: Option<usize>) {
        self.max_chars = max_chars;
        self.trim();
    }

    pub fn append_chunk(&mut self, chunk: &OutputChunk) {
        let (kind, text) = ChunkKind::of(chunk);
        self.append(kind, text);
    }

    pub fn append(&mut self, kind: ChunkKind, text: &str) {
        // Streams don't share a line; system messages always start on a fresh one.
        if self.open
            && let Some(line) = self.lines.last_mut()
            && (line.kind != kind || kind == ChunkKind::System)
        {
            if line.text.is_empty() {
                line.kind = kind;
            } else {
                self.close_line(None);
            }
        }
        let text = self.parsers[kind.index()].clean(text);
        let mut segments = text.split('\n').peekable();
        while let Some(segment) = segments.next() {
            if segments.peek().is_none() {
                self.append_segment(kind, segment);
            } else {
                self.append_segment(kind, segment.strip_suffix('\r').unwrap_or(segment));
                self.close_line(Some(kind));
            }
        }
        if kind == ChunkKind::System && self.open_line().is_some_and(|line| !line.text.is_empty()) {
            self.close_line(None);
        }
        self.trim();
    }

    fn append_segment(&mut self, kind: ChunkKind, segment: &str) {
        if segment.is_empty() {
            return;
        }
        for (i, piece) in segment.split('\r').enumerate() {
            if i > 0 {
                self.pending_cr = true;
            }
            if piece.is_empty() {
                continue;
            }
            self.ensure_open(kind);
            if self.pending_cr {
                // A carriage return rewinds: what follows overwrites the line.
                self.chars -= self.open_units as usize;
                self.open_units = 0;
                self.pending_cr = false;
                if let Some(line) = self.lines.last_mut() {
                    line.spans.clear();
                    line.text.clear();
                }
            }
            for span in self.parsers[kind.index()].spans(piece) {
                self.push_span(kind, &span);
            }
        }
    }

    fn push_span(&mut self, kind: ChunkKind, span: &Span) {
        let mut rest = span.text.as_str();
        while !rest.is_empty() {
            self.ensure_open(kind);
            let room = MAX_LINE_LENGTH.saturating_sub(self.open_units);
            let (text, tail) = rest.split_at(byte_of_col(rest, room));
            if !text.is_empty()
                && let Some(line) = self.lines.last_mut()
            {
                match line.spans.last_mut() {
                    Some(previous) if previous.style == span.style => previous.text.push_str(text),
                    _ => line.spans.push(Span { text: text.to_string(), style: span.style }),
                }
                line.text.push_str(text);
                let units = utf16::len(text);
                self.open_units += units;
                self.chars += units as usize;
            }
            rest = tail;
            if !rest.is_empty() {
                self.close_line(Some(kind));
            }
        }
    }

    fn ensure_open(&mut self, kind: ChunkKind) {
        if self.open {
            return;
        }
        self.lines.push(ConsoleLine { id: self.next_id, kind, spans: Vec::new(), text: String::new() });
        self.next_id += 1;
        self.open = true;
        self.open_units = 0;
        self.chars += 1;
    }

    /// Ends the current line (creating an empty one of `kind` for a bare newline).
    fn close_line(&mut self, kind: Option<ChunkKind>) {
        if !self.open
            && let Some(kind) = kind
        {
            self.ensure_open(kind);
        }
        let was_open = self.open;
        self.open = false;
        self.pending_cr = false;
        if was_open
            && self.report_lines
            && let Some(line) = self.lines.last()
        {
            self.completed.push(CompletedLine { text: line.text.clone(), kind: line.kind });
        }
    }

    /// Completes a trailing partial line (at process exit).
    pub fn flush(&mut self) {
        if self.open {
            self.close_line(None);
        }
    }

    fn trim(&mut self) {
        let mut remove = 0;
        if self.lines.len() > self.max_lines {
            // Drop in batches so trimming is amortised.
            remove = self.lines.len() - self.max_lines + self.max_lines.div_ceil(10);
        }
        if let Some(max_chars) = self.max_chars.filter(|max| self.chars > *max) {
            // Down to 90% of the buffer, never the line still being written.
            let mut excess = self.chars as i64 - (max_chars as f64 * 0.9).floor() as i64;
            let closed = self.lines.len() - usize::from(self.open);
            let mut count = 0;
            for line in &self.lines[..closed] {
                if excess <= 0 {
                    break;
                }
                excess -= i64::from(utf16::len(&line.text)) + 1;
                count += 1;
            }
            remove = remove.max(count);
        }
        let remove = remove.min(self.lines.len());
        if remove == 0 {
            return;
        }
        if remove == self.lines.len() {
            self.open = false;
            self.pending_cr = false;
        }
        let removed: usize = self.lines.drain(..remove).map(|line| utf16::len(&line.text) as usize + 1).sum();
        self.chars = self.chars.saturating_sub(removed);
        self.dropped += remove;
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.chars = 0;
        self.dropped = 0;
        self.open = false;
        self.open_units = 0;
        self.pending_cr = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::ansi::{AnsiColor, SpanStyle};

    fn texts(model: &ConsoleModel) -> Vec<&str> {
        model.lines().iter().map(|line| line.text.as_str()).collect()
    }

    fn fg(text: &str, color: AnsiColor) -> Span {
        Span { text: text.into(), style: SpanStyle { fg: Some(color), ..SpanStyle::default() } }
    }

    #[test]
    fn splits_chunks_into_lines_and_joins_partial_lines_across_chunks() {
        let mut m = ConsoleModel::default();
        m.append(ChunkKind::Stdout, "hello wo");
        m.append(ChunkKind::Stdout, "rld\nsecond\n");
        m.append(ChunkKind::Stdout, "third");
        assert_eq!(texts(&m), ["hello world", "second", "third"]);
    }

    #[test]
    fn keeps_streams_and_system_messages_on_their_own_lines() {
        let mut m = ConsoleModel::default();
        m.append_chunk(&OutputChunk::System { text: "cargo run\n".into() });
        m.append_chunk(&OutputChunk::Stdout { text: "out".into() });
        m.append_chunk(&OutputChunk::Stderr { text: "err\n".into() });
        m.append_chunk(&OutputChunk::System { text: "\nProcess finished with exit code 0\n".into() });
        let lines: Vec<(ChunkKind, &str)> = m.lines().iter().map(|line| (line.kind, line.text.as_str())).collect();
        assert_eq!(
            lines,
            [
                (ChunkKind::System, "cargo run"),
                (ChunkKind::Stdout, "out"),
                (ChunkKind::Stderr, "err"),
                (ChunkKind::System, ""),
                (ChunkKind::System, "Process finished with exit code 0"),
            ]
        );
    }

    #[test]
    fn handles_crlf_and_carriage_return_overwrites() {
        let mut m = ConsoleModel::default();
        m.append(ChunkKind::Stdout, "a\r\nb\r\n");
        m.append(ChunkKind::Stdout, "progress 10%\rprogress 50%");
        m.append(ChunkKind::Stdout, "\rprogress 100%\n");
        assert_eq!(texts(&m), ["a", "b", "progress 100%"]);
    }

    #[test]
    fn treats_a_cr_split_from_its_lf_across_chunks_as_a_line_ending() {
        let mut m = ConsoleModel::default();
        m.append(ChunkKind::Stdout, "line\r");
        m.append(ChunkKind::Stdout, "\nnext\n");
        assert_eq!(texts(&m), ["line", "next"]);
    }

    #[test]
    fn parses_sgr_colours_and_keeps_state_across_chunks() {
        let mut m = ConsoleModel::default();
        m.append(ChunkKind::Stdout, "\x1b[31mred ");
        m.append(ChunkKind::Stdout, "still\x1b[0m plain\n");
        m.append(ChunkKind::Stdout, "\x1b[1;92mbold green\x1b[22;39m\n");
        let (first, second) = (&m.lines()[0], &m.lines()[1]);
        assert_eq!(first.text, "red still plain");
        assert_eq!(first.spans, [fg("red still", AnsiColor::Named(1)), Span::plain(" plain")]);
        assert_eq!(first.spans[0].style.fg.map(AnsiColor::css).as_deref(), Some("var(--ansi-red)"));
        let mut bold_green = fg("bold green", AnsiColor::Named(10));
        bold_green.style.bold = true;
        assert_eq!(second.spans, [bold_green]);
        assert_eq!(AnsiColor::Named(10).css(), "var(--ansi-bright-green)");
    }

    #[test]
    fn supports_256_colour_and_truecolor() {
        let mut m = ConsoleModel::default();
        m.append(ChunkKind::Stdout, "\x1b[38;5;196mA\x1b[38;2;10;20;30mB\x1b[48;5;244mC\x1b[0m\n");
        let spans = &m.lines()[0].spans;
        let css = |color: Option<AnsiColor>| color.map(AnsiColor::css).unwrap_or_default();
        assert_eq!((spans[0].text.as_str(), css(spans[0].style.fg).as_str()), ("A", "rgb(255, 0, 0)"));
        assert_eq!((spans[1].text.as_str(), css(spans[1].style.fg).as_str()), ("B", "rgb(10, 20, 30)"));
        assert_eq!((spans[2].text.as_str(), css(spans[2].style.bg).as_str()), ("C", "rgb(128, 128, 128)"));
    }

    #[test]
    fn reassembles_escape_sequences_split_across_chunks() {
        let mut m = ConsoleModel::default();
        m.append(ChunkKind::Stdout, "x\x1b[3");
        m.append(ChunkKind::Stdout, "2mgreen\x1b[0m\n");
        assert_eq!(m.lines()[0].spans, [Span::plain("x"), fg("green", AnsiColor::Named(2))]);
    }

    #[test]
    fn strips_osc_hyperlinks_cursor_movement_and_other_control_characters() {
        let mut m = ConsoleModel::default();
        m.append(ChunkKind::Stdout, "\x1b]8;;https://x.dev\x07link\x1b]8;;\x07 \x1b[?25lok\x1b[?25h\x07\n");
        m.append(ChunkKind::Stdout, "50%\x1b[2K\x1b[1Gdone\n");
        assert_eq!(texts(&m), ["link ok", "done"]);
    }

    #[test]
    fn reports_completed_lines_plain_text_only() {
        let mut m = ConsoleModel::new(ConsoleOptions { report_lines: true, ..ConsoleOptions::default() });
        m.append(ChunkKind::Stdout, "\x1b[32mtest a ... ok\x1b[0m\npart");
        let line = |text: &str| CompletedLine { text: text.into(), kind: ChunkKind::Stdout };
        assert_eq!(m.take_completed(), [line("test a ... ok")]);
        m.flush();
        assert_eq!(m.take_completed(), [line("part")]);
    }

    #[test]
    fn caps_the_buffer_and_tracks_dropped_lines() {
        let mut m = ConsoleModel::new(ConsoleOptions { max_lines: 100, ..ConsoleOptions::default() });
        for i in 0..250 {
            m.append(ChunkKind::Stdout, &format!("line {i}\n"));
        }
        assert!(m.size() <= 100);
        assert_eq!(m.dropped() + m.size(), 250);
        assert_eq!(m.lines()[m.size() - 1].text, "line 249");
        // Ids stay monotonic after trimming.
        assert_eq!(m.lines()[0].id, m.dropped() as u64 + 1);
    }

    #[test]
    fn breaks_up_very_long_lines() {
        let mut m = ConsoleModel::default();
        let long = MAX_LINE_LENGTH as usize;
        m.append(ChunkKind::Stdout, &format!("{}\n", "x".repeat(long * 2 + 5)));
        assert_eq!(m.lines().iter().map(|line| line.text.len()).collect::<Vec<_>>(), [long, long, 5]);
    }

    #[test]
    fn clear_empties_the_buffer() {
        let mut m = ConsoleModel::default();
        m.append(ChunkKind::Stdout, "a\nb");
        m.clear();
        m.append(ChunkKind::Stdout, "c\n");
        assert_eq!(texts(&m), ["c"]);
    }

    // Not from a spec: the cycle buffer drops whole closed lines down to 90% and keeps the open one.
    #[test]
    fn the_character_limit_drops_old_lines_but_not_the_open_one() {
        let mut m = ConsoleModel::new(ConsoleOptions { max_chars: Some(20), ..ConsoleOptions::default() });
        m.append(ChunkKind::Stdout, "aaaaaaaa\nbbbbbbbb\ncccc");
        assert_eq!(texts(&m), ["bbbbbbbb", "cccc"]);
        assert_eq!(m.dropped(), 1);
        m.set_max_chars(Some(1));
        assert_eq!(texts(&m), ["cccc"]);
        assert_eq!(m.open_line().map(|line| line.text.as_str()), Some("cccc"));
    }
}
