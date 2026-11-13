# Integrating change lists, the shelf, patches and the before-commit checks

New files, not compiled yet:

- `app-gpui/src/workbench/shelf.rs` (the workbench module, its model tests `mod model_tests` and its end-to-end tests `mod tests`)
- `app-gpui/src/vcs/change_dialogs.rs` (Shelve, Unshelve, New / Edit / Move-to changelist dialogs, and the dialog parts the others share)
- `app-gpui/src/vcs/patch_dialogs.rs` (Create Patch, Apply Patch)
- `app-gpui/src/vcs/shelf_view.rs` (the Shelf tab)

No existing file was edited. Every anchor below is quoted from the file as it is now; line numbers are left out. Sections 1 to 9 are all required: without 4, 5 and 6 the new module calls methods that do not exist, without 7 the old `show_change_menu` is dead code under `-D warnings`. Run `cargo fmt` afterwards: some pasted lines and a few English string literals in `shelf.rs` run past 120 columns (rustfmt leaves long literals as they are).

## 1. `app-gpui/src/vcs/mod.rs`

Add, in alphabetical order (`change_dialogs` after `branches_popup`, `patch_dialogs` after `merge_view`, `shelf_view` after `prompt_dialogs`):

```rust
pub mod change_dialogs;
pub mod patch_dialogs;
pub mod shelf_view;
```

## 2. `app-gpui/src/vcs/prompt_dialogs.rs`: share the drawing helpers

The new dialogs draw their checkboxes, buttons and fields exactly as `PromptDialog` does. Make these four module functions visible to the `vcs` module:

- `fn checkbox(cx: &App, label: String, on: bool) -> Div {` → `pub(super) fn checkbox(cx: &App, label: String, on: bool) -> Div {`
- `fn button(cx: &App, label: String, primary: bool, danger: bool, enabled: bool) -> Div {` → `pub(super) fn button(…`
- `fn field_frame(cx: &App, invalid: bool) -> Div {` → `pub(super) fn field_frame(…`
- `fn tx(cx: &mut App, text: &str) -> String {` → `pub(super) fn tx(…`

## 3. `app-gpui/src/workbench/mod.rs`

### 3a. The module

After `mod settings_window;` add:

```rust
mod shelf;
```

### 3b. The field

In `pub struct Workbench`, after

```rust
    conflicts: conflicts::ConflictsSlot,
```

add:

```rust
    /// The project's change lists and shelf.
    shelf: shelf::ShelfSlot,
```

### 3c. Its initial value

In `Workbench::new`, after `conflicts: Default::default(),` add:

```rust
            shelf: Default::default(),
```

### 3d. The actions

In `pub fn register_actions(cx: &mut App)`, after `conflicts::register_actions(registry);` add:

```rust
    shelf::register_actions(registry);
```

### 3e. The context key

In `fn publish_context`, after `self.publish_merge_context(window, cx);` add:

```rust
        self.publish_shelf_context(window, cx);
```

### 3f. The Commit tool window's tree gets its lists and tabs

In `fn mount_panels`, in the `COMMIT_TOOL_WINDOW =>` arm, after

```rust
                    self.attach_commit_box(&changes, window, cx);
```

add:

```rust
                    self.attach_changes_extras(&changes, cx);
```

### 3g. The test helpers the new tests reuse

In `mod tests`, make these `pub(super)` (prefix `fn` with `pub(super) `): `fn wait_long(`, `fn click_on(`, `fn run_action(`, `fn git_in(`, `fn init_repo(`, `fn git_project(`, `fn wait_for_prompt(`, `fn retype(`, `fn notification_titles(`, `fn wait_for_notice(`, `fn commit_parts(`, `fn published(`, `fn publish(`. `shelf::tests` reaches them as `super::super::tests::…`. If `git_project`'s returned `&mut VisualTestContext` lifetime or a private type in a signature trips the privacy lint, the fallback is moving these helpers into a `#[cfg(test)] pub(super) mod test_support` of `workbench` (the bodies unchanged).

