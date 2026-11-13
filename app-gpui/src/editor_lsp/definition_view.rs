//! Quick Definition (⌥Space) and Quick Type Definition: the source of the definition at the caret in
//! a popup, without leaving the editor. With several definitions ← / → switch between them; F4 (or
//! the Edit Source button) opens the shown one; Escape closes the popup.

use super::files::target_label;
use super::hover::definition_span;
use super::hover_view::{code_font, popup_surface, tx};
use crate::platform::input;
use crate::platform::virtual_docs::basename;
use crate::theme;
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, MouseButton, Render, SharedString,
    Window, div, prelude::*, px,
};
use ide_core::Location;
use ide_core::ipc_path::to_ipc;

/// At most this many definitions are read and shown.
pub const MAX_TARGETS: usize = 20;

/// A definition and the lines it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionTarget {
    pub location: Location,
    /// The name at the location, else the file's name.
    pub name: String,
    /// The 0-based line of the first line shown.
    pub first_line: u32,
    pub lines: Vec<String>,
}

impl DefinitionTarget {
    /// The definition at `location` in a file of `text`, or `None` when the location lies past its
    /// end. The lines shown are a window around the name: the declaration's extent is not known.
    pub fn read(location: Location, text: &str) -> Option<Self> {
        let lines: Vec<&str> = text.lines().collect();
        let start = location.range.start;
        let line = *lines.get(start.line as usize)?;
        let (first, last) = definition_span(&location, lines.len() as u32, None);
        let name = target_label(line, start.col).unwrap_or_else(|| basename(&to_ipc(&location.path)).into_owned());
        let shown = lines[first as usize..=last as usize].iter().map(|line| (*line).to_owned()).collect();
        Some(Self { location, name, first_line: first, lines: shown })
    }
}

/// What the popup asks of its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefinitionEvent {
    /// F4 or Edit Source: close the popup and open the definition, the caret at its start.
    Open(Location),
    /// Escape or a click beside it: close the popup.
    Closed,
}

/// The definition popup, shown in the workbench's popup slot: it takes the keyboard.
pub struct DefinitionPopup {
    targets: Vec<DefinitionTarget>,
    index: usize,
    /// The project root paths are shown relative to.
    root: String,
    focus: FocusHandle,
    key_claim: u64,
}

impl EventEmitter<DefinitionEvent> for DefinitionPopup {}

impl DefinitionPopup {
    /// The popup over `targets` (not empty), the first one shown.
    pub fn new(targets: Vec<DefinitionTarget>, root: &str, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |popup, cx| popup.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|popup, cx| input::release_keys(popup.key_claim, cx)).detach();
        Self { targets, index: 0, root: root.to_owned(), focus, key_claim }
    }

    /// The definition shown, an index into the targets.
    pub fn index(&self) -> usize {
        self.index
    }

    pub fn targets(&self) -> &[DefinitionTarget] {
        &self.targets
    }

    /// The header: the name (with several definitions) or the file's name, where it is
    /// (`path:line`, relative to the project), and `n/count` with several.
    pub fn header(&self) -> Vec<String> {
        let Some(target) = self.targets.get(self.index) else { return Vec::new() };
        let path = to_ipc(&target.location.path);
        let relative = path.strip_prefix(&self.root).map_or(path.as_str(), |rest| rest.trim_start_matches('/'));
        let line = target.location.range.start.line + 1;
        let mut header = vec![
            if self.targets.len() > 1 { target.name.clone() } else { basename(&path).into_owned() },
            format!("{relative}:{line}"),
        ];
        if self.targets.len() > 1 {
            header.push(format!("{}/{}", self.index + 1, self.targets.len()));
        }
        header
    }

    /// The source lines shown, each after its 1-based line number.
    pub fn lines(&self) -> Vec<String> {
        let Some(target) = self.targets.get(self.index) else { return Vec::new() };
        let first = target.first_line + 1;
        target.lines.iter().enumerate().map(|(at, line)| format!("{}  {line}", first + at as u32)).collect()
    }

    fn select(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.targets.len() as isize;
        if count > 1 {
            self.index = (self.index as isize + delta).rem_euclid(count) as usize;
            cx.notify();
        }
    }

    fn open(&mut self, cx: &mut Context<Self>) {
        if let Some(target) = self.targets.get(self.index) {
            cx.emit(DefinitionEvent::Open(target.location.clone()));
        }
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt || modifiers.shift {
            return false;
        }
        match keystroke.key.as_str() {
            "escape" => cx.emit(DefinitionEvent::Closed),
            "f4" => self.open(cx),
            "left" if self.targets.len() > 1 => self.select(-1, cx),
            "right" if self.targets.len() > 1 => self.select(1, cx),
            _ => return false,
        }
        true
    }
}

