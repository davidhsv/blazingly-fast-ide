//! Images in agent replies name host files (or web and inline images), never paths relative to the
//! page: a source is resolved to a URL to show or a host file to read.

use crate::agents::conversation::{decode_uri_component, has_scheme};
use crate::platform::paths::ipc_path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageTarget {
    /// A web or `data:image/` URL, shown as is.
    Url(String),
    /// A host file, read through the backend.
    Path(String),
}

fn has_control(text: &str) -> bool {
    text.chars().any(|c| (c as u32) < 32 || c as u32 == 127)
}

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// `C:/…` (or `C:\…` when `backslash`).
fn starts_with_drive(path: &str, backslash: bool) -> bool {
    matches!(path.as_bytes(), [letter, b':', sep, ..]
        if letter.is_ascii_alphabetic() && (*sep == b'/' || (backslash && *sep == b'\\')))
}

/// Whether an `http(s)://` URL has a host a URL parser accepts.
fn web_url_is_valid(url: &str) -> bool {
    let Some((_, rest)) = url.split_once("://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !host.is_empty() && !host.contains(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '\\' | '^' | '|'))
}

/// The file a `file:` URL names: `file:///tmp/x`, `file://localhost/tmp/x`, `file:///C:/x`,
/// `file://server/share/x` (a network share).
fn file_url_path(url: &str) -> Option<String> {
    let rest = &url[5..];
    let (host, pathname) = match rest.strip_prefix("//") {
        Some(authority) => match authority.find('/') {
            Some(slash) => (&authority[..slash], &authority[slash..]),
            None => (authority, "/"),
        },
        None => ("", rest),
    };
    let pathname = pathname.split(['?', '#']).next().unwrap_or_default();
    let mut path = decode_uri_component(pathname)?;
    if path.len() > 1 && starts_with_drive(&path[1..], false) {
        path.remove(0);
    }
    let host = host.to_lowercase();
    if !host.is_empty() && host != "localhost" {
        path = format!("//{host}{path}");
    }
    Some(path)
}

/// Where the image `source` of a reply comes from, resolved against the task's `cwd`; `None` for
/// empty, malformed or unsupported sources (`javascript:`, `ftp:`, `data:text/…`, bad escapes).
pub fn image_target(source: &str, cwd: &str) -> Option<ImageTarget> {
    let src = source.trim();
    if src.is_empty() || has_control(src) {
        return None;
    }
    if starts_with_ignore_case(src, "data:image/") {
        return Some(ImageTarget::Url(src.to_owned()));
    }
    if starts_with_ignore_case(src, "http://") || starts_with_ignore_case(src, "https://") || src.starts_with("//") {
        let url = if src.starts_with("//") { format!("https:{src}") } else { src.to_owned() };
        return web_url_is_valid(&url).then_some(ImageTarget::Url(url));
    }
    let mut path = if starts_with_ignore_case(src, "file:") {
        file_url_path(src)?
    } else {
        if has_scheme(src) && !starts_with_drive(src, true) {
            return None;
        }
        let before_query = src.split(['?', '#']).next().unwrap_or_default();
        ipc_path(&decode_uri_component(before_query)?)
    };
    if path.is_empty() || has_control(&path) {
        return None;
    }
    if path.starts_with('/') || starts_with_drive(&path, false) {
        return Some(ImageTarget::Path(path));
    }
    let base = ipc_path(cwd);
    let base = base.strip_suffix('/').unwrap_or(&base);
    if starts_with_drive(base, false) || base.starts_with("//") {
        path = path.replace('\\', "/");
    }
    Some(ImageTarget::Path(format!("{base}/{path}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_host_image_paths() {
        for (src, cwd, path) in [
            ("images/code.png", "/repo", "/repo/images/code.png"),
            ("../code.png?v=2#preview", "/repo", "/repo/../code.png"),
            ("/tmp/截图%20one%23.png", "/repo", "/tmp/截图 one#.png"),
            ("file://localhost/tmp/code.png", "/repo", "/tmp/code.png"),
            ("C:/shots/code.png", "/repo", "C:/shots/code.png"),
            (r"C:\shots\code.png", "C:/repo", "C:/shots/code.png"),
            (r"images\code.png", r"C:\repo", "C:/repo/images/code.png"),
            (r"\\server\share\code.png", "C:/repo", "//server/share/code.png"),
            ("file://server/share/code.png", "C:/repo", "//server/share/code.png"),
            ("file:///C:/shots/code%20one.png", "C:/repo", "C:/shots/code one.png"),
        ] {
            assert_eq!(image_target(src, cwd), Some(ImageTarget::Path(path.to_owned())), "{src}");
        }
    }

    #[test]
    fn rejects_unsupported_or_malformed_sources() {
        for src in [
            "",
            "#section",
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "data:text/html,test",
            "ftp://example.com/x.png",
            "/tmp/%ZZ.png",
            "/tmp/code%00.png",
            "file:///tmp/%ZZ.png",
            "https://",
            "https://example.com/\ncode.png",
        ] {
            assert_eq!(image_target(src, "/repo"), None, "{src:?}");
        }
    }

    #[test]
    fn treats_protocol_relative_urls_as_https_rather_than_host_files() {
        assert_eq!(
            image_target("//example.com/code.png", "/repo"),
            Some(ImageTarget::Url("https://example.com/code.png".to_owned()))
        );
    }
}
