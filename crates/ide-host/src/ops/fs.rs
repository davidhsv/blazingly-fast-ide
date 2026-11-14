use super::Channel;
use crate::Host;
use ide_core::{IdeResult, RequestId};
use ide_vfs::FileEntry;
use std::path::PathBuf;

pub async fn entry_kind(host: &Host, path: PathBuf) -> IdeResult<ide_vfs::EntryKind> {
    host.blocking(move || {
        let meta = std::fs::metadata(path)?;
        Ok(if meta.is_dir() { ide_vfs::EntryKind::Dir } else { ide_vfs::EntryKind::File })
    })
    .await
}

pub async fn compare_directories(
    host: &Host,
    window: &str,
    left: PathBuf,
    right: PathBuf,
    request: RequestId,
    on_progress: Channel<ide_vfs::DirectoryCompareProgress>,
) -> IdeResult<Vec<ide_vfs::DirectoryCompareEntry>> {
    let project = host.project(window)?;
    let cancel = host.begin_request(request);
    let result = host
        .blocking(move || {
            let last = std::cell::Cell::new(std::time::Instant::now() - std::time::Duration::from_secs(1));
            ide_vfs::compare_directories(&left, &right, Some(&project.root), &project.exclude_globs(), &cancel, |p| {
                // Bound IPC traffic even when hashing multi-gigabyte files.
                if last.get().elapsed() >= std::time::Duration::from_millis(80) || p.total == Some(p.compared) {
                    let _ = on_progress.send(p);
                    last.set(std::time::Instant::now());
                }
            })
        })
        .await;
    host.end_request(request);
    result
}

pub async fn copy_merge(
    host: &Host,
    window: &str,
    source_root: PathBuf,
    destination_root: PathBuf,
    path: PathBuf,
    overwrite: bool,
) -> IdeResult<()> {
    if path.as_os_str().is_empty() || path.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return Err(ide_core::IdeError::invalid("expected a relative comparison entry path"));
    }
    let project = host.project(window)?;
    host.blocking(move || {
        let from = source_root.join(&path);
        let to = destination_root.join(&path);
        if overwrite {
            project.history.record_deletion(&to, DELETE_SNAPSHOT_LIMIT);
        }
        project.with_disk_sync(|| {
            ide_vfs::copy_merge(
                &from,
                &to,
                overwrite,
                Some(&project.root),
                &project.exclude_globs(),
                (&source_root, &destination_root),
            )
        })
    })
    .await
}

/// Files of a deleted folder snapshotted in Local History.
const DELETE_SNAPSHOT_LIMIT: usize = 2000;

/// Runs on the blocking pool: on a network share (`\\wsl.localhost`) every stat is slow.
pub async fn read_dir(host: &Host, window: &str, path: PathBuf) -> IdeResult<Vec<FileEntry>> {
    let project = host.project(window);
    host.blocking(move || {
        let excludes = project
            .map(|p| {
                let globs = p.exclude_globs();
                // `read_dir` matches relative to the enclosing repository: re-anchor
                // project-root patterns (`/build`) when the project is a repo subfolder.
                let repo = p.root.ancestors().find(|d| d.join(".git").exists());
                match repo.and_then(|r| p.root.strip_prefix(r).ok()).filter(|rel| !rel.as_os_str().is_empty()) {
                    Some(rel) => {
                        let rel = rel.to_string_lossy().replace('\\', "/");
                        globs.into_iter().map(|g| if g.starts_with('/') { format!("/{rel}{g}") } else { g }).collect()
                    }
                    None => globs,
                }
            })
            .unwrap_or_default();
        ide_vfs::read_dir(&path, &excludes)
    })
    .await
}

/// Creates an empty file; inside a project, honours the EditorConfig
/// `charset` (writes the BOM for `utf-8-bom` / UTF-16).
pub fn create_file(host: &Host, window: &str, path: PathBuf) -> IdeResult<()> {
    match host.project(window) {
        Ok(p) => {
            let defaults = p.documents.file_defaults();
            if defaults.editorconfig {
                ide_workspace::editorconfig::create_file(p.documents.editorconfig(), &path)?;
            } else {
                ide_vfs::create_file(&path)?;
            }
            // No EditorConfig charset: File Encodings › "Create UTF-8 files with BOM".
            let empty = std::fs::metadata(&path).map(|m| m.len() == 0).unwrap_or(false);
            let bom = defaults.new_file_bytes();
            if empty && !bom.is_empty() {
                std::fs::write(&path, bom)?;
            }
            Ok(())
        }
        Err(_) => ide_vfs::create_file(&path),
    }
}

