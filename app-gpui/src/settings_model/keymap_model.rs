//! Keymap editing: the effective bindings of a staged preset plus overrides (plugins' default
//! shortcuts count as the preset's), add / remove / reset, conflict detection with `when` overlap,
//! shortcut search and the user's own keymaps.

use crate::platform::actions::{ActionMeta, LocalizedAction};
use crate::platform::collate;
use crate::platform::context::{ContextKeys, evaluate_when};
use crate::platform::keymap::{Keymap, PRESET_TITLES, default_preset, is_mac_host, normalize_binding, preset};
use ide_core::settings::CustomKeymap;
use regex::{Captures, Regex};
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// The user keymaps of the settings, by id.
pub type CustomKeymaps = BTreeMap<String, CustomKeymap>;

/// The keymap being edited: a bundled preset and the overrides on top of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeymapDraft {
    pub preset: String,
    pub overrides: Keymap,
    /// The default shortcuts plugins declare for their actions: the defaults of what the preset
    /// leaves out.
    pub extensions: Keymap,
}

impl KeymapDraft {
    pub fn new(preset: &str, overrides: Keymap) -> Self {
        Self { preset: preset.to_owned(), overrides, extensions: Keymap::new() }
    }

    fn with_overrides(&self, overrides: Keymap) -> Self {
        Self { preset: self.preset.clone(), overrides, extensions: self.extensions.clone() }
    }
}

/// What the keymap editor shows and checks of an action.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeymapAction {
    pub id: String,
    /// As shown: in the UI's language.
    pub title: String,
    /// The English category, which places the action in the tree.
    pub category: Option<String>,
    pub when: Option<String>,
    pub keywords: Vec<String>,
    pub hidden: bool,
}

impl KeymapAction {
    pub fn new(id: &str, title: &str) -> Self {
        Self { id: id.to_owned(), title: title.to_owned(), ..Self::default() }
    }

    pub fn category(mut self, category: &str) -> Self {
        self.category = Some(category.to_owned());
        self
    }

    pub fn when(mut self, when: &str) -> Self {
        self.when = Some(when.to_owned());
        self
    }

    /// `meta` as the editor lists it, titled and searchable as `shown` (its localized form).
    pub fn of(meta: &ActionMeta, shown: LocalizedAction) -> Self {
        Self {
            id: meta.id.clone(),
            title: shown.title,
            category: meta.category.clone(),
            when: meta.when.clone(),
            keywords: shown.keywords,
            hidden: meta.hidden,
        }
    }
}

fn normalized(bindings: &[String]) -> Vec<String> {
    bindings.iter().map(|binding| normalize_binding(binding)).collect()
}

/// The defaults of `id`: the preset's, or for an action the preset leaves out, the shortcut its
/// plugin declares. Removing or changing one is an override like any other.
pub fn preset_bindings(draft: &KeymapDraft, id: &str) -> Vec<String> {
    let own = preset(&draft.preset).and_then(|bindings| bindings.get(id));
    own.or_else(|| draft.extensions.get(id)).map(|keys| normalized(keys)).unwrap_or_default()
}

pub fn resolve_bindings(draft: &KeymapDraft, id: &str) -> Vec<String> {
    match draft.overrides.get(id) {
        Some(keys) => normalized(keys),
        None => preset_bindings(draft, id),
    }
}

/// Hidden actions with shortcuts stay configurable, even after their bindings are removed.
pub fn is_keymap_action(action: &KeymapAction, draft: &KeymapDraft) -> bool {
    !action.hidden || draft.overrides.contains_key(&action.id) || !preset_bindings(draft, &action.id).is_empty()
}

pub fn is_modified(draft: &KeymapDraft, id: &str) -> bool {
    draft.overrides.contains_key(id)
}

/// The overrides with the full binding list of `id` set; no override when it equals the preset.
pub fn with_bindings(draft: &KeymapDraft, id: &str, bindings: &[String]) -> Keymap {
    let mut list: Vec<String> = Vec::with_capacity(bindings.len());
    for binding in normalized(bindings) {
        if !list.contains(&binding) {
            list.push(binding);
        }
    }
    let mut overrides = draft.overrides.clone();
    if list == preset_bindings(draft, id) {
        overrides.remove(id);
    } else {
        overrides.insert(id.to_owned(), list);
    }
    overrides
}

