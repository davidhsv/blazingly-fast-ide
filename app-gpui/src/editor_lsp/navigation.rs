//! Navigation from the editor: Go to Declaration / Implementation / Type Declaration (⌘B, ⌥⌘B,
//! ⇧⌘B, ⌘-click, a plain middle-button click, a force click), the targets chooser, and usages
//! highlighting.
//!
//! A navigation is a [`GotoFlow`]: it asks the server, and each answer is a [`GotoStep`] for the
//! caller to carry out (open a target, show the chooser, show a hint, try what needs no server).
//! Go to Implementation streams: the chooser opens with the implementations found first (Java:
//! the project's) while the server searches on, and fills in when the search ends; closing the
//! chooser cancels the search. An answer to a request that is no longer current is dropped.

use super::files::target_label;
use super::hint::{HintState, HintTimer, PendingHint};
use super::hover::{Modifiers, is_navigation_gesture};
use super::lsp::{Inflight, error_hint, is_cancelled, is_unavailable};
use super::positions::range_contains;
use crate::platform::dispatcher::{DispatchEnv, Dispatcher, Mods, Outcome, mouse_stroke};
use crate::platform::i18n::{I18n, Param};
use crate::platform::virtual_docs::{basename, is_virtual_uri, relative_path, virtual_container};
use ide_core::ipc_path::to_ipc;
use ide_core::{IdeError, Location, Pos, RequestId, SymbolKind};
use ide_lsp::types::GotoKind;
use std::collections::{HashMap, HashSet};

/// `locations` without repeated targets (same file and start).
pub fn dedupe_locations(locations: Vec<Location>) -> Vec<Location> {
    let mut seen = HashSet::new();
    locations
        .into_iter()
        .filter(|location| seen.insert((location.path.clone(), location.range.start.line, location.range.start.col)))
        .collect()
}

/// `targets` without repeated targets (same file and start), first occurrences first.
pub fn dedupe_targets(targets: Vec<Target>) -> Vec<Target> {
    let mut seen = HashSet::new();
    targets
        .into_iter()
        .filter(|target| {
            let at = &target.location;
            seen.insert((at.path.clone(), at.range.start.line, at.range.start.col))
        })
        .collect()
}

/// The status bar message of Highlight Usages in File, as IDEA words it. `name` is quoted
/// already; `toggle_keys` is the shortcut that turns the highlighting off again.
pub fn usages_status_text(count: usize, name: &str, toggle_keys: Option<&str>) -> String {
    let again = toggle_keys.map(|keys| format!("press {keys} again to remove the highlighting, ")).unwrap_or_default();
    let usages = if count == 1 { "usage" } else { "usages" };
    format!("{count} {usages} of {name} found ({again}Escape to remove all highlighting)")
}

/// The marker of answers that did not come from the language server.
pub const SYNTAX_NOTE: &str = "Based on syntax (language server not ready)";
/// How long [`SYNTAX_NOTE`] stays after a syntax-based navigation.
pub const SYNTAX_NOTE_TIMEOUT_MS: u64 = 2500;
/// The progress hint of a slow navigation.
pub const PROGRESS_TEXT: &str = "Finding navigation target…";
/// The hint when ⌘B on a declaration found no usages to show.
pub const NO_USAGES_TEXT: &str = "No usages found";
/// The action ⌘B on a declaration runs instead (IDEA shows its usages).
pub const SHOW_USAGES_ACTION: &str = "find.showUsages";
/// The action the chooser's pin runs, with the symbol and the targets.
pub const SHOW_USAGE_RESULTS_ACTION: &str = "find.showUsageResults";
/// The label of the chooser's pin button: the targets as a Find tool window tab.
pub const PIN_LABEL: &str = "Open in Find Tool Window";

/// What a navigation of `kind` says when it finds nothing.
pub fn not_found_text(kind: GotoKind) -> &'static str {
    match kind {
        GotoKind::Definition | GotoKind::Declaration => "Cannot find declaration to go to",
        GotoKind::Implementation => "No implementations found",
        GotoKind::TypeDefinition => "Cannot find type declaration",
    }
}

fn chooser_base(kind: GotoKind) -> &'static str {
    match kind {
        GotoKind::Definition | GotoKind::Declaration => "Choose Declaration",
        GotoKind::Implementation => "Choose Implementation",
        GotoKind::TypeDefinition => "Choose Type Declaration",
    }
}

fn is_declaration(kind: GotoKind) -> bool {
    matches!(kind, GotoKind::Definition | GotoKind::Declaration)
}

