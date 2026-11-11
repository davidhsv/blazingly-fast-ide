//! Tool window layout, IDEA semantics: each side (left / right / bottom) has a primary and a split
//! area, each showing at most one window; showing another window in the same area replaces it.
//!
//! [`ToolWindowLayout`] holds the registered windows and one project's layout (visible windows,
//! sizes, split ratios, placements, stripe buttons). It does no I/O: a transition returns the
//! [`Effects`] its caller carries out, and a project's layout is stored as the JSON of
//! [`ToolWindowLayout::serialized`] under [`storage_key`]. [`NamedLayouts`] are the snapshots of
//! Window › Tool Window Layouts, shared by every project, and [`ContentTabs`] the content tab
//! strips of the tool window headers.

use serde::{Serialize, Serializer};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// Storage key prefix of a project's layout; the project root follows.
pub const LAYOUT_STORAGE_PREFIX: &str = "openidea.layout.v1:";
/// Storage key of the named layouts.
pub const NAMED_LAYOUTS_STORAGE_KEY: &str = "openidea.toolWindowLayouts.v1";
/// Stored layout format: 1 = one area per side (no `v` field), 2 = primary + split areas.
const LAYOUT_VERSION: u32 = 2;

/// Share of a side taken by its primary area when both areas show (IDEA's sideWeight).
pub const DEFAULT_SPLIT: f64 = 0.5;
pub const MIN_SPLIT: f64 = 0.1;
pub const MAX_SPLIT: f64 = 0.9;
pub const DEFAULT_SIZES: PerAnchor<u32> = PerAnchor { left: 240, right: 300, bottom: 280 };
const DEFAULT_SPLITS: PerAnchor<f64> = PerAnchor { left: DEFAULT_SPLIT, right: DEFAULT_SPLIT, bottom: DEFAULT_SPLIT };
/// Smallest size a side can be given.
const MIN_SIZE: f64 = 80.0;
/// Largest stored size taken as valid.
const MAX_STORED_SIZE: f64 = 4000.0;
/// Pixels per Resize Tool Window step.
pub const RESIZE_STEP: i32 = 24;
/// What resizing by keyboard leaves of the viewport to the rest of the workbench.
const RESIZE_MARGIN: f64 = 160.0;
/// Stripe order of a window that declares none.
const DEFAULT_ORDER: f64 = 100.0;
/// The window a fresh project shows, and the one Jump to Last Tool Window falls back to.
const DEFAULT_WINDOW: &str = "project";

/// The key a project's layout is stored under (`scope`: the project root; "" without a project).
pub fn storage_key(scope: &str) -> String {
    format!("{LAYOUT_STORAGE_PREFIX}{scope}")
}

/// A side of the workbench tool windows dock on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Anchor {
    Left,
    Bottom,
    Right,
}

pub const ANCHORS: [Anchor; 3] = [Anchor::Left, Anchor::Bottom, Anchor::Right];

impl Anchor {
    /// The name the anchor is stored by.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Bottom => "bottom",
            Self::Right => "right",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        ANCHORS.into_iter().find(|anchor| anchor.as_str() == name)
    }
}

/// Where a window docks: a side's primary area, or its split area (IDEA's `isSplit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Left,
    LeftSplit,
    Bottom,
    BottomSplit,
    Right,
    RightSplit,
}

pub const SLOTS: [Slot; 6] =
    [Slot::Left, Slot::LeftSplit, Slot::Bottom, Slot::BottomSplit, Slot::Right, Slot::RightSplit];

impl Slot {
    pub fn new(anchor: Anchor, split: bool) -> Self {
        match (anchor, split) {
            (Anchor::Left, false) => Self::Left,
            (Anchor::Left, true) => Self::LeftSplit,
            (Anchor::Bottom, false) => Self::Bottom,
            (Anchor::Bottom, true) => Self::BottomSplit,
            (Anchor::Right, false) => Self::Right,
            (Anchor::Right, true) => Self::RightSplit,
        }
    }

    pub fn anchor(self) -> Anchor {
        match self {
            Self::Left | Self::LeftSplit => Anchor::Left,
            Self::Bottom | Self::BottomSplit => Anchor::Bottom,
            Self::Right | Self::RightSplit => Anchor::Right,
        }
    }

    pub fn is_split(self) -> bool {
        matches!(self, Self::LeftSplit | Self::BottomSplit | Self::RightSplit)
    }

    /// The other area of the same side.
    pub fn partner(self) -> Self {
        Self::new(self.anchor(), !self.is_split())
    }

    /// The name the area is stored by.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::LeftSplit => "left-split",
            Self::Bottom => "bottom",
            Self::BottomSplit => "bottom-split",
            Self::Right => "right",
            Self::RightSplit => "right-split",
        }
    }
}

/// The window shown in each area, if any.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SlotRecord {
    left: Option<String>,
    #[serde(rename = "left-split")]
    left_split: Option<String>,
    bottom: Option<String>,
    #[serde(rename = "bottom-split")]
    bottom_split: Option<String>,
    right: Option<String>,
    #[serde(rename = "right-split")]
    right_split: Option<String>,
}

impl SlotRecord {
    fn default_visible() -> Self {
        Self { left: Some(DEFAULT_WINDOW.to_owned()), ..Self::default() }
    }

    pub fn get(&self, slot: Slot) -> Option<&str> {
        match slot {
            Slot::Left => self.left.as_deref(),
            Slot::LeftSplit => self.left_split.as_deref(),
            Slot::Bottom => self.bottom.as_deref(),
            Slot::BottomSplit => self.bottom_split.as_deref(),
            Slot::Right => self.right.as_deref(),
            Slot::RightSplit => self.right_split.as_deref(),
        }
    }

    fn slot_mut(&mut self, slot: Slot) -> &mut Option<String> {
        match slot {
            Slot::Left => &mut self.left,
            Slot::LeftSplit => &mut self.left_split,
            Slot::Bottom => &mut self.bottom,
            Slot::BottomSplit => &mut self.bottom_split,
            Slot::Right => &mut self.right,
            Slot::RightSplit => &mut self.right_split,
        }
    }

    /// The recorded windows, in [`SLOTS`] order.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        SLOTS.into_iter().filter_map(move |slot| self.get(slot))
    }

    pub fn contains(&self, id: &str) -> bool {
        self.ids().any(|shown| shown == id)
    }

    pub fn is_empty(&self) -> bool {
        self.ids().next().is_none()
    }
}

/// One value per side.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PerAnchor<T> {
    pub left: T,
    pub right: T,
    pub bottom: T,
}

impl<T: Copy> PerAnchor<T> {
    pub fn get(&self, anchor: Anchor) -> T {
        match anchor {
            Anchor::Left => self.left,
            Anchor::Right => self.right,
            Anchor::Bottom => self.bottom,
        }
    }

    fn set(&mut self, anchor: Anchor, value: T) {
        match anchor {
            Anchor::Left => self.left = value,
            Anchor::Right => self.right = value,
            Anchor::Bottom => self.bottom = value,
        }
    }
}

/// Whether the stripe has a button for a window before the user decides (IDEA).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Stripe {
    #[default]
    Shown,
    /// Listed under the stripe's "…" until first opened (Find, Run…).
    OnUse,
}

/// What a feature declares about its tool window.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolWindow {
    pub id: String,
    pub title: String,
    /// The title comes from outside the IDE (a plugin) and is shown as written.
    pub verbatim: bool,
    /// Icon name.
    pub icon: String,
    pub anchor: Anchor,
    /// Order within its stripe group; lower first.
    pub order: Option<f64>,
    /// Docks in the side's split area (IDEA `secondary` / `isSplit`): Left Bottom, Bottom Right or
    /// Right Bottom, so it can show beside the primary window.
    pub secondary: bool,
    pub stripe: Stripe,
    /// Only shown when a project is open.
    pub requires_project: bool,
}

impl ToolWindow {
    /// A primary-area window with a stripe button, shown only when a project is open.
    pub fn new(id: &str, title: &str, icon: &str, anchor: Anchor) -> Self {
        Self {
            id: id.to_owned(),
            title: title.to_owned(),
            verbatim: false,
            icon: icon.to_owned(),
            anchor,
            order: None,
            secondary: false,
            stripe: Stripe::Shown,
            requires_project: true,
        }
    }

    /// The area the window docks in before the user moves it.
    pub fn default_slot(&self) -> Slot {
        Slot::new(self.anchor, self.secondary)
    }
}

/// User placement of a tool window (dragged on the stripes / "Move To"), overriding its declaration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Placement {
    pub anchor: Anchor,
    /// Split area; unset: the window's `secondary` when on its own side, else primary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "serialize_order")]
    pub order: Option<f64>,
}

