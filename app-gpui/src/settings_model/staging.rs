//! The Settings dialog's state. Edits are staged per (scope, key) and only written on Apply / OK,
//! one settings write per changed key, like IDEA's Configurable `isModified` / `apply` / `reset`
//! cycle.
//!
//! Nothing here writes: [`SettingsDialog::begin_apply`] hands out an [`ApplyRun`], which lists
//! the writes in order and is told how each went. SSH credentials and values kept outside the
//! settings are staged alongside and written after the settings they belong to.

use super::schema::{deep_equal, default_settings, get_at, put_at, put_at_opt, set_at};
use crate::platform::collate;
use serde_json::{Map, Value};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub use ide_workspace::settings::SettingsScope as Scope;

/// The page a fresh dialog shows.
pub const DEFAULT_PAGE: &str = "appearance";
/// The page SSH credentials are staged on.
pub const SSH_SETTINGS_PAGE: &str = "tools.sshConfigurations";

/// The name a scope is written by in a stage id and in a settings write.
pub fn scope_name(scope: Scope) -> &'static str {
    match scope {
        Scope::Global => "global",
        Scope::Project => "project",
    }
}

/// The id of the write of `key` in `scope`.
pub fn stage_key(scope: Scope, key: &str) -> String {
    format!("{}|{key}", scope_name(scope))
}

/// An edit waiting for Apply.
#[derive(Debug, Clone, PartialEq)]
pub struct StagedChange {
    pub scope: Scope,
    pub key: String,
    /// Literal map segments below `key`; dots inside plugin ids and keys are data.
    pub path: Option<Vec<String>>,
    /// `null` removes the key.
    pub value: Value,
    /// The page that made the change (Reset discards per page).
    pub page: String,
}

impl StagedChange {
    /// The value as the settings write carries it.
    pub fn json(&self) -> String {
        self.value.to_string()
    }
}

/// A value kept outside the settings, staged by a page under its own key.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalChange {
    pub page: String,
    pub value: Value,
}

/// A credential. Its text never shows in debug output.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(…)")
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProjectStructureSection {
    #[default]
    Project,
    Modules,
    Libraries,
    Facets,
    Artifacts,
    Sdks,
}

impl ProjectStructureSection {
    pub const ALL: [Self; 6] =
        [Self::Project, Self::Modules, Self::Libraries, Self::Facets, Self::Artifacts, Self::Sdks];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Modules => "modules",
            Self::Libraries => "libraries",
            Self::Facets => "facets",
            Self::Artifacts => "artifacts",
            Self::Sdks => "sdks",
        }
    }

    /// The section `raw` names (`modules`, `projectStructure.modules`); Project for anything else.
    pub fn parse(raw: Option<&str>) -> Self {
        let clean = raw.map(|raw| raw.strip_prefix("projectStructure.").unwrap_or(raw).trim());
        Self::ALL.into_iter().find(|section| Some(section.as_str()) == clean).unwrap_or_default()
    }
}

/// `page#focus`: the page and what to select on it.
pub fn split_page_target(target: &str) -> (&str, Option<&str>) {
    match target.split_once('#') {
        Some((page, focus)) => (page, Some(focus).filter(|focus| !focus.is_empty())),
        None => (target, None),
    }
}

/// IDEA's File › New Projects Setup › Settings for New Projects.
pub fn settings_title(new_projects: bool) -> &'static str {
    if new_projects { "Settings for New Projects" } else { "Settings" }
}

/// The id a page's external value is staged under.
pub fn external_key(page: &str, key: &str) -> String {
    format!("{page}|{key}")
}

fn is_inside(key: &str, parent: &str) -> bool {
    key.strip_prefix(parent).is_some_and(|rest| rest.starts_with('.'))
}

/// The part of `key` below `parent`, which it is inside of.
fn below<'k>(key: &'k str, parent: &str) -> &'k str {
    key.get(parent.len() + 1..).unwrap_or_default()
}

fn map_entry<'v>(value: Option<&'v Value>, path: &[String]) -> Option<&'v Value> {
    path.iter().try_fold(value?, |value, key| value.as_object()?.get(key))
}

/// `value` with the entry at the literal `path` set to `next`; `null` removes it, and a map left
/// empty by that goes too.
fn put_entry(value: Option<&Value>, path: &[String], next: &Value) -> Value {
    let Some((head, tail)) = path.split_first() else {
        return next.clone();
    };
    let mut out: Map<String, Value> = value.and_then(Value::as_object).cloned().unwrap_or_default();
    let child = if tail.is_empty() { next.clone() } else { put_entry(out.get(head), tail, next) };
    let emptied = !tail.is_empty() && child.as_object().is_some_and(Map::is_empty);
    if child.is_null() || emptied {
        out.remove(head);
    } else {
        out.insert(head.clone(), child);
    }
    Value::Object(out)
}

fn some(value: Option<&Value>) -> Option<Value> {
    value.filter(|value| !value.is_null()).cloned()
}

/// The state of the Settings dialog (and of Project Structure, which shares its staging).
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsDialog {
    pub open: bool,
    pub project_structure_open: bool,
    pub project_structure_section: ProjectStructureSection,
    pub page_id: String,
    /// What the page should select when opened as `page#focus` (an inspection id).
    pub focus: Option<String>,
    pub query: String,
    /// The effective settings when the dialog opened / last applied.
    pub base: Value,
    /// The defaults plus the global layer (no project values): what the Global scope edits.
    pub global_base: Value,
    /// "Settings for New Projects": every page edits the global layer.
    pub new_projects: bool,
    /// Staged edits by id, in the order they were first made.
    staged: Vec<(String, StagedChange)>,
    /// SSH credentials by configuration id; they stay outside the settings until Apply / OK.
    staged_secrets: Vec<(String, Secret)>,
    staged_external: Vec<(String, ExternalChange)>,
    /// Values written outside the settings since the dialog opened.
    external_applied: BTreeMap<String, Value>,
    /// The scope chosen per page (Global / This project).
    pub scopes: BTreeMap<String, Scope>,
    pub saving: bool,
    pub error: Option<String>,
    pub pages_loading: bool,
}

