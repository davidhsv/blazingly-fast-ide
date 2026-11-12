//! Read-only remote snapshots for the deployment comparison: the text preview of a row and the
//! exact byte comparison of Compare by Binary Content. Project files are never downloaded over:
//! the server's copy is opened into a local snapshot (`remote_open`) once per remote path, and the
//! diff receives it as immutable text, so its merge controls cannot write to the server.
//!
//! An operation ([`ComparisonOp`]) says what to read next ([`ComparisonStep`]) and takes the
//! answer; an aborted operation (another row selected, the scan stopped) answers every later read
//! with [`ComparisonStep::Aborted`].

use super::types::SyncEntry;
use crate::platform::i18n::{I18n, Param};
use crate::platform_more::remote_settings::deployment_connection;
use ide_core::IdeError;
use ide_core::settings::DeploymentServer;
use ide_ssh::FsConnection;
use ide_vfs::{ByteRange, FileProbe};
use std::collections::HashMap;

/// Largest file shown as text.
pub const PREVIEW_LIMIT: u64 = 8 * 1024 * 1024;
/// Bytes read per file and step of a content comparison.
pub const BLOCK_SIZE: u64 = 64 * 1024;

/// One side of a diff: the live local file (editable), or fixed text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffSide {
    File { path: String, label: String },
    Text { text: String, label: String },
}

/// The data of a `diff` editor input: the local file next to the server's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentDiff {
    pub path: String,
    pub title: String,
    pub left: DiffSide,
    /// Always [`DiffSide::Text`]: the server's copy is never editable.
    pub right: DiffSide,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeploymentPreview {
    Text(DeploymentDiff),
    Binary,
    TooLarge,
}

/// What an operation needs next, or how it ended.
#[derive(Debug, Clone, PartialEq)]
pub enum ComparisonStep {
    /// Open the server's file into a local snapshot (`remote_open(connection, remote_path)`), then
    /// call [`ComparisonOp::downloaded`].
    Download {
        connection: FsConnection,
        remote_path: String,
    },
    /// Another operation is downloading this snapshot: call [`ComparisonOp::resume`] once it
    /// finished.
    AwaitSnapshot {
        remote_path: String,
    },
    /// Probe these files (`fs_probe`), then call [`ComparisonOp::probed`] with the answers in order.
    Probe {
        paths: Vec<String>,
    },
    /// Read the snapshot's text (`fs_read_text`), then call [`ComparisonOp::text_read`].
    ReadText {
        path: String,
    },
    /// Read `len` bytes at `offset` of both files (`fs_read_range`), then call
    /// [`ComparisonOp::ranges_read`].
    ReadRanges {
        local: String,
        remote: String,
        offset: u64,
        len: u64,
    },
    Preview(DeploymentPreview),
    Equal(bool),
    Failed(String),
    Aborted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Snapshot {
    Pending,
    Ready(String),
}

/// The comparison of one server and project: the snapshots downloaded so far, by remote path.
#[derive(Debug, Clone)]
pub struct DeploymentComparison {
    connection: FsConnection,
    server_name: String,
    root: String,
    snapshots: HashMap<String, Snapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpKind {
    Preview,
    Equal,
}

/// A preview or content comparison of one row in progress.
#[derive(Debug, Clone)]
pub struct ComparisonOp {
    kind: OpKind,
    entry: SyncEntry,
    local_path: String,
    server_name: String,
    snapshot: Option<String>,
    offset: u64,
    aborted: bool,
}

impl DeploymentComparison {
    pub fn new(server: &DeploymentServer, root: &str) -> Self {
        let server_name = if server.name.is_empty() { server.id.clone() } else { server.name.clone() };
        Self {
            connection: deployment_connection(server),
            server_name,
            root: root.strip_suffix('/').unwrap_or(root).to_owned(),
            snapshots: HashMap::new(),
        }
    }

    fn op(&self, kind: OpKind, entry: SyncEntry) -> ComparisonOp {
        ComparisonOp {
            kind,
            local_path: format!("{}/{}", self.root, entry.path),
            entry,
            server_name: self.server_name.clone(),
            snapshot: None,
            offset: 0,
            aborted: false,
        }
    }

    /// Starts the text preview of a row.
    pub fn preview(&mut self, entry: SyncEntry, i18n: &mut I18n) -> (ComparisonOp, ComparisonStep) {
        let mut op = self.op(OpKind::Preview, entry);
        let local = op.entry.local.map_or(0, |stamp| stamp.size);
        let remote = op.entry.remote.map_or(0, |stamp| stamp.size);
        let step = if local.max(remote) > PREVIEW_LIMIT {
            ComparisonStep::Preview(DeploymentPreview::TooLarge)
        } else if op.entry.remote.is_some() {
            op.request_snapshot(self, i18n)
        } else {
            op.after_snapshot(i18n)
        };
        (op, step)
    }