/// Whole orders are written without a fraction, as every other writer of the stored layout does.
fn serialize_order<S: Serializer>(order: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error> {
    const EXACT_INTEGERS: f64 = 9_007_199_254_740_992.0;
    match *order {
        Some(value) if value.fract() == 0.0 && value.abs() < EXACT_INTEGERS => serializer.serialize_i64(value as i64),
        Some(value) => serializer.serialize_f64(value),
        None => serializer.serialize_none(),
    }
}

/// Area and stripe order of `window` once the user's placement (if any) is applied.
fn docking(window: &ToolWindow, placement: Option<&Placement>) -> (Slot, Option<f64>) {
    match placement {
        None => (window.default_slot(), window.order),
        Some(placement) => {
            let split = placement.split.unwrap_or(placement.anchor == window.anchor && window.secondary);
            (Slot::new(placement.anchor, split), placement.order.or(window.order))
        }
    }
}

/// Stripe order: by order, then by title.
fn by_order(a: &(&ToolWindow, Option<f64>), b: &(&ToolWindow, Option<f64>)) -> Ordering {
    let key = |order: Option<f64>| order.filter(|value| !value.is_nan()).unwrap_or(DEFAULT_ORDER);
    key(a.1).partial_cmp(&key(b.1)).unwrap_or(Ordering::Equal).then_with(|| compare_titles(&a.0.title, &b.0.title))
}

/// Alphabetical regardless of case; of two titles differing only by case, the lower-case one first.
fn compare_titles(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| b.cmp(a))
}

/// What Window › Tool Window Layouts saves and restores.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LayoutSnapshot {
    pub visible: SlotRecord,
    pub sizes: PerAnchor<u32>,
    /// Per side: the primary area's share when both areas show.
    pub splits: PerAnchor<f64>,
    pub placements: BTreeMap<String, Placement>,
    #[serde(rename = "stripeButtons")]
    pub stripe_buttons: BTreeMap<String, bool>,
}

impl LayoutSnapshot {
    /// A stored snapshot, whatever its shape: invalid parts take their defaults.
    pub fn from_value(raw: &Value) -> Self {
        Self::sanitize(Some(raw), f64::from(LAYOUT_VERSION))
    }

    fn sanitize(raw: Option<&Value>, version: f64) -> Self {
        let field = |name: &str| raw.and_then(|value| value.get(name));
        Self {
            visible: sanitize_visible(field("visible")),
            sizes: sanitize_sizes(field("sizes")),
            splits: sanitize_splits(field("splits")),
            placements: sanitize_placements(field("placements"), version),
            stripe_buttons: sanitize_stripe_buttons(field("stripeButtons")),
        }
    }
}

/// The layout of a fresh project (Restore Default Layout).
impl Default for LayoutSnapshot {
    fn default() -> Self {
        Self::sanitize(None, f64::from(LAYOUT_VERSION))
    }
}

/// A project's layout as stored.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SavedLayout {
    pub v: u32,
    #[serde(flatten)]
    pub layout: LayoutSnapshot,
}

fn sanitize_visible(raw: Option<&Value>) -> SlotRecord {
    let Some(raw) = raw.filter(|value| value.is_object() || value.is_array()) else {
        return SlotRecord::default_visible();
    };
    let mut out = SlotRecord::default();
    for slot in SLOTS {
        let id = raw.get(slot.as_str()).and_then(Value::as_str).filter(|id| !id.is_empty());
        *out.slot_mut(slot) = id.map(str::to_owned);
    }
    out
}

fn sanitize_sizes(raw: Option<&Value>) -> PerAnchor<u32> {
    let mut out = DEFAULT_SIZES;
    for anchor in ANCHORS {
        let size = raw.and_then(|sizes| sizes.get(anchor.as_str())).and_then(Value::as_f64);
        if let Some(size) = size.filter(|size| (MIN_SIZE..=MAX_STORED_SIZE).contains(size)) {
            out.set(anchor, size.round() as u32);
        }
    }
    out
}

fn sanitize_splits(raw: Option<&Value>) -> PerAnchor<f64> {
    let mut out = DEFAULT_SPLITS;
    for anchor in ANCHORS {
        let share = raw.and_then(|splits| splits.get(anchor.as_str())).and_then(Value::as_f64);
        if let Some(share) = share.filter(|share| (MIN_SPLIT..=MAX_SPLIT).contains(share)) {
            out.set(anchor, share);
        }
    }
    out
}

/// Stored placements. Format 1 had no split areas: `secondary` only picked the lower group of the
/// right stripe, which is now the Right Bottom area; on the left and bottom it meant nothing, so
/// the window's default applies.
fn sanitize_placements(raw: Option<&Value>, version: f64) -> BTreeMap<String, Placement> {
    let entries = raw.and_then(Value::as_object).into_iter().flatten();
    entries
        .filter_map(|(id, value)| {
            let anchor = value.get("anchor").and_then(Value::as_str).and_then(Anchor::parse)?;
            let split = if version >= f64::from(LAYOUT_VERSION) {
                value.get("split").and_then(Value::as_bool)
            } else {
                (anchor == Anchor::Right && value.get("secondary").is_some_and(truthy)).then_some(true)
            };
            let order = value.get("order").and_then(Value::as_f64);
            Some((id.clone(), Placement { anchor, split, order }))
        })
        .collect()
}

fn sanitize_stripe_buttons(raw: Option<&Value>) -> BTreeMap<String, bool> {
    let entries = raw.and_then(Value::as_object).into_iter().flatten();
    entries.filter_map(|(id, value)| Some((id.clone(), value.as_bool()?))).collect()
}

/// Whether a stored flag of any type counts as set.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// What the caller of a transition has to carry out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[must_use]
pub struct Effects {
    /// Store [`ToolWindowLayout::serialized`] under [`storage_key`] of [`ToolWindowLayout::scope`].
    pub persist: bool,
    /// Move the focus to the active editor (or the editor area when none is open).
    pub focus_editor: bool,
}

impl Effects {
    const NONE: Self = Self { persist: false, focus_editor: false };
    const PERSIST: Self = Self { persist: true, focus_editor: false };
    const FOCUS_EDITOR: Self = Self { persist: false, focus_editor: true };

    fn and(self, other: Self) -> Self {
        Self { persist: self.persist || other.persist, focus_editor: self.focus_editor || other.focus_editor }
    }
}

/// A request for a tool window's panel to take the focus; `seq` grows with every request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusRequest {
    pub id: String,
    pub seq: u64,
}

/// Handle of one [`ToolWindowLayout::register`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registration(u64);

#[derive(Debug, Clone)]
struct Registered {
    serial: u64,
    window: ToolWindow,
}

/// The registered tool windows and the layout of the open project.
#[derive(Debug, Clone)]
pub struct ToolWindowLayout {
    /// In registration order, which breaks ties of order and title.
    windows: Vec<Registered>,
    next_serial: u64,
    /// Last shown or focused tool window, kept after it is hidden (Jump to Last Tool Window).
    last_used: Option<String>,
    focus_seq: u64,
    scope: String,
    visible: SlotRecord,
    sizes: PerAnchor<u32>,
    splits: PerAnchor<f64>,
    maximized: Option<Anchor>,
    focused: Option<String>,
    last_active: Option<String>,
    mounted: BTreeSet<String>,
    focus_request: Option<FocusRequest>,
    /// Visible set stashed by "Hide All" so pressing it again restores.
    stashed: Option<SlotRecord>,
    stashed_maximized: Option<Anchor>,
    placements: BTreeMap<String, Placement>,
    stripe_buttons: BTreeMap<String, bool>,
    /// Layout before Distraction Free Mode; restored on exit and stored in place of `visible` meanwhile.
    distraction_free_stash: Option<SlotRecord>,
}

impl Default for ToolWindowLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolWindowLayout {
    /// No window registered, no project: the default layout.
    pub fn new() -> Self {
        let visible = SlotRecord::default_visible();
        Self {
            windows: Vec::new(),
            next_serial: 0,
            last_used: None,
            focus_seq: 0,
            scope: String::new(),
            mounted: visible.ids().map(str::to_owned).collect(),
            visible,
            sizes: DEFAULT_SIZES,
            splits: DEFAULT_SPLITS,
            maximized: None,
            focused: None,
            last_active: None,
            focus_request: None,
            stashed: None,
            stashed_maximized: None,
            placements: BTreeMap::new(),
            stripe_buttons: BTreeMap::new(),
            distraction_free_stash: None,
        }
    }

    /// Registers `window`, replacing a window of the same id (which keeps its place in the
    /// registration order). A visible window recorded in the wrong area moves to its own.
    pub fn register(&mut self, window: ToolWindow) -> (Registration, Effects) {
        self.next_serial += 1;
        let entry = Registered { serial: self.next_serial, window };
        match self.windows.iter().position(|registered| registered.window.id == entry.window.id) {
            Some(index) => self.windows[index] = entry,
            None => self.windows.push(entry),
        }
        (Registration(self.next_serial), self.registry_changed())
    }

