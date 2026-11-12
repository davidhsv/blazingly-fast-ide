//! Run configurations: their types and helpers, what the editor context can run, the Run tool
//! window's sessions and consoles, and the launch flows around them.
//!
//! The models do no I/O. A flow that needs the backend, a dialog or a timer hands out the next step as
//! data and continues with its answer.

/// Declares a lazily compiled regular expression.
macro_rules! pattern {
    ($name:ident, $source:expr) => {
        static $name: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new($source).expect("the pattern is valid"));
    };
}

pub mod ansi;
pub mod application;
pub mod application_update;
pub mod before_launch;
pub mod build_command;
pub mod build_task;
pub mod cmake;
pub mod command_name;
pub mod config_chooser;
pub mod config_types;
pub mod configs;
pub mod console;
pub mod container_debug;
pub mod context;
pub mod csharp_targets;
pub mod django_types;
pub mod driver;
pub mod env_dialog;
pub mod executors;
pub mod go_actions;
pub mod go_arguments;
pub mod go_generate;
pub mod go_targets;
pub mod go_types;
pub mod gradle_run;
pub mod inherited_main;
pub mod java_debug;
pub mod java_launch;
pub mod jvm_targets;
pub mod jvm_types;
pub mod links;
pub mod navigate;
pub mod segments;
pub mod sessions;
pub mod store;
pub mod targets;
pub mod terminal_output;
pub mod types;
pub mod view;