### 3h. `init_repo` keeps the IDE's per-user files out of `git status`

Saving the change lists (now on every new change) and shelving write `.openidea/.gitignore` through `keep_private` (`crates/ide-host/src/ops/vcs/commands.rs`). An untracked `.openidea/` would then appear in the changes tree and in `git status --porcelain` of every test using `init_repo`. Replace the body of `fn init_repo(root: &std::path::Path)` with:

```rust
    fn init_repo(root: &std::path::Path) {
        git_in(root, &["init", "-q", "-b", "main"]);
        git_in(root, &["config", "user.name", "Ada"]);
        git_in(root, &["config", "user.email", "ada@example.test"]);
        std::fs::write(root.join("a.txt"), "one\n").expect("the file is written");
        // The IDE's per-user state stays out of the repository, as the IDE itself would have it.
        std::fs::create_dir_all(root.join(".openidea")).expect("the folder is made");
        std::fs::write(root.join(".openidea/.gitignore"), "# Per-user IDE state\n/shelf/\n/changelists.json\n")
            .expect("the file is written");
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "first"]);
    }
```

Check: `grep -n '"init", "-q"' app-gpui/src/workbench/mod.rs`. Tests that make a repository without `init_repo` (e.g. through `trusted_project` + their own `git init`) get the untracked `.openidea/.gitignore` once a list is saved; fix them the same way if one asserts on an empty status or exact tree lines. No test asserts `git show --name-only` of the first commit at the time of reading.

## 4. `app-gpui/src/workbench/branches.rs`

### 4a. The project's lists and shelf follow the project

In `pub(super) fn mount_vcs`, replace

```rust
        self.mount_conflicts(window);
        self.refresh_vcs(cx);
```

with

```rust
        self.mount_conflicts(window);
        self.mount_shelf(window, cx);
        self.refresh_vcs(cx);
```

In `pub(super) fn drop_vcs(&mut self)`, after `self.drop_conflicts();` add:

```rust
        self.drop_shelf();
```

### 4b. Every repository read feeds the change lists

In `fn vcs_refreshed`, after

```rust
        self.conflicts_changed(conflicts, cx);
```

add:

```rust
        self.changelists_follow_status(cx);
```

It must run before the waiters are settled: a Commit that waited for the read then plans with the lists already following it.

### 4c. Writing to the Git console from the sibling module

In `impl Workbench`, after `pub fn vcs_console(&self) -> Option<&VcsConsole> { … }`, add:

```rust
    /// Writes `text` to the window's Git console.
    pub(super) fn log_vcs_console(&mut self, kind: ConsoleKind, text: &str) {
        if let Some(slot) = self.vcs.as_mut() {
            slot.console.log(kind, text, now_ms());
        }
    }
```

(`ConsoleKind` and `now_ms` are imported there already.)

## 5. `app-gpui/src/workbench/commit.rs`

### 5a. Imports

- Remove `use crate::vcs_ops::changelists::initial_changelists;`.
- Replace `use crate::vcs_ops::change_ops::{commit_precheck, commit_success, commit_title, committed};` with `use crate::vcs_ops::change_ops::{commit_candidates, commit_precheck, commit_success, commit_title, committed};`.

### 5b. The plan uses the project's lists

In `fn commit_box_input`, replace

```rust
            (Some(vcs), Some(slot)) => current_plan(vcs, &initial_changelists(), slot.state.read(cx)).included.len(),
```

with

```rust
            (Some(vcs), Some(slot)) => current_plan(vcs, self.changelists(), slot.state.read(cx)).included.len(),
```

and in the `CommitBoxInput { … }` literal there, after `plugin_checks: Vec::new(),` add:

```rust
            unavailable_checks: super::shelf::unavailable_commit_checks(),
```

### 5c. The commit runs the checks

In `async fn commit`, replace

