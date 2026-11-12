//! The terminal's screen: what a program's output has drawn on the grid, kept by
//! `alacritty_terminal`, and read back as rows of styled runs for painting.
//!
//! The emulator owns no process. Output is fed to it as it arrives from the backend's PTY, and
//! what the program must be answered (device reports, colour queries) comes back as effects for
//! whoever holds the PTY to write.

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Processor, Rgb};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

/// Lines kept above the screen.
pub const SCROLLBACK: usize = 10_000;

/// How much of its colour dim text keeps.
const DIM_FACTOR: f32 = 0.66;

/// The colours programs name rather than spell out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    /// Black, red, green, yellow, blue, magenta, cyan, white, then their bright variants.
    pub ansi: [Rgb; 16],
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
}

impl Default for Palette {
    /// xterm's stock colours: a stand-in until the theme's palette is set.
    fn default() -> Self {
        let rgb = |r, g, b| Rgb { r, g, b };
        Self {
            ansi: [
                rgb(0, 0, 0),
                rgb(205, 0, 0),
                rgb(0, 205, 0),
                rgb(205, 205, 0),
                rgb(0, 0, 238),
                rgb(205, 0, 205),
                rgb(0, 205, 205),
                rgb(229, 229, 229),
                rgb(127, 127, 127),
                rgb(255, 0, 0),
                rgb(0, 255, 0),
                rgb(255, 255, 0),
                rgb(92, 92, 255),
                rgb(255, 0, 255),
                rgb(0, 255, 255),
                rgb(255, 255, 255),
            ],
            foreground: rgb(229, 229, 229),
            background: rgb(0, 0, 0),
            cursor: rgb(229, 229, 229),
        }
    }
}

impl Palette {
    /// Colour `index` of the 256-colour table: the 16 named ones, the 6×6×6 cube, the greys.
    fn indexed(&self, index: u8) -> Rgb {
        match index {
            0..=15 => self.ansi[usize::from(index)],
            16..=231 => {
                let level = |step: u8| if step == 0 { 0 } else { 55 + step * 40 };
                let cube = index - 16;
                Rgb { r: level(cube / 36), g: level(cube / 6 % 6), b: level(cube % 6) }
            }
            232..=255 => {
                let grey = 8 + (index - 232) * 10;
                Rgb { r: grey, g: grey, b: grey }
            }
        }
    }
}

/// Cells of one row drawn alike. A wide character is a run of its own, two cells wide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The first cell of the run.
    pub col: usize,
    /// How many cells it covers.
    pub cells: usize,
    pub text: String,
    pub fg: Rgb,
    /// None for the terminal's own background.
    pub bg: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
}

/// Where the cursor is on the screen, and how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub shape: CursorShape,
}

/// What feeding output asks of the terminal's owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// An answer to the program: write it to the PTY.
    Write(String),
    /// The program named the terminal, or (None) gave the name back.
    Title(Option<String>),
    Bell,
    /// The program put text on the clipboard (OSC 52).
    Copy(String),
}

/// How a selection grows from where it started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectBy {
    Cell,
    Word,
    Line,
}

#[derive(Clone, Default)]
struct Events(Rc<RefCell<Vec<Event>>>);

impl EventListener for Events {
    fn send_event(&self, event: Event) {
        self.0.borrow_mut().push(event);
    }
}

struct Size {
    cols: usize,
    rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

/// A terminal screen with its scrollback.
pub struct Emulator {
    term: Term<Events>,
    parser: Processor,
    events: Events,
    palette: Palette,
    /// Pixel size of a cell, reported to programs that ask for the text area's size.
    cell: (u16, u16),
}

impl Emulator {
    /// A blank screen of `cols` × `rows` cells (at least 2 × 1).
    pub fn new(cols: usize, rows: usize) -> Self {
        let events = Events::default();
        let config = Config { scrolling_history: SCROLLBACK, ..Config::default() };
        let size = Size { cols: cols.max(2), rows: rows.max(1) };
        Self {
            term: Term::new(config, &size, events.clone()),
            parser: Processor::new(),
            events,
            palette: Palette::default(),
            cell: (0, 0),
        }
    }

    pub fn cols(&self) -> usize {
        self.term.columns()
    }

    pub fn rows(&self) -> usize {
        self.term.screen_lines()
    }

    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// Resizes the screen; `cell` is a cell's size in pixels. Lines reflow as the grid decides.
    pub fn resize(&mut self, cols: usize, rows: usize, cell: (u16, u16)) {
        self.cell = cell;
        let size = Size { cols: cols.max(2), rows: rows.max(1) };
        if size.cols != self.cols() || size.rows != self.rows() {
            self.term.resize(size);
        }
    }

