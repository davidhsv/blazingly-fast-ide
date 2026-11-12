//! The settings as the UI reads and writes them: the backend's store, one value at a time.
//!
//! A write goes to the store at once and every window is then told which key changed
//! (`IdeEvent::SettingsChanged`), the writer included, so that what depends on a setting is
//! refreshed in one place whoever wrote it.

use crate::backend::{Backend, owner_window};
use gpui_kit::App;
use ide_core::settings::Settings;
use ide_core::{IdeError, IdeEvent, IdeResult};
use ide_workspace::settings::SettingsScope;
use serde_json::Value;

/// A value to write to the global settings.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct SettingsWrite {
    pub key: &'static str,
    pub value: Value,
}

/// The settings in effect for `window`: its project's, or the global ones without a project.
/// The defaults while there is no backend.
pub fn effective(window: &str, cx: &App) -> Settings {
    cx.try_global::<Backend>()
        .map_or_else(Settings::default, |backend| ide_host::ops::project::settings(&backend.host, window))
}

/// The defaults plus the global layer, without any project's values.
pub fn global(cx: &App) -> Settings {
    cx.try_global::<Backend>().map_or_else(Settings::default, |backend| backend.host.global_settings.effective())
}

/// Sets `key` for `window`'s project or globally, and tells the windows it concerns: all of
/// them for a global value, the project's for a project value. Returns the settings now in
/// effect for `window`.
pub fn set(window: &str, scope: SettingsScope, key: &str, value: &Value, cx: &mut App) -> IdeResult<Settings> {
    let Some(backend) = cx.try_global::<Backend>() else {
        return Err(IdeError::internal("the backend is not running"));
    };
    let host = backend.host.clone();
    // Applying a value may start background work, which needs the backend's runtime around it.
    let applied = backend
        .block_on(async { ide_host::ops::project::set_settings(&host, window, scope, key, &value.to_string()) })?;
    let event = IdeEvent::SettingsChanged { key: key.to_owned() };
    if applied.project_scope {
        backend.emit(Some(owner_window(window)), event);
        if let Some(root) = applied.roots_changed {
            backend.emit(Some(owner_window(window)), IdeEvent::ProjectRootsChanged { root });
        }
    } else {
        backend.emit(None, event);
    }
    Ok(applied.settings)
}

/// Writes global values in order, stopping at the first that fails.
pub fn set_global(window: &str, writes: &[SettingsWrite], cx: &mut App) -> IdeResult<Settings> {
    let mut settings = None;
    for write in writes {
        settings = Some(set(window, SettingsScope::Global, write.key, &write.value, cx)?);
    }
    Ok(settings.unwrap_or_else(|| effective(window, cx)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AppDirs;
    use gpui_kit::TestAppContext;
    use serde_json::json;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A global write is stored, is in effect at once, and reaches every listener as a
    /// settings-changed event for all windows.
    #[gpui_kit::test]
    async fn a_write_is_stored_and_announced(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let home = tempfile::tempdir().expect("a home");
        // The window each event was for, and the key it named.
        type Heard = Vec<(Option<String>, String)>;
        let heard: Rc<RefCell<Heard>> = Rc::default();
        let log = heard.clone();
        cx.update(|cx| {
            assert_eq!(effective("w1", cx).appearance.theme, Settings::default().appearance.theme);
            assert!(set("w1", SettingsScope::Global, "appearance.theme", &json!("dark"), cx).is_err());
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            cx.global_mut::<Backend>().listen(move |event, _| {
                if let IdeEvent::SettingsChanged { key } = &event.event {
                    log.borrow_mut().push((event.window.clone(), key.clone()));
                }
            });
            let writes = [
                SettingsWrite { key: "appearance.theme", value: json!("dark") },
                SettingsWrite { key: "appearance.syncWithOs", value: json!(true) },
            ];
            let settings = set_global("w1", &writes, cx).expect("the writes succeed");
            assert_eq!(settings.appearance.theme, "dark");
            assert!(settings.appearance.sync_with_os);
            assert_eq!(global(cx).appearance.theme, "dark");
            // A value of the wrong shape is refused and nothing is announced for it.
            assert!(set("w1", SettingsScope::Global, "appearance.zoom", &json!("big"), cx).is_err());
        });
        for _ in 0..300 {
            cx.run_until_parked();
            if heard.borrow().len() >= 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(
            *heard.borrow(),
            [(None, "appearance.theme".to_owned()), (None, "appearance.syncWithOs".to_owned())]
        );
        let stored = std::fs::read_to_string(home.path().join("config/settings.toml"));
        assert!(stored.is_ok_and(|text| text.contains("dark")), "the global settings file holds the value");
    }
}