```rust
        let plan = current_plan(&vcs, &initial_changelists(), &state);
        let merging = vcs.branch.as_ref().and_then(|branch| branch.operation.as_deref()) == Some("merge");
        let commit = &settings::effective(&workbench.label, cx).vcs.commit;
        let skipped: Vec<&str> = [
            (commit.analyze_code, "analyze code"),
            (commit.check_todo, "check TODO"),
            (commit.reformat, "reformat"),
            (commit.optimize_imports, "optimize imports"),
            (commit.cleanup, "cleanup"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect();
        if !skipped.is_empty() && !request.anyway {
            tracing::info!("before-commit checks are not run: {}", skipped.join(", "));
        }
        (state, vcs, plan, merging, commit.clear_initial_message)
```

with

```rust
        let plan = current_plan(&vcs, workbench.changelists(), &state);
        let merging = vcs.branch.as_ref().and_then(|branch| branch.operation.as_deref()) == Some("merge");
        let clear = settings::effective(&workbench.label, cx).vcs.commit.clear_initial_message;
        (state, vcs, plan, merging, clear)
```

Then replace

```rust
    if let Some(notice) = commit_precheck(&request.message, amend, &plan, merging) {
        return notify(this, cx, notice);
    }
```

with

```rust
    if let Some(notice) = commit_precheck(&request.message, amend, &plan, merging) {
        return notify(this, cx, notice);
    }
    // Failed checks show in the box; the next Commit (Commit Anyway) skips them.
    if !request.anyway {
        let problems = super::shelf::run_commit_checks(this, cx, commit_candidates(&plan), amend).await?;
        if !problems.is_empty() {
            return this.update(cx, |workbench, cx| {
                let state = workbench.commit_state(cx);
                state.update(cx, |state, cx| {
                    state.check_problems = problems;
                    cx.notify();
                });
            });
        }
    }
```

`committed(…)` already clears `check_problems` after a commit (check `vcs_ops/change_ops.rs` `committed`; if it does not, add `state.check_problems.clear();` in the `state.update` after the commit).

### 5d. Review

In `fn commit_box_event`, replace

```rust
            // Only the before-commit checks report problems, and none run here.
            CommitBoxEvent::ProblemDetails(index) => tracing::debug!("no details for commit problem {index}"),
```

with

```rust
            CommitBoxEvent::ProblemDetails(index) => self.commit_problem_details(*index, window, cx),
```

### 5e. The tabs follow the commit state

In `pub(super) fn commit_state`, replace

```rust
        let observe = cx.observe(&state, |workbench, _, cx| workbench.sync_commit_box(cx));
```

with

```rust
        let observe = cx.observe(&state, |workbench, _, cx| {
            workbench.sync_commit_box(cx);
            workbench.sync_commit_tabs(cx);
        });
```

### 5f. The module doc

Replace the paragraph

```rust
//! The before-commit checks (code analysis, TODO, reformat…) have no runner here: a commit runs
//! without them whatever they are set to, and nothing reports them as passed.
```

with

```rust
//! Before the commit the before-commit checks run (`shelf::run_commit_checks`); what they find shows
//! in the box and the next Commit, Commit Anyway, skips them.
```

## 6. `app-gpui/src/vcs/commit_box.rs`: checks that do not run here

### 6a. The input

In `pub struct CommitBoxInput`, after

```rust
    /// Before-commit checks contributed by plugins.
    pub plugin_checks: Vec<PluginCheck>,
```

add:

```rust
    /// Checks that cannot run here, with why: their rows are disabled with the reason.
    pub unavailable_checks: Vec<(CommitCheck, String)>,
```

### 6b. The accessor

After `pub fn input(&self) -> &CommitBoxInput { … }` add:

```rust
    /// Why `check` cannot run here, when it cannot.
    pub fn unavailable_check(&self, check: CommitCheck) -> Option<&str> {
        self.input.unavailable_checks.iter().find(|(known, _)| *known == check).map(|(_, reason)| reason.as_str())
    }
```

### 6c. The rows

