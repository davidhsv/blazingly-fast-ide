//! Run targets plugins register for a project (a container a project's PHP / Node / Composer runs in).
//! A registration that asks for it offers Use / Not now once per project; the answer goes to the host,
//! which returns the settings it changed. The Settings window lists the targets without asking.
//!
//! [`RunTargets`] calls nothing: a refresh returns the request to make, its answer returns the
//! [`RunTargetEffect`]s; an answer for an older request or another project is dropped.

use crate::platform::i18n::{I18n, Param};
use crate::workbench::notifications::{NotificationAction, NotificationLevel, NotificationRequest};
use ide_host::plugins::run_targets::RegisteredRunTarget;
use std::collections::HashSet;

const KEY_PREFIX: &str = "plugin-run-target:";
const USE_COMMAND: &str = "plugin-run-target.use:";
const NOT_NOW_COMMAND: &str = "plugin-run-target.not-now:";

/// The notification key of a target's question.
pub fn notification_key(target_id: &str) -> String {
    format!("{KEY_PREFIX}{target_id}")
}

/// The target and the answer a button of the question carries (`Use`: true).
pub fn parse_answer(command: &str) -> Option<(&str, bool)> {
    command
        .strip_prefix(USE_COMMAND)
        .map(|id| (id, true))
        .or_else(|| command.strip_prefix(NOT_NOW_COMMAND).map(|id| (id, false)))
}

/// A `plugin_run_targets` request for project `root`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetsRequest {
    revision: u64,
    pub root: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RunTargetEffect {
    /// Withdraw the question with this notification key.
    Expire(String),
    Notify(NotificationRequest),
}

#[derive(Debug, Default)]
pub struct RunTargets {
    revision: u64,
    targets: Vec<RegisteredRunTarget>,
    /// `(root, target)` whose question was shown.
    shown: HashSet<(String, String)>,
}

impl RunTargets {
    pub fn targets(&self) -> &[RegisteredRunTarget] {
        &self.targets
    }

    /// Forgets the targets (another project, or none); `prompts`: their questions are withdrawn.
    pub fn clear(&mut self, prompts: bool) -> Vec<RunTargetEffect> {
        self.revision += 1;
        let effects = if prompts {
            self.targets.iter().map(|t| RunTargetEffect::Expire(notification_key(&t.id))).collect()
        } else {
            Vec::new()
        };
        self.shown.clear();
        self.targets.clear();
        effects
    }

    /// Asks for the targets of `root`; without a project they are cleared.
    pub fn refresh(&mut self, root: Option<&str>, prompts: bool) -> (Option<TargetsRequest>, Vec<RunTargetEffect>) {
        let Some(root) = root else { return (None, self.clear(prompts)) };
        self.revision += 1;
        (Some(TargetsRequest { revision: self.revision, root: root.to_owned() }), Vec::new())
    }

    /// The targets for `request`, with the window's project root now. With `prompts`, a target that
    /// asks is offered once, and questions of targets that no longer ask are withdrawn.
    pub fn answered(
        &mut self,
        request: &TargetsRequest,
        root: Option<&str>,
        targets: Vec<RegisteredRunTarget>,
        prompts: bool,
        i18n: &mut I18n,
    ) -> Vec<RunTargetEffect> {
        if request.revision != self.revision || root != Some(request.root.as_str()) {
            return Vec::new();
        }
        let mut effects = Vec::new();
        if prompts {
            for previous in &self.targets {
                if !targets.iter().any(|t| t.id == previous.id && t.prompt) {
                    effects.push(RunTargetEffect::Expire(notification_key(&previous.id)));
                    self.shown.remove(&(request.root.clone(), previous.id.clone()));
                }
            }
        }
        self.targets = targets;
        if !prompts {
            return effects;
        }
        for target in &self.targets {
            if !target.prompt || !self.shown.insert((request.root.clone(), target.id.clone())) {
                continue;
            }
            let body = i18n.tx_built(
                &["Use ", " for running PHP / Node / Composer in this project?"],
                &[Param::Text(target.target.name.clone())],
            );
            effects.push(RunTargetEffect::Notify(NotificationRequest {
                sticky: Some(true),
                group: Some("plugins".to_owned()),
                key: Some(notification_key(&target.id)),
                body: Some(body),
                actions: vec![
                    NotificationAction { label: i18n.tx("Use"), command: format!("{USE_COMMAND}{}", target.id) },
                    NotificationAction {
                        label: i18n.tx("Not now"),
                        command: format!("{NOT_NOW_COMMAND}{}", target.id),
                    },
                ],
                ..NotificationRequest::new(NotificationLevel::Info, i18n.tx("Run Targets"))
            }));
        }
        effects
    }