impl Default for SettingsDialog {
    fn default() -> Self {
        Self {
            open: false,
            project_structure_open: false,
            project_structure_section: ProjectStructureSection::default(),
            page_id: DEFAULT_PAGE.to_owned(),
            focus: None,
            query: String::new(),
            base: default_settings(),
            global_base: default_settings(),
            new_projects: false,
            staged: Vec::new(),
            staged_secrets: Vec::new(),
            staged_external: Vec::new(),
            external_applied: BTreeMap::new(),
            scopes: BTreeMap::new(),
            saving: false,
            error: None,
            pages_loading: false,
        }
    }
}

impl SettingsDialog {
    fn discard_staged(&mut self) {
        self.staged.clear();
        self.staged_secrets.clear();
        self.staged_external.clear();
        self.external_applied.clear();
        self.saving = false;
        self.error = None;
    }

    /// Opens the dialog on `target` (`page` or `page#focus`; the page last shown without one),
    /// over `settings`, the effective settings. With a project open the caller then loads the
    /// global layer into [`SettingsDialog::set_global_base`].
    pub fn open(&mut self, target: Option<&str>, new_projects: bool, settings: Value) {
        if let Some(target) = target {
            self.select_page(target);
        } else {
            self.focus = None;
        }
        self.open = true;
        self.project_structure_open = false;
        self.query.clear();
        self.global_base = settings.clone();
        self.base = settings;
        self.new_projects = new_projects;
        self.discard_staged();
    }

    /// Opens Project Structure on `section`; when it is open already, only the section changes.
    pub fn open_project_structure(&mut self, section: Option<&str>, settings: Value) {
        if self.project_structure_open {
            if section.is_some() {
                self.project_structure_section = ProjectStructureSection::parse(section);
            }
            return;
        }
        self.open = false;
        self.project_structure_open = true;
        self.project_structure_section = ProjectStructureSection::parse(section);
        self.global_base = settings.clone();
        self.base = settings;
        self.new_projects = false;
        self.discard_staged();
    }

    pub fn close_project_structure(&mut self) {
        self.project_structure_open = false;
        self.discard_staged();
    }

    /// The global layer arrived: what the Global scope compares edits with. Ignored once both
    /// windows are closed.
    pub fn set_global_base(&mut self, global: Value) {
        if self.open || self.project_structure_open {
            self.global_base = global;
        }
    }

    /// The effective settings changed under the open dialog.
    pub fn rebase(&mut self, settings: Value) {
        if self.open || self.project_structure_open {
            self.global_base = settings.clone();
            self.base = settings;
        }
    }

    pub fn close(&mut self) {
        self.open = false;
        self.pages_loading = false;
        self.discard_staged();
    }

    /// Shows `target` (`page#focus` also selects `focus` on it).
    pub fn select_page(&mut self, target: &str) {
        let (page, focus) = split_page_target(target);
        self.page_id = page.to_owned();
        self.focus = focus.map(str::to_owned);
    }

    pub fn set_page_scope(&mut self, page_id: &str, scope: Scope) {
        self.scopes.insert(page_id.to_owned(), scope);
    }

    fn base_for(&self, scope: Scope) -> &Value {
        match scope {
            Scope::Global => &self.global_base,
            Scope::Project => &self.base,
        }
    }

    /// The staged edits, in the order they were first made.
    pub fn staged(&self) -> impl Iterator<Item = &StagedChange> {
        self.staged.iter().map(|(_, change)| change)
    }

    pub fn has_staged(&self) -> bool {
        !self.staged.is_empty()
    }

    fn put_staged(&mut self, id: String, change: StagedChange) {
        match self.staged.iter_mut().find(|(staged, _)| *staged == id) {
            Some(slot) => slot.1 = change,
            None => self.staged.push((id, change)),
        }
    }

    fn drop_staged(&mut self, id: &str) {
        self.staged.retain(|(staged, _)| staged != id);
    }

    /// The staged edits as writes: whole-key edits as they are, literal map entries merged into
    /// one write of their key per scope. Which page owns an entry stays apart until here.
    fn merged_changes(&self) -> Vec<StagedChange> {
        let mut roots: Vec<StagedChange> = self.staged().filter(|change| change.path.is_none()).cloned().collect();
        for change in self.staged() {
            let Some(path) = &change.path else { continue };
            let found = roots.iter().position(|root| root.scope == change.scope && root.key == change.key);
            let at = found.unwrap_or_else(|| {
                let value = get_at(self.base_for(change.scope), &change.key).cloned().unwrap_or(Value::Null);
                roots.push(StagedChange { path: None, value, ..change.clone() });
                roots.len() - 1
            });
            let root = &mut roots[at];
            root.value = put_entry(Some(&root.value).filter(|value| !value.is_null()), path, &change.value);
        }
        roots
    }

    /// Stages one literal map entry of `key`, apart from every other page's entries.
    pub fn stage_map_entry(&mut self, page: &str, scope: Scope, key: &str, path: &[&str], value: Value) {
        if path.is_empty() {
            return self.stage(page, scope, key, value);
        }
        let path: Vec<String> = path.iter().map(|&segment| segment.to_owned()).collect();
        let id = format!("{}|{}", stage_key(scope, key), Value::from(path.clone()));
        let current = map_entry(get_at(self.base_for(scope), key), &path);
        if deep_equal(Some(&value), current) {
            self.drop_staged(&id);
        } else {
            let change = StagedChange { scope, key: key.to_owned(), path: Some(path), value, page: page.to_owned() };
            self.put_staged(id, change);
        }
        self.error = None;
    }