In `pub fn option_rows`, replace both

```rust
            rows.push((tx(cx, check.label()), check.on(&settings)));
```

with

```rust
            rows.push((tx(cx, check.label()), check.on(&settings) && self.unavailable_check(check).is_none()));
```

In `fn render_options`, replace

```rust
        let check = |check: CommitCheck, id: &'static str, cx: &mut Context<Self>| {
            let label = tx(cx, check.label());
            row(checkbox(cx, label, check.on(&settings)), id)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_check(check, cx)))
        };
```

with

```rust
        let unavailable = self.input.unavailable_checks.clone();
        let check = |check: CommitCheck, id: &'static str, cx: &mut Context<Self>| {
            let label = tx(cx, check.label());
            let reason = unavailable.iter().find(|(known, _)| *known == check).map(|(_, reason)| tx(cx, reason));
            match reason {
                // A check that cannot run shows unticked and greyed out, with why under it.
                Some(reason) => div()
                    .flex()
                    .flex_col()
                    .child(row(checkbox(cx, label, false), id).opacity(0.5))
                    .child(
                        div()
                            .pl(px(24.0))
                            .text_color(theme::color(cx, "fg-muted"))
                            .text_size(theme::length(cx, "fs-small"))
                            .child(SharedString::from(reason)),
                    )
                    .into_any_element(),
                None => row(checkbox(cx, label, check.on(&settings)), id)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_check(check, cx)))
                    .into_any_element(),
            }
        };
```

The call sites (`.child(check(CommitCheck::Reformat, "commit-check-reformat", cx))` …) take an `AnyElement` unchanged. `AnyElement` is imported there (`render_problems` uses it); if not, add it to the `gpui_kit` import.

## 7. `app-gpui/src/vcs/commit_view.rs`: change list groups, menus on every row, tabs, drag and drop

### 7a. Imports

- Add `ChangelistState` to the `use super::changes::{ … }` list.
- Add `AnyView` to the `gpui_kit::{ … }` list.
- Add `use crate::vcs_ops::git_tabs::ContentTab;`.

### 7b. `kind_color` for the sibling views

`fn kind_color(kind: EffectiveKind) -> &'static str {` → `pub(super) fn kind_color(kind: EffectiveKind) -> &'static str {`

### 7c. Events

In `pub enum CommitPanelEvent`, after `ShowMenu(gpui_kit::Point<gpui_kit::Pixels>),` add:

```rust
    /// Show content tab `id` (Commit, Shelf).
    SelectTab(String),
    /// Move the files at `paths` to change list `list` (they were dropped on its group).
    MoveToChangelist { paths: Vec<String>, list: String },
```

### 7d. Rows know their list

In `struct Row`, after `expanded: bool,` add `changelist: Option<String>,`. In `fn flatten`, in the `rows.push(Row { … })` literal, after `expanded,` add `changelist: node.changelist.clone(),`.

### 7e. The dragged changes

After `struct CommitParts { … }` add:

```rust
/// Changed files dragged onto a change list's group.
#[derive(Clone, Debug)]
pub struct DraggedChanges {
    pub paths: Vec<String>,
    label: String,
}

impl Render for DraggedChanges {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.0))
            .py(px(2.0))
            .rounded(theme::length(cx, "radius-small"))
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .text_color(theme::color(cx, "fg-strong"))
            .child(SharedString::from(self.label.clone()))
    }
}
```

### 7f. Fields

In `pub struct CommitPanel`, after `commit: Option<CommitParts>,` add:

```rust
    /// The change lists the changes are grouped by.
    lists: Option<ChangelistState>,
    /// The content tabs above the tree, the chosen one, and what a tab other than Commit shows in
    /// place of the changes.
    tabs: Vec<ContentTab>,
    tab: String,
    content: Option<AnyView>,
```

In `CommitPanel::new`, after `commit: None,` add:

```rust
            lists: None,
            tabs: Vec::new(),
            tab: "changes".to_owned(),
            content: None,
```

