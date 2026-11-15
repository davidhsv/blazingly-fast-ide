# Terminal: split groups, tab close, file drops and paste — integration notes

Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`. Nothing was compiled. Every new file passes `rustfmt --edition 2024 --check` (run from the worktree root, so `rustfmt.toml` with `max_width = 120` applies).

## New files (undeclared)

| File                                                           | Lines | What                                                                                                                                                                                                                                    |
| -------------------------------------------------------------- | ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/terminal/panel/groups.rs`                        | 583   | child of `terminal::panel`: per-group header + tab strip, the window's Options/Hide in the top-right group, draggable divider, tab menu and More menu items, `headers()` accessor, `TerminalTabCommand`, unit test of the divider clamp |
| `app-gpui/src/terminal/view/clipboard.rs`                      | 167   | child of `terminal::view`: ⌘V carries out `terminal::paste::paste_event_plan` (text, copied files, stored media with the still-same-session check), file drops through `terminal::drop::terminal_drop_text`                             |
| `app-gpui/src/workbench/terminal_groups.rs`                    | 150   | child of `workbench`: handles every `TerminalPanelEvent` (Options menu, Hide, tab menu, More menu, close tabs with the Terminate/Cancel question, paste notifications)                                                                  |
| `app-gpui/src/terminal/panel/tests/proofs23_groups.rs`         | 219   | child of `terminal::panel::tests`                                                                                                                                                                                                       |
| `app-gpui/src/terminal/view/tests/proofs23_paste.rs`           | 164   | child of `terminal::view::tests`                                                                                                                                                                                                        |
| `app-gpui/src/workbench/tests/proofs23_e2e_terminal_groups.rs` | 233   | child of `workbench::tests`                                                                                                                                                                                                             |

## `mod` lines

- `app-gpui/src/terminal/panel.rs`: `pub mod groups;` (hunk P1). `pub` because `workbench::terminal_groups` names `groups::TerminalTabCommand` and the panel tests name `groups::GroupHeader`.
- `app-gpui/src/terminal/panel.rs`, inside `mod tests`: `mod proofs23_groups;` (hunk P7).
- `app-gpui/src/terminal/view.rs`: `mod clipboard;` (hunk V1).
- `app-gpui/src/terminal/view.rs`, inside `mod tests`: `mod proofs23_paste;` (hunk V6).
- `app-gpui/src/workbench/mod.rs`: `mod terminal_groups;` (hunk W1).
- `app-gpui/src/workbench/mod.rs`, inside `mod tests`: `mod proofs23_e2e_terminal_groups;` (hunk W4).

## Hunks, in the order to apply them

Line numbers are at HEAD of this worktree; anchors are quoted exactly.

### P1 — `app-gpui/src/terminal/panel.rs` lines 7–22: imports and the child module

Replace

```rust
use super::store::{
    GroupLayout, SessionCommand, SessionSnapshot, SessionStatus, SplitOrientation, TerminalEffect, Terminals,
    start_directory,
};
use super::view::{TerminalEvent, TerminalView};
use crate::platform::command_line::parse_shell_command;
use crate::platform::settings;
use crate::platform::storage::UiStorage;
use crate::theme;
use gpui_kit::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render, SharedString,
    Subscription, Window, div, prelude::*, px,
};
use ide_terminal::TerminalOptions;
use std::collections::HashMap;
use std::path::PathBuf;
```

with

```rust
use super::store::{SessionCommand, SessionSnapshot, SessionStatus, TerminalEffect, Terminals, start_directory};
use super::view::{TerminalEvent, TerminalView};
use crate::platform::command_line::parse_shell_command;
use crate::platform::settings;
use crate::platform::storage::UiStorage;
use crate::theme;
use crate::workbench::notifications::NotificationRequest;
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Pixels, Point, Render, Subscription,
    Window, div, prelude::*,
};
use ide_terminal::TerminalOptions;
use std::collections::HashMap;
use std::path::PathBuf;

pub mod groups;
```

(`AnyElement`, `SharedString`, `px`, `GroupLayout`, `SplitOrientation` were used only by `layout_element` and the old `render`, both replaced below.)