    /// Starts the exact byte comparison of a row; rows missing a side or of different sizes differ
    /// without reading anything.
    pub fn equal(&mut self, entry: SyncEntry, i18n: &mut I18n) -> (ComparisonOp, ComparisonStep) {
        let mut op = self.op(OpKind::Equal, entry);
        let step = match (op.entry.local, op.entry.remote) {
            (Some(local), Some(remote)) if local.size == remote.size => op.request_snapshot(self, i18n),
            _ => ComparisonStep::Equal(false),
        };
        (op, step)
    }
}

impl ComparisonOp {
    /// Stops the operation: every later answer ends it with [`ComparisonStep::Aborted`].
    pub fn abort(&mut self) {
        self.aborted = true;
    }

    fn request_snapshot(&mut self, cmp: &mut DeploymentComparison, i18n: &mut I18n) -> ComparisonStep {
        let remote_path = self.entry.remote_path.clone();
        match cmp.snapshots.get(&remote_path) {
            Some(Snapshot::Ready(path)) => {
                self.snapshot = Some(path.clone());
                self.after_snapshot(i18n)
            }
            Some(Snapshot::Pending) => ComparisonStep::AwaitSnapshot { remote_path },
            None => {
                cmp.snapshots.insert(remote_path.clone(), Snapshot::Pending);
                ComparisonStep::Download { connection: cmp.connection.clone(), remote_path }
            }
        }
    }

    fn after_snapshot(&mut self, i18n: &mut I18n) -> ComparisonStep {
        match self.kind {
            OpKind::Preview => {
                let local = self.entry.local.is_some().then(|| self.local_path.clone());
                let paths: Vec<String> = local.into_iter().chain(self.snapshot.clone()).collect();
                if paths.is_empty() {
                    self.finish_preview(String::new(), i18n)
                } else {
                    ComparisonStep::Probe { paths }
                }
            }
            OpKind::Equal => match &self.snapshot {
                Some(remote) => ComparisonStep::ReadRanges {
                    local: self.local_path.clone(),
                    remote: remote.clone(),
                    offset: self.offset,
                    len: BLOCK_SIZE,
                },
                None => ComparisonStep::Equal(false),
            },
        }
    }

    /// The answer to [`ComparisonStep::Download`]: a snapshot is kept for every later operation, a
    /// failed download is forgotten so that the next operation tries again.
    pub fn downloaded(
        &mut self,
        cmp: &mut DeploymentComparison,
        result: Result<String, IdeError>,
        i18n: &mut I18n,
    ) -> ComparisonStep {
        let remote_path = self.entry.remote_path.clone();
        match result {
            Ok(path) => {
                cmp.snapshots.insert(remote_path, Snapshot::Ready(path.clone()));
                if self.aborted {
                    return ComparisonStep::Aborted;
                }
                self.snapshot = Some(path);
                self.after_snapshot(i18n)
            }
            Err(error) => {
                cmp.snapshots.remove(&remote_path);
                self.failed(&error, i18n)
            }
        }
    }

    /// Continues after [`ComparisonStep::AwaitSnapshot`]: uses the snapshot, or downloads it when
    /// the other download failed.
    pub fn resume(&mut self, cmp: &mut DeploymentComparison, i18n: &mut I18n) -> ComparisonStep {
        if self.aborted { ComparisonStep::Aborted } else { self.request_snapshot(cmp, i18n) }
    }

    /// The answer to [`ComparisonStep::Probe`]: binary and oversized files get no text preview.
    pub fn probed(&mut self, probes: &[FileProbe], i18n: &mut I18n) -> ComparisonStep {
        if self.aborted {
            return ComparisonStep::Aborted;
        }
        if probes.iter().any(|probe| probe.binary) {
            return ComparisonStep::Preview(DeploymentPreview::Binary);
        }
        if probes.iter().any(|probe| probe.too_large || probe.size > PREVIEW_LIMIT) {
            return ComparisonStep::Preview(DeploymentPreview::TooLarge);
        }
        match &self.snapshot {
            Some(path) => ComparisonStep::ReadText { path: path.clone() },
            None => self.finish_preview(String::new(), i18n),
        }
    }