### 7g. Methods

After `pub fn selected_file_opens(&self) -> bool { … }` add:

```rust
    /// Groups the changes by `lists`.
    pub fn set_changelists(&mut self, lists: ChangelistState, cx: &mut Context<Self>) {
        if self.lists.as_ref() == Some(&lists) {
            return;
        }
        self.lists = Some(lists);
        self.rebuild(cx);
        cx.notify();
    }

    /// The change list of the selected row: a list's group or what is below it.
    pub fn selected_changelist(&self) -> Option<String> {
        let key = self.selected.as_deref()?;
        self.rows.iter().find(|row| row.key == key)?.changelist.clone()
    }

    /// The change list whose group row is selected.
    pub fn selected_list_group(&self) -> Option<String> {
        let key = self.selected.as_deref()?;
        self.rows.iter().find(|row| row.key == key && row.node_type == NodeType::Group)?.changelist.clone()
    }

    /// The paths, without a trailing slash, of the selected row's files.
    pub fn selected_paths(&self) -> Vec<String> {
        let Some(key) = self.selected.as_deref() else { return Vec::new() };
        let Some(row) = self.rows.iter().find(|row| row.key == key) else { return Vec::new() };
        row.files.iter().map(|file| strip_slash(&file.path).to_owned()).collect()
    }

    /// Shows `tabs` above the tree with `selected` chosen; `content` takes the place of the changes
    /// and the message box while a tab other than Commit is chosen.
    pub fn set_content_tabs(
        &mut self,
        tabs: Vec<ContentTab>,
        selected: &str,
        content: Option<AnyView>,
        cx: &mut Context<Self>,
    ) {
        let same = match (&self.content, &content) {
            (Some(shown), Some(new)) => shown.entity_id() == new.entity_id(),
            (None, None) => true,
            _ => false,
        };
        if same && self.tabs == tabs && self.tab == selected {
            return;
        }
        self.tabs = tabs;
        self.tab = selected.to_owned();
        self.content = content;
        cx.notify();
    }

    pub fn tabs(&self) -> &[ContentTab] {
        &self.tabs
    }

    pub fn selected_tab(&self) -> &str {
        &self.tab
    }

    /// What a tab other than Commit shows, while it is chosen.
    pub fn content(&self) -> Option<&AnyView> {
        self.content.as_ref()
    }
```

### 7h. The tree is grouped by the lists

In `fn rebuild`, replace

```rust
        self.tree = build_change_tree(&self.status, &self.root, true, None, &[], false, i18n);
```

with

```rust
        self.tree = build_change_tree(&self.status, &self.root, true, self.lists.as_ref(), &[], false, i18n);
```

### 7i. A menu on every row, row selectors, drag and drop

In `fn render_tree`, before `uniform_list(` add:

```rust
        let drop_bg = theme::color(cx, "bg-tree-selected");
```

In the row chain, replace

```rust
                        let (clicked, toggled, pressed) = (row.key.clone(), row.key.clone(), row.key.clone());
                        div()
                            .id(SharedString::from(format!("change:{}", row.key)))
```

with

```rust
                        let (clicked, toggled, pressed) = (row.key.clone(), row.key.clone(), row.key.clone());
                        let row_label = row.label.clone();
                        let dragged = (row.node_type != NodeType::Group && !row.files.is_empty()).then(|| DraggedChanges {
                            paths: row.files.iter().map(|file| strip_slash(&file.path).to_owned()).collect(),
                            label: row.label.clone(),
                        });
                        let target = row.changelist.clone().filter(|_| row.node_type == NodeType::Group);
                        div()
                            .id(SharedString::from(format!("change:{}", row.key)))
                            .debug_selector(move || format!("commit-row-{row_label}"))
                            .when_some(dragged, |el, dragged| {
                                el.on_drag(dragged, |dragged: &DraggedChanges, _, _, cx| cx.new(|_| dragged.clone()))
                            })
                            .when_some(target, |el, list| {
                                el.drag_over::<DraggedChanges>(move |style, _, _, _| style.bg(drop_bg)).on_drop(
                                    cx.listener(move |_, dragged: &DraggedChanges, _, cx| {
                                        let paths = dragged.paths.clone();
                                        cx.emit(CommitPanelEvent::MoveToChangelist { paths, list: list.clone() });
                                    }),
                                )
                            })
```