pub fn add_binding(draft: &KeymapDraft, id: &str, binding: &str) -> Keymap {
    let mut bindings = resolve_bindings(draft, id);
    bindings.push(binding.to_owned());
    with_bindings(draft, id, &bindings)
}

pub fn remove_binding(draft: &KeymapDraft, id: &str, binding: &str) -> Keymap {
    let removed = normalize_binding(binding);
    let kept: Vec<String> = resolve_bindings(draft, id).into_iter().filter(|kept| *kept != removed).collect();
    with_bindings(draft, id, &kept)
}

pub fn reset_bindings(draft: &KeymapDraft, id: &str) -> Keymap {
    let mut overrides = draft.overrides.clone();
    overrides.remove(id);
    overrides
}

/// Re-bases the overrides on another preset: those that equal the new defaults are dropped.
pub fn switch_preset(draft: &KeymapDraft, preset: &str) -> KeymapDraft {
    let mut next = KeymapDraft::new(preset, Keymap::new());
    next.extensions = draft.extensions.clone();
    for (id, keys) in &draft.overrides {
        next.overrides = with_bindings(&next, id, keys);
    }
    next
}

// ---------------------------------------------------------------------------
// `when` overlap

/// A value a context key takes while two clauses are tried against each other.
#[derive(Debug, Clone, PartialEq)]
enum Fact {
    True,
    False,
    /// The key is not set.
    Unset,
    /// A value none of the clauses mention.
    Other,
    Text(String),
    Number(f64),
}

impl Fact {
    /// The value a clause compares a key with.
    fn literal(raw: &str) -> Self {
        if let Some(quoted) = raw.strip_prefix(['\'', '"']) {
            return Self::Text(quoted[..quoted.len().saturating_sub(1)].to_owned());
        }
        match raw {
            "true" => Self::True,
            "false" => Self::False,
            _ => match raw.parse::<f64>() {
                Ok(number) if number.is_finite() => Self::Number(number),
                _ => Self::Text(raw.to_owned()),
            },
        }
    }

    fn set(&self, ctx: &mut ContextKeys, key: &str) {
        match self {
            Self::True => ctx.set(key, true),
            Self::False => ctx.set(key, false),
            Self::Unset => false,
            Self::Other => ctx.set(key, "\u{0}other"),
            Self::Text(text) => ctx.set(key, text.as_str()),
            Self::Number(number) => ctx.set(key, *number),
        };
    }
}

/// Where the focus is while two clauses are tried against each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusLocation {
    Nowhere,
    Editor,
    /// A tool window none of the clauses name.
    ToolWindow,
    /// The view whose focus key has this index among the view keys.
    View(usize),
}

const TOOL_WINDOW_FOCUS: &str = "toolWindowFocus";
/// Beyond this many combinations two clauses are taken to overlap.
const MAX_COMBINATIONS: usize = 4096;

