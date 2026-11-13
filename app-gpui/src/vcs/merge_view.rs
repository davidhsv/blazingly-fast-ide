//! The three-way merge editor, after IntelliJ's merge tool: Yours on the left, the Result in the
//! middle, Theirs on the right, over [`MergeModel`].
//!
//! Like IntelliJ, the result starts as the base with every change of either side waiting in it. The
//! dividers join each change of a side still to be decided to its place in the result, and carry
//! the side's Accept arrow and Ignore cross (the platform's modifier key held: resolve using that
//! side, ignore both sides); a conflict whose sides changed different words has a wand beside it in
//! the result. The toolbar moves between the changes still open, applies the non-conflicting changes
//! from either side or both, resolves the simple conflicts, reverts the resolutions, opens the
//! comparisons of two versions and the settings, and says how many changes and conflicts are left.
//! The result can also be typed in, with the caret, selection, clipboard and input method support a
//! text input has; every edit and decision is one step to undo. Along the bottom, Accept Yours and
//! Accept Theirs take one version as it is, Apply saves the result (asking first when changes are
//! left unresolved) and Cancel gives up (asking first when the result was changed).
//!
//! [`MergeView`] owns no backend. It is built in its loading state and handed the three versions of
//! the file ([`MergeView::set_versions`]) or why they could not be read ([`MergeView::set_error`]);
//! what needs the backend, a dialog or a menu goes out as a [`MergeViewEvent`]. Its commands are the
//! [`MergeCommand`]s, which the workbench runs through [`MergeView::run`] and enables through
//! [`MergeView::enabled`].
//!
//! Offsets are UTF-16 code units of the texts with `\n` line breaks; lines are 0-based.

use crate::editor::buffer::{byte_of_col, col_of_byte};
use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::input;
use crate::platform::keymap::is_mac_host;
use crate::theme;
use crate::ui::text_field::TEXT_FIELD_CONTEXT;
use crate::ui::tooltip::WithTooltip;
use crate::vcs::changes::base_name;
use crate::vcs::conflicts::{MergeSides, MergeTitles, merge_titles};
use crate::vcs_diff::diff_scroll::{PaneLayout, PaneScroll, extent};
use crate::vcs_diff::doc::{Direction, Doc, utf16_len};
use crate::vcs_diff::merge_gutters::{
    MergeChange, MergeControl, MergePane, MergeSide, MergeTint, ResultMark, SideMarker, WAND_ICON, WAND_TITLE,
    merge_stripe, result_marks, side_marker, wand_marker,
};
use crate::vcs_more::merge_actions::MergeCommand;
use crate::vcs_more::merge_model::{
    AutoSide, MergeAction, MergeEdit, MergeModel, MergeState, MergeSummary, MergeTransaction, can_apply,
};
use crate::vcs_more::merge_scroll::{PANES, anchor, follow_top, map_between, transfer};
use crate::vcs_ops::dialogs::ConfirmRequest;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, App, Bounds, ClickEvent, ClipboardItem, Context, DispatchPhase, Div, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, Font, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, KeyDownEvent, Keystroke, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PathBuilder, Pixels, Point, Render, ScrollEvent, ShapedLine, SharedString, Style, TextAlign, TextRun,
    UTF16Selection, Window, div, fill, font, point, prelude::*, px, relative, size,
};
use std::collections::HashMap;
use std::ops::Range;

/// Width of a divider between two panes: room for a side's Accept and Ignore buttons.
const DIVIDER_WIDTH: f32 = 36.0;
/// The height a pane is taken to have before it is first drawn.
const DEFAULT_HEIGHT: f32 = 600.0;
/// Width of the result's wand column.
const WAND_WIDTH: f32 = 18.0;
/// Width of a pane's overview lane.
const LANE_WIDTH: f32 = 4.0;
/// How much of a change's colour its lines take.
const LINE_TINT: f32 = 0.14;
/// How much of a change's colour its band in a divider takes.
const BAND_TINT: f32 = 0.3;
/// Points along each edge of a divider band.
const BAND_STEPS: usize = 16;
const BUTTON_HEIGHT: f32 = 28.0;
/// The summary of a merge with nothing to show.
const NOTHING_LEFT: MergeSummary =
    MergeSummary { pending: 0, conflicts: 0, left: false, all: false, right: false, simple: false, revert: false };

/// What [`MergeView::set_versions`] shows: the three versions of the conflicted file and who stands
/// on each side. A side that deleted the file has no version.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MergeVersions {
    pub base: Option<String>,
    /// The user's side ("Yours"), already swapped during a rebase.
    pub ours: Option<String>,
    pub theirs: Option<String>,
    pub sides: MergeSides,
}

/// A question the merge editor asks before it goes on; the workbench shows it
/// ([`MergeQuestion::request`]) and hands the answer back to [`MergeView::answer`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeQuestion {
    /// Apply while this many changes are unresolved: save anyway?
    Unresolved(usize),
    /// Accept Yours / Accept Theirs after the result was changed: discard the changes?
    Discard(MergeSide),
    /// Cancel after the result was changed: exit anyway?
    Cancel,
}

impl MergeQuestion {
    /// The confirmation to show, in the UI language.
    pub fn request(self, i18n: &mut I18n) -> ConfirmRequest {
        match self {
            Self::Unresolved(count) => {
                let rest = if count == 1 {
                    " change is unresolved and keeps the base text. Save and mark the file as resolved anyway?"
                } else {
                    " changes are unresolved and keep the base text. Save and mark the file as resolved anyway?"
                };
                ConfirmRequest {
                    title: "Unresolved Changes".to_owned(),
                    message: i18n.tx_built(&["", rest], &[Param::from(count)]),
                    ok_label: "Mark Resolved".to_owned(),
                    danger: false,
                }
            }
            Self::Discard(side) => ConfirmRequest::danger(
                if side == MergeSide::Left { "Accept Yours" } else { "Accept Theirs" },
                i18n.tx("The result was changed. Discard the changes and take this version as it is?"),
                "Discard Changes",
            ),
            Self::Cancel => ConfirmRequest::danger(
                "Cancel Merge",
                i18n.tx("Exit without applying the changes made in the result?"),
                "Exit",
            ),
        }
    }
}

/// One version shown by a comparison of two of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareSide {
    pub label: String,
    pub text: String,
}

/// What the merge editor asks of the workbench.
#[derive(Clone, Debug, PartialEq)]
pub enum MergeViewEvent {
    /// Write `text` as the resolved file (`None`: the file is deleted, as the side taken deleted it)
    /// and stage it, then call [`MergeView::set_resolved`] and close the editor.
    Resolve { path: String, text: Option<String> },
    /// Ask `question`, and answer it with [`MergeView::answer`].
    Confirm(MergeQuestion),
    /// Close the editor without saving.
    Close,
    /// Read the three versions again and hand them to [`MergeView::set_versions`].
    Reload,
    /// Show the diff of two versions.
    Compare { left: CompareSide, right: CompareSide },
    /// Show this menu of merge actions (run through the action registry) at `position`, or under
    /// the pointer when there is none.
    ShowMenu { items: Vec<MenuItem<MergeCommand>>, position: Option<Point<Pixels>> },
    /// Automatically Apply Non-Conflicting Changes was switched: store the setting.
    AutoApplyChanged(bool),
    /// Open the file in an editor (once resolved).
    OpenFile { path: String },
    /// What the commands can do changed: publish [`MergeView::enabled`] to the context keys.
    CommandsChanged,
}

/// A button of the toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolButton {
    Previous,
    Next,
    Command(MergeCommand),
}

impl ToolButton {
    /// The name its selector is made of.
    pub fn name(self) -> &'static str {
        match self {
            Self::Previous => "previous",
            Self::Next => "next",
            Self::Command(command) => command.name(),
        }
    }
}

/// A toolbar button as drawn: its icon, English title and whether it can be pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolbarButton {
    pub button: ToolButton,
    pub icon: &'static str,
    pub title: &'static str,
    pub enabled: bool,
}

/// The toolbar from the left: `None` is a separator; the status text follows Compare Contents.
const TOOLBAR: [Option<(ToolButton, &str, &str)>; 13] = [
    Some((ToolButton::Previous, "arrow-up", "Previous Change")),
    Some((ToolButton::Next, "arrow-down", "Next Change")),
    None,
    Some((
        ToolButton::Command(MergeCommand::ApplyLeft),
        "diff-apply-left",
        "Apply Non-Conflicting Changes from the Left Side",
    )),
    Some((ToolButton::Command(MergeCommand::ApplyAll), "diff-apply-all", "Apply All Non-Conflicting Changes")),
    Some((
        ToolButton::Command(MergeCommand::ApplyRight),
        "diff-apply-right",
        "Apply Non-Conflicting Changes from the Right Side",
    )),
    Some((ToolButton::Command(MergeCommand::ResolveSimple), "diff-magic-toolbar", "Resolve Simple Conflicts")),
    Some((ToolButton::Command(MergeCommand::Revert), "diff-revert", "Revert Conflict Resolution")),
    None,
    Some((ToolButton::Command(MergeCommand::Compare), "diff", "Compare Contents")),
    // The status text goes here.
    None,
    Some((ToolButton::Command(MergeCommand::Reload), "refresh", "Reload from Disk")),
    Some((ToolButton::Command(MergeCommand::Settings), "settings", "Settings")),
];

/// Index of the status text among [`TOOLBAR`].
const STATUS_AT: usize = 10;

/// The versions each comparison shows, left then right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Version {
    Yours,
    Base,
    Result,
    Theirs,
}

fn comparison(command: MergeCommand) -> Option<(Version, Version)> {
    Some(match command {
        MergeCommand::CompareYoursResult => (Version::Yours, Version::Result),
        MergeCommand::CompareResultTheirs => (Version::Result, Version::Theirs),
        MergeCommand::CompareYoursTheirs => (Version::Yours, Version::Theirs),
        MergeCommand::CompareBaseYours => (Version::Base, Version::Yours),
        MergeCommand::CompareBaseResult => (Version::Base, Version::Result),
        MergeCommand::CompareBaseTheirs => (Version::Base, Version::Theirs),
        _ => return None,
    })
}

/// The context menu of a side pane.
pub fn side_menu(side: MergeSide) -> Vec<MenuItem<MergeCommand>> {
    let resolve = if side == MergeSide::Left { "merge.resolveUsingYours" } else { "merge.resolveUsingTheirs" };
    vec![
        MenuItem::action("merge.accept"),
        MenuItem::action(resolve),
        MenuItem::action("merge.ignore"),
        MenuItem::Separator,
        MenuItem::action("merge.annotate"),
    ]
}

/// The context menu of the result.
pub fn result_menu() -> Vec<MenuItem<MergeCommand>> {
    vec![
        MenuItem::action("merge.resolveAutomatically"),
        MenuItem::action("merge.ignore"),
        MenuItem::Separator,
        MenuItem::action("merge.cut"),
        MenuItem::action("merge.copy"),
        MenuItem::action("merge.paste"),
        MenuItem::action("merge.selectAll"),
    ]
}

/// Compare Contents: the six comparisons, those with Yours or Theirs on the left first.
pub fn compare_menu() -> Vec<MenuItem<MergeCommand>> {
    vec![
        MenuItem::action("merge.compareYoursResult"),
        MenuItem::action("merge.compareResultTheirs"),
        MenuItem::action("merge.compareYoursTheirs"),
        MenuItem::Separator,
        MenuItem::action("merge.compareBaseYours"),
        MenuItem::action("merge.compareBaseResult"),
        MenuItem::action("merge.compareBaseTheirs"),
    ]
}

/// The settings menu: Automatically Apply Non-Conflicting Changes, shown checked when on.
pub fn settings_menu() -> Vec<MenuItem<MergeCommand>> {
    vec![MenuItem::action("merge.autoApply")]
}

/// "All changes processed", "3 changes left", "3 changes left, 1 conflict".
pub fn status_text(summary: &MergeSummary, i18n: &mut I18n) -> String {
    let (pending, conflicts) = (summary.pending, summary.conflicts);
    if pending == 0 {
        return i18n.tx("All changes processed");
    }
    let left = if pending == 1 { " change left" } else { " changes left" };
    let left = i18n.tx_built(&["", left], &[Param::from(pending)]);
    if conflicts == 0 {
        return left;
    }
    let conflict = if conflicts == 1 { " conflict" } else { " conflicts" };
    format!("{left}, {}", i18n.tx_built(&["", conflict], &[Param::from(conflicts)]))
}