    /// The answer to [`ComparisonStep::ReadText`].
    pub fn text_read(&mut self, text: String, i18n: &mut I18n) -> ComparisonStep {
        if self.aborted { ComparisonStep::Aborted } else { self.finish_preview(text, i18n) }
    }

    fn finish_preview(&self, text: String, i18n: &mut I18n) -> ComparisonStep {
        let entry = &self.entry;
        let left = if entry.local.is_some() {
            let label = i18n.tx_built(&["Local: ", ""], &[Param::from(entry.path.as_str())]);
            DiffSide::File { path: self.local_path.clone(), label }
        } else {
            DiffSide::Text { text: String::new(), label: i18n.tx("Local: (does not exist)") }
        };
        let remote = if entry.remote.is_some() { entry.remote_path.clone() } else { i18n.tx("(does not exist)") };
        let right = DiffSide::Text { text, label: format!("{}: {remote}", self.server_name) };
        ComparisonStep::Preview(DeploymentPreview::Text(DeploymentDiff {
            path: self.local_path.clone(),
            title: entry.path.clone(),
            left,
            right,
        }))
    }

    /// The answer to [`ComparisonStep::ReadRanges`]: stops at the first block that differs, and
    /// fails when either file no longer has the size the listing showed. Empty files are read
    /// once too, to notice a file that changed since the listing.
    pub fn ranges_read(&mut self, local: &ByteRange, remote: &ByteRange, i18n: &I18n) -> ComparisonStep {
        if self.aborted {
            return ComparisonStep::Aborted;
        }
        let (Some(local_stamp), Some(remote_stamp)) = (self.entry.local, self.entry.remote) else {
            return ComparisonStep::Equal(false);
        };
        if local.total != local_stamp.size || remote.total != remote_stamp.size {
            return ComparisonStep::Failed(i18n.tx("A file changed during comparison. Refresh and compare again."));
        }
        if local.data != remote.data {
            return ComparisonStep::Equal(false);
        }
        self.offset += BLOCK_SIZE;
        if local_stamp.size == 0 || self.offset >= local_stamp.size {
            return ComparisonStep::Equal(true);
        }
        match &self.snapshot {
            Some(snapshot) => ComparisonStep::ReadRanges {
                local: self.local_path.clone(),
                remote: snapshot.clone(),
                offset: self.offset,
                len: BLOCK_SIZE,
            },
            None => ComparisonStep::Equal(false),
        }
    }

