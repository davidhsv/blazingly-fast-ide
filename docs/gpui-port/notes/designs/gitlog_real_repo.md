# Git log on a large repository: empty graph, empty subjects, no branch chip

Native lines are from `git show HEAD:<path>` at commit 90f5b83b2. The webview, `crates/` and `src-tauri/` lines are from the working tree. Everything below was read in code; nothing was compiled or run. Claims that only reasoning supports are marked **Hypothesis:**, each with the check that settles it.

## 1. Why the subject column is empty

**The subject text itself is fine.** It is `Commit::summary`, the git subject of the commit on one line (`crates/ide-vcs/src/log_index.rs:315-333`). It is the same whether or not `summary_only` is set. `summary_only` only decides whether `message` is loaded in full (`crates/ide-vcs/src/history.rs:376-378`). The native row paints `commit.summary` (`app-gpui/src/vcs/log_view.rs:556-563`), so multi-line messages play no part.

**The cause is the row's layout** (`log_view.rs:540-605`). The row is a flex row, and the subject is its only child that can shrink (`flex_1().min_w_0()`, `:558-559`). Everything else is `flex_none`:

- **Graph column:** one width for every row, the widest graph row clamped to 24 lanes: `graph.iter().map(|row| row.width).max().unwrap_or(1).min(24) as f32 * LANE` (`:516`, applied at `:550`). With `LANE = 14` (`:42`) that is up to 336 px.
- **Ref labels:** each label is `flex_none` at its full text width (`:564-574`).
- **Fixed columns:** author 130 px (`:578`), date 120 px (`:590`), and 72 px hash when shown.
- **Spacing:** an 8 px gap between children and 10 px right padding (`:546-547`).

The list gets only about 42% of the Log's width. The Branches pane takes `relative(0.18)` with a 120 px minimum (`:660-661`), and the details pane takes `relative(0.4)` with a 220 px minimum (`:679-680`). Both show by default (`app-gpui/src/vcs/log_options.rs:91,95`).

**When nothing is painted:** list width − (graph column + labels + 130 + 120 + gaps + 10) ≤ 0. The subject is then laid out 0 px wide and draws nothing, while the labels, author and date still draw. That is exactly what was captured.

- **One-commit repository:** the graph column is 1 lane (14 px), so the subject has room.
- **Large repository:** as soon as any loaded row is 24 lanes wide, every row's graph column is 336 px. At a 1600 px window the list is about 650 px, while the fixed parts need about 336 + 8 + ~115 (one `origin/…` label) + 8 + 130 + 8 + 120 + 8 + 10 ≈ 743 px.

**Hypothesis:** the 650 / 743 px figures assume the bottom tool window spans the window less its stripes. The check is the `git-log-subject-N` bounds in test 2 below.

Why rows become 24+ lanes wide is point 2: raw lane numbers with gaps.

**The webview cannot squeeze the subject to nothing:**

- **Graph per row:** each row draws its own graph at that row's width, inside the subject cell (`src/features/vcs/GitLog.tsx:2159-2172`, `<svg width={row.width * LANE}>` at `:2368-2370`).
- **Shrinking cells:** the subject cell is `flex: 1; min-width: 0; overflow-x: clip` (`src/features/vcs/GitLog.module.css:253-262`). The subject is `flex: 0 1 auto` (`:290-296`). The refs shrink with a per-label ellipsis down to 11 px "before crowding out the commit message" (`:306-318`).
- **Reserved room:** the list's rows get `minWidth` = widest graph row + the column widths + 120 px (`GitLog.tsx:1389-1396`, used at `:1773`). The list is `overflow: auto` (`GitLog.module.css:168-175`), so a wide graph scrolls horizontally instead of eating the subject.
- **Wider list:** the webview splits Branches 18% / commits 54% / details 28% (`GitLog.tsx:335-350`), against 18% / 42% / 40% natively.
- **Date column:** 140 px (`GitLog.tsx:1286-1289`), against 120 px natively.

## 2. Why the graph column is empty

**Native lanes are computed from the whole page's ref layout.** `GitLog::loaded` lays out each page with `compute_graph` and nothing else (`log_view.rs:315-332`). `graph_commit` passes the commit's refs (`:87-95`), so the graph is "ref aware" (`app-gpui/src/vcs/graph.rs:185`).

