//! Django tests (PyCharm's "Django tests" configuration): `manage.py test` with a target (app labels,
//! modules or dotted test paths), a settings module and further options. The command is the
//! interpreter (`python3`, `.venv/bin/python`, `uv`), the runner args its leading arguments
//! (`run python` for uv / poetry).

use crate::platform::command_line::{format_args, parse_args};
use crate::run::config_types::{CommandLine, Params, RunConfigType, RunConfigTypeField, TypeEnv};
use ide_run::RunConfig;

/// A Django project: it has a Django tests configuration (detected from `manage.py`, or made in the form).
pub fn has_django_tests(configs: &[RunConfig]) -> bool {
    configs.iter().any(|config| config.kind == "django-test")
}

/// Index of `manage.py` when `test` follows it.
fn manage_test(args: &[String]) -> Option<usize> {
    let is_manage = |arg: &str| arg.rsplit(['/', '\\']).next() == Some("manage.py");
    (0..args.len()).find(|i| is_manage(&args[*i]) && args.get(i + 1).is_some_and(|next| next == "test"))
}

/// Number of leading positional arguments (the test labels).
fn label_count(args: &[String]) -> usize {
    args.iter().position(|arg| arg.starts_with('-')).unwrap_or(args.len())
}

/// Dotted test label of a module file: `polls/tests.py` → `polls.tests`, `polls/tests/__init__.py` →
/// `polls.tests`.
pub fn django_label(rel_path: &str) -> String {
    let module = rel_path.strip_suffix(".py").unwrap_or(rel_path);
    let module = match module.strip_suffix("__init__") {
        Some(package) if package.is_empty() || package.ends_with('/') => package.strip_suffix('/').unwrap_or(package),
        _ => module,
    };
    module.replace('/', ".")
}

/// `args` of a `manage.py test` command line running `labels` in place of its own target (the labels
/// right after `test`); `None` for other command lines.
fn with_django_labels(args: &[String], labels: &[String]) -> Option<Vec<String>> {
    let at = manage_test(args)?;
    let rest = &args[at + 2..];
    Some([&args[..at + 2], labels, &rest[label_count(rest)..]].concat())
}

/// The command line (and, for a configuration made in the form, the Target field) of `config` running
/// `labels`; `None` when it doesn't run `manage.py test`.
pub fn django_tests_of(config: &RunConfig, labels: &[String]) -> Option<(Vec<String>, Option<Params>)> {
    let args = with_django_labels(&config.args, labels)?;
    let params = config.extras.params.contains_key("target").then(|| {
        let mut params = config.extras.params.clone();
        params.insert("target".to_string(), format_args(labels));
        params
    });
    Some((args, params))
}

fn value<'a>(params: &'a Params, key: &str) -> &'a str {
    params.get(key).map_or("", String::as_str)
}

fn command_line(params: &Params, env: &TypeEnv) -> CommandLine {
    let mut args = parse_args(value(params, "runnerArgs"), env.windows);
    args.extend(["manage.py".to_string(), "test".to_string()]);
    args.extend(parse_args(value(params, "target"), env.windows));
    let settings = value(params, "settings").trim();
    if !settings.is_empty() {
        args.extend(["--settings".to_string(), settings.to_string()]);
    }
    args.extend(parse_args(value(params, "options"), env.windows));
    let command = Some(value(params, "command").trim()).filter(|command| !command.is_empty()).unwrap_or("python3");
    CommandLine::new(command, args)
}

fn params_from(config: &RunConfig) -> Option<Params> {
    if config.kind != "django-test" {
        return None;
    }
    let at = manage_test(&config.args)?;
    let rest = &config.args[at + 2..];
    let labels = label_count(rest);
    let mut options = &rest[labels..];
    let mut settings = "";
    if let [flag, value, others @ ..] = options
        && flag == "--settings"
    {
        settings = value.as_str();
        options = others;
    }
    let pairs = [
        ("command", config.command.clone()),
        ("runnerArgs", format_args(&config.args[..at])),
        ("target", format_args(&rest[..labels])),
        ("settings", settings.to_string()),
        ("options", format_args(options)),
    ];
    Some(pairs.into_iter().map(|(key, value)| (key.to_string(), value)).collect())
}

pub fn django_test_type() -> RunConfigType {
    RunConfigType::new("django-test", "Django tests", "file-python", |params, env| Some(command_line(params, env)))
        .description("Run Django tests with manage.py test")
        .defaults(&[("command", "python3")])
        .debug_adapter("debugpy")
        .fields(vec![
            RunConfigTypeField::new("command", "Command")
                .mono()
                .placeholder("python3")
                .required()
                .hint("e.g. python3, .venv/bin/python, uv, poetry"),
            RunConfigTypeField::new("runnerArgs", "Runner arguments")
                .mono()
                .placeholder("run python")
                .hint("e.g. `run python` for uv/poetry; empty calls the interpreter itself"),
            RunConfigTypeField::new("target", "Target")
                .mono()
                .placeholder("polls.tests")
                .hint("App labels, modules or dotted test paths; empty runs every test"),
            RunConfigTypeField::new("settings", "Settings module")
                .mono()
                .placeholder("mysite.settings_test")
                .hint("Passed as --settings; empty keeps DJANGO_SETTINGS_MODULE or the default in manage.py"),
            RunConfigTypeField::new("options", "Options").mono().placeholder("e.g. --keepdb --parallel"),
        ])
        .params_from(params_from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::blank_config;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    // Not from a spec: a module file is labelled by its dotted path, a package by its folder.
    #[test]
    fn labels_modules_and_packages() {
        assert_eq!(django_label("polls/tests.py"), "polls.tests");
        assert_eq!(django_label("polls/tests/__init__.py"), "polls.tests");
        assert_eq!(django_label("__init__.py"), "");
        assert_eq!(django_label("polls/my__init__.py"), "polls.my__init__");
    }

    // Not from a spec: labels replace the command line's own target and keep its options.
    #[test]
    fn replaces_the_labels_of_a_manage_py_test_command_line() {
        let mut config = blank_config("django:test", "test", "django-test");
        config.command = "uv".into();
        config.args = strings(&["run", "python", "manage.py", "test", "polls", "--keepdb"]);
        let labels = strings(&["polls.tests.QuestionTests"]);
        let (args, params) = django_tests_of(&config, &labels).expect("a Django test run");
        assert_eq!(args, ["run", "python", "manage.py", "test", "polls.tests.QuestionTests", "--keepdb"]);
        assert_eq!(params, None);
        config.extras.params.insert("target".into(), "polls".into());
        let (_, params) = django_tests_of(&config, &labels).expect("a Django test run");
        let target = params.and_then(|params| params.get("target").cloned());
        assert_eq!(target.as_deref(), Some("polls.tests.QuestionTests"));
        config.args = strings(&["manage.py", "runserver"]);
        assert_eq!(django_tests_of(&config, &labels), None);
    }

    // Not from a spec: the form reproduces a detected command line, settings module included.
    #[test]
    fn the_form_round_trips_a_command_line() {
        let ty = django_test_type();
        let mut config = blank_config("django:test", "test", "django-test");
        config.command = ".venv/bin/python".into();
        config.args = strings(&["manage.py", "test", "polls", "--settings", "site.test", "--keepdb"]);
        let params = ty.derive_params(&config).expect("the form applies");
        assert_eq!(value(&params, "settings"), "site.test");
        assert_eq!(value(&params, "options"), "--keepdb");
        let line = ty.line(&params, &TypeEnv::default()).expect("a command line");
        assert_eq!((line.command.as_str(), &line.args), (".venv/bin/python", &config.args));
    }
}
