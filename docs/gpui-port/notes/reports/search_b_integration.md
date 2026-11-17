# Search (F-SEARCH-B): integration notes

Worktree `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`, HEAD `fc6572cb7`. Nothing here was compiled. Every anchor below was copied from `git show HEAD:<file>`.

New files (untracked):

- `app-gpui/src/everywhere/view/memory.rs`: `impl SearchEverywhere` for the scope (include non-project items, widen when nothing is found), the search history (⌥↑/⌥↓, start with the tab's last query, remember on close), recent actions, ⌥↵ (assign shortcut on an action), ⌘↵ on Text (Find in Files).
- `app-gpui/src/everywhere/view/parts.rs`: header (tabs plus the scope check box, wrapping in a narrow popup), the query field's hint, the footer describing the selected row, every result line (16 px icon slot, name with marked characters, an action's shortcut), the popup width rule.
- `app-gpui/src/everywhere/view/proofs41_stale.rs`: view tests (late answer of an old query is dropped; the width rule).
- `app-gpui/src/workbench/tests/proofs41_e2e_search_everywhere.rs`: end-to-end tests of the dialogs spec cases built here.
- `app-gpui/src/workbench/tests/proofs41_e2e_search_layout.rs`: the native versions of `e2e/searchEverywhereLayout.spec.ts` cases 1, 4 and 5.
- `app-gpui/src/workbench/tests/proofs41_recent_edited.rs`: Recent Files' Show edited only.

## (a) The list: row, clause, verdict

Verdicts: `already there` (native file:line, test), `build` (this slice), `needs` (what it waits for), `not native`. "BUILT" marks what the pieces below build; "build, not built" is small enough for this slice but was not written in this pass.

### `point.registerOverlay:search.everywhere` (missing clauses of its proof)