    /// Draws `output` on the screen. Returns what the program is owed and what it announced.
    pub fn feed(&mut self, output: &[u8]) -> Vec<Effect> {
        self.parser.advance(&mut self.term, output);
        let events = std::mem::take(&mut *self.events.0.borrow_mut());
        events
            .into_iter()
            .filter_map(|event| match event {
                Event::PtyWrite(text) => Some(Effect::Write(text)),
                Event::Title(title) => Some(Effect::Title(Some(title))),
                Event::ResetTitle => Some(Effect::Title(None)),
                Event::Bell => Some(Effect::Bell),
                Event::ClipboardStore(_, text) => Some(Effect::Copy(text)),
                Event::ColorRequest(index, format) => Some(Effect::Write(format(self.color_at(index)))),
                Event::TextAreaSizeRequest(format) => Some(Effect::Write(format(self.window_size()))),
                // A program does not get to read the clipboard, and the process is the backend's.
                _ => None,
            })
            .collect()
    }

    fn window_size(&self) -> WindowSize {
        let clamp = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
        WindowSize {
            num_lines: clamp(self.rows()),
            num_cols: clamp(self.cols()),
            cell_width: self.cell.0,
            cell_height: self.cell.1,
        }
    }

    /// Colour `index` of the terminal's table as it stands: one the program redefined, else the
    /// palette's.
    fn color_at(&self, index: usize) -> Rgb {
        if let Some(set) = self.term.colors()[index] {
            return set;
        }
        match u8::try_from(index) {
            Ok(index) => self.palette.indexed(index),
            Err(_) if index == NamedColor::Background as usize => self.palette.background,
            Err(_) if index == NamedColor::Cursor as usize => self.palette.cursor,
            Err(_) => self.palette.foreground,
        }
    }

    fn resolve(&self, color: Color) -> Rgb {
        match color {
            Color::Spec(rgb) => rgb,
            Color::Indexed(index) => self.color_at(usize::from(index)),
            Color::Named(named) => self.color_at(named as usize),
        }
    }

    /// The screen as rows of runs, top to bottom, as scrolled. Blank cells at the end of a row
    /// are left out.
    pub fn runs(&self) -> Vec<Vec<Run>> {
        let content = self.term.renderable_content();
        let offset = content.display_offset as i32;
        let mut rows: Vec<Vec<Run>> = vec![Vec::new(); self.rows()];
        for indexed in content.display_iter {
            let cell = indexed.cell;
            let Ok(row) = usize::try_from(indexed.point.line.0 + offset) else { continue };
            let Some(runs) = rows.get_mut(row) else { continue };
            if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            let (mut fg, mut bg) = (self.resolve(cell.fg), self.resolve(cell.bg));
            if cell.flags.contains(Flags::DIM) {
                let dim = |v: u8| (f32::from(v) * DIM_FACTOR).round() as u8;
                fg = Rgb { r: dim(fg.r), g: dim(fg.g), b: dim(fg.b) };
            }
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.flags.contains(Flags::HIDDEN) {
                fg = bg;
            }
            let wide = cell.flags.contains(Flags::WIDE_CHAR);
            let run = Run {
                col: indexed.point.column.0,
                cells: if wide { 2 } else { 1 },
                text: std::iter::once(cell.c).chain(cell.zerowidth().into_iter().flatten().copied()).collect(),
                fg,
                bg: (bg != self.palette.background || cell.flags.contains(Flags::INVERSE)).then_some(bg),
                bold: cell.flags.contains(Flags::BOLD),
                italic: cell.flags.contains(Flags::ITALIC),
                underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
                strikeout: cell.flags.contains(Flags::STRIKEOUT),
            };
            match runs.last_mut() {
                Some(last)
                    if !wide
                        && last.cells == last.text.chars().count()
                        && last.col + last.cells == run.col
                        && (last.fg, last.bg, last.bold, last.italic, last.underline, last.strikeout)
                            == (run.fg, run.bg, run.bold, run.italic, run.underline, run.strikeout) =>
                {
                    last.text.push_str(&run.text);
                    last.cells += 1;
                }
                _ => runs.push(run),
            }
        }
        for runs in &mut rows {
            // Trailing blanks draw nothing: spaces on the terminal's own background.
            while let Some(last) = runs.last_mut() {
                if last.bg.is_some() || last.underline || last.strikeout {
                    break;
                }
                let kept = last.text.trim_end_matches(' ').chars().count();
                if kept == last.text.chars().count() {
                    break;
                }
                if kept == 0 {
                    runs.pop();
                } else {
                    last.text = last.text.chars().take(kept).collect();
                    last.cells = kept;
                    break;
                }
            }
        }
        rows
    }

