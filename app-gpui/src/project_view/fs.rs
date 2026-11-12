//! File system access of the project view, as data: what the view asks of the project's file
//! system, the backend command each request comes to, and the task an upload of client files
//! to a remote project shows.

use crate::platform::i18n::I18n;
use crate::project_tree_paths::name_of;
use crate::vcs_ops::confirmations::{FileOperation, FileOperationKind};
use crate::workbench::tasks::{Cancel, Change, TaskId, TaskUpdate};
use ide_core::{IdeError, IdeResult, RequestId};

/// What the project view asks of the project's file system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsRequest {
    ReadDir(String),
    CreateFile(String),
    CreateDir(String),
    Rename {
        from: String,
        to: String,
    },
    /// Moves to the trash when possible.
    Delete(String),
    Copy {
        from: String,
        to: String,
    },
    /// Copies a file of the machine the UI runs on into the project. It answers with the entries
    /// that were skipped, each as `name: reason`.
    ImportLocal {
        from: String,
        to: String,
    },
}

/// A command of the backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendCall {
    FsReadDir(String),
    FsCreateFile(String),
    FsCreateDir(String),
    FsRename {
        from: String,
        to: String,
    },
    FsDelete(String),
    FsCopy {
        from: String,
        to: String,
    },
    /// Uploads a client file to the remote host. It runs on the client, so it is cancelled
    /// through the client's own `cancel_request`, not the remote one.
    RemoteImportLocal {
        from: String,
        to: String,
        request: RequestId,
    },
}

/// The backend command of `request`. An import is a copy within the project's file system; in a
/// remote window it is an upload instead, which [`LocalImport::upload`] starts.
pub fn backend_call(request: &FsRequest) -> BackendCall {
    match request {
        FsRequest::ReadDir(path) => BackendCall::FsReadDir(path.clone()),
        FsRequest::CreateFile(path) => BackendCall::FsCreateFile(path.clone()),
        FsRequest::CreateDir(path) => BackendCall::FsCreateDir(path.clone()),
        FsRequest::Rename { from, to } => BackendCall::FsRename { from: from.clone(), to: to.clone() },
        FsRequest::Delete(path) => BackendCall::FsDelete(path.clone()),
        FsRequest::Copy { from, to } | FsRequest::ImportLocal { from, to } => {
            BackendCall::FsCopy { from: from.clone(), to: to.clone() }
        }
    }
}

/// What a request that succeeded announces to the features following explicit file operations
/// (version control asks whether to add or remove the files).
pub fn file_operation(request: &FsRequest) -> Option<FileOperation> {
    let (kind, path) = match request {
        FsRequest::CreateFile(path) => (FileOperationKind::Created, path),
        FsRequest::Delete(path) => (FileOperationKind::Deleted, path),
        FsRequest::Copy { to, .. } | FsRequest::ImportLocal { to, .. } => (FileOperationKind::Created, to),
        FsRequest::ReadDir(_) | FsRequest::CreateDir(_) | FsRequest::Rename { .. } => return None,
    };
    Some(FileOperation { kind, paths: vec![path.clone()] })
}

/// An upload of client files to a remote project. One task spans every path of the batch, and
/// cancelling it also stops the paths not started yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalImport {
    /// The id of the task, from the request the batch began with.
    task: String,
    title: String,
    /// The name of the file being uploaded.
    message: Option<String>,
    /// The request of the upload under way.
    request: RequestId,
    cancelled: bool,
}

impl LocalImport {
    /// Begins a batch; `request` is a fresh request id. Returns the task to show.
    pub fn begin(request: RequestId, i18n: &I18n) -> (Self, TaskUpdate) {
        let import = Self {
            task: format!("file-import-{}", request.0),
            title: i18n.tx("Uploading files"),
            message: None,
            request,
            cancelled: false,
        };
        let shown = import.task(false);
        (import, shown)
    }

    fn task(&self, done: bool) -> TaskUpdate {
        TaskUpdate {
            message: self.message.clone(),
            done,
            cancel: Change::Set(Cancel::Producer),
            ..TaskUpdate::new(TaskId::Name(self.task.clone()), &self.title)
        }
    }