| Clause                                                      | Verdict                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| the preview pane                                            | build, not built: `ui/code_preview.rs` (`CodePreview`, used by Bookmarks and Find in Files) is the editor preview; the popup needs a vertical splitter with a ratio stored under `openidea.search.everywhere.layout` (webview `SearchEverywhere.tsx:629-648`) and the Preview toggle stored under `PREVIEW_KEY` (`everywhere/store.rs:16`, `set_preview` exists). Hypothesis: no reusable splitter with a stored ratio exists in `app-gpui/src/ui` (none listed there at HEAD); settle with `git grep -n "split_ratio\|Splitter" HEAD -- app-gpui/src`. |
| the funnel filters and the file-set scope                   | build, not built: models exist (`everywhere/filters.rs`, `SeRequest.filter`/`file_set`, `SearchEverywhereState::update_filters`); the view has no funnel popup (`FilterPopup.tsx`) nor the scope chooser of Classes/Files/Symbols/Text (`SearchEverywhere.tsx:542-553`).                                                                                                                                                                                                                                                                                |
| include non-project items and the retry everywhere          | BUILT (memory.rs `show_again`, `set_include_non_project`, `widen_when_nothing_found`, `note_query`; parts.rs `header` check box; "Nothing found" text). Backend: `find_files`/`find_symbols` take no such argument (`src-tauri/src/commands/nav.rs:27`, `:80`; webview `deps.ts`), so the flag only reaches the controller (`controller.rs:265`, symbol search).                                                                                                                                                                                        |
| the search history (⌥↑/⌥↓ and its menu)                     | ⌥↑/⌥↓, starting with the tab's last query, remembering on close: BUILT. The magnifier's Recent Searches menu (`SearchEverywhere.tsx:271-280`, `:589-599`): build, not built.                                                                                                                                                                                                                                                                                                                                                                            |
| several rows selected at once                               | build, not built: `find/row_selection.rs` has the model (used by `recent_view.rs:14`); the popup needs `RowSelection` over `SeRow` keys, Shift/primary clicks, opening in list order (`openInListOrder`). Six spec cases wait on it.                                                                                                                                                                                                                                                                                                                    |
| Open to the Side (⇧↵) and Open In Right Split               | needs: an open-in-the-other-pane operation; `TabCommand::OpenInOpposite => false` (`workbench/mod.rs`, `fn` with `TabCommand::SplitRight`) and Recent Files' `OpenFile { path, .. }` drops `side` (`workbench/mod.rs`, `show_recent_files`).                                                                                                                                                                                                                                                                                                            |
| ⌘↵ to Find in Files                                         | BUILT (memory.rs `find_in_files`, event `RunWith { id: "find.inPath", arg: {"query"} }`; `find/store.rs:135` reads `query`). The header's "Open in Find Tool Window" button: build, not built.                                                                                                                                                                                                                                                                                                                                                          |
| the ⌥↵ menu (copy path, Find in Directory, assign shortcut) | assign shortcut on an action row: BUILT (runs `settings.assignShortcut` with the id, which is not registered natively yet: needs F-SET-UPSTREAM-TOGGLES-B for the dialog itself). The menu for file/symbol/text/setting rows: build, not built.                                                                                                                                                                                                                                                                                                         |
| language server symbols and endpoints as sources            | needs: the view answers `SeFetch::WorkspaceSymbols` with `None` and `SeFetch::Routes` with `Failed` (`everywhere/view.rs:351-358`); a test needs the stand-in language server. Hypothesis: `ide_host::ops::lsp` has a workspace-symbol operation the adapter `lsp_workspace_symbols` calls; settle with `git grep -n "workspace_symbols" HEAD -- crates/ide-host/src src-tauri/src/commands`.                                                                                                                                                           |
| recent actions                                              | BUILT (memory.rs `recent_action_ids`, `remember_action`; `SeEnv.recent_actions`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| shortcuts shown beside actions                              | BUILT (parts.rs `item_line`, `Keymap::shortcut_label`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| highlighted match characters                                | BUILT (parts.rs `name`, through `recent_view::marked_text`, made `pub(crate)`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| the empty-result text                                       | already there (`everywhere/view.rs:633-650`); "Type to search" and "Nothing found in project files" shown by `workbench::tests::find_action_says_what_to_do_when_it_has_nothing_to_list`; "Nothing found" BUILT.                                                                                                                                                                                                                                                                                                                                        |
| refresh after the index finishes (spec case 2)              | build, not built: the popup subscribes to no backend event; it needs the `indexState` event (`SearchEverywhere.tsx:157-165`) to drop the Files signature and run again.                                                                                                                                                                                                                                                                                                                                                                                 |
| the toggle of a toggled action drawn in its row (`Toggle`)  | build, not built (`SearchEverywhere.tsx:822-838`); the state already shows through `reports_state_only_for_actions_with_toggled`.                                                                                                                                                                                                                                                                                                                                                                                                                       |
| syntax colours of text matches (`CodeText`)                 | build, not built (`SearchEverywhere.tsx:880-918`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| a folder result selects in the Project view                 | build, not built: native lists no folders (no `findDirs` source).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |

### `point.registerKeyClaim:(at) src/features/search/everywhere/SearchEverywhere.tsx:86`

| Clause                       | Verdict                                                                                 |
| ---------------------------- | --------------------------------------------------------------------------------------- |
| ⌥↵ claimed but does nothing  | BUILT for action rows (assign shortcut); other rows: build, not built (the menu above). |
| ⌘↵ claimed but does nothing  | BUILT on Text.                                                                          |
| ⌥↑/⌥↓ claimed but do nothing | BUILT (history).                                                                        |

### Actions `navigate.searchEverywhere`, `navigate.gotoFile`, `navigate.gotoSymbol`, `navigate.findAction`, `navigate.searchText`

Each: "opens Search Everywhere on its tab … the popup's own gaps are listed under the overlay". Added clause from the webview store (`store.ts:63-70`): asked for again on the shown tab it toggles include non-project items. BUILT (`show_again`, hunk M1). `navigate.findAction` on Actions toggles Include disabled actions (`SearchEverywhere.tsx:213`): BUILT (hunk V9).

### Actions `navigate.recentFiles`, `navigate.switcher`, overlay `point.registerOverlay:search.recent`

| Clause                                                            | Verdict                                                                                                                                                                                                                                                                                                                                       |
| ----------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Recent Locations                                                  | build, not built: the model exists (`everywhere/locations.rs`, `recent_model.rs:240-322`), nothing records places (webview `common/tracking.ts:89-108`: a 500 ms rest after caret moves and edits), no popup, `ShowRecentLocations` only closes (`workbench/mod.rs`, `RecentFilesEvent::Dismissed \| RecentFilesEvent::ShowRecentLocations`). |
| the files edited in this session (Show edited only shows nothing) | BUILT (hunks E1-E3, M3).                                                                                                                                                                                                                                                                                                                      |
| opening to the side                                               | needs: the other-pane open (above).                                                                                                                                                                                                                                                                                                           |
| version control colours on names                                  | build, not built. Hypothesis: the Project view's file colours have an accessor the popup can read; settle with `git grep -n "fn file_color\|status_color" HEAD -- app-gpui/src`.                                                                                                                                                              |
| keeping a row whose tab refused to close                          | needs: the close result of a tab with unsaved changes; the workbench calls `popup.removed(&key, cx)` right after `tabs.close_key(&key)` (`workbench/mod.rs`, `RecentFilesEvent::ClosePath`).                                                                                                                                                  |

### `navigate.recentLocations` (no `parity.json` row)

Opens Recent Locations (`src/features/search/index.ts:338`, ⇧⌘E). build, not built (the tracker and the popup, as above). The menu entry exists (`workbench/menu_spec.rs:367`).

### `point.registerOverlay:search.fileStructure`, `navigate.fileStructure`

| Clause                                                    | Verdict                                                                                                                         |
| --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| the last outline shown at once on reopening               | build, not built (`structure/store.ts` keeps it).                                                                               |
| reloading while open                                      | build, not built.                                                                                                               |
| tooltips                                                  | build, not built.                                                                                                               |
| the query in the monospace font                           | build, not built: one line (`field.set_code_font(true, cx)`, `ui/text_field.rs:176`) where `structure_view.rs` makes its field. |
| going to a member of a library class (a virtual document) | needs: virtual documents for library sources.                                                                                   |

### `point.registerToolWindow:structure`, `point.registerMenuGroup:search.structure.selectIn` (no `parity.json` row for the latter)

| Clause                                                     | Verdict                                                                                                                                                                                                                               |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| the line a preview shows is not followed                   | needs: the Structure window following a preview editor (`StructureView.tsx`); Hypothesis: depends on content tabs in tool windows (I-TW-CONTENT-TABS) only for the preview tabs.                                                      |
| Select In › Structure (`src/features/search/index.ts:202`) | build, not built: the Select In chooser (`project_view::actions`) lists fixed targets; a contributed target needs a registration point there (`navigate.selectIn` row says "contributed targets (selectIn menu groups)" are missing). |

### `point.registerToolWindow:bookmarks`

| Clause                                                          | Verdict                                                                                                  |
| --------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| a file removed on disk keeps its bookmark until the next change | build, not built: an `FsChanged` handler removing the bookmarks of removed paths (`bookmarks/store.ts`). |

### `point.registerMenuGroup:search.bookmarks`, `point.registerMenuGroup:search.bookmarks.files`

| Clause                                                                              | Verdict                                     |
| ----------------------------------------------------------------------------------- | ------------------------------------------- |
| greyed out without a project where the webview's row looks enabled and runs nothing | build, not built (the native row's `when`). |
| several Project view paths bookmarked at once                                       | needs: multi-selection in the Project view. |
| (point.registerOverlay:search.bookmarks is done)                                    | already there.                              |

### `feature:search`

Aggregates the rows above; moves with them.

### Spec files

See (c). `unit:src/features/search/__tests__/inherited.test.ts` case 3 (Python, from the IDE's inheritance graph): needs a native `pythonHierarchy`/`pythonSupertypeItems` (`src/editor/navigation/pythonHierarchy.ts`) over `ide_host::ops::nav::nav_inheritance` (`crates/ide-host/src/ops/nav.rs:213`), in the editor navigation slice. `fileStructure.test.tsx` case 3 (Vue script declarations nested under the script block): Hypothesis: the backend outline of a `.vue` file does not nest them; settle with a test calling `ide_host::ops` outline on a `.vue` fixture (`git grep -n "vue" HEAD -- crates/ide-syntax crates/ide-host/src/ops/syntax.rs`).

### Totals (clauses)

already there 2 (empty text; `point.registerOverlay:search.bookmarks`), BUILT 13 (include non-project and widen; history keys; start with the last query and remember on close; recent actions; shortcuts beside actions; highlighted characters; ⌘↵; ⌥↵ assign shortcut; the five actions' toggle on repeat counted once; Include disabled actions; edited files; "Nothing found"; footer description), build not built 21, needs 8 (other-pane open ×2, tab close result, LSP/endpoints sources, virtual documents, Structure following a preview, Project view multi-selection, Python hierarchy), not native 1 (the 125% browser zoom of e2e case 4).

## (b) Hunks for existing files

Apply piece by piece; each piece compiles on its own after the files of that piece are in place.

### Piece 1: Search Everywhere memory, scope, keys, rows (files `everywhere/view/memory.rs`, `everywhere/view/parts.rs`, `everywhere/view/proofs41_stale.rs`)

**R1** `app-gpui/src/everywhere/recent_view.rs`, anchor:

```rust
/// `text` with its matched characters on the search-match colour.
fn marked_text(cx: &App, text: &str, indices: &[usize]) -> Div {
```

becomes

```rust
/// `text` with its matched characters on the search-match colour.
pub(crate) fn marked_text(cx: &App, text: &str, indices: &[usize]) -> Div {
```

**V1** `app-gpui/src/everywhere/view.rs`, anchor:

```rust
use crate::platform::input;
use crate::platform::local_time::now_ms;
```

becomes

```rust
use crate::platform::input;
use crate::platform::keymap::{has_primary_modifier, is_mac_host};
use crate::platform::local_time::now_ms;
```

**V2** anchor:

```rust
use std::sync::Arc;
use std::time::Duration;
```

append after it:

```rust

mod memory;
mod parts;
#[cfg(test)]
mod proofs41_stale;

pub use parts::scope_label;
```

**V3** anchor:

```rust
    /// Open Settings on page `0`.
    OpenSettings(String),
}
```

becomes

```rust
    /// Open Settings on page `0`.
    OpenSettings(String),
    /// Run action `id` with `arg` once the popup has closed: ⌥↵ records a shortcut, ⌘↵ finds in files.
    RunWith { id: String, arg: serde_json::Value },
}
```

**V4** anchor:

```rust
    /// The claim on the keys the popup handles itself, ahead of the keymap.
    key_claim: u64,
    _field_events: Subscription,
}
```

becomes

```rust
    /// The claim on the keys the popup handles itself, ahead of the keymap.
    key_claim: u64,
    /// The scope, the walk through the search history and what is remembered when the popup closes.
    memory: memory::SeMemory,
    _field_events: Subscription,
}
```

**V5** anchor:

```rust
    ) -> Self {
        let placeholder = cx.default_global::<Localization>().0.tx("Type to search");
```

becomes

```rust
    ) -> Self {
        // Without a selection to search for, the tab's last query comes back.
        let remembered = query.map_or_else(|| memory::last_query(tab, cx), str::to_owned);
        let query = Some(remembered.as_str()).filter(|query| !query.is_empty());
        let placeholder = cx.default_global::<Localization>().0.tx("Type to search");
```

**V6** anchor:

```rust
        cx.on_release(|popup, cx| {
            input::release_keys(popup.key_claim, cx);
```

becomes

```rust
        cx.on_release(|popup, cx| {
            input::release_keys(popup.key_claim, cx);
            popup.remember_query(cx);
```

**V7** anchor:

```rust
            key_claim,
            _field_events: events,
        };
```

becomes

```rust
            key_claim,
            memory: memory::SeMemory::new(tab, query.unwrap_or_default()),
            _field_events: events,
        };
```

**V8** anchor (in `with_env`):

```rust
        let is_enabled = move |meta: &ActionMeta| is_action_enabled(meta, &context);
        let env = SeEnv {
            actions: &actions,
            is_enabled: &is_enabled,
            recent_actions: &[],
```

becomes

```rust
        let is_enabled = move |meta: &ActionMeta| is_action_enabled(meta, &context);
        let recent = memory::recent_action_ids(cx);
        let env = SeEnv {
            actions: &actions,
            is_enabled: &is_enabled,
            recent_actions: &recent,
```

**V9** anchor:

```rust
    fn query_changed(&mut self, cx: &mut Context<Self>) {
        let typed = self.field.read(cx).text().to_owned();
        if let Some(ParsedCommand::Run { tab, rest }) = parse_command(&typed) {
            self.field.update(cx, |field, cx| field.set_text(&rest, cx));
            self.tab = tab;
        }
        self.reset_selection();
```

becomes

```rust
    fn query_changed(&mut self, cx: &mut Context<Self>) {
        self.memory.history = None;
        self.requery(cx);
    }

    /// Searches for the field's text once it has rested: after every change, and when ⌥↑ or ⌥↓ put an
    /// earlier query in the field.
    fn requery(&mut self, cx: &mut Context<Self>) {
        let typed = self.field.read(cx).text().to_owned();
        if let Some(ParsedCommand::Run { tab, rest }) = parse_command(&typed) {
            self.field.update(cx, |field, cx| field.set_text(&rest, cx));
            self.tab = tab;
            self.memory.state.select_tab(tab);
        }
        self.note_query(cx);
        self.reset_selection();
```

**V10** anchor (in `run`):

```rust
        let mut request = SeRequest::new(self.search_query(), self.tab);
        request.extra = self.extra.clone();
```

becomes

```rust
        let mut request = SeRequest::new(self.search_query(), self.tab);
        request.extra = self.extra.clone();
        request.include_non_project = self.memory.state.include_non_project;
        // The Actions tab's check box is Include disabled actions.
        request.include_disabled = (self.tab == SeTab::Actions).then_some(self.memory.state.include_non_project);
```

**V11** anchor (in `refresh`):

```rust
        if self.pending_enter && self.settled(cx) {
            self.pending_enter = false;
            self.activate(cx);
        }
        cx.notify();
```

becomes

```rust
        if self.pending_enter && self.settled(cx) {
            self.pending_enter = false;
            self.activate(cx);
        }
        self.widen_when_nothing_found(cx);
        cx.notify();
```

**V12** anchor (in `select_tab`):

```rust
        if self.tab != tab {
            self.tab = tab;
            self.extra.clear();
```

becomes

```rust
        if self.tab != tab {
            self.tab = tab;
            self.memory.state.select_tab(tab);
            self.extra.clear();
```

**V13** anchor (in `activate`):

```rust
                (self.tab, self.debounce) = (tab, None);
```

becomes

```rust
                (self.tab, self.debounce) = (tab, None);
                self.memory.state.select_tab(tab);
                self.memory.history = None;
```

**V14** anchor (in `activate`):

```rust
                SeItem::Action(action) => SearchEverywhereEvent::RunAction(action.id),
```

becomes

```rust
                SeItem::Action(action) => {
                    memory::remember_action(&action.id, cx);
                    SearchEverywhereEvent::RunAction(action.id)
                }
```

**V15** anchor (in `claim_key`):

```rust
            "enter" if !modifiers.alt && !modifiers.platform => self.enter(cx),
            "enter" => {}
            "up" | "down" if modifiers.alt => {}
```

becomes

```rust
            "enter" if modifiers.alt => self.assign_shortcut(cx),
            "enter" if has_primary_modifier(&modifiers, is_mac_host()) => {
                if self.tab == SeTab::Text {
                    self.find_in_files(cx);
                }
            }
            "enter" => self.enter(cx),
            "up" | "down" if modifiers.alt => self.browse_history(key == "up", cx),
```

**V16** anchor:

```rust
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
```

becomes

```rust
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
```

**V17** anchor (the tabs):

```rust
                div()
                    .id(tab.id())
                    .h(theme::length(cx, "h-tab"))
```

becomes

```rust
                div()
                    .id(tab.id())
                    .debug_selector(move || format!("search-everywhere-tab-{}", tab.id()))
                    .h(theme::length(cx, "h-tab"))
```

**V18** anchor (the result lines), the whole arm:

```rust
                            SeRow::Item { item, .. } => match item {
                                SeItem::File(file) => line
                                    .child(theme::icon(cx, theme::icons::file_icon_name(&file.name), 16.0, fg))
                                    .child(SharedString::from(file.name.clone()))
                                    .child(detail(file.dir.clone())),
                                SeItem::Symbol(symbol) => line
                                    .child(SharedString::from(symbol.display.clone()))
                                    .children(symbol.container.clone().map(detail))
                                    .child(detail(symbol.rel.clone())),
                                SeItem::Action(action) => line
                                    .text_color(if action.enabled { fg } else { muted })
                                    .children(action.icon.as_deref().map(|icon| theme::icon(cx, icon, 16.0, fg)))
                                    .child(SharedString::from(action.title.clone()))
                                    .children(action.category.clone().map(detail)),
                                SeItem::Text(text) => line
                                    .child(SharedString::from(text.text.trim().to_owned()))
                                    .child(detail(format!("{}:{}", text.rel, text.line + 1))),
                                SeItem::Setting(setting) => line
                                    .child(SharedString::from(setting.title.clone()))
                                    .child(detail(setting.path.join(" | "))),
                            },
```

becomes

```rust
                            SeRow::Item { item, .. } => parts::item_line(line, index, item, cx),
```

**V19** anchor (the empty text):

```rust
            let text = if empty_query { "Type to search" } else { "Nothing found in project files" };
```

becomes

```rust
            let text = match (empty_query, self.memory.state.include_non_project) {
                (true, _) => "Type to search",
                (false, true) => "Nothing found",
                (false, false) => "Nothing found in project files",
            };
```

**V20** anchor (the popup frame, to the end of `render`):

```rust
        div()
            .w(px(680.0))
            .h(px(440.0))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .rounded(px(10.0))
            .overflow_hidden()
            .text_size(theme::length(cx, "fs-ui"))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(12.0))
                    .py(px(6.0))
                    .child(theme::icon(cx, "search", 16.0, muted))
                    .child(div().flex_1().min_w_0().child(self.field.clone())),
            )
            .child(tabs)
            .child(div().flex_1().min_h_0().mt(px(4.0)).child(body))
    }
```

becomes

```rust
        // 760 px wide, less in a window that leaves under 24 px on each side; at most 680 px the tabs take a
        // line of their own and the hint goes.
        let viewport = window.viewport_size();
        let width = parts::popup_width(f32::from(viewport.width));
        let narrow = width <= parts::NARROW_WIDTH;
        let hint = if narrow { None } else { parts::hint(self.tab, cx) };
        let header = self.header(tabs, narrow, cx);
        let footer = self.footer(cx);
        let faint = theme::color(cx, "fg-faint");
        let query = div()
            .debug_selector(|| "search-everywhere-query".to_owned())
            .h(px(32.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .pl(px(8.0))
            .pr(px(10.0))
            .bg(theme::color(cx, "bg-input"))
            .border_2()
            .border_color(theme::color(cx, "border"))
            .rounded(theme::length(cx, "radius-small"))
            .child(theme::icon(cx, "search", 16.0, theme::color(cx, "fg-subtle")))
            .child(div().flex_1().min_w_0().child(self.field.clone()))
            .children(hint.map(|hint| {
                div().flex_none().whitespace_nowrap().text_color(faint).child(SharedString::from(hint))
            }));
        div()
            .debug_selector(|| "search-everywhere".to_owned())
            .w(px(width))
            .h(px(440.0))
            .max_h(viewport.height * 0.8)
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .rounded(px(10.0))
            .overflow_hidden()
            .text_size(theme::length(cx, "fs-ui"))
            .child(header)
            .child(div().flex_none().px(px(8.0)).py(px(6.0)).child(query))
            .child(
                div()
                    .debug_selector(|| "search-everywhere-results".to_owned())
                    .flex_1()
                    .min_h_0()
                    .pb(px(8.0))
                    .child(body),
            )
            .children(footer)
    }
```

After V18 the render closure's `muted` stays used by the section and command rows and the tabs; if clippy reports `muted` unused anywhere, drop it there.

**M1** `app-gpui/src/workbench/mod.rs`, anchor (in `show_search_everywhere`):

```rust
        if let Some(open) = self.search_everywhere() {
            open.update(cx, |popup, cx| popup.select_tab(tab, cx));
            return;
        }
```

becomes

```rust
        if let Some(open) = self.search_everywhere() {
            open.update(cx, |popup, cx| popup.show_again(tab, cx));
            return;
        }
```

**M2** anchor (in `open_search_everywhere`):

```rust
                SearchEverywhereEvent::OpenSettings(page) => {
                    workbench.close_popup(window, cx);
```

insert before it:

```rust
                SearchEverywhereEvent::RunWith { id, arg } => {
                    workbench.close_popup(window, cx);
                    let (id, arg) = (id.clone(), arg.clone());
                    // Once this update is over: the action may open a dialog over the workbench.
                    window.defer(cx, move |window, cx| {
                        execute_action_from(ActionSource::Search, &id, Some(&arg), window, cx);
                    });
                }
```

### Piece 2: files edited in the window (Recent Files › Show edited only)

**E1** `app-gpui/src/workbench/editor_core.rs`, anchor:

```rust
use serde_json::Value;
use std::path::PathBuf;
```

becomes

```rust
use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;
```

**E2** anchor:

```rust
pub(super) struct EditorCore {
    nav: NavigationHistory,
    edits: EditHistory,
}
```

becomes

```rust
pub(super) struct EditorCore {
    nav: NavigationHistory,
    edits: EditHistory,
    /// The files the user changed in this window (typing, deleting, pasting, undo and redo; not the
    /// backend's own changes): what Recent Files' Show edited only lists.
    pub(super) edited: HashSet<String>,
}
```

**E3** anchor:

```rust
            EditorEvent::Edited { origin, .. } => {
                if !matches!(origin, EditOrigin::External | EditOrigin::History) {
```

becomes

```rust
            EditorEvent::Edited { origin, .. } => {
                if !matches!(origin, EditOrigin::External) {
                    let path = editor.read(cx).path().to_string_lossy().into_owned();
                    workbench.editor_core.edited.insert(path);
                }
                if !matches!(origin, EditOrigin::External | EditOrigin::History) {
```

**M3** `app-gpui/src/workbench/mod.rs`, anchor (in `show_recent_files`):

```rust
            tool_windows: self.tool_windows().map(|layout| layout.windows().cloned().collect()).unwrap_or_default(),
            ..RecentFilesInput::default()
```

becomes

```rust
            tool_windows: self.tool_windows().map(|layout| layout.windows().cloned().collect()).unwrap_or_default(),
            edited: self.editor_core.edited.clone(),
            ..RecentFilesInput::default()
```

### Piece 3: the test modules

**M4** `app-gpui/src/workbench/mod.rs`, anchor:

```rust
    mod proofs40_run_start;
```

becomes

```rust
    mod proofs40_run_start;
    mod proofs41_e2e_search_everywhere;
    mod proofs41_e2e_search_layout;
    mod proofs41_recent_edited;
```

(`proofs41_e2e_search_everywhere` and `proofs41_e2e_search_layout` need piece 1; `proofs41_recent_edited` needs piece 2.)

No `pub mod` line is needed: `memory`, `parts` and `proofs41_stale` are declared by V2 inside `everywhere/view.rs`, which then is the folder module `everywhere/view/` next to `view.rs` (Rust 2018 layout, as `structure_view.rs` with `structure_view/tests/`).

## (c) State per row after the pieces, and the spec cases

| Row                                                                                                  | State after                                                                                                                                      | Still missing                                                                                                                                                                                                                                 |
| ---------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `point.registerOverlay:search.everywhere`                                                            | partial                                                                                                                                          | preview, funnel and file-set scope, Recent Searches menu, multi-selection, other-pane open, ⌥↵ menu of non-action rows, header Find tool window button, LSP symbols and endpoints, refresh on index end, row toggles, syntax colours, folders |
| `point.registerKeyClaim:(at) …SearchEverywhere.tsx:86`                                               | done for the keys (⌥↵ on actions, ⌘↵ on Text, ⌥↑/⌥↓ all act); Hypothesis: the reviewer may keep it partial until ⌥↵ opens the menu on other rows | ⌥↵ menu on non-action rows                                                                                                                                                                                                                    |
| `action:navigate.searchEverywhere`, `gotoFile`, `gotoSymbol`, `findAction`, `searchText`             | partial (their own behaviour, repeat toggles the scope, is done; they point at the overlay's gaps)                                               | overlay gaps                                                                                                                                                                                                                                  |
| `action:navigate.recentFiles`, `action:navigate.switcher`, `point.registerOverlay:search.recent`     | partial                                                                                                                                          | Recent Locations, other-pane open, VCS colours, keeping a refused row                                                                                                                                                                         |
| `action:navigate.recentLocations`                                                                    | not started                                                                                                                                      | tracker and popup                                                                                                                                                                                                                             |
| `action:navigate.fileStructure`, `point.registerOverlay:search.fileStructure`                        | unchanged partial                                                                                                                                | as listed                                                                                                                                                                                                                                     |
| `point.registerToolWindow:structure`, `point.registerMenuGroup:search.structure.selectIn`            | unchanged                                                                                                                                        | as listed                                                                                                                                                                                                                                     |
| `point.registerToolWindow:bookmarks`, `point.registerMenuGroup:search.bookmarks`, `…bookmarks.files` | unchanged                                                                                                                                        | as listed                                                                                                                                                                                                                                     |
| `feature:search`                                                                                     | partial                                                                                                                                          | the above                                                                                                                                                                                                                                     |
| `unit:…/searchKeys.test.ts`                                                                          | done (unchanged)                                                                                                                                 |                                                                                                                                                                                                                                               |
| `unit:…/inherited.test.ts`                                                                           | partial 2/3                                                                                                                                      | Python inheritance graph                                                                                                                                                                                                                      |
| `unit:…/fileStructure.test.tsx`                                                                      | partial 4/5                                                                                                                                      | Vue case                                                                                                                                                                                                                                      |
| `unit:…/dialogs.test.tsx`                                                                            | partial: 9 of 23 in full after the pieces (5 before), 2 in part                                                                                  | see below                                                                                                                                                                                                                                     |
| `e2e:e2e/searchEverywhereLayout.spec.ts`                                                             | partial: 5 of 9 runs (cases 1 ×3, 4 ×2 at normal scale, 5)                                                                                       | cases 2 and 3 need the preview; 125% scale                                                                                                                                                                                                    |

`src/features/search/__tests__/dialogs.test.tsx` at HEAD (23 cases, the `it.each` rows counted one by one):

| #     | Case                                                                                         | Native test                                                                                                                                                                                                          |
| ----- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1     | shows recent files for an empty query, then flat results; Enter runs an action               | `workbench::tests::proofs_editor_search_everywhere::shows_recent_files_for_an_empty_query_then_flat_results` (Enter runs an action: `workbench::tests::search_everywhere_runs_actions_finds_text_and_switches_tabs`) |
| 2     | refreshes the current file query after index invalidation finishes                           | MISSING (build, not built)                                                                                                                                                                                           |
| 3     | the preview toggle is remembered                                                             | MISSING (preview)                                                                                                                                                                                                    |
| 4     | / lists scope commands; picking one or typing '/files q' switches the tab                    | `workbench::tests::search_everywhere_runs_actions_finds_text_and_switches_tabs`                                                                                                                                      |
| 5     | ⌥↑ / ⌥↓ browse the tab's search history and restore the draft                                | `workbench::tests::proofs41_e2e_search_everywhere::alt_up_and_alt_down_browse_the_tab_s_search_history_and_restore_the_draft`                                                                                        |
| 6     | ⇥ cycles scopes and repeating the scope action toggles non-project items                     | `workbench::tests::proofs41_e2e_search_everywhere::tab_cycles_scopes_and_repeating_the_scope_action_toggles_non_project_items`                                                                                       |
| 7     | labels the Actions tab's checkbox and assigns a shortcut with ⌥↵                             | `workbench::tests::proofs41_e2e_search_everywhere::labels_the_actions_tab_s_checkbox_and_assigns_a_shortcut_with_alt_enter`                                                                                          |
| 8     | filters Files by file type from the funnel, remembering the choice                           | MISSING (funnel)                                                                                                                                                                                                     |
| 9     | searches everywhere when nothing is found in the project                                     | `workbench::tests::proofs41_e2e_search_everywhere::searches_everywhere_when_nothing_is_found_in_the_project`                                                                                                         |
| 10    | extends, shrinks, crosses the anchor and clamps Shift selection; plain arrows select one     | MISSING (multi-selection)                                                                                                                                                                                            |
| 11    | Shift-click selects a range without opening; an ordinary click opens just that file          | MISSING (multi-selection)                                                                                                                                                                                            |
| 12-14 | uses the primary modifier to toggle a row on macos / windows / linux                         | MISSING (multi-selection)                                                                                                                                                                                            |
| 15    | awaits each open in list order, keeps tabs and restores an upward range's lead               | MISSING (multi-selection)                                                                                                                                                                                            |
| 16    | resets selection immediately on query changes and tab switches                               | MISSING (multi-selection)                                                                                                                                                                                            |
| 17-18 | opens selected classes / symbols at their positions                                          | MISSING (multi-selection)                                                                                                                                                                                            |
| 19    | ignores actions and settings in a mixed selection even when the lead is an action            | MISSING (multi-selection)                                                                                                                                                                                            |
| 20    | does not execute a multi-selection containing only actions                                   | MISSING (multi-selection)                                                                                                                                                                                            |
| 21    | Recent Files: extends and shrinks a keyboard range, then opens its files in order            | `workbench::tests::proofs_editor_search_recent::extends_and_shrinks_a_keyboard_range_then_opens_its_files_in_order`                                                                                                  |
| 22    | Recent Files: starts a range at the initially selected previous file                         | `workbench::tests::proofs_editor_search_recent::starts_a_range_at_the_initially_selected_previous_file`                                                                                                              |
| 23    | Recent Files: handles modifier clicks and resets on filter text changes                      | `workbench::tests::proofs_editor_search_recent::handles_modifier_clicks_and_resets_on_filter_text_changes`                                                                                                           |
| 24    | Switcher: opens the selection when Ctrl is released, once the popup closed, in the same task | partial: `workbench::tests::recent_files_and_the_switcher_go_between_the_editors` (not the focus order)                                                                                                              |
| 25    | Find in Files: streams results and highlights the matches while replacing, as IDEA           | `workbench::tests::proofs_editor_search_find::streams_results_and_highlights_the_matches_while_replacing` (partial, per its row: Replace All's enabled state)                                                        |
| 26-28 | Find in Files: spells its shortcuts the macos / linux / windows way (`it.each` at line 541)  | MISSING: no native test of the Find in Files popup's tooltip shortcuts per OS (it belongs to F-SEARCH-A, Find in Files)                                                                                              |

Note on the count: the table has 28 lines because the three `it.each` at lines 293, 358 and 541 expand to 3, 2 and 3 rows; counted once each, the file has 20 `it`s plus 3 `it.each` = 23, the brief's number. `parity.json` says 22: it predates the `:541` table.

`src/features/search/__tests__/fileStructure.test.tsx` (5): 1 `everywhere::structure_view::tests::the_arrows_walk_fold_and_unfold_the_tree` + `…::shows_the_outline_as_a_tree_with_the_symbol_at_the_caret_selected`; 2 `everywhere::structure_view::tests::proofs8_file_structure::narrows_down_on_typing_keeping_ancestors_or_only_highlights_when_that_is_off`; 3 MISSING (Vue); 4 `everywhere::structure_view::tests::typing_narrows_the_tree_and_escape_clears_the_filter_before_it_dismisses`; 5 `everywhere::structure_window::tests::offers_show_inherited_members_only_when_the_server_knows_type_hierarchies`.

`src/features/search/__tests__/inherited.test.ts` (3): 1 `everywhere::inherited::tests::collects_supertype_members_level_by_level_without_overridden_ones_or_constructors`; 2 `everywhere::inherited::tests::finds_nested_types_and_appends_inherited_members_after_a_types_own_ones`; 3 MISSING (Python graph).

`src/features/search/__tests__/searchKeys.test.ts` (1): `find::search_keys::tests::keeps_alt_enter_and_the_navigation_keys_from_the_editors_actions_behind_the_popup`.

`e2e/searchEverywhereLayout.spec.ts` (5 tests, 9 runs): 1 (×3: 1, 2, 24 results) `workbench::tests::proofs41_e2e_search_layout::without_the_preview_the_results_list_shows_its_rows` (one loop over the three counts); 2 preview splitter: MISSING (preview); 3 keyboard selection stays in view with the preview: MISSING (preview; the in-view half without the preview is what `select` + `scroll_to_item` do, untested); 4 (×2: en, zh) `workbench::tests::proofs41_e2e_search_layout::tabs_and_query_fit_narrow_windows` (normal scale only); 5 `workbench::tests::proofs41_e2e_search_layout::mixed_file_and_action_rows_align_icons_and_names_and_keep_shortcuts_readable`.

The numbers come from the CSS: popup 760 (`Dialog width={760}`, `SearchEverywhere.tsx:506`), 24 px backdrop padding (`src/ui/Dialog.module.css:9`), header 44 (`SearchEverywhere.module.css .header`), wrap at 680 (`@container (max-width: 680px)`), field 32 high with a 2 px border (`.field`), footer 30 (`.footer`), row 24 (`ROW`, `SearchEverywhere.tsx:88`), icon 16 (`.icon`). They are constants in `parts.rs` (the neighbours keep theirs as constants: `recent_view.rs:36-38`).

Popup tests per the hard rule "a late answer of an old query is not shown": Search Everywhere: `everywhere::view::proofs41_stale::a_late_answer_of_an_old_query_is_not_shown`. Recent Files asks once per popup (nothing supersedes it). File Structure: Hypothesis: it loads once per popup too (`structure_view.rs` `loads_the_symbols_of_a_file_from_the_backend`); settle by reading whether `FileStructure` reloads on edits.

## (d) Hypotheses, each with how to settle it

1. (Read, not a hypothesis) `TextField::set_text` tells the owner nothing (`ui/text_field.rs:130`: "The owner is not told"), so `browse_history` calls `requery` itself and the walk is not reset.
2. (Read) `ActionMeta`'s fields are all `pub` (`platform/actions.rs:24-66`), so `ActionMeta { hidden: true, ..ActionMeta::new(…) }` compiles.
3. `Context::defer` is `App::defer` through `Deref` (as `agents/session_view.rs:1029` uses it).
4. `div().id(("search-everywhere-name", index))` builds an `ElementId` from `(&str, usize)` (as `editor/paste_history_view.rs:293`).
5. Symbol, action and setting `indices` are byte offsets like the file ones (`model.rs:322-335`), as `marked_text` expects. Settle: read `match_actions` and `symbol_item` in `everywhere/model.rs`.
6. The paths of `RecentFiles` items and `editor.path()` are the same strings (both canonical), so `edited` matches. Settle: `proofs41_recent_edited` passing.
7. A workbench does not switch projects in place; if it does, `editor_core.edited` should be cleared there (webview `resetTracking`, `tracking.ts:63`). Settle: `git grep -n "fn open_project" HEAD -- app-gpui/src/workbench/mod.rs` and read whether it reuses `editor_core`.
8. The tests run on macOS: `cmd-enter` is the primary modifier there (`has_primary_modifier`).
9. The file index finds `FindAction.kt` for "Find Action" (a space in the query). If not, change the query of `mixed_file_and_action_rows_align_icons_and_names_and_keep_shortcuts_readable` to "FindAction" and check that the action still matches.
10. `Localization::0.set_locale(Some("zh"), "en")` switches the catalog in place and the next frame draws Chinese tabs.
11. `on_release` runs while the popup's `field` entity is still alive, so `remember_query` can read it.

## (e) Existing tests whose expectations change

- `workbench::tests::find_action_says_what_to_do_when_it_has_nothing_to_list` (in `workbench/mod.rs`): after "zzqqxx" the popup widens and says "Nothing found", not "Nothing found in project files" (webview `SearchEverywhere.tsx:441-452`, `:496`). Change its second `settle` to `"search-everywhere-empty:Nothing found"`. "Nothing found in project files" is then only seen before the widening; the native test of that text would be a tab where the user turned the scope off.
- Any test that lists a disabled action on the Actions tab with the check box off: disabled actions are left out there now (`SearchEverywhere.tsx:213`, `controller.rs` `include_disabled.unwrap_or(true)`). Hypothesis: none does; settle with `git grep -n "findAction" HEAD -- app-gpui/src/workbench` and reading each.
- Tests that open the same tab twice in one test now start with the last query, selected (`store.ts:72`). Typing replaces it, so `type_query` after opening still searches the typed text; a test that asserts an empty field or `lines()` before typing on a reopened tab changes. Seen at HEAD: `go_to_file_finds_a_file_by_name_and_opens_it` reopens Files and types (unchanged result).

## (f) Tests most likely to fail first

- `tabs_and_query_fit_narrow_windows`: `simulate_resize` on a window whose workbench centres the popup; the header's wrap and the Chinese labels' widths are estimates (6 tabs at 26 px high, 9 px padding); a tab could overflow the 472 px header at 520 px. Also the footer exists only with rows: after six Tab presses the tab is Files again, so `OrderService` rows come back, but Actions and Text pass through empty states in between.
- `mixed_file_and_action_rows_align_icons_and_names_and_keep_shortcuts_readable`: hypothesis 9; and the All tab may cut the file below the fold.
- `searches_everywhere_when_nothing_is_found_in_the_project`: the deferred `run` after widening; `settle` covers it, but if `settled()` stays false (the Symbols section keeps `loading` while a workspace-symbol answer is awaited), no widening happens. `SeFetch::WorkspaceSymbols` is answered at once with `None` (`view.rs:353`), so it should settle.
- `actions_run_from_the_popup_are_recent_and_the_footer_describes_the_selection`: `backspace` on a selected field deletes the selection (assumed); the Recent Actions header title is `"Recent Actions"` (`model.rs:116`).
- `alt_up_and_alt_down_…`: hypothesis 1.
- `a_late_answer_of_an_old_query_is_not_shown`: without a `Backend` global `carry_out` returns early, which is what the test wants; if `SearchEverywhere::new` needs `Keymap` (it does not at HEAD), add `input::init`.

## (g) What a look at the real window must check

- The header is above the query field now (tabs left, the check box right), 44 px, the frame colour; the query field is a 32 px box with a 2 px border and the hint on its right ("Type / to see commands" on All, "⌘↵ to open in Find tool window" on Text).
- In a window narrower than 728 px the tabs take a line of their own, the check box goes under them and the hint disappears; the popup never leaves the window.
- Action rows: the icon column is 16 px even without an icon, names line up with file names, the shortcut follows the name in the keys font, the category is on the right; disabled actions are faint.
- Matched characters are on the search-match colour in file, symbol, action, text and setting names.
- The footer (30 px) says the selected row's path, or for an action "Category | Title · ⌥↵ to assign a shortcut".
- ⌥↑/⌥↓ walk earlier queries; reopening a tab brings its last query back selected.
- Double Shift on All twice ticks "Include non-project items"; on Actions the box reads "Include disabled actions" and disabled actions show only when it is ticked.