and in the right-button handler replace

```rust
                                    if panel.selected_file().is_some() {
                                        cx.emit(CommitPanelEvent::ShowMenu(event.position));
                                    }
```

with

```rust
                                    cx.emit(CommitPanelEvent::ShowMenu(event.position));
```

### 7j. The tab strip and the tab's content

Replace the whole `impl Render for CommitPanel { … }` with:

```rust
impl CommitPanel {
    /// The content tabs (Commit, Shelf), drawn as `commit-tab-<id>`.
    fn render_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let (fg, muted) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let accent = theme::color(cx, "bg-button-primary");
        let tabs: Vec<AnyElement> = self
            .tabs
            .iter()
            .map(|tab| {
                let selected = tab.id == self.tab;
                let label = cx.default_global::<Localization>().0.tx(&tab.label);
                let label = match tab.count {
                    Some(count) => format!("{label} {count}"),
                    None => label,
                };
                let (id, selector) = (tab.id.clone(), tab.id.clone());
                div()
                    .id(SharedString::from(format!("commit-tab-{}", tab.id)))
                    .debug_selector(move || format!("commit-tab-{selector}"))
                    .h(px(26.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .text_color(if selected { fg } else { muted })
                    .when(selected, |el| el.border_b_1().border_color(accent))
                    .child(SharedString::from(label))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(CommitPanelEvent::SelectTab(id.clone()))))
                    .into_any_element()
            })
            .collect();
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(4.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .children(tabs)
            .into_any_element()
    }
}

impl Render for CommitPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let strip = (self.tabs.len() > 1).then(|| self.render_tabs(cx));
        if let Some(content) = self.content.clone() {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .text_size(theme::length(cx, "fs-ui"))
                .children(strip)
                .child(div().flex_1().min_h_0().w_full().child(content))
                .into_any_element();
        }
        let tree = self.render_tree(window, cx);
        // Only the tree takes the keyboard's keys: what is typed in the message box stays there.
        div()
            .size_full()
            .flex()
            .flex_col()
            .text_size(theme::length(cx, "fs-ui"))
            .children(strip)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::key_down))
                    .child(tree),
            )
            .children(self.commit.as_ref().map(|parts| {
                div()
                    .flex_none()
                    .w_full()
                    .border_t_1()
                    .border_color(theme::color(cx, "border"))
                    .child(parts.commit_box.clone())
            }))
            .into_any_element()
    }
}
```

### 7k. The view's own test

In `lists_the_repositorys_changes_by_group_and_directory`, replace

```rust
                CommitPanelEvent::ShowMenu(_) => {}
```

with

```rust
                CommitPanelEvent::ShowMenu(_) | CommitPanelEvent::SelectTab(_) | CommitPanelEvent::MoveToChangelist { .. } => {}
```

## 8. `app-gpui/src/workbench/diff_window.rs`

### 8a. The tree's events

In `pub(super) fn commit_panel_event`, replace

```rust
            CommitPanelEvent::ShowMenu(position) => self.show_change_menu(*position, window, cx),
```

with

```rust
            CommitPanelEvent::ShowMenu(position) => self.show_changes_menu(*position, window, cx),
            CommitPanelEvent::SelectTab(tab) => self.select_commit_tab(tab, window, cx),
            CommitPanelEvent::MoveToChangelist { paths, list } => self.move_dropped_changes(paths, list, cx),
```

### 8b. The old menu goes

