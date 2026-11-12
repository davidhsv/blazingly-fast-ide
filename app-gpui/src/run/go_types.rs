//! The Go Build and Go Test configuration types: one form, two sets of defaults.

use crate::platform::command_line::parse_args;
use crate::run::config_types::{
    BrowseKind, CommandLine, FieldControl, FieldOption, Params, RunConfigType, RunConfigTypeField, TypeEnv,
};
use crate::run::configs::{cwd_of, param};
use ide_run::RunConfig;

fn value<'a>(params: &'a Params, key: &str) -> &'a str {
    params.get(key).map_or("", String::as_str)
}

fn fields() -> Vec<RunConfigTypeField> {
    let is_test = |p: &Params| value(p, "goKind") == "test";
    let scopes = ["Directory", "File", "Package"].map(|label| FieldOption::new(label.to_lowercase(), label));
    vec![
        RunConfigTypeField::new("goKind", "Go configuration")
            .select(vec![FieldOption::new("build", "Go Build"), FieldOption::new("test", "Go Test")]),
        RunConfigTypeField::new("scope", "Run kind").select(scopes.to_vec()),
        RunConfigTypeField::new("target", "Target").mono().required().validate(|target, p| {
            let several_files = value(p, "goKind") == "test" && value(p, "scope") == "file" && target.contains('|');
            several_files.then(|| "Select one Go test file".to_string())
        }),
        RunConfigTypeField::new("outputDir", "Output directory")
            .mono()
            .browse(BrowseKind::Directory)
            .visible(move |p| !is_test(p)),
        RunConfigTypeField::new("runAfterBuild", "Run after build")
            .control(FieldControl::Checkbox)
            .visible(move |p| !is_test(p)),
        RunConfigTypeField::new("pattern", "Pattern").mono().visible(is_test),
        RunConfigTypeField::new("toolArgs", "Go tool arguments").mono(),
        RunConfigTypeField::new("useTags", "Use all custom build tags").control(FieldControl::Checkbox),
        RunConfigTypeField::new("programArgs", "Program arguments").mono(),
        RunConfigTypeField::new("module", "Module").mono().placeholder("Project root").suggestions(|configs, _| {
            let mut modules: Vec<String> = Vec::new();
            for config in configs.iter().filter(|config| config.kind == "go") {
                let module = param(config, "module").or_else(|| cwd_of(config)).unwrap_or_default().to_string();
                if !modules.contains(&module) {
                    modules.push(module);
                }
            }
            modules
        }),
    ]
}

/// The `go build` or `go test` command line of the form's values.
pub fn go_command_line(params: &Params, env: &TypeEnv) -> CommandLine {
    let tool = parse_args(value(params, "toolArgs"), env.windows);
    let target = Some(value(params, "target").trim()).filter(|target| !target.is_empty()).unwrap_or(".");
    let targets: Vec<String> = if value(params, "scope") == "file" {
        target.split('|').map(str::trim).filter(|file| !file.is_empty()).map(str::to_string).collect()
    } else {
        vec![target.to_string()]
    };
    let command = Some(value(params, "executable")).filter(|executable| !executable.is_empty()).unwrap_or("go");
    let mut args = Vec::new();
    if value(params, "goKind") == "test" {
        args.push("test".to_string());
        args.extend(tool);
        let pattern = value(params, "pattern");
        if !pattern.is_empty() {
            args.extend(["-run".to_string(), pattern.to_string()]);
        }
        args.extend(targets);
        let program = value(params, "programArgs");
        if !program.trim().is_empty() {
            args.push("-args".to_string());
            args.extend(parse_args(program, env.windows));
        }
    } else {
        args.push("build".to_string());
        args.extend(tool);
        args.extend(targets);
    }
    CommandLine::new(command, args)
}