    /// The text of each screen row, without trailing blanks.
    pub fn lines(&self) -> Vec<String> {
        self.runs()
            .into_iter()
            .map(|runs| {
                let mut line = String::new();
                for run in runs {
                    let width = line.chars().count();
                    line.extend(std::iter::repeat_n(' ', run.col.saturating_sub(width)));
                    line.push_str(&run.text);
                }
                line
            })
            .collect()
    }

    /// The cursor, when the program shows it and it is on the screen as scrolled.
    pub fn cursor(&self) -> Option<Cursor> {
        let content = self.term.renderable_content();
        if content.cursor.shape == CursorShape::Hidden {
            return None;
        }
        let row = usize::try_from(content.cursor.point.line.0 + content.display_offset as i32).ok()?;
        (row < self.rows()).then_some(Cursor { row, col: content.cursor.point.column.0, shape: content.cursor.shape })
    }

    /// Clears the terminal: the line the cursor is on becomes the first line, and everything
    /// else, on the screen and above it, is gone. The program is not told; a shell at its prompt
    /// simply finds itself at the top of an empty terminal.
    pub fn clear(&mut self) {
        self.scroll_to_bottom();
        self.clear_selection();
        let row = usize::try_from(self.term.grid().cursor.point.line.0).unwrap_or(0);
        let mut sequence = String::new();
        if row > 0 {
            // The screen scrolls up until the cursor's line is the first, and the cursor follows it.
            sequence.push_str(&format!("\x1b[{row}S\x1b[{row}A"));
        }
        if self.rows() > 1 {
            // What was below the cursor's line goes; the cursor stays where it is.
            sequence.push_str("\x1b7\x1b[2;1H\x1b[J\x1b8");
        }
        // And so does what scrolled off the top.
        sequence.push_str("\x1b[3J");
        let _ = self.feed(sequence.as_bytes());
    }