Delete the whole `fn show_change_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) { … }` with its doc comment. Its rows (Show Diff, Jump to Source) are in `shelf::node_menu` now. Then remove what only it used: `use crate::vcs_ops::menu::MenuItem;`, and `Pixels, Point` from the `gpui_kit::{ … }` import (check with a build: `show_diff_settings` uses `gpui_kit::point`, not `Point`).

### 8c. Show Diff in the Shelf tab

In `pub fn show_diff`, replace

```rust
        let paths = match show_diff_start(arg, context, false) {
            ShowDiffStart::Paths(paths) => paths,
            ShowDiffStart::Nothing(notice) => return self.notify(notice, cx),
            // There is no Shelf view to have the focus.
            ShowDiffStart::Shelf => return,
        };
```

with

```rust
        let paths = match show_diff_start(arg, context, self.shelf_focused(window, cx)) {
            ShowDiffStart::Paths(paths) => paths,
            ShowDiffStart::Nothing(notice) => return self.notify(notice, cx),
            // The Shelf tab has the keyboard: the diff of its selection.
            ShowDiffStart::Shelf => return self.show_shelf_diff(None, false, false, window, cx),
        };
```

and in the `ChangesView { … }` literal below it replace `lists: None,` with `lists: Some(self.changelists()),` (the chain of a diff stays within the file's change list, as the tree groups it).

## 9. `app-gpui/src/workbench/code_insight.rs`: what Analyze code counts

After `pub fn problems_badge(&self) -> Option<usize> { … }` add:

```rust
    /// Every file's problems as their producers reported them, before the inspection profile.
    pub(super) fn problems_map(&self) -> &problems::ProblemsMap {
        self.code_insight.problems.raw()
    }
```

## 10. Menus

`menu_spec.rs` already lists `vcs.createPatch`, `vcs.applyPatch`, `vcs.applyPatchFromClipboard` (Git › Patch) and `vcs.shelve`, `vcs.showShelf` (Git › Uncommitted Changes): nothing to add. The changes tree's menus are `shelf::list_menu` (a list's header) and `shelf::node_menu` (files, folders, groups); the Shelf tab's is `shelf::shelf_menu`. Rows naming actions other agents register (`vcs.rollback`, `vcs.addToVcs`, `vcs.showLog`, `vcs.localHistory`, `vcs.annotate`) appear once those are registered: `ui::popup_menu::resolve` drops unregistered actions.

## 11. IPC commands each action stands for

- `vcs.changelist.new|edit|delete|setActive|move`: `vcsChangelistsSave` (250 ms after a change), loaded with `vcsChangelistsLoad` when the project opens.
- `vcs.changelist.commit`: `vcsCommit` (through `vcs.commit`).
- `vcs.shelve`: `vcsShelve`, then `vcsShelves` and `vcsShelfDiff`.
- `vcs.showShelf`, `vcs.shelf.refresh`: `vcsShelves`, `vcsShelfDiff`.
- `vcs.unshelve`: `vcsUnshelve`.
- `vcs.shelf.delete`: `vcsShelfDelete`.
- `vcs.shelf.rename`: `vcsShelfRename`.
- `vcs.shelf.showDiff|showDiffNewTab`: `vcsShelfDiff`.
- `vcs.shelf.compareLocal`: `vcsShelfDiff`, `fsProbe`.
- `vcs.shelf.copyPatch`: `vcsShelfPatch`.
- `vcs.shelf.createPatch`: `vcsShelfPatch`, `vcsWritePatch`.
- `vcs.shelf.importPatches`: `fsReadText`, `vcsShelfImport`.
- `vcs.createPatch`: `vcsCreatePatch`, `vcsWritePatch`.
- `vcs.applyPatch|applyPatchFromClipboard`: `fsReadText`, `vcsPatchPreview`, `vcsApplyPatch`.
- Commit checks: `fsReadDir`, `docOpen`, `docClose`, `vcsFileAt`, `fsReadText`, `vcsDiffHunks` (called as `ide_vcs::diff_hunks`), `vcsConfigGet`.