/// "Choose Implementation of foo (3 found)"; `from_syntax` marks targets of the syntax fallback.
pub fn chooser_title(i18n: &mut I18n, kind: GotoKind, name: Option<&str>, count: usize, from_syntax: bool) -> String {
    if kind == GotoKind::Implementation && !from_syntax {
        return match name {
            Some(name) => {
                i18n.tx_built(&["Choose Implementation of ", " (", " found)"], &[Param::from(name), Param::from(count)])
            }
            None => i18n.tx_built(&["Choose Implementation (", " found)"], &[Param::from(count)]),
        };
    }
    let of = name.map(|name| format!(" of {name}")).unwrap_or_default();
    let syntax = if from_syntax { ", based on syntax" } else { "" };
    format!("{}{of} ({count} found{syntax})", chooser_base(kind))
}

/// The chooser's title while the server is still searching for more implementations.
pub fn searching_title(i18n: &mut I18n, name: Option<&str>, count: usize) -> String {
    match name {
        Some(name) => i18n.tx_built(
            &["Choose Implementation of ", " (", " found, searching…)"],
            &[Param::from(name), Param::from(count)],
        ),
        None => i18n.tx_built(&["Choose Implementation (", " found, searching…)"], &[Param::from(count)]),
    }
}

/// An index symbol the syntax fallback found: its chooser row shows the name and container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub container: Option<String>,
}

/// A navigation target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub location: Location,
    pub symbol: Option<TargetSymbol>,
}

impl From<Location> for Target {
    fn from(location: Location) -> Self {
        Self { location, symbol: None }
    }
}

/// A row of the targets chooser. The view picks the file icon from the target's path, unless the
/// row is an index symbol (`symbol`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRow {
    /// The declared name (what speed search matches), else the file name.
    pub label: String,
    /// "Container · file:line", "File.java · dir/File.java:4" or "dir/File.java:4".
    pub detail: String,
    /// The target's line, trimmed, when the file could be read.
    pub preview: Option<String>,
    pub symbol: Option<SymbolKind>,
    pub target: Location,
}

/// The chooser rows of `targets`. `lines` holds the lines of the target files that could be read
/// (by path); `root` is the project root that paths are shown relative to.
pub fn target_rows(targets: &[Target], lines: &HashMap<String, Vec<String>>, root: &str) -> Vec<TargetRow> {
    targets
        .iter()
        .map(|target| {
            let location = &target.location;
            let path = to_ipc(&location.path);
            let line = location.range.start.line;
            let shown =
                if is_virtual_uri(&path) { virtual_container(&path) } else { relative_path(root, &path).into_owned() };
            let at = format!("{shown}:{}", line + 1);
            let source = lines.get(&path).and_then(|file| file.get(line as usize));
            let preview = source.map(|text| text.trim().to_owned());
            if let Some(symbol) = &target.symbol {
                return TargetRow {
                    label: symbol.name.clone(),
                    detail: match &symbol.container {
                        Some(container) => format!("{container} · {at}"),
                        None => at,
                    },
                    preview,
                    symbol: Some(symbol.kind),
                    target: location.clone(),
                };
            }
            let file = basename(&path).into_owned();
            let name = target_label(source.map_or("", String::as_str), location.range.start.col);
            TargetRow {
                detail: if name.is_some() { format!("{file} · {at}") } else { at },
                label: name.unwrap_or(file),
                preview,
                symbol: None,
                target: location.clone(),
            }
        })
        .collect()
}

/// Whether `targets` is just the declaration the caret is on (⌘B then shows its usages).
pub fn is_self_target(targets: &[Target], path: &str, pos: Pos) -> bool {
    let [target] = targets else { return false };
    to_ipc(&target.location.path) == path && range_contains(target.location.range, pos)
}

/// The targets chooser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chooser {
    pub title: String,
    pub targets: Vec<Target>,
    /// The server is still searching: more targets may follow.
    pub searching: bool,
    /// The symbol the pin's Find tool window tab is named after ("" when unknown).
    pub symbol: String,
}