impl Focusable for DefinitionPopup {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for DefinitionPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.header();
        let mono = code_font(cx);
        let muted = theme::color(cx, "fg-muted");
        let strong = theme::color(cx, "fg-strong");
        let hovered = theme::color(cx, "bg-hover");
        let mut title = div().flex().items_center().gap(px(8.0)).px(px(8.0)).h(px(28.0));
        for (at, part) in header.into_iter().enumerate() {
            let part = div().child(SharedString::from(part));
            title = title.child(part.text_color(if at == 0 { strong } else { muted }));
        }
        let edit = div()
            .id("definition-edit-source")
            .debug_selector(|| "definition-edit-source".to_owned())
            .px(px(6.0))
            .rounded(theme::length(cx, "radius-small"))
            .hover(move |style| style.bg(hovered))
            .child(SharedString::from(format!("{} (F4)", tx(cx, "Edit Source"))))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(cx.listener(|popup, _, _, cx| popup.open(cx)));
        let body = div()
            .id("definition-body")
            .max_h(px(360.0))
            .overflow_y_scroll()
            .px(px(8.0))
            .py(px(4.0))
            .font_family(mono.family)
            .text_size(theme::length(cx, "fs-code"))
            .children(self.lines().into_iter().map(|line| div().whitespace_nowrap().child(SharedString::from(line))));
        popup_surface(cx)
            .debug_selector(|| "definition-popup".to_owned())
            .track_focus(&self.focus)
            .w(px(560.0))
            .flex()
            .flex_col()
            .text_size(theme::length(cx, "fs-ui"))
            .child(title.child(div().flex_1()).child(edit))
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use ide_core::{Pos, Range};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<DefinitionEvent>>>;

    fn location(path: &str, line: u32, col: u32) -> Location {
        Location { path: path.into(), range: Range::new(Pos::new(line, col), Pos::new(line, col + 3)), uri: None }
    }

    fn targets() -> Vec<DefinitionTarget> {
        let a = DefinitionTarget::read(location("/p/a.ts", 1, 9), "// a\nfunction run() {}\n").expect("in the file");
        let b = DefinitionTarget::read(location("/p/src/b.ts", 0, 2), "  run(): void;\n").expect("in the file");
        vec![a, b]
    }

    fn open(
        cx: &mut TestAppContext,
        targets: Vec<DefinitionTarget>,
    ) -> (Entity<DefinitionPopup>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(move |_, cx| DefinitionPopup::new(targets, "/p", cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &DefinitionEvent, _| seen.borrow_mut().push(event.clone())).detach();
            window.activate_window();
            window.focus(&view.focus_handle(cx), cx);
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    #[test]
    fn reads_the_lines_around_the_name_and_its_label() {
        let target = DefinitionTarget::read(location("/p/a.ts", 1, 9), "// a\nfunction run() {}\n").expect("read");
        assert_eq!((target.name.as_str(), target.first_line), ("run", 0));
        assert_eq!(target.lines, ["// a", "function run() {}"]);
        assert!(DefinitionTarget::read(location("/p/a.ts", 9, 0), "one line\n").is_none());
    }

    #[gpui_kit::test]
    async fn switches_between_several_definitions_with_left_right_and_edits_the_source_with_f4(
        cx: &mut TestAppContext,
    ) {
        let (view, events, cx) = open(cx, targets());
        assert_eq!(view.read_with(cx, |view, _| view.header()), ["run", "a.ts:2", "1/2"]);
        assert_eq!(view.read_with(cx, |view, _| view.lines()), ["1  // a", "2  function run() {}"]);
        cx.simulate_keystrokes("right");
        assert_eq!(view.read_with(cx, |view, _| view.header()), ["run", "src/b.ts:1", "2/2"]);
        cx.simulate_keystrokes("right");
        assert_eq!(view.read_with(cx, |view, _| view.index()), 0, "the arrows wrap");
        cx.simulate_keystrokes("left f4");
        assert_eq!(events.borrow().last(), Some(&DefinitionEvent::Open(location("/p/src/b.ts", 0, 2))));
        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().last(), Some(&DefinitionEvent::Closed));
    }

    #[gpui_kit::test]
    async fn one_definition_shows_its_file_and_edit_source_opens_it(cx: &mut TestAppContext) {
        let single = targets().into_iter().take(1).collect();
        let (view, events, cx) = open(cx, single);
        assert_eq!(view.read_with(cx, |view, _| view.header()), ["a.ts", "a.ts:2"]);
        cx.simulate_keystrokes("right");
        assert_eq!(view.read_with(cx, |view, _| view.index()), 0);
        let bounds = cx.debug_bounds("definition-edit-source").expect("the button is drawn");
        cx.simulate_click(bounds.center(), Modifiers::default());
        assert_eq!(*events.borrow(), [DefinitionEvent::Open(location("/p/a.ts", 1, 9))]);
    }
}
