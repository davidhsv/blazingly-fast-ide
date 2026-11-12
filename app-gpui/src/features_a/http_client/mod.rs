//! HTTP Client: highlighting of `.http` / `.rest` files, response body presentation, request URL
//! completion from the project's endpoints, and requests generated from Spring Web mappings in
//! Java / Kotlin sources.

pub mod endpoints;
pub mod format;
pub mod generate;
pub mod highlight;
pub mod request;
pub mod route_index;
pub mod source;
pub mod spring;
