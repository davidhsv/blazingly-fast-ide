//! The problems of the window reach the IDE's MCP host.

use super::*;
use ide_mcp::ToolRegistry;

/// The problem a language server reports for an open file is reported to the IDE's MCP host: an MCP client asking
/// for the file's problems gets it, with its 1-based line and column.
#[gpui_kit::test]
async fn the_problems_of_an_open_file_reach_the_mcp_hosts_file_problems(cx: &mut TestAppContext) {
    let problem = json!({ "range": range(0, 4, 7), "message": "unknown foo", "severity": 1, "source": "mock" });
    let answers = json!({ "diagnostics": [problem] });
    let (f, cx) = open(cx, &[("a.mock", "let foo = 1\n")], "a.mock", Some(answers), &[]);
    let registry = ide_host::mcp::Registry::new(cx.update(|_, cx| cx.global::<Backend>().host.clone()));
    let root = f.root.clone();
    let errors = |registry: &ide_host::mcp::Registry| -> Vec<Value> {
        let arguments = r#"{"filePath":"a.mock","errorsOnly":true}"#;
        let answer = registry.call(&root, "get_file_problems", arguments, ide_core::CancellationToken::new());
        let answer: Option<Value> = answer.ok().and_then(|text| serde_json::from_str(&text).ok());
        answer.and_then(|answer| answer["errors"].as_array().cloned()).unwrap_or_default()
    };
    settle(cx, "the problem reported to the MCP host", |_| !errors(&registry).is_empty());
    let reported = errors(&registry);
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert_eq!(reported[0]["description"], "unknown foo");
    assert_eq!((reported[0]["line"].as_u64(), reported[0]["column"].as_u64()), (Some(1), Some(5)));
    f.close(cx);
}