    /// The value of `key` as its page shows it: the staged edit, else the scope's value. Edits
    /// staged for a parent (a whole map entry) or inside `key` (one field of it) show through
    /// too. `None` for a key without a value.
    pub fn staged_value(&self, scope: Scope, key: &str) -> Option<Value> {
        let changes = self.merged_changes();
        if let Some(hit) = changes.iter().find(|change| change.scope == scope && change.key == key) {
            return some(Some(&hit.value));
        }
        let mut related: Vec<&StagedChange> = changes
            .iter()
            .filter(|change| change.scope == scope && (is_inside(key, &change.key) || is_inside(&change.key, key)))
            .collect();
        related.sort_by_key(|change| change.key.encode_utf16().count());
        let mut value = some(get_at(self.base_for(scope), key));
        for change in related {
            value = if is_inside(key, &change.key) {
                some(get_at(&change.value, below(key, &change.key)))
            } else {
                Some(put_at_opt(value.as_ref(), below(&change.key, key), change.value.clone()))
            };
        }
        value
    }

    /// Stages `value` for `key`; `null` removes the key on Apply. An edit inside an entry staged
    /// as a whole (a map entry just added) goes into that entry; a new value for `key` drops the
    /// edits staged inside it. An edit back to the scope's value is not staged.
    pub fn stage(&mut self, page: &str, scope: Scope, key: &str, value: Value) {
        self.error = None;
        let parent = self.staged().find(|change| change.scope == scope && is_inside(key, &change.key)).cloned();
        if let Some(parent) = parent {
            let merged = put_at(&parent.value, below(key, &parent.key), value);
            self.put_staged(stage_key(scope, &parent.key), StagedChange { value: merged, ..parent });
            return;
        }
        self.staged.retain(|(_, change)| {
            let replaced = is_inside(&change.key, key) || (change.key == key && change.path.is_some());
            !(change.scope == scope && replaced)
        });
        let id = stage_key(scope, key);
        // Back to the scope's current value: nothing to write.
        if deep_equal(Some(&value), get_at(self.base_for(scope), key)) {
            self.drop_staged(&id);
        } else {
            let change = StagedChange { scope, key: key.to_owned(), path: None, value, page: page.to_owned() };
            self.put_staged(id, change);
        }
    }

    /// Stages the credential of an SSH configuration. An empty field leaves the stored one as it is.
    pub fn stage_ssh_secret(&mut self, id: &str, value: &str) {
        self.staged_secrets.retain(|(staged, _)| staged != id);
        if !value.is_empty() {
            self.staged_secrets.push((id.to_owned(), Secret(value.to_owned())));
        }
        self.error = None;
    }

    pub fn staged_secret(&self, id: &str) -> Option<&Secret> {
        self.staged_secrets.iter().find(|(staged, _)| staged == id).map(|(_, secret)| secret)
    }

    pub fn has_staged_secrets(&self) -> bool {
        !self.staged_secrets.is_empty()
    }

    fn drop_secret(&mut self, id: &str, secret: &Secret) {
        self.staged_secrets.retain(|(staged, value)| !(staged == id && value == secret));
    }

    /// Stages a value kept outside the settings; `base` is what is stored now. Ignored while an
    /// Apply runs.
    pub fn stage_external(&mut self, page: &str, key: &str, value: Value, base: &Value) {
        if self.saving {
            return;
        }
        let id = external_key(page, key);
        self.staged_external.retain(|(staged, _)| *staged != id);
        if !deep_equal(Some(&value), Some(base)) {
            self.staged_external.push((id, ExternalChange { page: page.to_owned(), value }));
        }
        self.error = None;
    }

    pub fn staged_external(&self, page: &str, key: &str) -> Option<&Value> {
        let id = external_key(page, key);
        self.staged_external.iter().find(|(staged, _)| *staged == id).map(|(_, change)| &change.value)
    }

    /// What an external value was written as since the dialog opened.
    pub fn external_applied(&self, page: &str, key: &str) -> Option<&Value> {
        self.external_applied.get(&external_key(page, key))
    }

    /// Discards the staged edits of one page (the dialog's Reset button).
    pub fn reset_page(&mut self, page: &str) {
        self.staged.retain(|(_, change)| change.page != page);
        if page == SSH_SETTINGS_PAGE {
            self.staged_secrets.clear();
        }
        self.staged_external.retain(|(_, change)| change.page != page);
        self.error = None;
    }

    /// The writes to perform, the global layer first so that a project value still wins
    /// afterwards, each scope's by key.
    pub fn pending_changes(&self) -> Vec<StagedChange> {
        let mut changes = self.merged_changes();
        changes.retain(|change| !deep_equal(Some(&change.value), get_at(self.base_for(change.scope), &change.key)));
        changes.sort_by(|a, b| match (a.scope, b.scope) {
            (Scope::Global, Scope::Project) => Ordering::Less,
            (Scope::Project, Scope::Global) => Ordering::Greater,
            _ => collate::compare(&a.key, &b.key),
        });
        changes
    }

    /// Apply: what there is to write. Marks the dialog as saving when there is something.
    pub fn begin_apply(&mut self) -> ApplyStart {
        if self.saving {
            return ApplyStart::Busy;
        }
        let run = ApplyRun {
            changes: self.pending_changes(),
            initial: self.staged.clone(),
            secrets: self.staged_secrets.clone(),
            external: self.staged_external.clone(),
            secret_at: 0,
            external_at: 0,
            error: None,
        };
        if run.changes.is_empty() && run.secrets.is_empty() && run.external.is_empty() {
            return ApplyStart::Clean;
        }
        self.saving = true;
        self.error = None;
        ApplyStart::Run(run)
    }
}