- A commit that no open line leads to goes to its layout line minus one: `preferred = layout.layout[position].saturating_sub(1)` (`graph.rs:202-209`). `graph_layout` numbers the lines in the order of their heads: HEAD first, then main/master, then branches by name (`graph.rs:416-472`, ranks at `:399-411`).
- When that lane is past the end, the lanes are padded with `Gap` slots (`graph.rs:222-224`). Gaps are never drawn (`passing`/`up`/`down` only list `Taken` slots, `:214-232`), and `first_free` skips them (`:160-162`).
- So a log of every ref puts the newest commits of branches whose lines come late on lanes far to the right, with empty gap lanes on their left. HEAD's own line (lane 0) only starts at HEAD's first row.
- `width` is `max(lanes before, lanes after, lane + 1)` (`graph.rs:278-285`), so it counts the gaps.

**Exact condition for an empty graph cell in a row:** the node lane is ≥ 24, and no `Taken` lane < 24 passes through the row.

- The painter puts lane `l` at `x = 7 + 14·l` (`log_view.rs:447`).
- The cell's parent is `w(px(width)).overflow_hidden()` with `width ≤ 24·14` (`:516,550`), so anything at lane ≥ 24 is clipped.
- That holds for every row above the first commit of the 24 earliest-laid-out lines.

In the fixture below, rows 0-33 are on lanes 40 down to 24, with nothing on lanes 0-23, which covers more rows than the Git window shows.

**The webview packs the lanes before drawing.** `presentLogGraph` runs `computeGraph` over all loaded commits, then `hideLongEdges` (unless Show Long Edges is on), `compactLanes` and `straightenLaneChanges` (`src/features/vcs/logPresentation.ts:14-39`, at `:36-37`).

- `compactLanes` "packs each row's lanes side by side … lanes laid out by branch leave gaps (one per branch)". Its native port says so too (`app-gpui/src/vcs/graph.rs:766-794`).
- After packing, the newest row in the fixture is on lane 0 with width 1.
- The webview's `computeGraph` makes the same lane choice as native (`src/features/vcs/graph.ts:119-120`; its `firstFree` skips holes, `:70-73`). The difference is only that native never packs.

**The native port of the whole presentation exists but the view does not use it.** It is `present_log_graph` in `app-gpui/src/vcs_more/log_presentation.rs:43-71` (same pipeline, `:66-69`). It is called only from a log-actions test (`app-gpui/src/workbench/log_actions.rs:2532`).

**Native pages are laid out apart from each other.** `compute_graph(&page, previous_state)` builds `graph_layout` from that page only (`graph.rs:184`), so the second page's lane choices ignore the first page's refs. The existing page-equivalence test covers only commits without refs (`log_view.rs:813-840`, `refs: None`). The webview lays out all loaded commits each time (`GitLog.tsx:1355`).

**Other inputs ruled out by reading:**

- **Parents not in the loaded page:** they become `EdgeKind::Unloaded` lanes that are still drawn (`graph.rs:237-240`, arrows at `:290-295`).
- **Zero width:** the cell is `row.width.max(1)` wide (`log_view.rs:420`).
- **Lane colours:** `lane_color` returns `var(--…)` tokens stripped to names for `theme::color` (`log_view.rs:495-497`, `graph.rs:860-862`). **Hypothesis:** they resolve. If a token resolved to transparent, the one-commit repository would also show no node. The cheap check is to look at the node in a capture of the tiny repository; that report covered only the subject.

## 3. The default branch filter

**Native asks for every ref.** `GitLog::new` starts from `LogFilters::default()` (`log_view.rs:129`), whose `branches` is empty, "empty = all" (`app-gpui/src/vcs_ops/log_store.rs:58-60`). It reads at once in its constructor (`:133`) with `branches: filters.branches.clone()` (`:271`).

- In the backend, an empty `branches` makes `tips()` walk HEAD plus every local branch, every remote-tracking branch and every tag (`crates/ide-vcs/src/history.rs:1116-1200`, the fallback at `:1188-1200`).
- The Branch chip shows the joined branches or nothing (`app-gpui/src/vcs/log_filters.rs:416-425`), so it shows no value. That is the missing chip in the capture.
- **Hypothesis:** in the Tauri app the Branch chip is drawn with its value "HEAD" (`GitLog.tsx:902-906`), and the native chip without a value reads as no chip. Check: the chip's text in a native capture.

**The webview settles the filter before its first read, once the repository's branches are known** (`GitLog.tsx:299-310`):

