# I-TW-CONTENT-TABS: content tabs in a tool window, on the shared header

Read at HEAD `13c01811f` (branch `gpui`). Every `app-gpui/` reference was read with `git show HEAD:<path>`. "Measured" below means read in the file at that line or found (or not found) by `git grep … HEAD`. Anything derived by reasoning is marked Hypothesis.

What the slice is, per the work plan (`docs/gpui-port/notes/WORKPLAN.md:468-478`): rows `act:toolwindow.closeContent`, `act:toolwindow.viewMode.dockPinned`, `act:toolwindow.viewMode.dockUnpinned`, `act:toolwindow.viewMode.undock`, `mod:src/workbench/toolwindows/content.ts`, `unit:…/badges.test.tsx` (1 case), `unit:…/content.test.ts` (2), `unit:…/header.test.tsx` (8), `e2e:e2e/toolWindowViewMode.spec.ts` (3). Depends on Q-VIS2 (`docs/gpui-port/notes/reports/int_visual2_brief.md` item 2: the header's content slot, and item 1: Options and Hide on hover). Needed by F-HIERARCHY-USAGES (#11), F-WB-TOOLWINDOWS (#59), F-DB-RESULT-TABS (#123), F-RUN-TABS (#134).

Note at once: three of the slice's rows (the View Mode actions) and the e2e spec are not about content tabs at all; they are the Options menu's View Mode submenu and its overlay layout. They are designed here because the plan puts them here (parts 1, 3 and 4), but they make the slice about twice the plan's size (see (e)).

## (a) The webview's behaviour

### The content tab registry (`src/workbench/toolwindows/content.ts`)

- [ ] One set of strips per tool window id; `contentTabRegistry(id)` returns a stable registration function per id (`content.ts:8-20`; the spec asserts `contentTabRegistry("run") === contentTabRegistry("run")`, `content.test.ts:8`).
- [ ] `closeSelectedContent(id)`: tries each registered strip of `id` in registration order, stops at the first whose `closeSelected()` returns true; false when none (`content.ts:29-32`).
- [ ] `togglePinSelectedContent(id)`: same, with `togglePinSelected()` (`content.ts:23-26`).
- [ ] A strip registers itself with the nearest `TabStripRegistry` provider on mount and unregisters on unmount (`src/ui/Tabs.tsx:189-212`). A strip's `closeSelected` returns false when there is no selected item, no `onClose`, the item has `closable: false`, or the item is pinned and the strip supports pinning (`Tabs.tsx:197-203`). `togglePinSelected` returns false without `onTogglePin` or a selected item (`Tabs.tsx:204-209`).
- [ ] The header provides the registry around `headerContent` (`ToolWindows.tsx:891-896`); a window with its own header provides it only for the strip marked `closeTarget` (`ToolWindows.tsx:655-669`) or with `ToolWindowContentTabs` (`ToolWindows.tsx:673-678`).

### The two actions that reach the selected content tab

- [ ] `toolwindow.closeContent` "Close Tool Window Tab", `when: "toolWindowFocus && !popupOpen"`: the focused (else last active) window's selected closable tab, else the window is hidden (`src/workbench/toolwindows/actions.ts:233-244`). Bindings: IntelliJ macOS `Meta+W`, `Ctrl+Shift+F4`; IntelliJ Windows `Ctrl+F4`, `Ctrl+Shift+F4`; VS Code macOS `Meta+W`; VS Code Windows `Ctrl+W`, `Ctrl+F4`; Eclipse `Meta+W` / `Ctrl+W` (`src/platform/keymaps/intellij-macos.ts:187`, `intellij-windows.ts:188`, `vscode-macos.ts:205`, `vscode-windows.ts:37`, `eclipse-macos.ts:19`, `eclipse-windows.ts:19`).
- [ ] `editor.pinTab` "Pin / Unpin Tab", `when: "editorTabsOpen || toolWindowFocus"`: with a focused tool window, `togglePinSelectedContent(focused)` and nothing else; otherwise the active editor tab (`src/editor/core/actions.ts:1141-1154`).

### The strip (`src/ui/Tabs.tsx`, the `compact` variant every tool window uses)

- [ ] Item fields used by tool windows: `id`, `label`, `text`, `icon`, `closable`, `pinned`, `italic`, `dimmed`, `tooltip`, `trailing` (`Tabs.tsx:29-61`).
- [ ] Close button "Close Tab" (icon `close`, 11 px) on closable, unpinned tabs; Alt+click on it calls `onCloseOthers` when given (`Tabs.tsx:509-526`). A pinned tab of a strip with `onTogglePin` shows an "Unpin Tab" button (icon `pin`, 14 px) in the close slot instead, and every close gesture leaves it alone (`Tabs.tsx:389-392`, `493-508`).
- [ ] Middle mouse down on a closable tab closes it and does not select it (`Tabs.tsx:449-454`).
- [ ] Keys on a focused tab: ←/→ (↑/↓ vertical) move and select with wrap-around, Home/End, Delete/Backspace close a closable unpinned tab, Tab moves into the content when `focusContent` says so; nothing with Meta/Ctrl/Alt held (`Tabs.tsx:315-342`). Roving tabindex: the selected tab (or the first when none) is the tab stop (`Tabs.tsx:467`).
- [ ] Overflow: when the tabs do not fit, a "Show Hidden Tabs" chevron (icon `chevron-down`, 10 px) opens a menu of every tab with the selected one checked, placed `bottom-end` (`Tabs.tsx:286-301`, `344-350`, `531-551`). The wheel's vertical delta scrolls the strip sideways (`Tabs.tsx:382-385`).
- [ ] The selected tab's whole slot (label and close button) is scrolled into view on selection, on a click of the already-selected tab, and on every resize of the strip (`Tabs.tsx:249-306`, `revealHorizontally` at `557-563`).
- [ ] `onContextMenu(id, e)` gives each window its tab menu (`Tabs.tsx:455`).
- [ ] Tool windows pass `variant="compact"` (28 px, "popups, tool window sub-tabs", `Tabs.tsx:135`).

### The header (`src/workbench/toolwindows/ToolWindows.tsx`)

- [ ] `headerContent` (a component) renders right after the title inside the header (`ToolWindows.tsx:811`, `827`, `891-896`; contribution field `src/workbench/contributions.ts:46-50`); `tabsReplaceTitle` drops the title (`contributions.ts:51-52`, `ToolWindows.tsx:761`). The header gets the `headerTabbed` class (a bottom divider) when a window has `headerContent` (`ToolWindows.tsx:773`, `contributions.ts:47-48`). CSS: `.headerContent` `margin-left: 6px`, `min-width: 0`, `overflow: hidden`; `.headerContentOnly` `margin-left: 0` (`ToolWindows.module.css:475-484`).
- [ ] Options and Hide fade in on hover, focus-within, or while a header menu is open (`ToolWindows.module.css:487-497`; Q-VIS2 item 1 owns this).
- [ ] Double click on the header, not on a button, link, input, tablist or separator: Maximize / Restore Size (`ToolWindows.tsx:774-777`).
- [ ] Middle mouse down on the header, not on a button, link, tablist or separator: hides the window (`ToolWindows.tsx:778-786`).
- [ ] The title is translated at render time and follows a language change while open (`ToolWindows.tsx:762-763`).
- [ ] Options (⋮) menu: the window's own items, separator, View Mode ›, Move To ›, Maximize / Restore Size, separator, Hide with its shortcut (`ToolWindows.tsx:695-711`). View Mode › lists the three `toolwindow.viewMode.*` actions with `arg: id`, the current one checked (`ToolWindows.tsx:156-169`).
- [ ] A click on a `button[aria-haspopup]` in the window focuses that button, so closing its menu returns focus to the window rather than the editor (`ToolWindows.tsx:881-888`).

### Which windows use content tabs, and how

All of them hand a `<Tabs variant="compact">` to `headerContent` (`git grep headerContent HEAD -- src`: code-insight Problems/Usages/Hierarchy `src/features/code-insight/index.ts:86,100,111`, coverage, debugger `src/features/debugger/index.ts:89`, github, graphql, profiler, run `src/features/run/index.ts:429`, search `src/features/search/index.ts:191`, services `src/features/services/index.ts:80`, terminal, todo, vcs Commit and Git).

- [ ] Run (`src/features/run/RunView.tsx:316-352`): one tab per session (less archived agent tabs) then stack-trace tabs; close (`closeRunTab`, which hides the window and drops its stripe button after the last tab, `src/features/run/runTabs.ts:20-26`), pin (`pinSession`), context menu `runTabMenu` = action rows `run.closeTab`, `run.closeAllTabs`, `run.closeOtherTabs`, separator, `run.pinTab` labelled "Pin Tab" / "Unpin Tab" (`runTabs.ts:82-95`), each with an `enabled` predicate (`runTabs.ts:46,53,64,72`); trailing "Archived (n)" folder menu (`RunView.tsx:349`, `355-380`). Reuse: a new run takes a finished, unpinned tab, the same configuration's first, then the selected one, then the first (`src/features/run/store.ts:776-788`); `run.pinTabs` pins new tabs (`store.ts:929`).
- [ ] Debug (`src/features/debugger/ui/DebugToolWindow.tsx:136-171`): one tab per session (children "(child)"), icon `debug`, italic when terminated, tooltip the status, pinned only for a top-level session; close goes to the owner; menu `debugTabMenu` = `debug.closeTab`, `debug.closeAllTabs`, `debug.closeOtherTabs`, separator, `debug.pinTab` (`src/features/debugger/tabActions.ts:81-94`), with `enabled` predicates (`tabActions.ts:46,53,63,71`). Reuse: a new session takes an unpinned finished top-level tab, preferring one with the same name, then the selected one (`src/features/debugger/state.ts:309-330`).
- [ ] Find (`src/features/search/find/FindToolWindow.tsx:164-207`): one tab per search, pinnable; menu rows Close Tab, Close All Tabs, Close Other Tabs (disabled under two tabs), separator, Pin Tab / Unpin Tab. Reuse: a new search replaces the selected tab unless it is pinned or "Open results in new tab" is set; at most `MAX_TABS`, the oldest goes (`src/features/search/find/windowStore.ts:60-86`).
- [ ] Hierarchy (`src/features/code-insight/HierarchyView.tsx:62-101`): same menu as Find; a new hierarchy takes the selected tab unless it is pinned (`src/features/code-insight/hierarchy.ts:109-130`); closing the last tab closes the hierarchy (`hierarchy.ts:152-167`).
- [ ] Usages (`src/features/code-insight/UsagesView.tsx:64-116`): same menu as Find and Hierarchy; reuse unless pinned (`src/features/code-insight/usages.ts:138-157`).
- [ ] Services (`src/features/services/ServicesHost.tsx:28-42`): a tab per service view, shown only with two or more views; no close, no pin, no menu.

What is stored: nothing of the content tabs (Find, Hierarchy and Usages tabs live in memory; Hypothesis: no `saveJson` of tabs in those stores, measured only for `windowStore.ts` whose one `PINNED_KEY` is the Find popup's own pin, `src/features/search/find/store.ts:86-92`). The view modes are stored in the project's layout JSON under `viewModes`, only non-default values (`src/workbench/toolwindows/store.ts:225-231`, `265`, `333-339`, `399`) and copied into named layouts (`store.ts:757-805`); Restore Default Layout clears them (`store.ts:214`).

### View modes (`store.ts:52-57`, `217-231`, `715-735`; `actions.ts:209-213`, `282-302`; `ToolWindows.tsx:995-1130`, `1323-1327`)

- [ ] `dockPinned` (default, never stored), `dockUnpinned` (docked; hides when focus lands in the editor or another tool window), `undock` (shown over the editor without resizing it; hides the same way). Focus going to a popup, menu or dialog hides nothing (`store.ts:715-735`).
- [ ] The actions take the window from `arg` when it is a string, else the focused, else the last active window (`actions.ts:210-213`); `toggled` is true for the target's current mode (`actions.ts:291-295`).
- [ ] An undocked area leaves the grid track (`--tw-<anchor>: 0px`), lies over the editor at `--undock-size` (the side's stored size), and has an inner-edge separator "Resize undocked <anchor> tool window" that resizes it over the editor, clamped to `MIN[anchor]` (left 140, right 160, bottom 90) and to the grid minus `EDITOR_MIN` (200 × 60), arrow keys by 10 px (`ToolWindows.tsx:930-935`, `997-1063`). Two undocked areas of a side share its length by the split ratio (`ToolWindows.tsx:1094-1117`).

### The specs

- `content.test.ts`: "closes the selected tab of the first strip that can, else reports false"; "pins the selected tab of the first strip that can, else reports false".
- `header.test.tsx`: "middle click on the header hides the tool window"; "middle click on the header title hides the tool window"; "middle click on a header button leaves the tool window open"; "middle click on a header content tab closes the tab, not the window"; "header double click hides the editor and restores its previous size"; "View Mode › Undock lays the window over the editor and frees its track"; "a menu opened from the header returns focus to the window, not the editor"; "shows the header title in a language chosen while the window is open".
- `badges.test.tsx`: "hides stripe counts immediately and restores the current count when enabled".
- `e2e/toolWindowViewMode.spec.ts`: "Undock shows the Project window over the editor; focusing the editor hides it"; "a menu opened from an undocked window leaves it open"; "Dock Unpinned docks the window and hides it when another tool window takes focus".
- Not this slice's rows but the component's natural proofs (`src/ui/Tabs.test.tsx`, owned by F-UI-PRIMITIVES #93, `WORKPLAN.md:1330`): "Alt+click on a close button closes the other tabs", "middle click closes a tab without selecting it", "middle click ignores tabs that cannot close", "a hidden close button keeps the middle-click gesture", "lets a container close the selected tab when it is closable", "lets a container pin the selected tab when the strip supports pinning", "clicking the active tab brings its clipped close button back into view", "keeps the active tab revealed when the strip is resized narrower". Also from `store.test.ts` (F-WB-TOOLWINDOWS's row, `INVENTORY.md:4083`): "are remembered per window and project; Restore Default Layout docks and pins every window", "ignores unknown stored modes", "unpinned and undocked windows hide when focus moves to the editor or another tool window", "stay when focus goes to a popup or dialog; pinned windows never hide on focus", "toolwindow.viewMode.* set the mode of the window an Options menu names, else of the active one".

## (b) The native state at HEAD

- The header is drawn inline in `Workbench::render`'s `tool_window` closure (`app-gpui/src/workbench/mod.rs:4247-4313`): a row of `h-header`, padding 12/6, the raw `declared.title` (not passed through the text catalog: no `tx` on that chain, measured), a spacer, Options (`more:{id}`, left mouse down → `show_tool_window_options`) and Hide (`hide:{id}`). No content slot, no middle-click, no double-click handler (measured by reading the chain). The Terminal skips this header (`mod.rs:4259`). Q-VIS2 has not landed at HEAD (the header above is the HEAD header); its slot's shape is therefore unknown here.
- `Panel { view: AnyView, focus: Rc<dyn Fn(&App) -> FocusHandle>, _events }` (`mod.rs:225-232`); panels are kept by window id for the project's life, a hidden window keeps its view (`mod.rs:194-196`). Mounting per window: `mod.rs:2472-2530`.
- `ContentTabs<H>` exists in the layout model with `register`, `unregister`, `close_selected` (`app-gpui/src/workbench/tool_windows.rs:1127-1176`) and one test proving the first `content.test.ts` case (`tool_windows.rs:1767-1785`). It has no caller (measured: `git grep ContentTabs HEAD -- app-gpui/src` finds only `tool_windows.rs`; `INVENTORY.md:2116` says the same). There is no `toggle_pin_selected`.
- `toolwindow.closeContent` has no native handler (measured: no `handle("toolwindow.closeContent"` in `app-gpui/src`); the manifest has it (`app-gpui/assets/gen/actions.json:7789-7794`) and the menu spec lists it as "Close Active Tab" (`app-gpui/src/workbench/menu_spec.rs:756`); the diff window owns its key separately (`app-gpui/src/vcs_diff/dialog_view.rs:29`, `97`). `editor.pinTab` acts on editor tabs only (`mod.rs:1008-1010`).
- View modes: none. No `viewModes` field in `ToolWindowLayout` (`tool_windows.rs:452-476`), `LayoutSnapshot` (`tool_windows.rs:302-310`) or its sanitizer; the three `toolwindow.viewMode.*` actions are in the manifest as toggles (`actions.json:7834-7866`) with no handler (measured: no match for `viewMode` under `app-gpui/src/workbench`). The Options menu is Move To ›, Maximize / Restore Size, separator, Hide (`app-gpui/src/workbench/context_menus.rs:658-675`; commands `ToolWindowCommand` at `context_menus.rs:619-629`). `ToolWindowLayout::set_focused` only records focus (`tool_windows.rs:990-1000`); it is called from the window's focus tracking (`mod.rs:2322-2331`). `reset_placements` (`tool_windows.rs:765-776`) and `apply` (`tool_windows.rs:711-728`) know nothing of modes.
- Stripe badges: `Workbench::stripe_badge` reads `appearance.show_tool_window_badges` on every render, and only Run has a count (`mod.rs:2577-2589`). The badges spec has no native test (`INVENTORY.md:4078`).
- Menus: `MenuItem<C>` with `item`, `entry`, `submenu`, `action`, `action_with`, `disabled`, `icon`, `shortcut`, `labelled` (`app-gpui/src/vcs_ops/menu.rs:35-140`); `Workbench::show_menu` (`mod.rs:3509-3518`). Check rows: `check_row` in `context_menus.rs` (used at `context_menus.rs:646`, `687`).
- Native windows today:
  - Run: its own 28 px row under the header, inside the view (`app-gpui/src/run/view.rs:1286-1347`, drawn at `view.rs:1529`): icon, name, close button (`run-tab-{i}`, `run-tab-close-{i}`), click selects. No pin (no `pinned` on `RunSession`, `app-gpui/src/run/store.rs:80-99`), no menu, no middle click, no overflow. Closing: `RunStore::close_session_tab` (`store.rs:1304-1311`). Reuse: a start reuses the given session or the same configuration's idle session (`store.rs:1665-1678`, `1712-1714`), not the webview's "selected unpinned finished tab".
  - Debug: its own 28 px row (`app-gpui/src/debugger/window_view.rs:282-339`), italic when finished, close emits `DebugWindowEvent::CloseTab`. No pin (no `pinned` in `app-gpui/src/debugger/state.rs`), no menu, no tooltip, no middle click.
  - Usages: its own strip (`app-gpui/src/code_insight/usages_view.rs:1022-1077`, `h-compact-header`), pin icon shown on pinned tabs, close button even on pinned tabs, right-click menu `tab_menu` = Pin/Unpin, separator, Close Tab, Close Other Tabs (`usages_view.rs:483-494`): different order from the webview's and no Close All Tabs. Model: `UsagesTabs` with reuse unless pinned (`app-gpui/src/code_insight/usage_search.rs:354-441`).
  - Find: one query panel, no tabs (`app-gpui/src/find/view.rs:1-2`); "Open in Find Window" runs the query in that panel (`mod.rs:3022-3031`). A complete tab model `FindWindow` with pin and reuse exists (`app-gpui/src/find/window_store.rs:1-215`) and is not mounted (measured: no `FindWindow` outside that file).
  - Hierarchy: one tree, no tabs (`app-gpui/src/code_insight/hierarchy_view.rs:1-8`; no `pinned` in `hierarchy*.rs`).
  - Services: once the HTTP Client has been used, `ServiceTabs` draws its own `h-header` row of tabs (Services, HTTP Client) under the window's header, with the selected view's buttons (`app-gpui/src/http_client/service_tabs.rs:1-6`, `93-140`; wrapping at `app-gpui/src/workbench/http_client.rs:819-850`). This is what Q-VIS2 item 2 moves into the slot.
  - Database results: its own `render_tabs` (`app-gpui/src/database/results_view.rs:709`), F-DB-RESULT-TABS's.
- There is no shared tab strip under `app-gpui/src/ui/` (measured: its files are `code_preview`, `console_find`, `matching`, `menu_aim`, `pattern_index`, `popup_anchor`, `popup_menu`, `secret_field`, `text_area`, `text_field`, `tooltip`). Each strip above is hand-built.

## (c) Design

### Shape in one paragraph

The header stays the workbench's; each window that has content tabs hands the header, through the Q-VIS2 slot, an element built by one shared function, `content_tab_strip`, from a plain list of `ContentTab` values its own store already knows. The strip has no state of its own beyond a scroll handle and an "overflow menu open" flag that the owning view keeps in a `ContentTabStripState`. What a gesture on the strip means comes back to the owner as a `ContentTabEvent`; the owner changes its store. For the two actions that reach "the selected tab" from outside (`toolwindow.closeContent`, `editor.pinTab` in a tool window), each window registers a `StripHandle` in the existing `ContentTabs<H>` when it mounts. View modes are data of `ToolWindowLayout` and are stored with the layout.

Why this shape over the obvious alternative (a `ContentTabsView` entity owning the tabs, given to every window): the tabs' truth is already in five different stores (`RunStore.sessions`, `Debugger` sessions, `UsagesTabs`, `FindWindow`, the Services views), each with its own reuse and close rules (a run tab asks before terminating, a debug tab closes its owner, a hierarchy closes with its last tab). A component that owns tabs would duplicate those lists and need syncing on every store change; a stateless builder over the owner's list cannot drift, and is the webview's shape (the `Tabs` component is controlled: `items`, `activeId`, callbacks).

### `app-gpui/src/ui/content_tabs.rs` (new)

```rust
/// One tab as the owner describes it (the webview's `TabItem` subset tool windows use).
#[derive(Clone, Debug, PartialEq)]
pub struct ContentTab {
    pub id: SharedString,
    pub label: SharedString,
    pub icon: Option<SharedString>,
    /// `false`: no close button and no close gesture (Services' views).
    pub closable: bool,
    pub pinned: bool,
    pub italic: bool,
    pub dimmed: bool,
    pub tooltip: Option<SharedString>,
}

/// What the owner supports; a gesture it does not support is not offered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StripAbilities {
    pub close: bool,
    pub close_others: bool,
    pub pin: bool,
    pub context_menu: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ContentTabEvent {
    Select(SharedString),
    Close(SharedString),
    /// Alt+click on a close button.
    CloseOthers(SharedString),
    TogglePin(SharedString),
    ContextMenu { id: SharedString, position: Point<Pixels> },
}

/// Kept by the owning view: the strip's scroll position and its overflow menu.
#[derive(Default)]
pub struct ContentTabStripState {
    pub scroll: ScrollHandle,
    revealed: Option<SharedString>,
}

/// Whether tab `tab` can be closed by any gesture (button, middle click, Delete).
pub fn can_close(tab: &ContentTab, abilities: StripAbilities) -> bool {
    abilities.close && tab.closable && !(tab.pinned && abilities.pin)
}

/// `closeSelected` of the webview's strip handle: the id to close, if the selected tab can be.
pub fn closable_selected<'a>(tabs: &'a [ContentTab], active: Option<&str>, abilities: StripAbilities) -> Option<&'a str>;

/// `togglePinSelected`: the id to toggle, if the strip pins and a tab is selected.
pub fn pinnable_selected<'a>(tabs: &'a [ContentTab], active: Option<&str>, abilities: StripAbilities) -> Option<&'a str>;

/// The rows of the overflow menu: every tab, the selected one checked.
pub fn overflow_menu(tabs: &[ContentTab], active: Option<&str>) -> Vec<MenuItem<SharedString>>;

/// The content tab menu of Find, Hierarchy and Usages (IDEA's): Close Tab, Close All Tabs,
/// Close Other Tabs (disabled under two tabs), separator, Pin Tab / Unpin Tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TabMenuCommand { Close(SharedString), CloseAll, CloseOthers(SharedString), TogglePin(SharedString) }
pub fn tab_menu(tab: &ContentTab, count: usize, i18n: &I18n) -> Vec<MenuItem<TabMenuCommand>>;

/// The strip: `selector` prefixes every debug selector (`{selector}-tab-{i}`,
/// `{selector}-tab-close-{i}`, `{selector}-tab-unpin-{i}`, `{selector}-more`).
pub fn content_tab_strip<V: 'static>(
    selector: &'static str,
    tabs: &[ContentTab],
    active: Option<&str>,
    abilities: StripAbilities,
    state: &ContentTabStripState,
    on_event: impl Fn(&mut V, ContentTabEvent, &mut Window, &mut Context<V>) + Clone + 'static,
    cx: &mut Context<V>,
) -> Stateful<Div>;
```

Behaviour the function implements, each tied to (a): close button "Close Tab" on `can_close` tabs, Alt held → `CloseOthers` when `abilities.close_others`; pinned tab with `abilities.pin` → "Unpin Tab" button with icon `pin` in the close slot; middle mouse down on a `can_close` tab → `Close`, with `cx.stop_propagation()` so the header's middle-click does not hide the window (header.test case 4); right mouse down → `ContextMenu` with `stop_propagation`; click → `Select` and a reveal of that tab; the scroll area is `overflow_x_scroll().track_scroll(&state.scroll)`, vertical wheel delta converted to horizontal (Hypothesis: GPUI's horizontal scroll container does not do this itself; see (h)); overflow detected from the scroll handle after layout and shown as a chevron `{selector}-more` with tooltip "Show Hidden Tabs" whose menu is `overflow_menu`; heights and paddings from the `compact` variant's CSS (`src/ui/Tabs.module.css`, to be read by the writer for the numbers; Hypothesis: 28 px strip and the tokens `bg-tab-selected` / `bg-tab-hover` already used by Usages). Keyboard (←/→, Home/End, Delete/Backspace) acts when the strip's focus handle has the keyboard: one focus handle per strip, the selected tab drawn focused, not one handle per tab (the roving tabindex has no GPUI equivalent; Hypothesis: one handle per strip reproduces what a user sees).

Where it lives: `ui/` because F-UI-PRIMITIVES (#93) will grow it into the general `Tabs` primitive (editor variant, drag and drop, vertical); this slice writes only the compact horizontal subset.

### `app-gpui/src/workbench/tool_windows.rs` (changes)

```rust
/// What the header's tab strips are asked from outside.
pub struct StripHandle {
    pub close_selected: Rc<dyn Fn(&mut Window, &mut App) -> bool>,
    pub toggle_pin_selected: Rc<dyn Fn(&mut Window, &mut App) -> bool>,
}

impl<H> ContentTabs<H> {
    /// Pins or unpins the selected content tab of `window`: `toggle` is tried on its strips
    /// until one does. False when it has none that can be pinned.
    pub fn toggle_pin_selected(&self, window: &str, toggle: impl FnMut(&H) -> bool) -> bool;
    /// The handles of `window`'s strips, in registration order (to call them without borrowing
    /// the workbench while they run).
    pub fn handles(&self, window: &str) -> Vec<&H>;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewMode { #[default] DockPinned, DockUnpinned, Undock }
impl ViewMode {
    pub const ALL: [ViewMode; 3];
    pub fn id(self) -> &'static str;          // "dockPinned" | "dockUnpinned" | "undock"
    pub fn from_id(id: &str) -> Option<Self>;
    pub fn title(self) -> &'static str;       // "Dock Pinned" | "Dock Unpinned" | "Undock"
}

// ToolWindowLayout gains `view_modes: BTreeMap<String, ViewMode>` (only non-default entries).
impl ToolWindowLayout {
    pub fn view_mode_of(&self, id: &str) -> ViewMode;
    pub fn set_view_mode(&mut self, id: &str, mode: ViewMode) -> Effects;   // PERSIST, no-op when equal
    /// Focus moved to tool window `focused` (None: out of the tool windows); `editor`: into the
    /// editor area. Unpinned and undocked windows other than `focused` hide when focus lands in
    /// the editor or another tool window; a popup, menu or dialog hides nothing.
    pub fn focus_moved(&mut self, focused: Option<&str>, editor: bool) -> Effects;
    /// Whether the window shown in `slot` lies over the editor.
    pub fn is_undocked(&self, slot: Slot) -> bool;
}
```

`LayoutSnapshot` gains `#[serde(rename = "viewModes")] view_modes: BTreeMap<String, ViewMode>` sanitized like `store.ts:333-339` (keep only `dockUnpinned` / `undock` string values); `apply` takes it, `snapshot` gives it, `reset_placements` clears it ("Restore Default Layout docks and pins every window"). `LAYOUT_VERSION` stays 2 (the webview writes `viewModes` under v 2, `store.ts:265`).

Why `focus_moved` beside `set_focused` rather than inside it: `set_focused` runs on every focus change the window tracks, including focus into a popup, which must not hide; the caller (`mod.rs:2322`) knows whether focus went to the editor, a tool window or a popup and passes it. Hypothesis: the caller can tell a popup apart by the workbench's open popup (`close_popup` exists, `mod.rs:3530`); settle in (h).

Why the view modes are in `ToolWindowLayout` rather than a separate store: they are stored in the same JSON, copied by the same snapshot and cleared by the same reset; splitting would need a second key and a second sanitizer for one map.

### `app-gpui/src/workbench/context_menus.rs` (changes)

`ToolWindowCommand::ViewMode(ViewMode)`; `view_mode_menu(layout, id) -> MenuItem<ToolWindowCommand>` = submenu "View Mode" of three `check_row`s, current checked; `tool_window_options` becomes window's own rows (unchanged: the Project view's), separator, View Mode ›, Move To ›, Maximize / Restore Size, separator, Hide (`ToolWindows.tsx:695-711`); `run_tool_window_command` maps `ViewMode(mode)` to `layout.set_view_mode(id, mode)`. The rows are command rows, not `MenuItem::action_with("toolwindow.viewMode.x", id)`, because the existing Options menu is command rows and the action's `toggled` would have to be told the argument; the actions are still registered for the keymap and Find Action (below).

### `app-gpui/src/workbench/mod.rs` (changes)

- `Panel` gains `strip: Option<Rc<StripHandle>>` set by the mount functions that have tabs; `Content::Project` gains `content_tabs: ContentTabs<Rc<StripHandle>>`, registered when a panel with a strip is inserted (`mod.rs:2532-2534`). Panels live as long as the project (`mod.rs:194-196`), so nothing unregisters before a project switch, where the whole `Content::Project` goes.
- Handlers in `register_actions` (`mod.rs:600-1060`):

```rust
registry.handle("toolwindow.closeContent", |_, window, cx| { /* focused or last active id; clone the handles out of
    the workbench; call each `close_selected` until one returns true; none → layout.hide(id) */ });
// editor.pinTab: with a focused tool window, toggle_pin_selected on its strips and stop; else the editor tab as today.
for mode in ViewMode::ALL {
    registry.handle_toggle(&format!("toolwindow.viewMode.{}", mode.id()), /* toggled: target's mode == mode */,
        /* run: target = call.arg as str, else focused, else last active; layout.set_view_mode */);
}
```

The handles are cloned out before calling, because a handle updates its own view and may call back into the workbench (a Run close asks a confirmation through the workbench).

- Header (on top of the Q-VIS2 slot): `on_mouse_down(MouseButton::Middle)` → `layout.hide(id)` and `on_click` with `click_count == 2` → `layout.toggle_maximize(id)`, both on the header row; the Options, Hide and every tab stop propagation (they are the "button, a, tablist" exclusions of `ToolWindows.tsx:775`, `784`). The title goes through the text catalog at render time (header.test case 8). Hypothesis: the Q-VIS2 slot is an `Option<AnyElement>` or a view drawn in the row between title and spacer; whichever it is, the strip element goes there.
- Focus wiring at `mod.rs:2322-2331`: after `layout.set_focused`, call `layout.focus_moved(focused_panel, editor_focused)` when focus is not in a popup, and carry out its `Effects` like `with_layout` does.
- Undock rendering in the `side` closure (`mod.rs:4316-…`): an undocked slot is left out of the flex row that sizes the editor and drawn `absolute()` over the editor area's edge at the side's stored size, with an inner-edge drag handle `undock-edge:{anchor}` (debug selector) clamped as in (a); two undocked areas of one side share its length by the split ratio. Maximize hides the edge.

### Per window: how each adopts it

- Usages (`code_insight/usages_view.rs`): `tab_strip` (`usages_view.rs:1022-1077`) is replaced by `content_tab_strip("usages", …)` handed to the slot; `StripAbilities { close: true, close_others: false, pin: true, context_menu: true }`; `tab_menu` becomes `ui::content_tabs::tab_menu` (order Close, Close All, Close Others, separator, Pin; Close All closes every tab), the native order and missing Close All being a parity fix; a pinned tab now shows Unpin instead of close and is not closed by middle click. `UsagesTabs` (`usage_search.rs`) is unchanged.
- Debug (`debugger/window_view.rs`): `render_session_tabs` (`window_view.rs:282-339`) is replaced by the strip in the slot (italic when finished, icon `debug`, tooltip the status from `tab_statuses`, `window_view.rs:187`); abilities `close` only. Pin, the menu and reuse of unpinned finished tabs come with F-RUN-TABS (its rows `act:debug.pinTab`, `act:debug.closeTab`…), which needs `pinned` on the native session and I-ACTIONS-ENABLED's `enabled`.
- Run (`run/view.rs`): `render_tabs` (`view.rs:1286-1347`) is replaced by the strip in the slot; abilities `close` only; the close still goes through `RunStore::close_session_tab` and its confirmation. Pin, the menu (`run.*Tab` actions), the archive folder and "the window goes with its last tab" (`runTabs.ts:23-24`) come with F-RUN-TABS.
- Services (`http_client/service_tabs.rs`): Q-VIS2 moves its row into the slot; this slice redraws it with `content_tab_strip("service", …)`, abilities none (select only), and keeps the selected view's header buttons at the row's end. Shown only with two or more views, as the webview (`ServicesHost.tsx:31`), which natively is already the case (the tabs exist only once the HTTP Client is used).
- Find: nothing to adopt yet: the native window has no tabs. F-SEARCH-A (#27, which owns `tw:find`) mounts `FindWindow` (`find/window_store.rs`) and uses `content_tab_strip("find", …)` with `tab_menu`; the plan does not make F-SEARCH-A depend on this slice (see the report).
- Hierarchy: nothing to adopt yet: no tabs natively. F-HIERARCHY-USAGES (#11) adds the tab model (reuse unless pinned, closing the last closes the hierarchy) and uses the strip and `tab_menu`.
- Terminal and Database results keep their own strips in this slice (the Terminal draws a header per group, `mod.rs:4259`; Database results are F-DB-RESULT-TABS's).

## (d) Existing native files that change

- `app-gpui/src/ui/mod.rs`: `pub mod content_tabs;`.
- `app-gpui/src/workbench/tool_windows.rs`: `StripHandle`, `ContentTabs::{toggle_pin_selected, handles}`, `ViewMode`, `view_modes` in `ToolWindowLayout` and `LayoutSnapshot` (sanitize, snapshot, apply, reset), `view_mode_of`, `set_view_mode`, `focus_moved`, `is_undocked`; tests.
- `app-gpui/src/workbench/context_menus.rs`: `ToolWindowCommand::ViewMode`, `view_mode_menu`, the Options menu's order, `run_tool_window_command`.
- `app-gpui/src/workbench/mod.rs`: `Panel.strip`, `content_tabs` in `Content::Project`, the three handlers and `editor.pinTab`'s tool-window branch, header gestures and translated title, focus wiring for `focus_moved`, undock rendering and its edge.
- `app-gpui/src/workbench/code_insight.rs` (`mount_usages`, `code_insight.rs:726`): register the Usages strip handle and hand the strip to the slot.
- `app-gpui/src/code_insight/usages_view.rs`: strip and menu replaced.
- `app-gpui/src/workbench/debug.rs` (`mount_debug_window`, `debug.rs:504`) and `app-gpui/src/debugger/window_view.rs`: strip moved to the slot.
- `app-gpui/src/workbench/mod.rs` Run mount (`mod.rs:2472-2478`) and `app-gpui/src/run/view.rs`: strip moved to the slot.
- `app-gpui/src/http_client/service_tabs.rs`, `app-gpui/src/workbench/http_client.rs`: the row drawn with the shared strip.
- Text catalog: "Show Hidden Tabs", "Unpin Tab", "Close All Tabs", "View Mode", "Dock Pinned", "Dock Unpinned", "Undock" if the native catalog lacks them (Hypothesis: it reads `src/platform/locales/text/*.json`, which have them because the webview uses them).

## (e) Parts, in order (sizes are Hypothesis: hand estimates of code plus tests)

1. Model: `StripHandle`, `ContentTabs::toggle_pin_selected` / `handles`, `ViewMode` and `view_modes` in the layout (stored, sanitized, snapshot, reset), `focus_moved`, `is_undocked`. Pure, no UI; lands alone. About 350 lines.
2. Component: `ui/content_tabs.rs` with `can_close`, `closable_selected`, `pinnable_selected`, `overflow_menu`, `tab_menu`, `content_tab_strip`, and a small test view drawing it. Lands alone (nothing uses it yet). About 650 lines.
3. Header and actions: the slot consumed (needs Q-VIS2), middle-click and double-click on the header, translated title, `toolwindow.closeContent`, `editor.pinTab` in a tool window, the three View Mode actions and the Options menu's View Mode ›, Dock Unpinned through `focus_moved`, the badges test; Usages adopts the strip (the first real strip, so closeContent and pin have something to act on). About 600 lines.
4. Undock: the overlay rendering, the edge resize and its clamps, the two e2e cases and the header's Undock case. About 450 lines.
5. Adoption: Debug, Run, Services onto the strip in the slot. About 400 lines.

Total about 2,450 lines against the plan's 1,236 (`WORKPLAN.md:218`, `474`). Parts 1, 3 and 4 carry the View Mode rows, about 800 lines of it.

## (f) Tests per part

Every test names its spec case in a comment in the native style (`// src/…/x.test.ts › case`), as `tool_windows.rs:1767` does.

1. In `tool_windows.rs` tests: "pins the selected tab of the first strip that can, else reports false" (content.test, the missing case); from store.test (F-WB-TOOLWINDOWS's row, partial credit): "are remembered per window and project; Restore Default Layout docks and pins every window", "ignores unknown stored modes", "unpinned and undocked windows hide when focus moves to the editor or another tool window", "stay when focus goes to a popup or dialog; pinned windows never hide on focus". Failure paths: an unknown id leaves the layout unchanged; `set_view_mode` to the current mode returns no `PERSIST`; a stored `viewModes` that is not an object yields an empty map.
2. In `content_tabs.rs` tests (pure functions) and a window test drawing the strip in a test view: "lets a container close the selected tab when it is closable" and "lets a container pin the selected tab when the strip supports pinning" (pure), "middle click closes a tab without selecting it", "middle click ignores tabs that cannot close", "Alt+click on a close button closes the other tabs", "clicking the active tab brings its clipped close button back into view", "keeps the active tab revealed when the strip is resized narrower" (window, by `debug_bounds` of `{selector}-tab-close-{i}` within the strip's bounds); plus not from a spec: the overflow chevron appears only when the tabs exceed the strip and its menu checks the selected tab; a pinned tab shows `-tab-unpin-` and no `-tab-close-`.
3. In `workbench/tests/` (a new `proofs_tool_window_header.rs` beside `proofs_tool_windows.rs`, using `start`, `trusted_project`, `run_action`, `context_is_on`, `debug_bounds` and `simulate_event` as `proofs_tool_windows.rs:1-40` and `mod.rs:11752-11755` do): header.test "middle click on the header hides the tool window", "middle click on the header title hides the tool window", "middle click on a header button leaves the tool window open", "middle click on a header content tab closes the tab, not the window" (on the Usages strip, with a stand-in search), "header double click hides the editor and restores its previous size", "a menu opened from the header returns focus to the window, not the editor", "shows the header title in a language chosen while the window is open"; content.test end to end: `toolwindow.closeContent` with the Usages window focused closes its selected tab, and with no closable tab hides the window; `editor.pinTab` with Usages focused pins its selected tab and leaves the editor's tabs alone; store.test "toolwindow.viewMode.* set the mode of the window an Options menu names, else of the active one"; e2e "Dock Unpinned docks the window and hides it when another tool window takes focus" (by `run_action("toolwindow.problems")` or the Problems key); badges.test "hides stripe counts immediately and restores the current count when enabled" (Run's badge with a stand-in session, the setting toggled both ways, the count changed while hidden).
4. header.test "View Mode › Undock lays the window over the editor and frees its track" (editor area bounds unchanged width after Undock is the "freed track"; the window's bounds overlap the editor's); e2e "Undock shows the Project window over the editor; focusing the editor hides it" (including the edge drag of 100 px and Cmd+1 bringing it back focused) and "a menu opened from an undocked window leaves it open"; failure path: the edge drag stops at `EDITOR_MIN`.
5. One window test per adopted window: Debug (a stand-in adapter session: the strip is in the header row, i.e. its bounds lie within `h-header` of the header, and closing by middle click closes the session tab), Run (a stand-in runner: same; a live session's middle click asks first when `confirm_terminate`), Services (the HTTP Client tab in the header selects the view; no close button drawn).

## (g) What tests and the real window must never do

- No debug adapter, runner or language server but the stand-ins the existing proofs use (Debug and Run adoption tests); no network; nothing under the user's home (tests use `start(cx)`'s temp home, `proofs_tool_windows.rs:5`).
- Shots of the real window only through the integrator's script with a temp `HOME`, `OPENIDEA_GPUI_HOME` and a project under a neutral temp path (`int_visual2_brief.md:31`); never the Agents tool window; never the Services tab of Services unless it opens without being shown first (same brief, item 2's rule); no debug session in the real window (the Debug strip is proven by tests only).
- No test asserts pixels of the strip's colours beyond the tokens it names; no test sleeps for the overflow measurement (wait on `run_until_parked`).

## (h) Open questions only a compiler or the running app settles

1. The Q-VIS2 slot's type and where the header row ends (it has not landed at HEAD). Settle: read the header after Q-VIS2's commit; parts 3 to 5 start from it.
2. Whether GPUI's `ScrollHandle` on a horizontal `overflow_x_scroll` div reports a content width larger than its bounds after layout (for the overflow chevron) and whether a vertical wheel scrolls it sideways. Settle: in part 2's window test, draw ten tabs in a 200 px strip and read the handle's max offset; scroll with a vertical `ScrollWheelEvent` and read the offset.
3. Whether one focus handle per strip is enough for ←/→ and Delete to behave as the webview's roving focus, including Tab into the content. Settle: part 2's window test with key presses; look in the real window that the focus ring is on the selected tab.
4. How the focus tracking at `mod.rs:2322` tells "focus went into a popup or menu" from "focus went to the editor". Settle: compile part 3 against a test that opens the Options menu of a Dock Unpinned window and asserts it stays (store.test "stay when focus goes to a popup or dialog").
5. Whether an `absolute()` undocked area drawn inside the side's container paints over the editor and receives mouse events before it (z-order of later siblings). Settle: part 4's test clicks inside the overlay's bounds over the editor and asserts the window keeps the keyboard.
6. Whether a middle `MouseDownEvent` on a tab with `stop_propagation` keeps the header's middle handler from running when both are `on_mouse_down(Middle)` on nested elements (GPUI dispatches bubble phase innermost first; Hypothesis: yes). Settle: header.test case 4.
7. Whether the native text catalog already has "Show Hidden Tabs", "Dock Unpinned" and the other new strings. Settle: the text catalog test of the native app fails on a missing key.
