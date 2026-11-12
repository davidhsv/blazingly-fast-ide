//! What the editor context can run, and the models around it: the producers of temporary run
//! configurations for a file, the caret or a directory, the context menus and gutter popups offering
//! them, the `package.json` script gutter, and the lookups those need.
//!
//! The models do no I/O. A flow that needs the backend, the file system or storage takes the answer as
//! an argument, or hands out the next step as data and continues with its answer.

/// Declares a lazily compiled regular expression.
macro_rules! pattern {
    ($name:ident, $source:expr) => {
        static $name: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new($source).expect("the pattern is valid"));
    };
}

pub mod console_folds;
pub mod context_run;
pub mod external_tools;
pub mod languages;
pub mod npm_gutter;
pub mod producers;
pub mod ruby_tests;
pub mod runtimes;
pub mod service_order;
pub mod stack_trace;
#[cfg(test)]
mod store_cases;
#[cfg(test)]
mod target_cases;
#[cfg(test)]
mod update_policy_cases;