/// `settings` with `changes` applied in order: what Apply does to the session's settings when
/// there is no backend to write to.
pub fn session_settings(settings: &Value, changes: &[StagedChange]) -> Value {
    changes.iter().fold(settings.clone(), |settings, change| set_at(&settings, &change.key, change.value.clone()))
}

/// How an Apply starts.
#[derive(Debug, Clone, PartialEq)]
pub enum ApplyStart {
    /// Another Apply is running: this one fails.
    Busy,
    /// Nothing to write: this one succeeds at once.
    Clean,
    Run(ApplyRun),
}

/// A credential to store for an SSH configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretWrite {
    pub id: String,
    pub secret: Secret,
    /// Keep it in the keychain, not only for this session.
    pub save: bool,
}

/// An Apply in progress. Its caller:
///
/// 1. writes [`ApplyRun::changes`] in order, stopping at the first failure, re-reads the effective
///    settings when anything was written and reports with [`ApplyRun::writes_done`] (then loads
///    the global layer into [`SettingsDialog::set_global_base`] when a project is open);
/// 2. stores each [`ApplyRun::next_secret`] and reports with [`ApplyRun::secret_done`];
/// 3. writes each [`ApplyRun::next_external`] and reports with [`ApplyRun::external_done`];
/// 4. calls [`ApplyRun::finish`].
#[derive(Debug, Clone, PartialEq)]
pub struct ApplyRun {
    changes: Vec<StagedChange>,
    /// The staged edits when the Apply began: one changed meanwhile stays staged.
    initial: Vec<(String, StagedChange)>,
    secrets: Vec<(String, Secret)>,
    external: Vec<(String, ExternalChange)>,
    secret_at: usize,
    external_at: usize,
    error: Option<String>,
}

impl ApplyRun {
    /// The settings writes, in order.
    pub fn changes(&self) -> &[StagedChange] {
        &self.changes
    }

    /// The first `written` changes were written; `failure` is why the next one was not. `base`
    /// is the effective settings now, `has_project` whether a project is open (the global layer
    /// is then a layer of its own, updated here with the written global values until the
    /// backend's copy arrives).
    pub fn writes_done(
        &mut self,
        dialog: &mut SettingsDialog,
        written: usize,
        failure: Option<&str>,
        base: Value,
        has_project: bool,
    ) {
        let written = &self.changes[..written.min(self.changes.len())];
        if let (Some(failure), Some(failed)) = (failure, self.changes.get(written.len())) {
            self.error = Some(format!("{}: {failure}", failed.key));
        }
        let done: BTreeSet<String> = written.iter().map(|change| stage_key(change.scope, &change.key)).collect();
        dialog.global_base = if has_project {
            let global = written.iter().filter(|change| change.scope == Scope::Global);
            global.fold(dialog.global_base.clone(), |layer, change| put_at(&layer, &change.key, change.value.clone()))
        } else {
            base.clone()
        };
        dialog.base = base;
        let initial = &self.initial;
        dialog.staged.retain(|(id, change)| {
            let unchanged = initial.iter().any(|(staged, before)| staged == id && before == change);
            !(done.contains(&stage_key(change.scope, &change.key)) && unchanged)
        });
    }

    /// The next credential to store. `backend` is whether there is a keychain to store it in.
    /// A credential that was discarded meanwhile, or whose configuration was removed or does not
    /// take one, is never written.
    pub fn next_secret(&mut self, dialog: &mut SettingsDialog, backend: bool) -> Option<SecretWrite> {
        if self.error.is_some() {
            return None;
        }
        while let Some((id, secret)) = self.secrets.get(self.secret_at) {
            if dialog.staged_secret(id) != Some(secret) {
                self.secret_at += 1;
                continue;
            }
            let configs = get_at(&dialog.global_base, "ssh.configs").and_then(Value::as_array);
            let config = configs.into_iter().flatten().find(|config| config["id"] == id.as_str());
            let takes_secret = config.filter(|config| config["auth"] != "openSsh" && backend);
            match takes_secret {
                Some(config) => {
                    let save = config["saveSecret"].as_bool().unwrap_or(false);
                    return Some(SecretWrite { id: id.clone(), secret: secret.clone(), save });
                }
                None => {
                    dialog.drop_secret(id, secret);
                    self.secret_at += 1;
                }
            }
        }
        None
    }

    /// How storing the credential [`ApplyRun::next_secret`] handed out went.
    pub fn secret_done(&mut self, dialog: &mut SettingsDialog, result: Result<(), String>) {
        let Some((id, secret)) = self.secrets.get(self.secret_at) else { return };
        match result {
            Ok(()) => dialog.drop_secret(id, secret),
            Err(message) => self.error = Some(format!("SSH secret: {message}")),
        }
        self.secret_at += 1;
    }

    /// The next external value to write: its id ([`external_key`]) and the value.
    pub fn next_external(&mut self, dialog: &SettingsDialog) -> Option<(String, Value)> {
        if self.error.is_some() {
            return None;
        }
        while let Some((id, change)) = self.external.get(self.external_at) {
            // Staged again or reset meanwhile: not this Apply's to write.
            if dialog.staged_external.iter().any(|(staged, current)| staged == id && current == change) {
                return Some((id.clone(), change.value.clone()));
            }
            self.external_at += 1;
        }
        None
    }

    /// How writing the value [`ApplyRun::next_external`] handed out went: the value as stored.
    pub fn external_done(&mut self, dialog: &mut SettingsDialog, result: Result<Value, String>) {
        let Some((id, change)) = self.external.get(self.external_at) else { return };
        match result {
            Ok(value) => {
                dialog.staged_external.retain(|(staged, current)| !(staged == id && current == change));
                dialog.external_applied.insert(id.clone(), value);
            }
            Err(message) => self.error = Some(format!("{id}: {message}")),
        }
        self.external_at += 1;
    }

