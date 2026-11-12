//! A terminal in a window: a shell on the backend's PTY, its screen drawn natively.
//!
//! Output arrives from the backend in chunks and is fed to the [`Emulator`]; keys and typed
//! text are encoded and written to the PTY. The grid is sized to the room the view is given,
//! and the PTY is told, so full-screen programs lay out to it.

use super::emulator::{Effect, Emulator, Palette, SelectBy};
use super::encode::{KeyModes, KeyMods, encode_key, encode_paste};
use super::keys::{CLOSE_SESSION_ACTION, FIND_ACTION, KeyContext, KeyEvent, KeyFocus, KeyHandling, route_terminal_key};
use super::theme::terminal_theme;
use crate::backend::{Backend, BackendEvent};
use crate::platform::dispatcher::{Mods, stroke};
use crate::platform::input::{self, Keymap};
use crate::platform::settings;
use crate::theme;
use alacritty_terminal::vte::ansi::{CursorShape, Rgb};
use futures::StreamExt as _;
use futures::channel::mpsc::unbounded;
use gpui_kit::{
    App, Bounds, ClipboardItem, Context, DispatchPhase, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, Font, FontStyle, FontWeight, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels,
    Point, Render, Rgba, ScrollEvent, SharedString, Style, Task, TextAlign, TextRun, UTF16Selection, UnderlineStyle,
    Window, div, fill, font, outline, point, prelude::*, px, relative, size,
};
use ide_core::{IdeEvent, TerminalId};
use ide_host::Host;
use ide_terminal::TerminalOptions;
use std::ops::Range;
use std::sync::Arc;

/// What a terminal tells its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// The program named the terminal, or gave the name back.
    TitleChanged,
    /// The shell ended, with its exit code when it has one.
    Exited(Option<i32>),
    Bell,
    /// The user asked to close this terminal (⌘W).
    CloseRequested,
    /// The user asked to search its output (⌘F).
    FindRequested,
}

/// Where the grid was last painted.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Grid {
    bounds: Bounds<Pixels>,
    cell: (Pixels, Pixels),
}

/// One terminal session and its view.
pub struct TerminalView {
    focus: FocusHandle,
    host: Arc<Host>,
    /// The session on the backend; none once the shell has ended or when it could not start.
    id: Option<TerminalId>,
    /// The session's own title, and the one the program set over it.
    spawned_title: String,
    program_title: Option<String>,
    emulator: Emulator,
    exit: Option<Option<i32>>,
    /// Why the shell could not start.
    error: Option<String>,
    grid: Option<Grid>,
    /// Text being composed by the input method, shown at the cursor until it is committed.
    composing: Option<String>,
    /// A selection is being dragged out.
    selecting: bool,
    listener: Option<u64>,
    /// The claim on this terminal's keys, ahead of the keymap.
    key_claim: Option<u64>,
    _output: Task<()>,
}

impl EventEmitter<TerminalEvent> for TerminalView {}

fn hsla(color: Rgb) -> Hsla {
    Rgba { r: f32::from(color.r) / 255.0, g: f32::from(color.g) / 255.0, b: f32::from(color.b) / 255.0, a: 1.0 }.into()
}

/// The terminal's colours in the active theme.
fn palette(cx: &App) -> Palette {
    let Some(active) = cx.try_global::<theme::ActiveTheme>() else { return Palette::default() };
    let colors = terminal_theme(&active.theme);
    let rgb = |color: theme::tokens::Color| {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Rgb { r: byte(color.r), g: byte(color.g), b: byte(color.b) }
    };
    Palette {
        ansi: colors.ansi().map(rgb),
        foreground: rgb(colors.foreground),
        background: rgb(colors.background),
        cursor: rgb(colors.cursor),
    }
}

/// The key as a page would name it, which is how the terminal's key rules are written: `Enter`,
/// `ArrowLeft`, or the character typed.
fn dom_key(key: &str, shift: bool) -> String {
    match key {
        "enter" => "Enter".to_owned(),
        "escape" => "Escape".to_owned(),
        "backspace" => "Backspace".to_owned(),
        "tab" => "Tab".to_owned(),
        "left" => "ArrowLeft".to_owned(),
        "right" => "ArrowRight".to_owned(),
        "up" => "ArrowUp".to_owned(),
        "down" => "ArrowDown".to_owned(),
        "space" => " ".to_owned(),
        typed if shift && typed.chars().count() == 1 => typed.to_uppercase(),
        other => other.to_owned(),
    }
}

