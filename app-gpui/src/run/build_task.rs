//! Dispatches an awaitable build and consumes only its fresh completion status.
//!
//! The build feature reports through two context keys: [`BUILD_RUNNING`] while a build runs and
//! [`BUILD_LAST_STATUS`] once one finished. A caller checks [`begin_build`], executes the action of
//! the [`BuildRequest`] and waits for it, then reads the outcome with [`build_outcome`].

use crate::platform::context::{ContextKeys, ContextValue};
use serde_json::Value;

/// Context key that is truthy while a build runs.
pub const BUILD_RUNNING: &str = "build.running";
/// Context key holding how the last build ended: `success`, `failed` or `cancelled`.
pub const BUILD_LAST_STATUS: &str = "build.lastStatus";

/// A build action to execute and wait for.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildRequest {
    pub action: String,
    pub arg: Value,
}

/// Whether the build may be dispatched now: the error to report when it may not. On success the last
/// status is cleared, so that the status read afterwards is this build's.
pub fn begin_build(action_registered: bool, context: &mut ContextKeys) -> Result<(), &'static str> {
    if !action_registered {
        return Err("Building is not available");
    }
    if context.is_truthy(BUILD_RUNNING) {
        return Err("A build is already running");
    }
    context.clear(BUILD_LAST_STATUS);
    Ok(())
}

/// The error of a build that was dispatched after [`begin_build`]; `None` when it succeeded.
/// `executed`: whether the action ran (it does not while its own conditions refuse it).
pub fn build_outcome(executed: bool, context: &ContextKeys) -> Option<&'static str> {
    if !executed {
        return Some("A build is already running");
    }
    match context.get(BUILD_LAST_STATUS) {
        Some(ContextValue::Text(status)) => match &**status {
            "success" => None,
            "cancelled" => Some("Build cancelled"),
            "failed" => Some("Build failed"),
            _ => Some("Build did not start"),
        },
        _ => Some("Build did not start"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn awaits_fresh_results_from_each_build_action() {
        for _action in ["build.buildProject", "build.buildModule", "build.buildArtifacts"] {
            let cases = [
                (Some("success"), None),
                (Some("failed"), Some("Build failed")),
                (Some("cancelled"), Some("Build cancelled")),
                (None, Some("Build did not start")),
            ];
            for (status, error) in cases {
                let mut context = ContextKeys::new();
                context.set(BUILD_LAST_STATUS, "success");
                assert_eq!(begin_build(true, &mut context), Ok(()));
                // The stale status is gone while the build runs.
                assert_eq!(context.get(BUILD_LAST_STATUS), None);
                if let Some(status) = status {
                    context.set(BUILD_LAST_STATUS, status);
                }
                assert_eq!(build_outcome(true, &context), error);
            }
        }
    }

    #[test]
    fn reports_an_existing_build_without_changing_its_result() {
        let mut context = ContextKeys::new();
        context.set(BUILD_RUNNING, true);
        context.set(BUILD_LAST_STATUS, "failed");
        assert_eq!(begin_build(true, &mut context), Err("A build is already running"));
        assert_eq!(context.get(BUILD_LAST_STATUS), Some(&ContextValue::from("failed")));
    }

    // Not from a spec: a missing action and an action that refused to run are told apart.
    #[test]
    fn a_missing_or_refused_action_is_an_error() {
        let mut context = ContextKeys::new();
        assert_eq!(begin_build(false, &mut context), Err("Building is not available"));
        assert_eq!(build_outcome(false, &context), Some("A build is already running"));
    }
}
