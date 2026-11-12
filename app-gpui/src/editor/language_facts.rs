//! What the editing commands need to know about a document's language and its code style: the
//! comment tokens of the language definition, the effective indentation (Code Style of the
//! language, else the language definition, else Editor › General, then `.editorconfig`) and the
//! Smart Keys settings.

use crate::editor_commands::comments::LineCommentStyle;
use crate::editor_commands::indent::fallback_indent;
use crate::editor_commands::paste::PasteReindent;
use crate::editor_commands::{CommentTokens, IndentContext, LanguageFacts};
use ide_core::LanguageId;
use ide_core::settings::Settings;
use ide_host::Host;
use ide_workspace::EditorConfigProps;
use ide_workspace::editorconfig::IndentStyle;

/// The facts and settings an editor's commands read, taken when the editor opens and again
/// whenever the settings change.
#[derive(Debug, Clone, PartialEq)]
pub struct EditorFacts {
    pub language: LanguageFacts,
    /// Columns between tab stops.
    pub tab_size: u32,
    /// One level of indentation: spaces, or a tab.
    pub indent_unit: String,
    pub comment_style: LineCommentStyle,
    /// ⌥← / ⌥→ and ⌥⌫ stop at the humps of CamelCase words.
    pub camel_humps: bool,
    /// Smart Keys › Backspace › Unindent.
    pub backspace_unindent: String,
    pub reformat_on_paste: PasteReindent,
    /// Smart Keys › Enter › Insert documentation comment stub.
    pub insert_doc_stub: bool,
}

impl Default for EditorFacts {
    fn default() -> Self {
        Self::from_settings(LanguageFacts::default(), None, &Settings::default(), None)
    }
}

impl EditorFacts {
    /// The facts of `language` in the window `window` of `host`, with the file's `.editorconfig`.
    pub fn read(host: &Host, window: &str, language: &LanguageId, editorconfig: Option<&EditorConfigProps>) -> Self {
        let config = host.languages.get(language);
        let mut facts = LanguageFacts::new(language.as_str());
        let mut defined_indent = None;
        if let Some(config) = &config {
            facts.comments = CommentTokens {
                line: config.line_comment.clone().filter(|token| !token.is_empty()),
                block: config.block_comment.clone(),
            };
            defined_indent = config.indent.map(|indent| (indent.tab_size, indent.insert_spaces));
        }
        let settings = ide_host::ops::project::settings(host, window);
        Self::from_settings(facts, defined_indent, &settings, editorconfig)
    }

    fn from_settings(
        language: LanguageFacts,
        defined_indent: Option<(u32, bool)>,
        settings: &Settings,
        editorconfig: Option<&EditorConfigProps>,
    ) -> Self {
        let editor = &settings.editor;
        let style = editor.code_style.languages.get(&language.id);
        let (fallback_tab, fallback_spaces) = defined_indent.unwrap_or((editor.tab_size, editor.insert_spaces));
        let mut tab_size = style.and_then(|style| style.tab_size).unwrap_or(fallback_tab);
        let mut spaces = style.and_then(|style| style.use_tab).map_or(fallback_spaces, |tabs| !tabs);
        let mut indent_size = style.and_then(|style| style.indent).filter(|size| *size != tab_size);
        if let Some(config) = editorconfig.filter(|_| editor.code_style.editorconfig) {
            tab_size = config.indent_size.or(config.tab_width).unwrap_or(tab_size);
            spaces = config.indent_style.map_or(spaces, |style| style == IndentStyle::Space);
            indent_size = None;
        }
        let tab_size = tab_size.max(1);
        let indent_unit =
            if spaces { " ".repeat(indent_size.unwrap_or(tab_size).max(1) as usize) } else { "\t".into() };
        let defaults = LineCommentStyle::default();
        let comment_style = LineCommentStyle {
            at_first_column: style
                .and_then(|style| style.line_comment_at_first_column)
                .unwrap_or(defaults.at_first_column),
            add_space: style.and_then(|style| style.line_comment_add_space).unwrap_or(defaults.add_space),
        };
        let keys = &editor.smart_keys;
        let reformat_on_paste = match keys.reformat_on_paste.as_str() {
            "none" => PasteReindent::None,
            "indentBlock" => PasteReindent::IndentBlock,
            _ => PasteReindent::IndentEachLine,
        };
        Self {
            language,
            tab_size,
            indent_unit,
            comment_style,
            camel_humps: keys.camel_humps,
            backspace_unindent: keys.backspace_unindent.clone(),
            reformat_on_paste,
            insert_doc_stub: keys.insert_doc_stub,
        }
    }

    /// The indentation of a line, as the language without a parser computes it.
    pub fn indent(&self, cx: &IndentContext<'_>) -> Option<u32> {
        fallback_indent(cx, &self.language.id, self.language.comments.line.as_deref())
    }

    /// The comment tokens in force anywhere in the document: the language's own.
    pub fn comments_at(&self, _: u32) -> Vec<CommentTokens> {
        vec![self.language.comments.clone()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::editor_settings::LanguageCodeStyle;

    fn facts(defined: Option<(u32, bool)>, settings: &Settings, config: Option<&EditorConfigProps>) -> EditorFacts {
        EditorFacts::from_settings(LanguageFacts::new("rust"), defined, settings, config)
    }

    #[test]
    fn indentation_comes_from_code_style_then_the_language_then_the_editor_then_editorconfig() {
        let mut settings = Settings::default();
        settings.editor.tab_size = 8;
        assert_eq!(facts(None, &settings, None).indent_unit, " ".repeat(8));
        assert_eq!(facts(Some((2, true)), &settings, None).indent_unit, "  ");
        assert_eq!(facts(Some((4, false)), &settings, None).indent_unit, "\t");
        let style =
            LanguageCodeStyle { tab_size: Some(4), indent: Some(2), use_tab: Some(false), ..Default::default() };
        settings.editor.code_style.languages.insert("rust".into(), style);
        let styled = facts(Some((8, false)), &settings, None);
        assert_eq!((styled.tab_size, styled.indent_unit.as_str()), (4, "  "));
        let config =
            EditorConfigProps { indent_style: Some(IndentStyle::Tab), tab_width: Some(3), ..Default::default() };
        let configured = facts(None, &settings, Some(&config));
        assert_eq!((configured.tab_size, configured.indent_unit.as_str()), (3, "\t"));
        settings.editor.code_style.editorconfig = false;
        assert_eq!(facts(None, &settings, Some(&config)).indent_unit, "  ");
    }
}