    /// Removes the window `registration` registered, unless a later registration of the same id
    /// replaced it.
    pub fn unregister(&mut self, registration: Registration) -> Effects {
        let count = self.windows.len();
        self.windows.retain(|registered| registered.serial != registration.0);
        if self.windows.len() == count { Effects::NONE } else { self.registry_changed() }
    }

    fn registry_changed(&mut self) -> Effects {
        let Some(visible) = self.reconciled(&self.visible, &self.placements) else {
            return Effects::NONE;
        };
        self.mounted.extend(visible.ids().map(str::to_owned));
        self.visible = visible;
        Effects { persist: !self.scope.is_empty() && self.distraction_free_stash.is_none(), focus_editor: false }
    }

    /// The registered windows, in registration order.
    pub fn windows(&self) -> impl Iterator<Item = &ToolWindow> {
        self.windows.iter().map(|registered| &registered.window)
    }

    pub fn window(&self, id: &str) -> Option<&ToolWindow> {
        self.windows().find(|window| window.id == id)
    }

    /// The windows docked in `slot`, in stripe order.
    pub fn windows_in(&self, slot: Slot) -> Vec<&ToolWindow> {
        let mut members: Vec<(&ToolWindow, Option<f64>)> = self
            .windows()
            .filter_map(|window| {
                let (home, order) = docking(window, self.placements.get(&window.id));
                (home == slot).then_some((window, order))
            })
            .collect();
        members.sort_by(by_order);
        members.into_iter().map(|(window, _)| window).collect()
    }

    /// The area a registered window docks in, with the user's placement applied.
    pub fn slot_of(&self, id: &str) -> Option<Slot> {
        self.window(id).map(|window| docking(window, self.placements.get(id)).0)
    }

    pub fn anchor_of(&self, id: &str) -> Option<Anchor> {
        self.slot_of(id).map(Slot::anchor)
    }

    /// The project root the layout belongs to; "" without a project.
    pub fn scope(&self) -> &str {
        &self.scope
    }

    pub fn visible(&self) -> &SlotRecord {
        &self.visible
    }

    pub fn sizes(&self) -> PerAnchor<u32> {
        self.sizes
    }

    pub fn splits(&self) -> PerAnchor<f64> {
        self.splits
    }

    /// The side that covers the editor and the other sides.
    pub fn maximized(&self) -> Option<Anchor> {
        self.maximized
    }

    /// Tool window containing the focus. The `toolWindowFocus` context key is whether there is
    /// one, and `activeToolWindow` its id ("" without one).
    pub fn focused(&self) -> Option<&str> {
        self.focused.as_deref()
    }

    /// Most recently activated window (target of Hide Active Tool Window).
    pub fn last_active(&self) -> Option<&str> {
        self.last_active.as_deref()
    }

    /// The window the tool window actions act on: the focused one, else the last active.
    pub fn active(&self) -> Option<&str> {
        self.focused().or(self.last_active())
    }

    /// Whether `id` was shown at least once: such windows are kept alive while hidden in their area.
    pub fn is_mounted(&self, id: &str) -> bool {
        self.mounted.contains(id)
    }

    /// The latest request for a panel to take the focus.
    pub fn focus_request(&self) -> Option<&FocusRequest> {
        self.focus_request.as_ref()
    }

    /// Per-window placement overrides.
    pub fn placements(&self) -> &BTreeMap<String, Placement> {
        &self.placements
    }

    /// User choice of stripe buttons (opened → added, "Hide" → removed).
    pub fn stripe_buttons(&self) -> &BTreeMap<String, bool> {
        &self.stripe_buttons
    }

    pub fn is_distraction_free(&self) -> bool {
        self.distraction_free_stash.is_some()
    }

    pub fn is_visible(&self, id: &str) -> bool {
        self.slot_of(id).is_some_and(|slot| {
            self.maximized.is_none_or(|maximized| maximized == slot.anchor()) && self.visible.get(slot) == Some(id)
        })
    }

    /// The `toolWindow.<id>.visible` context key of every registered window.
    pub fn visibility_context(&self) -> Vec<(String, bool)> {
        let entry = |window: &ToolWindow| (format!("toolWindow.{}.visible", window.id), self.is_visible(&window.id));
        self.windows().map(entry).collect()
    }

    /// Whether `window` has a stripe button: the user's choice, else its default.
    pub fn has_stripe_button(&self, window: &ToolWindow) -> bool {
        self.stripe_buttons.get(&window.id).copied().unwrap_or(window.stripe != Stripe::OnUse)
    }

    /// `visible` with the windows recorded in an area they don't dock in (a layout saved before a
    /// window became split, or before split areas existed) moved to their area, unless that area
    /// already shows another window. None: no change.
    fn reconciled(&self, visible: &SlotRecord, placements: &BTreeMap<String, Placement>) -> Option<SlotRecord> {
        let mut out: Option<SlotRecord> = None;
        for slot in SLOTS {
            let Some(id) = visible.get(slot) else { continue };
            let Some(window) = self.window(id) else { continue };
            let home = docking(window, placements.get(id)).0;
            if home == slot {
                continue;
            }
            let moved = out.get_or_insert_with(|| visible.clone());
            *moved.slot_mut(slot) = None;
            if moved.get(home).is_none() {
                *moved.slot_mut(home) = Some(id.to_owned());
            }
        }
        out
    }

    /// Loads the layout stored for `scope` (a project root; "" without a project): `stored` is
    /// what [`storage_key`] holds. Missing or unreadable parts take their defaults.
    pub fn load(&mut self, scope: &str, stored: Option<&str>) {
        let saved = stored.and_then(|raw| serde_json::from_str::<Value>(raw).ok());
        let version = saved.as_ref().and_then(|saved| saved.get("v")).and_then(Value::as_f64).unwrap_or(1.0);
        let layout = LayoutSnapshot::sanitize(saved.as_ref(), version);
        let visible = self.reconciled(&layout.visible, &layout.placements).unwrap_or(layout.visible);
        *self = Self {
            windows: std::mem::take(&mut self.windows),
            next_serial: self.next_serial,
            last_used: self.last_used.take(),
            focus_seq: self.focus_seq,
            scope: scope.to_owned(),
            mounted: visible.ids().map(str::to_owned).collect(),
            visible,
            sizes: layout.sizes,
            splits: layout.splits,
            placements: layout.placements,
            stripe_buttons: layout.stripe_buttons,
            ..Self::new()
        };
    }

    /// The layout Window › Tool Window Layouts saves. Distraction Free Mode is not saved: the
    /// layout to restore is.
    pub fn snapshot(&self) -> LayoutSnapshot {
        LayoutSnapshot {
            visible: self.distraction_free_stash.as_ref().unwrap_or(&self.visible).clone(),
            sizes: self.sizes,
            splits: self.splits,
            placements: self.placements.clone(),
            stripe_buttons: self.stripe_buttons.clone(),
        }
    }

    /// The project's layout as it is stored.
    pub fn serialized(&self) -> SavedLayout {
        SavedLayout { v: LAYOUT_VERSION, layout: self.snapshot() }
    }

    /// Applies a saved layout to this project (and has it stored as the project's layout).
    pub fn apply(&mut self, snapshot: LayoutSnapshot) -> Effects {
        // Windows registered later keep their saved area; unknown ones simply never show.
        let visible = self.reconciled(&snapshot.visible, &snapshot.placements).unwrap_or(snapshot.visible);
        self.mounted.extend(visible.ids().map(str::to_owned));
        let focus_editor = self.focused.as_deref().is_some_and(|id| !visible.contains(id));
        self.visible = visible;
        self.sizes = snapshot.sizes;
        self.splits = snapshot.splits;
        self.placements = snapshot.placements;
        self.stripe_buttons = snapshot.stripe_buttons;
        self.maximized = None;
        self.stashed = None;
        self.distraction_free_stash = None;
        Effects { persist: true, focus_editor }
    }