    /// A read of this operation failed.
    pub fn failed(&self, error: &IdeError, i18n: &I18n) -> ComparisonStep {
        if self.aborted { ComparisonStep::Aborted } else { ComparisonStep::Failed(i18n.tx_template(&error.message)) }
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{FileStamp, SyncStatus};
    use super::*;
    use ide_core::error::ErrorKind;

    fn server() -> DeploymentServer {
        DeploymentServer {
            id: "s1".to_owned(),
            name: "Staging".to_owned(),
            ssh_config: "ssh".to_owned(),
            ..DeploymentServer::default()
        }
    }

    fn stamp(size: u64, mtime: u32) -> Option<FileStamp> {
        Some(FileStamp { size, mtime: Some(mtime) })
    }

    fn entry() -> SyncEntry {
        SyncEntry {
            path: "src/a.php".to_owned(),
            remote_path: "/srv/app/src/a.php".to_owned(),
            status: SyncStatus::Same,
            local: stamp(4, 1),
            remote: stamp(4, 1),
        }
    }

    fn probe(binary: bool) -> FileProbe {
        FileProbe { size: 4, binary, too_large: false }
    }

    fn range(total: u64, data: &str) -> ByteRange {
        ByteRange { offset: 0, data: data.to_owned(), total }
    }

    fn download(path: &str) -> ComparisonStep {
        ComparisonStep::Download {
            connection: FsConnection::Sftp { ssh: "ssh".to_owned() },
            remote_path: path.to_owned(),
        }
    }

    fn text_of(step: &ComparisonStep) -> &DeploymentDiff {
        match step {
            ComparisonStep::Preview(DeploymentPreview::Text(diff)) => diff,
            other => panic!("not a text preview: {other:?}"),
        }
    }

    // deployment content snapshots › keeps the local document live and the remote side immutable,
    // sharing one downloaded snapshot
    #[test]
    fn keeps_the_local_document_live_and_the_remote_side_immutable_sharing_one_downloaded_snapshot() {
        let mut i18n = I18n::default();
        let mut compare = DeploymentComparison::new(&server(), "/work/app/");
        let (mut op, step) = compare.preview(entry(), &mut i18n);
        assert_eq!(step, download("/srv/app/src/a.php"));
        let step = op.downloaded(&mut compare, Ok("/cache/a.php".to_owned()), &mut i18n);
        assert_eq!(step, ComparisonStep::Probe { paths: vec!["/work/app/src/a.php".into(), "/cache/a.php".into()] });
        let step = op.probed(&[probe(false), probe(false)], &mut i18n);
        assert_eq!(step, ComparisonStep::ReadText { path: "/cache/a.php".to_owned() });
        let step = op.text_read("old\n".to_owned(), &mut i18n);
        let diff = text_of(&step);
        assert_eq!(diff.path, "/work/app/src/a.php");
        assert!(matches!(&diff.left, DiffSide::File { path, .. } if path == "/work/app/src/a.php"));
        let right = DiffSide::Text { text: "old\n".to_owned(), label: "Staging: /srv/app/src/a.php".to_owned() };
        assert_eq!(diff.right, right);
        let (_, step) = compare.equal(entry(), &mut i18n);
        assert!(matches!(step, ComparisonStep::ReadRanges { ref remote, .. } if remote == "/cache/a.php"));
    }

    // deployment content snapshots › represents missing sides without reading missing files
    #[test]
    fn represents_missing_sides_without_reading_missing_files() {
        let mut i18n = I18n::default();
        let mut compare = DeploymentComparison::new(&server(), "/work/app");
        let (mut op, step) = compare.preview(SyncEntry { remote: None, ..entry() }, &mut i18n);
        assert_eq!(step, ComparisonStep::Probe { paths: vec!["/work/app/src/a.php".to_owned()] });
        let diff_step = op.probed(&[probe(false)], &mut i18n);
        let diff = text_of(&diff_step);
        let missing =
            matches!(&diff.right, DiffSide::Text { text, label } if text.is_empty() && label.contains("not exist"));
        assert!(missing);

        let (mut op, step) = compare.preview(SyncEntry { local: None, ..entry() }, &mut i18n);
        assert_eq!(step, download("/srv/app/src/a.php"));
        let step = op.downloaded(&mut compare, Ok("/cache/a.php".to_owned()), &mut i18n);
        assert_eq!(step, ComparisonStep::Probe { paths: vec!["/cache/a.php".to_owned()] });
        let step = op.probed(&[probe(false)], &mut i18n);
        let step = match step {
            ComparisonStep::ReadText { .. } => op.text_read("old\n".to_owned(), &mut i18n),
            other => other,
        };
        let diff = text_of(&step);
        assert!(matches!(&diff.left, DiffSide::Text { text, .. } if text.is_empty()));
        assert!(matches!(&diff.right, DiffSide::Text { text, .. } if text == "old\n"));
    }

    // deployment content snapshots › does not decode binaries or download oversized text previews
    #[test]
    fn does_not_decode_binaries_or_download_oversized_text_previews() {
        let mut i18n = I18n::default();
        let mut compare = DeploymentComparison::new(&server(), "/work/app");
        let (_, step) = compare.preview(SyncEntry { local: stamp(20_000_000, 1), ..entry() }, &mut i18n);
        assert_eq!(step, ComparisonStep::Preview(DeploymentPreview::TooLarge));
        let (mut op, step) = compare.preview(entry(), &mut i18n);
        assert_eq!(step, download("/srv/app/src/a.php"));
        op.downloaded(&mut compare, Ok("/cache/a.php".to_owned()), &mut i18n);
        let step = op.probed(&[probe(false), probe(true)], &mut i18n);
        assert_eq!(step, ComparisonStep::Preview(DeploymentPreview::Binary));
    }

    // deployment content snapshots › compares bytes even when size and timestamps match
    #[test]
    fn compares_bytes_even_when_size_and_timestamps_match() {
        let mut i18n = I18n::default();
        let mut compare = DeploymentComparison::new(&server(), "/work/app");
        let (mut op, _) = compare.equal(entry(), &mut i18n);
        op.downloaded(&mut compare, Ok("/cache/a.php".to_owned()), &mut i18n);
        assert_eq!(op.ranges_read(&range(4, "bmV3Cg=="), &range(4, "b2xkCg=="), &i18n), ComparisonStep::Equal(false));
        let (mut op, step) = compare.equal(SyncEntry { remote: stamp(4, 99), ..entry() }, &mut i18n);
        assert!(matches!(step, ComparisonStep::ReadRanges { offset: 0, .. }));
        assert_eq!(op.ranges_read(&range(4, "b2xkCg=="), &range(4, "b2xkCg=="), &i18n), ComparisonStep::Equal(true));
    }

    // deployment content snapshots › uses bounded reads and stops at the first differing block
    #[test]
    fn uses_bounded_reads_and_stops_at_the_first_differing_block() {
        let mut i18n = I18n::default();
        let mut compare = DeploymentComparison::new(&server(), "/work/app");
        let large = SyncEntry { local: stamp(150_000, 1), remote: stamp(150_000, 1), ..entry() };
        let (mut op, step) = compare.equal(large, &mut i18n);
        let mut step = match step {
            ComparisonStep::Download { .. } => op.downloaded(&mut compare, Ok("/cache/a.php".to_owned()), &mut i18n),
            other => other,
        };
        let mut reads = 0;
        while let ComparisonStep::ReadRanges { local, remote, offset, len } = &step {
            assert_eq!(*len, 65_536);
            reads += 2;
            let data = |path: &str| if *offset >= 65_536 { path.to_owned() } else { "same".to_owned() };
            let (a, b) = (range(150_000, &data(local)), range(150_000, &data(remote)));
            step = op.ranges_read(&a, &b, &i18n);
        }
        assert_eq!(step, ComparisonStep::Equal(false));
        assert_eq!(reads, 4);
    }

    // deployment content snapshots › detects changed sizes during comparison and handles empty files
    #[test]
    fn detects_changed_sizes_during_comparison_and_handles_empty_files() {
        let mut i18n = I18n::default();
        let mut compare = DeploymentComparison::new(&server(), "/work/app");
        let (mut op, _) = compare.equal(entry(), &mut i18n);
        op.downloaded(&mut compare, Ok("/cache/a.php".to_owned()), &mut i18n);
        let step = op.ranges_read(&range(5, "changed"), &range(5, "changed"), &i18n);
        assert!(matches!(step, ComparisonStep::Failed(ref message) if message.contains("changed during comparison")));
        let empty = SyncEntry { local: stamp(0, 1), remote: stamp(0, 1), ..entry() };
        let (mut op, step) = compare.equal(empty, &mut i18n);
        assert!(matches!(step, ComparisonStep::ReadRanges { offset: 0, .. }));
        assert_eq!(op.ranges_read(&range(0, ""), &range(0, ""), &i18n), ComparisonStep::Equal(true));
        let mut fresh = DeploymentComparison::new(&server(), "/work/app");
        let (_, step) = fresh.equal(SyncEntry { remote: stamp(8, 1), ..entry() }, &mut i18n);
        assert_eq!(step, ComparisonStep::Equal(false));
    }

    // deployment content snapshots › does not continue after selection is cancelled and retries
    // failed downloads
    #[test]
    fn does_not_continue_after_selection_is_cancelled_and_retries_failed_downloads() {
        let mut i18n = I18n::default();
        let mut compare = DeploymentComparison::new(&server(), "/work/app");
        let (mut op, _) = compare.preview(entry(), &mut i18n);
        op.abort();
        assert_eq!(op.downloaded(&mut compare, Ok("/cache/a.php".to_owned()), &mut i18n), ComparisonStep::Aborted);

        let mut retry = DeploymentComparison::new(&server(), "/work/app");
        let (mut op, _) = retry.preview(entry(), &mut i18n);
        let offline = IdeError::new(ErrorKind::Io, "Offline");
        assert_eq!(op.downloaded(&mut retry, Err(offline), &mut i18n), ComparisonStep::Failed("Offline".to_owned()));
        let (mut op, step) = retry.preview(entry(), &mut i18n);
        assert_eq!(step, download("/srv/app/src/a.php"));
        op.downloaded(&mut retry, Ok("/cache/a.php".to_owned()), &mut i18n);
        op.probed(&[probe(false), probe(false)], &mut i18n);
        let step = op.text_read("old\n".to_owned(), &mut i18n);
        assert!(matches!(step, ComparisonStep::Preview(DeploymentPreview::Text(_))));
    }

    // Not from a spec: a preview and a content check of the same row download one snapshot.
    #[test]
    fn a_second_operation_waits_for_the_snapshot_in_flight() {
        let mut i18n = I18n::default();
        let mut compare = DeploymentComparison::new(&server(), "/work/app");
        let (mut first, _) = compare.preview(entry(), &mut i18n);
        let (mut second, step) = compare.equal(entry(), &mut i18n);
        assert_eq!(step, ComparisonStep::AwaitSnapshot { remote_path: "/srv/app/src/a.php".to_owned() });
        first.downloaded(&mut compare, Ok("/cache/a.php".to_owned()), &mut i18n);
        assert!(matches!(second.resume(&mut compare, &mut i18n), ComparisonStep::ReadRanges { .. }));
    }
}