/// `text` with every line break as `\n`, as the panes and the model count offsets.
fn normalized(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Where `pos` of the result before `edits` (ordered, in offsets before them) is after them: a
/// position inside replaced text goes to the end of the replacement, one at its start stays there.
fn map_offset(edits: &[MergeEdit], pos: usize) -> usize {
    let mut delta = 0_isize;
    for edit in edits {
        if pos <= edit.from {
            break;
        }
        let inserted = utf16_len(&edit.insert);
        if pos < edit.to {
            return edit.from.saturating_add_signed(delta) + inserted;
        }
        delta += inserted as isize - (edit.to - edit.from) as isize;
    }
    pos.saturating_add_signed(delta)
}

fn slot(pane: MergePane) -> usize {
    match pane {
        MergePane::Side(MergeSide::Left) => 0,
        MergePane::Result => 1,
        MergePane::Side(MergeSide::Right) => 2,
    }
}

fn pane_name(pane: MergePane) -> &'static str {
    match pane {
        MergePane::Side(MergeSide::Left) => "left",
        MergePane::Result => "result",
        MergePane::Side(MergeSide::Right) => "right",
    }
}

fn side_name(side: MergeSide) -> &'static str {
    match side {
        MergeSide::Left => "left",
        MergeSide::Right => "right",
    }
}

fn done_on(change: &MergeChange, side: MergeSide) -> bool {
    match side {
        MergeSide::Left => change.done.left,
        MergeSide::Right => change.done.right,
    }
}

fn range_of(change: &MergeChange, side: MergeSide) -> (usize, usize) {
    match side {
        MergeSide::Left => change.left_range,
        MergeSide::Right => change.right_range,
    }
}

/// Whether the side's change is still to be decided: it is drawn, banded and offered there.
fn open_on(change: &MergeChange, side: MergeSide) -> bool {
    change.changes_side(side) && !done_on(change, side)
}

/// Name of the theme token a change is tinted with in a side pane and a divider.
fn change_token(change: &MergeChange) -> &'static str {
    if change.is_conflict() { "vcs-conflict" } else { "vcs-modified" }
}

/// UI text in the active language; `text` may be built from a template of the catalog.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx_template(text))
}

/// The height of a line of the panes.
fn line_height(cx: &App) -> Pixels {
    let height = theme::length(cx, "lh-code");
    if height > px(0.0) { height } else { px(20.0) }
}

/// The first font of the theme's `--font-mono` list that is installed.
fn code_font(cx: &App) -> Font {
    let families =
        cx.try_global::<theme::ActiveTheme>().map(|t| t.theme.font_families("font-mono")).unwrap_or_default();
    let installed = cx.text_system().all_font_names();
    let family =
        families.into_iter().find(|f| installed.iter().any(|name| name == f)).unwrap_or_else(|| "Menlo".to_owned());
    font(SharedString::from(family))
}

/// The next character boundary after (before) `pos`: a surrogate pair is one character.
fn char_boundary(doc: &Doc, pos: usize, forward: bool) -> usize {
    let units = doc.units();
    let high = |unit: u16| (0xD800..=0xDBFF).contains(&unit);
    let low = |unit: u16| (0xDC00..=0xDFFF).contains(&unit);
    if forward {
        if pos >= units.len() {
            return units.len();
        }
        if high(units[pos]) && units.get(pos + 1).is_some_and(|unit| low(*unit)) { pos + 2 } else { pos + 1 }
    } else {
        if pos == 0 {
            return 0;
        }
        let pos = pos.min(units.len());
        if pos >= 2 && low(units[pos - 1]) && high(units[pos - 2]) { pos - 2 } else { pos - 1 }
    }
}

/// A pane's document as [`crate::vcs_diff::diff_scroll`] measures it: lines of one height.
struct Geometry<'a> {
    doc: &'a Doc,
    line_height: f64,
}

impl Geometry<'_> {
    fn height(&self) -> f64 {
        self.doc.lines() as f64 * self.line_height
    }
}

impl PaneLayout for Geometry<'_> {
    fn doc_len(&self) -> usize {
        self.doc.len()
    }

    fn line_top(&self, pos: usize) -> f64 {
        (self.doc.line_at(pos).number - 1) as f64 * self.line_height
    }

    fn line_bottom(&self, pos: usize) -> f64 {
        self.doc.line_at(pos).number as f64 * self.line_height
    }
}

/// The caret and selection of a pane: from `anchor` to `head`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Caret {
    anchor: usize,
    head: usize,
}

impl Caret {
    fn at(pos: usize) -> Self {
        Self { anchor: pos, head: pos }
    }

    fn range(self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
}

/// What a pane drew last, for the pointer and the input method.
struct Painted {
    bounds: Bounds<Pixels>,
    text_left: Pixels,
    line_height: Pixels,
    first_line: usize,
    /// The text of each drawn line from `first_line`, shaped.
    lines: Vec<(String, ShapedLine)>,
}

#[derive(Default)]
struct PaneState {
    scroll_top: Pixels,
    height: Option<Pixels>,
    painted: Option<Painted>,
}

/// The merge in progress.
struct Loaded {
    model: MergeModel,
    /// Yours and Theirs as the side panes show them.
    sides: [Doc; 2],
    /// Yours and Theirs as stored, which Accept Yours / Accept Theirs write; `None` when that side
    /// deleted the file.
    versions: [Option<String>; 2],
    base: Option<String>,
    titles: MergeTitles,
    /// The file breaks its lines with `\r\n`: the result is saved so.
    crlf: bool,
    /// Steps made in the result, and how many of them are the starting point (the non-conflicting
    /// changes applied automatically), which is not undone and does not count as a change.
    steps: usize,
    floor: usize,
}

impl Loaded {
    fn side(&self, side: MergeSide) -> &Doc {
        match side {
            MergeSide::Left => &self.sides[0],
            MergeSide::Right => &self.sides[1],
        }
    }

    fn doc(&self, pane: MergePane) -> &Doc {
        match pane {
            MergePane::Side(side) => self.side(side),
            MergePane::Result => self.model.doc(),
        }
    }

    fn version_text(&self, version: Version) -> String {
        match version {
            Version::Yours => self.versions[0].clone().unwrap_or_default(),
            Version::Theirs => self.versions[1].clone().unwrap_or_default(),
            Version::Base => self.base.clone().unwrap_or_default(),
            Version::Result => self.model.text(),
        }
    }

    /// The result as it is saved: with the file's own line breaks.
    fn result_text(&self) -> String {
        let text = self.model.text();
        if self.crlf { text.replace('\n', "\r\n") } else { text }
    }
}

pub struct MergeView {
    focus: FocusHandle,
    path: String,
    /// Automatically Apply Non-Conflicting Changes.
    auto_apply: bool,
    loaded: Option<Loaded>,
    error: Option<String>,
    resolved: bool,
    /// Left, result and right.
    carets: [Caret; 3],
    active: MergePane,
    panes: [PaneState; 3],
    dragging: Option<MergePane>,
    /// The text being composed by an input method in the result.
    marked: Option<Range<usize>>,
    key_claim: u64,
}

impl EventEmitter<MergeViewEvent> for MergeView {}

/// The text shown when a version holds binary data.
pub const BINARY_TEXT: &str = "Binary files cannot be resolved in the merge editor.";

impl MergeView {
    /// The merge editor of the conflicted file `path`, waiting for its versions; `auto_apply` is the
    /// stored Automatically Apply Non-Conflicting Changes.
    pub fn new(path: &str, auto_apply: bool, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let weak = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| weak.update(cx, |view, cx| view.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|view, cx| input::release_keys(view.key_claim, cx)).detach();
        Self {
            focus,
            path: path.to_owned(),
            auto_apply,
            loaded: None,
            error: None,
            resolved: false,
            carets: [Caret::default(); 3],
            active: MergePane::Result,
            panes: Default::default(),
            dragging: None,
            marked: None,
            key_claim,
        }
    }

    // ---------------------------------------------------------------------------------------------
    // What the workbench tells the view

    /// Shows the three versions of the file, nothing decided yet (but the non-conflicting changes,
    /// with Automatically Apply Non-Conflicting Changes on). A version holding binary data cannot be
    /// merged.
    pub fn set_versions(&mut self, versions: MergeVersions, cx: &mut Context<Self>) {
        let all = [&versions.base, &versions.ours, &versions.theirs];
        if all.iter().any(|text| text.as_ref().is_some_and(|text| text.contains('\0'))) {
            self.set_error(BINARY_TEXT, cx);
            return;
        }
        let reference = versions.base.as_ref().or(versions.ours.as_ref()).or(versions.theirs.as_ref());
        let crlf = reference.is_some_and(|text| text.contains("\r\n"));
        let ours = normalized(versions.ours.as_deref().unwrap_or_default());
        let theirs = normalized(versions.theirs.as_deref().unwrap_or_default());
        let mut model = MergeModel::start(&ours, versions.base.as_deref().unwrap_or_default(), &theirs);
        let mut steps = 0;
        if self.auto_apply
            && let Some(start) = model.state().apply_non_conflicting(AutoSide::Base)
        {
            model.dispatch(&start);
            steps = 1;
        }
        self.loaded = Some(Loaded {
            model,
            sides: [Doc::new(&ours), Doc::new(&theirs)],
            versions: [versions.ours, versions.theirs],
            base: versions.base,
            titles: merge_titles(Some(&versions.sides)),
            crlf,
            steps,
            floor: steps,
        });
        self.error = None;
        self.resolved = false;
        self.carets = [Caret::default(); 3];
        self.marked = None;
        for pane in &mut self.panes {
            pane.scroll_top = px(0.0);
        }
        self.changed(cx);
    }

    /// The versions could not be read: the message shows with Retry.
    pub fn set_error(&mut self, message: &str, cx: &mut Context<Self>) {
        self.error = Some(message.to_owned());
        self.loaded = None;
        self.changed(cx);
    }

    /// The resolved file was written and staged: the editor says so, with Open File.
    pub fn set_resolved(&mut self, cx: &mut Context<Self>) {
        self.resolved = true;
        self.changed(cx);
    }

    /// The answer to a question the view asked ([`MergeViewEvent::Confirm`]).
    pub fn answer(&mut self, question: MergeQuestion, yes: bool, cx: &mut Context<Self>) {
        if !yes {
            return;
        }
        match question {
            MergeQuestion::Unresolved(_) => self.resolve_with_result(cx),
            MergeQuestion::Discard(side) => self.resolve_with_side(side, cx),
            MergeQuestion::Cancel => cx.emit(MergeViewEvent::Close),
        }
    }

    // ---------------------------------------------------------------------------------------------
    // What the view shows

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn is_resolved(&self) -> bool {
        self.resolved
    }

    pub fn auto_apply(&self) -> bool {
        self.auto_apply
    }

    /// The result's text, once the versions are shown.
    pub fn text(&self) -> Option<String> {
        self.loaded.as_ref().map(|loaded| loaded.model.text())
    }

    /// The changes of the merge, in document order.
    pub fn changes(&self) -> &[MergeChange] {
        match &self.loaded {
            Some(loaded) => loaded.model.changes(),
            None => &[],
        }
    }

    /// The English titles of the side panes.
    pub fn titles(&self) -> Option<&MergeTitles> {
        self.loaded.as_ref().map(|loaded| &loaded.titles)
    }

    /// How much is left and which toolbar commands have something to do.
    pub fn summary(&self) -> MergeSummary {
        self.loaded.as_ref().map_or(NOTHING_LEFT, |loaded| loaded.model.state().summarize())
    }

    /// Whether the user decided or edited anything: what Accept Yours / Theirs and Cancel ask about.
    pub fn is_modified(&self) -> bool {
        self.loaded.as_ref().is_some_and(|loaded| loaded.steps > loaded.floor)
    }

    /// The pane the caret is in.
    pub fn active_pane(&self) -> MergePane {
        self.active
    }

    /// The selection of `pane`, in its document.
    pub fn selection(&self, pane: MergePane) -> Range<usize> {
        self.carets[slot(pane)].range()
    }