    /// Whether an answer to a question of `root` still applies (the window's project is `current`).
    pub fn may_answer(root: &str, current: Option<&str>) -> bool {
        current == Some(root)
    }

    /// The host refused the answer: the question may be shown again, and the error is reported.
    pub fn answer_failed(&mut self, root: &str, target_id: &str, error: &str, i18n: &I18n) -> RunTargetEffect {
        self.shown.remove(&(root.to_owned(), target_id.to_owned()));
        RunTargetEffect::Notify(NotificationRequest {
            group: Some("plugins".to_owned()),
            body: Some(error.to_owned()),
            ..NotificationRequest::new(NotificationLevel::Error, i18n.tx("Run Targets"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn target(prompt: bool) -> RegisteredRunTarget {
        serde_json::from_value(json!({
            "id": "plugin-target:4:testddev",
            "pluginId": "test",
            "pluginName": "Test",
            "prompt": prompt,
            "target": {
                "id": "ddev", "name": "DDEV", "program": "ddev", "args": ["exec"],
                "workdirArgs": [], "containerRoot": null, "commands": [],
            },
        }))
        .expect("a target")
    }

    fn ask(
        targets: &mut RunTargets,
        root: &str,
        listed: Vec<RegisteredRunTarget>,
        prompts: bool,
    ) -> Vec<RunTargetEffect> {
        let (request, _) = targets.refresh(Some(root), prompts);
        targets.answered(&request.expect("a request"), Some(root), listed, prompts, &mut I18n::default())
    }

    fn questions(effects: &[RunTargetEffect]) -> Vec<&NotificationRequest> {
        effects.iter().filter_map(|e| if let RunTargetEffect::Notify(n) = e { Some(n) } else { None }).collect()
    }

    #[test]
    fn loads_settings_window_targets_after_its_project_arrives_without_prompting_there() {
        let mut targets = RunTargets::default();
        assert_eq!(targets.refresh(None, false), (None, Vec::new()));
        let effects = ask(&mut targets, "/settings-owner", vec![target(true)], false);
        assert!(effects.is_empty(), "no question in the Settings window");
        assert_eq!(targets.targets().len(), 1);
        assert!(ask(&mut targets, "/settings-owner", Vec::new(), false).is_empty());
        assert!(targets.targets().is_empty());
    }

    #[test]
    fn offers_use_not_now_once_without_changing_the_selection_on_registration() {
        let mut targets = RunTargets::default();
        let effects = ask(&mut targets, "/project", vec![target(true)], true);
        let [question] = questions(&effects)[..] else { panic!("one question") };
        assert_eq!(question.body.as_deref(), Some("Use DDEV for running PHP / Node / Composer in this project?"));
        let labels: Vec<&str> = question.actions.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(labels, ["Use", "Not now"]);
        assert_eq!(parse_answer(&question.actions[1].command), Some(("plugin-target:4:testddev", false)));
        assert!(questions(&ask(&mut targets, "/project", vec![target(true)], true)).is_empty(), "asked once");
        assert!(RunTargets::may_answer("/project", Some("/project")));
    }

    #[test]
    fn use_applies_only_the_host_returned_settings_and_a_removed_target_disappears() {
        let mut targets = RunTargets::default();
        let effects = ask(&mut targets, "/project", vec![target(true)], true);
        let question = questions(&effects)[0].clone();
        assert_eq!(parse_answer(&question.actions[0].command), Some(("plugin-target:4:testddev", true)));
        ask(&mut targets, "/project", vec![target(false)], true);
        let effects = ask(&mut targets, "/project", Vec::new(), true);
        assert!(targets.targets().is_empty());
        assert_eq!(effects, vec![RunTargetEffect::Expire(notification_key("plugin-target:4:testddev"))]);
        let (request, _) = targets.refresh(Some("/project"), true);
        let request = request.expect("a request");
        let stale = targets.answered(&request, Some("/other"), vec![target(true)], true, &mut I18n::default());
        assert!(stale.is_empty(), "an answer for another project is dropped");
    }
}
