//! Generate Request(s) in HTTP Client: the requests of Spring endpoints rendered with the server
//! address and body skeletons, appended to the `generated-requests.http` scratch.
//!
//! The files it needs are read by the caller, one at a time: [`GenerateRequests::pending`] names
//! the next one and [`GenerateRequests::answer`] takes its text. Then [`GenerateRequests::blocks`]
//! renders the requests, and [`scratch_file`] says where they go.

use super::request::{BodyLookup, RenderOptions, config_candidates, host_from_files, render_request, skeleton};
use super::source::parse_source;
use super::spring::Endpoint;
use crate::platform::paths::ipc_path;

/// The scratch file generated requests are appended to.
pub const GENERATED_REQUESTS: &str = "generated-requests.http";

/// One request per endpoint; `path` picks another path of the mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub endpoint: Endpoint,
    pub path: Option<String>,
}

/// The scratch directory and the generated-requests file in it, as `/`-separated UI paths, for
/// the app data directory as the OS names it.
pub fn scratch_file(app_data_dir: &str) -> (String, String) {
    let dir = format!("{}/scratches", ipc_path(app_data_dir).trim_end_matches('/'));
    let path = format!("{dir}/{GENERATED_REQUESTS}");
    (dir, path)
}

/// Rendering the requests of endpoints of one source file.
#[derive(Debug)]
pub struct GenerateRequests {
    targets: Vec<Target>,
    configs: Vec<(String, Option<Option<String>>)>,
    bodies: BodyLookup,
}

impl GenerateRequests {
    /// `None` when there is nothing to generate.
    pub fn new(source_path: &str, text: &str, targets: Vec<Target>, project_root: Option<&str>) -> Option<Self> {
        if targets.is_empty() {
            return None;
        }
        let configs = config_candidates(source_path, project_root).into_iter().map(|p| (p, None)).collect();
        let endpoints: Vec<&Endpoint> = targets.iter().map(|t| &t.endpoint).collect();
        let bodies = BodyLookup::new(source_path, &parse_source(text), &endpoints);
        Some(Self { targets, configs, bodies })
    }

    /// The file to read next; `None` once everything is read.
    pub fn pending(&self) -> Option<&str> {
        match self.configs.iter().find(|(_, text)| text.is_none()) {
            Some((path, _)) => Some(path.as_str()),
            None => self.bodies.pending(),
        }
    }

    /// The text of the pending file, `None` when it is missing or unreadable.
    pub fn answer(&mut self, text: Option<&str>) {
        if let Some((_, slot)) = self.configs.iter_mut().find(|(_, text)| text.is_none()) {
            *slot = Some(text.map(str::to_owned));
        } else {
            self.bodies.answer(text);
        }
    }

    /// The request blocks, in target order.
    pub fn blocks(self) -> Vec<String> {
        let reads = self.configs.iter().map(|(path, text)| (path.as_str(), text.as_ref().and_then(Option::as_deref)));
        let host = host_from_files(reads);
        let host = host.as_str();
        let classes = self.bodies.finish();
        self.targets
            .iter()
            .map(|Target { endpoint, path }| {
                let body = endpoint.body_type.as_deref().map(|ty| skeleton(ty, &classes));
                render_request(endpoint, &RenderOptions { host, path: path.as_deref(), body })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::request::append_requests;
    use super::super::spring::spring_controllers;
    use super::*;

    const SCRATCH: &str = "C:/Users/me/AppData/Roaming/ai.l0ng.openidea/scratches/generated-requests.http";

    const JAVA: &str = r#"package com.example.demo.web;

import org.springframework.web.bind.annotation.*;

@RestController
@RequestMapping("/api")
public class DemoController {
    @PostMapping("/h66")
    public String h66() { return "ok"; }
}
"#;

    #[test]
    fn opens_the_scratch_at_its_ui_path_and_appends_the_request() {
        let (_, path) = scratch_file(r"C:\Users\me\AppData\Roaming\ai.l0ng.openidea");
        assert_eq!(path, SCRATCH);
        let endpoint = spring_controllers(JAVA).remove(0).endpoints.remove(0);
        let source = "D:/p/src/main/java/com/example/demo/web/DemoController.java";
        let mut generate =
            GenerateRequests::new(source, JAVA, vec![Target { endpoint, path: None }], None).expect("targets");
        // Files read as empty, like a project without Spring configuration files.
        while generate.pending().is_some() {
            generate.answer(Some(""));
        }
        let insert = append_requests("", &generate.blocks()).insert;
        assert!(insert.contains("### DemoController.h66"));
        assert!(insert.contains("POST http://localhost:8080/api/h66"));
    }
}