### P2 — `panel.rs` lines 24–29: the panel's events

Replace

```rust
/// What the terminal tool window tells its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalPanelEvent {
    /// The last tab closed: the tool window has nothing to show.
    Emptied,
}
```

with

```rust
/// What the terminal tool window tells its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalPanelEvent {
    /// The last tab closed: the tool window has nothing to show.
    Emptied,
    /// The header's Options button was pressed here: the window's Options menu.
    ShowOptions(Point<Pixels>),
    /// The header's Hide button.
    Hide,
    /// A right click on tab `tab`: its menu.
    TabMenu { tab: String, position: Point<Pixels> },
    /// A group's More Terminal Options button was pressed here; its group is the active one now.
    MoreMenu(Point<Pixels>),
    /// Close these tabs as their Close Tab button does (its ✕, the tab menu, ⌘W in its terminal),
    /// asking first whether to terminate a command that runs.
    CloseTabs(Vec<String>),
    /// A paste failed: the notification that says why.
    Notify(NotificationRequest),
}
```

### P3 — `panel.rs` lines 38–41 and 53–54: the divider state

In the struct, replace

```rust
    /// The panel has just started with a tab opened for the occasion, which no one has claimed.
    fresh: bool,
}
```

with

```rust
    /// The panel has just started with a tab opened for the occasion, which no one has claimed.
    fresh: bool,
    /// The sizes the groups were dragged to, and the drag going on.
    splits: groups::Splits,
}
```

and in `new`, replace

```rust
            views: HashMap::new(),
            fresh: false,
        };
```

with

```rust
            views: HashMap::new(),
            fresh: false,
            splits: groups::Splits::default(),
        };
```

### P4 — `panel.rs` lines 324–329: ⌘W and failed pastes go to the window

Replace

```rust
            TerminalEvent::CloseRequested => {
                if let Some(id) = self.terminals.tab_of_session(key).map(|tab| tab.id.clone()) {
                    self.terminals.close_tab(&id);
                }
            }
            TerminalEvent::Bell | TerminalEvent::FindRequested => {}
```

with

```rust
            // The window closes it as the tab's Close Tab button does, asking about a running command.
            TerminalEvent::CloseRequested => {
                if let Some(id) = self.terminals.tab_of_session(key).map(|tab| tab.id.clone()) {
                    cx.emit(TerminalPanelEvent::CloseTabs(vec![id]));
                }
            }
            TerminalEvent::PasteFailed(notification) => cx.emit(TerminalPanelEvent::Notify(notification.clone())),
            TerminalEvent::Bell | TerminalEvent::FindRequested => {}
```

### P5 — `panel.rs` lines 253–279: delete `layout_element`

Delete the whole function starting at

```rust
    /// The tab groups as the layout splits them, side by side or stacked, each showing its
    /// selected tab's terminal, with a one-pixel line between them.
    fn layout_element(&self, layout: &GroupLayout, cx: &App) -> AnyElement {
```

through its closing `    }` (the line before `    /// What the terminal of session \`key\` reports about itself.`). `groups::render_groups`replaces it; the`terminal-group:<group>`selector it drew is kept by`group_column`.

### P6 — `panel.rs` lines 343–423: the render

Replace the whole `impl Render for TerminalPanel { … }` block (from `impl Render for TerminalPanel {` through the `}` before `#[cfg(test)]`) with

```rust
impl Render for TerminalPanel {
    /// Each tab group with its own header and tab strip; see [`groups`].
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .track_focus(&self.focus)
            .text_size(theme::length(cx, "fs-ui"))
            .child(self.render_groups(cx))
    }
}
```

### P7 — `panel.rs` line 431: the test module

After

```rust
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::time::Duration;
```

(inside `mod tests`, lines 430–431) add

```rust

    mod proofs23_groups;
```

### V1 — `app-gpui/src/terminal/view.rs` lines 8 and 30: imports and the child module

Replace line 8

```rust
use super::encode::{KeyModes, KeyMods, encode_key, encode_paste};
```

with

```rust
use super::encode::{KeyModes, KeyMods, encode_key};
```

and after line 30 `use std::sync::Arc;` add