    /// Moves a tool window to an area (stripe group / "Move To"), before `before` (or last). Orders
    /// of the target area are renumbered so the drop position sticks. A visible window stays
    /// visible in its new area (replacing what was shown there).
    pub fn move_window(&mut self, id: &str, slot: Slot, before: Option<&str>) -> Effects {
        let Some(old_slot) = self.slot_of(id) else {
            return Effects::NONE;
        };
        let was_visible = self.visible.get(old_slot) == Some(id);
        let mut members: Vec<(String, Anchor)> = self
            .windows_in(slot)
            .into_iter()
            .filter(|window| window.id != id)
            .map(|window| (window.id.clone(), window.anchor))
            .collect();
        let at = before.and_then(|before| members.iter().position(|(member, _)| member == before));
        members.insert(at.unwrap_or(members.len()), (id.to_owned(), slot.anchor()));
        for (index, (member, anchor)) in members.into_iter().enumerate() {
            let order = Some(((index + 1) * 10) as f64);
            let placement = if member == id {
                Placement { anchor: slot.anchor(), split: Some(slot.is_split()), order }
            } else {
                let current = self.placements.get(&member).copied();
                Placement { order, ..current.unwrap_or(Placement { anchor, split: None, order: None }) }
            };
            self.placements.insert(member, placement);
        }
        if was_visible && old_slot != slot {
            *self.visible.slot_mut(old_slot) = None;
            *self.visible.slot_mut(slot) = Some(id.to_owned());
        }
        let old_anchor = old_slot.anchor();
        if was_visible && self.maximized == Some(old_anchor) && old_anchor != slot.anchor() {
            self.maximized = None;
        }
        Effects::PERSIST
    }

    /// Forgets every placement override and stripe button choice ("Restore Default Layout").
    pub fn reset_placements(&mut self) -> Effects {
        // Windows shown in an area they no longer belong to are hidden.
        for slot in SLOTS {
            let shown = self.visible.get(slot).and_then(|id| self.window(id));
            if shown.is_some_and(|window| window.default_slot() != slot) {
                *self.visible.slot_mut(slot) = None;
            }
        }
        self.placements.clear();
        self.stripe_buttons.clear();
        Effects::PERSIST
    }

    fn request_focus(&mut self, id: &str) {
        self.focus_seq += 1;
        self.focus_request = Some(FocusRequest { id: id.to_owned(), seq: self.focus_seq });
    }

    /// Shows `id` in its area, replacing the window shown there; `focus` also asks its panel to
    /// take the focus. Unknown ids are ignored.
    pub fn show(&mut self, id: &str, focus: bool) -> Effects {
        let Some(slot) = self.slot_of(id) else {
            return Effects::NONE;
        };
        self.last_used = Some(id.to_owned());
        self.mounted.insert(id.to_owned());
        *self.visible.slot_mut(slot) = Some(id.to_owned());
        // Opening a window puts its button on the stripe (IDEA).
        self.stripe_buttons.insert(id.to_owned(), true);
        self.last_active = Some(id.to_owned());
        self.stashed = None;
        if self.maximized.is_some_and(|maximized| maximized != slot.anchor()) {
            self.maximized = None;
        }
        if focus {
            self.request_focus(id);
        }
        Effects::PERSIST
    }

    /// Hides `id` if its area shows it. The focus it held goes to the other window of a maximized
    /// side, else to the editor.
    pub fn hide(&mut self, id: &str) -> Effects {
        let Some(slot) = self.slot_of(id) else {
            return Effects::NONE;
        };
        if self.visible.get(slot) != Some(id) {
            return Effects::NONE;
        }
        let had_focus = self.focused.as_deref() == Some(id);
        let was_maximized = self.maximized == Some(slot.anchor());
        let partner = self.visible.get(slot.partner()).map(str::to_owned);
        *self.visible.slot_mut(slot) = None;
        // The side stays maximized while its other area still shows a window.
        if was_maximized && partner.is_none() {
            self.maximized = None;
        }
        if self.last_active.as_deref() == Some(id) {
            self.last_active = None;
        }
        let refocus = if !had_focus {
            Effects::NONE
        } else if let Some(partner) = partner.filter(|_| was_maximized) {
            self.focus(&partner)
        } else {
            Effects::FOCUS_EDITOR
        };
        Effects::PERSIST.and(refocus)
    }

    /// Stripe button context menu "Hide": closes the window and removes its button (back under "…").
    pub fn remove_from_stripe(&mut self, id: &str) -> Effects {
        let hidden = self.hide(id);
        self.stripe_buttons.insert(id.to_owned(), false);
        Effects::PERSIST.and(hidden)
    }

    pub fn toggle(&mut self, id: &str) -> Effects {
        if self.is_visible(id) { self.hide(id) } else { self.show(id, true) }
    }

    /// The `toolwindow.<id>` action: hidden → show + focus; visible but unfocused → focus; visible
    /// and focused → hide (IDEA behaviour).
    pub fn activate(&mut self, id: &str) -> Effects {
        if !self.is_visible(id) {
            self.show(id, true)
        } else if self.focused.as_deref() != Some(id) {
            self.focus(id)
        } else {
            self.hide(id)
        }
    }

    /// Asks the panel of `id` to take the focus, showing the window first if it is hidden.
    pub fn focus(&mut self, id: &str) -> Effects {
        if !self.is_visible(id) {
            return self.show(id, true);
        }
        self.request_focus(id);
        self.last_active = Some(id.to_owned());
        Effects::NONE
    }

    /// The last shown or focused tool window, if it is still registered.
    pub fn last_used(&self) -> Option<&str> {
        self.last_used.as_deref().filter(|id| self.window(id).is_some())
    }

    /// F12: focuses the last used tool window, reopening it if it was hidden.
    pub fn jump_to_last(&mut self) -> Effects {
        let fallback = || self.window(DEFAULT_WINDOW).map(|window| window.id.as_str());
        let target = self.last_used().or_else(fallback).map(str::to_owned);
        target.map_or(Effects::NONE, |id| self.focus(&id))
    }

    /// Hides every visible window; called again with nothing visible, restores them.
    pub fn hide_all(&mut self) -> Effects {
        if !self.visible.is_empty() {
            self.stashed = Some(std::mem::take(&mut self.visible));
            self.stashed_maximized = self.maximized.take();
            Effects { persist: true, focus_editor: self.focused.is_some() }
        } else if let Some(stashed) = self.stashed.take() {
            self.visible = stashed;
            self.maximized = self.stashed_maximized.take();
            Effects::PERSIST
        } else {
            Effects::NONE
        }
    }

    /// Entering Distraction Free Mode: remembers the visible windows and hides them. Windows can
    /// still be opened meanwhile (by shortcut); exiting restores the remembered layout. No-op
    /// without a project layout loaded.
    pub fn enter_distraction_free(&mut self) -> Effects {
        if self.scope.is_empty() || self.distraction_free_stash.is_some() {
            return Effects::NONE;
        }
        self.distraction_free_stash = Some(std::mem::take(&mut self.visible));
        self.maximized = None;
        self.stashed = None;
        Effects { persist: true, focus_editor: self.focused.is_some() }
    }

    pub fn exit_distraction_free(&mut self) -> Effects {
        let Some(visible) = self.distraction_free_stash.take() else {
            return Effects::NONE;
        };
        self.mounted.extend(visible.ids().map(str::to_owned));
        let focus_editor = self.focused.as_deref().is_some_and(|id| !visible.contains(id));
        self.visible = visible;
        self.maximized = None;
        self.stashed = None;
        Effects { persist: true, focus_editor }
    }

    pub fn hide_active(&mut self) -> Effects {
        self.active().map(str::to_owned).map_or(Effects::NONE, |id| self.hide(&id))
    }

    /// Sets the size of a side in pixels. Ignored while the side is maximized and below the
    /// minimum size.
    pub fn set_size(&mut self, anchor: Anchor, px: f64) -> Effects {
        let rounded = px.round();
        if !rounded.is_finite() || self.maximized == Some(anchor) || rounded < MIN_SIZE {
            return Effects::NONE;
        }
        let size = rounded as u32;
        if self.sizes.get(anchor) == size {
            return Effects::NONE;
        }
        self.sizes.set(anchor, size);
        Effects::PERSIST
    }

    /// Window › Resize Tool Window: moves the inner edge of the active tool window by one step,
    /// within a viewport of the given size.
    pub fn resize_active(&mut self, dir: ResizeDirection, viewport_width: f64, viewport_height: f64) -> Effects {
        let Some(anchor) = self.active().filter(|id| self.is_visible(id)).and_then(|id| self.anchor_of(id)) else {
            return Effects::NONE;
        };
        let delta = resize_delta(anchor, dir);
        if delta == 0 {
            return Effects::NONE;
        }
        let extent = if anchor == Anchor::Bottom { viewport_height } else { viewport_width };
        let size = f64::from(self.sizes.get(anchor)) + f64::from(delta);
        self.set_size(anchor, size.min(extent - RESIZE_MARGIN))
    }

    /// Sets the share of `anchor`'s side given to its primary area when both areas show.
    pub fn set_split(&mut self, anchor: Anchor, ratio: f64) -> Effects {
        if ratio.is_nan() {
            return Effects::NONE;
        }
        let share = ((ratio * 1000.0).round() / 1000.0).clamp(MIN_SPLIT, MAX_SPLIT);
        if self.splits.get(anchor) == share {
            return Effects::NONE;
        }
        self.splits.set(anchor, share);
        Effects::PERSIST
    }

