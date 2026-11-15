//! The global mise config limited to the files a caller names. One test in its own binary: it sets
//! the process's global config files and the variable that names the user's.

use ide_core::version_managers::Tool;
use ide_sdk::pinned::{pinned_version, set_global_configs};

#[test]
fn the_global_config_is_read_until_an_empty_list_is_named_and_again_after_none() {
    let tmp = tempfile::tempdir().unwrap();
    let global = tmp.path().join("xdg/mise/config.toml");
    std::fs::create_dir_all(global.parent().unwrap()).unwrap();
    std::fs::write(&global, "[tools]\nnode = '20.11'\n").unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    // The variable is set for the lookup (and for children), not in this process's environment.
    ide_core::process::set_env_overlay(vec![("MISE_GLOBAL_CONFIG_FILE".into(), Some(global.clone().into()))]);

    assert_eq!(pinned_version(&project, Tool::Node).as_deref(), Some("20.11"), "the user's global config");
    set_global_configs(Some(Vec::new()));
    assert_eq!(pinned_version(&project, Tool::Node), None, "no global config is read");
    std::fs::write(project.join("mise.toml"), "[tools]\nnode = '22'\n").unwrap();
    assert_eq!(pinned_version(&project, Tool::Node).as_deref(), Some("22"), "the project's own config still is");
    std::fs::remove_file(project.join("mise.toml")).unwrap();
    set_global_configs(None);
    assert_eq!(pinned_version(&project, Tool::Node).as_deref(), Some("20.11"), "the user's again");
}