```rust

mod clipboard;
```

### V2 — `view.rs` lines 40–44: the paste failure event

Replace

```rust
    /// The user asked to close this terminal (⌘W).
    CloseRequested,
    /// The user asked to search its output (⌘F).
    FindRequested,
}
```

with

```rust
    /// The user asked to close this terminal (⌘W).
    CloseRequested,
    /// The user asked to search its output (⌘F).
    FindRequested,
    /// A paste could not be done: the notification that says why.
    PasteFailed(crate::workbench::notifications::NotificationRequest),
}
```

### V3 — `view.rs` lines 72–74 and 207–209: pastes being stored

Replace

```rust
    /// The claim on this terminal's keys, ahead of the keymap.
    key_claim: Option<u64>,
    _output: Task<()>,
```

with

```rust
    /// The claim on this terminal's keys, ahead of the keymap.
    key_claim: Option<u64>,
    /// Pasted media being stored on the backend.
    storing: usize,
    _output: Task<()>,
```

and replace

```rust
            key_claim: Some(key_claim),
            _output: output,
```

with

```rust
            key_claim: Some(key_claim),
            storing: 0,
            _output: output,
```

### V4 — `view.rs` lines 322–330: ⌘V carries out the paste plan

Replace

```rust
    /// Types the clipboard's text into the program.
    pub fn paste(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()).filter(|text| !text.is_empty()) else {
            return false;
        };
        let pasted = encode_paste(&text, self.emulator.bracketed_paste());
        self.write(&pasted, cx);
        true
    }
```

with

```rust
    /// Pastes the clipboard into the program: its text, the quoted paths of copied files, or the path
    /// of the file a copied picture is stored in.
    pub fn paste(&mut self, cx: &mut Context<Self>) -> bool {
        self.paste_clipboard(cx)
    }
```

### V5 — `view.rs` lines 571–574: files dropped on the terminal

Replace

```rust
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
```

with

```rust
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_drop(cx.listener(Self::files_dropped))
```

### V6 — `view.rs` line 781: the test module

Replace

```rust
    mod proofs14_ime;
```

with

```rust
    mod proofs14_ime;
    mod proofs23_paste;
```

### W1 — `app-gpui/src/workbench/mod.rs` line 74: the child module

Replace

```rust
mod terminal_actions;
mod todo_window;
```

with

```rust
mod terminal_actions;
mod terminal_groups;
mod todo_window;
```

### W2 — `mod.rs` lines 2290–2299: every panel event goes to `terminal_panel_event`

Replace

```rust
                    use crate::terminal::panel::{TerminalPanel, TerminalPanelEvent};
                    let (label, root) = (self.label.clone(), root.clone());
                    let terminals = cx.new(|cx| TerminalPanel::new(&label, root, cx));
                    // The last tab closing leaves nothing to show.
                    let closes =
                        cx.subscribe_in(&terminals, window, |workbench, _, event: &TerminalPanelEvent, window, cx| {
                            let TerminalPanelEvent::Emptied = event;
                            workbench.with_layout(window, cx, |layout| layout.hide(TERMINAL_TOOL_WINDOW));
                        });
                    Panel::new(terminals, Some(closes))
```

with

```rust
                    use crate::terminal::panel::{TerminalPanel, TerminalPanelEvent};
                    let (label, root) = (self.label.clone(), root.clone());
                    let terminals = cx.new(|cx| TerminalPanel::new(&label, root, cx));
                    let events =
                        cx.subscribe_in(&terminals, window, |workbench, _, event: &TerminalPanelEvent, window, cx| {
                            workbench.terminal_panel_event(event, window, cx);
                        });
                    Panel::new(terminals, Some(events))
```

(The old `let TerminalPanelEvent::Emptied = event;` is irrefutable only while the enum has one variant: without this hunk P2 does not compile.)

### W3 — `mod.rs` lines 4096–4100 and 4152–4154: no window header over the Terminal

The Terminal draws a header per group itself (title, tabs, Options `more:terminal`, Hide `hide:terminal`). First replace

```rust
                            .flex_col()
                            .child(
                                div()
                                    .h(theme::length(cx, "h-header"))
```