    /// Maximizes the side of `id` (showing the window first if it is hidden), or restores it.
    pub fn toggle_maximize(&mut self, id: &str) -> Effects {
        let Some(anchor) = self.anchor_of(id) else {
            return Effects::NONE;
        };
        let shown = if self.is_visible(id) { Effects::NONE } else { self.show(id, true) };
        self.maximized = if self.maximized == Some(anchor) { None } else { Some(anchor) };
        self.request_focus(id);
        shown
    }

    /// Reveals the editor when navigation opens or focuses an editor tab. Whether a side was maximized.
    pub fn restore_maximized(&mut self) -> bool {
        self.maximized.take().is_some()
    }

    /// Records which tool window contains the focus (none: the focus is elsewhere).
    pub fn set_focused(&mut self, id: Option<&str>) {
        if self.focused.as_deref() == id {
            return;
        }
        if let Some(id) = id {
            self.last_used = Some(id.to_owned());
            self.last_active = Some(id.to_owned());
        }
        self.focused = id.map(str::to_owned);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Size change of a window docked at `anchor` for an arrow: moving its inner edge outwards grows it.
pub fn resize_delta(anchor: Anchor, dir: ResizeDirection) -> i32 {
    let (grow, shrink) = match anchor {
        Anchor::Left => (ResizeDirection::Right, ResizeDirection::Left),
        Anchor::Right => (ResizeDirection::Left, ResizeDirection::Right),
        Anchor::Bottom => (ResizeDirection::Up, ResizeDirection::Down),
    };
    if dir == grow {
        RESIZE_STEP
    } else if dir == shrink {
        -RESIZE_STEP
    } else {
        0
    }
}

/// A layout saved under a name.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NamedLayout {
    pub name: String,
    pub layout: LayoutSnapshot,
}

/// Window › Tool Window Layouts (IDEA's ToolWindowLayoutManager): named snapshots of the tool
/// window layout, shared by every project. Stored as its JSON under [`NAMED_LAYOUTS_STORAGE_KEY`],
/// which the caller rewrites after every change.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct NamedLayouts {
    /// In save order.
    layouts: Vec<NamedLayout>,
    /// Name of the layout last saved or restored (shown checked).
    active: Option<String>,
}

impl NamedLayouts {
    /// The stored layouts; malformed entries are left out.
    pub fn parse(stored: Option<&str>) -> Self {
        let raw = stored.and_then(|raw| serde_json::from_str::<Value>(raw).ok());
        let field = |name: &str| raw.as_ref().and_then(|raw| raw.get(name));
        let entries = field("layouts").and_then(Value::as_array).into_iter().flatten();
        let layouts: Vec<NamedLayout> = entries
            .filter_map(|entry| {
                let name = entry.get("name").and_then(Value::as_str).filter(|name| !name.trim().is_empty())?;
                let layout = LayoutSnapshot::sanitize(entry.get("layout"), f64::from(LAYOUT_VERSION));
                Some(NamedLayout { name: name.to_owned(), layout })
            })
            .collect();
        let active = field("active").and_then(Value::as_str).filter(|name| layouts.iter().any(|l| l.name == *name));
        Self { active: active.map(str::to_owned), layouts }
    }

    pub fn names(&self) -> Vec<&str> {
        self.layouts.iter().map(|layout| layout.name.as_str()).collect()
    }

    pub fn active(&self) -> Option<&str> {
        self.active.as_deref()
    }

    /// Whether no layout is saved (the `toolWindowLayoutsSaved` context key is the opposite).
    pub fn is_empty(&self) -> bool {
        self.layouts.is_empty()
    }

    /// Save Current Layout as…: replaces a layout of the same name. False when the name is empty.
    pub fn save(&mut self, name: &str, layout: LayoutSnapshot) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let entry = NamedLayout { name: name.to_owned(), layout };
        match self.layouts.iter().position(|existing| existing.name == name) {
            Some(index) => self.layouts[index] = entry,
            None => self.layouts.push(entry),
        }
        self.active = Some(name.to_owned());
        true
    }

    /// The layout saved as `name`, for [`ToolWindowLayout::apply`]; it becomes the active one.
    #[must_use]
    pub fn restore(&mut self, name: &str) -> Option<LayoutSnapshot> {
        let layout = self.layouts.iter().find(|layout| layout.name == name)?.layout.clone();
        self.active = Some(name.to_owned());
        Some(layout)
    }

    /// The default layout, for [`ToolWindowLayout::apply`]; no saved layout is active afterwards.
    #[must_use]
    pub fn restore_default(&mut self) -> LayoutSnapshot {
        self.active = None;
        LayoutSnapshot::default()
    }

    /// Renames `from`; false when `to` is empty or taken by another layout.
    pub fn rename(&mut self, from: &str, to: &str) -> bool {
        let to = to.trim();
        if to.is_empty() || (to != from && self.layouts.iter().any(|layout| layout.name == to)) {
            return false;
        }
        for layout in self.layouts.iter_mut().filter(|layout| layout.name == from) {
            layout.name = to.to_owned();
        }
        if self.active.as_deref() == Some(from) {
            self.active = Some(to.to_owned());
        }
        true
    }

    pub fn delete(&mut self, name: &str) {
        self.layouts.retain(|layout| layout.name != name);
        if self.active.as_deref() == Some(name) {
            self.active = None;
        }
    }
}

/// Handle of one [`ContentTabs::register`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentStripId(u64);

#[derive(Debug)]
struct ContentStrip<H> {
    id: ContentStripId,
    window: String,
    handle: H,
}

/// Content tab strips rendered in each tool window's header (Run sessions, Find tabs, terminals…),
/// each known by a handle `H` of the strip's owner.
#[derive(Debug)]
pub struct ContentTabs<H> {
    /// In registration order.
    strips: Vec<ContentStrip<H>>,
    next_id: u64,
}

impl<H> Default for ContentTabs<H> {
    fn default() -> Self {
        Self { strips: Vec::new(), next_id: 0 }
    }
}

