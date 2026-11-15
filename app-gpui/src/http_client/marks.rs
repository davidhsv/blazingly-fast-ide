//! The gutter marks of the HTTP Client: a run mark on the request line of every request of an
//! `.http` file, and a generate mark on every Spring controller and request mapping of a Java or
//! Kotlin file, with what each offers.
//!
//! Lines are 0-based; offsets are UTF-16 code units, as the editor counts them.

use crate::features_a::http_client::spring::{Controller, Endpoint, spring_at, spring_controllers};
use ide_http::HttpRequestBlock;

/// The run mark of one request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestMark {
    /// 0-based index among the file's requests.
    pub index: u32,
    /// The request line, where the mark is drawn.
    pub line: u32,
    /// First and last line of the request's block.
    pub start_line: u32,
    pub end_line: u32,
    /// What the mark's tooltip names: the request's name, else its method and URL.
    pub label: String,
}

/// The marks of `blocks` (a file's requests) in a text of `line_count` lines; a request past the
/// end of the text (parsed from an older text) has none.
pub fn request_marks(blocks: &[HttpRequestBlock], line_count: u32) -> Vec<RequestMark> {
    blocks
        .iter()
        .filter(|block| block.line < line_count)
        .map(|block| RequestMark {
            index: block.index,
            line: block.line,
            start_line: block.start_line,
            end_line: block.end_line,
            label: block.name.clone().unwrap_or_else(|| format!("{} {}", block.method, block.url)),
        })
        .collect()
}

/// The request whose block holds the caret's `line`, else the nearest one above it, else the first.
pub fn request_at_line(marks: &[RequestMark], line: u32) -> Option<&RequestMark> {
    let mut best = None;
    for mark in marks {
        if mark.start_line <= line && line <= mark.end_line.max(mark.line) {
            return Some(mark);
        }
        if mark.line <= line {
            best = Some(mark);
        }
    }
    best.or_else(|| marks.first())
}

/// The 0-based line of UTF-16 offset `offset` of `text` (the last line for an offset past its end).
pub fn line_of(text: &str, offset: usize) -> u32 {
    let mut at = 0usize;
    let mut line = 0u32;
    for c in text.chars() {
        if at >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
        }
        at += c.len_utf16();
    }
    line
}

/// What a generate mark stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpringMarkKind {
    /// A controller with mappings: Generate Requests for Controller.
    Controller { name: String },
    /// A request mapping: Generate Request in HTTP Client (one row per path when it has several).
    Endpoint { method: String, paths: Vec<String> },
}

/// A generate mark of a Java or Kotlin file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpringMark {
    pub line: u32,
    /// The UTF-16 offset of the controller's or the method's name: the caret the mark's actions
    /// work at.
    pub offset: usize,
    pub kind: SpringMarkKind,
}

impl SpringMark {
    /// The tooltip: the action of a controller, else the mapping's method and first path.
    pub fn title(&self) -> String {
        match &self.kind {
            SpringMarkKind::Controller { .. } => "Generate Requests for Controller".to_owned(),
            SpringMarkKind::Endpoint { method, paths } => {
                format!("{method} {}", paths.first().map_or("/", String::as_str))
            }
        }
    }
}

/// The generate marks of `text`: one per controller with mappings and one per mapping, the first
/// one of a line kept.
pub fn spring_marks(text: &str) -> Vec<SpringMark> {
    let mut marks: Vec<SpringMark> = Vec::new();
    let mut push = |mark: SpringMark| {
        if !marks.iter().any(|m| m.line == mark.line) {
            marks.push(mark);
        }
    };
    for controller in spring_controllers(text) {
        if !controller.endpoints.is_empty() {
            push(SpringMark {
                line: line_of(text, controller.offset),
                offset: controller.offset,
                kind: SpringMarkKind::Controller { name: controller.name.clone() },
            });
        }
        for endpoint in &controller.endpoints {
            push(SpringMark {
                line: line_of(text, endpoint.offset),
                offset: endpoint.offset,
                kind: SpringMarkKind::Endpoint { method: endpoint.http_method.clone(), paths: endpoint.paths.clone() },
            });
        }
    }
    marks
}

/// What can be generated at a caret: the mapping it is in and the mappings of its controller.
pub struct SpringTargets {
    pub endpoint: Option<Endpoint>,
    pub controller: Option<Controller>,
}

/// The mapping and the controller holding UTF-16 offset `caret` of `text`.
pub fn spring_targets(text: &str, caret: usize) -> SpringTargets {
    let controllers = spring_controllers(text);
    let at = spring_at(&controllers, caret);
    SpringTargets { endpoint: at.endpoint.cloned(), controller: at.controller.cloned() }
}