/// What the caller does next for a navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GotoStep {
    /// Nothing (a stale or cancelled answer, or a closed chooser).
    Nothing,
    /// The server answered nothing right after an edit (it may still be analysing it): ask again
    /// with [`GotoFlow::retry`] after the delay.
    Retry { delay_ms: u64 },
    /// Open the target. `syntax_note`: then show [`SYNTAX_NOTE`] in the target's editor.
    Go { target: Location, syntax_note: bool },
    /// Show the chooser, or refill the one that is open.
    Chooser(Chooser),
    /// Show this hint at the caret (English source text).
    Hint(String),
    /// The server knows no declaration here: follow a web reference (`class` / `id` / `var(--x)`),
    /// a link or a file path at the caret if there is one, else show the hint `otherwise`.
    References { otherwise: String },
    /// The server cannot answer: follow a web reference or a link at the caret if there is one,
    /// else ask the syntax fallback and hand its answer to [`GotoFlow::fallback`].
    Fallback,
    /// The caret is on the declaration: run [`SHOW_USAGES_ACTION`], and show [`NO_USAGES_TEXT`]
    /// when it shows nothing.
    ShowUsages,
}

/// The single target opens; several go to the chooser.
fn go_or_choose(
    targets: Vec<Target>,
    title: impl FnOnce(usize) -> String,
    symbol: &str,
    from_syntax: bool,
) -> GotoStep {
    match <[Target; 1]>::try_from(targets) {
        Ok([target]) => GotoStep::Go { target: target.location, syntax_note: from_syntax },
        Err(targets) => GotoStep::Chooser(Chooser {
            title: title(targets.len()),
            targets,
            searching: false,
            symbol: symbol.to_owned(),
        }),
    }
}

/// The step for targets a plugin's navigation provider found (no server asked); a provider that
/// found none answers nothing.
pub fn contributed_step(i18n: &I18n, targets: Vec<Location>, symbol: &str) -> GotoStep {
    if targets.is_empty() {
        return GotoStep::Nothing;
    }
    let targets = targets.into_iter().map(Target::from).collect();
    go_or_choose(targets, |_| i18n.tx("Choose Declaration"), symbol, false)
}

/// The delay before asking again after an empty answer right after an edit.
pub const EDIT_RETRY_DELAY_MS: u64 = 250;
/// An edit this recent may not have been analysed yet.
const EDIT_RETRY_WINDOW_MS: u64 = 1500;
const EDIT_RETRIES: u32 = 3;

/// A request a [`GotoFlow`] sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GotoRequest {
    pub id: RequestId,
    pub kind: GotoKind,
    pub pos: Pos,
    /// Go to Implementation: partial results come through [`GotoFlow::partial`].
    pub streamed: bool,
}

/// One navigation, from the request to the target, the chooser or the hint.
#[derive(Debug, Clone)]
pub struct GotoFlow {
    kind: GotoKind,
    path: String,
    pos: Pos,
    /// The name at the caret, for titles and the pin.
    name: Option<String>,
    /// Why the server only checks this file's syntax, when it does.
    outside: Option<&'static str>,
    inflight: Inflight,
    pending: Option<PendingHint>,
    attempt: u32,
    /// The current request answered.
    finished: bool,
    /// The chooser opened with the first implementations.
    opened: bool,
    closed: bool,
    first: Vec<Target>,
    server_down: bool,
    error: Option<IdeError>,
}

impl GotoFlow {
    /// A navigation of `kind` from `pos` of the file `path`; `name` is the navigation word there,
    /// `outside` the reason of [`super::outside_project::outside_project_reason`] for the file.
    pub fn new(kind: GotoKind, path: &str, pos: Pos, name: Option<String>, outside: Option<&'static str>) -> Self {
        Self {
            kind,
            path: path.to_owned(),
            pos,
            name,
            outside,
            inflight: Inflight::default(),
            pending: None,
            attempt: 0,
            finished: false,
            opened: false,
            closed: false,
            first: Vec::new(),
            server_down: false,
            error: None,
        }
    }

    fn request(&mut self, id: RequestId) -> GotoRequest {
        self.inflight.begin(id);
        self.finished = false;
        GotoRequest { id, kind: self.kind, pos: self.pos, streamed: self.kind == GotoKind::Implementation }
    }

    /// Sends the request; the caller starts a timer of [`super::hint::PENDING_DELAY_MS`] and then
    /// calls [`GotoFlow::progress_due`]. `caret` is where the progress hint shows.
    pub fn start(&mut self, hints: &HintState, caret: u32, id: RequestId) -> GotoRequest {
        self.pending = Some(hints.show_pending(PROGRESS_TEXT, caret));
        self.request(id)
    }

    /// Asks again after a [`GotoStep::Retry`].
    pub fn retry(&mut self, id: RequestId) -> GotoRequest {
        self.attempt += 1;
        self.request(id)
    }

