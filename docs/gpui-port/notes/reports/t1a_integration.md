# T1-AGENT-UNDO and T1-TEMPLATES-UP: integration notes

Written without a compiler against `HEAD` (76bde66d6). Every anchor below is copied from `git show HEAD:<file>`; if the working tree moved, find the same text. Each item lands alone, in the order given; the `mod` lines of a slice's tests go with the item whose tests they are.

New files (all untracked):

- Slice A: `app-gpui/src/workbench/tests/proofs39_agent_undo.rs` (end to end, 4 tests); `app-gpui/src/workbench/tests/proofs39_agent_undo_pending.rs` (1 test, not declared: waits on the per-editor marker model)
- M1: `app-gpui/src/templates/proofs39_completion_enter.rs` (model), `app-gpui/src/editor/intelligence/tests/proofs39_completion_snippet.rs` (end to end)
- M2: `app-gpui/src/templates/postfix_action.rs` (model), `app-gpui/src/workbench/editor_templates/postfix_action.rs` (wiring, a child of `editor_templates`), `app-gpui/src/templates/proofs39_java_var.rs` (model tests), `app-gpui/src/workbench/tests/proofs39_java_var.rs` (end to end), `app-gpui/src/settings_view/pages/editor_postfix_completion/tests/proofs39_java_var_row.rs` (page)
- M3: `app-gpui/src/templates/postfix_model/zig.rs` (data), `app-gpui/src/templates/proofs39_zig_postfix.rs` (model tests), `app-gpui/src/settings_view/pages/editor_postfix_completion/tests/proofs39_zig_tab.rs` (page)
- M4: `app-gpui/src/templates/live_builtins/zig.rs` (data), `app-gpui/src/templates/proofs39_zig_live.rs` (model tests), `app-gpui/src/settings_view/pages/editor_live_templates/tests/proofs39_zig_group.rs` (page)
- M5: `app-gpui/src/templates/live_surrounders/zig.rs` (data), `app-gpui/src/templates/proofs39_zig_surrounders.rs` (model tests)
- M8: `app-gpui/src/templates/proofs39_django_emmet.rs` (model test)

## (a) Hunks for existing files

### Slice A: an agent's workspace edit is recorded for Edit › Undo

A1. `app-gpui/src/workbench/editor_language.rs`, in `watch_workspace_edits`. Anchor (replace the whole block):

```rust
            // An agent's edit through the MCP tools is undoable but not announced as a refactoring.
            if let IdeEvent::WorkspaceEditApplied { id, paths, by_agent: false } = &event.event
                && event.window.as_deref().is_none_or(|window| window == label)
            {
                let paths: Vec<String> = paths.iter().map(|path| to_ipc(&path.0)).collect();
                let id = *id;
                let _ = this.update(cx, |workbench, cx| workbench.workspace_edit_applied(id, paths, cx));
            }
```

New:

```rust
            // An agent's edit through the MCP tools is undoable but not announced as a refactoring.
            if let IdeEvent::WorkspaceEditApplied { id, paths, by_agent } = &event.event
                && event.window.as_deref().is_none_or(|window| window == label)
            {
                let paths: Vec<String> = paths.iter().map(|path| to_ipc(&path.0)).collect();
                let (id, by_agent) = (*id, *by_agent);
                let _ = this.update(cx, |workbench, cx| workbench.workspace_edit_applied(id, paths, by_agent, cx));
            }
```

A2. Same file. Anchor:

```rust
    fn workspace_edit_applied(&mut self, id: u32, paths: Vec<String>, cx: &mut Context<Self>) {
        let applied = &mut self.language.applied;
        // A new edit drops the redo of an undone one it overlaps.
        applied.retain(|edit| !(edit.undone && paths.iter().any(|path| edit.affects(path))));
        applied.push(AppliedEdit { id, paths, undone: false });
        if applied.len() > REMEMBERED_EDITS {
            applied.remove(0);
        }
        let project = self.undo_project(cx);
```

New:

```rust
    /// The backend applied workspace edit `id` to `paths`: Edit › Undo in an editor of one of them
    /// takes it back. An agent's edit (`by_agent`) raises no notification: an agent makes many.
    fn workspace_edit_applied(&mut self, id: u32, paths: Vec<String>, by_agent: bool, cx: &mut Context<Self>) {
        let applied = &mut self.language.applied;
        // A new edit drops the redo of an undone one it overlaps.
        applied.retain(|edit| !(edit.undone && paths.iter().any(|path| edit.affects(path))));
        applied.push(AppliedEdit { id, paths, undone: false });
        if applied.len() > REMEMBERED_EDITS {
            applied.remove(0);
        }
        if by_agent {
            return;
        }
        let project = self.undo_project(cx);
```

