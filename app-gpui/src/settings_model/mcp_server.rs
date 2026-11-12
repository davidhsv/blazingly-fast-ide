//! Settings › Tools › MCP Server: the page and the client configurations it copies.

use super::page_spec::{OptionSpec, PageSpec};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The state of the built-in MCP server, as the backend reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerStatus {
    pub running: bool,
    pub error: Option<String>,
    pub port: Option<u16>,
    pub project: Option<String>,
    pub url: Option<String>,
    pub token: Option<String>,
    /// The command line of the stdio bridge: the executable, then its arguments.
    pub stdio_command: Vec<String>,
}

fn client_config(server: Value) -> String {
    let config = json!({ "mcpServers": { "openidea": server } });
    // A JSON value of strings and lists always serializes.
    serde_json::to_string_pretty(&config).unwrap_or_default()
}

/// The `mcpServers` entry of an MCP client configuration (Claude Code, Cursor…) for the HTTP
/// endpoint; `None` while the server has no address or token.
pub fn http_config(status: &McpServerStatus) -> Option<String> {
    let (url, token) = (status.url.as_deref()?, status.token.as_deref()?);
    if url.is_empty() || token.is_empty() {
        return None;
    }
    let headers = json!({ "Authorization": format!("Bearer {token}") });
    Some(client_config(json!({ "type": "http", "url": url, "headers": headers })))
}

/// The same through the stdio bridge: no token, and it works for as long as the IDE runs.
pub fn stdio_config(status: &McpServerStatus) -> Option<String> {
    let (command, args) = status.stdio_command.split_first()?;
    if command.is_empty() {
        return None;
    }
    Some(client_config(json!({ "type": "stdio", "command": command, "args": args })))
}

/// The MCP Server page. It edits the global settings only.
pub fn mcp_server_page() -> PageSpec {
    PageSpec::new("tools.mcpServer", "MCP Server", 604)
        .group("Tools")
        .keywords(&["mcp", "model context protocol", "ai", "agent", "claude", "codex", "tools", "stdio"])
        .section(
            None,
            vec![
                OptionSpec::Toggle { key: "mcpServer.enabled", label: "Enable MCP server" },
                OptionSpec::Custom {
                    label: "Endpoint",
                    keywords: vec!["Copy HTTP Config", "Copy Stdio Config", "Client configuration", "Status"],
                },
            ],
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> McpServerStatus {
        McpServerStatus {
            running: true,
            error: None,
            port: Some(1234),
            project: Some("/p".to_owned()),
            url: Some("http://127.0.0.1:1234/mcp?project=/p".to_owned()),
            token: Some("secret".to_owned()),
            stdio_command: ["/Applications/OpenIDEA.app/Contents/MacOS/openidea", "--mcp-stdio", "--project", "/p"]
                .map(str::to_owned)
                .to_vec(),
        }
    }

    fn server(config: Option<String>) -> Value {
        let parsed: Value = config.and_then(|config| serde_json::from_str(&config).ok()).unwrap_or(Value::Null);
        parsed["mcpServers"]["openidea"].clone()
    }

    // MCP server settings › defaults to off and only offers global scope
    #[test]
    fn only_offers_global_scope() {
        let page = mcp_server_page();
        assert!(!page.project_scope);
        assert_eq!(page.sections[0].options[0].key(), Some("mcpServer.enabled"));
    }

    // MCP server settings › copies a reusable stdio configuration without the token
    #[test]
    fn copies_a_reusable_stdio_configuration_without_the_token() {
        let status = status();
        let config = stdio_config(&status).expect("a stdio configuration");
        let (command, args) = (&status.stdio_command[0], &status.stdio_command[1..]);
        let expected = json!({ "type": "stdio", "command": command, "args": args });
        assert_eq!(server(Some(config.clone())), expected);
        assert!(!config.contains("secret"));
        assert_eq!(stdio_config(&McpServerStatus { stdio_command: Vec::new(), ..status }), None);
    }

    // MCP server settings › copies the current HTTP endpoint and bearer header
    #[test]
    fn copies_the_current_http_endpoint_and_bearer_header() {
        let status = status();
        let expected = json!({
            "type": "http",
            "url": status.url,
            "headers": { "Authorization": "Bearer secret" },
        });
        assert_eq!(server(http_config(&status)), expected);
        assert_eq!(http_config(&McpServerStatus { url: None, token: None, ..status }), None);
    }
}