    pub fn task_id(&self) -> TaskId {
        TaskId::Name(self.task.clone())
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// The task's Cancel was chosen. Returns the request to cancel, through the client's own
    /// `cancel_request`: the upload runs there.
    pub fn cancel(&mut self) -> RequestId {
        self.cancelled = true;
        self.request
    }

    /// Starts the upload of `from` to `to` under the fresh `request`: the command to make and
    /// the task as it now shows. A cancelled batch starts nothing.
    pub fn upload(&mut self, from: &str, to: &str, request: RequestId) -> IdeResult<(BackendCall, TaskUpdate)> {
        if self.cancelled {
            return Err(IdeError::cancelled());
        }
        self.request = request;
        self.message = Some(name_of(from).to_owned());
        let call = BackendCall::RemoteImportLocal { from: from.to_owned(), to: to.to_owned(), request };
        Ok((call, self.task(false)))
    }

    /// The batch ended, whatever became of its uploads: the task as finished.
    pub fn finish(&self) -> TaskUpdate {
        self.task(true)
    }
}

#[cfg(test)]
pub mod testing {
    //! An in-memory file system with the backend's semantics (no overwrite, folders first), and
    //! the loop that reads folders of it into a view.

    use super::FsRequest;
    use crate::platform::collate;
    use crate::project_tree_paths::{is_inside, name_of, parent_of};
    use crate::project_view::store::ProjectViewState;
    use ide_core::error::ErrorKind;
    use ide_core::{IdeError, IdeResult};
    use ide_vfs::{EntryKind, FileEntry};
    use std::collections::{BTreeMap, BTreeSet};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MemNode {
        File { size: u64 },
        Dir,
    }

    #[derive(Debug, Clone, Default)]
    pub struct MemoryFs {
        pub nodes: BTreeMap<String, MemNode>,
        /// Folders whose content lists as ignored and excluded.
        pub ignored: BTreeSet<String>,
        /// The folders read, in order.
        pub reads: Vec<String>,
        /// The copies and imports made, in order, also the ones that failed.
        pub calls: Vec<FsRequest>,
    }

    impl MemoryFs {
        /// The folder `root` holding `files`: paths with their content, `None` for a folder.
        pub fn new(files: &[(&str, Option<&str>)], root: &str) -> Self {
            let mut fs = Self::default();
            fs.nodes.insert(root.to_owned(), MemNode::Dir);
            for (path, content) in files {
                match content {
                    None => fs.mkdirp(path),
                    Some(content) => {
                        fs.mkdirp(parent_of(path));
                        fs.nodes.insert((*path).to_owned(), MemNode::File { size: content.len() as u64 });
                    }
                }
            }
            fs
        }

        fn mkdirp(&mut self, path: &str) {
            let mut at = path;
            while !at.is_empty() && !self.nodes.contains_key(at) {
                self.nodes.insert(at.to_owned(), MemNode::Dir);
                if at == "/" {
                    break;
                }
                at = parent_of(at);
            }
        }

        pub fn has(&self, path: &str) -> bool {
            self.nodes.contains_key(path)
        }

        pub fn is_dir(&self, path: &str) -> bool {
            self.nodes.get(path) == Some(&MemNode::Dir)
        }

        /// The paths at or below `path`.
        fn tree_of(&self, path: &str) -> Vec<String> {
            self.nodes.keys().filter(|at| at.as_str() == path || is_inside(at, path)).cloned().collect()
        }

        pub fn read_dir(&mut self, path: &str) -> IdeResult<Vec<FileEntry>> {
            self.reads.push(path.to_owned());
            match self.nodes.get(path) {
                None => return Err(IdeError::new(ErrorKind::NotFound, format!("{path} not found"))),
                Some(MemNode::File { .. }) => {
                    return Err(IdeError::new(ErrorKind::InvalidInput, format!("{path} is not a directory")));
                }
                Some(MemNode::Dir) => {}
            }
            let mut entries: Vec<FileEntry> = self
                .nodes
                .iter()
                .filter(|(at, _)| at.as_str() != path && parent_of(at) == path)
                .map(|(at, node)| {
                    let name = name_of(at);
                    let excluded = self.ignored.iter().any(|folder| at == folder || is_inside(at, folder));
                    let (kind, size) = match node {
                        MemNode::File { size } => (EntryKind::File, *size),
                        MemNode::Dir => (EntryKind::Dir, 0),
                    };
                    FileEntry {
                        name: name.to_owned(),
                        path: at.clone().into(),
                        kind,
                        size,
                        mtime_ms: 0.0,
                        ignored: excluded,
                        excluded,
                        hidden: name.starts_with('.'),
                        target_kind: None,
                    }
                })
                .collect();
            entries.sort_by(|a, b| {
                let folders = (b.kind == EntryKind::Dir).cmp(&(a.kind == EntryKind::Dir));
                folders.then_with(|| collate::compare_natural(&a.name, &b.name))
            });
            Ok(entries)
        }

