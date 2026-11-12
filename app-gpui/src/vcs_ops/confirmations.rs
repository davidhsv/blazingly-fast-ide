//! Settings › Version Control › Confirmation: files created in the IDE are added to Git (ask /
//! silently / never); tracked files deleted in the IDE have the deletion staged (`git rm`), with
//! the same three choices.
//!
//! One file operation is followed by a [`FileConfirmation`], in steps the caller drives:
//!
//! 1. [`begin`], then refresh the status;
//! 2. [`FileConfirmation::refreshed`]: done, a question to ask, or go on;
//! 3. after the question, [`FileConfirmation::answered`] (it may hand back a setting to save);
//! 4. refresh the status again when [`FileConfirmation::needs_refresh`], then
//!    [`FileConfirmation::stage`]: the files to stage and the operation's title;
//! 5. once staged (or failed), [`FileConfirmation::finished`]; when it says so, refresh the status
//!    and stage what [`FileConfirmation::settled`] returns.
//!
//! Every step checks that nothing voided the operation meanwhile (another project or repository,
//! the feature shut down).

use super::dialogs::{AskAnswer, AskButton, AskRequest};
use super::store::VcsState;
use super::unity_metadata::{MetadataAddition, ProjectState, UnityMetadataStaging, stage_title};
use crate::platform::i18n::{I18n, Param};
use crate::platform::paths::{ipc_path, is_same_or_inside};
use crate::platform::settings::SettingsWrite;
use crate::vcs::changes::{EffectiveKind, effective_kind, rel_to, strip_slash};
use ide_core::settings::VcsConfirmationSettings;
use serde_json::json;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileOperationKind {
    Created,
    Deleted,
}

/// Files the IDE created or deleted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileOperation {
    pub kind: FileOperationKind,
    pub paths: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmMode {
    Ask,
    Silently,
    Never,
}

/// The confirmation setting that applies to operations of `kind`.
pub fn confirm_mode(kind: FileOperationKind, settings: &VcsConfirmationSettings) -> ConfirmMode {
    let value = match kind {
        FileOperationKind::Created => &settings.on_create,
        FileOperationKind::Deleted => &settings.on_delete,
    };
    match value.as_str() {
        "silently" => ConfirmMode::Silently,
        "never" => ConfirmMode::Never,
        _ => ConfirmMode::Ask,
    }
}

fn below(inner: &str, outer: &str) -> bool {
    inner.strip_prefix(outer).is_some_and(|rest| rest.starts_with('/'))
}

/// Status paths of `kind` affected by `paths` (a deleted folder covers its content).
fn matching(paths: &[String], kind: EffectiveKind, vcs: &VcsState) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |path: &str| {
        if !out.iter().any(|known| known == path) {
            out.push(path.to_owned());
        }
    };
    for file in &vcs.status {
        if effective_kind(file) != Some(kind) || file.staged.is_some() {
            continue;
        }
        let path = strip_slash(&ipc_path(&file.path)).to_owned();
        for wanted in paths {
            if path == *wanted || below(&path, wanted) {
                add(&path);
            } else if below(wanted, &path) {
                // Inside an untracked folder git reports only the folder: add just the new path.
                add(wanted);
            }
        }
    }
    out
}

/// The question asked before `files` are added to or removed from Git.
fn confirm_files(kind: FileOperationKind, files: &[String], root: &str, i18n: &mut I18n) -> AskRequest {
    let add = kind == FileOperationKind::Created;
    let count = files.len();
    let listed: Vec<&str> = files.iter().take(10).map(|file| rel_to(root, file)).collect();
    let question = match (add, count) {
        (true, 1) => i18n.tx("Do you want to add the following file to Git?"),
        (true, _) => i18n.tx_built(&["Do you want to add the following ", " files to Git?"], &[Param::from(count)]),
        (false, 1) => i18n.tx("Do you want to remove the following file from Git?"),
        (false, _) => {
            i18n.tx_built(&["Do you want to remove the following ", " files from Git?"], &[Param::from(count)])
        }
    };
    AskRequest {
        title: if add { "Add File to Git" } else { "Remove File from Git" }.to_owned(),
        body: format!("{question}\n{}{}", listed.join("\n"), if count > 10 { "\n…" } else { "" }),
        warning: false,
        width: Some(480),
        checkbox: Some("Don't ask again".to_owned()),
        buttons: vec![AskButton { id: "ok", label: if add { "Add" } else { "Remove" }, primary: true }],
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfirmationStep {
    /// Nothing to stage.
    Done,
    /// Ask; the answer goes to [`FileConfirmation::answered`].
    Ask(AskRequest),
    /// Go on to staging.
    Stage,
}

/// What an answer came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answered {
    /// "Don't ask again" was ticked: the setting to save before going on.
    pub remember: Option<SettingsWrite>,
    /// Go on to staging.
    pub proceed: bool,
}

/// The `git add` (or `git rm`) to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageRequest {
    /// Title of the operation, in the UI language.
    pub title: String,
    pub files: Vec<String>,
}