/// The form's values for a plain `go run|build|test <target>` command line. Older arbitrary command
/// lines stay in the shared command form.
fn params_from(config: &RunConfig) -> Option<Params> {
    let [op, target] = config.args.as_slice() else { return None };
    if !matches!(op.as_str(), "run" | "build" | "test") || target.starts_with('-') {
        return None;
    }
    let pairs = [
        ("goKind", if op == "test" { "test" } else { "build" }),
        ("scope", "package"),
        ("target", target.as_str()),
        ("runAfterBuild", if op == "run" { "true" } else { "false" }),
        ("module", cwd_of(config).unwrap_or_default()),
        ("executable", config.command.as_str()),
    ];
    Some(pairs.into_iter().map(|(key, value)| (key.to_string(), value.to_string())).collect())
}

pub fn go_build_type() -> RunConfigType {
    RunConfigType::new("go", "Go Build", "file-go", |params, env| Some(go_command_line(params, env)))
        .description("Build and run a Go application")
        .defaults(&[
            ("goKind", "build"),
            ("scope", "package"),
            ("target", "."),
            ("runAfterBuild", "true"),
            ("useTags", "true"),
        ])
        .debug_adapter("delve")
        .no_terminal()
        .fields(fields())
        .params_from(params_from)
}

pub fn go_test_type() -> RunConfigType {
    let mut ty = go_build_type().description("Run Go tests").defaults(&[
        ("goKind", "test"),
        ("scope", "package"),
        ("target", "."),
        ("useTags", "true"),
    ]);
    ty.id = "go-test".to_string();
    ty.label = "Go Test".to_string();
    ty
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::types::{form_params, new_config_of_type, with_params};

    fn edited(params: &Params, changes: &[(&str, &str)]) -> Params {
        let mut next = params.clone();
        next.extend(changes.iter().map(|(key, value)| (key.to_string(), value.to_string())));
        next
    }

    #[test]
    fn keeps_build_arguments_and_program_arguments_separate_and_persists_the_argv() {
        let (ty, env) = (go_build_type(), TypeEnv::default());
        let config = new_config_of_type(&ty, &[], None, None, &env);
        let changes = [
            ("target", "./cmd/app"),
            ("toolArgs", "-tags \"demo other\" -race"),
            ("programArgs", "hello \"two words\""),
            ("module", "app"),
        ];
        let edited = with_params(&config, edited(&config.extras.params, &changes), Some(&ty), &env);
        assert_eq!(edited.args, ["build", "-tags", "demo other", "-race", "./cmd/app"]);
        let argv: Vec<String> = serde_json::from_str(&edited.extras.params["programArgv"]).expect("a JSON list");
        assert_eq!(argv, ["hello", "two words"]);
        assert!(!edited.is_test);
    }

    #[test]
    fn test_configs_use_the_existing_go_kind_and_put_test_binary_flags_after_args() {
        let env = TypeEnv::default();
        let config = new_config_of_type(&go_test_type(), &[], None, None, &env);
        let changes = [("pattern", "^TestOne$"), ("toolArgs", "-race"), ("programArgs", "-test.short")];
        let test = with_params(&config, edited(&config.extras.params, &changes), Some(&go_build_type()), &env);
        assert_eq!(test.kind, "go");
        assert!(test.is_test);
        assert_eq!(test.args, ["test", "-race", "-run", "^TestOne$", ".", "-args", "-test.short"]);
    }

    #[test]
    fn offers_existing_detected_configs_the_form_without_rewriting_old_saved_command_lines() {
        let (ty, env) = (go_build_type(), TypeEnv::default());
        let mut config = new_config_of_type(&ty, &[], None, None, &env);
        config.extras.params.clear();
        config.args = vec!["run".to_string(), ".".to_string()];
        let params = form_params(&config, Some(&ty), &env).expect("the form applies");
        let shown = ["goKind", "runAfterBuild", "target"].map(|key| value(&params, key));
        assert_eq!(shown, ["build", "true", "."]);
        assert_eq!(config.args, ["run", "."]);
        config.args = ["run", "-tags", "old", "."].map(str::to_string).to_vec();
        assert_eq!(form_params(&config, Some(&ty), &env), None);
    }
}