- `initLogBranchFilter` (`src/features/vcs/logStore.ts:240-252`) resolves `resolveLogBranches(remembered, names)` (`:208-213`): the remembered refs that still exist, else `["HEAD"]`. A remembered empty list means every branch.
- The memory is `localStorage` key `openidea.vcs.logBranches:<repo root>` (`:215-233`), written by `setLogBranches` on every Branch choice in the main Log (`:255-271`), including clearing the chip (`GitLog.tsx:906`, `setLogBranches([])`).
- In a project of several repositories the filter is not settled and stays empty: all branches (`logStore.ts:242`).
- The whole Log session (tabs, filters, selection) is also stored per root under `openidea.vcs.logSession.v1:<root>` (`logStore.ts:558`, `:598-660`).

**The native port of all this exists, but nothing in the app calls it.** It is `app-gpui/src/vcs_more/log_branch_memory.rs:11-68` (`branch_memory_key`, `remembered_log_branches`, `init_log_branch_filter`, `set_log_branches`). It is called only from tests (`vcs_more/log_loading.rs:1531-1538`). The parity row `unit:src/features/vcs/__tests__/logBranchMemory.test.ts` already records this.

The native Branch choices set the filter without remembering it: `LogFilterEvent::SetBranches` → `filter_log` (`app-gpui/src/workbench/log_window.rs:246-248`), and the Branches pane's `SetLogFilter` (`:298-300`).

## 4. Other differences a user sees on first opening a large repository

**Page read ahead.** Both read pages of 300 (`log_view.rs:38`; `logStore.ts:108`). The thresholds differ:

- The webview asks for the next page while the last visible row is within a page of the end: `last.index >= commits.length - PAGE` (`GitLog.tsx:1453-1456`). So it reads a second page at once, and has 600 commits while the top rows show.
- Native reads on only when `range.end + PAGE/2 >= len` (`log_view.rs:522`) or `index + PAGE/2 >= len` on selection (`:345`). So it has 300.

**PageDown / PageUp.** Webview 20 rows (`GitLog.tsx:1559-1560`); native 10 (`log_view.rs:357`).

**Summaries against full messages.**

- The webview asks `summaryOnly: true` (`logStore.ts:950`). The backend then serves positions from a cached view of the whole history and leaves `message` at the subject (`history.rs:272-345`).
- Native sets no `summary_only` (`log_view.rs:268-290`). So every row loads its full message (`history.rs:376-378` through `materialize_row` at `:517`), and the full messages stay in memory in `Rc<Vec<Commit>>` for every loaded row.
- **Hypothesis:** on a repository with long messages this is several times the summary-only memory. Check: `vmmap`/heap of the native app after loading 3000 rows, with and without the hunk below.
- The details pane already reads the full message of the selection separately (`app-gpui/src/vcs/log_details.rs:1041-1046`, the event at `:806`). Edit Message reads it when `message_truncated` (`log_actions.rs:808-821`).
- Copy Commit Message does not: it copies the row's `message` (`log_actions.rs:1447-1451`). The webview reads the details when truncated (`GitLog.tsx:1248-1251`).

**Re-read on every VCS change, visible or not.**

- Native re-reads everything loaded (limit = max(300, loaded)) on every `IdeEvent::VcsChanged` of its window (`log_view.rs:234-242`), with full messages, even while the Git window is hidden.
- `VcsChanged` is sent for any change under `.git` other than the IDE's own index writes (`crates/ide-host/src/project.rs:468-470`, `:2076-2078`, and `:1610`).
- The webview reloads only while the log is active, and only when the refs signature (HEAD and every branch tip) changed since the last load (`GitLog.tsx:312-321`). A change made while it is hidden is picked up when it becomes active again (same effect, `[version, active]`).
- The native refs signature exists (`log_store.rs:173-185`) and is unused by the view.

**Focus on opening.**

- Native gives the keyboard to the panel's focus, which is the commit list: `Panel::new` takes `view.focus_handle` (`app-gpui/src/workbench/mod.rs:301-305`), applied at `:2409-2416`. The test `workbench::tests::the_git_tool_window_shows_the_projects_log` asserts the list has it.
- The webview focuses the first `[data-autofocus]`, else the first focusable that is not a button or link, in the tool window body (`src/workbench/toolwindows/ToolWindows.tsx:841-851`; `FOCUSABLE` at `src/ui/popupState.ts:99-100`).
- In the Git log the Branches pane comes first (`GitLog.tsx:333-339`). Its first non-button focusable is the "Filter branches and tags" field (`GitLog.tsx:544-552`). Without the pane, it is the filter bar's "Text or hash" field (`GitLog.tsx:937-942`).
- **Hypothesis:** nothing above the body (the Log/Console tab strip) is inside `[data-toolwindow-body]` and focusable first. Check in the Tauri app: open Git, `document.activeElement.getAttribute("aria-label")`.