    /// The token of the colour each line of `pane` is tinted with: a change still to be decided
    /// there (in the result: each change not resolved yet).
    pub fn line_tints(&self, pane: MergePane) -> Vec<Option<&'static str>> {
        let Some(loaded) = &self.loaded else { return Vec::new() };
        let tints = self.tints(loaded, pane);
        (0..loaded.doc(pane).lines()).map(|line| tints.get(&line).and_then(|tint| tint.0)).collect()
    }

    pub fn toolbar(&self) -> Vec<ToolbarButton> {
        let open = self.summary().pending > 0 && !self.resolved;
        TOOLBAR
            .iter()
            .flatten()
            .map(|&(button, icon, title)| {
                let enabled = match button {
                    ToolButton::Previous | ToolButton::Next => open,
                    ToolButton::Command(command) => self.enabled(command),
                };
                ToolbarButton { button, icon, title, enabled }
            })
            .collect()
    }

    // ---------------------------------------------------------------------------------------------
    // Commands

    /// The changes the selection of the active pane covers, and what `command` does to them.
    fn selection_decision(&self, command: MergeCommand) -> Option<(Vec<usize>, MergeAction)> {
        let loaded = self.loaded.as_ref()?;
        let range = self.carets[slot(self.active)].range();
        let state = loaded.model.state();
        let indexes = match self.active {
            MergePane::Result => state.changes_in(range.start, range.end),
            MergePane::Side(side) => state.changes_in_side(side, loaded.side(side), range.start, range.end),
        };
        let action = match (command, self.active) {
            (MergeCommand::ResolveAutomatically, _) => MergeAction::Auto { side: AutoSide::Base },
            (MergeCommand::Ignore, MergePane::Result) => MergeAction::Ignore { side: None },
            (MergeCommand::Ignore, MergePane::Side(side)) => MergeAction::Ignore { side: Some(side) },
            (MergeCommand::ResolveUsingYours, _) => MergeAction::Accept { side: MergeSide::Left, resolve: true },
            (MergeCommand::ResolveUsingTheirs, _) => MergeAction::Accept { side: MergeSide::Right, resolve: true },
            (MergeCommand::Accept, MergePane::Side(side)) => MergeAction::Accept { side, resolve: false },
            _ => return None,
        };
        Some((indexes, action))
    }

    /// Whether `command` has something to do now.
    pub fn enabled(&self, command: MergeCommand) -> bool {
        if matches!(command, MergeCommand::Reload | MergeCommand::Cancel) {
            return !self.resolved;
        }
        let Some(loaded) = self.loaded.as_ref().filter(|_| self.error.is_none() && !self.resolved) else {
            return false;
        };
        let summary = loaded.model.state().summarize();
        let selection = self.carets[slot(self.active)].range();
        match command {
            MergeCommand::ApplyLeft => summary.left,
            MergeCommand::ApplyAll => summary.all,
            MergeCommand::ApplyRight => summary.right,
            MergeCommand::ResolveSimple => summary.simple,
            MergeCommand::Revert => summary.revert,
            MergeCommand::Accept
            | MergeCommand::ResolveUsingYours
            | MergeCommand::ResolveUsingTheirs
            | MergeCommand::Ignore
            | MergeCommand::ResolveAutomatically => {
                self.selection_decision(command).is_some_and(|(indexes, action)| {
                    let changes = loaded.model.changes();
                    indexes.iter().any(|&i| changes.get(i).is_some_and(|c| can_apply(loaded.model.doc(), c, action)))
                })
            }
            // Blame annotations are not shown in the side panes.
            MergeCommand::Annotate => false,
            MergeCommand::Cut => self.active == MergePane::Result && !selection.is_empty(),
            MergeCommand::Copy => !selection.is_empty(),
            MergeCommand::Paste => self.active == MergePane::Result,
            _ => true,
        }
    }

    /// Runs `command`, when it has something to do.
    pub fn run(&mut self, command: MergeCommand, cx: &mut Context<Self>) {
        self.run_at(command, None, cx);
    }

    /// Runs `command`; a menu it opens goes to `position`.
    fn run_at(&mut self, command: MergeCommand, position: Option<Point<Pixels>>, cx: &mut Context<Self>) {
        if !self.enabled(command) {
            return;
        }
        match command {
            MergeCommand::ApplyLeft => {
                self.transact(|s| s.apply_non_conflicting(AutoSide::Side(MergeSide::Left)), true, cx)
            }
            MergeCommand::ApplyAll => self.transact(|s| s.apply_non_conflicting(AutoSide::Base), true, cx),
            MergeCommand::ApplyRight => {
                self.transact(|s| s.apply_non_conflicting(AutoSide::Side(MergeSide::Right)), true, cx)
            }
            MergeCommand::ResolveSimple => self.transact(MergeState::resolve_simple, true, cx),
            MergeCommand::Revert => self.transact(MergeState::revert_resolved, true, cx),
            MergeCommand::AcceptYours => self.accept_side(MergeSide::Left, cx),
            MergeCommand::AcceptTheirs => self.accept_side(MergeSide::Right, cx),
            MergeCommand::Apply => self.apply(cx),
            MergeCommand::Cancel => {
                let event = if self.is_modified() {
                    MergeViewEvent::Confirm(MergeQuestion::Cancel)
                } else {
                    MergeViewEvent::Close
                };
                cx.emit(event);
            }
            MergeCommand::Reload => cx.emit(MergeViewEvent::Reload),
            MergeCommand::Compare => cx.emit(MergeViewEvent::ShowMenu { items: compare_menu(), position }),
            MergeCommand::Settings => cx.emit(MergeViewEvent::ShowMenu { items: settings_menu(), position }),
            MergeCommand::AutoApply => {
                self.auto_apply = !self.auto_apply;
                cx.emit(MergeViewEvent::AutoApplyChanged(self.auto_apply));
                if self.auto_apply {
                    self.transact(|s| s.apply_non_conflicting(AutoSide::Base), true, cx);
                }
                self.changed(cx);
            }
            MergeCommand::Accept
            | MergeCommand::ResolveUsingYours
            | MergeCommand::ResolveUsingTheirs
            | MergeCommand::Ignore
            | MergeCommand::ResolveAutomatically => {
                if let Some((indexes, action)) = self.selection_decision(command) {
                    self.transact(|s| s.apply_to(&indexes, action), false, cx);
                }
            }
            MergeCommand::Cut => {
                if self.copy(cx) {
                    self.edit_result(None, "", cx);
                }
            }
            MergeCommand::Copy => {
                self.copy(cx);
            }
            MergeCommand::Paste => self.paste(cx),
            MergeCommand::SelectAll => self.select_all(cx),
            // Resolving one change takes the change: see [`MergeView::apply_control`].
            MergeCommand::Change | MergeCommand::Annotate => {}
            MergeCommand::CompareYoursResult
            | MergeCommand::CompareResultTheirs
            | MergeCommand::CompareYoursTheirs
            | MergeCommand::CompareBaseYours
            | MergeCommand::CompareBaseResult
            | MergeCommand::CompareBaseTheirs => self.compare(command, cx),
        }
    }

    fn compare(&mut self, command: MergeCommand, cx: &mut Context<Self>) {
        let (Some(loaded), Some((a, b))) = (&self.loaded, comparison(command)) else { return };
        let label = |version: Version| match version {
            Version::Yours => tx(cx, &loaded.titles.yours),
            Version::Theirs => tx(cx, &loaded.titles.theirs),
            Version::Base => tx(cx, "Base"),
            Version::Result => tx(cx, "Result"),
        };
        let left = CompareSide { label: label(a), text: loaded.version_text(a) };
        let right = CompareSide { label: label(b), text: loaded.version_text(b) };
        cx.emit(MergeViewEvent::Compare { left, right });
    }

    /// A press on an Accept arrow, an Ignore cross or a wand.
    pub fn apply_control(&mut self, control: MergeControl, cx: &mut Context<Self>) {
        let (index, action) = match control {
            MergeControl::Accept { index, side, resolve } => (index, MergeAction::Accept { side, resolve }),
            MergeControl::Ignore { index, side, resolve } => {
                (index, MergeAction::Ignore { side: (!resolve).then_some(side) })
            }
            MergeControl::ResolveAutomatically { index } => (index, MergeAction::Auto { side: AutoSide::Base }),
        };
        if self.resolved {
            return;
        }
        self.transact(|s| s.apply_to(&[index], action), false, cx);
    }

    /// Carries out the step `make` returns as one undo step; `reveal` then moves the caret to the
    /// first change still open.
    fn transact(
        &mut self,
        make: impl FnOnce(&MergeState) -> Option<MergeTransaction>,
        reveal: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(transaction) = self.loaded.as_ref().and_then(|loaded| make(loaded.model.state())) else { return };
        self.dispatch(&transaction, cx);
        let first = self.changes().iter().find(|c| !c.is_resolved()).map(|c| c.from);
        if let (true, Some(first)) = (reveal, first) {
            self.carets[slot(MergePane::Result)] = Caret::at(first);
            self.center(MergePane::Result, first, cx);
        }
    }

    /// Carries out `transaction`, the result's caret following the text it changes.
    fn dispatch(&mut self, transaction: &MergeTransaction, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded.as_mut() else { return };
        let caret = &mut self.carets[slot(MergePane::Result)];
        let edits = &transaction.edits;
        *caret = Caret { anchor: map_offset(edits, caret.anchor), head: map_offset(edits, caret.head) };
        loaded.model.dispatch(transaction);
        loaded.steps += 1;
        self.changed(cx);
    }