Nothing else changes: an agent's id is never the `WorkspaceUndo` notification's current entry, so `history()` answers `None` for it in `step_refactoring` and no "Refactoring undone" follows its Undo (as `workspaceUndo.ts:44-50` filters the history event by the notification's id).

A3 (optional, docs only). Same file, module comment. Anchor:

```rust
//! and comes back to the window; what a server answers is turned into edits, hints, popups and
//! dialogs by the models of [`crate::editor_lsp`] and [`crate::editor_refactor`]. A refactoring the
//! backend applied across files (`WorkspaceEditApplied`) is remembered so that it is undone as one
```

New third line: `//! dialogs by the models of [`crate::editor_lsp`] and [`crate::editor_refactor`]. A refactoring or an agent's edit the` and reflow the paragraph (rustfmt does not reflow comments; keep lines under 120).

A4. `app-gpui/src/workbench/mod.rs`, inside `mod tests`. Anchor:

```rust
    mod proofs35_settings_vim;
```

New (after it):

```rust
    mod proofs39_agent_undo;
```

A5. `app-gpui/src/workbench/mod.rs`, in `open_project`, where the project has opened. The listener is installed today only with the first editor (`editor_intelligence.rs:24`), so an agent's edit made while no editor is open is never remembered; the webview starts its history with the editor feature (`src/editor/core/index.ts:42-44` → `controller.ts:327-328` → `document.ts:761-763` `workspaceHistory.start()`). `watch_workspace_edits` returns at once when its listener exists, so calling it again is harmless. Anchor:

```rust
                        menu::refresh_recent(cx);
                        workbench.load_bookmarks(&root.to_string_lossy(), cx);
```

New:

```rust
                        menu::refresh_recent(cx);
                        workbench.load_bookmarks(&root.to_string_lossy(), cx);
                        // Workspace edits are remembered from the start, those made before an editor opens too.
                        workbench.watch_workspace_edits(cx);
```

Proved by `workbench::tests::proofs39_agent_undo::an_agents_edit_made_before_any_editor_opened_is_undone_from_the_editor_opened_later` (fails without A5).

### Slice B, M1: Enter in a completion's snippet is the editor's

B1.1. `app-gpui/src/templates/expand.rs`. Anchor:

```rust
    /// The expansion's own edit is reported by the editor after the session started; the fields
    /// are already where that edit put them, so its report must not move them again.
    own_edits: usize,
}
```

New:

```rust
    /// The expansion's own edit is reported by the editor after the session started; the fields
    /// are already where that edit put them, so its report must not move them again.
    own_edits: usize,
    /// The fields are a live template's, where Enter moves to the next field; a completion item's
    /// snippet leaves Enter to the editor.
    from_template: bool,
}
```

B1.2. Same file. Anchor:

```rust
        Self { fields, completing: Vec::new(), own_edits: 0 }
```

New:

```rust
        Self { fields, completing: Vec::new(), own_edits: 0, from_template: false }
```

B1.3. Same file, `step`. Anchor:

```rust
    pub fn step(&self, key: FieldKey, lookup_open: bool) -> Option<FieldStep> {
        let moved = field_key(&self.fields, key, lookup_open)?;
```

New:

```rust
    pub fn step(&self, key: FieldKey, lookup_open: bool) -> Option<FieldStep> {
        if key == FieldKey::Enter && !self.from_template {
            return None;
        }
        let moved = field_key(&self.fields, key, lookup_open)?;
```

and in the same function the anchor:

```rust
            session: moved.active.map(|fields| Self { fields, completing: self.completing.clone(), own_edits: 0 }),
```

New:

```rust
            session: moved.active.map(|fields| Self {
                fields,
                completing: self.completing.clone(),
                own_edits: 0,
                from_template: self.from_template,
            }),
```

Also extend the doc comment of `step` (anchor `/// \`None\` leaves the key to the editor: the lookup takes Tab and Enter while it is open, and`): append the sentence "Enter is the editor's in a completion item's snippet." to that paragraph.

B1.4. Same file, `TemplateEdit::new`. Anchor:

```rust
        let session = insertion.active.map(|fields| TemplateSession { fields, completing, own_edits: 1 });
```

New:

```rust
        let session =
            insertion.active.map(|fields| TemplateSession { fields, completing, own_edits: 1, from_template: true });
```

B1.5. `app-gpui/src/editor/intelligence.rs`, inside `mod tests`. Anchor:

```rust
    mod proofs33_template_keys;
```

New (after it):

```rust
    mod proofs39_completion_snippet;
```

B1.6. `app-gpui/src/templates/mod.rs`. Anchor:

```rust
#[cfg(test)]
mod proofs34_update_tag_dialog;
```

New (after it; the other `proofs39_*` lines of B2 to B8 go after this one, in any order):

```rust
#[cfg(test)]
mod proofs39_completion_enter;
```

### Slice B, M2: Java `.var` runs Introduce Variable

B2.1. `app-gpui/src/templates/postfix_model.rs`, struct. Anchor:

```rust
    /// Wrap compound expressions (`a + b`) in parentheses for `$EXPR$`.
    pub wrap_compound: bool,
}

/// A template before its set gives it an id and languages.
struct Def {
    key: &'static str,
    description: &'static str,
    body: &'static str,
    variables: Vec<TemplateVariable>,
    statement: bool,
    wrap_compound: bool,
}

/// A template offered for any expression.
fn expr(key: &'static str, description: &'static str, body: &'static str) -> Def {
    Def { key, description, body, variables: Vec::new(), statement: false, wrap_compound: false }
}
```

New:

```rust
    /// Wrap compound expressions (`a + b`) in parentheses for `$EXPR$`.
    pub wrap_compound: bool,
    /// An editor action run on the selected expression instead of inserting `body` (Java's `.var`
    /// is Introduce Variable); `body` stays for when the action is not available.
    pub action: Option<&'static str>,
}

/// A template before its set gives it an id and languages.
struct Def {
    key: &'static str,
    description: &'static str,
    body: &'static str,
    variables: Vec<TemplateVariable>,
    statement: bool,
    wrap_compound: bool,
    action: Option<&'static str>,
}

/// A template offered for any expression.
fn expr(key: &'static str, description: &'static str, body: &'static str) -> Def {
    Def { key, description, body, variables: Vec::new(), statement: false, wrap_compound: false, action: None }
}
```

B2.2. Same file, `impl Def`. Anchor:

```rust
    fn var(mut self, variable: TemplateVariable) -> Self {
        self.variables.push(variable);
        self
    }
}
```

New:

```rust
    fn var(mut self, variable: TemplateVariable) -> Self {
        self.variables.push(variable);
        self
    }

    /// Runs the editor action `action` on the expression instead of inserting the text.
    fn action(mut self, action: &'static str) -> Self {
        self.action = Some(action);
        self
    }
}
```

B2.3. Same file, `set`. Anchor:

```rust
            statement: d.statement,
            wrap_compound: d.wrap_compound,
        })
        .collect()
```

New:

```rust
            statement: d.statement,
            wrap_compound: d.wrap_compound,
            action: d.action,
        })
        .collect()
```

B2.4. Same file, `java()`. Anchor:

```rust
            stmt("var", "var name = expr;", "var $NAME$ = $EXPR$;$END$").var(name()),
```

New (the language server declares the expression's type, as IDEA does by default):

```rust
            stmt("var", "T name = expr;", "var $NAME$ = $EXPR$;$END$")
                .var(name())
                .action("refactor.extractVariable"),
```

B2.5. `app-gpui/src/templates/mod.rs`. Anchor:

```rust
pub mod popup_store;
pub mod postfix_model;
```

New:

```rust
pub mod popup_store;
pub mod postfix_action;
pub mod postfix_model;
```

B2.6. `app-gpui/src/templates/editor.rs`, imports. Anchor:

```rust
use super::postfix_store::PostfixSettings;
```

New:

```rust
use super::postfix_action::PostfixAction;
use super::postfix_store::PostfixSettings;
```

B2.7. Same file, `TemplateExpansion`. Anchor:

```rust
    /// Select `from..to` before applying (Surround With keeps the wrapped text selected).
    pub select_range: bool,
}
```

New:

```rust
    /// Select `from..to` before applying (Surround With keeps the wrapped text selected).
    pub select_range: bool,
    /// The editor action the postfix template stands for, run instead of inserting the text.
    pub postfix_action: Option<PostfixAction>,
}
```

B2.8. Same file, `TemplateExpansion::new`. Anchor:

```rust
            options: ExpandOptions::default(),
            select_range: false,
        }
```

New:

```rust
            options: ExpandOptions::default(),
            select_range: false,
            postfix_action: None,
        }
```

B2.9. Same file, `PendingChoice`. Anchor:

```rust
    PostfixExpression {
        template: PostfixTemplate,
        target_end: u32,
        candidates: Vec<PostfixCandidate>,
    },
```

New:

```rust
    PostfixExpression {
        template: PostfixTemplate,
        /// Offset of the `.` before the key.
        dot: u32,
        target_end: u32,
        candidates: Vec<PostfixCandidate>,
    },
```

B2.10. Same file, `postfix_expansion`. Anchor (whole function):

```rust
fn postfix_expansion(template: &PostfixTemplate, candidate: &PostfixCandidate, end: u32) -> TemplateExpansion {
    let wrap = template.wrap_compound && is_compound(&candidate.text);
    let expr = if wrap { format!("({})", candidate.text) } else { candidate.text.clone() };
    let mut expansion = TemplateExpansion::new(&template.body, &template.variables, candidate.from, end);
    expansion.options.predefined.insert("EXPR".to_owned(), expr);
    expansion.options.expression = Some(candidate.text.clone());
    expansion
}
```

New:

```rust
fn postfix_expansion(
    template: &PostfixTemplate,
    candidate: &PostfixCandidate,
    dot: u32,
    end: u32,
) -> TemplateExpansion {
    let wrap = template.wrap_compound && is_compound(&candidate.text);
    let expr = if wrap { format!("({})", candidate.text) } else { candidate.text.clone() };
    let mut expansion = TemplateExpansion::new(&template.body, &template.variables, candidate.from, end);
    expansion.options.predefined.insert("EXPR".to_owned(), expr);
    expansion.options.expression = Some(candidate.text.clone());
    expansion.postfix_action =
        template.action.map(|action| PostfixAction { action, dot, statement: template.statement });
    expansion
}
```

B2.11. Same file, `PendingChoice::pick`. Anchor:

```rust
            Self::PostfixExpression { template, target_end, candidates } => {
                match key.parse::<usize>().ok().and_then(|index| candidates.get(index)) {
                    // The key may have grown since the target was computed.
                    Some(candidate) => Picked::Expand(postfix_expansion(template, candidate, (*target_end).max(head))),
```

New:

```rust
            Self::PostfixExpression { template, dot, target_end, candidates } => {
                match key.parse::<usize>().ok().and_then(|index| candidates.get(index)) {
                    // The key may have grown since the target was computed.
                    Some(candidate) => {
                        Picked::Expand(postfix_expansion(template, candidate, *dot, (*target_end).max(head)))
                    }
```

B2.12. Same file, `apply_postfix`. Anchors:

```rust
        [only] => Some(TemplateAction::Expand(postfix_expansion(template, only, target.end.max(head)))),
```

New:

```rust
        [only] => Some(TemplateAction::Expand(postfix_expansion(template, only, target.dot, target.end.max(head)))),
```

and

```rust
                pending: PendingChoice::PostfixExpression {
                    template: template.clone(),
                    target_end: target.end,
```

New:

```rust
                pending: PendingChoice::PostfixExpression {
                    template: template.clone(),
                    dot: target.dot,
                    target_end: target.end,
```

B2.13. `app-gpui/src/workbench/editor_templates.rs`, the child module. Anchor:

```rust
use std::collections::HashMap;
use std::ops::Range;
```

New:

```rust
use std::collections::HashMap;
use std::ops::Range;

mod postfix_action;
```

B2.14. Same file, `template_action`. Anchor:

```rust
            TemplateAction::Expand(expansion) => self.expand_template(editor, &expansion, cx),
```

New:

```rust
            TemplateAction::Expand(expansion) => self.expand_or_run_action(editor, expansion, window, cx),
```

B2.15. Same file, `template_chooser_event`. Anchor:

```rust
            Picked::Expand(expansion) => self.expand_template(&editor, &expansion, cx),
```

New:

```rust
            Picked::Expand(expansion) => self.expand_or_run_action(&editor, expansion, window, cx),
```

B2.16. `app-gpui/src/templates/mod.rs`, after the anchor of B1.6 (`#[cfg(test)]` / `mod proofs34_update_tag_dialog;`):

```rust
#[cfg(test)]
mod proofs39_java_var;
```

B2.17. `app-gpui/src/workbench/mod.rs`, inside `mod tests`, after the A4 line (or after `    mod proofs35_settings_vim;` when A4 has not landed):

```rust
    mod proofs39_java_var;
```

B2.18. `app-gpui/src/settings_view/pages/editor_postfix_completion.rs`, inside `mod tests`. Anchor:

```rust
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};

    fn page(cx: &mut TestAppContext) -> (Entity<PostfixPageView>, Seen, &mut VisualTestContext) {
```

New:

```rust
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};

    mod proofs39_java_var_row;

    fn page(cx: &mut TestAppContext) -> (Entity<PostfixPageView>, Seen, &mut VisualTestContext) {
```

### Slice B, M3: the Zig postfix templates and their tab

B3.1. `app-gpui/src/templates/postfix_model.rs`. Anchor:

```rust
use std::collections::BTreeSet;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostfixTemplate {
```

New:

```rust
use std::collections::BTreeSet;
use std::sync::LazyLock;

mod zig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostfixTemplate {
```

B3.2. Same file. Anchor:

```rust
        LazyLock::new(|| [ts_like(), rust(), python(), go(), java(), kotlin()].concat());
    &TEMPLATES
}

/// The language sets, as the settings page titles them: `(id prefix, title)`.
pub const POSTFIX_GROUPS: [(&str, &str); 6] = [
    ("ts", "JavaScript / TypeScript"),
    ("rust", "Rust"),
    ("py", "Python"),
    ("go", "Go"),
    ("java", "Java"),
```

New:

```rust
        LazyLock::new(|| [ts_like(), rust(), python(), go(), zig::zig(), java(), kotlin()].concat());
    &TEMPLATES
}

/// The language sets, as the settings page titles them: `(id prefix, title)`.
pub const POSTFIX_GROUPS: [(&str, &str); 7] = [
    ("ts", "JavaScript / TypeScript"),
    ("rust", "Rust"),
    ("py", "Python"),
    ("go", "Go"),
    ("zig", "Zig"),
    ("java", "Java"),
```

B3.3. Same file, tests (expectations change). Anchor:

```rust
        for lang in ["typescript", "rust", "python", "go", "java", "kotlin"] {
```

New:

```rust
        for lang in ["typescript", "rust", "python", "go", "zig", "java", "kotlin"] {
```

and anchor:

```rust
        assert_eq!(postfix_templates().len(), 99);
```

New (12 Zig templates):

```rust
        assert_eq!(postfix_templates().len(), 111);
```

B3.4. `app-gpui/src/settings_view/pages/editor_postfix_completion.rs`, tests (expectations change). Anchor:

```rust
        assert_eq!(lines[4], "[JavaScript / TypeScript] | Rust | Python | Go | Java | Kotlin");
```

New:

```rust
        assert_eq!(lines[4], "[JavaScript / TypeScript] | Rust | Python | Go | Zig | Java | Kotlin");
```

and anchor:

```rust
        assert_eq!(lines[4], "JavaScript / TypeScript | [Rust] | Python | Go | Java | Kotlin");
```

New:

```rust
        assert_eq!(lines[4], "JavaScript / TypeScript | [Rust] | Python | Go | Zig | Java | Kotlin");
```

B3.5. Test declarations. `app-gpui/src/templates/mod.rs`, after the anchor of B1.6:

```rust
#[cfg(test)]
mod proofs39_zig_postfix;
```

and `app-gpui/src/settings_view/pages/editor_postfix_completion.rs`, inside `mod tests`, after the anchor `    use gpui_kit::{Entity, TestAppContext, VisualTestContext};` (next to B2.18's line when it has landed):

```rust
    mod proofs39_zig_tab;
```

### Slice B, M4: the Zig live templates

B4.1. `app-gpui/src/templates/live_builtins.rs`. Anchor:

```rust
use std::collections::BTreeSet;
use std::sync::LazyLock;

/// The languages sharing the JavaScript / TypeScript templates.
```

New:

```rust
use std::collections::BTreeSet;
use std::sync::LazyLock;

mod zig;

/// The languages sharing the JavaScript / TypeScript templates.
```

B4.2. Same file. Anchor:

```rust
        [rust(), ts(), python(), go(), java(), kotlin(), php(), sql(), general(), surround()].concat()
```

New:

```rust
        [rust(), ts(), python(), go(), zig::zig(), java(), kotlin(), php(), sql(), general(), surround()].concat()
```

B4.3. Same file, `ships_the_requested_built_ins` (expectation changes, `live.test.ts:152`). Anchor:

```rust
        for a in ["iferr", "fori", "func", "test"] {
            assert!(has("go", a), "go {a}");
        }
```

New:

```rust
        for a in ["iferr", "fori", "func", "test"] {
            assert!(has("go", a), "go {a}");
        }
        for a in ["pfn", "test", "st", "iferr", "defer", "for"] {
            assert!(has("zig", a), "zig {a}");
        }
```

B4.4. `app-gpui/src/settings_view/pages/editor_live_templates/tests.rs`. Anchor:

```rust
use gpui_kit::{Focusable, TestAppContext, VisualTestContext};
use serde_json::{Value, json};
```

New (after it):

```rust

mod proofs39_zig_group;
```

B4.5. `app-gpui/src/templates/mod.rs`, after the anchor of B1.6:

```rust
#[cfg(test)]
mod proofs39_zig_live;
```

### Slice B, M5: the Zig surrounders

B5.1. `app-gpui/src/templates/live_surrounders.rs`. Anchor:

```rust
use std::collections::BTreeSet;
use std::sync::LazyLock;

/// What a surrounder wraps.
```

New:

```rust
use std::collections::BTreeSet;
use std::sync::LazyLock;

mod zig;

/// What a surrounder wraps.
```

B5.2. Same file, `all_for`. Anchor:

```rust
    static GO: LazyLock<Vec<Surrounder>> = LazyLock::new(go);
    static RUST: LazyLock<Vec<Surrounder>> = LazyLock::new(rust);
```

New:

```rust
    static GO: LazyLock<Vec<Surrounder>> = LazyLock::new(go);
    static ZIG: LazyLock<Vec<Surrounder>> = LazyLock::new(zig::zig);
    static RUST: LazyLock<Vec<Surrounder>> = LazyLock::new(rust);
```

and anchor:

```rust
        "go" => GO.as_slice(),
        "rust" => RUST.as_slice(),
```

New:

```rust
        "go" => GO.as_slice(),
        "zig" => ZIG.as_slice(),
        "rust" => RUST.as_slice(),
```

B5.3. `app-gpui/src/templates/mod.rs`, after the anchor of B1.6:

```rust
#[cfg(test)]
mod proofs39_zig_surrounders;
```

### Slice B, M8: Emmet in Django templates (model only)

B8.1. `app-gpui/src/templates/emmet_model.rs`. Anchor:

```rust
const MARKUP_LANGUAGES: [&str; 3] = ["html", "vue", "svelte"];
```

New:

```rust
const MARKUP_LANGUAGES: [&str; 4] = ["html", "vue", "svelte", "django-html"];
```

B8.2. Same file, `syntax_at`. Anchor:

```rust
    if after(text.rfind("<!--"), text.rfind("-->")) || open_region(text, "script").is_some() {
        return None;
    }
```

New:

```rust
    if after(text.rfind("<!--"), text.rfind("-->")) || open_region(text, "script").is_some() {
        return None;
    }
    // Not inside a Django tag, variable or comment (`{% if user|`).
    static DJANGO_OPEN: LazyLock<Regex> = LazyLock::new(|| pattern(r"\{[%{#]"));
    static DJANGO_CLOSE: LazyLock<Regex> = LazyLock::new(|| pattern(r"[%}#]\}"));
    let last = |found: &Regex| found.find_iter(text).last().map(|at| at.start());
    if language == "django-html" && after(last(&DJANGO_OPEN), last(&DJANGO_CLOSE)) {
        return None;
    }
```

(`find_iter` scans left to right without overlaps, as the webview's `matchAll` does, so `{{{` and `}}}` count the same.) The existing `has_emmet` assertion (`emmet_model.rs:613`) is unaffected.

B8.3. `app-gpui/src/templates/mod.rs`, after the anchor of B1.6:

```rust
#[cfg(test)]
mod proofs39_django_emmet;
```

## (b) Parity rows

The keys as `docs/gpui-port/parity.json` spells them: `event:WorkspaceEditApplied` (not `evt:`, as the brief and `WORKPLAN.md` write it) and `settings:editor.general.postfixCompletion` (not `set:`, as `WORKPLAN.md` writes it).

| Key                                                         | Status at HEAD | Proposed | Proof to write                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| ----------------------------------------------------------- | -------------- | -------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `event:WorkspaceEditApplied`                                | partial        | partial  | the rename test plus `workbench::tests::proofs39_agent_undo::does_not_announce_an_agents_edit_as_a_refactoring`, `…::edit_undo_takes_back_an_agents_edit_over_two_files_and_redo_applies_it_again`, `…::reports_a_refused_undo_instead_of_discarding_newer_work`, `…::an_agents_edit_made_before_any_editor_opened_is_undone_from_the_editor_opened_later`: an agent's edit is applied without a notification, remembered also with no editor open, and Edit › Undo / Redo take it back and put it again over both files, on disk and in both editors; a refused Undo keeps both files and names the changed one. Missing: an Undo started in one editor while another editor of the edit has newer typing is refused by the backend's message, not "Undo newer editor changes before undoing this refactoring" (`proofs39_agent_undo_pending.rs`); a workspace edit clears an open editor's own history, so typing made before it can no longer be undone (`workspaceHistory.test.ts` "preserves earlier local history…"); both need `editor_model::workspace_history` wired into the editors |
| `unit:src/features/code-insight/workspaceUndo.test.ts`      | partial        | done     | the three existing `code_insight::workspace_undo::tests::*` plus `workbench::tests::proofs39_agent_undo::does_not_announce_an_agents_edit_as_a_refactoring`; 4 of 4                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `unit:src/features/templates/__tests__/editor.test.ts`      | partial        | done     | the 14 existing plus the 3 below; 17 of 17                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `unit:src/features/templates/__tests__/postfix.test.ts`     | partial        | done     | the 8 existing, `templates::postfix_model::tests::every_language_has_the_core_keys` (with B3.3), `templates::proofs39_zig_postfix::zig_optional_unwrap_and_expansion`; 10 of 10                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `unit:src/features/templates/__tests__/live.test.ts`        | partial        | done     | the 18 names of the proof text, `templates::live_builtins::tests::ships_the_requested_built_ins` checking zig after B4.3; 18 of 18                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `unit:src/features/templates/__tests__/surrounders.test.ts` | partial        | done     | the 3 existing plus `templates::proofs39_zig_surrounders::wraps_zig_in_its_parenthesised_brace_syntax`; 4 of 4                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `unit:src/features/templates/__tests__/emmet.test.ts`       | partial        | partial  | `templates::emmet_model::tests` plus `templates::proofs39_django_emmet::expands_markup_in_django_templates_not_inside_template_tags_variables_or_comments`; 5 of 8; the three expansion cases need the Emmet engine's tables                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `settings:editor.general.postfixCompletion`                 | partial        | partial  | add `settings_view::pages::editor_postfix_completion::tests::proofs39_zig_tab::the_zig_tab_lists_the_zig_templates` and `…::proofs39_java_var_row::the_java_tab_describes_var_as_introduce_variable_does`; the missing clause (Expand templates with Tab switched off has no test; Show postfix templates in code completion reaches nothing) is untouched                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `settings:editor.liveTemplates`                             | partial        | partial  | add `settings_view::pages::editor_live_templates::tests::proofs39_zig_group::the_page_lists_the_zig_templates`; the missing clause (Show live templates in code completion reaches nothing; plugin snippets) is untouched                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |

`ipc:redo_workspace_edit` (done) gains Edit › Redo as a second path to it (`edit_undo_takes_back…`); nothing to change in its status.

Per spec file, every case at HEAD and its native test:

`src/features/code-insight/workspaceUndo.test.ts` (4):

- waits for pending typing before undoing the entire backend transaction → `code_insight::workspace_undo::tests::waits_for_pending_typing_before_undoing_the_entire_backend_transaction`
- reports a refused undo instead of discarding newer work → `code_insight::workspace_undo::tests::reports_a_refused_undo_instead_of_discarding_newer_work`; end to end `workbench::tests::proofs39_agent_undo::reports_a_refused_undo_instead_of_discarding_newer_work`
- does not apply an old notification's id to a different project → `code_insight::workspace_undo::tests::does_not_apply_an_old_notifications_id_to_a_different_project`
- does not announce an agent's edit as a refactoring → `workbench::tests::proofs39_agent_undo::does_not_announce_an_agents_edit_as_a_refactoring`

`src/features/templates/__tests__/editor.test.ts` (17):

- expands a live template abbreviation → `templates::editor::tests::expands_a_live_template_abbreviation`
- falls through when nothing matches → `templates::editor::tests::falls_through_when_nothing_matches`
- does not expand code templates inside comments → `templates::editor::tests::does_not_expand_code_templates_inside_comments`
- expands todo with the language's comment start → `templates::editor::tests::expands_todo_with_the_languages_comment_start`
- expands postfix templates replacing the expression → `templates::editor::tests::expands_postfix_templates_replacing_the_expression`
- wraps compound expressions for .not and offers a chooser → `templates::editor::tests::wraps_compound_expressions_for_not_and_offers_a_chooser`
- runs Introduce Variable on the expression, ended as a statement → `workbench::tests::proofs39_java_var::runs_introduce_variable_on_the_expression_ended_as_a_statement` (model: `templates::proofs39_java_var::the_var_key_becomes_a_semicolon_and_the_expression_is_selected`)
- falls back to the var template without Introduce Variable → `workbench::tests::proofs39_java_var::falls_back_to_the_var_template_without_introduce_variable` (model: `templates::proofs39_java_var::the_var_template_replaces_the_expression_when_the_action_does_not_run`; declined: `workbench::tests::proofs39_java_var::a_declined_introduce_variable_leaves_the_var_declaration`)
- wraps the selected lines → `templates::editor::tests::wraps_the_selected_lines`
- lists the language's surrounders with numeric mnemonics; a mnemonic key picks → `templates::editor::tests::lists_the_languages_surrounders_with_numeric_mnemonics_and_a_mnemonic_key_picks`
- offers expression surrounders for a selection inside a line → `templates::editor::tests::offers_expression_surrounders_for_a_selection_inside_a_line`
- Enter moves to the next field like Tab → `templates::editor::tests::enter_moves_to_the_next_field_like_tab`
- leaves Enter to the editor in a completion's snippet → `editor::intelligence::tests::proofs39_completion_snippet::leaves_enter_to_the_editor_in_a_completions_snippet` (model: `templates::proofs39_completion_enter::leaves_enter_to_the_editor_in_a_completions_snippet`)
- Esc first drops the field's selection, then leaves the template → `templates::editor::tests::esc_first_drops_the_fields_selection_then_leaves_the_template`
- opens a lookup restricted to live templates and ends with it → `templates::editor::tests::opens_a_lookup_restricted_to_live_templates_and_ends_with_it`
- offers postfix templates after a dot → `templates::editor::tests::offers_postfix_templates_after_a_dot`
- offers live templates for a word → `templates::editor::tests::offers_live_templates_for_a_word`

`src/features/templates/__tests__/postfix.test.ts` (10):

- handles call chains, indexing, strings and generics → `templates::postfix_model::tests::handles_call_chains_indexing_strings_and_generics`
- detects compound expressions → `templates::postfix_model::tests::detects_compound_expressions`
- TypeScript: call chain and wider binary expression → `templates::postfix_model::tests::typescript_call_chain_and_wider_binary_expression`
- Rust: method chain with ? → `templates::postfix_model::tests::rust_method_chain_with_question_mark`
- Python: not a statement inside a call → `templates::postfix_model::tests::python_not_a_statement_inside_a_call`
- Go, Java, Kotlin → `templates::postfix_model::tests::go_java_kotlin`
- returns null without a key position or expression → `templates::postfix_model::tests::returns_none_without_a_key_position_or_expression`
- expands with $EXPR$ and a suggested name → `templates::postfix_model::tests::expands_with_expr_and_a_suggested_name`
- Zig: optional unwrap and expansion → `templates::proofs39_zig_postfix::zig_optional_unwrap_and_expansion`
- every language has the core keys → `templates::postfix_model::tests::every_language_has_the_core_keys` (after B3.3)

`src/features/templates/__tests__/live.test.ts` (18): the 18 names of the current proof text, unchanged; `templates::live_builtins::tests::ships_the_requested_built_ins` covers zig after B4.3. (The proof text says "17 of 18; not covered: ships the requested built-ins" while it lists that test.)

`src/features/templates/__tests__/surrounders.test.ts` (4):

- numbers surrounders 1-9, 0, then letters → `templates::live_surrounders::tests::numbers_surrounders_1_to_9_0_then_letters`
- gives templates the first abbreviation character not taken yet → `templates::live_surrounders::tests::gives_templates_the_first_abbreviation_character_not_taken_yet`
- wraps Zig in its parenthesised brace syntax → `templates::proofs39_zig_surrounders::wraps_zig_in_its_parenthesised_brace_syntax`
- filters statement and expression surrounders by the selection → `templates::live_surrounders::tests::filters_statement_and_expression_surrounders_by_the_selection`

`src/features/templates/__tests__/emmet.test.ts` (8):

- expands markup in HTML text, not inside tags, comments or scripts → `templates::emmet_model::tests::expands_markup_in_html_text_not_inside_tags_comments_or_scripts`
- expands markup in Django templates, not inside template tags, variables or comments → `templates::proofs39_django_emmet::expands_markup_in_django_templates_not_inside_template_tags_variables_or_comments`
- uses stylesheet syntax inside rules and <style> → `templates::emmet_model::tests::uses_stylesheet_syntax_inside_rules_and_style`
- expands abbreviations into snippets with fields → MISSING (engine tables)
- fills attribute placeholders and escapes snippet syntax → MISSING (engine tables)
- expands CSS properties and ignores unknown words → MISSING (engine tables)
- finds the innermost element around the caret → `templates::emmet_model::tests::finds_the_innermost_element_around_the_caret`
- renames, adds classes and sets attributes → `templates::emmet_model::tests::renames_adds_classes_and_sets_attributes`

## (c) Behaviours

Slice A:

- An agent's edit is recorded like a refactoring's, in the same history; only the notification is left out (`src/features/code-insight/workspaceUndo.ts:44-46`; `src/editor/core/model/workspaceHistory.ts:34-49` has no `byAgent` filter): built (A1, A2).
- The history starts with the window, before any editor opens (`src/editor/core/index.ts:42-44`, `controller.ts:327-328`, `document.ts:761-763`): built (A5).
- The notification's Undo / Redo follows its own edit only (`workspaceUndo.ts:47-50`): built already; an agent's edit leaves an earlier refactoring's notification as it is, and its own Undo raises nothing.
- Undo and Redo span every file of the edit, open or closed, including files it created, renamed or deleted: the backend restores the snapshot of every path (`crates/ide-workspace/src/documents/workspace_edit.rs:854-890`) through the same call both sides (`ide_host::ops::lsp::undo_workspace_edit` / `redo_workspace_edit`, IPC `undo_workspace_edit` / `redo_workspace_edit`): built already; tested here on two text files (created, renamed and deleted files are not tested).
- A file changed after the edit by another program, or a closed file's bytes changed: the backend refuses (`validate_preconditions(&record.after)`, "<path> changed; request the refactoring again") and the window shows "Cannot undo refactoring" with that body (`workspaceHistory.ts:107-111`, `:167-172`): built already, tested end to end now.
- An Undo started in one editor while another open editor of the edit has newer typing: the webview refuses before asking the backend, "Undo newer editor changes before undoing this refactoring" (`workspaceHistory.ts:154-156`); natively the backend refuses with its own message, since an unsaved buffer counts in its fingerprints (`workspace_edit.rs:348-369`). No data is lost either way; the message differs: left out, written as `proofs39_agent_undo_pending.rs`. Fix: wire `app-gpui/src/editor_model/workspace_history.rs` (written and tested, not used) into the editors.
- Typing made in an editor before the edit: the webview keeps it below the edit's marker (`workspaceHistory.test.ts` "preserves earlier local history…"); natively a workspace edit clears the editor's own history (`app-gpui/src/editor/view.rs` `remote_patch`, `workspace_edit` true for both a refactoring and an agent's edit), so that typing can no longer be undone and Edit › Undo goes straight to the edit. The text is kept; the earlier undo steps are not: left out, same fix.
- Typing made in an editor after the edit: Edit › Undo in that editor undoes the typing first, then the edit, on both sides (natively because `refactoring_history` runs only with nothing of the editor's own left): built already, as in the webview.
- Up to 20 remembered edits (`workspaceHistory.ts:47`, `REMEMBERED_EDITS`): built already.

Slice B:

- M1 `templateSnippet` (`src/features/templates/editor.ts:199-211`, installed at `:607`): built (B1.*). A template picked from a lookup is applied by a function (`editor.ts:341`), so the webview's state sees no `pickedCompletion` and Enter walks its fields: natively `accept_lookup_template` goes through `TemplateEdit::new`, which marks the session as a template's. Emmet's snippets count as templates in the webview; Emmet's Tab expansion is not built natively.
- M2 `action` on Java `.var` (`postfix/model.ts:268-276`), the description "T name = expr;", `applyPostfix` checking `getAction` first (`editor.ts:497-502`), `runPostfixAction` (`editor.ts:526-546`): built (B2.*). "Declines" is `execute_action` answering false: the action is not registered or its `when` does not hold (`app-gpui/src/platform/actions.rs:416-425`, as `src/platform/actions.ts:217-224`); a handler that ran and failed counts as run on both sides (`actions.ts:242-248`). Natively the action runs one turn later (`window.defer`), since its handler updates the window.
- M3 Zig postfix set and tab (`postfix/model.ts:219-258`, `:366`, `:376`): built (B3.*, `postfix_model/zig.rs`): 12 templates.
- M4 Zig live templates (`live/builtins.ts:189-238`, `:550`): built (B4.*, `live_builtins/zig.rs`): 19 templates.
- M5 Zig surrounders (`live/surrounders.ts:146-156`, `:189`): built (B5.*, `live_surrounders/zig.rs`): 10 surrounders.
- M8 Django in Emmet's context (`emmet/model.ts:31`, `:71-72`): built on the model (B8.*); nothing in the app reaches it until Emmet's Tab expansion is compiled.
- Postfix templates in the completion lookup (`editor.ts:568-600`): not built natively before this slice (`expand.rs:377` drops them); the M2 action runs from Tab and the Expressions chooser only.

## (d) Hypotheses

- Hypothesis: a background tab's `Editor` entity stays alive and follows the document. Settled by `edit_undo_takes_back…` (it reads `editor_a` while b's tab is active).
- Hypothesis: the backend's watcher does not reload or touch a closed file's record when another program writes it. Settled by `reports_a_refused_undo…` (the second Undo must go through once b is written back).
- Hypothesis: a `.java` file in a project of the default mode starts no language server (the tests set no `ide.mode`; `proofs33_editor_templates.rs` says the default starts none). Settled by `workbench::tests::proofs39_java_var` running without one (`window_lsp_report` in a failing wait would show it).
- Hypothesis: the lookup's item `dependency` replaces the typed `dep` (as `call` replaces `ca` in `proofs33_template_keys.rs`). Settled by the first text assertion of `proofs39_completion_snippet`.
- Read and settled, not hypotheses: `${1}…${0}` gives two fields (`$0` is `FINAL_STOP`, `snippet.rs:21`); a line break typed at an empty field keeps the session, the field's end mapping after the insertion (`ActiveSnippet::map`, `Assoc::After`); a `when` naming an unset key is false (`context.rs:101-103`, `:181`); `suggest_variable_name("name()", "java", …)` gives `name` (`editor_context.rs:192-230`); autosave is on by default and saves a tab that is left (`workbench/mod.rs:1913-1923`, test at `:5036`), which is why the pending test turns it off and the compiled refusal test uses a closed file written by another program.

## (e) Existing tests whose expectations change

- `templates::postfix_model::tests::every_language_has_the_core_keys`: zig in the language list (B3.3), `src/features/templates/__tests__/postfix.test.ts:112-119`.
- `templates::postfix_model::tests::widening_stops_at_what_is_not_an_expression`: `postfix_templates().len()` 99 → 111 (B3.3), `src/features/templates/postfix/model.ts:219-258`.
- `templates::live_builtins::tests::ships_the_requested_built_ins`: the zig line (B4.3), `src/features/templates/__tests__/live.test.ts:152`.
- `settings_view::pages::editor_postfix_completion::tests::toggles_postfix_templates` and `…::options_are_stored_at_once_and_off_disables_the_rest`: the tab line gains `Zig` after `Go` (B3.4), `src/features/templates/postfix/model.ts:371-379`.
- Nothing asserts Java's `.var` description, the number of live templates, or Enter in a completion item's snippet; the Live Templates page's tests filter by `psvm` and stay as they are.

## (f) Tests most likely to fail first

- `workbench::tests::proofs39_java_var::runs_introduce_variable_on_the_expression_ended_as_a_statement`: `ActionRegistry::handle` replaces the window's handler only because `start` registered it first; the run waits for `seen` with `settle`. If the Tab never reaches the templates (another claim takes it), the text stays `….var` and `seen` never fills.
- `editor::intelligence::tests::proofs39_completion_snippet::…`: the Enter may indent the new line in a `.mock` file, which the assertions allow; the caret assertion after Tab assumes the snippet's end is the end of the text.
- `workbench::tests::proofs39_agent_undo::reports_a_refused_undo_instead_of_discarding_newer_work`: the file watcher may report b's write; if it makes the backend open or reload b, the refusal still holds but the message path may differ.
- `workbench::tests::proofs39_agent_undo::an_agents_edit_made_before_any_editor_opened…`: fails without A5, by design.
- `settings_view::pages::editor_live_templates::tests::proofs39_zig_group::the_page_lists_the_zig_templates`: the filter matches abbreviation, description and group; no other built-in holds "zig" at HEAD (`git show HEAD:app-gpui/src/templates/live_builtins.rs | grep -ic zig` is 0).
- Compile: the three `zig.rs` files use their parent's private helpers through `super::`, which a child module may; `live_builtins/zig.rs` names `LiveTemplate` by its full path. `TemplateExpansion` gains a field; there is no struct literal of it outside `TemplateExpansion::new` at HEAD (`without_action` and the workbench's `postfix_action.rs` use update syntax). `PendingChoice::PostfixExpression` gains `dot`; its only pattern and constructor are in B2.11 and B2.12.

## Corrections to the inputs

- The brief and `WORKPLAN.md` name the row `evt:WorkspaceEditApplied`; `parity.json` has `event:WorkspaceEditApplied`. `WORKPLAN.md` names `set:editor.general.postfixCompletion`; the key is `settings:editor.general.postfixCompletion`.
- `recheck_after_merge.md` M4 says 20 Zig live templates; the list has 19 (`live/builtins.ts:189-238`), as the commit message of e96880141 says.
- The brief says the editor model's workspace history is found by grepping `workspace_edit_applied`: that finds only `editor_language.rs`; the model is `app-gpui/src/editor_model/workspace_history.rs`, declared and tested but used by nothing.
- The brief puts end-to-end tests under `app-gpui/src/workbench/tests/`; the M1 one is under `app-gpui/src/editor/intelligence/tests/`, the only fixture with a fake language server that answers completions.
