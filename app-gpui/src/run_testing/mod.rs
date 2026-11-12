//! Test runs: the parsers that turn a test process's output into test events, the results tree the
//! events fold into, the last known state of each test, the command lines that run a selection of
//! tests again, and the executions the Services tool window lists.
//!
//! The models do no I/O. Output arrives as completed lines (see [`crate::run::store`]), report files
//! as their text, and a flow that needs the backend hands out its next step as data.

/// Declares a lazily compiled regular expression.
macro_rules! pattern {
    ($name:ident, $source:expr) => {
        static $name: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new($source).expect("the pattern is valid"));
    };
}

pub mod cargo;
pub mod comparison;
pub mod dotnet;
pub mod go;
pub mod js;
pub mod jvm;
pub mod parsers;
pub mod phpunit;
pub mod plugin;
pub mod pytest;
pub mod rerun;
pub mod results;
pub mod ruby;
pub mod service_sessions;
pub mod services;
pub mod states;
pub mod teamcity;
pub mod types;
pub mod unittest;