    /// Takes the last step back, down to the starting point.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded.as_mut().filter(|loaded| loaded.steps > loaded.floor) else { return };
        if loaded.model.undo() {
            loaded.steps -= 1;
            self.after_history(cx);
        }
    }

    /// Makes the last undone step again.
    pub fn redo(&mut self, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded.as_mut() else { return };
        if loaded.model.redo() {
            loaded.steps += 1;
            self.after_history(cx);
        }
    }

    fn after_history(&mut self, cx: &mut Context<Self>) {
        let len = self.loaded.as_ref().map_or(0, |loaded| loaded.model.doc().len());
        let caret = &mut self.carets[slot(MergePane::Result)];
        *caret = Caret { anchor: caret.anchor.min(len), head: caret.head.min(len) };
        self.marked = None;
        self.changed(cx);
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(MergeViewEvent::CommandsChanged);
        cx.notify();
    }

    /// Apply: saves the result, asking first while changes are left unresolved.
    fn apply(&mut self, cx: &mut Context<Self>) {
        let remaining = self.summary().pending;
        if remaining > 0 {
            cx.emit(MergeViewEvent::Confirm(MergeQuestion::Unresolved(remaining)));
        } else {
            self.resolve_with_result(cx);
        }
    }

    fn resolve_with_result(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.loaded.as_ref().map(Loaded::result_text) else { return };
        cx.emit(MergeViewEvent::Resolve { path: self.path.clone(), text: Some(text) });
    }

    /// Accept Yours / Accept Theirs: that version as it is, whatever was done in the result (asking
    /// first when something was).
    fn accept_side(&mut self, side: MergeSide, cx: &mut Context<Self>) {
        if self.is_modified() {
            cx.emit(MergeViewEvent::Confirm(MergeQuestion::Discard(side)));
        } else {
            self.resolve_with_side(side, cx);
        }
    }

    fn resolve_with_side(&mut self, side: MergeSide, cx: &mut Context<Self>) {
        let Some(loaded) = &self.loaded else { return };
        let text = match side {
            MergeSide::Left => loaded.versions[0].clone(),
            MergeSide::Right => loaded.versions[1].clone(),
        };
        cx.emit(MergeViewEvent::Resolve { path: self.path.clone(), text });
    }

    /// Next / Previous Change: the caret goes to the next (previous) change still open, round to the
    /// first (last).
    pub fn go_to_change(&mut self, direction: Direction, cx: &mut Context<Self>) {
        let open: Vec<usize> = self.changes().iter().filter(|c| !c.is_resolved()).map(|c| c.from).collect();
        let head = self.carets[slot(MergePane::Result)].head;
        let target = match direction {
            Direction::Next => open.iter().find(|from| **from > head).or(open.first()),
            Direction::Previous => open.iter().rev().find(|from| **from < head).or(open.last()),
        };
        let Some(&target) = target else { return };
        self.active = MergePane::Result;
        self.carets[slot(MergePane::Result)] = Caret::at(target);
        self.center(MergePane::Result, target, cx);
        self.changed(cx);
    }

    // ---------------------------------------------------------------------------------------------
    // Editing the result

    /// Replaces `range` of the result (the selection when none is given) with `text`, as one step.
    fn edit_result(&mut self, range: Option<Range<usize>>, text: &str, cx: &mut Context<Self>) {
        if self.resolved || self.active != MergePane::Result {
            return;
        }
        let Some(len) = self.loaded.as_ref().map(|loaded| loaded.model.doc().len()) else { return };
        let range = range.unwrap_or_else(|| self.carets[slot(MergePane::Result)].range());
        let (from, to) = (range.start.min(len), range.end.min(len));
        let text = normalized(text);
        if from == to && text.is_empty() {
            return;
        }
        self.dispatch(&MergeTransaction::edit(from, to, &text), cx);
        self.carets[slot(MergePane::Result)] = Caret::at(from + utf16_len(&text));
        self.reveal_caret(MergePane::Result, cx);
    }

    /// Backspace / Delete: the selection, else the character before (after) the caret.
    fn delete(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(loaded) = &self.loaded else { return };
        let caret = self.carets[slot(MergePane::Result)];
        let range = if caret.anchor == caret.head {
            let to = char_boundary(loaded.model.doc(), caret.head, forward);
            to.min(caret.head)..to.max(caret.head)
        } else {
            caret.range()
        };
        if !range.is_empty() {
            self.edit_result(Some(range), "", cx);
        }
    }

    fn selected_text(&self) -> Option<String> {
        let loaded = self.loaded.as_ref()?;
        let range = self.carets[slot(self.active)].range();
        (!range.is_empty()).then(|| loaded.doc(self.active).slice_string(range.start, range.end))
    }

    /// Copies the selection of the active pane; whether there was one.
    fn copy(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(text) = self.selected_text() else { return false };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    fn paste(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.edit_result(None, &text, cx);
        }
    }

    fn select_all(&mut self, cx: &mut Context<Self>) {
        let Some(len) = self.loaded.as_ref().map(|loaded| loaded.doc(self.active).len()) else { return };
        self.carets[slot(self.active)] = Caret { anchor: 0, head: len };
        self.changed(cx);
    }

    /// Moves the caret of the active pane to `to`; `extend` keeps the selection's anchor.
    fn move_caret(&mut self, to: usize, extend: bool, cx: &mut Context<Self>) {
        let caret = &mut self.carets[slot(self.active)];
        caret.head = to;
        if !extend {
            caret.anchor = to;
        }
        self.reveal_caret(self.active, cx);
        self.changed(cx);
    }

    /// Left / Right: a character, or to the side of the selection the arrow points at.
    fn move_horizontal(&mut self, forward: bool, extend: bool, cx: &mut Context<Self>) {
        let Some(loaded) = &self.loaded else { return };
        let caret = self.carets[slot(self.active)];
        let to = if !extend && caret.anchor != caret.head {
            if forward { caret.range().end } else { caret.range().start }
        } else {
            char_boundary(loaded.doc(self.active), caret.head, forward)
        };
        self.move_caret(to, extend, cx);
    }

    /// Up / Down by `lines`, keeping the column where the line is long enough.
    fn move_vertical(&mut self, lines: isize, extend: bool, cx: &mut Context<Self>) {
        let Some(loaded) = &self.loaded else { return };
        let doc = loaded.doc(self.active);
        let head = self.carets[slot(self.active)].head;
        let line = doc.line_at(head);
        let target = doc.line(line.number.saturating_add_signed(lines).max(1));
        let to = target.from + (head - line.from).min(target.to - target.from);
        self.move_caret(to, extend, cx);
    }

    /// Home / End: the start (end) of the caret's line, or of the document.
    fn move_to_edge(&mut self, end: bool, whole: bool, extend: bool, cx: &mut Context<Self>) {
        let Some(loaded) = &self.loaded else { return };
        let doc = loaded.doc(self.active);
        let line = doc.line_at(self.carets[slot(self.active)].head);
        let to = match (whole, end) {
            (true, false) => 0,
            (true, true) => doc.len(),
            (false, false) => line.from,
            (false, true) => line.to,
        };
        self.move_caret(to, extend, cx);
    }

    // ---------------------------------------------------------------------------------------------
    // Keyboard and pointer

    /// F7 / Shift+F7 move between the changes, ⌘Z / ⇧⌘Z undo and redo, ahead of the keymap.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if self.loaded.is_none() {
            return false;
        }
        let modifiers = &keystroke.modifiers;
        let primary = modifiers.secondary(cx) && !modifiers.alt;
        let plain = !modifiers.control && !modifiers.platform && !modifiers.alt;
        match keystroke.key.as_str() {
            "f7" if plain && modifiers.shift => self.go_to_change(Direction::Previous, cx),
            "f7" if plain => self.go_to_change(Direction::Next, cx),
            "z" if primary && !modifiers.shift => self.undo(cx),
            "z" if primary => self.redo(cx),
            "y" if primary && !modifiers.shift && !is_mac_host() => self.redo(cx),
            _ => return false,
        }
        true
    }

    /// Caret movement in the active pane; typing, Backspace, Delete, Enter and Tab in the result;
    /// ⌘A, ⌘C, ⌘X and ⌘V.
    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.loaded.is_none() || self.resolved {
            return;
        }
        let keystroke = &event.keystroke;
        let (modifiers, key) = (keystroke.modifiers, keystroke.key.as_str());
        let editable = self.active == MergePane::Result;
        if modifiers.secondary(cx) && !modifiers.alt && !modifiers.shift {
            let handled = match key {
                "a" => {
                    self.select_all(cx);
                    true
                }
                "c" => self.copy(cx),
                "x" if editable => {
                    let copied = self.copy(cx);
                    if copied {
                        self.edit_result(None, "", cx);
                    }
                    copied
                }
                "v" if editable => {
                    self.paste(cx);
                    true
                }
                _ => false,
            };
            if handled {
                cx.stop_propagation();
                return;
            }
        }
        let extend = modifiers.shift;
        let document = modifiers.platform || modifiers.control;
        let unmodified = !(modifiers.control || modifiers.alt || modifiers.platform || modifiers.function);
        let page = self.page_lines(self.active, cx);
        match key {
            "left" | "right" => self.move_horizontal(key == "right", extend, cx),
            "up" | "down" if document => self.move_to_edge(key == "down", true, extend, cx),
            "up" => self.move_vertical(-1, extend, cx),
            "down" => self.move_vertical(1, extend, cx),
            "pageup" => self.move_vertical(-page, extend, cx),
            "pagedown" => self.move_vertical(page, extend, cx),
            "home" | "end" => self.move_to_edge(key == "end", document, extend, cx),
            "backspace" | "delete" if editable => self.delete(key == "delete", cx),
            "enter" if editable && unmodified => self.edit_result(None, "\n", cx),
            "tab" if editable && !modifiers.modified() => self.edit_result(None, "\t", cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// The offset of `pane`'s document under `position`, as the pane was drawn last.
    fn offset_at(&self, pane: MergePane, position: Point<Pixels>) -> Option<usize> {
        let doc = self.loaded.as_ref()?.doc(pane);
        let state = &self.panes[slot(pane)];
        let painted = state.painted.as_ref()?;
        let y = position.y - painted.bounds.top() + state.scroll_top;
        let line = ((y / painted.line_height).floor().max(0.0) as usize).min(doc.lines() - 1);
        let at = doc.line(line + 1);
        let x = position.x - painted.bounds.left() - painted.text_left;
        let col = line
            .checked_sub(painted.first_line)
            .and_then(|i| painted.lines.get(i))
            .map_or(at.to - at.from, |(text, shaped)| col_of_byte(text, shaped.closest_index_for_x(x)) as usize);
        Some(at.from + col.min(at.to - at.from))
    }

    fn mouse_down(&mut self, pane: MergePane, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        // The press is the pane's: the root tracking focus does not take it back.
        window.prevent_default();
        let Some(at) = self.offset_at(pane, event.position) else { return };
        self.active = pane;
        self.marked = None;
        let caret = &mut self.carets[slot(pane)];
        *caret = Caret { anchor: if event.modifiers.shift { caret.anchor } else { at }, head: at };
        self.dragging = Some(pane);
        self.changed(cx);
    }

    fn mouse_move(&mut self, pane: MergePane, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) || self.dragging != Some(pane) {
            self.dragging = None;
            return;
        }
        let Some(at) = self.offset_at(pane, event.position) else { return };
        let caret = &mut self.carets[slot(pane)];
        if caret.head != at {
            caret.head = at;
            self.changed(cx);
        }
    }

    /// The context menu of a pane; a press outside the selection moves the caret there first.
    fn context_menu(&mut self, pane: MergePane, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        window.prevent_default();
        self.active = pane;
        if let Some(at) = self.offset_at(pane, event.position) {
            let caret = &mut self.carets[slot(pane)];
            let range = caret.range();
            if at < range.start || at > range.end {
                *caret = Caret::at(at);
            }
        }
        self.changed(cx);
        let items = match pane {
            MergePane::Side(side) => side_menu(side),
            MergePane::Result => result_menu(),
        };
        cx.emit(MergeViewEvent::ShowMenu { items, position: Some(event.position) });
    }

    // ---------------------------------------------------------------------------------------------
    // Scrolling

    fn height(&self, pane: MergePane) -> Pixels {
        self.panes[slot(pane)].height.unwrap_or(px(DEFAULT_HEIGHT))
    }

    fn page_lines(&self, pane: MergePane, cx: &App) -> isize {
        ((self.height(pane) / line_height(cx)).floor() as isize).max(1)
    }

    fn lines_of(&self, pane: MergePane) -> usize {
        self.loaded.as_ref().map_or(1, |loaded| loaded.doc(pane).lines())
    }

    fn max_scroll(&self, pane: MergePane, cx: &App) -> Pixels {
        (line_height(cx) * self.lines_of(pane) as f32 - self.height(pane)).max(px(0.0))
    }

    fn set_scroll(&mut self, pane: MergePane, top: Pixels, cx: &App) {
        let max = self.max_scroll(pane, cx);
        self.panes[slot(pane)].scroll_top = top.clamp(px(0.0), max);
    }

    fn pane_scroll(&self, pane: MergePane, cx: &App) -> PaneScroll {
        PaneScroll {
            top: f64::from(self.panes[slot(pane)].scroll_top),
            left: 0.0,
            client_height: f64::from(self.height(pane)),
            scroll_height: self.lines_of(pane) as f64 * f64::from(line_height(cx)),
            max_left: 0.0,
            doc_offset: 0.0,
        }
    }

    /// Scrolls `pane` by `delta` (positive: further down); the others follow.
    pub fn scroll_by(&mut self, pane: MergePane, delta: Pixels, cx: &mut Context<Self>) {
        let top = self.panes[slot(pane)].scroll_top + delta;
        self.set_scroll(pane, top, cx);
        self.follow(pane, cx);
        cx.notify();
    }

    pub fn scroll_top(&self, pane: MergePane) -> Pixels {
        self.panes[slot(pane)].scroll_top
    }

    /// Scrolls `pos` of `pane` to the middle of the pane; the others follow.
    fn center(&mut self, pane: MergePane, pos: usize, cx: &mut Context<Self>) {
        let Some(line) = self.loaded.as_ref().map(|loaded| loaded.doc(pane).line_at(pos).number - 1) else { return };
        let top = line_height(cx) * line as f32 - self.height(pane) * 0.5;
        self.set_scroll(pane, top, cx);
        self.follow(pane, cx);
    }

    /// Scrolls the caret of `pane` into view; the others follow.
    fn reveal_caret(&mut self, pane: MergePane, cx: &mut Context<Self>) {
        let Some(line) = self.loaded.as_ref().map(|l| l.doc(pane).line_at(self.carets[slot(pane)].head).number - 1)
        else {
            return;
        };
        let line_height = line_height(cx);
        let (top, bottom) = (line_height * line as f32, line_height * (line + 1) as f32);
        let scroll = self.panes[slot(pane)].scroll_top;
        let height = self.height(pane);
        if top < scroll {
            self.set_scroll(pane, top, cx);
        } else if bottom > scroll + height {
            self.set_scroll(pane, bottom - height, cx);
        } else {
            return;
        }
        self.follow(pane, cx);
    }

    /// Document y of a side pane in the result (`to_result`), or of the result in the side pane.
    fn transfer(&self, side: MergeSide, y: f64, to_result: bool, line_height: f64) -> f64 {
        let Some(loaded) = &self.loaded else { return y };
        let side_pane = Geometry { doc: loaded.side(side), line_height };
        let result = Geometry { doc: loaded.model.doc(), line_height };
        let ends = (side_pane.height(), result.height());
        transfer(
            loaded.model.changes(),
            |c| {
                let (from, to) = range_of(c, side);
                extent(&side_pane, from, to)
            },
            |c| extent(&result, c.from, c.to),
            ends,
            y,
            to_result,
        )
    }

    /// The other panes line up with `master`, a third down it, through the changes between them.
    fn follow(&mut self, master: MergePane, cx: &App) {
        let line_height = f64::from(line_height(cx));
        let (offset, y) = anchor(&self.pane_scroll(master, cx));
        for pane in PANES {
            if pane == master {
                continue;
            }
            let mapped =
                map_between(master, pane, y, |side, y, to_result| self.transfer(side, y, to_result, line_height));
            if let Some(top) = follow_top(&self.pane_scroll(pane, cx), mapped, offset) {
                self.set_scroll(pane, Pixels::from(top), cx);
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Drawing

    /// The tint of each line of `pane` that has one (its token, and whether its text is dimmed as
    /// settled), and the lines with a mark above them where a change has no lines.
    fn tints(&self, loaded: &Loaded, pane: MergePane) -> HashMap<usize, (Option<&'static str>, bool)> {
        let mut out = HashMap::new();
        let doc = loaded.doc(pane);
        let line_of = |pos: usize| doc.line_at(pos.min(doc.len())).number - 1;
        match pane {
            MergePane::Result => {
                for mark in result_marks(loaded.model.changes(), doc) {
                    if let ResultMark::Line { from, tint } = mark {
                        out.insert(line_of(from), (tint.color_token(), tint == MergeTint::Resolved));
                    }
                }
            }
            MergePane::Side(side) => {
                for change in loaded.model.changes().iter().filter(|c| open_on(c, side)) {
                    let (from, to) = range_of(change, side);
                    if to > from {
                        for line in line_of(from)..=line_of(to - 1) {
                            out.insert(line, (Some(change_token(change)), false));
                        }
                    }
                }
            }
        }
        out
    }

    /// The lines of `pane` with a thin mark above them: where a change that has no lines there goes.
    fn points(&self, loaded: &Loaded, pane: MergePane) -> HashMap<usize, &'static str> {
        let doc = loaded.doc(pane);
        let line_of = |pos: usize| doc.line_at(pos.min(doc.len())).number - 1;
        match pane {
            MergePane::Result => result_marks(loaded.model.changes(), doc)
                .into_iter()
                .filter_map(|mark| match mark {
                    ResultMark::Point { at, tint } => tint.color_token().map(|token| (line_of(at), token)),
                    ResultMark::Line { .. } => None,
                })
                .collect(),
            MergePane::Side(side) => loaded
                .model
                .changes()
                .iter()
                .filter(|c| open_on(c, side))
                .filter_map(|c| {
                    let (from, to) = range_of(c, side);
                    (to == from).then(|| (line_of(from), change_token(c)))
                })
                .collect(),
        }
    }

    /// The lines of `pane` in view.
    fn visible_lines(&self, pane: MergePane, cx: &App) -> Range<usize> {
        let line_height = line_height(cx);
        let lines = self.lines_of(pane);
        let first = ((self.panes[slot(pane)].scroll_top / line_height).floor().max(0.0) as usize).min(lines);
        let count = (self.height(pane) / line_height).ceil().max(0.0) as usize + 1;
        first..(first + count).min(lines)
    }

    /// What `pane` paints in a pane `height` tall.
    fn snapshot(&self, pane: MergePane, height: Pixels, line_height: Pixels, focused: bool) -> Option<PaneSnapshot> {
        let loaded = self.loaded.as_ref()?;
        let doc = loaded.doc(pane);
        let state = &self.panes[slot(pane)];
        let first = ((state.scroll_top / line_height).floor().max(0.0) as usize).min(doc.lines());
        let count = (height / line_height).ceil().max(0.0) as usize + 1;
        let last = (first + count).min(doc.lines());
        let tints = self.tints(loaded, pane);
        let points = self.points(loaded, pane);
        let caret = self.carets[slot(pane)];
        let selection = caret.range();
        let show_caret = focused && self.active == pane;
        let caret_line = doc.line_at(caret.head.min(doc.len())).number - 1;
        let rows = (first..last)
            .map(|line| {
                let at = doc.line(line + 1);
                let col = |pos: usize| (pos.clamp(at.from, at.to + 1) - at.from) as u32;
                let (from, to) = (col(selection.start), col(selection.end));
                let (tint, dim) = tints.get(&line).copied().unwrap_or((None, false));
                RowPaint {
                    line,
                    text: doc.line_text(at),
                    tint,
                    dim,
                    point: points.get(&line).copied(),
                    selected: (to > from).then_some((from, to)),
                    caret: (show_caret && caret_line == line).then(|| col(caret.head.min(at.to))),
                }
            })
            .collect();
        let lines = doc.lines().max(1) as f32;
        let lane = merge_stripe(pane, loaded.model.changes())
            .into_iter()
            .map(|mark| {
                let top = (doc.line_at(mark.from.min(doc.len())).number - 1) as f32 / lines;
                let to = mark.to.unwrap_or(mark.from).min(doc.len());
                let bottom = doc.line_at(to).number as f32 / lines;
                (top, bottom, mark.color)
            })
            .collect();
        Some(PaneSnapshot { scroll_top: state.scroll_top, first_line: first, rows, lane })
    }

    fn painted(&mut self, pane: MergePane, painted: Painted) {
        let state = &mut self.panes[slot(pane)];
        state.height = Some(painted.bounds.size.height);
        state.painted = Some(painted);
    }

    /// The bands of the divider beside `side`'s pane, with their edges relative to the divider's
    /// top: left edge then right edge.
    fn bands(&self, side: MergeSide, line_height: f64) -> Vec<([f64; 4], &'static str)> {
        let Some(loaded) = &self.loaded else { return Vec::new() };
        let side_pane = Geometry { doc: loaded.side(side), line_height };
        let result = Geometry { doc: loaded.model.doc(), line_height };
        let off_side = -f64::from(self.panes[slot(MergePane::Side(side))].scroll_top);
        let off_result = -f64::from(self.panes[slot(MergePane::Result)].scroll_top);
        let height = f64::from(self.height(MergePane::Result));
        loaded
            .model
            .changes()
            .iter()
            .filter(|c| open_on(c, side))
            .filter_map(|c| {
                let (from, to) = range_of(c, side);
                let (s0, s1) = extent(&side_pane, from, to);
                let (r0, r1) = extent(&result, c.from, c.to);
                let (s0, s1, r0, r1) = (s0 + off_side, s1 + off_side, r0 + off_result, r1 + off_result);
                let edges = match side {
                    MergeSide::Left => [s0, s1, r0, r1],
                    MergeSide::Right => [r0, r1, s0, s1],
                };
                let visible = edges[1].max(edges[3]) >= 0.0 && edges[0].min(edges[2]) <= height;
                visible.then_some((edges, change_token(c)))
            })
            .collect()
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let buttons = self.toolbar();
        let summary = self.summary();
        let status = status_text(&summary, &mut cx.default_global::<Localization>().0);
        let mut bar = div()
            .debug_selector(|| "merge-toolbar".to_owned())
            .flex_none()
            .h(theme::length(cx, "h-header"))
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(6.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"));
        let mut next = buttons.into_iter();
        for (index, entry) in TOOLBAR.iter().enumerate() {
            if index == STATUS_AT {
                let color = theme::color(cx, if summary.pending == 0 { "fg-muted" } else { "vcs-conflict" });
                bar = bar.child(div().flex_1()).child(
                    div()
                        .debug_selector(|| "merge-status".to_owned())
                        .px(px(8.0))
                        .text_color(color)
                        .text_size(theme::length(cx, "fs-small"))
                        .child(SharedString::from(status.clone())),
                );
                continue;
            }
            if entry.is_none() {
                bar = bar.child(div().w(px(1.0)).h(px(14.0)).mx(px(4.0)).bg(theme::color(cx, "border-strong")));
                continue;
            }
            let Some(button) = next.next() else { break };
            let selector = format!("merge-tool-{}", button.button.name());
            let color = theme::color(cx, if button.enabled { "fg-muted" } else { "fg-faint" });
            let pressed = button.button;
            bar = bar.child(
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .size(px(22.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme::length(cx, "radius-small"))
                    .when(button.enabled, |el| el.cursor_pointer().hover(|el| el.bg(theme::color(cx, "bg-hover"))))
                    .when(!button.enabled, |el| el.opacity(0.5))
                    .with_tooltip(button.title)
                    .child(theme::icon(cx, button.icon, 14.0, color))
                    .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                        window.focus(&view.focus, cx);
                        match pressed {
                            ToolButton::Previous => view.go_to_change(Direction::Previous, cx),
                            ToolButton::Next => view.go_to_change(Direction::Next, cx),
                            ToolButton::Command(command) => view.run_at(command, Some(event.position()), cx),
                        }
                    })),
            );
        }
        bar
    }

    fn render_labels(&self, loaded: &Loaded, cx: &mut Context<Self>) -> Div {
        let label = |text: String, first: bool| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .px(px(12.0))
                .overflow_hidden()
                .whitespace_nowrap()
                .when(!first, |el| el.border_l_1().border_color(theme::color(cx, "border")))
                .child(SharedString::from(text))
        };
        div()
            .debug_selector(|| "merge-labels".to_owned())
            .flex_none()
            .h(theme::length(cx, "h-row"))
            .flex()
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .text_color(theme::color(cx, "fg-secondary"))
            .text_size(theme::length(cx, "fs-small"))
            .child(label(tx(cx, &loaded.titles.yours), true))
            .child(label(tx(cx, "Result"), false))
            .child(label(tx(cx, &loaded.titles.theirs), false))
    }

    /// A small square button over a pane or a divider.
    fn control_button(
        &self,
        selector: String,
        icon: &'static str,
        title: &'static str,
        cx: &mut Context<Self>,
        control: impl Fn(bool) -> MergeControl + 'static,
    ) -> gpui_kit::Stateful<Div> {
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .absolute()
            .size(px(16.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .cursor_pointer()
            .hover(|el| el.bg(theme::color(cx, "bg-control-hover")))
            .with_tooltip(title)
            .child(theme::icon(cx, icon, 14.0, theme::color(cx, "fg-secondary")))
            // Pointer presses keep the panes' focus and selection.
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                window.focus(&view.focus, cx);
                let resolve = event.modifiers().secondary(cx);
                view.apply_control(control(resolve), cx);
            }))
    }

    fn render_pane(&self, pane: MergePane, cx: &mut Context<Self>) -> Div {
        let line_height = line_height(cx);
        let scroll_top = self.panes[slot(pane)].scroll_top;
        let mut overlays: Vec<AnyElement> = Vec::new();
        if pane == MergePane::Result
            && let Some(loaded) = &self.loaded
        {
            let doc = loaded.model.doc();
            let gutter = theme::length(cx, "w-gutter");
            let wands: Vec<(usize, MergeControl)> = self
                .visible_lines(pane, cx)
                .filter_map(|line| wand_marker(loaded.model.changes(), doc, doc.line(line + 1).from).map(|w| (line, w)))
                .collect();
            for (line, wand) in wands {
                let MergeControl::ResolveAutomatically { index } = wand else { continue };
                let top = line_height * line as f32 - scroll_top + (line_height - px(16.0)) * 0.5;
                let button = self
                    .control_button(format!("merge-wand-{index}"), WAND_ICON, WAND_TITLE, cx, move |_| wand)
                    .top(top)
                    .left(gutter + px(1.0));
                overlays.push(button.into_any_element());
            }
        }
        let selector = format!("merge-pane-{}", pane_name(pane));
        div()
            .debug_selector(move || selector.clone())
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| view.mouse_down(pane, event, window, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| view.context_menu(pane, event, window, cx)),
            )
            .on_mouse_move(cx.listener(move |view, event: &MouseMoveEvent, _, cx| view.mouse_move(pane, event, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|view, _: &MouseUpEvent, _, _| view.dragging = None))
            .child(PaneElement { view: cx.entity(), pane })
            .children(overlays)
    }

    /// The divider beside `side`'s pane: the bands of the side's open changes, and their Accept and
    /// Ignore buttons at the first line of each.
    fn render_divider(&self, side: MergeSide, cx: &mut Context<Self>) -> Div {
        let line_height = line_height(cx);
        let pane = MergePane::Side(side);
        let scroll_top = self.panes[slot(pane)].scroll_top;
        let markers: Vec<(usize, SideMarker)> = match &self.loaded {
            Some(loaded) => {
                let doc = loaded.side(side);
                self.visible_lines(pane, cx)
                    .filter_map(|line| {
                        side_marker(loaded.model.changes(), side, doc, doc.line(line + 1).from).map(|m| (line, m))
                    })
                    .collect()
            }
            None => Vec::new(),
        };
        let mut column = div()
            .debug_selector(move || format!("merge-divider-{}", side_name(side)))
            .relative()
            .flex_none()
            .w(px(DIVIDER_WIDTH))
            .h_full()
            .overflow_hidden()
            .bg(theme::color(cx, "bg-app"))
            .border_l_1()
            .border_r_1()
            .border_color(theme::color(cx, "border"))
            .child(DividerElement { view: cx.entity(), side });
        for (line, marker) in markers {
            let top = line_height * line as f32 - scroll_top + (line_height - px(16.0)) * 0.5;
            let name = side_name(side);
            let index = marker.index;
            let accept = self
                .control_button(
                    format!("merge-accept-{name}-{index}"),
                    marker.accept_icon(),
                    SideMarker::ACCEPT_TITLE,
                    cx,
                    move |resolve| marker.accept(resolve),
                )
                .top(top);
            let ignore = self
                .control_button(
                    format!("merge-ignore-{name}-{index}"),
                    SideMarker::IGNORE_ICON,
                    SideMarker::IGNORE_TITLE,
                    cx,
                    move |resolve| marker.ignore(resolve),
                )
                .top(top);
            // The Accept arrow is on the result's side of the divider.
            let (accept, ignore) = match side {
                MergeSide::Left => (accept.right(px(1.0)), ignore.left(px(1.0))),
                MergeSide::Right => (accept.left(px(1.0)), ignore.right(px(1.0))),
            };
            column = column.child(accept).child(ignore);
        }
        column
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> Div {
        let button = |view: &Self, cx: &mut Context<Self>, command: MergeCommand, label: &str, primary: bool| {
            let enabled = view.enabled(command);
            let label = tx(cx, label);
            let selector = format!("merge-footer-{}", command.name());
            let face = div()
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector.clone())
                .flex_none()
                .h(px(BUTTON_HEIGHT))
                .min_w(px(72.0))
                .px(px(12.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme::length(cx, "radius-small"))
                .border_1()
                .whitespace_nowrap()
                .child(SharedString::from(label))
                .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                    window.focus(&view.focus, cx);
                    view.run(command, cx);
                }));
            let face = if primary {
                let fill = theme::color(cx, "bg-button-primary");
                face.bg(fill).border_color(fill).text_color(theme::color(cx, "fg-button-primary"))
            } else {
                face.bg(theme::color(cx, "bg-button"))
                    .border_color(theme::color(cx, "border-button"))
                    .text_color(theme::color(cx, "fg-strong"))
            };
            if enabled { face } else { face.opacity(0.5) }
        };
        let accept_yours = button(self, cx, MergeCommand::AcceptYours, "Accept Yours", false);
        let accept_theirs = button(self, cx, MergeCommand::AcceptTheirs, "Accept Theirs", false);
        let cancel = button(self, cx, MergeCommand::Cancel, "Cancel", false);
        let apply = button(self, cx, MergeCommand::Apply, "Apply", true);
        div()
            .debug_selector(|| "merge-footer".to_owned())
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .px(px(12.0))
            .py(px(8.0))
            .border_t_1()
            .border_color(theme::color(cx, "border"))
            .child(div().flex().gap(px(8.0)).child(accept_yours).child(accept_theirs))
            .child(div().flex().gap(px(8.0)).child(cancel).child(apply))
    }

    /// A message in place of the panes, with one button.
    fn render_message(
        &self,
        text: String,
        button: (&str, &'static str, MergeViewEvent),
        cx: &mut Context<Self>,
    ) -> Div {
        let (label, selector, event) = button;
        let label = tx(cx, label);
        div()
            .debug_selector(|| "merge-message".to_owned())
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.0))
            .p(px(24.0))
            .text_color(theme::color(cx, "fg-subtle"))
            .child(SharedString::from(text))
            .child(
                div()
                    .id(selector)
                    .debug_selector(move || selector.to_owned())
                    .h(px(BUTTON_HEIGHT))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .rounded(theme::length(cx, "radius-small"))
                    .border_1()
                    .border_color(theme::color(cx, "border-button"))
                    .bg(theme::color(cx, "bg-button"))
                    .text_color(theme::color(cx, "fg-strong"))
                    .cursor_pointer()
                    .child(SharedString::from(label))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(event.clone()))),
            )
    }
}