impl<H> ContentTabs<H> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a strip to the header of tool window `window`.
    pub fn register(&mut self, window: &str, handle: H) -> ContentStripId {
        self.next_id += 1;
        let id = ContentStripId(self.next_id);
        self.strips.push(ContentStrip { id, window: window.to_owned(), handle });
        id
    }

    pub fn unregister(&mut self, strip: ContentStripId) {
        self.strips.retain(|registered| registered.id != strip);
    }

    /// Closes the selected content tab of tool window `window`: `close` is tried on its strips
    /// until one closes a tab. False when it has none that can be closed.
    pub fn close_selected(&self, window: &str, mut close: impl FnMut(&H) -> bool) -> bool {
        self.strips.iter().filter(|strip| strip.window == window).any(|strip| close(&strip.handle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const VIEWPORT_WIDTH: f64 = 1024.0;
    const VIEWPORT_HEIGHT: f64 = 768.0;

    /// A layout with the storage its effects are carried out on.
    struct Harness {
        layout: ToolWindowLayout,
        storage: BTreeMap<String, String>,
        registrations: Vec<Registration>,
    }

    impl Harness {
        /// Project and Structure on the left, Terminal and Problems at the bottom, `/work/a` loaded.
        fn new() -> Self {
            let mut harness = Self::with_windows(vec![
                ToolWindow::new("project", "Project", "folder", Anchor::Left),
                ToolWindow::new("structure", "Structure", "structure", Anchor::Left),
                ToolWindow::new("terminal", "Terminal", "terminal", Anchor::Bottom),
                ToolWindow::new("problems", "Problems", "problems", Anchor::Bottom),
            ]);
            harness.load("/work/a");
            harness
        }

        /// [`Self::new`] plus Bookmarks in the left split area.
        fn with_bookmarks() -> Self {
            let mut harness = Self::new();
            let bookmarks = ToolWindow::new("bookmarks", "Bookmarks", "bookmark", Anchor::Left);
            harness.register(ToolWindow { secondary: true, ..bookmarks });
            harness
        }

        /// Project and Structure on the left, Terminal at the bottom, `/work/a` loaded.
        fn for_named_layouts() -> Self {
            let mut harness = Self::with_windows(vec![
                ToolWindow::new("project", "Project", "folder", Anchor::Left),
                ToolWindow::new("structure", "Structure", "structure", Anchor::Left),
                ToolWindow::new("terminal", "Terminal", "terminal", Anchor::Bottom),
            ]);
            harness.load("/work/a");
            harness
        }

        fn with_windows(windows: Vec<ToolWindow>) -> Self {
            let mut harness =
                Self { layout: ToolWindowLayout::new(), storage: BTreeMap::new(), registrations: Vec::new() };
            for window in windows {
                harness.register(window);
            }
            harness
        }

        fn register(&mut self, window: ToolWindow) -> Registration {
            let (registration, effects) = self.layout.register(window);
            self.run(|_| effects);
            self.registrations.push(registration);
            registration
        }

        fn run(&mut self, transition: impl FnOnce(&mut ToolWindowLayout) -> Effects) {
            if transition(&mut self.layout).persist {
                let json = serde_json::to_string(&self.layout.serialized()).unwrap();
                self.storage.insert(storage_key(self.layout.scope()), json);
            }
        }

        fn resize(&mut self, dir: ResizeDirection) {
            self.run(|l| l.resize_active(dir, VIEWPORT_WIDTH, VIEWPORT_HEIGHT));
        }

        fn load(&mut self, scope: &str) {
            let stored = self.storage.get(&storage_key(scope)).map(String::as_str);
            self.layout.load(scope, stored);
        }

        fn store(&mut self, scope: &str, saved: Value) {
            self.storage.insert(storage_key(scope), saved.to_string());
        }

        fn stored(&self, scope: &str) -> Value {
            serde_json::from_str(&self.storage[&storage_key(scope)]).unwrap()
        }

        fn context_visible(&self, id: &str) -> Option<bool> {
            let key = format!("toolWindow.{id}.visible");
            self.layout.visibility_context().into_iter().find(|(name, _)| *name == key).map(|(_, visible)| visible)
        }

        fn apply_named(&mut self, layouts: &mut NamedLayouts, name: &str) -> bool {
            let Some(layout) = layouts.restore(name) else {
                return false;
            };
            self.run(|layout_state| layout_state.apply(layout));
            true
        }
    }

    // src/workbench/toolwindows/store.test.ts › shows one window per anchor; showing another replaces it
    #[test]
    fn shows_one_window_per_anchor_showing_another_replaces_it() {
        let mut h = Harness::new();
        assert!(h.layout.is_visible("project"));
        h.run(|l| l.show("structure", false));
        assert!(h.layout.is_visible("structure"));
        assert!(!h.layout.is_visible("project"));
        h.run(|l| l.show("terminal", false));
        assert!(h.layout.is_visible("terminal"));
        assert!(h.layout.is_visible("structure"));
        assert_eq!(h.context_visible("terminal"), Some(true));
        // previously shown windows stay mounted (keep-alive)
        assert!(h.layout.is_mounted("project"));
    }

    // src/workbench/toolwindows/store.test.ts › toggle shows (requesting focus) and hides
    #[test]
    fn toggle_shows_requesting_focus_and_hides() {
        let mut h = Harness::new();
        h.run(|l| l.toggle("terminal"));
        assert!(h.layout.is_visible("terminal"));
        assert_eq!(h.layout.focus_request().map(|request| request.id.as_str()), Some("terminal"));
        h.run(|l| l.toggle("terminal"));
        assert!(!h.layout.is_visible("terminal"));
    }

    // src/workbench/toolwindows/store.test.ts › activate: hidden → show, visible unfocused → focus, focused → hide
    #[test]
    fn activate_shows_then_focuses_then_hides() {
        let mut h = Harness::new();
        h.run(|l| l.activate("problems"));
        assert!(h.layout.is_visible("problems"));
        let seq = h.layout.focus_request().unwrap().seq;
        h.run(|l| l.activate("problems"));
        assert!(h.layout.is_visible("problems"));
        assert!(h.layout.focus_request().unwrap().seq > seq);
        h.layout.set_focused(Some("problems"));
        assert_eq!(h.layout.focused(), Some("problems"));
        h.run(|l| l.activate("problems"));
        assert!(!h.layout.is_visible("problems"));
    }

    // src/workbench/toolwindows/store.test.ts › hide all stashes and restores; hide active hides the focused window
    #[test]
    fn hide_all_stashes_and_restores_hide_active_hides_the_focused_window() {
        let mut h = Harness::new();
        h.run(|l| l.show("terminal", false));
        h.run(ToolWindowLayout::hide_all);
        assert!(!h.layout.is_visible("project"));
        assert!(!h.layout.is_visible("terminal"));
        h.run(ToolWindowLayout::hide_all);
        assert!(h.layout.is_visible("project"));
        assert!(h.layout.is_visible("terminal"));
        h.layout.set_focused(Some("terminal"));
        h.run(ToolWindowLayout::hide_active);
        assert!(!h.layout.is_visible("terminal"));
        assert!(h.layout.is_visible("project"));
    }

    // src/workbench/toolwindows/store.test.ts › restores the maximized side through the shared Hide All toggle
    #[test]
    fn restores_the_maximized_side_through_the_shared_hide_all_toggle() {
        let mut h = Harness::new();
        h.run(|l| l.show("terminal", false));
        h.run(|l| l.toggle_maximize("terminal"));
        let before = h.layout.visible().clone();
        h.run(ToolWindowLayout::hide_all);
        assert_eq!(h.layout.maximized(), None);
        h.run(ToolWindowLayout::hide_all);
        assert_eq!(h.layout.visible(), &before);
        assert_eq!(h.layout.maximized(), Some(Anchor::Bottom));
    }

    // src/workbench/toolwindows/store.test.ts › resizes the active window by moving its inner edge
    #[test]
    fn resizes_the_active_window_by_moving_its_inner_edge() {
        let mut h = Harness::new();
        h.run(|l| l.show("terminal", false));
        h.layout.set_focused(Some("terminal"));
        let before = h.layout.sizes().bottom;
        h.resize(ResizeDirection::Up);
        assert_eq!(h.layout.sizes().bottom, before.saturating_add_signed(RESIZE_STEP));
        h.resize(ResizeDirection::Down);
        h.resize(ResizeDirection::Down);
        assert_eq!(h.layout.sizes().bottom, before.saturating_add_signed(-RESIZE_STEP));
        h.resize(ResizeDirection::Left);
        assert_eq!(h.layout.sizes().bottom, before.saturating_add_signed(-RESIZE_STEP));
        h.layout.set_focused(Some("project"));
        let left = h.layout.sizes().left;
        h.resize(ResizeDirection::Right);
        assert_eq!(h.layout.sizes().left, left.saturating_add_signed(RESIZE_STEP));
    }

    // src/workbench/toolwindows/store.test.ts › ignores unknown ids and hiding a window that isn't visible
    #[test]
    fn ignores_unknown_ids_and_hiding_a_window_that_is_not_visible() {
        let mut h = Harness::new();
        h.run(|l| l.show("nope", false));
        h.run(|l| l.hide("structure"));
        assert!(h.layout.is_visible("project"));
    }

    // src/workbench/toolwindows/store.test.ts › is exposed through the ToolWindowService
    #[test]
    fn shows_hides_and_reports_visibility_as_the_tool_window_service_does() {
        let mut h = Harness::new();
        h.run(|l| l.show("terminal", false));
        assert!(h.layout.is_visible("terminal"));
        h.run(|l| l.hide("terminal"));
        assert!(!h.layout.is_visible("terminal"));
    }

    fn preserves_the_layout_through_maximize_and_reload(anchor: Anchor) {
        let mut h = Harness::new();
        h.run(|l| l.move_window("terminal", Slot::new(anchor, false), None));
        h.run(|l| l.move_window("problems", Slot::new(anchor, true), None));
        h.run(|l| l.show("terminal", false));
        h.run(|l| l.show("problems", false));
        h.run(|l| l.set_size(anchor, 987.0));
        h.run(|l| l.set_split(anchor, 0.63));
        let before = (h.layout.visible().clone(), h.layout.sizes(), h.layout.splits());
        h.run(|l| l.toggle_maximize("terminal"));
        h.run(|l| l.set_size(anchor, 111.0));
        assert!(h.layout.is_visible("terminal"));
        assert!(h.layout.is_visible("problems"));
        if anchor != Anchor::Left {
            assert!(!h.layout.is_visible("project"));
            assert_eq!(h.context_visible("project"), Some(false));
        }
        h.run(|l| l.toggle_maximize("terminal"));
        assert_eq!((h.layout.visible().clone(), h.layout.sizes(), h.layout.splits()), before);
        h.run(|l| l.toggle_maximize("terminal"));
        // Saving another layout choice while maximized must retain all the original anchors.
        h.run(|l| l.set_split(anchor, 0.63));
        h.run(|l| l.show("terminal", false));
        h.load("/work/a");
        assert_eq!((h.layout.visible().clone(), h.layout.sizes(), h.layout.splits()), before);
        assert_eq!(h.layout.maximized(), None);
    }

    // src/workbench/toolwindows/store.test.ts › preserves the left layout through maximize and reload
    #[test]
    fn preserves_the_left_layout_through_maximize_and_reload() {
        preserves_the_layout_through_maximize_and_reload(Anchor::Left);
    }

    // src/workbench/toolwindows/store.test.ts › preserves the right layout through maximize and reload
    #[test]
    fn preserves_the_right_layout_through_maximize_and_reload() {
        preserves_the_layout_through_maximize_and_reload(Anchor::Right);
    }

    // src/workbench/toolwindows/store.test.ts › preserves the bottom layout through maximize and reload
    #[test]
    fn preserves_the_bottom_layout_through_maximize_and_reload() {
        preserves_the_layout_through_maximize_and_reload(Anchor::Bottom);
    }

    // src/workbench/toolwindows/store.test.ts › restores other anchors on navigation, opening another side, or closing the maximized window
    #[test]
    fn restores_other_anchors_on_navigation_opening_another_side_or_closing_the_maximized_window() {
        let mut h = Harness::new();
        h.run(|l| l.show("terminal", false));
        h.run(|l| l.toggle_maximize("terminal"));
        h.layout.restore_maximized();
        assert!(h.layout.is_visible("project"));
        h.run(|l| l.toggle_maximize("terminal"));
        h.run(|l| l.show("project", true));
        assert_eq!(h.layout.maximized(), None);
        h.run(|l| l.toggle_maximize("terminal"));
        h.run(|l| l.hide("terminal"));
        assert_eq!(h.layout.maximized(), None);
        assert!(h.layout.is_visible("project"));
    }

    // src/workbench/toolwindows/store.test.ts › persists visible windows and sizes per project root
    #[test]
    fn persists_visible_windows_and_sizes_per_project_root() {
        let mut h = Harness::new();
        h.run(|l| l.show("structure", false));
        h.run(|l| l.show("terminal", false));
        h.run(|l| l.set_size(Anchor::Bottom, 333.0));
        h.load("/work/b");
        assert!(h.layout.is_visible("project"));
        assert!(!h.layout.is_visible("terminal"));
        assert_ne!(h.layout.sizes().bottom, 333);
        h.load("/work/a");
        assert!(h.layout.is_visible("structure"));
        assert!(h.layout.is_visible("terminal"));
        assert_eq!(h.layout.sizes().bottom, 333);
    }

    // src/workbench/toolwindows/store.test.ts › survives corrupt storage
    #[test]
    fn survives_corrupt_storage() {
        let mut h = Harness::new();
        h.storage.insert(storage_key("/work/c"), "{not json".to_owned());
        h.load("/work/c");
        assert!(h.layout.is_visible("project"));
    }

    // src/workbench/toolwindows/store.test.ts › a later registration with the same id replaces the earlier one
    #[test]
    fn a_later_registration_with_the_same_id_replaces_the_earlier_one() {
        let mut h = Harness::new();
        let real = h.register(ToolWindow::new("project", "Project", "folder", Anchor::Left));
        // the earlier registration must not remove the replacement
        let earlier = h.registrations[0];
        h.run(|l| l.unregister(earlier));
        assert!(h.layout.is_visible("project"));
        h.run(|l| l.unregister(real));
    }

    // src/workbench/toolwindows/store.test.ts › moves a window to another area, keeps it visible and persists
    #[test]
    fn moves_a_window_to_another_area_keeps_it_visible_and_persists() {
        let mut h = Harness::new();
        h.run(|l| l.show("structure", false));
        h.run(|l| l.move_window("structure", Slot::Right, None));
        assert_eq!(h.layout.anchor_of("structure"), Some(Anchor::Right));
        assert_eq!(h.layout.visible().get(Slot::Left), None);
        assert_eq!(h.layout.visible().get(Slot::Right), Some("structure"));
        assert_eq!(h.context_visible("structure"), Some(true));
        h.load("/work/a");
        assert_eq!(h.layout.anchor_of("structure"), Some(Anchor::Right));
        assert_eq!(h.layout.visible().get(Slot::Right), Some("structure"));
        h.load("/work/b");
        assert_eq!(h.layout.anchor_of("structure"), Some(Anchor::Left));
    }

    // src/workbench/toolwindows/store.test.ts › orders windows within the target area
    #[test]
    fn orders_windows_within_the_target_area() {
        let mut h = Harness::new();
        h.run(|l| l.move_window("terminal", Slot::Left, Some("project")));
        let terminal = h.layout.placements()["terminal"];
        let project = h.layout.placements()["project"];
        assert_eq!((terminal.anchor, terminal.split), (Anchor::Left, Some(false)));
        assert!(terminal.order.unwrap() < project.order.unwrap());
        h.run(|l| l.move_window("terminal", Slot::Bottom, None));
        assert_eq!(h.layout.anchor_of("terminal"), Some(Anchor::Bottom));
    }

    // src/workbench/toolwindows/store.test.ts › restores the default layout
    #[test]
    fn restores_the_default_placements() {
        let mut h = Harness::new();
        h.run(|l| l.move_window("project", Slot::RightSplit, None));
        let project = h.layout.placements()["project"];
        assert_eq!((project.anchor, project.split), (Anchor::Right, Some(true)));
        assert_eq!(h.layout.slot_of("project"), Some(Slot::RightSplit));
        h.run(ToolWindowLayout::reset_placements);
        assert_eq!(h.layout.slot_of("project"), Some(Slot::Left));
    }

    // src/workbench/toolwindows/store.test.ts › sanitizes stored placements
    #[test]
    fn sanitizes_stored_placements() {
        let raw = json!({
            "a": { "anchor": "top" },
            "b": { "anchor": "bottom", "order": 5, "split": true },
            "c": null,
        });
        let expected = Placement { anchor: Anchor::Bottom, split: Some(true), order: Some(5.0) };
        assert_eq!(
            sanitize_placements(Some(&raw), f64::from(LAYOUT_VERSION)),
            BTreeMap::from([("b".to_owned(), expected)])
        );
    }

    // src/workbench/toolwindows/store.test.ts › shows one window per area, both areas of a side at once
    #[test]
    fn shows_one_window_per_area_both_areas_of_a_side_at_once() {
        let mut h = Harness::with_bookmarks();
        h.run(|l| l.show("bookmarks", false));
        assert!(h.layout.is_visible("project"));
        assert!(h.layout.is_visible("bookmarks"));
        assert_eq!(h.layout.visible().get(Slot::Left), Some("project"));
        assert_eq!(h.layout.visible().get(Slot::LeftSplit), Some("bookmarks"));
        h.run(|l| l.show("structure", false));
        assert!(!h.layout.is_visible("project"));
        assert!(h.layout.is_visible("bookmarks"));
    }

    // src/workbench/toolwindows/store.test.ts › Move To places a window in any of the six areas
    #[test]
    fn move_to_places_a_window_in_any_of_the_six_areas() {
        let mut h = Harness::with_bookmarks();
        h.run(|l| l.show("terminal", false));
        h.run(|l| l.move_window("terminal", Slot::BottomSplit, None));
        assert_eq!(h.layout.visible().get(Slot::Bottom), None);
        assert_eq!(h.layout.visible().get(Slot::BottomSplit), Some("terminal"));
        h.run(|l| l.show("problems", false));
        assert!(h.layout.is_visible("terminal"));
        assert!(h.layout.is_visible("problems"));
        h.run(|l| l.move_window("bookmarks", Slot::Left, None));
        assert_eq!(h.layout.slot_of("bookmarks"), Some(Slot::Left));
        // An explicit primary placement on its own side overrides the window's split default.
        h.load("/work/a");
        assert_eq!(h.layout.slot_of("bookmarks"), Some(Slot::Left));
        assert!(h.layout.is_visible("terminal"));
    }

    // src/workbench/toolwindows/store.test.ts › keeps the side maximized while its other area shows a window
    #[test]
    fn keeps_the_side_maximized_while_its_other_area_shows_a_window() {
        let mut h = Harness::with_bookmarks();
        h.run(|l| l.show("bookmarks", false));
        h.run(|l| l.toggle_maximize("project"));
        h.layout.set_focused(Some("bookmarks"));
        h.run(|l| l.hide("bookmarks"));
        assert_eq!(h.layout.maximized(), Some(Anchor::Left));
        assert_eq!(h.layout.focus_request().map(|request| request.id.as_str()), Some("project"));
        h.run(|l| l.hide("project"));
        assert_eq!(h.layout.maximized(), None);
    }

    // src/workbench/toolwindows/store.test.ts › hide all stashes and restores both areas
    #[test]
    fn hide_all_stashes_and_restores_both_areas() {
        let mut h = Harness::with_bookmarks();
        h.run(|l| l.show("bookmarks", false));
        h.run(ToolWindowLayout::hide_all);
        assert!(!h.layout.is_visible("bookmarks"));
        h.run(ToolWindowLayout::hide_all);
        assert!(h.layout.is_visible("project"));
        assert!(h.layout.is_visible("bookmarks"));
    }

    // src/workbench/toolwindows/store.test.ts › persists the split ratio, clamped
    #[test]
    fn persists_the_split_ratio_clamped() {
        let mut h = Harness::with_bookmarks();
        h.run(|l| l.set_split(Anchor::Left, 0.3));
        h.run(|l| l.set_split(Anchor::Bottom, 5.0));
        h.load("/work/a");
        assert_eq!(h.layout.splits(), PerAnchor { left: 0.3, bottom: 0.9, right: 0.5 });
    }

    // src/workbench/toolwindows/store.test.ts › migrates a layout saved with one area per side
    #[test]
    fn migrates_a_layout_saved_with_one_area_per_side() {
        let mut h = Harness::with_bookmarks();
        h.store(
            "/work/old",
            json!({
                "visible": { "left": "bookmarks", "right": "terminal", "bottom": "problems" },
                "sizes": { "left": 300, "right": 300, "bottom": 200 },
                "placements": {
                    // Old "Right Bottom" stripe group: now the Right Bottom area.
                    "terminal": { "anchor": "right", "secondary": true, "order": 10 },
                    // Renumbered in the old bottom group with the window's flag: meaningless there.
                    "problems": { "anchor": "bottom", "secondary": true, "order": 20 },
                    "bookmarks": { "anchor": "left", "order": 30 },
                },
            }),
        );
        h.load("/work/old");
        assert_eq!(h.layout.slot_of("terminal"), Some(Slot::RightSplit));
        assert_eq!(h.layout.slot_of("problems"), Some(Slot::Bottom));
        assert_eq!(h.layout.slot_of("bookmarks"), Some(Slot::LeftSplit));
        assert!(h.layout.is_visible("bookmarks"));
        assert!(h.layout.is_visible("terminal"));
        assert!(h.layout.is_visible("problems"));
        assert_eq!(h.layout.sizes().left, 300);
        h.run(|l| l.show("project", false));
        let stored = h.stored("/work/old");
        assert_eq!(stored["v"], json!(2));
        assert_eq!(stored["visible"]["left"], json!("project"));
        assert_eq!(stored["visible"]["left-split"], json!("bookmarks"));
        assert_eq!(stored["visible"]["right-split"], json!("terminal"));
    }

    // src/workbench/toolwindows/store.test.ts › moves a late-registered window recorded in the wrong area to its own
    #[test]
    fn moves_a_late_registered_window_recorded_in_the_wrong_area_to_its_own() {
        let mut h = Harness::with_bookmarks();
        h.store("/work/late", json!({ "visible": { "left": "late" } }));
        h.load("/work/late");
        assert_eq!(h.layout.visible().get(Slot::Left), Some("late"));
        h.register(ToolWindow { secondary: true, ..ToolWindow::new("late", "Late", "x", Anchor::Left) });
        assert_eq!(h.layout.visible().get(Slot::Left), None);
        assert_eq!(h.layout.visible().get(Slot::LeftSplit), Some("late"));
        assert_eq!(h.context_visible("late"), Some(true));
    }

    // src/workbench/toolwindows/store.test.ts › adds an on-use window's button when opened and removes it on Hide, persisted per project
    #[test]
    fn adds_an_on_use_windows_button_when_opened_and_removes_it_on_hide_persisted_per_project() {
        let mut h = Harness::new();
        let find = ToolWindow { stripe: Stripe::OnUse, ..ToolWindow::new("find", "Find", "search", Anchor::Bottom) };
        h.register(find.clone());
        assert!(!h.layout.has_stripe_button(&find));
        h.run(|l| l.show("find", false));
        assert!(h.layout.has_stripe_button(&find));
        h.run(|l| l.remove_from_stripe("find"));
        assert!(!h.layout.is_visible("find"));
        assert!(!h.layout.has_stripe_button(&find));

        h.run(|l| l.remove_from_stripe("terminal"));
        h.load("/work/a");
        let expected = BTreeMap::from([("find".to_owned(), false), ("terminal".to_owned(), false)]);
        assert_eq!(h.layout.stripe_buttons(), &expected);
        h.run(ToolWindowLayout::reset_placements);
        assert!(h.layout.stripe_buttons().is_empty());
    }

    /// The stored JSON is shared with the other writers of the layout: key names, `null` for an
    /// empty area and whole numbers must stay as they are.
    #[test]
    fn stores_the_layout_in_the_shared_json_shape() {
        let mut h = Harness::with_windows(vec![ToolWindow::new("project", "Project", "folder", Anchor::Left)]);
        h.load("/work/a");
        h.run(|l| l.move_window("project", Slot::RightSplit, None));
        assert_eq!(
            h.storage[&storage_key("/work/a")],
            concat!(
                r#"{"v":2,"visible":{"left":null,"left-split":null,"bottom":null,"bottom-split":null,"#,
                r#""right":null,"right-split":"project"},"sizes":{"left":240,"right":300,"bottom":280},"#,
                r#""splits":{"left":0.5,"right":0.5,"bottom":0.5},"#,
                r#""placements":{"project":{"anchor":"right","split":true,"order":10}},"stripeButtons":{}}"#,
            )
        );
    }

    // src/workbench/toolwindows/layouts.test.ts › saves and restores visible windows and sizes by name
    #[test]
    fn saves_and_restores_visible_windows_and_sizes_by_name() {
        let mut h = Harness::for_named_layouts();
        let mut layouts = NamedLayouts::default();
        h.run(|l| l.show("structure", false));
        h.run(|l| l.show("terminal", false));
        h.run(|l| l.set_size(Anchor::Bottom, 400.0));
        assert!(layouts.save("Debugging", h.layout.snapshot()));
        h.run(|l| l.show("project", false));
        h.run(|l| l.set_size(Anchor::Bottom, 200.0));

        assert!(h.apply_named(&mut layouts, "Debugging"));
        assert!(h.layout.is_visible("structure"));
        assert!(h.layout.is_visible("terminal"));
        assert_eq!(h.layout.sizes().bottom, 400);
        assert_eq!(layouts.active(), Some("Debugging"));
        assert!(!h.apply_named(&mut layouts, "Missing"));
    }

    // src/workbench/toolwindows/layouts.test.ts › restores the default layout
    #[test]
    fn restores_the_default_layout() {
        let mut h = Harness::for_named_layouts();
        let mut layouts = NamedLayouts::default();
        h.run(|l| l.show("terminal", false));
        let fresh = layouts.restore_default();
        h.run(|l| l.apply(fresh));
        assert!(!h.layout.is_visible("terminal"));
        assert!(h.layout.is_visible("project"));
    }

    // src/workbench/toolwindows/layouts.test.ts › replaces a layout of the same name, renames and deletes
    #[test]
    fn replaces_a_layout_of_the_same_name_renames_and_deletes() {
        let h = Harness::for_named_layouts();
        let mut layouts = NamedLayouts::default();
        assert!(layouts.save("A", h.layout.snapshot()));
        assert!(layouts.save("B", h.layout.snapshot()));
        assert!(layouts.save("A", h.layout.snapshot()));
        assert_eq!(layouts.names(), ["A", "B"]);
        assert!(!layouts.rename("A", "B"));
        assert!(!layouts.rename("A", " "));
        assert!(layouts.rename("A", "C"));
        assert_eq!(layouts.names(), ["C", "B"]);
        assert_eq!(layouts.active(), Some("C"));
        layouts.delete("C");
        assert_eq!(layouts.names(), ["B"]);
        assert_eq!(layouts.active(), None);
    }

    // src/workbench/toolwindows/layouts.test.ts › persists across reloads and ignores malformed entries
    #[test]
    fn persists_across_reloads_and_ignores_malformed_entries() {
        let h = Harness::for_named_layouts();
        let mut layouts = NamedLayouts::default();
        assert!(layouts.save("Kept", h.layout.snapshot()));
        let stored = serde_json::to_string(&layouts).unwrap();
        assert_eq!(NamedLayouts::parse(Some(&stored)).names(), ["Kept"]);
        let malformed = json!({ "layouts": [{ "name": "" }, { "nope": 1 }, { "name": "X", "layout": 5 }] }).to_string();
        assert_eq!(NamedLayouts::parse(Some(&malformed)).names(), ["X"]);
    }

    // src/workbench/toolwindows/content.test.ts › closes the selected tab of the first strip that can, else reports false
    #[test]
    fn closes_the_selected_tab_of_the_first_strip_that_can_else_reports_false() {
        let mut tabs = ContentTabs::new();
        let a = tabs.register("run", ("a", false));
        let b = tabs.register("run", ("b", true));
        let mut closed = Vec::new();
        assert!(tabs.close_selected("run", |(name, closes)| {
            if *closes {
                closed.push(*name);
            }
            *closes
        }));
        assert_eq!(closed, ["b"]);
        tabs.unregister(b);
        assert!(!tabs.close_selected("run", |(_, closes)| *closes));
        tabs.unregister(a);
        assert!(!tabs.close_selected("other", |(_, closes)| *closes));
    }
}
