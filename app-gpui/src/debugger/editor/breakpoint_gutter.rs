//! Breakpoints in the editor: markers on the line-number gutter, click to toggle, right-click for
//! the properties popover, and line tracking through edits.

use crate::debugger::breakpoints::code_lines::line_has_code;
use crate::debugger::breakpoints::model::{
    BreakpointEntry, BreakpointProps, BreakpointStatus, is_conditional, map_line_numbers, map_lines,
};
use crate::debugger::breakpoints::special::{SpecialBreakpoint, SpecialBreakpoints, SpecialKind, SpecialPatch};
use crate::debugger::breakpoints::store::BreakpointStore;
use crate::debugger::breakpoints::toggle::toggle_breakpoint_smart;
use crate::editor::buffer;
use crate::platform::virtual_docs::class_file_breakpoint_reason;
use ide_core::TextChange;
use ropey::Rope;
use std::ops::Range;

/// How a breakpoint's marker is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakpointLook {
    Normal,
    Disabled,
    Muted,
    Unverified,
    Verified,
    Conditional,
    Log,
    Method,
    MethodDisabled,
    MethodVerified,
    Field,
    FieldDisabled,
    FieldVerified,
}

impl BreakpointLook {
    /// Whether the whole line is tinted, as IDEA does for an active breakpoint.
    pub fn tints_line(self) -> bool {
        !matches!(self, Self::Disabled | Self::Muted | Self::MethodDisabled | Self::FieldDisabled)
    }
}

/// The look of a line breakpoint. `running`: a debug session is live (only then does a breakpoint
/// show as bound or not).
pub fn look_of(b: &BreakpointEntry, status: Option<&BreakpointStatus>, muted: bool, running: bool) -> BreakpointLook {
    if !b.enabled {
        return BreakpointLook::Disabled;
    }
    // Mute Breakpoints: IDEA's grey dot, so it isn't taken for a disabled breakpoint.
    if muted {
        return BreakpointLook::Muted;
    }
    let verified = status.filter(|_| running).map(|status| status.verified);
    if verified == Some(false) {
        BreakpointLook::Unverified
    } else if !b.suspend {
        BreakpointLook::Log
    } else if is_conditional(b) {
        BreakpointLook::Conditional
    } else if verified == Some(true) {
        BreakpointLook::Verified
    } else {
        BreakpointLook::Normal
    }
}

/// The look of a method breakpoint or field watchpoint.
pub fn special_look(
    b: &SpecialBreakpoint,
    status: Option<&BreakpointStatus>,
    muted: bool,
    running: bool,
) -> BreakpointLook {
    let method = b.kind() == SpecialKind::Method;
    if !b.enabled() {
        return if method { BreakpointLook::MethodDisabled } else { BreakpointLook::FieldDisabled };
    }
    if muted {
        return BreakpointLook::Muted;
    }
    match status.filter(|_| running).map(|status| status.verified) {
        Some(false) => BreakpointLook::Unverified,
        Some(true) if method => BreakpointLook::MethodVerified,
        Some(true) => BreakpointLook::FieldVerified,
        None if method => BreakpointLook::Method,
        None => BreakpointLook::Field,
    }
}

/// A marker on the line-number gutter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub line: u32,
    /// The breakpoint's id.
    pub id: String,
    pub look: BreakpointLook,
}

/// The markers of the editor showing `path`, ordered by line. A method breakpoint or watchpoint
/// takes its line; a line breakpoint on the same line shows no marker of its own.
pub fn build_markers(
    breakpoints: &BreakpointStore,
    special: &SpecialBreakpoints,
    path: &str,
    line_count: u32,
    running: bool,
) -> Vec<Marker> {
    let muted = breakpoints.muted();
    let mut markers: Vec<Marker> = special
        .special_at(path)
        .iter()
        .filter_map(|b| {
            let line = b.line().filter(|line| *line < line_count)?;
            Some(Marker { line, id: b.id().to_owned(), look: special_look(b, special.status(b.id()), muted, running) })
        })
        .collect();
    let lines = breakpoints.breakpoints_of(path).iter().filter(|b| b.line < line_count);
    let plain: Vec<Marker> = lines
        .filter(|b| !markers.iter().any(|taken| taken.line == b.line))
        .map(|b| {
            let look = look_of(b, breakpoints.status(&b.id), muted, running);
            Marker { line: b.line, id: b.id.clone(), look }
        })
        .collect();
    markers.extend(plain);
    markers.sort_by_key(|marker| marker.line);
    markers
}