impl Focusable for MergeView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for MergeView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let doc = self.loaded.as_ref()?.model.doc();
        let clamped = range.start.min(doc.len())..range.end.min(doc.len());
        if clamped != range {
            *adjusted = Some(clamped.clone());
        }
        Some(doc.slice_string(clamped.start, clamped.end))
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        self.loaded.as_ref()?;
        let caret = self.carets[slot(MergePane::Result)];
        Some(UTF16Selection { range: caret.range(), reversed: caret.head < caret.anchor })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.clone()
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Committed text replaces the composition, else the given range, else the selection.
        let range = range.or_else(|| self.marked.take());
        self.marked = None;
        self.edit_result(range, text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active != MergePane::Result || self.loaded.is_none() {
            return;
        }
        let range =
            range.or_else(|| self.marked.clone()).unwrap_or_else(|| self.carets[slot(MergePane::Result)].range());
        let start = range.start;
        self.edit_result(Some(range), new_text, cx);
        let head = self.carets[slot(MergePane::Result)].head;
        self.marked = (head > start).then_some(start..head);
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let doc = self.loaded.as_ref()?.model.doc();
        let state = &self.panes[slot(MergePane::Result)];
        let painted = state.painted.as_ref()?;
        let line = doc.line_at(range.start.min(doc.len()));
        let (text, shaped) = painted.lines.get((line.number - 1).checked_sub(painted.first_line)?)?;
        let x = |pos: usize| {
            let col = (pos.clamp(line.from, line.to) - line.from) as u32;
            painted.bounds.left() + painted.text_left + shaped.x_for_index(byte_of_col(text, col))
        };
        let top = painted.bounds.top() + painted.line_height * (line.number - 1) as f32 - state.scroll_top;
        let (left, right) = (x(range.start), x(range.end));
        Some(Bounds::new(point(left, top), size((right - left).max(px(1.0)), painted.line_height)))
    }

    fn character_index_for_point(&mut self, at: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        self.offset_at(MergePane::Result, at)
    }
}