/// One file operation on its way to the index.
#[derive(Debug)]
pub struct FileConfirmation {
    kind: FileOperationKind,
    mode: ConfirmMode,
    paths: Vec<String>,
    /// The status paths the operation covers.
    files: Vec<String>,
    addition: MetadataAddition,
}

/// Starts following `op`; `None` when it does not concern Git (no repository, turned off in the
/// settings, files outside the project).
pub fn begin(
    op: &FileOperation,
    vcs: &VcsState,
    settings: &VcsConfirmationSettings,
    metadata: &mut UnityMetadataStaging,
    project: &ProjectState,
) -> Option<FileConfirmation> {
    let root = vcs.root.as_deref().filter(|root| vcs.available && !root.is_empty())?;
    let mode = confirm_mode(op.kind, settings);
    if mode == ConfirmMode::Never {
        return None;
    }
    let paths: Vec<String> = op
        .paths
        .iter()
        .map(|path| strip_slash(&ipc_path(path)).to_owned())
        .filter(|path| is_same_or_inside(path, root))
        .collect();
    if paths.is_empty() {
        return None;
    }
    let created: &[String] = if op.kind == FileOperationKind::Created { &paths } else { &[] };
    let addition = metadata.begin(created, project);
    Some(FileConfirmation { kind: op.kind, mode, paths, files: Vec::new(), addition })
}

impl FileConfirmation {
    /// The status was refreshed: what the operation covers in it.
    pub fn refreshed(
        &mut self,
        vcs: &VcsState,
        metadata: &UnityMetadataStaging,
        project: &ProjectState,
        i18n: &mut I18n,
    ) -> ConfirmationStep {
        if !self.addition.current(metadata, project) {
            return ConfirmationStep::Done;
        }
        let kind = match self.kind {
            FileOperationKind::Created => EffectiveKind::Untracked,
            FileOperationKind::Deleted => EffectiveKind::Deleted,
        };
        self.files = matching(&self.paths, kind, vcs);
        if self.files.is_empty() {
            return ConfirmationStep::Done;
        }
        let confirmed = self.addition.include(&self.files, vcs);
        if self.mode == ConfirmMode::Ask {
            let root = vcs.root.as_deref().unwrap_or_default();
            ConfirmationStep::Ask(confirm_files(self.kind, &confirmed, root, i18n))
        } else {
            ConfirmationStep::Stage
        }
    }

    /// The question was answered.
    pub fn answered(&self, answer: &AskAnswer, metadata: &UnityMetadataStaging, project: &ProjectState) -> Answered {
        if !self.addition.current(metadata, project) {
            return Answered { remember: None, proceed: false };
        }
        let ok = answer.button == "ok";
        let key = match self.kind {
            FileOperationKind::Created => "vcs.confirmation.onCreate",
            FileOperationKind::Deleted => "vcs.confirmation.onDelete",
        };
        let value = json!(if ok { "silently" } else { "never" });
        Answered { remember: answer.checked.then_some(SettingsWrite { key, value }), proceed: ok }
    }

    /// Whether the status has to be refreshed before [`FileConfirmation::stage`].
    pub fn needs_refresh(&self) -> bool {
        self.addition.needs_refresh()
    }

    /// The files to stage now; `None` when nothing is left to stage or the operation was voided.
    pub fn stage(
        &mut self,
        vcs: &VcsState,
        metadata: &mut UnityMetadataStaging,
        project: &ProjectState,
        i18n: &mut I18n,
    ) -> Option<StageRequest> {
        let files = self.addition.before_stage(&self.files, metadata, vcs, project);
        if files.is_empty() || !self.addition.current(metadata, project) {
            return None;
        }
        let title = match (self.kind, files.len()) {
            (FileOperationKind::Created, count) => stage_title(count, i18n),
            (FileOperationKind::Deleted, 1) => i18n.tx("Remove 1 file from Git"),
            (FileOperationKind::Deleted, count) => {
                i18n.tx_built(&["Remove ", " files from Git"], &[Param::from(count)])
            }
        };
        Some(StageRequest { title, files })
    }

    /// The staging ended (`ok`: it succeeded). Returns whether the caller refreshes the status and
    /// then stages what [`FileConfirmation::settled`] returns.
    pub fn finished(&mut self, ok: bool, metadata: &mut UnityMetadataStaging, project: &ProjectState) -> bool {
        self.addition.finished(ok, metadata, project)
    }