pub async fn create_file_bytes(host: &Host, path: PathBuf, data: Vec<u8>) -> IdeResult<()> {
    host.blocking(move || ide_vfs::create_file_bytes(&path, &data)).await
}

/// Toggle Read-only Attribute: sets the writable bit (and the open document's
/// state). Returns whether the file is read-only now.
pub fn set_writable(host: &Host, window: &str, path: PathBuf, writable: bool) -> IdeResult<bool> {
    match host.project(window) {
        Ok(p) => p.documents.set_writable(&path, writable),
        Err(_) => ide_vfs::set_writable(&path, writable).map(|()| !writable),
    }
}

/// Moves to the trash (or where [`Host::set_trash`] says); Local History keeps the contents so
/// deleted files can be restored.
pub async fn delete(host: &Host, window: &str, path: PathBuf) -> IdeResult<()> {
    let history = host.project(window).ok().map(|p| p.history.clone());
    let trash = host.trash_override();
    host.blocking(move || {
        let paths = ide_workspace::file_ops::deletion_paths(&path)?;
        if let Some(h) = history {
            for p in &paths {
                h.record_deletion(p, DELETE_SNAPSHOT_LIMIT);
            }
        }
        match trash {
            // Every path the project's rules delete together (an asset and its `.meta`).
            Some(trash) => paths.iter().try_for_each(|p| trash(p.as_path())),
            None => ide_workspace::file_ops::delete(&path),
        }
    })
    .await
}

pub async fn copy(host: &Host, from: PathBuf, to: PathBuf) -> IdeResult<()> {
    host.blocking(move || ide_workspace::file_ops::copy(&from, &to)).await
}

pub async fn rename(host: &Host, from: PathBuf, to: PathBuf) -> IdeResult<()> {
    host.blocking(move || ide_workspace::file_ops::rename(&from, &to)).await
}

/// Reads a text file without opening it as a document; prefers the unsaved buffer.
pub async fn read_text(host: &Host, window: &str, path: PathBuf) -> IdeResult<String> {
    let project = host.project(window).ok();
    host.blocking(move || match project {
        Some(p) => p.text_of(&path),
        None => Ok(ide_vfs::load_text(&path)?.text),
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HostConfig;
    use ide_core::{IdeError, NullSink, error::ErrorKind};
    use std::path::Path;
    use std::sync::Arc;

    /// A delete goes where `set_trash` says instead of the system Trash: a file and a folder leave
    /// their place whole; a path that is not there is refused.
    #[tokio::test]
    async fn a_delete_goes_where_set_trash_says_instead_of_the_system_trash() {
        let root = tempfile::tempdir().unwrap();
        let host = Host::new(HostConfig {
            config_dir: root.path().join("config"),
            data_dir: root.path().join("data"),
            local_data_dir: root.path().join("data"),
            languages: Arc::new(ide_language::LanguageRegistry::load(None).unwrap()),
            syntax: Arc::default(),
            sink_for: Box::new(|_| Arc::new(NullSink)),
            owner: |window| window,
            runtime: tokio::runtime::Handle::current(),
            project_environment: false,
        });
        let trash = root.path().join("trash");
        std::fs::create_dir_all(&trash).unwrap();
        let into = trash.clone();
        host.set_trash(Some(Arc::new(move |path: &Path| {
            let name = path.file_name().ok_or_else(|| IdeError::invalid("a path without a name"))?;
            std::fs::rename(path, into.join(name)).map_err(IdeError::from)
        })));
        let dir = root.path().join("project/src");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        delete(&host, "w", dir.join("a.txt")).await.unwrap();
        assert!(!dir.join("a.txt").exists());
        assert_eq!(std::fs::read_to_string(trash.join("a.txt")).unwrap(), "a\n");
        delete(&host, "w", dir.clone()).await.unwrap();
        assert!(!dir.exists() && trash.join("src").is_dir());
        let error = delete(&host, "w", dir.join("gone.txt")).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::NotFound);
    }
}