**Selection on opening.** Native selects nothing. The webview restores the selection stored in the Log session of the root (`logStore.ts:598-660`).

**Lane order of remote branches.** The webview passes `remoteNames` into the graph, which ranks remote branches after local ones (`graph.ts:64`, through `multiRoot.ts` `graphCommit`). Native passes none (`log_view.rs:87-95`), so remote branches rank with the local ones (`graph.rs:399-411`). This changes lane order and colours, not emptiness.

## Smallest changes, in the order to apply them

Each hunk names its file and quotes its anchor from HEAD.

### H1: debug selectors on the row cells (test dependency, no behaviour change)

`app-gpui/src/vcs/log_view.rs`, in `commit_list`. These are what the tests measure.

Anchor `.child(div().w(px(width)).h_full().flex_none().overflow_hidden().child(GraphCell {` → becomes part of H3.

Anchor:

```rust
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
```

becomes:

```rust
                            .child(
                                div()
                                    .debug_selector(move || format!("git-log-subject-{index}"))
                                    .flex_1()
                                    .min_w(px(SUBJECT_MIN))
```

Add next to `const LANE` (`:41-42`):

```rust
/// What the subject of a row keeps beside the graph, the references and the columns: a wide graph pushes the
/// columns out of view rather than the subject.
const SUBJECT_MIN: f32 = 120.0;
```

Why: 120 px is the room the webview reserves for the subject (`GitLog.tsx:1393`, the `120` base of `minWidth`). Native has no horizontal scroll, so the reserve goes on the subject itself. The columns on the right are clipped when a row's graph is very wide. A full match is a horizontally scrolling list (`uniform_list` with `ListHorizontalSizingBehavior::Unconstrained` sized from the widest row), left as a follow-up.

### H2: lay out and pack the graph of everything loaded

`app-gpui/src/vcs/log_view.rs`.

Import, anchor `use super::graph::{GraphCommit, GraphRow, GraphState, compute_graph, lane_color};` becomes:

```rust
use super::graph::{
    GraphCommit, GraphRow, GraphState, LONG_EDGE, compact_lanes, compute_graph, hide_long_edges, lane_color,
    straighten_lane_changes,
};
```

Remove the field (anchor `    /// The lanes open below the last commit read, from which the next page's graph continues.` and `    lanes: GraphState,`) and its initialiser `            lanes: GraphState::default(),`.

In `loaded`, replace the block from `        // A page continues the graph where the last one stopped: what is already drawn is not` through `        Rc::make_mut(&mut self.commits).extend(page);` with:

```rust
        if again {
            self.commits = Rc::default();
        }
        Rc::make_mut(&mut self.commits).extend(page);
        self.present_graph();
```

Add to `impl GitLog`:

```rust
    /// Lays out the graph of every commit read, as it is drawn: where a line goes depends on every reference read,
    /// so a page is not laid out apart from the ones before it, and the gaps the layout leaves between lines are
    /// packed away.
    fn present_graph(&mut self) {
        let commits: Vec<GraphCommit> = self.commits.iter().map(graph_commit).collect();
        let laid = compute_graph(&commits, GraphState::default()).rows;
        let edges = if self.options.long_edges { laid } else { hide_long_edges(&laid, LONG_EDGE) };
        self.graph = Rc::new(straighten_lane_changes(&compact_lanes(&edges)));
    }
```

`set_options` (anchor `        self.options = options;\n        cx.notify();`) becomes:

```rust
        let relayout = options.long_edges != self.options.long_edges;
        self.options = options;
        if relayout {
            self.present_graph();
        }
        cx.notify();
```

Why: this is the webview's pipeline (`logPresentation.ts:36-37`), without Collapse Linear and Linearize. Those change which rows exist, and this view keeps one row per commit. Laying out all loaded commits each time is what the webview does (`GitLog.tsx:1355`).

**Hypothesis:** this costs O(loaded) per page, a few ms at a few thousand rows. Check: time `present_graph` at 3000 rows in a unit test.

### H3: graph cell at its row's width, labels that shrink, the date at 140 px, the webview's split

`app-gpui/src/vcs/log_view.rs`, `commit_list`.

Delete `        let width = graph.iter().map(|row| row.width).max().unwrap_or(1).min(24) as f32 * LANE;`.

Replace

```rust
                            .child(div().w(px(width)).h_full().flex_none().overflow_hidden().child(GraphCell {
                                row: cell,
                                head,
                                colors: colors.clone(),
                                background,
                            }))
```