/// The physical key, for the keys whose control code does not depend on the layout.
fn dom_code(key: &str) -> String {
    match key {
        "[" => "BracketLeft".to_owned(),
        "]" => "BracketRight".to_owned(),
        "\\" => "Backslash".to_owned(),
        "space" => "Space".to_owned(),
        letter if letter.len() == 1 && letter.as_bytes()[0].is_ascii_alphabetic() => {
            format!("Key{}", letter.to_ascii_uppercase())
        }
        _ => String::new(),
    }
}

impl TerminalView {
    /// Starts a shell for `window_label` and shows its terminal. A shell that cannot start
    /// leaves a view that says why.
    pub fn new(window_label: &str, options: TerminalOptions, cx: &mut Context<Self>) -> Self {
        let host = cx.global::<Backend>().host.clone();
        let (cols, rows) = (usize::from(options.cols), usize::from(options.rows));
        let (sender, mut chunks) = unbounded::<String>();
        let on_output: ide_host::ops::Emit<String> = Arc::new(move |chunk| {
            // The receiver is gone only when the view is.
            let _ = sender.unbounded_send(chunk);
        });
        // Starting a process needs the backend's runtime around it.
        let spawned = cx
            .global::<Backend>()
            .block_on(async { ide_host::ops::term::spawn(&host, window_label, options, on_output) });
        let output = cx.spawn(async move |view, cx| {
            while let Some(mut output) = chunks.next().await {
                // What has piled up is drawn in one go.
                while let Ok(more) = chunks.try_recv() {
                    output.push_str(&more);
                }
                if view.update(cx, |view: &mut TerminalView, cx| view.output(output.as_bytes(), cx)).is_err() {
                    break;
                }
            }
        });
        let weak = cx.weak_entity();
        let listener = cx.global_mut::<Backend>().listen(move |event, cx| {
            let _ = weak.update(cx, |view, cx| view.backend_event(event, cx));
        });
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |view, cx| view.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|view, cx| {
            if let Some(claim) = view.key_claim {
                input::release_keys(claim, cx);
            }
            if let Some(listener) = view.listener.filter(|_| cx.has_global::<Backend>()) {
                cx.global_mut::<Backend>().unlisten(listener);
            }
            // The shell does not outlive its terminal.
            if let Some(id) = view.id
                && let Err(error) = ide_host::ops::term::kill(&view.host, id)
            {
                tracing::debug!("terminal {id:?} was already gone: {}", error.message);
            }
        })
        .detach();
        let mut emulator = Emulator::new(cols, rows);
        emulator.set_palette(palette(cx));
        let (id, spawned_title, error) = match spawned {
            Ok(info) => (Some(info.id), info.title, None),
            Err(error) => (None, String::new(), Some(error.message)),
        };
        Self {
            focus,
            host,
            id,
            spawned_title,
            program_title: None,
            emulator,
            exit: None,
            error,
            grid: None,
            composing: None,
            selecting: false,
            listener: Some(listener),
            key_claim: Some(key_claim),
            _output: output,
        }
    }

    /// The backend's id of the session, while its shell runs.
    pub fn id(&self) -> Option<TerminalId> {
        self.id
    }

    /// What the tab shows: the name the program set, else the session's own.
    pub fn title(&self) -> &str {
        self.program_title.as_deref().unwrap_or(&self.spawned_title)
    }

    /// The session's own title: the shell's name.
    pub fn shell_title(&self) -> &str {
        &self.spawned_title
    }

    /// The title the running program set, if it set one.
    pub fn program_title(&self) -> Option<&str> {
        self.program_title.as_deref()
    }

    /// The exit code once the shell has ended (`Some(None)`: ended without one).
    pub fn exit(&self) -> Option<Option<i32>> {
        self.exit
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn emulator(&self) -> &Emulator {
        &self.emulator
    }

    /// Sends `data` to the program. Anything typed brings the view back to the present.
    pub fn write(&mut self, data: &str, cx: &mut Context<Self>) {
        let Some(id) = self.id else { return };
        self.emulator.scroll_to_bottom();
        self.emulator.clear_selection();
        if let Err(error) = ide_host::ops::term::write(&self.host, id, data) {
            tracing::warn!("cannot write to terminal {id:?}: {}", error.message);
        }
        cx.notify();
    }

    fn output(&mut self, output: &[u8], cx: &mut Context<Self>) {
        for effect in self.emulator.feed(output) {
            match effect {
                // An answer to the program is not typing: the view stays where it is scrolled.
                Effect::Write(answer) => {
                    if let Some(id) = self.id
                        && let Err(error) = ide_host::ops::term::write(&self.host, id, &answer)
                    {
                        tracing::warn!("cannot answer terminal {id:?}: {}", error.message);
                    }
                }
                Effect::Title(title) => {
                    self.program_title = title.filter(|title| !title.is_empty());
                    cx.emit(TerminalEvent::TitleChanged);
                }
                Effect::Bell => cx.emit(TerminalEvent::Bell),
                Effect::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            }
        }
        cx.notify();
    }

    fn backend_event(&mut self, event: &BackendEvent, cx: &mut Context<Self>) {
        let IdeEvent::TerminalExited { terminal, code } = &event.event else { return };
        if self.id != Some(*terminal) {
            return;
        }
        self.id = None;
        self.exit = Some(*code);
        let suffix = code.filter(|code| *code != 0).map(|code| format!(" (exit code {code})")).unwrap_or_default();
        let _ = self.emulator.feed(format!("\r\n\x1b[2m[Process completed{suffix}]\x1b[0m\r\n").as_bytes());
        cx.emit(TerminalEvent::Exited(*code));
        cx.notify();
    }

    /// Fits the grid to `cols` × `rows` cells of `cell` pixels and tells the PTY.
    fn fit(&mut self, cols: usize, rows: usize, cell: (Pixels, Pixels), cx: &mut Context<Self>) {
        let (cols, rows) = (cols.max(2), rows.max(1));
        if (cols, rows) == (self.emulator.cols(), self.emulator.rows()) {
            return;
        }
        let pixels = |v: Pixels| f32::from(v).round().clamp(0.0, f32::from(u16::MAX)) as u16;
        self.emulator.resize(cols, rows, (pixels(cell.0), pixels(cell.1)));
        if let Some(id) = self.id {
            let clamp = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
            if let Err(error) = ide_host::ops::term::resize(&self.host, id, clamp(cols), clamp(rows)) {
                tracing::warn!("cannot resize terminal {id:?}: {}", error.message);
            }
        }
        cx.notify();
    }

    /// Clears the terminal down to the line the cursor is on.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.emulator.clear();
        cx.notify();
    }

    /// Copies the selection; false when nothing is selected.
    pub fn copy(&self, cx: &mut Context<Self>) -> bool {
        let Some(text) = self.emulator.selected_text() else { return false };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    /// Types the clipboard's text into the program.
    pub fn paste(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()).filter(|text| !text.is_empty()) else {
            return false;
        };
        let pasted = encode_paste(&text, self.emulator.bracketed_paste());
        self.write(&pasted, cx);
        true
    }

    fn modes(&self, cx: &App) -> KeyModes {
        KeyModes {
            app_cursor: self.emulator.app_cursor(),
            option_as_meta: settings::global(cx).terminal.option_as_meta,
        }
    }

    /// The terminal's own keys, taken before the IDE's shortcuts: copy and paste, clear, the
    /// shell's line-editing strokes for ⌘ and ⌥ with the arrows, Ctrl with a letter by the letter
    /// it types, and Escape. With Settings › Terminal › Override IDE shortcuts off, Ctrl strokes
    /// and Escape go to the keymap first. True when the key was the terminal's.
    fn claim_key(&mut self, keystroke: &gpui_kit::Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        let mods =
            Mods { meta: modifiers.platform, ctrl: modifiers.control, alt: modifiers.alt, shift: modifiers.shift };
        let (named, code) = (dom_key(&keystroke.key, modifiers.shift), dom_code(&keystroke.key));
        let canonical = stroke(&input::key_name(&keystroke.key), mods);
        let terminal = settings::global(cx).terminal;
        let handling = {
            let event = KeyEvent { ime: keystroke.is_ime_in_progress(), ..KeyEvent::new(&named, &code, mods) };
            let context = KeyContext {
                stroke: Some(&canonical),
                focus: KeyFocus::Session { has_selection: self.emulator.selected_text().is_some() },
                override_ide_shortcuts: terminal.override_ide_shortcuts,
                escape_to_editor: terminal.escape_to_editor,
                bindings: cx.global::<Keymap>().0.effective_bindings(),
                mac: cfg!(target_os = "macos"),
            };
            route_terminal_key(&event, &context)
        };
        match handling {
            KeyHandling::Pass => return false,
            KeyHandling::Send(text) => self.write(&text, cx),
            KeyHandling::Copy => {
                self.copy(cx);
            }
            KeyHandling::Paste => {
                self.paste(cx);
            }
            KeyHandling::ClearSelection => {
                self.emulator.clear_selection();
                cx.notify();
            }
            KeyHandling::Clear => self.clear(cx),
            KeyHandling::RunAction(CLOSE_SESSION_ACTION) => cx.emit(TerminalEvent::CloseRequested),
            KeyHandling::RunAction(FIND_ACTION) => cx.emit(TerminalEvent::FindRequested),
            // The find bar's keys: there is no find bar in this view.
            KeyHandling::RunAction(_) | KeyHandling::CloseFind | KeyHandling::OpenFind => return false,
        }
        true
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let key = keystroke.key.as_str();
        // Any other ⌘ shortcut is the application's.
        if modifiers.platform {
            return;
        }
        // Shift with the paging keys looks back through the output instead of reaching the program.
        if modifiers.shift && !modifiers.control && !modifiers.alt && !self.emulator.alt_screen() {
            let page = self.emulator.rows().saturating_sub(1).max(1) as i32;
            let scrolled = match key {
                "pageup" => Some(page),
                "pagedown" => Some(-page),
                _ => None,
            };
            if let Some(lines) = scrolled {
                self.emulator.scroll_lines(lines);
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }
        let mods = KeyMods { shift: modifiers.shift, alt: modifiers.alt, ctrl: modifiers.control };
        if let Some(sequence) = encode_key(key, mods, self.modes(cx)) {
            self.write(&sequence, cx);
            cx.stop_propagation();
        }
    }

    /// The cell under a point of the window.
    fn cell_at(&self, position: Point<Pixels>) -> Option<(usize, usize)> {
        let grid = self.grid?;
        let col = ((position.x - grid.bounds.left()).max(px(0.0)) / grid.cell.0) as usize;
        let row = ((position.y - grid.bounds.top()).max(px(0.0)) / grid.cell.1) as usize;
        Some((row.min(self.emulator.rows() - 1), col.min(self.emulator.cols() - 1)))
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let Some((row, col)) = self.cell_at(event.position) else { return };
        let by = match event.click_count {
            0 | 1 => SelectBy::Cell,
            2 => SelectBy::Word,
            _ => SelectBy::Line,
        };
        if event.modifiers.shift {
            self.emulator.select_to(row, col);
        } else if by == SelectBy::Cell {
            // A plain click selects nothing until it is dragged.
            self.emulator.clear_selection();
        } else {
            self.emulator.select(row, col, by);
        }
        self.selecting = true;
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selecting || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let Some((row, col)) = self.cell_at(event.position) else { return };
        if self.emulator.selected_cols(row).is_none() && self.emulator.selected_text().is_none() {
            self.emulator.select(row, col, SelectBy::Cell);
        }
        self.emulator.select_to(row, col);
        cx.notify();
    }

    fn scrolled(&mut self, lines: i32, cx: &mut Context<Self>) {
        if lines == 0 {
            return;
        }
        // A full-screen program has no scrollback: the wheel moves its own cursor instead.
        if self.emulator.alt_screen() {
            let key = if lines > 0 { "up" } else { "down" };
            if let Some(sequence) = encode_key(key, KeyMods::default(), self.modes(cx)) {
                self.write(&sequence.repeat(lines.unsigned_abs() as usize), cx);
            }
        } else {
            self.emulator.scroll_lines(lines);
            cx.notify();
        }
    }

    /// The terminal's font and its size: the editor's, or the one set for the terminal.
    fn font(cx: &App) -> (Font, Pixels, f32) {
        let terminal = settings::global(cx).terminal;
        let installed = cx.text_system().all_font_names();
        let named =
            (!terminal.use_editor_font && installed.contains(&terminal.font_family)).then_some(terminal.font_family);
        let family = named.unwrap_or_else(|| {
            let families =
                cx.try_global::<theme::ActiveTheme>().map(|t| t.theme.font_families("font-mono")).unwrap_or_default();
            families.into_iter().find(|family| installed.contains(family)).unwrap_or_else(|| "Menlo".to_owned())
        });
        (
            font(SharedString::from(family)),
            px(terminal.font_size.clamp(6.0, 72.0)),
            terminal.line_height.clamp(0.8, 3.0),
        )
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: 0..0, reversed: false })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.composing.as_ref().map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.composing = None;
        cx.notify();
    }

    fn replace_text_in_range(&mut self, _: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        self.composing = None;
        self.write(text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        new_text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A composition is only shown; the program gets the text once it is committed.
        self.composing = (!new_text.is_empty()).then(|| new_text.to_owned());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let (grid, cursor) = (self.grid?, self.emulator.cursor()?);
        let origin = point(
            grid.bounds.left() + grid.cell.0 * cursor.col as f32,
            grid.bounds.top() + grid.cell.1 * cursor.row as f32,
        );
        Some(Bounds::new(origin, size(grid.cell.0, grid.cell.1)))
    }

    fn character_index_for_point(&mut self, _: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        None
    }
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The theme may have changed since the last frame.
        let colours = palette(cx);
        if self.emulator.palette() != &colours {
            self.emulator.set_palette(colours);
        }
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(|view, _, _, _| view.selecting = false))
            .child(TerminalElement { view: cx.entity() })
    }
}