    /// The progress delay elapsed: shows "Finding navigation target…" while the request runs.
    pub fn progress_due(&mut self, hints: &mut HintState) -> Option<HintTimer> {
        self.pending.as_mut().and_then(|pending| hints.pending_due(pending))
    }

    fn progress_done(&mut self, hints: &mut HintState) {
        if let Some(pending) = self.pending.as_mut() {
            hints.pending_done(pending);
        }
    }

    /// The first implementations the server found while it searches on: the chooser opens with
    /// them. Later chunks, empty ones and stale ones change nothing.
    pub fn partial(
        &mut self,
        id: RequestId,
        locations: Vec<Location>,
        hints: &mut HintState,
        i18n: &mut I18n,
    ) -> Option<GotoStep> {
        if self.kind != GotoKind::Implementation
            || self.finished
            || self.opened
            || !self.inflight.is_current(id)
            || locations.is_empty()
        {
            return None;
        }
        self.opened = true;
        self.progress_done(hints);
        self.first = dedupe_targets(locations.into_iter().map(Target::from).collect());
        Some(GotoStep::Chooser(Chooser {
            title: searching_title(i18n, self.name.as_deref(), self.first.len()),
            targets: self.first.clone(),
            searching: true,
            symbol: self.name.clone().unwrap_or_default(),
        }))
    }

    /// The chooser closed: returns the search to cancel in the backend while it still runs.
    pub fn chooser_closed(&mut self) -> Option<RequestId> {
        self.closed = true;
        self.inflight.cancel()
    }

    /// Fills the open chooser with all implementations, those it shows first.
    fn complete(&self, rest: Vec<Location>, i18n: &mut I18n) -> GotoStep {
        if self.closed {
            return GotoStep::Nothing;
        }
        let all = dedupe_targets(self.first.iter().cloned().chain(rest.into_iter().map(Target::from)).collect());
        GotoStep::Chooser(Chooser {
            title: chooser_title(i18n, GotoKind::Implementation, self.name.as_deref(), all.len(), false),
            targets: all,
            searching: false,
            symbol: self.name.clone().unwrap_or_default(),
        })
    }

    /// The server answered request `id`. `since_edit_ms` is the time since the document last
    /// changed (`None`: not since it opened).
    pub fn answered(
        &mut self,
        id: RequestId,
        result: Result<Vec<Location>, IdeError>,
        since_edit_ms: Option<u64>,
        hints: &mut HintState,
        i18n: &mut I18n,
    ) -> GotoStep {
        if !self.inflight.finish(id) {
            return GotoStep::Nothing;
        }
        self.finished = true;
        match result {
            Ok(locations) => {
                let recent = since_edit_ms.is_some_and(|ms| ms <= EDIT_RETRY_WINDOW_MS);
                if locations.is_empty() && !self.opened && self.attempt < EDIT_RETRIES && recent {
                    return GotoStep::Retry { delay_ms: EDIT_RETRY_DELAY_MS };
                }
                self.progress_done(hints);
                if self.opened {
                    return self.complete(locations, i18n);
                }
                let targets = dedupe_targets(locations.into_iter().map(Target::from).collect());
                self.resolve(targets, false, i18n)
            }
            Err(error) => {
                self.progress_done(hints);
                // The chooser shows the first ones: closing it cancelled the search, or the rest failed.
                if self.opened {
                    return self.complete(Vec::new(), i18n);
                }
                if is_cancelled(&error) || !is_declaration(self.kind) {
                    return error_hint(&error, true).map_or(GotoStep::Nothing, GotoStep::Hint);
                }
                self.server_down = is_unavailable(&error);
                self.error = Some(error);
                GotoStep::Fallback
            }
        }
    }

    /// The syntax fallback's answer after [`GotoStep::Fallback`]: its targets, or `None` when it
    /// cannot answer for this file (the server's error shows then).
    pub fn fallback(&mut self, targets: Option<Vec<Target>>, i18n: &mut I18n) -> GotoStep {
        match targets {
            Some(targets) => self.resolve(dedupe_targets(targets), true, i18n),
            None => {
                let hint = self.error.as_ref().and_then(|error| error_hint(error, true));
                hint.map_or(GotoStep::Nothing, GotoStep::Hint)
            }
        }
    }