/// Whether two `when` clauses can be true at the same time. Enumerates the context keys they
/// mention (true / unset / each compared literal / another value) and evaluates both. Focus keys
/// are one focus location: `editor…Focus` keys only hold in the editor, a view key
/// (`projectViewFocus`, `terminalFocus`…) only in that view, `toolWindowFocus` in any view. Too
/// many keys, or a clause that does not parse, counts as overlapping.
pub fn whens_overlap(a: Option<&str>, b: Option<&str>) -> bool {
    static COMPARISON: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"([A-Za-z0-9_.\-:/]+)\s*(==|!=)\s*('[^']*'|"[^"]*"|[A-Za-z0-9_.\-:/]+)"#)
            .expect("the pattern is valid")
    });
    static IDENT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[A-Za-z_][A-Za-z0-9_.\-:/]*").expect("the pattern is valid"));
    let (Some(a), Some(b)) = (a.map(str::trim), b.map(str::trim)) else { return true };
    if a.is_empty() || b.is_empty() || a == b {
        return true;
    }
    // Each key with the values to try, in the order the keys are first mentioned.
    let mut domain: Vec<(String, Vec<Fact>)> = Vec::new();
    let mut add = |key: &str, value: Option<Fact>| {
        let at = domain.iter().position(|(known, _)| known == key).unwrap_or_else(|| {
            domain.push((key.to_owned(), vec![Fact::True, Fact::Unset, Fact::Other]));
            domain.len() - 1
        });
        if let Some(value) = value.filter(|value| !domain[at].1.contains(value)) {
            domain[at].1.push(value);
        }
    };
    for clause in [a, b] {
        let rest = COMPARISON.replace_all(clause, |found: &Captures<'_>| {
            add(&found[1], Some(Fact::literal(&found[3])));
            " "
        });
        for key in IDENT.find_iter(&rest) {
            add(key.as_str(), None);
        }
    }
    let is_focus = |key: &str| key.ends_with("Focus");
    let is_editor = |key: &str| is_focus(key) && key.starts_with("editor");
    let is_view = |key: &str| is_focus(key) && !key.starts_with("editor") && key != TOOL_WINDOW_FOCUS;
    let editor_keys: Vec<&str> = domain.iter().map(|(key, _)| key.as_str()).filter(|key| is_editor(key)).collect();
    let view_keys: Vec<&str> = domain.iter().map(|(key, _)| key.as_str()).filter(|key| is_view(key)).collect();
    let mut locations = vec![FocusLocation::Nowhere, FocusLocation::Editor, FocusLocation::ToolWindow];
    locations.extend((0..view_keys.len()).map(FocusLocation::View));
    // Free variables: everything except the derived view / tool-window focus keys.
    let editor_values = [Fact::True, Fact::False];
    let free: Vec<(&str, &[Fact])> = domain
        .iter()
        .filter(|(key, _)| !is_view(key) && key != TOOL_WINDOW_FOCUS)
        .map(|(key, values)| (key.as_str(), if is_editor(key) { &editor_values[..] } else { values.as_slice() }))
        .collect();
    let Some(combinations) = free.iter().try_fold(1usize, |n, (_, values)| n.checked_mul(values.len())) else {
        return true;
    };
    if combinations.saturating_mul(locations.len()) > MAX_COMBINATIONS {
        return true;
    }
    for &location in &locations {
        for index in 0..combinations {
            let mut ctx = ContextKeys::new();
            let mut rest = index;
            for (key, values) in &free {
                values[rest % values.len()].set(&mut ctx, key);
                rest /= values.len();
            }
            if location != FocusLocation::Editor {
                for key in &editor_keys {
                    ctx.set(key, false);
                }
            }
            for (view, key) in view_keys.iter().enumerate() {
                ctx.set(key, location == FocusLocation::View(view));
            }
            let in_tool_window = matches!(location, FocusLocation::ToolWindow | FocusLocation::View(_));
            ctx.set(TOOL_WINDOW_FOCUS, in_tool_window);
            match (evaluate_when(Some(a), &ctx), evaluate_when(Some(b), &ctx)) {
                (Ok(true), Ok(true)) | (Err(_), _) | (_, Err(_)) => return true,
                _ => {}
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Conflicts

/// How a binding clashes with another action's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// The identical binding.
    Same,
    /// One is the first stroke of the other's chord.
    Prefix,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub action_id: String,
    pub title: String,
    pub category: Option<String>,
    pub binding: String,
    pub kind: ConflictKind,
}

fn is_chord_prefix(prefix: &str, chord: &str) -> bool {
    chord.strip_prefix(prefix).is_some_and(|rest| rest.starts_with(' '))
}

/// The other actions whose bindings clash with `binding` in a context that overlaps with
/// `action_id`'s: identical bindings first, each kind by title.
pub fn find_conflicts(draft: &KeymapDraft, binding: &str, action_id: &str, actions: &[KeymapAction]) -> Vec<Conflict> {
    let binding = normalize_binding(binding);
    let own_when = actions.iter().find(|action| action.id == action_id).and_then(|action| action.when.as_deref());
    let mut out = Vec::new();
    for other in actions.iter().filter(|other| other.id != action_id) {
        for bound in resolve_bindings(draft, &other.id) {
            let kind = if bound == binding {
                ConflictKind::Same
            } else if is_chord_prefix(&binding, &bound) || is_chord_prefix(&bound, &binding) {
                ConflictKind::Prefix
            } else {
                continue;
            };
            if !whens_overlap(own_when, other.when.as_deref()) {
                continue;
            }
            out.push(Conflict {
                action_id: other.id.clone(),
                title: other.title.clone(),
                category: other.category.clone(),
                binding: bound,
                kind,
            });
        }
    }
    out.sort_by(|x, y| {
        let rank = |conflict: &Conflict| conflict.kind != ConflictKind::Same;
        rank(x).cmp(&rank(y)).then_with(|| collate::compare(&x.title, &y.title))
    });
    out
}

/// The overrides with every conflicting assignment removed (IDEA's "Remove other assignments").
pub fn remove_conflicts(draft: &KeymapDraft, conflicts: &[Conflict]) -> Keymap {
    conflicts.iter().fold(draft.overrides.clone(), |overrides, conflict| {
        remove_binding(&draft.with_overrides(overrides), &conflict.action_id, &conflict.binding)
    })
}

/// The shortcut filter: an action matches when one of its bindings equals `strokes` or starts
/// with them. No strokes match everything.
pub fn binding_matches(bindings: &[String], strokes: &str) -> bool {
    if strokes.is_empty() {
        return true;
    }
    let query = normalize_binding(strokes);
    bindings.iter().any(|binding| *binding == query || is_chord_prefix(&query, binding))
}

// ---------------------------------------------------------------------------
// User keymaps (gear menu: Duplicate… / Rename… / Restore Defaults / Delete)

fn preset_or_default(id: &str) -> String {
    if preset(id).is_some() { id.to_owned() } else { default_preset(is_mac_host()).to_owned() }
}

/// The base preset and the overrides of the selected keymap: a bundled preset with the overrides
/// of the settings, or a user keymap with its own.
pub fn active_draft(selected: &str, bundled_overrides: &Keymap, customs: &CustomKeymaps) -> KeymapDraft {
    match customs.get(selected).filter(|_| preset(selected).is_none()) {
        Some(custom) => KeymapDraft::new(&preset_or_default(&custom.parent), custom.overrides.clone()),
        None => KeymapDraft::new(&preset_or_default(selected), bundled_overrides.clone()),
    }
}

fn preset_title(id: &str) -> Option<&'static str> {
    PRESET_TITLES.iter().find(|(preset, _)| *preset == id).map(|(_, title)| *title)
}

/// Display name of a keymap id.
pub fn keymap_name(id: &str, customs: &CustomKeymaps) -> String {
    customs.get(id).map(|custom| custom.name.as_str()).or_else(|| preset_title(id)).unwrap_or(id).to_owned()
}

fn name_taken(name: &str, customs: &CustomKeymaps, own_id: Option<&str>) -> bool {
    PRESET_TITLES.iter().any(|(_, title)| *title == name)
        || customs.iter().any(|(id, custom)| Some(id.as_str()) != own_id && custom.name == name)
}

/// `"<name> copy"`, `"<name> copy 2"`, … not used by any keymap yet.
pub fn copy_name(name: &str, customs: &CustomKeymaps) -> String {
    let base = format!("{name} copy");
    if !name_taken(&base, customs, None) {
        return base;
    }
    (2u32..).map(|i| format!("{base} {i}")).find(|candidate| !name_taken(candidate, customs, None)).unwrap_or(base)
}

/// The error text for a proposed keymap name, or `None` when it is valid. `own_id` is the keymap
/// being renamed, which may keep its name.
pub fn validate_keymap_name(name: &str, customs: &CustomKeymaps, own_id: Option<&str>) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return Some("Keymap name cannot be empty".to_owned());
    }
    name_taken(name, customs, own_id).then(|| format!("Keymap '{name}' already exists"))
}