with

```rust
                            .flex_col()
                            // The Terminal draws a header for each of its tab groups.
                            .when(id != TERMINAL_TOOL_WINDOW, |el| el.child(
                                div()
                                    .h(theme::length(cx, "h-header"))
```

then replace

```rust
                                    ),
                            )
                            .child(div().flex_1().min_h_0().child(panel.view.clone())),
```

with

```rust
                                    ),
                            ))
                            .child(div().flex_1().min_h_0().child(panel.view.clone())),
```

Run `cargo fmt` afterwards (the block re-indents).

### W4 — `mod.rs` line 4364: the e2e tests

Replace

```rust
    mod proofs21_shell;
```

with

```rust
    mod proofs21_shell;
    mod proofs23_e2e_terminal_groups;
```

### T1 — `app-gpui/src/workbench/terminal_actions.rs` lines 8–20: imports

Replace

```rust
use super::services_window::on_workbench;
use super::{TERMINAL_TOOL_WINDOW, Workbench};
use crate::backend::Backend;
use crate::platform::actions::ActionRegistry;
use crate::platform::i18n::Localization;
use crate::platform::settings;
use crate::terminal::panel::TerminalPanel;
use crate::terminal::store::{
    SessionCommand, SessionStatus, SplitOrientation, Terminals, start_directory, terminate_confirmation,
};
use crate::vcs::prompt_dialogs::PromptDialog;
use crate::vcs_ops::dialogs::ConfirmRequest;
use gpui_kit::{Context, Entity, Window};
```

with

```rust
use super::services_window::on_workbench;
use super::{TERMINAL_TOOL_WINDOW, Workbench};
use crate::platform::actions::ActionRegistry;
use crate::platform::settings;
use crate::terminal::panel::TerminalPanel;
use crate::terminal::store::{SessionCommand, SplitOrientation, Terminals, start_directory};
use gpui_kit::{Context, Entity, Window};
```

### T2 — `terminal_actions.rs` lines 189–252: Close Terminal Session takes the one close path

Replace the three functions `close_terminal_session`, `end_terminal_tab` and `close_terminal_tab` (from `    /// Close Terminal Session: the active tab closes, after Terminate / Cancel when its session is` through the `    }` that ends `close_terminal_tab`, the line before the `}` closing `impl Workbench`) with

```rust
    /// Close Terminal Session: the active tab closes as its Close Tab button closes it, after
    /// Terminate / Cancel when its session runs a command.
    fn close_terminal_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.terminal_panel() else { return };
        let Some(tab) = panel.read(cx).terminals().active_tab().map(str::to_owned) else { return };
        self.request_close_terminal_tabs(vec![tab], window, cx);
    }
```

### T3 — `terminal_actions.rs` lines 257–265: the tests keep their imports

After

```rust
    use super::super::plugins_window::test_support::trusted_project;
    use super::super::tests::{click_on, run_action, start, wait_long};
    use super::*;
```

add

```rust
    use crate::backend::Backend;
    use crate::vcs::prompt_dialogs::PromptDialog;
```

(`close_project`, `prompt_shows` and `proofs15_terminal.rs` use them through `super::*`.)

## Menu entries

None in `menu_spec.rs`. The menus are built by the panel (`TerminalPanel::tab_menu`, `TerminalPanel::more_menu`) and shown by `Workbench::show_menu` in `workbench/terminal_groups.rs`.

## Parity rows: expected state and proving tests

Test paths after the `mod` lines: `terminal::panel::tests::proofs23_groups::…`, `terminal::view::tests::proofs23_paste::…`, `workbench::tests::proofs23_e2e_terminal_groups::…`, `terminal::panel::groups::tests::…`.