/// Paints a [`TerminalView`]'s grid.
struct TerminalElement {
    view: Entity<TerminalView>,
}

impl IntoElement for TerminalElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

struct Metrics {
    font: Font,
    font_size: Pixels,
    cell: (Pixels, Pixels),
    ascent_gap: Pixels,
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Metrics;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Metrics {
        let (font, font_size, line_height) = TerminalView::font(cx);
        let run = TextRun {
            len: 1,
            font: font.clone(),
            color: Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        // Every cell is as wide as the font's `M` and as tall as its line, times the setting.
        let measured = window.text_system().shape_line("M".into(), font_size, &[run], None);
        let natural = measured.ascent + measured.descent;
        let cell = (measured.width.max(px(1.0)), (natural * line_height).ceil().max(px(1.0)));
        let cols = (bounds.size.width / cell.0).floor() as usize;
        let rows = (bounds.size.height / cell.1).floor() as usize;
        self.view.update(cx, |view, cx| {
            view.grid = Some(Grid { bounds, cell });
            view.fit(cols, rows, cell, cx);
        });
        Metrics { font, font_size, cell, ascent_gap: (cell.1 - natural) / 2.0 }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        metrics: &mut Metrics,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.view.read(cx).focus.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.view.clone()), cx);
        let (view, cell_height) = (self.view.clone(), metrics.cell.1);
        window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && bounds.contains(&event.position) {
                // Scrolling the content down shows older output.
                let lines = (event.delta.pixel_delta(cell_height).y / cell_height).round() as i32;
                view.update(cx, |view, cx| view.scrolled(lines, cx));
                window.refresh();
            }
        });

        let focused = focus.is_focused(window);
        let view = self.view.read(cx);
        let emulator = &view.emulator;
        let palette = emulator.palette().clone();
        let (cell_width, cell_height) = metrics.cell;
        let origin_of = |row: usize, col: usize| {
            point(bounds.left() + cell_width * col as f32, bounds.top() + cell_height * row as f32)
        };
        let rows = emulator.runs();
        let selections: Vec<Option<Range<usize>>> = (0..rows.len()).map(|row| emulator.selected_cols(row)).collect();
        let cursor = emulator.cursor();
        let composing = view.composing.clone();
        let selection_colour = theme::color(cx, "bg-editor-selection");

        window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, hsla(palette.background)));
            for (row, runs) in rows.iter().enumerate() {
                for run in runs {
                    if let Some(background) = run.bg {
                        let area =
                            Bounds::new(origin_of(row, run.col), size(cell_width * run.cells as f32, cell_height));
                        window.paint_quad(fill(area, hsla(background)));
                    }
                }
                if let Some(selected) = &selections[row] {
                    let area = Bounds::new(
                        origin_of(row, selected.start),
                        size(cell_width * (selected.end - selected.start) as f32, cell_height),
                    );
                    window.paint_quad(fill(area, selection_colour));
                }
                for run in runs.iter().filter(|run| !run.text.trim().is_empty() || run.underline || run.strikeout) {
                    let colour = hsla(run.fg);
                    let line =
                        |thickness: f32| UnderlineStyle { thickness: px(thickness), color: Some(colour), wavy: false };
                    let text_run = TextRun {
                        len: run.text.len(),
                        font: Font {
                            weight: if run.bold { FontWeight::BOLD } else { metrics.font.weight },
                            style: if run.italic { FontStyle::Italic } else { metrics.font.style },
                            ..metrics.font.clone()
                        },
                        color: colour,
                        background_color: None,
                        underline: run.underline.then(|| line(1.0)),
                        strikethrough: run
                            .strikeout
                            .then(|| gpui_kit::StrikethroughStyle { thickness: px(1.0), color: Some(colour) }),
                    };
                    let shaped =
                        window.text_system().shape_line(run.text.clone().into(), metrics.font_size, &[text_run], None);
                    let at = origin_of(row, run.col) + point(px(0.0), metrics.ascent_gap);
                    let _ = shaped.paint(at, cell_height - metrics.ascent_gap * 2.0, TextAlign::Left, None, window, cx);
                }
            }
            if let Some(cursor) = cursor {
                let cell = Bounds::new(origin_of(cursor.row, cursor.col), size(cell_width, cell_height));
                let colour = hsla(palette.cursor);
                if let Some(composing) = &composing {
                    let text_run = TextRun {
                        len: composing.len(),
                        font: metrics.font.clone(),
                        color: hsla(palette.foreground),
                        background_color: Some(hsla(palette.background)),
                        underline: Some(UnderlineStyle { thickness: px(1.0), color: Some(colour), wavy: false }),
                        strikethrough: None,
                    };
                    let shaped =
                        window.text_system().shape_line(composing.clone().into(), metrics.font_size, &[text_run], None);
                    let at = cell.origin + point(px(0.0), metrics.ascent_gap);
                    let _ = shaped.paint_background(at, cell_height, TextAlign::Left, None, window, cx);
                    let _ = shaped.paint(at, cell_height, TextAlign::Left, None, window, cx);
                } else if !focused {
                    window.paint_quad(outline(cell, colour, gpui_kit::BorderStyle::Solid));
                } else {
                    let shape = match cursor.shape {
                        CursorShape::Underline => {
                            Bounds::new(point(cell.left(), cell.bottom() - px(2.0)), size(cell_width, px(2.0)))
                        }
                        CursorShape::Beam => Bounds::new(cell.origin, size(px(2.0), cell_height)),
                        _ => cell,
                    };
                    let mut block = colour;
                    if shape == cell {
                        // The character under a block cursor stays readable through it.
                        block.a = 0.6;
                    }
                    window.paint_quad(fill(shape, block));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AppDirs;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::collections::BTreeMap;
    use std::time::Duration;

    fn settle(cx: &mut VisualTestContext, what: &str, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..3000 {
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{what}: not within thirty seconds");
    }

    fn screen(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, _| view.emulator.lines().join("\n"))
    }

    /// A real shell on the backend's PTY: what is typed reaches it, what it prints is on the
    /// grid, the grid is sized to the view and the shell knows, and its end is reported with
    /// the exit code.
    #[gpui_kit::test]
    async fn a_shell_runs_in_the_terminal_and_reports_its_exit(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            crate::terminal::test_support::isolate_shells(home.path(), cx);
        });
        let options = TerminalOptions {
            cwd: ide_core::IpcPath(folder.path().to_path_buf()),
            shell: None,
            args: Vec::new(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
            shell_integration: false,
        };
        let (view, cx) = cx.add_window_view(|_, cx| TerminalView::new("w1", options, cx));
        assert_eq!(view.read_with(cx, |view, _| view.error().map(str::to_owned)), None);
        let events: std::rc::Rc<std::cell::RefCell<Vec<TerminalEvent>>> = Default::default();
        let log = events.clone();
        let _subscription = cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &TerminalEvent, _| log.borrow_mut().push(event.clone()))
        });
        cx.update(|window, cx| {
            let focus = view.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        // The grid takes the size the view was painted at, not the one the session was asked for.
        settle(cx, "the grid is fitted", |cx| view.read_with(cx, |view, _| view.grid.is_some()));
        let (cols, rows) = view.read_with(cx, |view, _| (view.emulator.cols(), view.emulator.rows()));
        assert!(cols >= 2 && rows >= 1 && (cols, rows) != (80, 24), "{cols}x{rows}");

        // Typed text and Enter go to the shell; its answer is drawn. `stty size` is the shell's
        // own idea of the grid.
        cx.simulate_input("echo marker-$((6*7)); stty size");
        cx.simulate_keystrokes("enter");
        settle(cx, "the shell answers", |cx| screen(&view, cx).contains("marker-42"));
        settle(cx, "the shell knows the size", |cx| screen(&view, cx).contains(&format!("{rows} {cols}")));

        // Pasting types the clipboard's text.
        cx.write_to_clipboard(ClipboardItem::new_string("echo pasted-$((40+2))".to_owned()));
        cx.simulate_keystrokes("cmd-v enter");
        settle(cx, "the paste ran", |cx| screen(&view, cx).contains("pasted-42"));

        // ⌘K clears the terminal down to the prompt's line.
        cx.simulate_keystrokes("cmd-k");
        settle(cx, "the screen is cleared", |cx| {
            !screen(&view, cx).contains("pasted-42") && view.read_with(cx, |view, _| view.emulator.history() == 0)
        });
        // Ctrl+C reaches the program as an interrupt: the shell is back for the next command.
        cx.simulate_input("sleep 30");
        cx.simulate_keystrokes("enter ctrl-c");
        cx.simulate_input("echo back-$((1+1))");
        cx.simulate_keystrokes("enter");
        settle(cx, "the interrupt ended the sleep", |cx| screen(&view, cx).contains("back-2"));

        cx.simulate_input("exit 3");
        cx.simulate_keystrokes("enter");
        settle(cx, "the exit is reported", |cx| view.read_with(cx, |view, _| view.exit().is_some()));
        assert_eq!(view.read_with(cx, |view, _| (view.exit(), view.id())), (Some(Some(3)), None));
        assert!(screen(&view, cx).contains("[Process completed (exit code 3)]"));
        assert!(events.borrow().contains(&TerminalEvent::Exited(Some(3))));
    }

    #[gpui_kit::test]
    async fn the_palette_is_the_themes(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert_eq!(palette(cx), Palette::default(), "without a theme, xterm's colours");
            cx.set_global(theme::ActiveTheme::default());
            let light = palette(cx);
            let hex = |c: Rgb| format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b);
            let theme = &cx.global::<theme::ActiveTheme>().theme;
            let token = |name: &str| theme.color(name).map(|c| c.to_hex());
            assert_eq!(Some(hex(light.ansi[1])), token("terminal-ansi-red"));
            assert_eq!(Some(hex(light.ansi[9])), token("terminal-ansi-bright-red"));
            assert_eq!(Some(hex(light.background)), token("bg-island"));
            assert_eq!(Some(hex(light.foreground)), token("fg-editor"));
            assert_eq!(Some(hex(light.cursor)), token("caret"));
        });
    }
}
