//! The application update policies held to what their forms and sessions must offer: the policy
//! fields of [`crate::run::jvm_types`] and the guards of [`crate::run::application_update`].

use crate::run::application_update::{UpdateMode, is_updatable, update_allowed};
use crate::run::config_types::RunConfigTypeField;
use crate::run::configs::blank_config;
use crate::run::jvm_types::{UPDATE_POLICY_DEFAULTS, application_type, spring_boot_type, update_policy_fields};
use ide_run::RunConfig;

fn config(kind: &str) -> RunConfig {
    blank_config(kind, kind, kind)
}

/// Whether the mode named `mode` applies; a name that is no mode never does.
fn allowed(config: &RunConfig, mode: &str) -> bool {
    UpdateMode::parse(mode).is_some_and(|mode| update_allowed(config, mode))
}

fn option_values<'f>(fields: &'f [RunConfigTypeField], key: &str) -> Vec<&'f str> {
    let field = fields.iter().find(|field| field.key == key);
    field.map(|field| field.options.iter().map(|option| option.value.as_str()).collect()).unwrap_or_default()
}

#[test]
fn shares_safe_defaults_and_policy_fields_in_tomcat_and_spring_boot() {
    let spring = spring_boot_type();
    let defaults = ["onUpdate", "onDeactivation"].map(|key| spring.defaults.get(key).map(String::as_str));
    assert_eq!(defaults, [Some("ask"), Some("none")]);
    assert_eq!(UPDATE_POLICY_DEFAULTS, [("onUpdate", "ask"), ("onDeactivation", "none")]);
    let tomcat = update_policy_fields(true);
    assert_eq!(option_values(&tomcat, "onUpdate"), ["ask", "resources", "classes", "redeploy", "restart"]);
    assert_eq!(option_values(&spring.fields, "onUpdate"), ["ask", "resources", "classes", "restart"]);
    for fields in [&tomcat, &spring.fields] {
        assert_eq!(option_values(fields, "onDeactivation"), ["none", "resources", "classes"]);
    }
}

#[test]
fn keeps_plain_java_applications_and_build_tool_goals_outside_the_feature() {
    assert!(!application_type().fields.iter().any(|field| field.key == "onUpdate"));
    for kind in ["application", "maven", "gradle", "java-file"] {
        assert!(!is_updatable(&config(kind)), "{kind}");
    }
}

#[test]
fn enables_local_tomcat_and_spring_boot_sessions() {
    for kind in ["tomcat", "spring-boot"] {
        let local = config(kind);
        assert!(is_updatable(&local), "{kind}");
        for mode in ["resources", "classes", "restart"] {
            assert!(allowed(&local, mode), "{kind}: {mode}");
        }
        assert!(!allowed(&local, "invalid"), "{kind}");
        let mut remote = config(kind);
        remote.extras.target = Some("remote".to_string());
        assert!(!is_updatable(&remote), "{kind}");
    }
}

#[test]
fn preserves_war_restrictions_and_limits_redeploy_to_tomcat() {
    let mut war = config("tomcat");
    war.extras.params.insert("deployment".to_string(), "war".to_string());
    assert!(!allowed(&war, "resources"));
    assert!(!allowed(&war, "classes"));
    assert!(allowed(&war, "redeploy"));
    assert!(!allowed(&config("spring-boot"), "redeploy"));
}