/// Where a document change came from, for the editors that mirror one document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChangeOrigin {
    /// Mirrored from another pane showing the same document.
    pub mirrored: bool,
    /// Applied by the backend to every pane (a refactoring, a reload).
    pub remote: bool,
    /// For a remote change: this editor is the one of the file's editors that moves the stored
    /// lines (the active one when it shows the file, else the first).
    pub writer: bool,
}

/// An editor of `path` changed its document: the originating editor moves the stored lines with
/// the text. Returns false when this editor only has to redraw its markers.
pub fn document_changed(
    breakpoints: &mut BreakpointStore,
    special: &mut SpecialBreakpoints,
    path: &str,
    edit: (&Rope, &[TextChange], &Rope),
    origin: ChangeOrigin,
    now_ms: f64,
) -> bool {
    if origin.mirrored || (origin.remote && !origin.writer) {
        return false;
    }
    let (old_doc, changes, new_doc) = edit;
    if !breakpoints.breakpoints_of(path).is_empty() {
        let moved = map_lines(breakpoints.breakpoints_of(path), old_doc, changes, new_doc);
        breakpoints.set_file_breakpoints(path, moved, now_ms);
    }
    let placed: Vec<(String, u32)> =
        special.special_at(path).iter().filter_map(|b| Some((b.id().to_owned(), b.line()?))).collect();
    if !placed.is_empty() {
        let lines: Vec<u32> = placed.iter().map(|(_, line)| *line).collect();
        let moved: Vec<(String, u32)> = map_line_numbers(&lines, old_doc, changes, new_doc)
            .into_iter()
            .map(|(index, line)| (placed[index].0.clone(), line))
            .collect();
        special.set_special_lines(path, &moved, now_ms);
    }
    true
}

/// Why the gutter of `path` takes no breakpoints (an output class without source), as the hover
/// text of the gutter and the status bar message of a click there.
pub fn gutter_blocked(path: &str) -> Option<&'static str> {
    class_file_breakpoint_reason(path)
}

/// A mouse button the gutter acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GutterButton {
    Primary,
    Middle,
}

/// A press or release in the breakpoint area of the gutter: the line numbers and the empty space
/// of the other gutters left of the fold column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GutterPress {
    pub button: GutterButton,
    pub shift: bool,
    pub alt: bool,
}

/// The editor a gutter belongs to.
#[derive(Debug, Clone, Copy)]
pub struct GutterEditor<'a> {
    pub path: &'a str,
    pub language: &'a str,
    pub doc: &'a Rope,
    /// The document's comment nodes as UTF-16 offset ranges (none without a parser).
    pub comments: &'a [Range<u32>],
}

/// What a press in the breakpoint area came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressOutcome {
    /// Nothing for the gutter; the press goes where it would have.
    Ignored,
    /// The file takes no breakpoints: show the message in the status bar.
    Blocked(&'static str),
    /// Middle click: the breakpoint on the line was enabled or disabled.
    Toggled,
    /// The press is the gutter's: a release over the same line is a click ([`gutter_click`]); a
    /// drag across the gutter selects lines instead.
    AwaitRelease,
}

fn on_line(breakpoints: &BreakpointStore, special: &SpecialBreakpoints, path: &str, line: u32) -> Existing {
    match (special.special_on_line(path, line), breakpoints.at_line(path, line)) {
        (Some(special), _) => Existing::Special(special.id().to_owned(), special.enabled()),
        (None, Some(bp)) => Existing::Line(bp.id.clone(), bp.enabled),
        (None, None) => Existing::None,
    }
}