    fn resolve(&self, targets: Vec<Target>, from_syntax: bool, i18n: &mut I18n) -> GotoStep {
        if targets.is_empty() {
            let not_found = not_found_text(self.kind);
            let hint = if self.server_down {
                format!("{not_found}: no language server is running for this file")
            } else if let Some(reason) = self.outside {
                format!("{}. {}", i18n.tx(not_found), i18n.tx(reason))
            } else {
                not_found.to_owned()
            };
            return if is_declaration(self.kind) && !from_syntax {
                GotoStep::References { otherwise: hint }
            } else {
                GotoStep::Hint(hint)
            };
        }
        if self.kind == GotoKind::Definition && is_self_target(&targets, &self.path, self.pos) {
            return GotoStep::ShowUsages;
        }
        let name = self.name.as_deref();
        let title = |count| chooser_title(i18n, self.kind, name, count, from_syntax);
        go_or_choose(targets, title, name.unwrap_or(""), from_syntax)
    }
}

/// What a mouse button press over editor text does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointerNavigation {
    /// Not a navigation: the press goes on to the editor (caret, selection).
    Pass,
    /// Put the caret at `caret`, then run the action the mouse shortcut is bound to (⌥⌘-click:
    /// Go to Implementation); the press is consumed.
    Run { action: String, caret: u32 },
    /// Put the caret at `caret` and go to the declaration there; the press is consumed.
    Declaration { caret: u32 },
}

/// The action that adds or removes a caret with a click (⌥⇧-click), which navigation leaves alone.
const ADD_CARET_ACTION: &str = "editor.addOrRemoveCaret";

fn modifiers(mods: Mods) -> Modifiers {
    Modifiers { meta: mods.meta, ctrl: mods.ctrl, alt: mods.alt, shift: mods.shift }
}

/// A press of the 0-based `button` with `mods` at document position `pos` (`None` when the pointer
/// is over no text). A mouse shortcut of the keymap wins (`env`'s effective bindings and enabled
/// actions); otherwise ⌘/Ctrl-click and a plain middle-button click go to the declaration, as in
/// IDEA. Other buttons and modifier combinations pass.
pub fn pointer_navigation(
    button: u32,
    mods: Mods,
    pos: Option<u32>,
    mac: bool,
    dispatcher: &Dispatcher,
    env: &dyn DispatchEnv,
) -> PointerNavigation {
    if button > 1 {
        return PointerNavigation::Pass;
    }
    if button == 0
        && let Some(stroke) = mouse_stroke(button, mods)
        && env.bindings().iter().any(|(binding, id)| id == ADD_CARET_ACTION && *binding == stroke)
    {
        return PointerNavigation::Pass;
    }
    if let Outcome::Run(action) = dispatcher.mouse_shortcut(button, mods, env) {
        return pos.map_or(PointerNavigation::Pass, |caret| PointerNavigation::Run { action, caret });
    }
    let navigates = if button == 1 { mods == Mods::NONE } else { is_navigation_gesture(modifiers(mods), mac) };
    match pos {
        Some(caret) if navigates => PointerNavigation::Declaration { caret },
        _ => PointerNavigation::Pass,
    }
}