with

```rust
                            .child(
                                div()
                                    .debug_selector(move || format!("git-log-graph-{index}"))
                                    .w(px(cell.width.max(1) as f32 * LANE))
                                    .h_full()
                                    .flex_none()
                                    .overflow_hidden()
                                    .child(GraphCell { row: cell, head, colors: colors.clone(), background }),
                            )
```

`cell.width` is read before `cell` moves: same expression order, the width is evaluated first.

Replace `                            .children(labels.into_iter().map(|label| {` … `                            }))` (the labels block) by the same children inside a container that shrinks:

```rust
                            .when(!labels.is_empty(), |row| {
                                row.child(
                                    div()
                                        .flex()
                                        .flex_shrink()
                                        .min_w(px(11.0))
                                        .overflow_hidden()
                                        .gap(px(8.0))
                                        .children(labels.into_iter().map(|label| {
                                            /* the existing label body, unchanged */
                                        })),
                                )
                            })
```

Why: the refs shrink before they crowd out the subject (`GitLog.module.css:306-318`).

Date column: anchor `.w(px(120.0))` inside `.when(options.show_date,` becomes `.w(px(140.0))` (`GitLog.tsx:1289`).

Details pane: anchor `.w(relative(0.4))` becomes `.w(relative(0.28))` (webview `defaultSize="28%"`, `GitLog.tsx:346`).

### H4: read a page ahead; PageDown 20

`app-gpui/src/vcs/log_view.rs`.

- `                if range.end + PAGE as usize / 2 >= commits.len() {` becomes `                if range.end + PAGE as usize > commits.len() {`. `range.end` is the last index + 1, so this is `GitLog.tsx:1455`.
- `        if index + PAGE as usize / 2 >= self.commits.len() {` becomes `        if index + PAGE as usize >= self.commits.len() {`.
- `        let page = 10;` becomes `        let page = 20;` (`GitLog.tsx:1559-1560`).

### H5: summaries only, and Copy Commit Message reads the whole message

`app-gpui/src/vcs/log_view.rs`, in the `LogQuery` literal: add `            summary_only: true,` before `            ..LogQuery::default()`.

`app-gpui/src/workbench/log_actions.rs:1447-1451`, `LogCommand::CopyCommitMessage`: when `commit.message_truncated`, read `git::vcs_log_details` as `edit_message` does at `:808-821`, then copy. This needs a spawn in the window like the other commit actions.

Why: `logStore.ts:950`, `GitLog.tsx:1248-1251`.

### H6: settle and remember the branch filter; re-read on ref moves, only while shown

1. `app-gpui/src/vcs/log_view.rs` `GitLog::new`: anchor `            filters: LogFilters::default(),` becomes

   ```rust
               // The current branch until the window settles the filter of the repository.
               filters: LogFilters { branches: vec!["HEAD".to_owned()], ..LogFilters::default() },
   ```

   The first read is then HEAD's, never every ref. With nothing remembered, settling resolves to `["HEAD"]` again, and `set_filters` reads nothing more (`:204-209`).

2. `app-gpui/src/vcs/log_view.rs`: delete the VcsChanged re-read, so the window drives it from its refs signature:
   - `fn backend_event` (`:234-242`);
   - the listener in `new` (`:101-112`) and its `listener` field;
   - the `BackendEvent`/`IdeEvent` imports, if then unused.

3. `app-gpui/src/vcs_more/log_branch_memory.rs:22`: `fn remember_log_branches` becomes `pub fn remember_log_branches`.

4. `app-gpui/src/workbench/log_window.rs`, struct `LogSlot`: add

   ```rust
       /// The repository whose branch filter the log was settled for.
       branch_scope: Option<String>,
       /// HEAD and the branch tips the log was last read at.
       refs: Option<String>,
       /// A ref moved while the Git window was hidden: the log reads again when it shows.
       stale: bool,
   ```

   Initialise them `None, None, false` in `mount_git_log`'s `LogSlot { … }`.

