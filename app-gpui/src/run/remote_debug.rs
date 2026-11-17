//! Debugging on run targets. The debugger runs on this machine, so Debug of a configuration that runs
//! on another machine (SSH), in a container (Docker, Compose) or on a plugin's target is refused with a
//! notice that offers Run or the configuration's editor instead.
//!
//! Python under debugpy in a container is debugged there only where the window can start the container
//! with debugpy waiting and attach to it; without that path it is refused like the rest.

use crate::platform::i18n::{I18n, Param};
use crate::platform_more::remote_settings::in_container;
use crate::run::store::{RunSettings, with_run_target};
use crate::workbench::notifications::{NotificationAction, NotificationLevel, NotificationRequest};
use ide_core::settings::RunTarget;
use ide_host::plugins::run_targets::RegisteredRunTarget;
use ide_run::RunConfig;

/// The notification key of the refusal: a second refusal replaces the first.
pub const NOTICE_KEY: &str = "run.remoteDebug";
/// The notice's Run button: `run.runConfig` for the configuration id that follows.
pub const RUN_COMMAND: &str = "run.runConfig:";
/// The notice's Edit Configuration… button: `run.editConfigs` for the configuration id that follows.
pub const EDIT_COMMAND: &str = "run.editConfigs:";

/// The run targets a configuration can name: those of the settings and those plugins registered.
pub struct Targets<'a> {
    pub saved: &'a [RunTarget],
    pub plugins: &'a [RegisteredRunTarget],
}

/// A target as the notice names it.
struct Named {
    name: String,
    container: bool,
}

impl Targets<'_> {
    fn find(&self, id: &str) -> Option<Named> {
        if let Some(target) = self.saved.iter().find(|target| target.id == id) {
            let name = target.name.trim();
            let name = if name.is_empty() { target.id.clone() } else { name.to_owned() };
            return Some(Named { name, container: in_container(target.kind) });
        }
        self.plugins
            .iter()
            .find(|target| target.id == id)
            .map(|target| Named { name: format!("{} ({})", target.target.name, target.plugin_name), container: false })
    }
}

/// The notice that refuses to debug `config`, or `None` when it runs on this machine. The run target is
/// the configuration's own, else the one the settings choose for its tool, else the default target.
/// `container_debug`: the window can debug Python in a container.
pub fn remote_debug_notice(
    config: &RunConfig,
    settings: &RunSettings,
    targets: &Targets,
    container_debug: bool,
    i18n: &mut I18n,
) -> Option<NotificationRequest> {
    // A compound runs nowhere itself: each of its configurations is asked when it is debugged.
    if config.kind == crate::run::types::COMPOUND {
        return None;
    }
    let config = with_run_target(config, settings);
    let id = config.extras.target.as_deref().filter(|target| !target.is_empty())?;
    let found = targets.find(id);
    let container = found.as_ref().is_some_and(|target| target.container);
    let python = config.debug_adapter.as_deref() == Some("debugpy");
    if container && python && container_debug {
        return None;
    }
    let target = found.map_or_else(|| id.to_owned(), |target| target.name);
    let title = if container && !python {
        "Only Python can be debugged in containers"
    } else {
        "Debugging on run targets is not supported"
    };
    let mut request = NotificationRequest::new(NotificationLevel::Warning, title);
    request.group = Some("run".to_owned());
    request.key = Some(NOTICE_KEY.to_owned());
    request.body = Some(i18n.tx_built(
        &["'", "' runs on ", ". Run it, or choose “Local machine” under “Run on” to debug it."],
        &[Param::from(config.name.as_str()), Param::from(target.as_str())],
    ));
    request.actions = vec![
        NotificationAction { label: "Run".to_owned(), command: format!("{RUN_COMMAND}{}", config.id) },
        NotificationAction {
            label: "Edit Configuration…".to_owned(), command: format!("{EDIT_COMMAND}{}", config.id)
        },
    ];
    Some(request)
}