/// The breakpoint a gutter line has: its id and whether it is enabled.
enum Existing {
    None,
    Special(String, bool),
    Line(String, bool),
}

/// A button went down on `line` of the breakpoint area. `can_place`: new breakpoints may go into
/// files of the editor's language (`DebuggableLanguages::can_place_breakpoint`).
pub fn gutter_mouse_down(
    breakpoints: &mut BreakpointStore,
    special: &mut SpecialBreakpoints,
    editor: &GutterEditor,
    line: u32,
    press: GutterPress,
    can_place: bool,
    now_ms: f64,
) -> PressOutcome {
    if let Some(reason) = gutter_blocked(editor.path) {
        return PressOutcome::Blocked(reason);
    }
    let existing = on_line(breakpoints, special, editor.path, line);
    if press.button == GutterButton::Middle {
        match existing {
            Existing::None => return PressOutcome::Ignored,
            Existing::Special(id, enabled) => {
                special.update_special(&id, SpecialPatch { enabled: Some(!enabled), ..Default::default() }, now_ms);
            }
            Existing::Line(id, enabled) => {
                breakpoints.update_breakpoint(editor.path, &id, now_ms, |b| b.enabled = !enabled);
            }
        }
        return PressOutcome::Toggled;
    }
    if !can_place && breakpoints.at_line(editor.path, line).is_none() {
        return PressOutcome::Ignored;
    }
    PressOutcome::AwaitRelease
}

/// What a click on a gutter line came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickOutcome {
    /// Nothing changed (a modifier click on an existing breakpoint, a line without code).
    Nothing,
    /// A breakpoint was added or removed.
    Changed,
    /// A logging breakpoint was added: open its properties at the pointer.
    OpenProperties { id: String },
}

/// IDEA's gutter clicks: a click toggles; on a line without a breakpoint ⇧-click adds a
/// non-suspending logging breakpoint and opens its properties, ⌥-click a temporary one. Blank and
/// comment-only lines take none. On an existing breakpoint only a plain click removes it.
pub fn gutter_click(
    breakpoints: &mut BreakpointStore,
    special: &mut SpecialBreakpoints,
    editor: &GutterEditor,
    line: u32,
    press: GutterPress,
    now_ms: f64,
) -> ClickOutcome {
    let path = editor.path;
    match on_line(breakpoints, special, path, line) {
        Existing::None => {}
        _ if press.shift || press.alt => return ClickOutcome::Nothing,
        Existing::Special(id, _) => {
            special.remove_special(&id, now_ms);
            return ClickOutcome::Changed;
        }
        Existing::Line(id, _) => {
            breakpoints.remove_breakpoint(path, &id, now_ms);
            return ClickOutcome::Changed;
        }
    }
    if !line_has_code(editor.doc, line, editor.comments) {
        return ClickOutcome::Nothing;
    }
    if press.alt {
        breakpoints.add_breakpoint(path, line, BreakpointProps { temporary: true, ..Default::default() }, now_ms);
        return ClickOutcome::Changed;
    }
    if press.shift {
        let props = BreakpointProps { suspend: Some(false), log_hit: true, ..Default::default() };
        let created = breakpoints.add_breakpoint(path, line, props, now_ms);
        return ClickOutcome::OpenProperties { id: created.id };
    }
    toggle_breakpoint_smart(breakpoints, special, path, line, editor.language, &editor.doc.to_string(), now_ms);
    ClickOutcome::Changed
}

/// A right-click on `line` of the breakpoint area: the id of the breakpoint whose properties
/// open. Other lines get the editor's gutter menu.
pub fn gutter_context_menu(
    breakpoints: &BreakpointStore,
    special: &SpecialBreakpoints,
    path: &str,
    line: u32,
) -> Option<String> {
    match on_line(breakpoints, special, path, line) {
        Existing::None => None,
        Existing::Special(id, _) | Existing::Line(id, _) => Some(id),
    }
}

