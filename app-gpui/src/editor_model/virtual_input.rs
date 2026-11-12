//! Read-only virtual documents a language server provides (library classes from `jdt://`,
//! decompiled C# from `csharp:`): their tab input, and how an open request is routed before a
//! binary viewer is chosen.

use crate::editor::tabs::EditorInput;
use crate::platform::i18n::I18n;
use crate::platform::virtual_docs::{is_class_file, is_virtual_uri};
use ide_lsp::types::VirtualDocument;
use serde_json::json;

/// The tab key of a virtual document.
pub fn virtual_key(uri: &str) -> String {
    format!("virtual:{uri}")
}

/// A read-only virtual document's input: its `path` is the URI, which also identifies its
/// document; `data` holds what the tab and its notice show.
pub fn virtual_input(document: &VirtualDocument) -> EditorInput {
    EditorInput {
        kind: "virtual".to_owned(),
        key: virtual_key(&document.uri),
        path: Some(document.uri.clone()),
        data: Some(json!({
            "title": document.title,
            "description": document.description,
            "decompiled": document.decompiled,
        })),
    }
}

/// Inputs shown in the text editor: files and virtual documents.
pub fn is_text_input(input: &EditorInput) -> bool {
    matches!(input.kind.as_str(), "file" | "virtual") && input.path.is_some()
}

/// How an open request for a path goes on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenRoute {
    /// Fetch the server's contents and open them read-only ([`virtual_input`]).
    Virtual(String),
    /// A compiled class on disk: resolve it to its source or library class first
    /// (`lspResolveClassFile`), then route the answer again.
    ResolveClassFile(String),
    /// A file: a contributed viewer may claim it, else it opens as text.
    File(String),
}

pub fn open_route(path: &str) -> OpenRoute {
    if is_virtual_uri(path) {
        OpenRoute::Virtual(path.to_owned())
    } else if is_class_file(path) {
        OpenRoute::ResolveClassFile(path.to_owned())
    } else {
        OpenRoute::File(path.to_owned())
    }
}

/// The warning shown when a library class cannot be opened (the server has no contents, or the
/// class file cannot be resolved); no tab opens. Returns the title and body.
pub fn library_class_failure(error: &str, i18n: &I18n) -> (String, String) {
    (i18n.tx("Cannot open library class"), error.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const JDT: &str =
        "jdt://contents/lib-1.0.jar/org.example/Lib.class?=demo/%5C/m2%5C/lib-1.0.jar%3Corg.example%28Lib.class";

    fn document(uri: &str) -> VirtualDocument {
        VirtualDocument {
            uri: uri.to_owned(),
            text: "public class Lib {}\n".to_owned(),
            language_id: ide_core::LanguageId("java".to_owned()),
            title: "Lib.class".to_owned(),
            description: Some("/m2/lib-1.0.jar".to_owned()),
            decompiled: true,
        }
    }

    #[test]
    fn describes_a_library_class_as_a_text_input_keyed_by_its_uri() {
        let input = virtual_input(&document(JDT));
        assert_eq!(input.kind, "virtual");
        assert_eq!(input.key, format!("virtual:{JDT}"));
        assert_eq!(input.path.as_deref(), Some(JDT));
        let data = json!({ "title": "Lib.class", "description": "/m2/lib-1.0.jar", "decompiled": true });
        assert_eq!(input.data, Some(data));
        assert!(is_text_input(&input));
        let diff = EditorInput { kind: "diff".into(), key: "d".into(), path: None, data: None };
        assert!(!is_text_input(&diff));
    }

    #[test]
    fn routes_before_choosing_a_binary_viewer() {
        let cases = [
            ("/m2/lib.jar!/org/example/Lib.class", JDT, OpenRoute::Virtual(JDT.to_owned())),
            ("/p/target/classes/Lib.class", "/p/Lib.java", OpenRoute::File("/p/Lib.java".to_owned())),
            (
                "/p/build/classes/Lib.class",
                "file:///p/target/classes/Lib.class",
                OpenRoute::Virtual("file:///p/target/classes/Lib.class".to_owned()),
            ),
        ];
        for (path, target, routed) in cases {
            assert_eq!(open_route(path), OpenRoute::ResolveClassFile(path.to_owned()), "{path}");
            assert_eq!(open_route(target), routed, "{path}");
        }
    }

    #[test]
    fn does_not_open_a_tab_when_the_server_has_no_contents() {
        let i18n = I18n::default();
        for uri in ["jdt://contents/missing.jar/p/X.class", "csharp:/p/App.csproj/decompiled/Missing.cs"] {
            assert_eq!(open_route(uri), OpenRoute::Virtual(uri.to_owned()));
            let error = format!("no contents for {uri}");
            let (title, body) = library_class_failure(&error, &i18n);
            assert_eq!(title, "Cannot open library class");
            assert_eq!(body, error);
        }
    }
}