    /// The metadata files to stage after the refresh [`FileConfirmation::finished`] asked for.
    pub fn settled(&self, metadata: &mut UnityMetadataStaging, vcs: &VcsState, project: &ProjectState) -> Vec<String> {
        self.addition.settled(metadata, vcs, project)
    }
}

#[cfg(test)]
mod tests {
    use super::super::ops::{VcsConsole, op_failed};
    use super::*;
    use crate::vcs::changes::{ChangeKind, FileStatus};
    use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
    use std::collections::VecDeque;

    const ASSET: &str = "/p/Assets/New.cs";
    const META: &str = "/p/Assets/New.cs.meta";

    fn fs(path: &str, staged: Option<ChangeKind>, unstaged: Option<ChangeKind>) -> FileStatus {
        let rel_path = path.get(3..).unwrap_or_default().to_owned();
        FileStatus { path: path.to_owned(), rel_path, staged, unstaged, renamed_from: None }
    }

    fn new_file(path: &str) -> FileStatus {
        fs(path, None, Some(ChangeKind::Untracked))
    }

    fn added(path: &str) -> FileStatus {
        fs(path, Some(ChangeKind::Added), None)
    }

    fn added_modified(path: &str) -> FileStatus {
        fs(path, Some(ChangeKind::Added), Some(ChangeKind::Modified))
    }

    type Hook = Box<dyn FnOnce(&mut World)>;

    fn hook(run: impl FnOnce(&mut World) + 'static) -> Hook {
        Box::new(run)
    }

    /// What the next `git add` does instead of staging its files and succeeding.
    enum StageMock {
        /// Succeeds after running the hook.
        Then(Hook),
        Fail(&'static str),
        /// Stays in flight until [`World::complete`].
        Deferred,
    }

    enum Staged {
        Ok,
        Failed(String),
        Pending,
    }

    enum InFlight {
        Initial(Box<FileConfirmation>, String),
        Metadata(String),
    }

    /// The window around the confirmations: repository and project state, the settings, and a
    /// driver that carries the steps out the way a view does.
    struct World {
        vcs: VcsState,
        project: ProjectState,
        settings: VcsConfirmationSettings,
        metadata: UnityMetadataStaging,
        console: VcsConsole,
        i18n: I18n,
        /// The files of every `git add`.
        staged: Vec<Vec<String>>,
        /// The bodies of the questions asked.
        asked: Vec<String>,
        notified: Vec<NotificationRequest>,
        /// Runs while the question is open.
        ask_hook: Option<Hook>,
        decline: bool,
        /// What the next status refreshes do, in order (`None`: nothing).
        refresh_hooks: VecDeque<Option<Hook>>,
        stage_mocks: VecDeque<StageMock>,
        in_flight: Option<InFlight>,
    }

    impl World {
        fn new() -> Self {
            let root = "/p".to_owned();
            let settings = VcsConfirmationSettings {
                on_create: "ask".to_owned(),
                on_delete: "never".to_owned(),
                ..VcsConfirmationSettings::default()
            };
            Self {
                vcs: VcsState {
                    available: true,
                    root: Some(root.clone()),
                    roots: vec![root.clone()],
                    repo: Some(root.clone()),
                    status_loaded: true,
                    status: vec![new_file(ASSET)],
                    ..VcsState::default()
                },
                project: ProjectState { root: Some(root), unity: true, opening: false },
                settings,
                metadata: UnityMetadataStaging::default(),
                console: VcsConsole::default(),
                i18n: I18n::default(),
                staged: Vec::new(),
                asked: Vec::new(),
                notified: Vec::new(),
                ask_hook: None,
                decline: false,
                refresh_hooks: VecDeque::new(),
                stage_mocks: VecDeque::new(),
                in_flight: None,
            }
        }

        fn set_vcs(&mut self, change: impl FnOnce(&mut VcsState)) {
            let prev = self.vcs.clone();
            change(&mut self.vcs);
            let files = self.metadata.vcs_changed(&prev, &self.vcs);
            self.stage_metadata(files);
        }

        fn publish(&mut self, status: Vec<FileStatus>) {
            self.set_vcs(|vcs| vcs.status = status);
        }

        fn set_project(&mut self, change: impl FnOnce(&mut ProjectState)) {
            let prev = self.project.clone();
            change(&mut self.project);
            self.metadata.project_changed(&prev, &self.project);
        }