5. Same file, add:

   ```rust
       /// Settles the log's branch filter once the repository's branches are known (the choice last made there,
       /// else the current branch; every branch in a project of several repositories), and reads the log again when
       /// HEAD or a branch tip moved: at once while the Git window shows, else when it shows next.
       fn settle_git_log(&mut self, cx: &mut Context<Self>) {
           let visible = self.is_tool_window_visible(GIT_TOOL_WINDOW);
           let Some(vcs) = self.vcs_state().filter(|vcs| vcs.branches_loaded) else { return };
           let repo = vcs.current_repo_root().map(str::to_owned);
           let single = vcs.roots.len() <= 1;
           let names: Vec<String> = vcs
               .branches
               .iter()
               .filter(|branch| !branch.name.ends_with("/HEAD"))
               .map(|branch| branch.name.clone())
               .collect();
           let refs = refs_signature(vcs.branch.as_ref(), &vcs.branches);
           let storage = cx.try_global::<UiStorage>();
           let settled = match repo.as_deref().filter(|_| single) {
               Some(repo) => resolve_log_branches(storage.and_then(|s| remembered_log_branches(s, repo)).as_deref(), &names),
               None => Vec::new(),
           };
           let Some(slot) = self.git_log.as_mut() else { return };
           let log = slot.log.clone();
           if slot.branch_scope != repo {
               slot.branch_scope = repo;
               slot.refs = Some(refs);
               slot.stale = false;
               let patch = LogFiltersPatch { branches: Some(settled), ..LogFiltersPatch::default() };
               log.update(cx, |log, cx| log.set_filters(patch, cx));
               return;
           }
           if slot.refs.as_ref() != Some(&refs) {
               slot.refs = Some(refs);
               slot.stale = true;
           }
           if visible && std::mem::take(&mut slot.stale) {
               log.update(cx, |log, cx| log.reload(cx));
           }
       }
   ```

   Imports: `crate::vcs_ops::log_store::{refs_signature, resolve_log_branches}`, `crate::vcs_more::log_branch_memory::{remembered_log_branches, remember_log_branches}`, `GIT_TOOL_WINDOW` from `super`.

   **Hypothesis:** the clippy-clean form of the borrow split may need the `vcs` reads in their own block. The values are all owned before `self.git_log.as_mut()`.

6. Same file: first line of `sync_log_panes` (anchor `        let Some(slot) = &self.git_log else { return };` right after `pub(super) fn sync_log_panes`): insert `        self.settle_git_log(cx);` before it.

   `sync_log_panes` runs on mount (`log_window.rs:156`) and after every VCS state change (`app-gpui/src/workbench/branches.rs:1658`), which is the webview's `[root, repo, branchesLoaded, active]` / `[version, active]` effects. Make `settle_git_log` `pub(super)` for step 7.

7. `app-gpui/src/workbench/mod.rs`, in the layout change path: anchor `        self.mount_panels(window, cx);` (the one followed by `let Content::Project { layout, focus_served, panels, .. }`, about `:2409`) gets `self.settle_git_log(cx);` after it. Showing the window reads a stale log.

8. Remember the main Log's branch choices: `log_window.rs` `LogFilterEvent::SetBranches(branches)` (`:246-248`) and `BranchesPaneEvent::SetLogFilter { branches, .. }` (`:298-300`). After `filter_log`:

   ```rust
   if let Some(repo) = self.vcs_state().filter(|vcs| vcs.roots.len() <= 1).and_then(|vcs| vcs.current_repo_root()).map(str::to_owned) {
       remember_log_branches(cx.default_global::<UiStorage>(), &repo, &branches);
   }
   ```

   Clone `branches` before it moves into the patch. Webview: `logStore.ts:255-271`. The webview also splits `..` entries into the range there. Native `SetBranches` does not; that is out of scope here.

### H7: the keyboard to the Branches pane's field on opening

`app-gpui/src/workbench/log_window.rs` `mount_git_log`: anchor `        Panel::new(log, None)` becomes

```rust
        // Like any tool window, the Git window gives the keyboard to the first field of its content: the Branches
        // pane's filter while the pane shows, else the filter bar's text field.
        let (pane, bar, shown) = (branches_for_focus, filter_bar_for_focus, log.clone());
        Panel {
            view: log.into(),
            focus: Rc::new(move |cx| {
                if shown.read(cx).options().show_branches {
                    pane.read(cx).search_field().focus_handle(cx)
                } else {
                    bar.read(cx).text_field().focus_handle(cx)
                }
            }),
            _events: None,
        }
```

`branches_for_focus` and `filter_bar_for_focus` are clones of `branches` and `filter_bar` taken before they move into `LogSlot`. `Panel`'s fields are private to `workbench`, and `log_window` is its child.

Apply it only once the hypothesis in §4 (focus on opening) is confirmed in the Tauri app.

## Existing tests the hunks change