/// The action and the configuration id a button of the notice names.
pub fn notice_command(command: &str) -> Option<(&'static str, &str)> {
    command
        .strip_prefix(RUN_COMMAND)
        .map(|id| ("run.runConfig", id))
        .or_else(|| command.strip_prefix(EDIT_COMMAND).map(|id| ("run.editConfigs", id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::blank_config;
    use ide_core::settings::RunTargetKind;

    fn target(kind: RunTargetKind, id: &str, name: &str) -> RunTarget {
        RunTarget { kind, id: id.to_owned(), name: name.to_owned(), ..RunTarget::default() }
    }

    fn on(target: &str, adapter: Option<&str>) -> RunConfig {
        let mut config = blank_config("app", "app.py", "python");
        config.debug_adapter = adapter.map(str::to_owned);
        config.extras.target = Some(target.to_owned());
        config
    }

    fn notice(config: &RunConfig, settings: &RunSettings, container_debug: bool) -> Option<NotificationRequest> {
        let saved = [target(RunTargetKind::Ssh, "box", "Build box"), target(RunTargetKind::Docker, "img", "  "), {
            target(RunTargetKind::Compose, "web", "Web")
        }];
        let targets = Targets { saved: &saved, plugins: &[] };
        remote_debug_notice(config, settings, &targets, container_debug, &mut I18n::default())
    }

    // Not from a spec: a configuration on this machine is debugged; one on an SSH target is refused with
    // the target's name and the configuration's id behind both buttons.
    #[test]
    fn refuses_a_configuration_on_another_machine_and_names_its_target() {
        let settings = RunSettings::default();
        assert!(notice(&blank_config("app", "app.py", "python"), &settings, false).is_none());
        assert!(notice(&on("", Some("debugpy")), &settings, false).is_none());
        let refused = notice(&on("box", Some("debugpy")), &settings, false).expect("refused");
        assert_eq!(refused.title, "Debugging on run targets is not supported");
        assert_eq!(
            refused.body.as_deref(),
            Some("'app.py' runs on Build box. Run it, or choose “Local machine” under “Run on” to debug it.")
        );
        assert_eq!((refused.level, refused.key.as_deref()), (NotificationLevel::Warning, Some(NOTICE_KEY)));
        let buttons: Vec<_> = refused.actions.iter().map(|a| (a.label.as_str(), a.command.as_str())).collect();
        assert_eq!(buttons, [("Run", "run.runConfig:app"), ("Edit Configuration…", "run.editConfigs:app")]);
        assert_eq!(notice_command("run.editConfigs:app"), Some(("run.editConfigs", "app")));
        assert_eq!(notice_command("run.runConfig:app"), Some(("run.runConfig", "app")));
        assert_eq!(notice_command("run.debugConfig:app"), None);
    }

    // Not from a spec: in a container only Python is debugged, and only where the window can start the
    // container under debugpy; a target that is gone is named by its id, an unnamed one by its id too.
    #[test]
    fn in_a_container_only_python_is_debugged_and_only_with_the_container_path() {
        let settings = RunSettings::default();
        let node = notice(&on("web", Some("node")), &settings, true).expect("refused");
        assert_eq!(node.title, "Only Python can be debugged in containers");
        assert!(notice(&on("web", Some("debugpy")), &settings, true).is_none());
        let without_path = notice(&on("web", Some("debugpy")), &settings, false).expect("refused");
        assert_eq!(without_path.title, "Debugging on run targets is not supported");
        let unnamed = notice(&on("img", Some("debugpy")), &settings, false).expect("refused");
        assert!(unnamed.body.as_deref().is_some_and(|body| body.contains("runs on img.")), "{:?}", unnamed.body);
        let gone = notice(&on("gone", None), &settings, false).expect("refused");
        assert!(gone.body.as_deref().is_some_and(|body| body.contains("runs on gone.")), "{:?}", gone.body);
    }

    // Not from a spec: a configuration that names no target takes the settings' target for its tool or
    // the default target; `local` keeps a PHP tool on this machine whatever the default is. A compound is
    // not refused by its own name.
    #[test]
    fn the_settings_target_applies_to_a_configuration_without_its_own() {
        let mut settings = RunSettings { default_target: "box".to_owned(), ..RunSettings::default() };
        assert!(notice(&blank_config("app", "app.py", "python"), &settings, false).is_some());
        assert!(notice(&blank_config("all", "All", "compound"), &settings, false).is_none());
        settings.php_target = "local".to_owned();
        assert!(notice(&blank_config("index", "php", "php"), &settings, false).is_none());
    }
}