impl Render for MergeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .debug_selector(|| "merge-view".to_owned())
            .size_full()
            .flex()
            .flex_col()
            .key_context(TEXT_FIELD_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .bg(theme::color(cx, "bg-app"))
            .text_size(theme::length(cx, "fs-ui"));
        if self.resolved {
            let name = base_name(&self.path).to_owned();
            let text = cx
                .default_global::<Localization>()
                .0
                .tx_built(&["", " has been resolved and staged."], &[Param::from(name)]);
            let open = MergeViewEvent::OpenFile { path: self.path.clone() };
            return root.child(self.render_message(text, ("Open File", "merge-open-file", open), cx));
        }
        if let Some(error) = self.error.clone() {
            let text = tx(cx, &error);
            return root.child(self.render_message(text, ("Retry", "merge-retry", MergeViewEvent::Reload), cx));
        }
        let toolbar = self.render_toolbar(cx);
        let Some(loaded) = self.loaded.as_ref() else {
            return root.child(toolbar).child(div().debug_selector(|| "merge-loading".to_owned()).flex_1());
        };
        let labels = self.render_labels(loaded, cx);
        let body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .font_family(code_font(cx).family)
            .text_size(theme::length(cx, "fs-code"))
            .child(self.render_pane(MergePane::Side(MergeSide::Left), cx))
            .child(self.render_divider(MergeSide::Left, cx))
            .child(self.render_pane(MergePane::Result, cx))
            .child(self.render_divider(MergeSide::Right, cx))
            .child(self.render_pane(MergePane::Side(MergeSide::Right), cx));
        let footer = self.render_footer(cx);
        root.child(toolbar).child(labels).child(body).child(footer)
    }
}

/// What a line paints, read from the view before drawing.
struct RowPaint {
    line: usize,
    text: String,
    /// Token of the line's tint.
    tint: Option<&'static str>,
    /// A settled change: the text is dimmed.
    dim: bool,
    /// Token of the thin mark above the line, where a change without lines goes.
    point: Option<&'static str>,
    /// Selected columns; an end past the text runs over the line break.
    selected: Option<(u32, u32)>,
    caret: Option<u32>,
}