fn new_keymap_id(name: &str, customs: &CustomKeymaps) -> String {
    let mut slug = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.strip_prefix('-').unwrap_or(&slug);
    let slug = slug.strip_suffix('-').unwrap_or(slug);
    let slug = if slug.is_empty() { "keymap" } else { slug };
    let taken = |id: &str| customs.contains_key(id) || preset(id).is_some();
    let first = format!("custom.{slug}");
    if !taken(&first) {
        return first;
    }
    (2u32..).map(|i| format!("custom.{slug}-{i}")).find(|id| !taken(id)).unwrap_or(first)
}

/// Copies the selected keymap (its effective overrides) under `name`; returns the new id and the
/// user keymaps with it.
pub fn duplicate_keymap(draft: &KeymapDraft, customs: &CustomKeymaps, name: &str) -> (String, CustomKeymaps) {
    let id = new_keymap_id(name, customs);
    let copy =
        CustomKeymap { name: name.trim().to_owned(), parent: draft.preset.clone(), overrides: draft.overrides.clone() };
    let mut customs = customs.clone();
    customs.insert(id.clone(), copy);
    (id, customs)
}

pub fn rename_keymap(customs: &CustomKeymaps, id: &str, name: &str) -> CustomKeymaps {
    let mut customs = customs.clone();
    if let Some(custom) = customs.get_mut(id) {
        custom.name = name.trim().to_owned();
    }
    customs
}