/// A row of the editor menu's Spring group or of a generate mark's menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpringRow {
    /// Generate Request in HTTP Client for the mapping at the caret (its first path).
    Endpoint,
    /// Generate Request in HTTP Client, a submenu with one `METHOD path` row per path.
    EndpointPaths { method: String, paths: Vec<String> },
    /// Generate Requests for Controller.
    Controller,
}

/// The Spring rows for the caret at `caret`: the mapping's (a path choice for several paths), then
/// the controller's when it has mappings.
pub fn spring_rows(text: &str, caret: usize) -> Vec<SpringRow> {
    let targets = spring_targets(text, caret);
    let mut rows = Vec::new();
    if let Some(endpoint) = &targets.endpoint {
        rows.push(if endpoint.paths.len() > 1 {
            SpringRow::EndpointPaths { method: endpoint.http_method.clone(), paths: endpoint.paths.clone() }
        } else {
            SpringRow::Endpoint
        });
    }
    if targets.controller.is_some_and(|controller| !controller.endpoints.is_empty()) {
        rows.push(SpringRow::Controller);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(index: u32, line: u32, start_line: u32, end_line: u32, name: Option<&str>) -> HttpRequestBlock {
        HttpRequestBlock {
            index,
            name: name.map(str::to_owned),
            method: "GET".to_owned(),
            url: format!("/r{index}"),
            line,
            start_line,
            end_line,
        }
    }

    const CONTROLLER: &str = "import org.springframework.web.bind.annotation.*;\n\n\
        @RestController\n@RequestMapping(\"/api\")\npublic class Users {\n    \
        @GetMapping({\"/a\", \"/b\"})\n    public String list() { return \"\"; }\n\n    \
        @PostMapping(\"/c\")\n    public String add() { return \"\"; }\n}\n";

    /// Every request gets a mark on its request line, titled by its name or its method and URL; one
    /// past the end of the text has none.
    #[test]
    fn marks_every_request_line_and_names_it() {
        let blocks = [block(0, 1, 0, 2, Some("Users")), block(1, 4, 4, 6, None), block(2, 9, 9, 9, None)];
        let marks = request_marks(&blocks, 8);
        let shown: Vec<(u32, &str)> = marks.iter().map(|m| (m.line, m.label.as_str())).collect();
        assert_eq!(shown, [(1, "Users"), (4, "GET /r1")]);
    }

    /// The caret's request is the one whose block holds it, else the nearest above, else the first.
    #[test]
    fn finds_the_request_of_the_caret_line() {
        let marks = request_marks(&[block(0, 2, 1, 3, None), block(1, 6, 5, 7, None)], 10);
        let at = |line| request_at_line(&marks, line).map(|m| m.index);
        assert_eq!(at(2), Some(0));
        assert_eq!(at(7), Some(1));
        assert_eq!(at(4), Some(0), "between blocks: the one above");
        assert_eq!(at(0), Some(0), "above every block: the first");
        assert_eq!(request_at_line(&[], 3), None);
    }

    /// The controller and each mapping get a mark on their line, titled by the action or the mapping.
    #[test]
    fn marks_the_controller_and_its_mappings() {
        let marks = spring_marks(CONTROLLER);
        let shown: Vec<(u32, String)> = marks.iter().map(|m| (m.line, m.title())).collect();
        assert_eq!(
            shown,
            [
                (4, "Generate Requests for Controller".to_owned()),
                (6, "GET /api/a".to_owned()),
                (9, "POST /api/c".to_owned())
            ]
        );
        assert!(spring_marks("class Plain {}\n").is_empty());
    }

    /// A mapping with several paths offers a path choice; the controller's row follows.
    #[test]
    fn offers_a_path_choice_for_a_mapping_with_several_paths() {
        let list = CONTROLLER.find("list").expect("the method");
        assert_eq!(
            spring_rows(CONTROLLER, list),
            [
                SpringRow::EndpointPaths {
                    method: "GET".to_owned(),
                    paths: vec!["/api/a".to_owned(), "/api/b".to_owned()]
                },
                SpringRow::Controller
            ]
        );
        let add = CONTROLLER.find("add()").expect("the method");
        assert_eq!(spring_rows(CONTROLLER, add), [SpringRow::Endpoint, SpringRow::Controller]);
        assert!(spring_rows("class Plain {}\n", 3).is_empty());
    }

    #[test]
    fn counts_lines_in_utf16_offsets() {
        assert_eq!(line_of("é\nb\nc", 2), 1);
        assert_eq!(line_of("a\nb", 99), 1);
        assert_eq!(line_of("", 0), 0);
    }
}