- **`vcs::log_view::tests::shows_a_repositorys_commits_with_their_graph`** calls the private `backend_event` (`log_view.rs:801-805`), which H6.2 removes. Replace it with `log.update(cx, |log, cx| log.reload(cx));`. Its graph asserts (`:779-781`) survive H2. The merge row keeps two down segments and width ≥ 2 once packed. **Hypothesis:** check by running.
- **`workbench::tests::the_log_filters_by_text_and_by_the_branch_chosen_in_the_branches_pane`** (`workbench/mod.rs:11795-11820`) asserts the all-branches default: "Without a branch filter the log shows every branch". With H6 the log opens on HEAD, `["beta change", "alpha fix", "first"]`. Assert that, or clear the Branch chip first.
- **`workbench::tests::the_git_tool_window_shows_the_projects_log`** (`mod.rs:5752`) asserts the list has the keyboard. With H7 it is the branch filter field.
- **`workbench::log_actions::tests::cherry_pick_applies_a_commit_of_another_branch_and_a_conflicting_one_opens_the_conflicts`** waits for 4 commits (`log_actions.rs:1882`), but `main` holds 2 (`:1863-1872`). Same for **`collapse_and_expand_linear_branches_set_the_option_the_log_presents_its_graph_with`**, which waits for 6 (`:2526-2539`) where `main` holds 2. Both hang with H6 unless they clear the Branch chip first.
- **Hypothesis:** `the_commit_menu_offers_history_rewriting_for_a_pushed_commit_too`, `refuses_every_rewrite_of_pushed_commits_of_a_protected_branch` and `asks_before_rewriting_pushed_commits_and_rewrites_nothing_when_declined` (`show_log(…, 4)` at `:2683,2714,2759`) may count commits of other branches. Check by running.
- **Hypothesis:** log_actions tests that press keys right after `show_log` without focusing the list (`the_logs_keys_work_while_the_log_has_the_keyboard` focuses it, `:1779`) change under H7. Check by running.

## The tests (written)

**File:** `app-gpui/src/workbench/tests/proofs42_gitlog_real_repo_pending.rs` (new, undeclared).

**Declaration:** add `    mod proofs42_gitlog_real_repo_pending;` inside `mod tests` of `app-gpui/src/workbench/mod.rs`, after the line `    mod proofs41_recent_edited;`.

**Dependencies:**

- H1 for the `git-log-graph-N` / `git-log-subject-N` selectors. Without H1, tests 1-2 panic at "is drawn", which is also a failure before the fix.
- H6.3 is not needed by the tests: they read the memory through the existing `remembered_log_branches`.

**Repositories.** Each test builds its repository in its temp folder:

- with one `git fast-import` stream;
- with `GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null`;
- with author and committer Ada <ada@example.test> at fixed dates (1700000000 + minutes);
- with no network. Remote-tracking refs are written by the stream as `refs/remotes/origin/*`.

**`branches_newer_than_head`** has 140 commits. `main` has 50 commits. `stable`, checked out, adds 10. 40 remote-tracking branches `origin/topic-00..39` each fork from `main` with 2 commits, newer than HEAD, the higher number the newer.

Why 40: under every ref, HEAD's line is laid out first, `main`'s tip is on it, and `topic-k` gets line k+2, so lane k+1. The newest rows (topic-39 down to topic-23, two each) are rows 0-33 on lanes 40-24. That is past the 24 lanes the 336 px column shows, with nothing on lanes 0-23. 34 rows is more than a bottom tool window shows, and 24 + 16 more branches is the fewest for that. The same 40 lanes give 41-lane rows, so before H3 the column is 336 px and squeezes the subject.

**`three_commits`** and a **700-commit linear `main`** complete the set. 700 is enough to read two full pages (600) and leave a third unread.