        pub fn create_file(&mut self, path: &str) -> IdeResult<()> {
            if self.has(path) {
                return Err(IdeError::new(ErrorKind::AlreadyExists, format!("{path} already exists")));
            }
            self.mkdirp(parent_of(path));
            self.nodes.insert(path.to_owned(), MemNode::File { size: 0 });
            Ok(())
        }

        pub fn create_dir(&mut self, path: &str) -> IdeResult<()> {
            if self.has(path) {
                return Err(IdeError::new(ErrorKind::AlreadyExists, format!("{path} already exists")));
            }
            self.mkdirp(path);
            Ok(())
        }

        pub fn rename(&mut self, from: &str, to: &str) -> IdeResult<()> {
            if !self.has(from) {
                return Err(IdeError::new(ErrorKind::NotFound, format!("{from} not found")));
            }
            if self.has(to) && from.to_lowercase() != to.to_lowercase() {
                return Err(IdeError::new(ErrorKind::AlreadyExists, format!("{to} already exists")));
            }
            if is_inside(to, from) {
                return Err(IdeError::new(ErrorKind::InvalidInput, "cannot move a folder into itself"));
            }
            for at in self.tree_of(from) {
                if let Some(node) = self.nodes.remove(&at) {
                    self.nodes.insert(format!("{to}{}", &at[from.len()..]), node);
                }
            }
            Ok(())
        }

        pub fn delete(&mut self, path: &str) -> IdeResult<()> {
            if !self.has(path) {
                return Err(IdeError::new(ErrorKind::NotFound, format!("{path} not found")));
            }
            for at in self.tree_of(path) {
                self.nodes.remove(&at);
            }
            Ok(())
        }

        pub fn copy(&mut self, from: &str, to: &str) -> IdeResult<()> {
            if !self.has(from) {
                return Err(IdeError::new(ErrorKind::NotFound, format!("{from} not found")));
            }
            if self.has(to) {
                return Err(IdeError::new(ErrorKind::AlreadyExists, format!("{to} already exists")));
            }
            for at in self.tree_of(from) {
                if let Some(node) = self.nodes.get(&at).copied() {
                    self.nodes.insert(format!("{to}{}", &at[from.len()..]), node);
                }
            }
            Ok(())
        }

        /// Carries out `request`. An import answers with the entries it skipped: none here.
        pub fn apply(&mut self, request: &FsRequest) -> IdeResult<Vec<String>> {
            match request {
                FsRequest::ReadDir(path) => self.read_dir(path).map(|_| Vec::new()),
                FsRequest::CreateFile(path) => self.create_file(path).map(|()| Vec::new()),
                FsRequest::CreateDir(path) => self.create_dir(path).map(|()| Vec::new()),
                FsRequest::Rename { from, to } => self.rename(from, to).map(|()| Vec::new()),
                FsRequest::Delete(path) => self.delete(path).map(|()| Vec::new()),
                FsRequest::Copy { from, to } | FsRequest::ImportLocal { from, to } => {
                    self.calls.push(request.clone());
                    self.copy(from, to).map(|()| Vec::new())
                }
            }
        }
    }

