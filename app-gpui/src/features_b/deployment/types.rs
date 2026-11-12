//! The deployment backend's IPC shapes (`ide-remote`: `deploy.rs`, `mapping.rs`, `service.rs`),
//! field for field and with the same serde names, so answers deserialize unchanged.

use ide_core::IpcPath;
use serde::{Deserialize, Serialize};

/// Which of the given paths a server's mappings deploy (`deploy_coverage`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentCoverage {
    pub server: String,
    pub paths: Vec<String>,
    pub skipped: Vec<TransferError>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferReport {
    pub files: u32,
    /// Files left alone: excluded, or outside every mapping.
    pub skipped: u32,
    pub errors: Vec<TransferError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferError {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStamp {
    pub size: u64,
    pub mtime: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncStatus {
    LocalOnly,
    RemoteOnly,
    LocalNewer,
    RemoteNewer,
    /// Same time, different size.
    Different,
    Same,
}

/// One file of Sync with Deployed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncEntry {
    /// Project-relative, `/`-separated.
    pub path: String,
    pub remote_path: String,
    pub local: Option<FileStamp>,
    pub remote: Option<FileStamp>,
    pub status: SyncStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncAction {
    Upload,
    Download,
    DeleteRemote,
    DeleteLocal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStep {
    /// Project-relative, `/`-separated.
    pub path: String,
    pub action: SyncAction,
}

/// A mapping resolved by the backend for the Remote Host browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedMapping {
    pub local: IpcPath,
    pub deployment: String,
}

/// Resolved root and project mappings of the Remote Host browser (`deploy_server_root`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentRoot {
    pub root: String,
    pub mappings: Vec<ResolvedMapping>,
}