/// A force click (macOS trackpad), IDEA's "Force touch" shortcut: without modifiers it goes to the
/// declaration under the pointer like ⌘-click; a modified one is left to the click gestures.
pub fn force_click(mods: Mods, pos: Option<u32>) -> PointerNavigation {
    match pos {
        Some(caret) if mods == Mods::NONE => PointerNavigation::Declaration { caret },
        _ => PointerNavigation::Pass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::buffer;
    use crate::editor_lsp::positions::offset_to_pos;
    use crate::platform::keymap::{Keymap, KeymapState};
    use ide_core::Range;
    use ide_core::error::ErrorKind;
    use ropey::Rope;

    #[test]
    fn drops_duplicate_targets() {
        let location = |path: &str, line: u32| Location {
            path: path.into(),
            range: Range::new(Pos::new(line, 0), Pos::new(line, 1)),
            uri: None,
        };
        assert_eq!(dedupe_locations(vec![location("/a", 1), location("/a", 1), location("/b", 1)]).len(), 2);
    }

    #[test]
    fn reads_like_ideas_status_bar_message() {
        assert_eq!(
            usages_status_text(1, "'x'", Some("⇧⌘F7")),
            "1 usage of 'x' found (press ⇧⌘F7 again to remove the highlighting, Escape to remove all highlighting)"
        );
        assert_eq!(usages_status_text(3, "'x'", None), "3 usages of 'x' found (Escape to remove all highlighting)");
    }

    // The editor of the mouse spec: "service.run()" in /p/Main.java, the pointer at offset 9.

    const MAIN: &str = "/p/Main.java";
    const POINTER: u32 = 9;
    const META: Mods = Mods { meta: true, ..Mods::NONE };
    const CTRL: Mods = Mods { ctrl: true, ..Mods::NONE };
    const META_ALT: Mods = Mods { meta: true, alt: true, ..Mods::NONE };
    const CTRL_ALT: Mods = Mods { ctrl: true, alt: true, ..Mods::NONE };

    /// The effective keymap and the registered actions.
    struct Keys {
        state: KeymapState,
        actions: HashSet<&'static str>,
    }

    impl DispatchEnv for Keys {
        fn bindings(&self) -> &[(String, String)] {
            self.state.effective_bindings()
        }
        fn key_enabled(&self, action_id: &str) -> bool {
            self.actions.contains(action_id)
        }
    }

    fn keys(preset: &str, overrides: &[(&str, &[&str])]) -> Keys {
        let mut state = KeymapState::new(preset);
        let map: Keymap = overrides
            .iter()
            .map(|(id, bindings)| ((*id).to_owned(), bindings.iter().map(|b| (*b).to_owned()).collect()))
            .collect();
        state.set_overrides(map);
        Keys { state, actions: HashSet::from([IMPLEMENTATION_ACTION]) }
    }

    const IMPLEMENTATION_ACTION: &str = "navigate.implementation";

    fn click(keys: &Keys, mac: bool, button: u32, mods: Mods) -> PointerNavigation {
        pointer_navigation(button, mods, Some(POINTER), mac, &Dispatcher::new(mac), keys)
    }

    fn at(path: &str, line: u32) -> Location {
        Location { path: path.into(), range: Range::new(Pos::new(line, 0), Pos::new(line, 1)), uri: None }
    }

    fn service_impl() -> Location {
        Location { path: "/p/ServiceImpl.java".into(), range: Range::new(Pos::new(4, 2), Pos::new(4, 5)), uri: None }
    }

    /// A navigation of `kind` started at the pointer of the mouse spec.
    fn from_pointer(kind: GotoKind, hints: &HintState) -> (GotoFlow, GotoRequest) {
        let doc = Rope::from_str("service.run()");
        let mut flow = GotoFlow::new(kind, MAIN, offset_to_pos(&doc, POINTER), Some("run".into()), None);
        let request = flow.start(hints, POINTER, RequestId(1));
        (flow, request)
    }

    #[test]
    fn uses_plugin_navigation_for_clicks_without_asking_the_language_server() {
        let keys = keys("intellij-macos", &[]);
        let PointerNavigation::Declaration { caret } = click(&keys, true, 0, META) else {
            panic!("⌘-click goes to the declaration");
        };
        assert_eq!(caret, POINTER);
        // The provider answered first: no flow, no server request.
        let mapper =
            Location { path: "/p/Mapper.xml".into(), range: Range::new(Pos::new(2, 4), Pos::new(2, 8)), uri: None };
        assert_eq!(
            contributed_step(&I18n::default(), vec![mapper.clone()], "service.run()"),
            GotoStep::Go { target: mapper, syntax_note: false }
        );
    }

    #[test]
    fn opens_the_implementation_of_the_symbol_clicked_with_option_command() {
        let keys = keys("intellij-macos", &[]);
        assert_eq!(
            click(&keys, true, 0, META_ALT),
            PointerNavigation::Run { action: IMPLEMENTATION_ACTION.into(), caret: POINTER }
        );
        let mut hints = HintState::default();
        let (mut flow, request) = from_pointer(GotoKind::Implementation, &hints);
        assert_eq!((request.kind, request.pos), (GotoKind::Implementation, Pos::new(0, 9)));
        let step = flow.answered(request.id, Ok(vec![service_impl()]), None, &mut hints, &mut I18n::default());
        assert_eq!(step, GotoStep::Go { target: service_impl(), syntax_note: false });
    }

    #[test]
    fn preserves_declaration_navigation_on_os() {
        // Every platform with the macOS preset, as the spec runs it.
        let keys = keys("intellij-macos", &[]);
        for (mac, mods) in [(true, META), (false, CTRL)] {
            assert_eq!(click(&keys, mac, 0, mods), PointerNavigation::Declaration { caret: POINTER }, "mac: {mac}");
        }
        let mut hints = HintState::default();
        let (mut flow, request) = from_pointer(GotoKind::Definition, &hints);
        assert_eq!((request.kind, request.pos), (GotoKind::Definition, Pos::new(0, 9)));
        let step = flow.answered(request.id, Ok(vec![service_impl()]), None, &mut hints, &mut I18n::default());
        assert!(matches!(step, GotoStep::Go { .. }));
    }

    #[test]
    fn opens_the_definition_from_a_plain_middle_button_click() {
        let keys = keys("intellij-macos", &[]);
        assert_eq!(click(&keys, true, 1, Mods::NONE), PointerNavigation::Declaration { caret: POINTER });
    }

    #[test]
    fn ignores_other_button_modifier_combinations() {
        let keys = keys("intellij-macos", &[]);
        for (button, mods) in
            [(1, META_ALT), (2, META_ALT), (0, Mods { shift: true, ..META_ALT }), (0, Mods { ctrl: true, ..META_ALT })]
        {
            assert_eq!(click(&keys, true, button, mods), PointerNavigation::Pass, "{button} {mods:?}");
        }
    }

    #[test]
    fn reports_missing_implementations_without_navigating_to_the_declaration() {
        let mut hints = HintState::default();
        let (mut flow, request) = from_pointer(GotoKind::Implementation, &hints);
        let step = flow.answered(request.id, Ok(Vec::new()), None, &mut hints, &mut I18n::default());
        assert_eq!(step, GotoStep::Hint("No implementations found".into()));
    }

    #[test]
    fn opens_the_implementation_with_ctrl_alt_click_on_os() {
        let keys = keys("intellij-windows", &[]);
        assert_eq!(
            click(&keys, false, 0, CTRL_ALT),
            PointerNavigation::Run { action: IMPLEMENTATION_ACTION.into(), caret: POINTER }
        );
    }

    #[test]
    fn does_not_navigate_with_option_command_when_the_binding_is_removed() {
        let keys = keys("intellij-macos", &[(IMPLEMENTATION_ACTION, &["Meta+Alt+B"])]);
        assert_eq!(click(&keys, true, 0, META_ALT), PointerNavigation::Pass);
    }

    #[test]
    fn follows_a_different_mouse_binding_assigned_to_the_implementation_action() {
        let keys = keys("intellij-macos", &[(IMPLEMENTATION_ACTION, &["Ctrl+Alt+Button1"])]);
        assert_eq!(click(&keys, true, 0, META_ALT), PointerNavigation::Pass);
        assert_eq!(
            click(&keys, true, 0, CTRL_ALT),
            PointerNavigation::Run { action: IMPLEMENTATION_ACTION.into(), caret: POINTER }
        );
    }

    #[test]
    fn does_not_run_the_binding_if_the_click_has_no_document_position() {
        let keys = keys("intellij-macos", &[]);
        let dispatcher = Dispatcher::new(true);
        assert_eq!(pointer_navigation(0, META_ALT, None, true, &dispatcher, &keys), PointerNavigation::Pass);
    }

    #[test]
    fn allows_the_same_gesture_to_be_reassigned_to_another_action() {
        let mut keys =
            keys("intellij-macos", &[(IMPLEMENTATION_ACTION, &["Meta+Alt+B"]), ("test.mouse", &["Alt+Meta+Button1"])]);
        keys.actions.insert("test.mouse");
        let run = PointerNavigation::Run { action: "test.mouse".into(), caret: POINTER };
        assert_eq!(click(&keys, true, 0, META_ALT), run);
    }

    #[test]
    fn goes_to_the_declaration_under_the_pointer_like_command_click() {
        assert_eq!(force_click(Mods::NONE, Some(POINTER)), PointerNavigation::Declaration { caret: POINTER });
    }

    #[test]
    fn leaves_a_modified_force_click_to_the_click_gesture() {
        for mods in [META, Mods { alt: true, ..Mods::NONE }, CTRL, Mods { shift: true, ..Mods::NONE }] {
            assert_eq!(force_click(mods, Some(POINTER)), PointerNavigation::Pass, "{mods:?}");
        }
    }

    // The editor of the pending-navigation spec: "run()" in /p/main.rs, the caret at 0.

    fn run_flow(kind: GotoKind, hints: &HintState) -> (GotoFlow, GotoRequest) {
        let doc = Rope::from_str("run()");
        let word = buffer::word_at(&doc, 0);
        let name = buffer::slice(&doc, word.start, word.end);
        let mut flow = GotoFlow::new(kind, "/p/main.rs", Pos::new(0, 0), Some(name), None);
        let request = flow.start(hints, 0, RequestId(7));
        (flow, request)
    }

    fn hint_text(hints: &HintState) -> Option<&str> {
        hints.hint().map(|hint| hint.text.as_str())
    }

    fn labels(step: &GotoStep) -> Vec<String> {
        let GotoStep::Chooser(chooser) = step else { panic!("a chooser: {step:?}") };
        target_rows(&chooser.targets, &HashMap::new(), "/p").into_iter().map(|row| row.label).collect()
    }

    fn title(step: &GotoStep) -> &str {
        let GotoStep::Chooser(chooser) = step else { panic!("a chooser: {step:?}") };
        &chooser.title
    }

    #[test]
    fn shows_progress_while_finding_a_definition_then_opens_the_target_and_clears_the_hint() {
        let mut hints = HintState::default();
        let mut i18n = I18n::default();
        let (mut flow, request) = run_flow(GotoKind::Definition, &hints);
        assert!(flow.progress_due(&mut hints).is_some());
        assert_eq!(hint_text(&hints), Some("Finding navigation target…"));
        let lib = Location { path: "/p/lib.rs".into(), range: Range::new(Pos::new(3, 4), Pos::new(3, 7)), uri: None };
        let step = flow.answered(request.id, Ok(vec![lib.clone()]), None, &mut hints, &mut i18n);
        assert_eq!(step, GotoStep::Go { target: lib, syntax_note: false });
        assert_eq!(hint_text(&hints), None);
    }

    #[test]
    fn replaces_progress_with_the_server_error() {
        let mut hints = HintState::default();
        let mut i18n = I18n::default();
        let (mut flow, request) = run_flow(GotoKind::Implementation, &hints);
        flow.progress_due(&mut hints);
        let error = IdeError::new(ErrorKind::Protocol, "Server timed out");
        let step = flow.answered(request.id, Err(error), None, &mut hints, &mut i18n);
        assert_eq!(step, GotoStep::Hint("Server timed out".into()));
        assert_eq!(hint_text(&hints), None);
    }

    #[test]
    fn opens_the_chooser_with_the_implementations_found_first_then_adds_the_rest() {
        let mut hints = HintState::default();
        let mut i18n = I18n::default();
        let (mut flow, request) = run_flow(GotoKind::Implementation, &hints);
        flow.progress_due(&mut hints);
        let first = flow.partial(request.id, vec![at("/p/Own.java", 3)], &mut hints, &mut i18n).expect("it opens");
        assert_eq!(title(&first), "Choose Implementation of run (1 found, searching…)");
        assert_eq!(labels(&first), ["Own.java"]);
        assert_eq!(hint_text(&hints), None);
        // A later chunk changes nothing.
        assert_eq!(flow.partial(request.id, vec![at("/p/Other.java", 1)], &mut hints, &mut i18n), None);

        let all = vec![at("/p/Library.java", 8), at("/p/Own.java", 3)];
        let step = flow.answered(request.id, Ok(all), None, &mut hints, &mut i18n);
        assert_eq!(title(&step), "Choose Implementation of run (2 found)");
        assert_eq!(labels(&step), ["Own.java", "Library.java"]);
        // Nothing to cancel any more.
        assert_eq!(flow.chooser_closed(), None);
    }

    #[test]
    fn cancels_the_search_when_the_chooser_closes() {
        let mut hints = HintState::default();
        let mut i18n = I18n::default();
        let (mut flow, request) = run_flow(GotoKind::Implementation, &hints);
        assert!(flow.partial(request.id, vec![at("/p/Own.java", 3)], &mut hints, &mut i18n).is_some());
        assert_eq!(flow.chooser_closed(), Some(request.id));
        let cancelled = IdeError::new(ErrorKind::Cancelled, "cancelled");
        assert_eq!(flow.answered(request.id, Err(cancelled), None, &mut hints, &mut i18n), GotoStep::Nothing);
        assert_eq!(hint_text(&hints), None);
    }

    #[test]
    fn goes_to_the_only_implementation_when_none_came_first() {
        let mut hints = HintState::default();
        let mut i18n = I18n::default();
        let (mut flow, request) = run_flow(GotoKind::Implementation, &hints);
        let step = flow.answered(request.id, Ok(vec![at("/p/Library.java", 8)]), None, &mut hints, &mut i18n);
        assert_eq!(step, GotoStep::Go { target: at("/p/Library.java", 8), syntax_note: false });
    }
}