        fn set_root(&mut self, root: &str) {
            self.set_project(|project| project.root = Some(root.to_owned()));
            self.set_vcs(|vcs| {
                vcs.root = Some(root.to_owned());
                vcs.roots = vec![root.to_owned()];
                vcs.repo = Some(root.to_owned());
            });
        }

        fn refresh(&mut self) {
            if let Some(Some(hook)) = self.refresh_hooks.pop_front() {
                hook(self);
            }
        }

        fn stage(&mut self, files: Vec<String>) -> Staged {
            self.staged.push(files.clone());
            match self.stage_mocks.pop_front() {
                None => {
                    let staged = |file: &FileStatus| {
                        let mut file = file.clone();
                        if files.contains(&file.path) {
                            file.staged = Some(ChangeKind::Added);
                            file.unstaged = None;
                        }
                        file
                    };
                    let status = self.vcs.status.iter().map(staged).collect();
                    self.publish(status);
                    Staged::Ok
                }
                Some(StageMock::Then(hook)) => {
                    hook(self);
                    Staged::Ok
                }
                Some(StageMock::Fail(message)) => Staged::Failed(message.to_owned()),
                Some(StageMock::Deferred) => Staged::Pending,
            }
        }

        /// The operation around a `git add` ended: a failure is reported, the status refreshed.
        fn op_ended(&mut self, title: &str, result: &Staged) -> bool {
            let failure = match result {
                Staged::Failed(message) => op_failed(&mut self.console, title, message, true, &mut self.i18n, 0.0),
                Staged::Ok | Staged::Pending => None,
            };
            let ok = failure.is_none();
            self.notified.extend(failure);
            self.refresh();
            ok
        }

        /// The IDE created the files at `paths`.
        fn create(&mut self, paths: &[&str]) {
            let paths = paths.iter().map(|path| (*path).to_owned()).collect();
            let op = FileOperation { kind: FileOperationKind::Created, paths };
            let begun = begin(&op, &self.vcs, &self.settings, &mut self.metadata, &self.project);
            let Some(mut flow) = begun else { return };
            self.refresh();
            match flow.refreshed(&self.vcs, &self.metadata, &self.project, &mut self.i18n) {
                ConfirmationStep::Done => return,
                ConfirmationStep::Stage => {}
                ConfirmationStep::Ask(request) => {
                    self.asked.push(request.body);
                    if let Some(hook) = self.ask_hook.take() {
                        hook(self);
                    }
                    let answer = AskAnswer::new(if self.decline { "cancel" } else { "ok" }, false);
                    if !flow.answered(&answer, &self.metadata, &self.project).proceed {
                        return;
                    }
                }
            }
            if flow.needs_refresh() {
                self.refresh();
            }
            let request = flow.stage(&self.vcs, &mut self.metadata, &self.project, &mut self.i18n);
            let Some(StageRequest { title, files }) = request else { return };
            match self.stage(files) {
                Staged::Pending => self.in_flight = Some(InFlight::Initial(Box::new(flow), title)),
                result => self.initial_ended(flow, &title, &result),
            }
        }

        fn initial_ended(&mut self, mut flow: FileConfirmation, title: &str, result: &Staged) {
            let ok = self.op_ended(title, result);
            if flow.finished(ok, &mut self.metadata, &self.project) {
                self.refresh();
                let files = flow.settled(&mut self.metadata, &self.vcs, &self.project);
                self.stage_metadata(files);
            }
        }

        fn stage_metadata(&mut self, mut files: Vec<String>) {
            while !files.is_empty() {
                let title = stage_title(files.len(), &mut self.i18n);
                let result = self.stage(files);
                if matches!(result, Staged::Pending) {
                    self.in_flight = Some(InFlight::Metadata(title));
                    return;
                }
                self.op_ended(&title, &result);
                files = self.metadata.stage_finished(&self.vcs);
            }
        }

