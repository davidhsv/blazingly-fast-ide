//! The views of deployment: the Remote Host tool window, the Sync with Deployed dialog with its
//! content preview, and the SSH Password dialog. They read the models of
//! [`crate::features_b::deployment`] and tell the workbench through events what to carry out.

pub mod remote_host_view;
pub mod secret_dialog;
pub mod sync_dialog;
pub mod sync_preview;
