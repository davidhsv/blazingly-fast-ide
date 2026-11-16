//! What the settings pages that connect over SSH (SSH Configurations, Deployment, Run Targets) ask
//! of the backend in common: running a call on the backend's runtime with its answer back on the
//! window's thread, trusting the host key a connection failed on (`ssh_trust_host`) and handing a
//! configuration its password or passphrase (`ssh_set_secret`), each called as their commands
//! call them. A page holds them as values a test can replace with a stand-in.

use super::Then;
use crate::backend::Backend;
use crate::database::store::SecretText;
use gpui_kit::App;
use ide_core::IdeError;
use std::rc::Rc;
use std::sync::Arc;

/// Trusts the host key that failed the last connection of an SSH configuration.
pub type HostTrust = Rc<dyn Fn(String, Then<Result<(), IdeError>>, &mut App)>;
/// Hands an SSH configuration its password or passphrase, kept in the saved passwords with `save`
/// (without it, a password saved before is forgotten, as the backend does).
pub type SecretSetter = Rc<dyn Fn(String, SecretText, bool, Then<Result<(), IdeError>>, &mut App)>;

/// Runs `work` on the backend's runtime and hands its answer to `then` on the window's thread.
pub fn on_backend<T, F, W>(work: W, then: Then<Result<T, IdeError>>, cx: &mut App)
where
    T: Send + 'static,
    F: std::future::Future<Output = Result<T, IdeError>> + Send + 'static,
    W: FnOnce(Arc<ide_host::Host>) -> F + Send + 'static,
{
    let Some(backend) = cx.try_global::<Backend>() else {
        then(Err(IdeError::internal("The backend is not running")), cx);
        return;
    };
    let work = backend.with_host(work);
    cx.spawn(async move |cx| {
        let result = work.await.unwrap_or_else(|error| Err(IdeError::internal(error.to_string())));
        cx.update(|cx| then(result, cx));
    })
    .detach();
}

/// The backend's host key trust (`ssh_trust_host`).
pub fn backend_trust_host() -> HostTrust {
    Rc::new(|id: String, then: Then<Result<(), IdeError>>, cx: &mut App| {
        on_backend(move |host| async move { host.ssh.trust_host(&id).await }, then, cx);
    })
}

/// The backend's password or passphrase of a configuration (`ssh_set_secret`).
pub fn backend_set_secret() -> SecretSetter {
    Rc::new(|id: String, secret: SecretText, save: bool, then: Then<Result<(), IdeError>>, cx: &mut App| {
        on_backend(
            move |host| async move {
                host.ssh.set_secret(&id, Some(secret.into_inner()), save);
                Ok(())
            },
            then,
            cx,
        );
    })
}