    /// Reads the folder `path` into `state` unless it is loaded (always with `force`), and the
    /// single folders chained below it.
    pub fn load(state: &mut ProjectViewState, fs: &mut MemoryFs, path: &str, force: bool) {
        let mut next = Some((path.to_owned(), force));
        while let Some((path, force)) = next.take() {
            if !state.begin_load(&path, force) {
                break;
            }
            let generation = state.generation();
            match fs.read_dir(&path) {
                Ok(entries) => next = state.dir_listed(generation, &path, entries).map(|below| (below, false)),
                Err(error) => state.dir_failed(generation, &path, &error),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workbench::tasks::Tasks;
    use ide_core::error::ErrorKind;

    fn import(from: &str, to: &str) -> FsRequest {
        FsRequest::ImportLocal { from: from.to_owned(), to: to.to_owned() }
    }

    // src/features/project-tree/fs.test.ts › client file routing ›
    // uploads OS files locally and cancels the client request rather than a remote one
    #[test]
    fn uploads_os_files_locally_and_cancels_the_client_request_rather_than_a_remote_one() {
        let mut tasks = Tasks::default();
        let (mut batch, shown) = LocalImport::begin(RequestId(1), &I18n::default());
        tasks.upsert_task(shown, 0.0);
        let (call, shown) = batch.upload("/client/a", "/remote/a", RequestId(2)).expect("the upload starts");
        tasks.upsert_task(shown, 0.0);
        let (from, to) = ("/client/a".to_owned(), "/remote/a".to_owned());
        assert_eq!(call, BackendCall::RemoteImportLocal { from, to, request: RequestId(2) });
        assert_eq!(tasks.tasks().len(), 1);
        assert_eq!(tasks.tasks()[0].message.as_deref(), Some("a"));
        assert_eq!(tasks.tasks()[0].cancel, Some(Cancel::Producer));
        // Cancel stops the upload on the client, where it runs.
        assert_eq!(batch.cancel(), RequestId(2));
        tasks.upsert_task(batch.finish(), 0.0);
        assert!(tasks.task(&batch.task_id()).is_some_and(|task| task.done));
    }

    // src/features/project-tree/fs.test.ts › client file routing ›
    // surfaces a disconnected upload and finishes its task
    #[test]
    fn surfaces_a_disconnected_upload_and_finishes_its_task() {
        let (mut batch, _) = LocalImport::begin(RequestId(1), &I18n::default());
        assert!(batch.upload("/client/a", "/remote/a", RequestId(2)).is_ok());
        // The failure is the caller's to report; the task ends whatever became of the upload.
        let failure = IdeError::unavailable("Remote project is disconnected");
        assert_eq!(failure.message, "Remote project is disconnected");
        assert!(batch.finish().done && !batch.is_cancelled());
    }

    // src/features/project-tree/fs.test.ts › client file routing ›
    // shares one task across paths and prevents starting another after Cancel
    #[test]
    fn shares_one_task_across_paths_and_prevents_starting_another_after_cancel() {
        let mut tasks = Tasks::default();
        let (mut batch, shown) = LocalImport::begin(RequestId(1), &I18n::default());
        tasks.upsert_task(shown, 0.0);
        for (from, to, request) in [("/client/a", "/remote/a", 2), ("/client/b", "/remote/b", 3)] {
            let (_, shown) = batch.upload(from, to, RequestId(request)).expect("the upload starts");
            tasks.upsert_task(shown, 0.0);
        }
        assert_eq!(tasks.tasks().len(), 1);
        assert!(!tasks.tasks()[0].done);
        assert_eq!(batch.cancel(), RequestId(3));
        let refused = batch.upload("/client/c", "/remote/c", RequestId(4)).expect_err("a cancelled batch is over");
        assert_eq!(refused.kind, ErrorKind::Cancelled);
        assert!(batch.finish().done);
    }

    // src/features/project-tree/fs.test.ts › client file routing › uses project copy for a local destination
    #[test]
    fn uses_project_copy_for_a_local_destination() {
        let call = backend_call(&import("/client/a", "/project/a"));
        assert_eq!(call, BackendCall::FsCopy { from: "/client/a".to_owned(), to: "/project/a".to_owned() });
        let announced = file_operation(&import("/client/a", "/project/a")).expect("a copy creates a file");
        assert_eq!((announced.kind, announced.paths), (FileOperationKind::Created, vec!["/project/a".to_owned()]));
    }

    // Not from a spec: the in-memory file system lists folders first in natural order and
    // refuses to overwrite.
    #[test]
    fn the_memory_file_system_lists_folders_first_and_refuses_to_overwrite() {
        let files = [("/p/a10.txt", Some("x")), ("/p/a2.txt", Some("x")), ("/p/z", None)];
        let mut fs = testing::MemoryFs::new(&files, "/p");
        let names: Vec<String> = fs.read_dir("/p").expect("lists").into_iter().map(|entry| entry.name).collect();
        assert_eq!(names, ["z", "a2.txt", "a10.txt"]);
        assert_eq!(fs.copy("/p/a2.txt", "/p/a10.txt").map_err(|error| error.kind), Err(ErrorKind::AlreadyExists));
        assert!(fs.rename("/p/z", "/p/y").is_ok() && fs.is_dir("/p/y") && !fs.has("/p/z"));
        assert_eq!(fs.read_dir("/p/gone").map_err(|error| error.kind).err(), Some(ErrorKind::NotFound));
        assert_eq!(file_operation(&FsRequest::CreateDir("/p/d".to_owned())), None);
    }
}
