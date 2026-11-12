//! Feature logic without views: each module holds a feature's models, stores and step machines,
//! which take backend answers as arguments and return the requests to make.

pub mod analysis;
pub mod color_preview;
pub mod coverage;
pub mod dependencies;
pub mod deployment;
pub mod dotnet;
pub mod libraries;
pub mod macros;
pub mod mcp;
pub mod perf;
pub mod profiler;
pub mod remote_dev;
pub mod run_anything;
pub mod ssh;
pub mod terminal;
pub mod viewers;