/// Removes a user keymap; returns what is left and the keymap to select: its parent preset.
pub fn delete_keymap(customs: &CustomKeymaps, id: &str) -> (CustomKeymaps, String) {
    let mut customs = customs.clone();
    let parent = customs.remove(id).map(|removed| removed.parent).unwrap_or_default();
    (customs, preset_or_default(&parent))
}

/// Stores edited overrides on the user keymap `id`.
pub fn with_custom_overrides(customs: &CustomKeymaps, id: &str, overrides: Keymap) -> CustomKeymaps {
    let mut customs = customs.clone();
    if let Some(custom) = customs.get_mut(id) {
        custom.overrides = overrides;
    }
    customs
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// A keymap from `(action id, bindings)` pairs.
    pub(crate) fn keymap(entries: &[(&str, &[&str])]) -> Keymap {
        let owned = |keys: &[&str]| keys.iter().map(|&key| key.to_owned()).collect();
        entries.iter().map(|&(id, keys)| (id.to_owned(), owned(keys))).collect()
    }

    pub(crate) fn mac_draft(overrides: &[(&str, &[&str])]) -> KeymapDraft {
        KeymapDraft::new("intellij-macos", keymap(overrides))
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{keymap, mac_draft};
    use super::*;

    fn ids(conflicts: &[Conflict]) -> Vec<&str> {
        conflicts.iter().map(|conflict| conflict.action_id.as_str()).collect()
    }

    // overrides › adds, removes and resets, dropping overrides equal to the preset
    #[test]
    fn adds_removes_and_resets_dropping_overrides_equal_to_the_preset() {
        let draft = mac_draft(&[]);
        let added = add_binding(&draft, "file.save", "Ctrl+S");
        assert_eq!(added["file.save"], ["Meta+S", "Ctrl+S"]);
        let d2 = draft.with_overrides(added);
        assert!(is_modified(&d2, "file.save"));
        assert_eq!(remove_binding(&d2, "file.save", "Ctrl+S"), Keymap::new());
        let unbound = remove_binding(&draft, "file.save", "Meta+S");
        assert_eq!(unbound, keymap(&[("file.save", &[])]));
        assert!(resolve_bindings(&draft.with_overrides(unbound), "file.save").is_empty());
        assert_eq!(reset_bindings(&d2, "file.save"), Keymap::new());
    }

    // overrides › de-duplicates equivalent bindings
    #[test]
    fn de_duplicates_equivalent_bindings() {
        assert_eq!(add_binding(&mac_draft(&[]), "file.save", "meta+s"), Keymap::new());
    }

    // overrides › switching presets keeps real overrides and rebases the rest
    #[test]
    fn switching_presets_keeps_real_overrides_and_rebases_the_rest() {
        let d = mac_draft(&[("file.save", &["Meta+S", "Ctrl+S"]), ("navigate.gotoFile", &["Meta+P"])]);
        let next = switch_preset(&d, "vscode-macos");
        assert_eq!(next.preset, "vscode-macos");
        // Meta+P is VS Code's own default for gotoFile: no longer an override.
        assert_eq!(next.overrides, keymap(&[("file.save", &["Meta+S", "Ctrl+S"])]));
        assert_eq!(resolve_bindings(&next, "navigate.gotoFile"), ["Meta+P"]);
        assert_eq!(resolve_bindings(&next, "navigate.declaration"), ["F12"]);
    }

    // plugin shortcuts › show as the defaults, can be removed or changed, and take part in conflicts
    #[test]
    fn plugin_shortcuts_show_as_the_defaults_can_be_removed_or_changed_and_take_part_in_conflicts() {
        let id = "plugin.acme.tidy";
        let mut draft = KeymapDraft::new(default_preset(is_mac_host()), Keymap::new());
        draft.extensions = keymap(&[(id, &["Ctrl+Alt+Shift+F9"])]);
        assert_eq!(resolve_bindings(&draft, id), [normalize_binding("Ctrl+Alt+Shift+F9")]);
        assert!(!is_modified(&draft, id));
        let removed = remove_binding(&draft, id, "Ctrl+Alt+Shift+F9");
        assert_eq!(removed.get(id), Some(&Vec::new()));
        let unbound = draft.with_overrides(removed);
        assert!(resolve_bindings(&unbound, id).is_empty());
        assert_eq!(reset_bindings(&unbound, id), Keymap::new());
        let actions = [KeymapAction::new("acme.other", "Other"), KeymapAction::new(id, "Tidy")];
        let conflicts = find_conflicts(&draft, "Ctrl+Alt+Shift+F9", "acme.other", &actions);
        assert!(ids(&conflicts).contains(&id));
    }

    fn conflict_actions() -> Vec<KeymapAction> {
        vec![
            KeymapAction::new("code.duplicateLine", "Duplicate Line").category("Code").when("editorFocus"),
            KeymapAction::new("vcs.showDiff", "Show Diff").category("VCS").when("changesViewFocus"),
            KeymapAction::new("project.newFile", "New File").category("Project").when("projectViewFocus"),
            KeymapAction::new("code.generate", "Generate").category("Code").when("editorFocus"),
            KeymapAction::new("chord.one", "Chord"),
            KeymapAction::new("mine", "Mine").when("editorFocus"),
        ]
    }

    fn conflict_draft() -> KeymapDraft {
        mac_draft(&[("chord.one", &["Meta+K Meta+C"]), ("mine", &[])])
    }

    // conflicts › reports same-binding conflicts in overlapping contexts only
    #[test]
    fn reports_same_binding_conflicts_in_overlapping_contexts_only() {
        let (draft, actions) = (conflict_draft(), conflict_actions());
        let c = find_conflicts(&draft, "Meta+D", "mine", &actions);
        assert_eq!(ids(&c), ["code.duplicateLine"]);
        assert_eq!(c[0].kind, ConflictKind::Same);
        // Meta+N: New File (project view) and Generate (editor); only the editor one overlaps.
        assert_eq!(ids(&find_conflicts(&draft, "Meta+N", "mine", &actions)), ["code.generate"]);
    }

    // conflicts › reports chord prefix conflicts
    #[test]
    fn reports_chord_prefix_conflicts() {
        let c = find_conflicts(&conflict_draft(), "Meta+K", "mine", &conflict_actions());
        let chord = c.iter().find(|conflict| conflict.action_id == "chord.one").expect("the chord conflicts");
        assert_eq!((chord.kind, chord.binding.as_str()), (ConflictKind::Prefix, "Meta+K Meta+C"));
    }

    // conflicts › removes other assignments
    #[test]
    fn removes_other_assignments() {
        let draft = conflict_draft();
        let c = find_conflicts(&draft, "Meta+D", "mine", &conflict_actions());
        let overrides = remove_conflicts(&draft, &c);
        assert_eq!(overrides.get("code.duplicateLine"), Some(&Vec::new()));
        assert_eq!(overrides.get("vcs.showDiff"), None);
    }

    // conflicts › evaluates when-clause overlap
    #[test]
    fn evaluates_when_clause_overlap() {
        let overlap = |a: &str, b: &str| whens_overlap(Some(a), Some(b));
        assert!(whens_overlap(None, Some("editorFocus")));
        assert!(overlap("editorFocus", "editorFocus"));
        assert!(!overlap("editorFocus", "!editorFocus"));
        assert!(!overlap("editorFocus", "projectViewFocus"));
        assert!(!overlap("editorFocus && lang == 'rust'", "lang == 'go'"));
        assert!(overlap("editorFocus || debug", "debug && !editorFocus"));
        assert!(!overlap("a == 1", "a != 1"));
        // Focus keys: one focus location at a time; views imply toolWindowFocus.
        assert!(!overlap("editorFocus", "terminalFocus"));
        assert!(overlap("editorFocus", "editorTextFocus"));
        assert!(overlap("toolWindowFocus && !popupOpen", "projectViewFocus"));
        assert!(!overlap("toolWindowFocus", "editorFocus"));
        assert!(overlap("projectOpen", "editorFocus"));
    }

    // shortcut search › matches exact bindings and chord first strokes
    #[test]
    fn matches_exact_bindings_and_chord_first_strokes() {
        let bound = |binding: &str| vec![binding.to_owned()];
        assert!(binding_matches(&bound("Meta+K Meta+C"), "Meta+K"));
        assert!(binding_matches(&bound("Shift+Meta+O"), "Meta+Shift+O"));
        assert!(!binding_matches(&bound("Meta+O"), "Meta+Shift+O"));
    }

    fn vscode_draft() -> KeymapDraft {
        KeymapDraft::new("vscode-macos", keymap(&[("file.save", &["Ctrl+S"])]))
    }

    // user keymaps › duplicates, renames and deletes
    #[test]
    fn duplicates_renames_and_deletes() {
        let draft = vscode_draft();
        let none = CustomKeymaps::new();
        assert_eq!(copy_name("macOS", &none), "macOS copy");
        let (id, customs) = duplicate_keymap(&draft, &none, "Mine");
        assert_eq!(id, "custom.mine");
        let mine = CustomKeymap {
            name: "Mine".to_owned(),
            parent: "vscode-macos".to_owned(),
            overrides: keymap(&[("file.save", &["Ctrl+S"])]),
        };
        assert_eq!(customs.get(&id), Some(&mine));
        assert_eq!(copy_name("Mine", &customs), "Mine copy");
        let exists = |name: &str, customs: &CustomKeymaps, own: Option<&str>| {
            validate_keymap_name(name, customs, own).is_some_and(|error| error.contains("already exists"))
        };
        assert!(exists("Mine", &customs, None));
        assert_eq!(validate_keymap_name("Mine", &customs, Some(&id)), None);
        assert!(exists("macOS", &none, None));
        assert!(validate_keymap_name("  ", &none, None).is_some_and(|error| error.contains("empty")));
        assert_eq!(duplicate_keymap(&draft, &customs, "mine!").0, "custom.mine-2");
        let renamed = rename_keymap(&customs, &id, "Ours");
        assert_eq!(keymap_name(&id, &renamed), "Ours");
        assert_eq!(delete_keymap(&renamed, &id), (CustomKeymaps::new(), "vscode-macos".to_owned()));
    }

    // user keymaps › edits target the selected keymap
    #[test]
    fn edits_target_the_selected_keymap() {
        let draft = vscode_draft();
        let (_, customs) = duplicate_keymap(&draft, &CustomKeymaps::new(), "Mine");
        let bundled = keymap(&[("x.y", &[])]);
        assert_eq!(active_draft("custom.mine", &bundled, &customs), draft);
        assert_eq!(active_draft("intellij-macos", &bundled, &customs), mac_draft(&[("x.y", &[])]));
        assert_eq!(active_draft("custom.gone", &Keymap::new(), &customs).preset, default_preset(is_mac_host()));
        let next = with_custom_overrides(&customs, "custom.mine", Keymap::new());
        assert_eq!(next.get("custom.mine").map(|custom| &custom.overrides), Some(&Keymap::new()));
    }

    // Not from a spec: names of presets and unknown ids, a second copy, a clause that does not
    // parse, and a hidden action that is listed only while it has or had a shortcut.
    #[test]
    fn names_copies_and_hidden_actions() {
        let (id, customs) = duplicate_keymap(&vscode_draft(), &CustomKeymaps::new(), "macOS copy");
        assert_eq!(id, "custom.macos-copy");
        assert_eq!(copy_name("macOS", &customs), "macOS copy 2");
        assert_eq!(keymap_name("vscode-macos", &customs), "VS Code (macOS)");
        assert_eq!(keymap_name("nope", &customs), "nope");
        assert_eq!(duplicate_keymap(&vscode_draft(), &customs, "!!").0, "custom.keymap");
        assert!(whens_overlap(Some("a &&"), Some("b")));
        assert!(whens_overlap(Some("  "), Some("b")));
        let hidden = KeymapAction { hidden: true, ..KeymapAction::new("internal.x", "Internal") };
        assert!(!is_keymap_action(&hidden, &mac_draft(&[])));
        assert!(is_keymap_action(&hidden, &mac_draft(&[("internal.x", &[])])));
        assert!(is_keymap_action(&KeymapAction::new("internal.x", "Internal"), &mac_draft(&[])));
    }
}