struct PaneSnapshot {
    scroll_top: Pixels,
    first_line: usize,
    rows: Vec<RowPaint>,
    /// The overview lane's marks: top and bottom as fractions of the pane, and the colour's token.
    lane: Vec<(f32, f32, &'static str)>,
}

/// Paints the lines of a pane in view.
struct PaneElement {
    view: Entity<MergeView>,
    pane: MergePane,
}

impl IntoElement for PaneElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

struct PanePaint {
    line_height: Pixels,
    text_left: Pixels,
    first_line: usize,
    /// Backgrounds, below the text.
    fills: Vec<(Bounds<Pixels>, Hsla)>,
    lines: Vec<(String, ShapedLine, Point<Pixels>)>,
    numbers: Vec<(Point<Pixels>, ShapedLine)>,
    /// Marks over the text: the caret, the points, the overview lane.
    overs: Vec<(Bounds<Pixels>, Hsla)>,
}

impl Element for PaneElement {
    type RequestLayoutState = ();
    type PrepaintState = PanePaint;

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
    ) -> PanePaint {
        let font_size = theme::length(cx, "fs-code");
        let line_height = line_height(cx);
        let code_font = code_font(cx);
        let (plain, dimmed) = (theme::color(cx, "fg-editor"), theme::color(cx, "fg-subtle"));
        let number_color = theme::color(cx, "fg-gutter");
        let selection_color = theme::color(cx, "bg-editor-selection");
        let gutter = theme::length(cx, "w-gutter");
        let text_left = gutter + px(6.0) + if self.pane == MergePane::Result { px(WAND_WIDTH) } else { px(0.0) };
        let view = self.view.read(cx);
        let focused = view.focus.is_focused(window);
        let mut paint = PanePaint {
            line_height,
            text_left,
            first_line: 0,
            fills: Vec::new(),
            lines: Vec::new(),
            numbers: Vec::new(),
            overs: Vec::new(),
        };
        let Some(snapshot) = view.snapshot(self.pane, bounds.size.height, line_height, focused) else { return paint };
        paint.first_line = snapshot.first_line;
        let run = |len: usize, color: Hsla| TextRun {
            len,
            font: code_font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        for row in snapshot.rows {
            let top = bounds.top() + line_height * row.line as f32 - snapshot.scroll_top;
            if let Some(token) = row.tint {
                let color = theme::color(cx, token);
                let line = Bounds::new(point(bounds.left(), top), size(bounds.size.width, line_height));
                paint.fills.push((line, color.opacity(LINE_TINT)));
            }
            let color = if row.dim { dimmed } else { plain };
            let runs = [run(row.text.len(), color)];
            let shaped = window.text_system().shape_line(row.text.clone().into(), font_size, &runs, None);
            let origin = point(bounds.left() + text_left, top);
            let length = utf16_len(&row.text) as u32;
            let x = |col: u32| shaped.x_for_index(byte_of_col(&row.text, col.min(length)));
            if let Some((from, to)) = row.selected {
                let left = x(from);
                let right = if to > length { x(length) + px(6.0) } else { x(to) };
                let area = Bounds::new(point(origin.x + left, top), size((right - left).max(px(0.0)), line_height));
                paint.fills.push((area, selection_color));
            }
            if let Some(col) = row.caret {
                let caret = Bounds::new(point(origin.x + x(col), top), size(px(1.5), line_height));
                paint.overs.push((caret, theme::color(cx, "caret")));
            }
            if let Some(token) = row.point {
                let origin = point(bounds.left() + gutter, top - px(1.0));
                let mark = Bounds::new(origin, size(bounds.size.width - gutter, px(2.0)));
                paint.overs.push((mark, theme::color(cx, token)));
            }
            let label = (row.line + 1).to_string();
            let runs = [run(label.len(), number_color)];
            let shaped_number = window.text_system().shape_line(label.clone().into(), font_size, &runs, None);
            let number_x = (bounds.left() + gutter - px(8.0) - shaped_number.width).max(bounds.left());
            paint.numbers.push((point(number_x, top), shaped_number));
            paint.lines.push((row.text, shaped, origin));
        }
        let lane_left = bounds.right() - px(LANE_WIDTH);
        for (top, bottom, token) in snapshot.lane {
            let y = bounds.top() + bounds.size.height * top;
            let height = (bounds.size.height * (bottom - top)).max(px(2.0));
            paint.overs.push((Bounds::new(point(lane_left, y), size(px(LANE_WIDTH), height)), theme::color(cx, token)));
        }
        paint
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepared: &mut PanePaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (view, pane) = (self.view.clone(), self.pane);
        window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && bounds.contains(&event.position) {
                let delta = event.delta.pixel_delta(line_height(cx)).y;
                view.update(cx, |view, cx| view.scroll_by(pane, -delta, cx));
                window.refresh();
            }
        });
        if pane == MergePane::Result {
            let focus = self.view.read(cx).focus.clone();
            window.handle_input(&focus, ElementInputHandler::new(bounds, self.view.clone()), cx);
        }
        let line_height = prepared.line_height;
        window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, theme::color(cx, "bg-editor")));
            for (area, color) in &prepared.fills {
                window.paint_quad(fill(*area, *color));
            }
            for (_, shaped, origin) in &prepared.lines {
                let _ = shaped.paint(*origin, line_height, TextAlign::Left, None, window, cx);
            }
            for (origin, number) in &prepared.numbers {
                let _ = number.paint(*origin, line_height, TextAlign::Left, None, window, cx);
            }
            for (area, color) in &prepared.overs {
                window.paint_quad(fill(*area, *color));
            }
        });
        let painted = Painted {
            bounds,
            text_left: prepared.text_left,
            line_height,
            first_line: prepared.first_line,
            lines: std::mem::take(&mut prepared.lines).into_iter().map(|(text, shaped, _)| (text, shaped)).collect(),
        };
        self.view.update(cx, |view, _| view.painted(pane, painted));
    }
}

/// Paints the bands of a divider: each open change of a side joined to its place in the result.
struct DividerElement {
    view: Entity<MergeView>,
    side: MergeSide,
}

impl IntoElement for DividerElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

/// Where `y` of a band edge from `from` (left) to `to` (right) is at `t` along it, as the cubic
/// curve with its control points at 30% and 70% of the width draws it.
fn curve(width: f32, from: f64, to: f64, t: f64) -> (f32, f32) {
    let u = 1.0 - t;
    let w = f64::from(width);
    let x = 3.0 * u * u * t * 0.3 * w + 3.0 * u * t * t * 0.7 * w + t * t * t * w;
    let y = u * u * u * from + 3.0 * u * u * t * from + 3.0 * u * t * t * to + t * t * t * to;
    (x as f32, y as f32)
}

/// The outline of a band between `[a0, a1]` on the left and `[b0, b1]` on the right; an empty side
/// is a 2px sliver on its boundary.
fn band_outline([a0, a1, b0, b1]: [f64; 4], width: f32, origin: Point<Pixels>) -> Vec<Point<Pixels>> {
    let sliver = |y0: f64, y1: f64| if y1 - y0 < 2.0 { (y0 - 1.0, y0 + 1.0) } else { (y0, y1) };
    let ((a0, a1), (b0, b1)) = (sliver(a0, a1), sliver(b0, b1));
    let at = |(x, y): (f32, f32)| point(origin.x + px(x), origin.y + px(y));
    let steps = (0..=BAND_STEPS).map(|step| step as f64 / BAND_STEPS as f64);
    let top = steps.clone().map(|t| at(curve(width, a0, b0, t)));
    let bottom = steps.rev().map(|t| at(curve(width, a1, b1, t)));
    top.chain(bottom).collect()
}

/// The outline of a band and its colour.
type BandShape = (Vec<Point<Pixels>>, Hsla);