    /// Scrolls the view by `lines` towards older output (negative: towards newer).
    pub fn scroll_lines(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    /// How many lines the view is scrolled back.
    pub fn scrolled_back(&self) -> usize {
        self.term.grid().display_offset()
    }

    /// How many lines are kept above the screen.
    pub fn history(&self) -> usize {
        self.term.history_size()
    }

    fn point(&self, row: usize, col: usize) -> Point {
        let line = row.min(self.rows() - 1) as i32 - self.scrolled_back() as i32;
        Point::new(Line(line), Column(col.min(self.cols() - 1)))
    }

    /// Starts a selection at the cell at `row`, `col` of the screen as scrolled.
    pub fn select(&mut self, row: usize, col: usize, by: SelectBy) {
        let kind = match by {
            SelectBy::Cell => SelectionType::Simple,
            SelectBy::Word => SelectionType::Semantic,
            SelectBy::Line => SelectionType::Lines,
        };
        self.term.selection = Some(Selection::new(kind, self.point(row, col), Side::Left));
    }

    /// Extends the selection to include the cell at `row`, `col`.
    pub fn select_to(&mut self, row: usize, col: usize) {
        let point = self.point(row, col);
        if let Some(selection) = &mut self.term.selection {
            selection.update(point, Side::Right);
        }
    }

    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    pub fn selected_text(&self) -> Option<String> {
        self.term.selection_to_string().filter(|text| !text.is_empty())
    }

    /// The selected cells of screen row `row`, if any.
    pub fn selected_cols(&self, row: usize) -> Option<Range<usize>> {
        let selection = self.term.renderable_content().selection?;
        let line = Line(row as i32 - self.scrolled_back() as i32);
        let cols: Vec<usize> =
            (0..self.cols()).filter(|&col| selection.contains(Point::new(line, Column(col)))).collect();
        Some(*cols.first()?..cols.last()? + 1)
    }

    fn mode(&self) -> TermMode {
        *self.term.mode()
    }

    /// Whether the cursor keys send their application sequences (`ESC O A`).
    pub fn app_cursor(&self) -> bool {
        self.mode().contains(TermMode::APP_CURSOR)
    }

    /// Whether pasted text is to be wrapped in the bracketed-paste markers.
    pub fn bracketed_paste(&self) -> bool {
        self.mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// Whether a full-screen program has taken the screen (no scrollback there).
    pub fn alt_screen(&self) -> bool {
        self.mode().contains(TermMode::ALT_SCREEN)
    }

    /// Whether the program wants mouse events instead of the terminal selecting text.
    pub fn mouse_reporting(&self) -> bool {
        self.mode().intersects(TermMode::MOUSE_MODE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fed(cols: usize, rows: usize, output: &str) -> Emulator {
        let mut emulator = Emulator::new(cols, rows);
        assert_eq!(emulator.feed(output.as_bytes()), [], "plain output asks for nothing");
        emulator
    }

    #[test]
    fn output_lands_on_the_grid_and_moves_the_cursor() {
        let emulator = fed(20, 4, "one\r\ntwo words\r\n$ ");
        assert_eq!(emulator.lines(), ["one", "two words", "$", ""]);
        assert_eq!(emulator.cursor(), Some(Cursor { row: 2, col: 2, shape: CursorShape::Block }));
        assert_eq!((emulator.cols(), emulator.rows()), (20, 4));
    }

    #[test]
    fn colours_and_attributes_split_a_row_into_runs() {
        let emulator = fed(
            40,
            2,
            "ok \x1b[1;31mred\x1b[0m \x1b[4;38;5;196mcube\x1b[0m \x1b[48;2;1;2;3mbg\x1b[0m \x1b[7mrev\x1b[0m",
        );
        let palette = Palette::default();
        let rows = emulator.runs();
        let runs = &rows[0];
        let texts: Vec<&str> = runs.iter().map(|run| run.text.as_str()).collect();
        assert_eq!(texts, ["ok ", "red", " ", "cube", " ", "bg", " ", "rev"]);
        assert_eq!((runs[0].col, runs[0].cells, runs[0].fg, runs[0].bg), (0, 3, palette.foreground, None));
        assert!(runs[1].bold && runs[1].fg == palette.ansi[1] && !runs[1].underline);
        assert!(runs[3].underline && runs[3].fg == Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(runs[5].bg, Some(Rgb { r: 1, g: 2, b: 3 }));
        // Reverse video swaps the two, and its background is drawn even though it is a default.
        assert_eq!((runs[7].fg, runs[7].bg), (palette.background, Some(palette.foreground)));
        assert_eq!(runs[7].col, 15);
        assert!(rows[1].is_empty());
    }

    #[test]
    fn the_256_colour_table_has_the_cube_and_the_greys() {
        let palette = Palette::default();
        assert_eq!(palette.indexed(9), palette.ansi[9]);
        assert_eq!(palette.indexed(16), Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(palette.indexed(21), Rgb { r: 0, g: 0, b: 255 });
        assert_eq!(palette.indexed(110), Rgb { r: 135, g: 175, b: 215 });
        assert_eq!(palette.indexed(231), Rgb { r: 255, g: 255, b: 255 });
        assert_eq!(palette.indexed(232), Rgb { r: 8, g: 8, b: 8 });
        assert_eq!(palette.indexed(255), Rgb { r: 238, g: 238, b: 238 });
    }

    #[test]
    fn a_wide_character_is_its_own_two_cell_run() {
        let emulator = fed(20, 1, "a漢b");
        let rows = emulator.runs();
        let shape: Vec<(usize, usize, &str)> = rows[0].iter().map(|r| (r.col, r.cells, r.text.as_str())).collect();
        assert_eq!(shape, [(0, 1, "a"), (1, 2, "漢"), (3, 1, "b")]);
        assert_eq!(emulator.cursor().map(|cursor| cursor.col), Some(4));
    }

    #[test]
    fn queries_are_answered_and_announcements_reported() {
        let mut emulator = Emulator::new(80, 24);
        // Device status: where is the cursor?
        assert_eq!(emulator.feed(b"ab\x1b[6n"), [Effect::Write("\x1b[1;3R".to_owned())]);
        assert_eq!(emulator.feed(b"\x1b]0;build\x07"), [Effect::Title(Some("build".to_owned()))]);
        assert_eq!(emulator.feed(b"\x07"), [Effect::Bell]);
        // The background colour, asked for by programs that adapt to light and dark.
        emulator.set_palette(Palette { background: Rgb { r: 0x12, g: 0x34, b: 0x56 }, ..Palette::default() });
        assert_eq!(emulator.feed(b"\x1b]11;?\x07"), [Effect::Write("\x1b]11;rgb:1212/3434/5656\x07".to_owned())]);
        // The text area in pixels and cells.
        emulator.resize(80, 24, (8, 17));
        assert_eq!(emulator.feed(b"\x1b[14t"), [Effect::Write("\x1b[4;408;640t".to_owned())]);
    }

    #[test]
    fn modes_follow_what_the_program_sets() {
        let mut emulator = Emulator::new(20, 4);
        assert!(!emulator.app_cursor() && !emulator.bracketed_paste() && !emulator.alt_screen());
        emulator.feed(b"\x1b[?1h\x1b[?2004h\x1b[?1049h\x1b[?1000h");
        assert!(emulator.app_cursor() && emulator.bracketed_paste() && emulator.alt_screen());
        assert!(emulator.mouse_reporting());
        emulator.feed(b"\x1b[?25l");
        assert_eq!(emulator.cursor(), None, "a hidden cursor is not drawn");
        emulator.feed(b"\x1b[?25h\x1b[?1l\x1b[?2004l\x1b[?1049l\x1b[?1000l");
        assert!(!emulator.app_cursor() && !emulator.bracketed_paste() && !emulator.alt_screen());
        assert!(emulator.cursor().is_some() && !emulator.mouse_reporting());
    }

    #[test]
    fn older_output_scrolls_back_and_new_output_keeps_the_place() {
        let mut emulator = Emulator::new(10, 3);
        let output: String = (1..=8).map(|n| format!("line {n}\r\n")).collect();
        emulator.feed(output.as_bytes());
        assert_eq!(emulator.lines(), ["line 7", "line 8", ""]);
        assert_eq!((emulator.history(), emulator.scrolled_back()), (6, 0));
        emulator.scroll_lines(2);
        assert_eq!(emulator.lines(), ["line 5", "line 6", "line 7"]);
        assert_eq!(emulator.cursor(), None, "the cursor's row is scrolled out of view");
        emulator.scroll_lines(100);
        assert_eq!((emulator.lines()[0].as_str(), emulator.scrolled_back()), ("line 1", 6));
        emulator.scroll_to_bottom();
        assert_eq!(emulator.lines(), ["line 7", "line 8", ""]);
    }

    #[test]
    fn clearing_keeps_only_the_cursors_line() {
        let mut emulator = Emulator::new(12, 4);
        let output: String = (1..=8).map(|n| format!("line {n}\r\n")).collect();
        emulator.feed(output.as_bytes());
        emulator.feed(b"$ typed");
        assert_eq!(emulator.lines(), ["line 6", "line 7", "line 8", "$ typed"]);
        emulator.scroll_lines(2);
        emulator.clear();
        assert_eq!(emulator.lines(), ["$ typed", "", "", ""]);
        assert_eq!((emulator.history(), emulator.scrolled_back()), (0, 0));
        assert_eq!(emulator.cursor().map(|cursor| (cursor.row, cursor.col)), Some((0, 7)));
        // Typing goes on where it was, and clearing an already clear terminal changes nothing.
        emulator.feed(b"!");
        emulator.clear();
        assert_eq!(emulator.lines(), ["$ typed!", "", "", ""]);
        // Output below the cursor's line goes too.
        emulator.feed(b"\r\nbelow\x1b[A");
        emulator.clear();
        assert_eq!(emulator.lines(), ["$ typed!", "", "", ""]);
    }

    #[test]
    fn a_resize_keeps_the_text_and_reflows_long_lines() {
        let mut emulator = fed(10, 3, "0123456789abcde");
        assert_eq!(emulator.lines(), ["0123456789", "abcde", ""]);
        emulator.resize(15, 3, (8, 16));
        assert_eq!(emulator.lines(), ["0123456789abcde", "", ""]);
        emulator.resize(1, 0, (8, 16));
        assert_eq!((emulator.cols(), emulator.rows()), (2, 1), "never smaller than two cells by one");
    }

    #[test]
    fn selections_grow_by_cell_word_or_line_and_give_their_text() {
        let mut emulator = fed(20, 3, "alpha beta\r\ngamma");
        assert_eq!(emulator.selected_text(), None);
        emulator.select(0, 6, SelectBy::Cell);
        emulator.select_to(0, 8);
        assert_eq!(emulator.selected_text().as_deref(), Some("bet"));
        assert_eq!(emulator.selected_cols(0), Some(6..9));
        assert_eq!(emulator.selected_cols(1), None);
        emulator.select_to(1, 1);
        assert_eq!(emulator.selected_text().as_deref(), Some("beta\nga"));
        emulator.select(0, 1, SelectBy::Word);
        assert_eq!(emulator.selected_text().as_deref(), Some("alpha"));
        emulator.select(1, 3, SelectBy::Line);
        assert_eq!(emulator.selected_text().as_deref(), Some("gamma\n"));
        emulator.clear_selection();
        assert_eq!((emulator.selected_text(), emulator.selected_cols(1)), (None, None));
    }
}