/// How many lines the gutter of `doc` has.
pub fn gutter_lines(doc: &Rope) -> u32 {
    buffer::line_count(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debugger::breakpoints::model::testing::edit;
    use ide_dap::BreakpointState;

    const PATH: &str = "/proj/a.py";
    const PLAIN: GutterPress = GutterPress { button: GutterButton::Primary, shift: false, alt: false };
    const SHIFT: GutterPress = GutterPress { shift: true, ..PLAIN };
    const ALT: GutterPress = GutterPress { alt: true, ..PLAIN };
    const MIDDLE: GutterPress = GutterPress { button: GutterButton::Middle, ..PLAIN };

    struct Fixture {
        breakpoints: BreakpointStore,
        special: SpecialBreakpoints,
        doc: Rope,
    }

    impl Fixture {
        fn new(text: &str) -> Self {
            let mut fixture = Self {
                breakpoints: BreakpointStore::default(),
                special: SpecialBreakpoints::default(),
                doc: Rope::from_str(text),
            };
            fixture.breakpoints.load(Some("/proj"), None, 0.0);
            fixture.special.load(Some("/proj"), None, 0.0);
            fixture
        }

        /// A press and a release on the first line.
        fn click(&mut self, path: &str, language: &str, press: GutterPress) -> PressOutcome {
            let editor = GutterEditor { path, language, doc: &self.doc, comments: &[] };
            let down = gutter_mouse_down(&mut self.breakpoints, &mut self.special, &editor, 0, press, true, 0.0);
            if down == PressOutcome::AwaitRelease {
                gutter_click(&mut self.breakpoints, &mut self.special, &editor, 0, press, 0.0);
            }
            down
        }

        fn markers(&self, running: bool) -> Vec<Marker> {
            build_markers(&self.breakpoints, &self.special, PATH, gutter_lines(&self.doc), running)
        }

        fn list(&self) -> &[BreakpointEntry] {
            self.breakpoints.breakpoints_of(PATH)
        }
    }

    // src/features/debugger/__tests__/editor.test.ts › breakpoint gutter
    #[test]
    fn renders_markers_on_line_numbers_and_tracks_edits() {
        let mut fixture = Fixture::new("a\nb\nc\nd\ne");
        fixture.breakpoints.toggle_breakpoint(PATH, 2, 0.0);
        let markers = fixture.markers(false);
        assert_eq!(markers.iter().map(|m| (m.line, m.look)).collect::<Vec<_>>(), [(2, BreakpointLook::Normal)]);

        let (changes, next) = edit(&fixture.doc, 0, 0, "new\n");
        let old = std::mem::replace(&mut fixture.doc, next.clone());
        let origin = ChangeOrigin::default();
        let edit = (&old, changes.as_slice(), &next);
        assert!(document_changed(&mut fixture.breakpoints, &mut fixture.special, PATH, edit, origin, 0.0));
        assert_eq!(fixture.list().iter().map(|b| b.line).collect::<Vec<_>>(), [3]);

        let id = fixture.list()[0].id.clone();
        fixture.breakpoints.update_breakpoint(PATH, &id, 0.0, |b| b.enabled = false);
        let markers = fixture.markers(false);
        assert_eq!((markers[0].line, markers[0].look), (3, BreakpointLook::Disabled));
        assert!(!markers[0].look.tints_line());
    }

    // src/features/debugger/__tests__/editor.test.ts › breakpoint gutter redraws
    #[test]
    fn follows_a_session_starting_and_ending() {
        let mut fixture = Fixture::new("a\nb");
        fixture.breakpoints.toggle_breakpoint(PATH, 0, 0.0);
        fixture.breakpoints.apply_states(PATH, &[BreakpointState { line: 0, verified: true, message: None }]);
        assert_eq!(fixture.markers(false)[0].look, BreakpointLook::Normal);
        assert_eq!(fixture.markers(true)[0].look, BreakpointLook::Verified);
        assert_eq!(fixture.markers(false)[0].look, BreakpointLook::Normal);
    }

    // src/features/debugger/__tests__/editor.test.ts › output class gutter
    #[test]
    fn disables_ordinary_temporary_and_logging_breakpoints_in_an_output_class_and_explains_why() {
        let path = "file:///proj/target/classes/C.class";
        let mut fixture = Fixture::new("class C {}");
        assert!(gutter_blocked(path).is_some_and(|reason| reason.contains("output classes without source code")));
        for press in [PLAIN, ALT, SHIFT] {
            assert!(matches!(fixture.click(path, "java", press), PressOutcome::Blocked(_)));
        }
        assert!(fixture.breakpoints.breakpoints_of(path).is_empty());
    }

    // src/features/debugger/__tests__/editor.test.ts › breakpoint look
    #[test]
    fn shows_the_verified_icon_only_while_a_session_has_bound_it() {
        let b = BreakpointEntry {
            id: "1".to_owned(),
            line: 0,
            enabled: true,
            condition: None,
            log_message: None,
            hit_condition: None,
            temporary: false,
            suspend: true,
            log_hit: false,
        };
        let off = BreakpointEntry { enabled: false, ..b.clone() };
        let status = |verified| BreakpointStatus { verified, message: None, line: 0 };
        assert_eq!(look_of(&b, None, false, false), BreakpointLook::Normal);
        assert_eq!(look_of(&b, Some(&status(true)), false, true), BreakpointLook::Verified);
        assert_eq!(look_of(&b, Some(&status(false)), false, true), BreakpointLook::Unverified);
        assert_eq!(look_of(&off, Some(&status(true)), false, true), BreakpointLook::Disabled);
        assert_eq!(look_of(&b, Some(&status(true)), true, true), BreakpointLook::Muted);
        assert_eq!(look_of(&off, None, true, false), BreakpointLook::Disabled);
    }

    // src/features/debugger/__tests__/editor.test.ts › breakpoint gutter clicks
    #[test]
    fn shift_click_adds_a_non_suspending_logging_breakpoint_alt_click_a_temporary_one() {
        let mut fixture = Fixture::new("a\nb");
        fixture.click(PATH, "python", SHIFT);
        let b = &fixture.list()[0];
        assert!(b.line == 0 && !b.suspend && b.log_hit);
        // Modifier clicks leave an existing breakpoint alone; a plain click removes it.
        fixture.click(PATH, "python", ALT);
        fixture.click(PATH, "python", SHIFT);
        assert_eq!(fixture.list().len(), 1);
        fixture.click(PATH, "python", PLAIN);
        assert!(fixture.list().is_empty());
        fixture.click(PATH, "python", ALT);
        assert!(fixture.list()[0].line == 0 && fixture.list()[0].temporary);
    }

    // src/features/debugger/__tests__/editor.test.ts › breakpoint gutter clicks
    #[test]
    fn adds_nothing_on_a_blank_line_but_still_removes_a_breakpoint_there() {
        let mut fixture = Fixture::new("\na");
        for press in [PLAIN, ALT, SHIFT] {
            fixture.click(PATH, "python", press);
        }
        assert!(fixture.list().is_empty());
        fixture.breakpoints.toggle_breakpoint(PATH, 0, 0.0);
        fixture.click(PATH, "python", PLAIN);
        assert!(fixture.list().is_empty());
    }

    // src/features/debugger/__tests__/editor.test.ts › breakpoint gutter clicks
    #[test]
    fn middle_click_enables_or_disables_an_existing_breakpoint() {
        let mut fixture = Fixture::new("a\nb");
        assert_eq!(fixture.click(PATH, "python", MIDDLE), PressOutcome::Ignored);
        assert!(fixture.list().is_empty());
        fixture.breakpoints.toggle_breakpoint(PATH, 0, 0.0);
        assert_eq!(fixture.click(PATH, "python", MIDDLE), PressOutcome::Toggled);
        assert!(!fixture.list()[0].enabled);
        fixture.click(PATH, "python", MIDDLE);
        assert!(fixture.list()[0].enabled);
    }

    // Not from a spec: a ⇧-click hands back the new breakpoint for its properties, a right-click
    // names the breakpoint on its line, and mirrored or foreign backend changes move nothing.
    #[test]
    fn opens_properties_and_leaves_mirrored_changes_to_the_originating_editor() {
        let mut fixture = Fixture::new("a\nb");
        let editor = GutterEditor { path: PATH, language: "python", doc: &fixture.doc, comments: &[] };
        let outcome = gutter_click(&mut fixture.breakpoints, &mut fixture.special, &editor, 1, SHIFT, 0.0);
        let id = fixture.breakpoints.breakpoints_of(PATH)[0].id.clone();
        assert_eq!(outcome, ClickOutcome::OpenProperties { id: id.clone() });
        assert_eq!(gutter_context_menu(&fixture.breakpoints, &fixture.special, PATH, 1), Some(id));
        assert_eq!(gutter_context_menu(&fixture.breakpoints, &fixture.special, PATH, 0), None);
        assert_eq!(fixture.markers(false)[0].look, BreakpointLook::Log);

        let (changes, next) = edit(&fixture.doc, 0, 0, "new\n");
        let edit = (&fixture.doc, changes.as_slice(), &next);
        for origin in
            [ChangeOrigin { mirrored: true, ..Default::default() }, ChangeOrigin { remote: true, ..Default::default() }]
        {
            assert!(!document_changed(&mut fixture.breakpoints, &mut fixture.special, PATH, edit, origin, 0.0));
        }
        assert_eq!(fixture.breakpoints.breakpoints_of(PATH)[0].line, 1);
        let writer = ChangeOrigin { remote: true, writer: true, ..Default::default() };
        assert!(document_changed(&mut fixture.breakpoints, &mut fixture.special, PATH, edit, writer, 0.0));
        assert_eq!(fixture.breakpoints.breakpoints_of(PATH)[0].line, 2);
    }

    // Not from a spec: a language without a debugger takes no new breakpoint, but one that is there
    // can still be removed.
    #[test]
    fn a_language_without_a_debugger_takes_no_new_breakpoint() {
        let mut fixture = Fixture::new("a\nb");
        let doc = fixture.doc.clone();
        let editor = GutterEditor { path: PATH, language: "markdown", doc: &doc, comments: &[] };
        let press = |fixture: &mut Fixture| {
            gutter_mouse_down(&mut fixture.breakpoints, &mut fixture.special, &editor, 0, PLAIN, false, 0.0)
        };
        assert_eq!(press(&mut fixture), PressOutcome::Ignored);
        fixture.breakpoints.toggle_breakpoint(PATH, 0, 0.0);
        assert_eq!(press(&mut fixture), PressOutcome::AwaitRelease);
    }

    // Not from a spec: a method breakpoint takes the marker of its line and moves with edits.
    #[test]
    fn a_method_breakpoint_takes_the_marker_of_its_line() {
        let mut fixture = Fixture::new("def run():\n  pass");
        fixture.breakpoints.toggle_breakpoint(PATH, 0, 0.0);
        let method = fixture.special.toggle_method_at(PATH, 0, "run", 0.0).expect("a method breakpoint");
        let markers = fixture.markers(true);
        assert_eq!(markers, [Marker { line: 0, id: method.id.clone(), look: BreakpointLook::Method }]);
        fixture.special.update_special(&method.id, SpecialPatch { enabled: Some(false), ..Default::default() }, 0.0);
        assert_eq!(fixture.markers(true)[0].look, BreakpointLook::MethodDisabled);
        let (changes, next) = edit(&fixture.doc, 0, 0, "\n");
        let edit = (&fixture.doc, changes.as_slice(), &next);
        document_changed(&mut fixture.breakpoints, &mut fixture.special, PATH, edit, ChangeOrigin::default(), 0.0);
        assert_eq!(fixture.special.methods()[0].line, Some(1));
    }
}