## 12. What is not wired, and why

- **Reformat code, Optimize imports, Cleanup.** These rows are disabled with the reason (6c). The source applies `source.organizeImports` / `source.fixAll` code actions and `lspFormat`, filtered through `formatGuard` (Do-not-format globs, guarded edits). No `formatGuard` model exists here, and no test language server can prove it, so they do not run.
- **CRLF check.** It reads the file's stored bytes (`ide_vfs::load_text`). The source calls `fsReadText`, which in a project returns `Project::text_of`, and that normalises line breaks. So the source's check never fires for a project file. This is a deliberate deviation.
- **Analyze code.** It counts the problems the window holds per file and opens each file no producer has reported on, waiting up to 2 s. The source also treats a file that got an empty diagnostics event as analysed; `ProblemsMap` drops empty reports, so such a file is opened again and waited on.
- **Check TODO's file links.** The hits are listed as `rel:line text` in the problem text. `CommitProblem` has one details link, not one per hit.
- **Show Diff in a New Tab.** It opens in the diff window. This build has no editor-tab diffs.
- **The modal commit interface.** It has no Git-window Shelf tab, so Show Shelf uses the Commit tool window.
- **The Stash tab and "Combine stashes and shelves".** Those tabs are the stash agent's. `sync_commit_tabs` offers Commit and Shelf only.
- **Shelf rows' time.** The shelf rows omit the time (`formatTime`) because there are no date-format options to plumb here.
- **Select Target Changelist (Undo Commit).** It is the log agent's. Its dialog is `ChangelistDialog` with `ChangelistDialogMode::Target` and a suggested name.
- **The changes tree's Accept Yours / Theirs and Delete… rows.** These are other agents' actions.

## 13. Assumptions to check first

1. `vcs_dialog_view`, `open_vcs_dialog`, `close_vcs_dialog`, `show_prompt_dialog`, `run_op`, `call`, `save_all`, `notify`, `vcs_state`, `finished`, `run_action_later` and `This` are `pub(super)` in `branches.rs`. They are as read.
2. `local_changes_aside` (keep the lists of files that are put aside) approximates the source's `gitWork > 0` with `remote_busy`, `operations` and `branch.operation`.
3. `ide_vcs::shelf::import_patch` names the shelf after the file (the test only asserts the name starts with "fix").
4. GPUI's test platform answers `prompt_for_new_path` through `simulate_new_path_selection` after the prompt was made (test `create_patch_from_selected_changes…`).
5. The backend's git runs without `GIT_CONFIG_GLOBAL=/dev/null`. A user `core.autocrlf` would silence the CRLF test, as with the existing tests' reliance on the user's git.
6. `AnyView::entity_id`, `when_some`, `drag_over`, `on_drag` and `on_drop` exist in `gpui-pre-0.3.12/src/elements/div.rs` and `view.rs` (read). `on_drag`, `on_drop` and `drag_over` have no prior use in the crate.

## 14. Tests most likely to fail first

- `a_change_dragged_onto_another_list_moves_there`: the first drag in the crate. GPUI starts a drag only after the pressed mouse moves; if it needs more than the 8 px step, add moves.
- `line_separators_and_a_detached_head_…`: relies on `branch.head == None` when detached, and on the user's git config (5).
- Existing commit tests in `mod.rs`: every commit now runs the checks. Analyze code opens the committed `.txt` files and waits up to 2 s on the executor's clock. Tests that wait with `wait_long` are fine; one that only calls `run_until_parked` after a commit click would hang. `grep -n 'click_on(cx, "commit-commit")\|COMMIT_KEYS' app-gpui/src/workbench/mod.rs` and check each waits with `wait_long`.
- `unshelve_restores_…`: assumes `delete_path` with a file removes only that file from the shelf.
- `a_patch_that_does_not_apply_shows_the_error`: assumes `git apply --3way` fails without conflicts for a patch on a missing file with unknown blobs. The notice is then "Apply patch (bad.patch) failed".