| Key                                        | Expected               | Proving tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | What stays missing                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| ------------------------------------------ | ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:terminal.splitRight`               | done                   | `workbench::tests::proofs23_e2e_terminal_groups::split_right_gives_the_terminal_a_tab_group_with_its_own_header_unsplit_merges_it_back` (run from the tab menu row, through the registry, asserts two strips, `Local (2)` selected in the second, two headers, Options in the second only), `terminal::panel::tests::proofs23_groups::dragging_the_line_between_two_groups_resizes_them_down_to_sixty_pixels`, plus the existing tests                                                                                         | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `action:terminal.splitDown`                | done                   | existing `the_splits_lay_their_shells_side_by_side_or_stacked_and_unsplit_joins_them` (stacked) plus the per-group header and divider code shared with Split Right (`group_column`, `split_element` handle both orientations)                                                                                                                                                                                                                                                                                                  | the vertical divider drag has no test of its own (`dragged` is orientation-free; hypothesis: add a vertical drag to the panel test if a reviewer wants it proven)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `action:terminal.splitAndMoveDown`         | done                   | existing tests plus the same per-group header code                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `action:terminal.unsplit`                  | done                   | `…split_right_gives_the_terminal_a_tab_group_with_its_own_header_unsplit_merges_it_back` (Unsplit from the tab menu: one strip `Local, Local (2)`, one header with title and buttons)                                                                                                                                                                                                                                                                                                                                          | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `point.registerToolWindow:terminal`        | done                   | the e2e tests above, `…proofs23_groups::each_group_has_a_header_whose_tabs_and_new_tab_button_act_on_that_group`                                                                                                                                                                                                                                                                                                                                                                                                               | the header's middle click (hide) and double click (maximize, another slice)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `feature:terminal`                         | partial                | all tests here                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | rename popup, tab drag and drop, find bar, pane context menu, New Predefined Session list in the More menu, links, mouse reporting, plugin sessions, command titles                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `action:terminal.closeSession`             | done                   | `workbench::tests::proofs23_e2e_terminal_groups::the_close_tab_button_and_cmd_w_ask_before_terminating_a_running_command` (✕ and ⌘W ask `Process 'Local' Is Running`; Cancel keeps it and its command, Terminate closes, last tab hides the Terminal), `…closing_the_last_tab_of_a_terminal_group_focuses_its_neighbour`, `…close_other_tabs_asks_about_a_running_program_and_stops_at_cancel`, `terminal::panel::tests::proofs23_groups::cmd_w_in_a_terminal_asks_the_window_to_close_its_tab`, plus the existing close tests | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `unit:src/features/terminal/drop.test.ts`  | done (5/5)             | `terminal::drop::tests` (4) and `terminal::panel::tests::proofs23_groups::pastes_only_into_the_terminal_under_the_cursor_without_executing`                                                                                                                                                                                                                                                                                                                                                                                    | the "detached and hidden sessions" case: natively a hidden tab's terminal is not drawn, so it has no hitbox and takes no drop; only the shown terminal under the pointer gets it, which the split test proves                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `unit:src/features/terminal/paste.test.ts` | done (24/24, see note) | `terminal::paste::tests` (19), `terminal::view::tests::proofs23_paste::keeps_existing_copied_files_on_their_original_quoted_paths_during_paste`, `…stores_raw_local_clipboard_media_through_paste`, `…ignores_an_image_after_its_target_is_replaced_during_upload`, `…rejects_oversized_images_before_reading_or_uploading`                                                                                                                                                                                                    | Note: the clipboard is read synchronously natively (`cx.read_from_clipboard()`), so "after the terminal is disposed" during the clipboard read, "target changes while reading", "replaced during file recovery" and "replaced during read" collapse into the only asynchronous step, the upload, which `ignores_an_image_after_its_target_is_replaced_during_upload` covers via `media_stored` with a stale target; "disposed during upload" is the dropped view: `view.update` in `store_media` fails and nothing is pasted (not driven by a test). "passes recovered multiline paths through xterm bracketed paste" is `encode_paste` (`terminal::encode::tests`); natively a local target always gets space-joined quoted paths. |
| `e2e:e2e/toolwindows.spec.ts`              | partial (9/12)         | adds `…split_right_gives_the_terminal_a_tab_group_with_its_own_header_unsplit_merges_it_back` and `…closing_the_last_tab_of_a_terminal_group_focuses_its_neighbour`                                                                                                                                                                                                                                                                                                                                                            | the 3 maximize-by-double-click cases (another slice)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |

IPC commands the paths stand for: `term_is_busy` (`ide_host::ops::term::is_busy`, as `src-tauri/src/commands/term.rs:59`), `term_store_image` (`ide_host::ops::term::store_image`, as `src-tauri/src/commands/term.rs:29-36`), `term_write` (`ide_host::ops::term::write`, through `TerminalView::write`).

## Webview behaviour each piece follows

- Per-group header: `src/features/terminal/TerminalView.tsx:209-215` (a `ToolWindowHeader` per group while split, `title={corners.title === group}`, `actions={corners.actions === group}`), corners at `:297-300` (`layoutGroups(layout)[0]`, `topRightGroup(layout)`); unsplit the tabs sit in the window header (`index.ts:64` `headerContent: … TerminalTabs`). Native draws the header in the panel in both cases, so the unsplit look (title, tabs, buttons on one row) is the same and the workbench header is skipped for the Terminal (W3).
- Header metrics: `src/workbench/toolwindows/ToolWindows.module.css:358-416` (`--h-header`, gap 6, padding `0 6px 0 8px`, `.headerTabbed` border-bottom `--border`, title padding `0 4px`, content `margin-left: 6px`, actions gap 2); Options and Hide: `ToolWindows.tsx:662-679` (`more` and `minus`, size small = 20px `Button.module.css:108-112`).
- Tab strip: `TerminalView.tsx:345-484` (tabs, ✕ "Close Tab", italic and `--fg-subtle` for exited sessions `:358-369`, tooltip `:59-64`, `+` New Tab after `activateGroup` `:453-459`, chevron More Terminal Options `:460-474` with items `:404-415`), tab metrics `src/ui/Tabs.module.css:29-110` (28px, radius-tab, selected `--bg-tab-selected` with `--border-tab-selected`, hover `--bg-toolbar-hover`, close slot 20px + 4px, icon 11px shown on hover/selected).
- Tab click selects in its group: `store.ts:279-285`; press in a pane activates its tab: `store.ts:93-96` (session focus → `selectTab`).
- Tab menu: `TerminalView.tsx:377-401` (Rename Session…, Close Tab, Close Other Tabs disabled with one tab, Split Right, Split and Move Right only with others, Split Down, Split and Move Down only with others, Unsplit only while split).
- Unsplit: `store.ts:403-421`; close of a group's last tab hands the active tab to the neighbour: `store.ts:212-236`, and `TerminalView.tsx:371-375` (hide when none left, else focus).
- Divider: `TerminalView.tsx:149-166` (`SplitHandle` between `SplitPanel minSize={60}`), `src/ui/Splitter.module.css` (1px `--border`, 7px grab area, `--border-strong` hover/active, col/row-resize cursor); react-resizable-panels numbers are pixels (`node_modules/react-resizable-panels/dist/react-resizable-panels.d.ts:456`).
- Close question: `store.ts:249-265` (`confirmEndSessions`, `requestCloseTab`), busy rule `session.ts:624-633`; ⌘W: `keys.ts:44` → `executeAction("terminal.closeSession")` → `index.ts:203-218`. Close Other Tabs stops at the first refusal: `TerminalView.tsx:385-392`.
- Drop: `drop.ts:20-29` (only the host under the pointer, `paste(droppedPathsText(paths))`, no Enter; `session.ts:322` routes it through `this.paste` = xterm's bracketed paste).
- Paste: `paste.ts:78-121` (paste event plan, `pasteMedia` `:65-76` with `target.current()` checks, `pasteError` `:37-44` only for a current target), target `session.ts:334-350` (`shell: this.snapshot.title`).

## Behaviours not built, and why

- Rename Session… (tab menu row and double click): needs a rename popup anchored under the tab; no native popup exists for it. The row is left out of `tab_menu`.
- Tab drag and drop (reorder, move to another group, drop on an edge to split): the store has `move_tab`, `move_tab_to_split`, `group_drop_index`, but no view drags tabs yet.
- More Terminal Options: the "New Predefined Session" header and the shell list (`termAvailableShells`) are not in `more_menu`; `MenuItem` has no header row kind, and the native app has no shell listing yet.
- The pane context menu (Copy, Paste, Select All, Clear Buffer, Split, Close Session, Restart Session) and the find bar.
- Header: middle click hides, double click maximizes, actions shown only on hover/focus (`opacity: 0` in CSS); icon buttons change colour on hover (fg-muted → fg-strong). Native buttons stay visible and keep their colour.
- The divider's hover colour relies on `group_hover` with the group set on its own grab area (hypothesis below); the hover state of the line while dragging uses `--border-strong`.
- Remote windows: `files_dropped` passes `remote_window: false` and the paste target is local (`remote: false`, `supported: true`). Hypothesis: the native app has no remote window yet; check with `grep -rn "RemoteWindow" app-gpui/src --include=*.rs | grep -v platform_more/remote.rs` (only `features_b/remote_dev/container.rs` uses it, as data).

## Assumptions to check first

1. Hypothesis: `deferred(grab)` makes the divider's 7px grab area the top hitbox over both neighbouring terminals, so a press there starts the drag and not a text selection. Check: the panel drag test; if presses go to the terminal, drop `deferred` and keep `occlude()` with the grab painted after the next pane.
2. Hypothesis: `group_hover(name, …)` on the line inside the grab that carries `.group(name)` colours it on hover. Visual only; no test depends on it.
3. Hypothesis: the drop listener on `TerminalView`'s root fires for `FileDropEvent::Entered` + `Submit` sent with `simulate_event` (GPUI turns them into a drag of `ExternalPaths` and a mouse up, `window.rs:5932-5966`; drop listeners fire on a hovered hitbox and stop propagation, `elements/div.rs:2964-2990`).
4. Hypothesis: `cx.simulate_keystrokes("cmd-w")` and `"cmd-v"` reach `TerminalView::claim_key` on Linux and Windows too (`keys.rs:73-80` maps ⌘ letters on every platform; GPUI's `cmd` is the platform key).
5. `TerminalPanel::headers`, `tab_menu`, `more_menu` are public accessors added for tests and the workbench; `groups::Splits` is `pub` with private fields so the private `splits` field of the public `TerminalPanel` does not leak a private type.
6. The undeclared `app-gpui/src/workbench/tests/proofs18_e2e_toolwindows_pending.rs` holds two tests with the same e2e names (lines 71–113) written before these views existed; it expects `crate::terminal::panel::TabCommand` and a `terminal-header:1` selector, which do not exist. These new tests supersede them; delete those two tests (or leave the file undeclared).

## Tests most likely to fail on first run

- `dragging_the_line_between_two_groups_resizes_them_down_to_sixty_pixels`: depends on assumption 1 and on the `canvas` measure running before the press (it runs at every paint). Widths are compared with ±1.5px for rounding of the 1px line.
- `pastes_only_into_the_terminal_under_the_cursor_without_executing`: assumption 3; also the left pane must be wide enough for `printf '<%s>\n' '/a b' /plain` on one row (about 30 columns).
- `stores_raw_local_clipboard_media_through_paste`: the backend stores into `<home>/…/data/terminal-images` with `rustix` `NOFOLLOW` on the data directory (`crates/ide-host/src/ops/term/images.rs`); on macOS the temp home is under `/var` (a symlink), which only matters for the last component. The path is read back from the screen with the rows concatenated; a wrapped row ending in a space would break it (the stored name has none).
- `closing_the_last_tab_of_a_terminal_group_focuses_its_neighbour`: `NEW_TAB_KEYS` (`cmd-t` / `ctrl-shift-t`) must reach `terminal.new` while `terminalFocus` is on; `terminal.new` is bound with `keyWhen: terminalFocus` in `assets/gen/actions.json`.
- `the_close_tab_button_and_cmd_w_ask_before_terminating_a_running_command`: the busy check now runs on the backend for every close (as before for Close Terminal Session); a shell idle by the time of the click closes at once, so the test first waits for `is_busy`, as `proofs15_terminal.rs` does.
- `each_group_has_a_header_whose_tabs_and_new_tab_button_act_on_that_group`: the tab's centre must fall on its label, not its ✕ (label ≈ 35px + 10px padding, ✕ slot 24px at the end).