| Test                                                                        | Builds                                                                                                          | Asserts on the window                                                                                                                                                                            | Fails today because                                        |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------- |
| `the_log_opens_on_the_current_branch_and_its_branch_chip_reads_head`        | 140-commit fixture, 1600×900 window, Git window shown                                                           | Branch chip `Some("HEAD")`; filter `["HEAD"]`; 60 commits, `stable 10` first, `main 50` 11th, `main 1` last; every graph row `(lane 0, width 1)`; rows 0-3 node inside cell and subject ≥ 120 px | chip `None`, 140 commits, `topic 39 2` first               |
| `with_every_branch_shown_the_newest_rows_draw_their_node_and_their_subject` | same; Branch chip cleared (`FilterKind::Branch`)                                                                | 140 commits, first three `topic 39 2`, `topic 39 1`, `topic 38 2`; `graph()[0].lane == 0`; rows 0-3 `(lane+1)·14 ≤ cell width` and subject ≥ 120 px; `remembered_log_branches(repo) == Some([])` | lane 40 in a 336 px cell; subject 0 px; nothing remembered |
| `the_log_reads_a_page_ahead_and_page_down_moves_twenty_rows`                | 700 linear commits                                                                                              | 600 commits read (two pages asked of the backend); Down selects `commit 700`, PageDown selects `commit 680`                                                                                      | 300 read (the wait runs out); PageDown gives `commit 690`  |
| `showing_the_git_window_gives_the_keyboard_to_the_branch_filter_field`      | 3 commits                                                                                                       | Branches pane search field focused; list not focused                                                                                                                                             | list focused                                               |
| `a_hidden_log_reads_the_repository_again_only_when_it_shows`                | 3 commits; Git window hidden; outside commit `later` (fixed date); `VcsChanged` emitted through `Backend::emit` | once the window's VCS state has the new HEAD, the log is not loading and still lists 3; shown again, it lists `later, three, two, one`                                                           | the log re-reads on the event while hidden                 |

**Most likely to fail on first run, and why:**

- **Subject ≥ 120 px (tests 1-2):** depends on the bottom tool window's real width at 1600 px. **Hypothesis:** about 1520 px of body, so the list is about 840 px with H3's 28% details. Lower the bound only if the measured list is narrower than expected, never below what H1's reserve guarantees.
- **Rows 0-3 must be in view:** if the Git window is under about 4 rows tall, `drawn` panics with "is drawn".
- **Test 5:** hiding relies on `toolwindow.git` focusing, then hiding (`tool_windows.rs:846-857`). The new-HEAD wait relies on the workbench re-reading the branch on the emitted event (`mod.rs:2060-2070`).
- **Test 3:** with the old thresholds it fails by timeout, after wait_long's ~60 s, not by assertion.

## Inventory rows (`docs/gpui-port/parity.json`)

| Key                                                            | Status today                                      | What is wrong                                                                                                                                                                             | After H1-H7 + these tests                                                                                                                               |
| -------------------------------------------------------------- | ------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ipc:vcs_log`                                                  | partial                                           | Its proofs include `the_log_filters_by_text_and_by_the_branch_chosen_in_the_branches_pane`, which enshrines the all-refs default. None asks `summary_only`, and none asserts a drawn row. | Stays partial (multi-root, seek, `stopAt` boundary joins not used natively); add tests 1-3 to the proof.                                                |
| `unit:src/features/vcs/__tests__/logBranchMemory.test.ts`      | partial ("nothing in the app reaches it")         | Correct as written.                                                                                                                                                                       | Tests 1-2 make the app reach it. It can become done if its remaining cases (multi-root skip, deleted branch → HEAD) are also driven through the window. |
| `boundid:toolwindow.git`                                       | done                                              | The proof's "with the log, focused" asserts the native focus target, not the webview's. **Hypothesis:** confirm the webview's target first.                                               | Done with test 4 once H7 lands.                                                                                                                         |
| `unit:src/features/vcs/__tests__/graph.test.ts`                | partial                                           | Lane packing (`packs_the_lanes_of_each_row_side_by_side_in_their_order`) is proved on the model, but the view never packs, so the proof says nothing about the log.                       | Unchanged status; tests 1-2 add the drawn proof.                                                                                                        |
| `unit:src/features/vcs/__tests__/logGraphCorrectness.test.tsx` | partial ("the graph cell's drawing … not ported") | Correct.                                                                                                                                                                                  | Tests 1-2 prove node placement inside the drawn cell; keyboard navigation still missing.                                                                |
| `unit:src/features/vcs/__tests__/logUiFeatures.test.tsx`       | partial                                           | "column … behaviour of the commit list" is not proved.                                                                                                                                    | Still partial; test 2 covers the subject's reserved width only.                                                                                         |
| `unit:src/features/vcs/__tests__/logLoading.test.ts`           | partial                                           | Paging thresholds are not proved through the window.                                                                                                                                      | Test 3 adds the read-ahead; still partial.                                                                                                              |
| `feature:vcs`                                                  | partial                                           | Cites `vcs::log_view::tests::shows_a_repositorys_commits_with_their_graph`, which asserts state, never what is drawn.                                                                     | Add tests 1-5.                                                                                                                                          |
| `context:vcsLogFocus`, `action:vcs.log.showBranches`           | done                                              | Not affected: the branch field is in a pane `vcsLogFocus` already counts (`log_window.rs:167-177`).                                                                                       | Unchanged.                                                                                                                                              |