    /// Ends the Apply; whether everything was saved. A failure is the dialog's `error`.
    pub fn finish(self, dialog: &mut SettingsDialog) -> bool {
        dialog.saving = false;
        dialog.error = self.error;
        dialog.error.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::settings::{SshAuth, SshConfiguration};
    use serde_json::json;

    const GLOBAL: Scope = Scope::Global;
    const PROJECT: Scope = Scope::Project;

    /// A backend holding the settings in memory, as the spec's.
    #[derive(Default)]
    struct FakeBackend {
        settings: Value,
        /// A key whose write fails.
        fail: Option<&'static str>,
        sets: Vec<(Scope, String, String)>,
        secrets: Vec<(String, String, bool)>,
        /// How many of the next credential writes fail.
        failing_secrets: usize,
        /// Whether each credential's configuration was in the settings when it was written.
        configured_at_secret: Vec<bool>,
    }

    impl FakeBackend {
        fn over(settings: &Value) -> Self {
            Self { settings: settings.clone(), ..Self::default() }
        }

        fn set(&mut self, change: &StagedChange) -> Result<(), String> {
            self.sets.push((change.scope, change.key.clone(), change.json()));
            if self.fail == Some(change.key.as_str()) {
                return Err("invalid value".to_owned());
            }
            let value = serde_json::from_str(&change.json()).unwrap_or(Value::Null);
            self.settings = set_at(&self.settings, &change.key, value);
            Ok(())
        }

        fn set_secret(&mut self, write: &SecretWrite) -> Result<(), String> {
            self.secrets.push((write.id.clone(), write.secret.expose().to_owned(), write.save));
            let configs = get_at(&self.settings, "ssh.configs").and_then(Value::as_array).cloned().unwrap_or_default();
            self.configured_at_secret.push(configs.iter().any(|config| config["id"] == write.id.as_str()));
            if self.failing_secrets > 0 {
                self.failing_secrets -= 1;
                return Err("Unavailable".to_owned());
            }
            Ok(())
        }
    }

    /// Runs an Apply against `backend`, as the dialog's caller does.
    fn apply(dialog: &mut SettingsDialog, backend: &mut FakeBackend) -> bool {
        let mut run = match dialog.begin_apply() {
            ApplyStart::Busy => return false,
            ApplyStart::Clean => return true,
            ApplyStart::Run(run) => run,
        };
        let mut written = 0;
        let mut failure = None;
        for change in run.changes().to_vec() {
            match backend.set(&change) {
                Ok(()) => written += 1,
                Err(message) => {
                    failure = Some(message);
                    break;
                }
            }
        }
        run.writes_done(dialog, written, failure.as_deref(), backend.settings.clone(), false);
        while let Some(write) = run.next_secret(dialog, true) {
            let result = backend.set_secret(&write);
            run.secret_done(dialog, result);
        }
        while let Some((_, value)) = run.next_external(dialog) {
            run.external_done(dialog, Ok(value));
        }
        run.finish(dialog)
    }

    fn opened(page: Option<&str>) -> SettingsDialog {
        let mut dialog = SettingsDialog::default();
        dialog.open(page, false, default_settings());
        dialog
    }

    fn keys_and_values(changes: &[StagedChange]) -> Vec<(&str, &Value)> {
        changes.iter().map(|change| (change.key.as_str(), &change.value)).collect()
    }

    // settings staging › stages edits without writing and shows staged values
    #[test]
    fn stages_edits_without_writing_and_shows_staged_values() {
        let mut dialog = opened(Some("editor.general"));
        dialog.stage("editor.general", GLOBAL, "editor.showWhitespace", json!(true));
        assert_eq!(dialog.staged_value(GLOBAL, "editor.showWhitespace"), Some(json!(true)));
        assert_eq!(dialog.staged_value(PROJECT, "editor.showWhitespace"), Some(json!(false)));
        assert_eq!(dialog.base["editor"]["showWhitespace"], false);
    }

    // settings staging › compares the Global scope with the global layer, not the project's values
    #[test]
    fn compares_the_global_scope_with_the_global_layer_not_the_projects_values() {
        let mut dialog = opened(Some("editor.general"));
        dialog.base = set_at(&dialog.base, "editor.tabSize", json!(8));
        assert_eq!(dialog.staged_value(GLOBAL, "editor.tabSize"), Some(json!(4)));
        assert_eq!(dialog.staged_value(PROJECT, "editor.tabSize"), Some(json!(8)));
        // 8 is new for the global layer even though the project already has it.
        dialog.stage("editor.general", GLOBAL, "editor.tabSize", json!(8));
        let expected = StagedChange {
            scope: GLOBAL,
            key: "editor.tabSize".to_owned(),
            path: None,
            value: json!(8),
            page: "editor.general".to_owned(),
        };
        assert_eq!(dialog.pending_changes(), [expected]);
        dialog.stage("editor.general", PROJECT, "editor.tabSize", json!(8));
        assert_eq!(dialog.pending_changes().len(), 1);
    }

    // settings staging › stages entries of map settings and fields inside them
    #[test]
    fn stages_entries_of_map_settings_and_fields_inside_them() {
        let page = "languages.servers";
        let mut dialog = opened(Some("editor.general"));
        // A field of an entry nobody has set yet; reading the map shows it.
        dialog.stage(page, GLOBAL, "languageServers.rust-analyzer.args", json!(["--log"]));
        let shown = dialog.staged_value(GLOBAL, "languageServers");
        assert_eq!(shown, Some(json!({ "rust-analyzer": { "args": ["--log"] } })));
        // Cleared again: nothing to write.
        dialog.stage(page, GLOBAL, "languageServers.rust-analyzer.args", Value::Null);
        assert!(dialog.pending_changes().is_empty());

        // A new entry, then an edit inside it: one write of the whole entry.
        dialog.stage(page, GLOBAL, "languageServers.ruff", json!({ "command": "ruff", "languages": ["python"] }));
        dialog.stage(page, GLOBAL, "languageServers.ruff.args", json!(["server"]));
        dialog.stage(page, GLOBAL, "languageServers.ruff.command", Value::Null);
        assert_eq!(dialog.staged_value(GLOBAL, "languageServers.ruff.args"), Some(json!(["server"])));
        let entry = json!({ "languages": ["python"], "args": ["server"] });
        assert_eq!(keys_and_values(&dialog.pending_changes()), [("languageServers.ruff", &entry)]);

        // Removing an entry drops the edits staged inside it.
        dialog.stage(page, GLOBAL, "languageServers.gopls.env", json!({ "GOFLAGS": "-mod=mod" }));
        dialog.stage(page, GLOBAL, "languageServers.gopls", Value::Null);
        dialog.stage(page, GLOBAL, "languageServers.ruff", Value::Null);
        assert!(dialog.pending_changes().is_empty());
        assert_eq!(dialog.staged_value(GLOBAL, "languageServers"), Some(json!({})));
    }

    // settings staging › opens Settings for New Projects on request
    #[test]
    fn opens_settings_for_new_projects_on_request() {
        let mut dialog = opened(Some("editor.general"));
        assert!(!dialog.new_projects);
        dialog.open(Some("languages.java"), true, default_settings());
        assert!(dialog.new_projects);
        assert_eq!(dialog.page_id, "languages.java");
        dialog.open(None, false, default_settings());
        assert!(!dialog.new_projects);
        assert_eq!(dialog.page_id, "languages.java");
        assert_eq!(settings_title(true), "Settings for New Projects");
    }

    // settings staging › drops edits that return to the effective value
    #[test]
    fn drops_edits_that_return_to_the_effective_value() {
        let mut dialog = opened(Some("editor.general"));
        dialog.stage("editor.general", GLOBAL, "editor.tabSize", json!(2));
        dialog.stage("editor.general", GLOBAL, "editor.tabSize", json!(4));
        assert!(dialog.pending_changes().is_empty());
    }

    // settings staging › diffs into one settingsSet call per changed key, global before project
    #[test]
    fn diffs_into_one_write_per_changed_key_global_before_project() {
        let mut dialog = opened(Some("editor.general"));
        dialog.stage("editor.font", PROJECT, "editor.fontSize", json!(15));
        dialog.stage("editor.general", GLOBAL, "editor.showWhitespace", json!(true));
        dialog.stage("editor.files", GLOBAL, "files.exclude", json!(["**/dist"]));
        dialog.stage("keymap", GLOBAL, "keymapOverrides", json!({ "file.save": ["Ctrl+S"] }));
        let mut backend = FakeBackend::over(&dialog.base);
        assert!(apply(&mut dialog, &mut backend));
        let sets: Vec<(Scope, &str, &str)> =
            backend.sets.iter().map(|(scope, key, json)| (*scope, key.as_str(), json.as_str())).collect();
        assert_eq!(
            sets,
            [
                (GLOBAL, "editor.showWhitespace", "true"),
                (GLOBAL, "files.exclude", r#"["**/dist"]"#),
                (GLOBAL, "keymapOverrides", r#"{"file.save":["Ctrl+S"]}"#),
                (PROJECT, "editor.fontSize", "15"),
            ]
        );
        assert!(!dialog.has_staged());
        assert_eq!(dialog.base["editor"]["showWhitespace"], true);
        assert_eq!(backend.settings["files"]["exclude"], json!(["**/dist"]));
    }

    // settings staging › keeps failed and unsent edits staged and reports the error
    #[test]
    fn keeps_failed_and_unsent_edits_staged_and_reports_the_error() {
        let mut dialog = opened(Some("editor.general"));
        dialog.stage("appearance", GLOBAL, "appearance.theme", json!("dark"));
        dialog.stage("editor.general", GLOBAL, "editor.showWhitespace", json!(true));
        let mut backend = FakeBackend { fail: Some("editor.showWhitespace"), ..FakeBackend::over(&dialog.base) };
        assert!(!apply(&mut dialog, &mut backend));
        assert_eq!(backend.sets.len(), 2);
        assert_eq!(dialog.error.as_deref(), Some("editor.showWhitespace: invalid value"));
        assert_eq!(dialog.staged().map(|change| change.key.as_str()).collect::<Vec<_>>(), ["editor.showWhitespace"]);
        assert_eq!(dialog.base["appearance"]["theme"], "dark");
    }

    // settings staging › Reset discards one page's edits
    #[test]
    fn reset_discards_one_pages_edits() {
        let mut dialog = opened(Some("editor.general"));
        dialog.stage("appearance", GLOBAL, "appearance.theme", json!("dark"));
        dialog.stage("editor.general", GLOBAL, "editor.showWhitespace", json!(true));
        dialog.reset_page("appearance");
        assert_eq!(dialog.staged().map(|change| change.page.as_str()).collect::<Vec<_>>(), ["editor.general"]);
        dialog.close();
        assert!(!dialog.open && !dialog.has_staged());
    }

    // settings staging › applies to the session when there is no backend
    #[test]
    fn applies_to_the_session_when_there_is_no_backend() {
        let mut dialog = opened(Some("appearance"));
        dialog.stage("appearance", GLOBAL, "appearance.uiFontSize", json!(14));
        let ApplyStart::Run(mut run) = dialog.begin_apply() else { panic!("there is an edit to apply") };
        let written: Vec<String> = run.changes().iter().map(|change| stage_key(change.scope, &change.key)).collect();
        assert_eq!(written, ["global|appearance.uiFontSize"]);
        let session = session_settings(&default_settings(), run.changes());
        assert_eq!(session["appearance"]["uiFontSize"], 14);
        run.writes_done(&mut dialog, written.len(), None, session, false);
        assert_eq!(run.next_secret(&mut dialog, false), None);
        assert!(run.finish(&mut dialog));
        assert!(!dialog.has_staged() && !dialog.saving);
        assert_eq!(dialog.global_base["appearance"]["uiFontSize"], 14);
    }

    const SSH_ID: &str = "ssh-staging";

    fn ssh_config(save_secret: bool) -> Value {
        let config = SshConfiguration {
            id: SSH_ID.to_owned(),
            auth: SshAuth::Password,
            save_secret,
            ..SshConfiguration::default()
        };
        serde_json::to_value(config).unwrap_or(Value::Null)
    }

    /// A dialog on the SSH page over settings holding one password configuration.
    fn ssh_dialog() -> SettingsDialog {
        let mut dialog = SettingsDialog::default();
        let settings = set_at(&default_settings(), "ssh.configs", json!([ssh_config(false)]));
        dialog.open(Some(SSH_SETTINGS_PAGE), false, settings);
        dialog
    }

    // SSH secret staging › writes only the final value once, after settings, with Save=%s
    #[test]
    fn writes_only_the_final_value_once_after_settings() {
        for save_secret in [false, true] {
            let mut dialog = ssh_dialog();
            let mut backend = FakeBackend::over(&dialog.base);
            dialog.stage_ssh_secret(SSH_ID, "first");
            dialog.stage_ssh_secret(SSH_ID, "final");
            dialog.stage(SSH_SETTINGS_PAGE, GLOBAL, "ssh.configs", json!([ssh_config(save_secret)]));
            assert!(backend.secrets.is_empty());
            assert!(apply(&mut dialog, &mut backend));
            assert_eq!(backend.secrets, [(SSH_ID.to_owned(), "final".to_owned(), save_secret)]);
            // The configuration is written first when it changed.
            assert_eq!(backend.sets.len(), usize::from(save_secret));
            assert_eq!(backend.configured_at_secret, [true]);
            assert!(!dialog.has_staged_secrets());
            assert!(!backend.settings.to_string().contains("final"));
            assert!(apply(&mut dialog, &mut backend));
            assert_eq!(backend.secrets.len(), 1);
        }
    }

    // SSH secret staging › saves a new configuration before its credential
    #[test]
    fn saves_a_new_configuration_before_its_credential() {
        let mut dialog = opened(Some(SSH_SETTINGS_PAGE));
        let mut backend = FakeBackend::over(&dialog.base);
        dialog.stage(SSH_SETTINGS_PAGE, GLOBAL, "ssh.configs", json!([ssh_config(false)]));
        dialog.stage_ssh_secret(SSH_ID, "secret");
        assert!(apply(&mut dialog, &mut backend));
        assert_eq!(backend.configured_at_secret, [true]);
        assert_eq!(backend.secrets.len(), 1);
    }

    // SSH secret staging › Cancel discards the credential without writing it
    #[test]
    fn cancel_discards_the_credential_without_writing_it() {
        let mut dialog = ssh_dialog();
        let mut backend = FakeBackend::over(&dialog.base);
        dialog.stage_ssh_secret(SSH_ID, "secret");
        dialog.close();
        dialog.open(Some(SSH_SETTINGS_PAGE), false, backend.settings.clone());
        assert!(!dialog.has_staged_secrets());
        assert!(apply(&mut dialog, &mut backend));
        assert!(backend.secrets.is_empty());
    }

    // SSH secret staging › Reset clears only this page's credentials, and an empty field leaves storage untouched
    #[test]
    fn reset_clears_only_this_pages_credentials_and_an_empty_field_leaves_storage_untouched() {
        let mut dialog = ssh_dialog();
        let mut backend = FakeBackend::over(&dialog.base);
        dialog.stage_ssh_secret(SSH_ID, "secret");
        dialog.reset_page("appearance");
        assert_eq!(dialog.staged_secret(SSH_ID).map(Secret::expose), Some("secret"));
        dialog.reset_page(SSH_SETTINGS_PAGE);
        assert!(!dialog.has_staged_secrets());
        dialog.stage_ssh_secret(SSH_ID, "secret");
        dialog.stage_ssh_secret(SSH_ID, "");
        assert!(!dialog.has_staged_secrets());
        assert!(apply(&mut dialog, &mut backend));
        assert!(backend.secrets.is_empty());
    }

    // SSH secret staging › retains a failed write for retry without reapplying successful settings
    #[test]
    fn retains_a_failed_write_for_retry_without_reapplying_successful_settings() {
        let mut dialog = ssh_dialog();
        let mut backend = FakeBackend { failing_secrets: 1, ..FakeBackend::over(&dialog.base) };
        dialog.stage_ssh_secret(SSH_ID, "secret");
        dialog.stage("editor.general", GLOBAL, "editor.tabSize", json!(2));
        assert!(!apply(&mut dialog, &mut backend));
        assert_eq!(dialog.staged_secret(SSH_ID).map(Secret::expose), Some("secret"));
        assert_eq!(dialog.error.as_deref(), Some("SSH secret: Unavailable"));
        assert!(!dialog.saving && !dialog.has_staged());
        assert!(apply(&mut dialog, &mut backend));
        assert_eq!(backend.sets.len(), 1);
        assert_eq!(backend.secrets.len(), 2);
        assert!(!dialog.has_staged_secrets());
        assert_eq!(dialog.error, None);
    }

    // SSH secret staging › does not write credentials when settings fail
    #[test]
    fn does_not_write_credentials_when_settings_fail() {
        let mut dialog = ssh_dialog();
        let mut backend = FakeBackend { fail: Some("ssh.configs"), ..FakeBackend::over(&dialog.base) };
        dialog.stage_ssh_secret(SSH_ID, "secret");
        dialog.stage(SSH_SETTINGS_PAGE, GLOBAL, "ssh.configs", json!([]));
        assert!(!apply(&mut dialog, &mut backend));
        assert!(backend.secrets.is_empty());
        assert_eq!(dialog.staged_secret(SSH_ID).map(Secret::expose), Some("secret"));
    }

    // SSH secret staging › does not write a credential for a configuration removed before Apply
    #[test]
    fn does_not_write_a_credential_for_a_configuration_removed_before_apply() {
        let mut dialog = ssh_dialog();
        let mut backend = FakeBackend::over(&dialog.base);
        dialog.stage_ssh_secret(SSH_ID, "secret");
        dialog.stage(SSH_SETTINGS_PAGE, GLOBAL, "ssh.configs", json!([]));
        assert!(apply(&mut dialog, &mut backend));
        assert!(backend.secrets.is_empty());
        assert!(!dialog.has_staged_secrets());
    }

    // literal map entry staging › preserves literal dots, per-page ownership, and merges one write per scope
    #[test]
    fn preserves_literal_dots_per_page_ownership_and_merges_one_write_per_scope() {
        let mut dialog = opened(None);
        dialog.stage_map_entry("templates", GLOBAL, "plugins", &["example.helper", "prompt.with.dots"], json!(["one"]));
        let profiles = json!([{ "name": "Local" }]);
        dialog.stage_map_entry("llm", GLOBAL, "plugins", &["example.helper", "profiles"], profiles);
        dialog.stage_map_entry("other", PROJECT, "plugins", &["other.plugin", "enabled"], json!(true));
        assert_eq!(dialog.pending_changes().len(), 2);
        dialog.reset_page("llm");
        let helper = json!({ "example.helper": { "prompt.with.dots": ["one"] } });
        assert_eq!(dialog.staged_value(GLOBAL, "plugins"), Some(helper.clone()));
        let mut backend = FakeBackend::over(&dialog.base);
        assert!(apply(&mut dialog, &mut backend));
        let sets: Vec<(Scope, &str, Value)> = backend
            .sets
            .iter()
            .map(|(scope, key, json)| (*scope, key.as_str(), serde_json::from_str(json).unwrap_or(Value::Null)))
            .collect();
        let other = json!({ "other.plugin": { "enabled": true } });
        assert_eq!(sets, [(GLOBAL, "plugins", helper), (PROJECT, "plugins", other)]);
        assert!(!dialog.has_staged());
    }

    // literal map entry staging › removes only its own override and clears a reverted edit
    #[test]
    fn removes_only_its_own_override_and_clears_a_reverted_edit() {
        let mut dialog = opened(None);
        let base = set_at(&default_settings(), "plugins", json!({ "example.helper": { "a.b": 1, "keep": true } }));
        dialog.base = base.clone();
        dialog.global_base = base;
        dialog.stage_map_entry("a", GLOBAL, "plugins", &["example.helper", "a.b"], Value::Null);
        assert_eq!(dialog.staged_value(GLOBAL, "plugins"), Some(json!({ "example.helper": { "keep": true } })));
        dialog.stage_map_entry("a", GLOBAL, "plugins", &["example.helper", "a.b"], json!(1));
        assert!(!dialog.has_staged());
    }

    // inspection options and focus (issue #42) › opens Settings on an inspection: page#id, ancestors to expand
    #[test]
    fn opens_settings_on_an_inspection_page_and_id() {
        let target = split_page_target("editor.inspections#lsp:ruff:F401");
        assert_eq!(target, ("editor.inspections", Some("lsp:ruff:F401")));
        assert_eq!(split_page_target("editor.general"), ("editor.general", None));
    }

    // Not from a spec: an external value is staged against what is stored, written after the
    // settings and remembered as applied; Project Structure shares the staging and parses its
    // section; a second Apply while one runs is refused.
    #[test]
    fn stages_external_values_and_project_structure_sections() {
        let mut dialog = opened(Some("tools.docker#x"));
        assert_eq!(dialog.focus.as_deref(), Some("x"));
        dialog.stage_external("tools.docker", "socket", json!("/a"), &json!("/a"));
        assert_eq!(dialog.staged_external("tools.docker", "socket"), None);
        dialog.stage_external("tools.docker", "socket", json!("/b"), &json!("/a"));
        assert_eq!(dialog.staged_external("tools.docker", "socket"), Some(&json!("/b")));
        let ApplyStart::Run(mut run) = dialog.begin_apply() else { panic!("there is a value to apply") };
        assert_eq!(dialog.begin_apply(), ApplyStart::Busy);
        run.writes_done(&mut dialog, 0, None, default_settings(), true);
        let (id, value) = run.next_external(&dialog).expect("the external value");
        assert_eq!((id.as_str(), &value), ("tools.docker|socket", &json!("/b")));
        run.external_done(&mut dialog, Ok(json!("/b")));
        assert_eq!(run.next_external(&dialog), None);
        assert!(run.finish(&mut dialog));
        assert_eq!(dialog.external_applied("tools.docker", "socket"), Some(&json!("/b")));
        assert_eq!(dialog.begin_apply(), ApplyStart::Clean);

        dialog.open_project_structure(Some("projectStructure.modules"), default_settings());
        assert!(dialog.project_structure_open && !dialog.open);
        assert_eq!(dialog.project_structure_section, ProjectStructureSection::Modules);
        dialog.open_project_structure(Some("nonsense"), default_settings());
        assert_eq!(dialog.project_structure_section, ProjectStructureSection::Project);
        dialog.set_page_scope("editor.font", PROJECT);
        assert_eq!(dialog.scopes.get("editor.font"), Some(&PROJECT));
        dialog.close_project_structure();
        dialog.set_global_base(json!({}));
        assert_eq!(dialog.global_base, default_settings());
    }
}