impl Element for DividerElement {
    type RequestLayoutState = ();
    type PrepaintState = Vec<BandShape>;

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
        _: &mut Window,
        cx: &mut App,
    ) -> Vec<BandShape> {
        let line_height = f64::from(line_height(cx));
        let width = f32::from(bounds.size.width);
        self.view
            .read(cx)
            .bands(self.side, line_height)
            .into_iter()
            .map(|(edges, token)| {
                (band_outline(edges, width, bounds.origin), theme::color(cx, token).opacity(BAND_TINT))
            })
            .collect()
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        bands: &mut Vec<BandShape>,
        window: &mut Window,
        _: &mut App,
    ) {
        window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
            for (outline, color) in bands.iter() {
                let Some((first, rest)) = outline.split_first() else { continue };
                let mut path = PathBuilder::fill();
                path.move_to(*first);
                for point in rest {
                    path.line_to(*point);
                }
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, *color);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<MergeViewEvent>>>;

    const LEFT: MergePane = MergePane::Side(MergeSide::Left);
    const RIGHT: MergePane = MergePane::Side(MergeSide::Right);

    fn lines(lines: &[&str]) -> String {
        lines.iter().map(|line| format!("{line}\n")).collect()
    }

    fn base() -> String {
        lines(&["a", "b", "c", "d", "e", "f", "g"])
    }

    /// Changes: 0 a conflict (b / L / R), 1 on the right only (d), 2 the same on both sides (g).
    fn versions() -> MergeVersions {
        MergeVersions {
            base: Some(base()),
            ours: Some(lines(&["a", "L", "c", "d", "e", "f", "G"])),
            theirs: Some(lines(&["a", "R", "c", "D", "e", "f", "G"])),
            sides: MergeSides {
                operation: Some("merge".to_owned()),
                branch: Some("main".to_owned()),
                commit: None,
                other_branch: Some("feature".to_owned()),
            },
        }
    }

    fn open<'a>(
        cx: &'a mut TestAppContext,
        auto_apply: bool,
        start: MergeVersions,
    ) -> (Entity<MergeView>, Events, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(move |_, cx| MergeView::new("/p/src/a.txt", auto_apply, cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &MergeViewEvent, _| seen.borrow_mut().push(event.clone())).detach();
            window.activate_window();
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        view.update(cx, |view, cx| view.set_versions(start, cx));
        cx.run_until_parked();
        (view, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str, modifiers: Modifiers) {
        let centre = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
        cx.simulate_click(centre, modifiers);
        cx.run_until_parked();
    }

    /// Clicks line `line` of `pane`, at its start.
    fn click_line(cx: &mut VisualTestContext, pane: &'static str, line: usize) {
        let bounds = cx.debug_bounds(pane).unwrap_or_else(|| panic!("{pane} is drawn"));
        let line_height = cx.update(|_, cx| line_height(cx));
        let at = point(bounds.left() + px(2.0), bounds.top() + line_height * (line as f32 + 0.5));
        cx.simulate_click(at, Modifiers::default());
        cx.run_until_parked();
    }

    fn text(view: &Entity<MergeView>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, _| view.text()).expect("the versions are shown")
    }

    /// The events other than [`MergeViewEvent::CommandsChanged`].
    fn asked(events: &Events) -> Vec<MergeViewEvent> {
        events.borrow().iter().filter(|event| **event != MergeViewEvent::CommandsChanged).cloned().collect()
    }

    fn enabled(view: &Entity<MergeView>, cx: &mut VisualTestContext) -> Vec<(&'static str, bool)> {
        view.read_with(cx, |view, _| view.toolbar().iter().map(|b| (b.button.name(), b.enabled)).collect())
    }

    // Not from a spec: the result starts as the base with every change pending, tinted in each pane
    // where it is still to be decided, and the toolbar counts what is left.
    #[gpui_kit::test]
    async fn starts_from_the_base_and_counts_the_changes_left(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, false, versions());
        assert_eq!(text(&view, cx), base());
        let status = view.read_with(cx, |view, _| status_text(&view.summary(), &mut I18n::default()));
        assert_eq!(status, "3 changes left, 1 conflict");
        let tints = view.read_with(cx, |view, _| view.line_tints(MergePane::Result));
        let expected = [None, Some("vcs-conflict"), None, Some("vcs-modified"), None, None, Some("vcs-modified"), None];
        assert_eq!(tints, expected);
        // The left side did not change the fourth line: it has nothing to decide there.
        let left = view.read_with(cx, |view, _| view.line_tints(LEFT));
        assert_eq!(left[1..4], [Some("vcs-conflict"), None, None]);
        let titles = view.read_with(cx, |view, _| view.titles().cloned()).expect("titles");
        assert_eq!((titles.yours.as_str(), titles.theirs.as_str()), ("Changes from main", "Changes from feature"));
        assert_eq!(
            enabled(&view, cx),
            [
                ("previous", true),
                ("next", true),
                ("applyLeft", true),
                ("applyAll", true),
                ("applyRight", true),
                ("resolveSimple", true),
                ("revert", false),
                ("compare", true),
                ("reload", true),
                ("settings", true)
            ]
        );
        assert!(cx.debug_bounds("merge-accept-left-0").is_some() && cx.debug_bounds("merge-accept-right-1").is_some());
        assert!(cx.debug_bounds("merge-accept-left-1").is_none(), "the left side has no second change");
    }

    // Not from a spec: the arrows of a conflict take one side, then the other below it; Ignore keeps
    // the result's text; each step undoes with the state of the change.
    #[gpui_kit::test]
    async fn accepts_both_sides_of_a_conflict_from_the_arrows_and_undoes_them(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, false, versions());
        click(cx, "merge-accept-left-0", Modifiers::default());
        assert_eq!(text(&view, cx), lines(&["a", "L", "c", "d", "e", "f", "g"]));
        assert!(cx.debug_bounds("merge-accept-left-0").is_none(), "the left side of the conflict is decided");
        click(cx, "merge-accept-right-0", Modifiers::default());
        assert_eq!(text(&view, cx), lines(&["a", "L", "R", "c", "d", "e", "f", "g"]));
        click(cx, "merge-ignore-right-1", Modifiers::default());
        assert_eq!(text(&view, cx), lines(&["a", "L", "R", "c", "d", "e", "f", "g"]));
        assert!(view.read_with(cx, |view, _| view.changes()[1].is_resolved()));
        assert!(view.read_with(cx, |view, _| view.is_modified()));

        view.update(cx, |view, cx| view.undo(cx));
        assert!(!view.read_with(cx, |view, _| view.changes()[1].is_resolved()));
        view.update(cx, |view, cx| {
            view.undo(cx);
            view.undo(cx);
        });
        assert_eq!(text(&view, cx), base());
        assert!(!view.read_with(cx, |view, _| view.is_modified()));
        view.update(cx, |view, cx| view.redo(cx));
        assert_eq!(text(&view, cx), lines(&["a", "L", "c", "d", "e", "f", "g"]));

        // With the modifier key held, the arrow resolves the conflict using that side alone.
        let (view, _, cx) = open(cx, false, versions());
        let primary = cx.update(|_, cx| Modifiers::secondary_key(cx));
        click(cx, "merge-accept-right-0", primary);
        assert_eq!(text(&view, cx), lines(&["a", "R", "c", "d", "e", "f", "g"]));
        assert!(view.read_with(cx, |view, _| view.changes()[0].is_resolved()));
    }

    // Not from a spec: the toolbar applies the non-conflicting changes and moves to what is left;
    // Next / Previous Change go round the open changes.
    #[gpui_kit::test]
    async fn applies_the_non_conflicting_changes_and_moves_between_the_changes_left(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, false, versions());
        click(cx, "merge-tool-applyAll", Modifiers::default());
        assert_eq!(text(&view, cx), lines(&["a", "b", "c", "D", "e", "f", "G"]));
        let status = view.read_with(cx, |view, _| status_text(&view.summary(), &mut I18n::default()));
        assert_eq!(status, "1 change left, 1 conflict");
        // The caret is on the conflict left.
        assert_eq!(view.read_with(cx, |view, _| view.selection(MergePane::Result)), 2..2);
        assert!(!view.read_with(cx, |view, _| view.enabled(MergeCommand::ApplyAll)));
        assert!(view.read_with(cx, |view, _| view.enabled(MergeCommand::Revert)));

        let (view, _, cx) = open(cx, false, versions());
        cx.simulate_keystrokes("f7");
        assert_eq!(view.read_with(cx, |view, _| view.selection(MergePane::Result)), 2..2);
        cx.simulate_keystrokes("f7");
        assert_eq!(view.read_with(cx, |view, _| view.selection(MergePane::Result)), 6..6);
        cx.simulate_keystrokes("shift-f7 shift-f7");
        // Round from the first change to the last.
        assert_eq!(view.read_with(cx, |view, _| view.selection(MergePane::Result)), 12..12);
    }

    // Not from a spec: the result is typed in; Backspace, Enter and undo work on it, and the changes
    // follow the text.
    #[gpui_kit::test]
    async fn types_in_the_result(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, false, versions());
        click_line(cx, "merge-pane-result", 0);
        assert_eq!(view.read_with(cx, |view, _| view.active_pane()), MergePane::Result);
        cx.simulate_input("top");
        cx.simulate_keystrokes("enter");
        assert_eq!(text(&view, cx), format!("top\n{}", base()));
        assert_eq!(view.read_with(cx, |view, _| view.changes()[0].from), 6);
        cx.simulate_keystrokes("backspace");
        assert_eq!(text(&view, cx), format!("top{}", base()));
        view.update(cx, |view, cx| view.undo(cx));
        assert_eq!(text(&view, cx), format!("top\n{}", base()));
        // A side pane is read-only.
        click_line(cx, "merge-pane-left", 1);
        cx.simulate_input("x");
        assert_eq!(text(&view, cx), format!("top\n{}", base()));
    }

    // Not from a spec: Accept and Ignore act on the changes in the selection of the pane the caret is
    // in, and the side's context menu offers them.
    #[gpui_kit::test]
    async fn accepts_the_changes_in_the_selection_of_a_side(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, false, versions());
        click_line(cx, "merge-pane-left", 0);
        assert!(!view.read_with(cx, |view, _| view.enabled(MergeCommand::Accept)), "line 1 is unchanged");
        click_line(cx, "merge-pane-left", 1);
        assert!(view.read_with(cx, |view, _| view.enabled(MergeCommand::Accept)));
        view.update(cx, |view, cx| view.run(MergeCommand::Accept, cx));
        assert_eq!(text(&view, cx), lines(&["a", "L", "c", "d", "e", "f", "g"]));

        let bounds = cx.debug_bounds("merge-pane-right").expect("the right pane is drawn");
        cx.simulate_event(MouseDownEvent {
            position: bounds.center(),
            button: MouseButton::Right,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        });
        cx.run_until_parked();
        let menu = asked(&events).into_iter().find_map(|event| match event {
            MergeViewEvent::ShowMenu { items, .. } => Some(items),
            _ => None,
        });
        assert_eq!(menu, Some(side_menu(MergeSide::Right)));
        assert_eq!(view.read_with(cx, |view, _| view.active_pane()), RIGHT);
    }

    // Not from a spec: Apply asks before saving a result with changes left, and saves it with the
    // file's own line breaks; once everything is decided it saves at once.
    #[gpui_kit::test]
    async fn asks_before_applying_a_result_with_unresolved_changes(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, false, versions());
        click(cx, "merge-footer-apply", Modifiers::default());
        assert_eq!(asked(&events), [MergeViewEvent::Confirm(MergeQuestion::Unresolved(3))]);
        let request = MergeQuestion::Unresolved(3).request(&mut I18n::default());
        assert_eq!(request.ok_label, "Mark Resolved");
        assert!(request.message.starts_with("3 changes are unresolved"));
        view.update(cx, |view, cx| view.answer(MergeQuestion::Unresolved(3), true, cx));
        let saved = MergeViewEvent::Resolve { path: "/p/src/a.txt".to_owned(), text: Some(base()) };
        assert_eq!(asked(&events).last(), Some(&saved));

        let crlf = |text: String| text.replace('\n', "\r\n");
        let start = MergeVersions {
            base: versions().base.map(crlf),
            ours: versions().ours.map(crlf),
            theirs: versions().theirs.map(crlf),
            ..versions()
        };
        let (view, events, cx) = open(cx, false, start);
        click(cx, "merge-tool-applyAll", Modifiers::default());
        click(cx, "merge-accept-left-0", Modifiers::default());
        click(cx, "merge-ignore-right-0", Modifiers::default());
        assert_eq!(view.read_with(cx, |view, _| view.summary().pending), 0);
        click(cx, "merge-footer-apply", Modifiers::default());
        let text = crlf(lines(&["a", "L", "c", "D", "e", "f", "G"]));
        assert_eq!(asked(&events), [MergeViewEvent::Resolve { path: "/p/src/a.txt".to_owned(), text: Some(text) }]);
    }

    // Not from a spec: Accept Yours / Accept Theirs take that version as it is (none when it deleted
    // the file), asking first once the result was changed; Cancel asks only then too.
    #[gpui_kit::test]
    async fn takes_a_whole_side_and_asks_before_discarding_changes(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, false, versions());
        click(cx, "merge-footer-acceptTheirs", Modifiers::default());
        let theirs = MergeViewEvent::Resolve { path: "/p/src/a.txt".to_owned(), text: versions().theirs };
        assert_eq!(asked(&events), [theirs]);
        click(cx, "merge-footer-cancel", Modifiers::default());
        assert_eq!(asked(&events).last(), Some(&MergeViewEvent::Close));

        click(cx, "merge-accept-left-0", Modifiers::default());
        events.borrow_mut().clear();
        click(cx, "merge-footer-acceptYours", Modifiers::default());
        click(cx, "merge-footer-cancel", Modifiers::default());
        let questions = [MergeQuestion::Discard(MergeSide::Left), MergeQuestion::Cancel];
        assert_eq!(asked(&events), questions.map(MergeViewEvent::Confirm));
        view.update(cx, |view, cx| view.answer(MergeQuestion::Discard(MergeSide::Left), true, cx));
        let yours = MergeViewEvent::Resolve { path: "/p/src/a.txt".to_owned(), text: versions().ours };
        assert_eq!(asked(&events).last(), Some(&yours));
        view.update(cx, |view, cx| view.answer(MergeQuestion::Cancel, false, cx));
        assert_eq!(asked(&events).len(), 3, "a question answered No does nothing");

        let (_, events, cx) = open(cx, false, MergeVersions { ours: None, ..versions() });
        click(cx, "merge-footer-acceptYours", Modifiers::default());
        assert_eq!(asked(&events), [MergeViewEvent::Resolve { path: "/p/src/a.txt".to_owned(), text: None }]);
    }

    // Not from a spec: with Automatically Apply Non-Conflicting Changes on, the merge starts with them
    // applied, which is neither a change to ask about nor a step to undo.
    #[gpui_kit::test]
    async fn starts_with_the_non_conflicting_changes_applied_when_asked_to(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, true, versions());
        let applied = lines(&["a", "b", "c", "D", "e", "f", "G"]);
        assert_eq!(text(&view, cx), applied);
        assert!(!view.read_with(cx, |view, _| view.is_modified()));
        view.update(cx, |view, cx| view.undo(cx));
        assert_eq!(text(&view, cx), applied);
        click(cx, "merge-footer-cancel", Modifiers::default());
        assert_eq!(asked(&events), [MergeViewEvent::Close]);
        view.update(cx, |view, cx| view.run(MergeCommand::AutoApply, cx));
        assert!(!view.read_with(cx, |view, _| view.auto_apply()));
        assert_eq!(asked(&events).last(), Some(&MergeViewEvent::AutoApplyChanged(false)));
    }

    // Not from a spec: the wand resolves a conflict whose sides changed different words.
    #[gpui_kit::test]
    async fn resolves_a_conflict_of_different_words_with_the_wand(cx: &mut TestAppContext) {
        let start = MergeVersions {
            base: Some("let a = 1 + b;\n".to_owned()),
            ours: Some("let x = 1 + b;\n".to_owned()),
            theirs: Some("let a = 1 + 2;\n".to_owned()),
            sides: MergeSides::default(),
        };
        let (view, _, cx) = open(cx, false, start);
        click(cx, "merge-wand-0", Modifiers::default());
        assert_eq!(text(&view, cx), "let x = 1 + 2;\n");
        assert!(cx.debug_bounds("merge-wand-0").is_none());
    }

    // Not from a spec: a comparison shows two of the versions with their titles.
    #[gpui_kit::test]
    async fn compares_two_versions(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, false, versions());
        view.update(cx, |view, cx| view.run(MergeCommand::CompareYoursResult, cx));
        let expected = MergeViewEvent::Compare {
            left: CompareSide { label: "Changes from main".to_owned(), text: versions().ours.unwrap_or_default() },
            right: CompareSide { label: "Result".to_owned(), text: base() },
        };
        assert_eq!(asked(&events), [expected]);
        click(cx, "merge-tool-compare", Modifiers::default());
        let menu = asked(&events).into_iter().find_map(|event| match event {
            MergeViewEvent::ShowMenu { items, position } => Some((items, position.is_some())),
            _ => None,
        });
        assert_eq!(menu, Some((compare_menu(), true)));
    }

    // Not from a spec: binary versions are not merged; Retry asks for the versions again, and a
    // resolved file offers to open it.
    #[gpui_kit::test]
    async fn refuses_binary_versions_and_offers_to_retry(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, false, MergeVersions { theirs: Some("a\0b".to_owned()), ..versions() });
        assert_eq!(view.read_with(cx, |view, _| view.error().map(str::to_owned)), Some(BINARY_TEXT.to_owned()));
        click(cx, "merge-retry", Modifiers::default());
        assert_eq!(asked(&events), [MergeViewEvent::Reload]);
        view.update(cx, |view, cx| {
            view.set_versions(versions(), cx);
            view.set_resolved(cx);
        });
        cx.run_until_parked();
        assert!(!view.read_with(cx, |view, _| view.enabled(MergeCommand::Apply)));
        click(cx, "merge-open-file", Modifiers::default());
        assert_eq!(asked(&events).last(), Some(&MergeViewEvent::OpenFile { path: "/p/src/a.txt".to_owned() }));
    }

    // Not from a spec: every menu row is a merge command, and the caret follows the edits a decision
    // makes before it.
    #[test]
    fn menus_name_merge_commands_and_carets_follow_edits() {
        let menus = [side_menu(MergeSide::Left), side_menu(MergeSide::Right), result_menu(), compare_menu()];
        for item in menus.iter().flatten().chain(&settings_menu()) {
            if let MenuItem::Action(action) = item {
                assert!(MergeCommand::from_id(action.action).is_some(), "{}", action.action);
            }
        }
        let edits = [MergeEdit { from: 2, to: 4, insert: "LLLL\n".to_owned() }];
        assert_eq!(map_offset(&edits, 1), 1);
        assert_eq!(map_offset(&edits, 2), 2);
        assert_eq!(map_offset(&edits, 3), 7);
        assert_eq!(map_offset(&edits, 6), 9);
        let doc = Doc::new("a😀b");
        assert_eq!(char_boundary(&doc, 1, true), 3);
        assert_eq!(char_boundary(&doc, 3, false), 1);
    }
}