        /// The `git add` in flight succeeds.
        fn complete(&mut self) {
            match self.in_flight.take() {
                Some(InFlight::Initial(flow, title)) => self.initial_ended(*flow, &title, &Staged::Ok),
                Some(InFlight::Metadata(title)) => {
                    self.op_ended(&title, &Staged::Ok);
                    let files = self.metadata.stage_finished(&self.vcs);
                    self.stage_metadata(files);
                }
                None => panic!("no git add is in flight"),
            }
        }
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › includes existing metadata in the confirmation and stages both files
    #[test]
    fn includes_existing_metadata_in_the_confirmation_and_stages_both_files() {
        let mut world = World::new();
        world.publish(vec![new_file(ASSET), new_file(META)]);
        world.create(&[ASSET]);
        assert!(world.asked[0].contains("Assets/New.cs.meta"));
        assert_eq!(world.staged, [vec![ASSET, META]]);
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › adds delayed metadata only, preserving later unstaged script edits
    #[test]
    fn adds_delayed_metadata_only_preserving_later_unstaged_script_edits() {
        let mut world = World::new();
        world.create(&[ASSET]);
        world.publish(vec![added_modified(ASSET), new_file(META), new_file("/p/Assets/Other.cs.meta")]);
        assert_eq!(world.staged, [vec![ASSET], vec![META]]);
        assert_eq!(world.vcs.status[0].unstaged, Some(ChangeKind::Modified));
        assert_eq!(world.asked.len(), 1);
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › finds metadata generated while the confirmation is open
    #[test]
    fn finds_metadata_generated_while_the_confirmation_is_open() {
        let mut world = World::new();
        world.ask_hook = Some(hook(|world| world.publish(vec![new_file(ASSET), new_file(META)])));
        world.create(&[ASSET]);
        assert_eq!(world.staged, [vec![ASSET, META]]);
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › also pairs metadata in silent mode
    #[test]
    fn also_pairs_metadata_in_silent_mode() {
        let mut world = World::new();
        world.settings.on_create = "silently".to_owned();
        world.create(&[ASSET]);
        world.publish(vec![added(ASSET), new_file(META)]);
        assert!(world.asked.is_empty());
        assert_eq!(world.staged.last(), Some(&vec![META.to_owned()]));
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › waits for a completed status refresh before adding delayed metadata
    #[test]
    fn waits_for_a_completed_status_refresh_before_adding_delayed_metadata() {
        let mut world = World::new();
        world.create(&[ASSET]);
        world.set_vcs(|vcs| vcs.loading = true);
        world.publish(vec![added(ASSET), new_file(META)]);
        assert_eq!(world.staged.len(), 1);
        world.set_vcs(|vcs| vcs.loading = false);
        assert_eq!(world.staged.last(), Some(&vec![META.to_owned()]));
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › finds metadata generated during the initial staging operation
    #[test]
    fn finds_metadata_generated_during_the_initial_staging_operation() {
        let mut world = World::new();
        let generated = hook(|world| world.publish(vec![added(ASSET), new_file(META)]));
        world.stage_mocks.push_back(StageMock::Then(generated));
        world.create(&[ASSET]);
        assert_eq!(world.staged, [vec![ASSET], vec![META]]);
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › does not follow metadata after declined / never / failure
    #[test]
    fn does_not_follow_metadata_after_declined_never_or_failure() {
        for reason in ["declined", "never", "failure"] {
            let mut world = World::new();
            match reason {
                "declined" => world.decline = true,
                "never" => world.settings.on_create = "never".to_owned(),
                _ => world.stage_mocks.push_back(StageMock::Fail("stage failed")),
            }
            world.create(&[ASSET]);
            let calls = world.staged.len();
            world.publish(vec![added(ASSET), new_file(META)]);
            assert_eq!(world.staged.len(), calls, "{reason}");
        }
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › cancels pending metadata after unstage / delete / switch / opening / repository / dispose
    #[test]
    fn cancels_pending_metadata_after_the_addition_is_taken_back_or_voided() {
        for reason in ["unstage", "delete", "switch", "opening", "repository", "dispose"] {
            let mut world = World::new();
            world.create(&[ASSET]);
            match reason {
                "unstage" => world.publish(vec![new_file(ASSET)]),
                "delete" => world.publish(vec![fs(ASSET, Some(ChangeKind::Added), Some(ChangeKind::Deleted))]),
                "switch" => world.set_project(|project| project.root = Some("/other".to_owned())),
                "opening" => {
                    world.set_project(|project| project.opening = true);
                    world.set_project(|project| project.opening = false);
                }
                "repository" => {
                    world.set_vcs(|vcs| vcs.repo = Some("/p/other".to_owned()));
                    world.set_vcs(|vcs| vcs.repo = Some("/p".to_owned()));
                }
                _ => world.metadata.dispose(),
            }
            world.publish(vec![added(ASSET), new_file(META)]);
            assert_eq!(world.staged.len(), 1, "{reason}");
        }
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › does not pair files in normal project / outside Assets / metadata itself
    #[test]
    fn does_not_pair_files_in_a_normal_project_outside_assets_or_metadata_itself() {
        for reason in ["normal project", "outside Assets", "metadata itself"] {
            let mut world = World::new();
            if reason == "normal project" {
                world.set_project(|project| project.unity = false);
            }
            let path = match reason {
                "outside Assets" => "/p/New.cs",
                "metadata itself" => META,
                _ => ASSET,
            };
            world.publish(vec![new_file(path), new_file(&format!("{path}.meta"))]);
            world.create(&[path]);
            assert_eq!(world.staged, [vec![path]], "{reason}");
        }
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › does not stage modified / ignored metadata
    #[test]
    fn does_not_stage_modified_or_ignored_metadata() {
        for kind in [ChangeKind::Modified, ChangeKind::Ignored] {
            let mut world = World::new();
            world.create(&[ASSET]);
            world.publish(vec![added(ASSET), fs(META, None, Some(kind))]);
            assert_eq!(world.staged.len(), 1, "{kind:?}");
        }
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › does not repeatedly retry a failed metadata addition
    #[test]
    fn does_not_repeatedly_retry_a_failed_metadata_addition() {
        let mut world = World::new();
        world.create(&[ASSET]);
        world.stage_mocks.push_back(StageMock::Fail("index locked"));
        world.publish(vec![added(ASSET), new_file(META)]);
        world.publish(vec![added(ASSET), new_file(META)]);
        assert_eq!(world.staged.len(), 2);
        let reported = |notice: &NotificationRequest| {
            notice.level == NotificationLevel::Error && notice.body.as_deref() == Some("index locked")
        };
        assert!(world.notified.iter().any(reported));
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › does not stage files if the project changed while the dialog was open
    #[test]
    fn does_not_stage_files_if_the_project_changed_while_the_dialog_was_open() {
        let mut world = World::new();
        world.ask_hook = Some(hook(|world| {
            let other = ProjectState { root: Some("/other".to_owned()), ..ProjectState::default() };
            world.set_project(|project| *project = other);
        }));
        world.create(&[ASSET]);
        assert!(world.staged.is_empty());
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › discards an open confirmation after repository / opening / dispose
    #[test]
    fn discards_an_open_confirmation_after_repository_opening_or_dispose() {
        for reason in ["repository", "opening", "dispose"] {
            let mut world = World::new();
            world.ask_hook = Some(hook(move |world| match reason {
                "repository" => world.set_vcs(|vcs| vcs.repo = Some("/p/nested".to_owned())),
                "opening" => world.set_project(|project| project.opening = true),
                _ => world.metadata.dispose(),
            }));
            world.create(&[ASSET]);
            assert!(world.staged.is_empty(), "{reason}");
        }
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › does not stage a source or metadata staged by the user while the dialog was open
    #[test]
    fn does_not_stage_a_source_or_metadata_staged_by_the_user_while_the_dialog_was_open() {
        let mut world = World::new();
        world.publish(vec![new_file(ASSET), new_file(META)]);
        world.ask_hook = Some(hook(|world| world.publish(vec![added_modified(ASSET), added_modified(META)])));
        world.create(&[ASSET]);
        assert!(world.staged.is_empty());
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › preserves manually staged metadata even when it was explicitly included in the creation event
    #[test]
    fn preserves_manually_staged_metadata_even_when_it_was_explicitly_included_in_the_creation_event() {
        let mut world = World::new();
        world.publish(vec![new_file(ASSET), new_file(META)]);
        world.ask_hook = Some(hook(|world| world.publish(vec![new_file(ASSET), added_modified(META)])));
        world.create(&[ASSET, META]);
        assert_eq!(world.staged, [vec![ASSET]]);
        assert_eq!(world.vcs.status[1].unstaged, Some(ChangeKind::Modified));
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › rechecks metadata that becomes added / modified / ignored / missing while the dialog is open
    #[test]
    fn rechecks_metadata_that_changes_while_the_dialog_is_open() {
        let changed = [
            Some(added_modified(META)),
            Some(fs(META, None, Some(ChangeKind::Modified))),
            Some(fs(META, None, Some(ChangeKind::Ignored))),
            None,
        ];
        for meta in changed {
            let mut world = World::new();
            world.publish(vec![new_file(ASSET), new_file(META)]);
            let label = format!("{meta:?}");
            world.ask_hook = Some(hook(move |world| {
                world.publish(std::iter::once(new_file(ASSET)).chain(meta).collect());
            }));
            world.create(&[ASSET]);
            assert_eq!(world.staged, [vec![ASSET]], "{label}");
        }
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › waits for successful staging before following metadata
    #[test]
    fn waits_for_successful_staging_before_following_metadata() {
        let mut world = World::new();
        world.stage_mocks.push_back(StageMock::Deferred);
        world.create(&[ASSET]);
        world.publish(vec![added(ASSET), new_file(META)]);
        assert_eq!(world.staged.len(), 1);
        world.complete();
        assert_eq!(world.staged, [vec![ASSET], vec![META]]);
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › remembers an unstage while the initial staging response is still in flight
    #[test]
    fn remembers_an_unstage_while_the_initial_staging_response_is_still_in_flight() {
        let mut world = World::new();
        world.stage_mocks.push_back(StageMock::Deferred);
        world.create(&[ASSET]);
        world.publish(vec![added(ASSET)]);
        world.publish(vec![new_file(ASSET)]);
        world.publish(vec![added(ASSET), new_file(META)]);
        world.complete();
        assert_eq!(world.staged, [vec![ASSET]]);
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › does not re-add metadata already included in the successful initial operation
    #[test]
    fn does_not_re_add_metadata_already_included_in_the_successful_initial_operation() {
        let mut world = World::new();
        world.publish(vec![new_file(ASSET), new_file(META)]);
        world.stage_mocks.push_back(StageMock::Deferred);
        world.create(&[ASSET]);
        world.publish(vec![added(ASSET), added(META)]);
        world.publish(vec![added(ASSET), new_file(META)]);
        world.complete();
        assert_eq!(world.staged, [vec![ASSET, META]]);
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › does not follow metadata after an in-flight addition outlives its project
    #[test]
    fn does_not_follow_metadata_after_an_in_flight_addition_outlives_its_project() {
        let mut world = World::new();
        world.stage_mocks.push_back(StageMock::Deferred);
        world.create(&[ASSET]);
        world.set_project(|project| project.opening = true);
        world.complete();
        world.publish(vec![added(ASSET), new_file(META)]);
        assert_eq!(world.staged.len(), 1);
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › ignores incomplete or failed status snapshots
    #[test]
    fn ignores_incomplete_or_failed_status_snapshots() {
        let mut world = World::new();
        world.create(&[ASSET]);
        world.set_vcs(|vcs| vcs.status_loaded = false);
        world.publish(vec![new_file(ASSET)]);
        world.set_vcs(|vcs| {
            vcs.status_loaded = true;
            vcs.error = Some("refresh failed".to_owned());
        });
        world.publish(vec![added_modified(ASSET), new_file(META)]);
        assert_eq!(world.staged.len(), 1);
        world.set_vcs(|vcs| vcs.error = None);
        assert_eq!(world.staged.last(), Some(&vec![META.to_owned()]));
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › does not cancel an addition when the first refresh discovers the repository
    #[test]
    fn does_not_cancel_an_addition_when_the_first_refresh_discovers_the_repository() {
        let mut world = World::new();
        world.set_vcs(|vcs| {
            vcs.repo = None;
            vcs.roots.clear();
            vcs.status_loaded = false;
        });
        let discover = hook(|world| {
            world.set_vcs(|vcs| {
                vcs.roots = vec!["/p".to_owned()];
                vcs.repo = Some("/p".to_owned());
                vcs.status_loaded = true;
            });
        });
        world.refresh_hooks.push_back(Some(discover));
        world.create(&[ASSET]);
        world.publish(vec![added(ASSET), new_file(META)]);
        assert_eq!(world.staged, [vec![ASSET], vec![META]]);
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › does not stage from a failed pre-addition refresh
    #[test]
    fn does_not_stage_from_a_failed_pre_addition_refresh() {
        let mut world = World::new();
        world.ask_hook = Some(hook(|world| world.set_vcs(|vcs| vcs.error = Some("refresh failed".to_owned()))));
        world.create(&[ASSET]);
        assert!(world.staged.is_empty());
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › rechecks the project after the pre-staging promise resolves
    #[test]
    fn rechecks_the_project_after_the_pre_staging_refresh() {
        let mut world = World::new();
        world.refresh_hooks.push_back(None);
        world.refresh_hooks.push_back(Some(hook(|world| world.set_project(|project| project.opening = true))));
        world.create(&[ASSET]);
        assert!(world.staged.is_empty());
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › does not infer metadata consent for children of a copied directory
    #[test]
    fn does_not_infer_metadata_consent_for_children_of_a_copied_directory() {
        let mut world = World::new();
        world.create(&["/p/Assets"]);
        assert_eq!(world.staged, [vec![ASSET]]);
        world.publish(vec![added(ASSET), new_file(META)]);
        assert_eq!(world.staged.len(), 1);
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › keeps a batch limited to the explicitly created files
    #[test]
    fn keeps_a_batch_limited_to_the_explicitly_created_files() {
        let other = "/p/Assets/Other.cs";
        let mut world = World::new();
        world.publish(vec![new_file(ASSET), new_file(META), new_file(other), new_file(&format!("{other}.meta"))]);
        world.create(&[ASSET, ASSET]);
        assert_eq!(world.staged, [vec![ASSET, META]]);
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › pairs files with native or literal path characters under %s
    #[test]
    fn pairs_files_with_native_or_literal_path_characters() {
        let share = "//server/share/project";
        let rows = [
            ("D:/Unity Project", "D:\\Unity Project\\Assets\\New.cs", "D:/Unity Project/Assets/New.cs"),
            (share, "\\\\server\\share\\project\\Assets\\New.cs", "//server/share/project/Assets/New.cs"),
            ("/p", "/p/Assets/Name\\Part.cs", "/p/Assets/Name\\Part.cs"),
            ("/project space", "/project space/Assets/角色.cs", "/project space/Assets/角色.cs"),
        ];
        for (root, input, path) in rows {
            let mut world = World::new();
            world.set_root(root);
            let meta = format!("{path}.meta");
            world.publish(vec![new_file(path), new_file(&meta)]);
            world.create(&[input]);
            assert_eq!(world.staged, [vec![path, meta.as_str()]], "{root}");
        }
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › does not pair %s outside Assets
    #[test]
    fn does_not_pair_files_outside_assets() {
        for path in ["/p/AssetsOther/New.cs", "/p/Packages/New.cs"] {
            let mut world = World::new();
            world.publish(vec![new_file(path), new_file(&format!("{path}.meta"))]);
            world.create(&[path]);
            assert_eq!(world.staged, [vec![path]]);
        }
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › does not add files from another project's similarly named path
    #[test]
    fn does_not_add_files_from_another_projects_similarly_named_path() {
        let other = "/project/Assets/New.cs";
        let mut world = World::new();
        world.publish(vec![new_file(other), new_file(&format!("{other}.meta"))]);
        world.create(&[other]);
        assert!(world.staged.is_empty());
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › cancels metadata when the asset's repository disappears
    #[test]
    fn cancels_metadata_when_the_assets_repository_disappears() {
        let mut world = World::new();
        world.set_vcs(|vcs| vcs.roots = vec!["/p".to_owned(), "/p/Assets".to_owned()]);
        world.create(&[ASSET]);
        world.set_vcs(|vcs| vcs.roots = vec!["/p".to_owned()]);
        world.publish(vec![added(ASSET), new_file(META)]);
        assert_eq!(world.staged.len(), 1);
    }

    // src/features/vcs/__tests__/confirmations.test.ts › Unity new-file Git confirmations › still cancels other pending additions while metadata staging is in flight
    #[test]
    fn still_cancels_other_pending_additions_while_metadata_staging_is_in_flight() {
        let other = "/p/Assets/Other.cs";
        let other_meta = format!("{other}.meta");
        let mut world = World::new();
        world.publish(vec![new_file(ASSET), new_file(other)]);
        world.create(&[ASSET, other]);
        world.stage_mocks.push_back(StageMock::Deferred);
        world.publish(vec![added(ASSET), new_file(META), added(other)]);
        world.publish(vec![added(ASSET), new_file(META), new_file(other)]);
        world.publish(vec![added(ASSET), added(META), added(other), new_file(&other_meta)]);
        world.complete();
        assert_eq!(world.staged, [vec![ASSET, other], vec![META]]);
    }

    // Not from a spec: a deleted tracked file is asked about, and "Don't ask again" saves the answer.
    #[test]
    fn asks_before_staging_a_deletion_and_remembers_the_answer() {
        let mut world = World::new();
        world.settings.on_delete = "ask".to_owned();
        world.vcs.status = vec![fs("/p/src/a.ts", None, Some(ChangeKind::Deleted))];
        let op = FileOperation { kind: FileOperationKind::Deleted, paths: vec!["/p/src".to_owned()] };
        let mut flow = begin(&op, &world.vcs, &world.settings, &mut world.metadata, &world.project).unwrap();
        let step = flow.refreshed(&world.vcs, &world.metadata, &world.project, &mut world.i18n);
        let ConfirmationStep::Ask(request) = step else { panic!("expected a question") };
        assert_eq!(request.title, "Remove File from Git");
        assert_eq!(request.body, "Do you want to remove the following file from Git?\nsrc/a.ts");
        let answered = flow.answered(&AskAnswer::new("ok", true), &world.metadata, &world.project);
        let remember = SettingsWrite { key: "vcs.confirmation.onDelete", value: json!("silently") };
        assert_eq!(answered, Answered { remember: Some(remember), proceed: true });
        assert!(!flow.needs_refresh());
        let request = flow.stage(&world.vcs, &mut world.metadata, &world.project, &mut world.i18n).unwrap();
        assert_eq!(request.title, "Remove 1 file from Git");
        assert_eq!(request.files, ["/p/src/a.ts"]);
        assert!(!flow.finished(true, &mut world.metadata, &world.project));
    }
}
