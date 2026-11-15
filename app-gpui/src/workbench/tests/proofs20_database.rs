//! A data source whose server refuses the connection for want of a password asks for it; the answer is
//! handed to the backend for the session and the connection is tried again with it.

use super::*;
use crate::workbench::database::FieldsPrompt;
use std::io::{Read as _, Write as _};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

/// The password the server takes for the user `reader`.
const PASSWORD: &str = "Zq7hunter";
/// The `Authorization` header of `reader` with [`PASSWORD`].
const CREDENTIALS: &str = "Basic cmVhZGVyOlpxN2h1bnRlcg==";
/// The one database the server lists.
const SCHEMA: &str = "analytics";
/// What the server answers an authenticated request: a column `name` holding [`SCHEMA`].
const ANSWER: &str = "[\"name\"]\n[\"String\"]\n[\"analytics\"]\n";
/// What the server answers a request without the right password, as ClickHouse does.
const REFUSAL: &str =
    "Code: 516. DB::Exception: reader: Authentication failed: password is incorrect. (AUTHENTICATION_FAILED)";

/// The head of the request read from `stream`, once its body has been read too; `None` when the client
/// went away.
fn read_request(stream: &mut TcpStream) -> Option<String> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    let end = loop {
        let count = stream.read(&mut buffer).ok().filter(|count| *count > 0)?;
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
    };
    let head = String::from_utf8_lossy(&bytes[..end]).into_owned();
    let length: usize = head
        .lines()
        .find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").and_then(|v| v.trim().parse().ok()))
        .unwrap_or(0);
    while bytes.len() < end + 4 + length {
        let count = stream.read(&mut buffer).ok().filter(|count| *count > 0)?;
        bytes.extend_from_slice(&buffer[..count]);
    }
    Some(head)
}

/// How many requests the server refused and answered.
type Seen = Arc<Mutex<(usize, usize)>>;

/// A ClickHouse HTTP server on a loopback port, as far as the Database tools ask of it: a request whose
/// Basic authentication is not `reader` with [`PASSWORD`] fails as ClickHouse fails a login (403, code
/// 516); every other one is answered [`ANSWER`]. Each connection carries one request.
fn clickhouse_server() -> (u16, Seen) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a loopback port");
    let port = listener.local_addr().expect("the listener is bound").port();
    let seen: Seen = Arc::default();
    let counts = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let Some(head) = read_request(&mut stream) else { continue };
            let authorized = head.lines().any(|line| {
                line.split_once(':').is_some_and(|(name, value)| {
                    name.eq_ignore_ascii_case("authorization") && value.trim() == CREDENTIALS
                })
            });
            let (status, headers, body) = if authorized {
                ("200 OK", "X-ClickHouse-Format: JSONCompactEachRowWithNamesAndTypes\r\n", ANSWER)
            } else {
                ("403 Forbidden", "X-ClickHouse-Exception-Code: 516\r\n", REFUSAL)
            };
            {
                let mut counts = counts.lock().expect("the counts are readable");
                if authorized {
                    counts.1 += 1;
                } else {
                    counts.0 += 1;
                }
            }
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
                body.len()
            );
            // A client that gave up on the answer has nothing more to read.
            if stream.write_all(response.as_bytes()).is_ok() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
    });
    (port, seen)
}

/// Expanding a ClickHouse data source whose server wants a password asks for it ("Save password" off, so
/// nothing is stored); Enter hands it to the backend, the connection is tried again with it, and the
/// tree lists the database the server answers with, the source connected.
#[gpui_kit::test]
async fn the_password_a_connection_asks_for_reaches_the_backend_and_the_retry_connects(cx: &mut TestAppContext) {
    // The source's proxy mode is the default, the environment's proxy, which does not exempt loopback:
    // with one set, the request and its password would go to that proxy instead of the stand-in server.
    for name in ["http_proxy", "HTTP_PROXY", "https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"] {
        assert!(std::env::var_os(name).is_none(), "{name} is set: the password must not reach a proxy");
    }
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "notes\n")]);
    cx.update(|window, _| window.activate_window());
    let (port, seen) = clickhouse_server();
    let source = ide_db::DataSource {
        id: "ch".into(),
        name: "warehouse".into(),
        driver: ide_db::DbDriver::Clickhouse,
        host: "127.0.0.1".into(),
        port: Some(port),
        user: "reader".into(),
        ..Default::default()
    };
    assert!(!source.save_password, "the password is kept for the session only");
    cx.update(|_, cx| {
        let backend = cx.global::<Backend>();
        let host = backend.host.clone();
        backend.block_on(async move {
            ide_host::ops::db::save_sources(&host, "w1", vec![source]).await.expect("the source is saved");
        });
    });
    run_action(cx, "db.showToolWindow", None);
    let tree = db_tree(&workbench, cx);
    let connected = |cx: &mut VisualTestContext| tree.read_with(cx, |tree, _| tree.data().connected.contains("ch"));
    db_expand(&tree, "warehouse", cx);

    let prompt = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.database_prompt::<FieldsPrompt>());
    wait_long(cx, "the password question", |cx| prompt(cx).is_some());
    let (refused, answered) = *seen.lock().expect("the counts are readable");
    assert!(refused > 0 && answered == 0, "the server refused the connection without a password");
    assert!(!connected(cx));

    let field = prompt(cx).and_then(|prompt| prompt.read_with(cx, |prompt, _| prompt.secret_input()));
    let field = field.expect("the question has a masked field");
    cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
    cx.simulate_input(PASSWORD);
    cx.simulate_keystrokes("enter");
    let listed = |cx: &mut VisualTestContext| {
        tree.read_with(cx, |tree, _| tree.rows().iter().any(|row| row.label == SCHEMA && !row.loading))
    };
    wait_long(cx, "the database the server lists", |cx| listed(cx) && connected(cx));
    assert!(prompt(cx).is_none(), "the question closed");
    assert!(seen.lock().expect("the counts are readable").1 > 0, "the server took the password");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
