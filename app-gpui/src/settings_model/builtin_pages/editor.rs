//! Keymap and the Editor pages: General and its sub-pages, Code Editing, Font, Color Scheme and its
//! scope pages, Code Style, File Encodings, File Types and Inlay Hints.
//!
//! A checkbox's dependent options follow it in the same section; a control drawn after a
//! checkbox's label ("Show the documentation popup in [500] ms") is an option of its own with an
//! empty label, so the search indexes only the checkbox's words; a radio group is a choice.

use super::{EDITOR_GROUP, custom, custom_owned, number, select, text, toggle};
use crate::settings_model::page_spec::{OptionSpec, PageSpec};
use crate::settings_model::pages::SettingsPage;
use crate::theme::color_scheme::attributes;
use crate::theme::themes::BUILTIN_THEMES;

/// The scope Code Style and its pages share: the scheme.
pub const CODE_STYLE_SCOPE: &str = "editor.codeStyle";

/// The Color Scheme pages of a language, after General, Language Defaults and Debugger: (id,
/// title), in the order the attribute tree first names each language.
pub const COLOR_SCHEME_LANGUAGES: [(&str, &str); 7] = [
    ("editor.colorScheme.python", "Python"),
    ("editor.colorScheme.markdown", "Markdown"),
    ("editor.colorScheme.regexp", "RegExp"),
    ("editor.colorScheme.go", "Go"),
    ("editor.colorScheme.java", "Java"),
    ("editor.colorScheme.kotlin", "Kotlin"),
    ("editor.colorScheme.php", "PHP"),
];

/// The roots of the attribute tree that are not a language.
pub const COLOR_SCHEME_SCOPES: [&str; 3] = ["General", "Language Defaults", "Debugger"];

/// Languages without a Code Style page.
const NO_CODE_STYLE: [&str; 5] = ["plaintext", "log", "diff", "ignore", "gitattributes"];

/// The encodings a file can be read and created in.
const ENCODINGS: [(&str, &str); 11] = [
    ("utf-8", "UTF-8"),
    ("utf-16le", "UTF-16LE"),
    ("utf-16be", "UTF-16BE"),
    ("gbk", "GBK"),
    ("gb18030", "GB18030"),
    ("big5", "Big5"),
    ("shift_jis", "Shift_JIS"),
    ("euc-kr", "EUC-KR"),
    ("windows-1252", "windows-1252"),
    ("iso-8859-1", "ISO-8859-1"),
    ("koi8-r", "KOI8-R"),
];

const TOP_BOTTOM: [(&str, &str); 2] = [("top", "Top"), ("bottom", "Bottom")];

/// An Editor page; it edits the global settings unless it says otherwise.
fn editor_page(id: &'static str, title: &'static str, order: f64) -> PageSpec {
    PageSpec::new(id, title, order).group(EDITOR_GROUP)
}

/// A page of the Code Style scheme: one scope for all of them, picked on the page.
fn scheme_page(spec: PageSpec) -> PageSpec {
    PageSpec { scope_group: Some(CODE_STYLE_SCOPE), ..spec.project_scope().own_scope_switch() }
}

pub(super) fn keymap() -> PageSpec {
    PageSpec::new("keymap", "Keymap", 10)
        .keywords(&["shortcut", "keyboard", "hotkey", "binding", "preset", "vs code", "intellij"])
}

pub(super) fn file_types() -> PageSpec {
    editor_page("editor.files", "File Types", 29.0)
        .project_scope()
        .keywords(&[
            "ignore",
            "ignored",
            "excluded",
            "glob",
            "exclude patterns",
            "associate",
            "association",
            "extension",
        ])
        .section(
            Some("Recognized File Types"),
            vec![custom("File name patterns", &["Hashbang patterns", "file type", "language"])],
        )
        .section(
            Some("Ignored Files and Folders"),
            vec![custom("Ignore files and folders", &[]), custom("Exclude patterns", &[])],
        )
}

fn general() -> PageSpec {
    editor_page("editor.general", "General", 20.0)
        .project_scope()
        .keywords(&["mouse", "scroll", "wrap", "trailing spaces", "save"])
        .section(
            Some("Markdown Images"),
            vec![
                text("editor.general.markdownImageFolder", "Image folder"),
                text("editor.general.markdownImageNamePattern", "Image name pattern"),
            ],
        )
        .section(
            Some("Mouse Control"),
            vec![
                toggle("editor.general.wheelZoom", "Change font size with Command+Mouse Wheel"),
                toggle("editor.general.dragAndDrop", "Enable drag-and-drop functionality in editor"),
            ],
        )
        .section(
            Some("Scrolling"),
            vec![select(
                "editor.general.caretScroll",
                "When caret is moved",
                &[
                    ("center", "Prefer scrolling editor canvas to keep caret line centered"),
                    ("minimize", "Prefer moving caret line to minimize editor scrolling"),
                ],
            )],
        )
        .section(
            Some("Soft Wraps"),
            vec![
                toggle("editor.softWrap", "Soft-wrap files"),
                text("editor.general.softWrapFiles", "File patterns"),
                toggle("editor.general.softWrapIndent", "Use original line's indent for wrapped parts"),
            ],
        )
        .section(
            Some("Virtual Space"),
            vec![toggle("editor.general.scrollPastEnd", "Show virtual space at file bottom")],
        )
        .section(
            Some("Save Files"),
            vec![
                toggle("files.trimTrailingWhitespace", "Remove trailing spaces on:"),
                select(
                    "editor.general.stripTrailingScope",
                    "",
                    &[("modified", "Modified lines"), ("all", "All lines")],
                ),
                toggle("editor.general.keepTrailingOnCaretLine", "Keep trailing spaces on caret line"),
                toggle(
                    "editor.general.removeTrailingBlankLines",
                    "Remove trailing blank lines at the end of saved files",
                ),
                toggle("files.ensureFinalNewline", "Ensure every saved file ends with a line break"),
                toggle("editor.formatOnSave", "Reformat code on save"),
            ],
        )
}

fn auto_import() -> PageSpec {
    editor_page("editor.general.autoImport", "Auto Import", 0.0)
        .keywords(&["import", "organize imports", "unambiguous", "paste"])
        .section(
            None,
            vec![
                select(
                    "editor.autoImport.importsOnPaste",
                    "Insert imports on paste",
                    &[("always", "Always"), ("ask", "Ask"), ("never", "Never")],
                ),
                toggle("editor.autoImport.addUnambiguous", "Add unambiguous imports on the fly"),
                toggle("editor.autoImport.optimizeOnTheFly", "Optimize imports on the fly"),
            ],
        )
}

fn appearance() -> PageSpec {
    editor_page("editor.general.appearance", "Appearance", 1.0)
        .keywords(&["caret", "line numbers", "whitespace", "indent guides", "distraction free", "zen"])
        .section(
            None,
            vec![
                toggle("editor.appearance.caretBlinking", "Caret blinking (ms):"),
                number("editor.appearance.caretBlinkPeriod", "", 100.0, 2000.0, None),
                toggle("editor.appearance.smoothCaretBlinking", "Use smooth caret blinking"),
                toggle("editor.appearance.blockCaret", "Use block caret"),
                toggle("editor.appearance.smoothCaretMovement", "Use smooth caret movement"),
                select("editor.appearance.caretMovementStyle", "", &[("snappy", "Snappy"), ("gliding", "Gliding")]),
                toggle(
                    "editor.appearance.hardWrapGuides",
                    "Show hard wrap and visual guides (configured in Code Style options)",
                ),
                toggle("editor.appearance.lineNumbers", "Show line numbers"),
                toggle("editor.appearance.methodSeparators", "Show method separators"),
                toggle("editor.showWhitespace", "Show whitespaces"),
                toggle("editor.appearance.whitespaceLeading", "Leading"),
                toggle("editor.appearance.whitespaceInner", "Inner"),
                toggle("editor.appearance.whitespaceTrailing", "Trailing"),
                toggle("editor.appearance.indentGuides", "Show indent guides"),
                toggle("editor.appearance.intentionBulb", "Show intention bulb"),
                number(
                    "editor.appearance.distractionFreeWidth",
                    "Distraction free mode text width:",
                    20.0,
                    500.0,
                    Some("chars"),
                ),
            ],
        )
}

fn breadcrumbs() -> PageSpec {
    editor_page("editor.general.breadcrumbs", "Breadcrumbs", 2.0).section(
        None,
        vec![
            toggle("editor.breadcrumbs.show", "Show breadcrumbs:"),
            select("editor.breadcrumbs.placement", "", &TOP_BOTTOM),
            custom("Show breadcrumbs for:", &["languages"]),
        ],
    )
}

fn code_completion() -> PageSpec {
    editor_page("editor.general.codeCompletion", "Code Completion", 3.0)
        .keywords(&["autocomplete", "suggestions", "case sensitive", "parameter info", "popup", "sort", "parentheses"])
        .section(
            None,
            vec![
                toggle("editor.completion.matchCase", "Match case:"),
                select(
                    "editor.completion.caseSensitivity",
                    "",
                    &[("first", "First letter only"), ("all", "All letters")],
                ),
                toggle("editor.completion.autoInsertSingle", "Automatically insert single suggestions"),
                toggle("editor.completion.sortAlphabetically", "Sort suggestions alphabetically"),
                toggle("editor.completion.autoPopup", "Show suggestions as you type"),
                toggle(
                    "editor.completion.insertOnSpecialKeys",
                    "Insert selected suggestion by pressing space, dot, or other context-dependent keys",
                ),
                toggle("editor.completion.showDocumentation", "Show the documentation popup in"),
                number("editor.completion.documentationDelay", "", 0.0, 5000.0, Some("ms")),
                toggle("editor.completion.insertParentheses", "Insert parentheses automatically when applicable"),
            ],
        )
        .section(
            Some("Parameter Info"),
            vec![
                toggle("editor.completion.parameterInfo", "Show the parameter info popup in"),
                number("editor.completion.parameterInfoDelay", "", 0.0, 5000.0, Some("ms")),
            ],
        )
}

fn code_folding() -> PageSpec {
    editor_page("editor.general.codeFolding", "Code Folding", 4.0)
        .keywords(&["fold", "collapse"])
        .section(None, vec![toggle("editor.folding.showOutline", "Show code folding outline")])
        .section(
            Some("Fold by default:"),
            vec![
                toggle("editor.folding.fileHeader", "File header"),
                toggle("editor.folding.imports", "Imports"),
                toggle("editor.folding.docComments", "Documentation comments"),
            ],
        )
}

fn editor_tabs() -> PageSpec {
    editor_page("editor.general.editorTabs", "Editor Tabs", 5.0)
        .keywords(&["tab limit", "preview tab", "close button", "pinned", "squeeze", "rows"])
        .section(
            Some("Appearance"),
            vec![
                select(
                    "editor.tabs.placement",
                    "Tab placement",
                    &[("top", "Top"), ("bottom", "Bottom"), ("left", "Left"), ("right", "Right"), ("none", "None")],
                ),
                custom("Show tabs in:", &["One row", "Scroll the tabs panel", "Squeeze tabs", "Multiple rows"]),
                toggle("editor.tabs.pinnedSeparateRow", "Show pinned tabs in a separate row"),
                toggle("editor.tabs.showFileIcon", "Show file icon"),
                toggle("editor.tabs.showExtension", "Show file extension"),
                toggle("editor.tabs.showDirectoryForNonUnique", "Show directory for non-unique file names"),
                toggle("editor.tabs.markModified", "Mark modified (*)"),
                toggle("editor.tabs.showFullPathOnHover", "Show full path on mouse hover"),
                custom("Close button position:", &["Right", "Left", "None"]),
            ],
        )
        .section(
            Some("Tab Order"),
            vec![
                toggle("editor.tabs.sortAlphabetically", "Sort tabs alphabetically"),
                toggle("editor.tabs.openAtEnd", "Open new tabs at the end"),
            ],
        )
        .section(Some("Opening Policy"), vec![toggle("editor.tabs.previewTab", "Enable preview tab")])
        .section(
            Some("Closing Policy"),
            vec![
                number("editor.tabs.tabLimit", "Tab limit", 1.0, 100.0, None),
                select(
                    "editor.tabs.closingPolicy",
                    "When the tab limit is reached",
                    &[("unchanged", "Close non-modified files first"), ("lru", "Close least recently used files")],
                ),
                select(
                    "editor.tabs.activateOnClose",
                    "When the current tab is closed, activate",
                    &[
                        ("left", "The tab on the left"),
                        ("right", "The tab on the right"),
                        ("mru", "Most recently opened tab"),
                    ],
                ),
            ],
        )
}

fn gutter_icons() -> PageSpec {
    editor_page("editor.general.gutterIcons", "Gutter Icons", 6.0).section(
        None,
        vec![
            toggle("editor.showGutterIcons", "Show gutter icons"),
            custom("Run", &["run configurations", "test"]),
            custom("Bookmarks", &[]),
            custom("Implemented and overridden", &["implementations", "subclasses", "inheritors"]),
            custom("Implementing and overriding", &["super method", "overrides", "implements"]),
            custom("Color preview", &["color", "swatch", "colour"]),
        ],
    )
}

fn inline_completion() -> PageSpec {
    editor_page("editor.general.inlineCompletion", "Inline Completion", 7.0)
        .keywords(&["ghost text", "inline suggestions", "full line completion", "copilot"])
        .section(
            None,
            vec![
                toggle("editor.inlineCompletion.enabled", "Enable inline completion"),
                toggle("editor.inlineCompletion.onlyOnPause", "Show only when typing pauses for"),
                number("editor.inlineCompletion.pauseMs", "", 0.0, 3000.0, Some("ms")),
            ],
        )
        .section(Some("Languages"), vec![custom("Show inline completion for:", &[])])
}

fn rich_text_copy() -> PageSpec {
    editor_page("editor.general.richTextCopy", "Rich-Text Copy", 8.0)
        .section(None, vec![toggle("editor.general.richTextCopy", "Copy as rich text by default")])
}

fn smart_keys() -> PageSpec {
    editor_page("editor.general.smartKeys", "Smart Keys", 9.0)
        .keywords(&["brackets", "quotes", "home", "camel humps", "paste", "indent"])
        .section(
            None,
            vec![
                toggle("editor.smartKeys.smartHome", "Home moves caret to first non-whitespace character"),
                toggle("editor.smartKeys.camelHumps", "Use \"CamelHumps\" words"),
                toggle("editor.smartKeys.pairBrackets", "Insert paired brackets (), [], {}"),
                toggle("editor.smartKeys.pairQuotes", "Insert pair quote"),
                toggle("editor.smartKeys.surroundSelection", "Surround selection on typing quote or brace"),
                toggle("editor.smartKeys.tabOutOfPair", "Jump outside closing bracket/quote with Tab"),
                select(
                    "editor.smartKeys.reformatOnPaste",
                    "Reformat on paste",
                    &[("none", "None"), ("indentBlock", "Indent Block"), ("indentEachLine", "Indent Each Line")],
                ),
                toggle("editor.smartKeys.escapeOnPaste", "Escape text on paste in string literals"),
            ],
        )
        .section(
            Some("Enter"),
            vec![
                toggle("editor.smartKeys.smartIndent", "Smart indent"),
                toggle("editor.smartKeys.insertDocStub", "Insert documentation comment stub"),
            ],
        )
        .section(
            Some("Backspace"),
            vec![select(
                "editor.smartKeys.backspaceUnindent",
                "Unindent",
                &[
                    ("autoindent", "To proper indent position"),
                    ("nearest", "To nearest indent position"),
                    ("none", "Disabled"),
                ],
            )],
        )
}

fn sticky_lines() -> PageSpec {
    editor_page("editor.general.stickyLines", "Sticky Lines", 10.0).keywords(&["sticky scroll"]).section(
        None,
        vec![
            toggle("editor.stickyScroll", "Show sticky lines while scrolling"),
            number("editor.codeEditing.stickyLinesMax", "Maximum number of lines", 1.0, 20.0, None),
        ],
    )
}

fn code_editing() -> PageSpec {
    editor_page("editor.codeEditing", "Code Editing", 21.0)
        .keywords(&["highlight", "quick documentation", "hover", "error"])
        .section(
            Some("Highlight on Caret Movement"),
            vec![
                toggle("editor.codeEditing.highlightUsages", "Usages of element at caret"),
                toggle("editor.codeEditing.highlightBraces", "Matched brace"),
                toggle("editor.codeEditing.highlightSelection", "Occurrences of selected text"),
            ],
        )
        .section(
            Some("Quick Documentation"),
            vec![
                toggle("editor.codeEditing.quickDocOnHover", "Show quick documentation on hover"),
                number("editor.codeEditing.quickDocDelay", "Tooltip delay", 100.0, 5000.0, Some("ms")),
            ],
        )
        .section(
            Some("Error Highlighting"),
            vec![toggle(
                "editor.codeEditing.nextErrorErrorsOnly",
                "Next error action goes to high priority problems only",
            )],
        )
}

fn inlay_hints() -> PageSpec {
    editor_page("editor.inlayHints", "Inlay Hints", 33.0)
        .keywords(&["hints", "code vision", "usages", "parameter hints", "type hints"])
        .section(
            None,
            vec![
                toggle("editor.inlayHints", "Show inlay hints"),
                toggle("editor.inlay.types", "Types"),
                toggle("editor.inlay.parameters", "Parameter names"),
                toggle("editor.inlay.other", "Other hints (chaining, closing labels…)"),
            ],
        )
        .section(
            Some("Code Vision"),
            vec![
                toggle("editor.inlay.codeVisionUsages", "Usages"),
                toggle("editor.inlay.codeVisionAuthor", "Code author"),
            ],
        )
        .section(Some("Languages"), vec![custom("Show inlay hints for:", &[])])
}

fn font() -> PageSpec {
    editor_page("editor.font", "Font", 22.0)
        .project_scope()
        .keywords(&["typeface", "monospace", "ligatures", "font size", "line spacing", "font weight", "bold"])
        .section(
            None,
            vec![
                custom("Font", &["font family", "Show only monospaced fonts"]),
                custom("Size", &["Line height"]),
                toggle("editor.ligatures", "Enable ligatures"),
            ],
        )
        .section(
            Some("Typography Settings"),
            vec![custom("Main font weight", &["Bold font weight"]), custom("Fallback font", &[])],
        )
}

/// The names of the attributes under the given roots of the attribute tree.
fn attribute_names(roots: &[&str]) -> Vec<&'static str> {
    let roots_of = |path: &[String]| path.first().is_some_and(|root| roots.contains(&root.as_str()));
    attributes().iter().filter(|attribute| roots_of(&attribute.path)).map(|attribute| attribute.name.as_str()).collect()
}

/// The scheme chooser, its import / export and the preview: the same on every Color Scheme page,
/// with the attributes of the page's roots.
fn scheme_editor(roots: &[&str]) -> OptionSpec {
    custom_owned("Scheme", attribute_names(roots))
}

fn color_scheme_pages() -> Vec<PageSpec> {
    let schemes: Vec<&'static str> = BUILTIN_THEMES.iter().map(|theme| theme.label).collect();
    let mut keywords = vec!["colors", "syntax highlighting", "theme", "foreground", "background", "bold", "italic"];
    keywords.extend(&schemes);
    let landing = editor_page("editor.colorScheme", "Color Scheme", 23.0)
        .keywords(&keywords)
        .section(None, vec![custom_owned("Scheme", schemes)]);
    let general = editor_page("editor.colorScheme.general", "General", 1.0)
        .keywords(&attribute_names(&["General"]))
        .section(None, vec![scheme_editor(&["General"])]);
    let defaults = editor_page("editor.colorScheme.language-defaults", "Language Defaults", 2.0)
        .keywords(&["semantic highlighting", "rainbow"])
        .section(None, vec![scheme_editor(&["Language Defaults"])])
        .section(
            Some("Language Defaults"),
            vec![
                toggle("editor.semanticHighlighting", "Semantic highlighting"),
                toggle("editor.rainbowBrackets", "Rainbow brackets"),
            ],
        );
    let debugger = editor_page("editor.colorScheme.debugger", "Debugger", 3.0)
        .keywords(&["breakpoint", "inline values"])
        .section(None, vec![scheme_editor(&["Debugger"])]);
    let languages = COLOR_SCHEME_LANGUAGES.iter().enumerate().map(|(index, &(id, language))| {
        editor_page(id, language, 4.0 + index as f64)
            .keywords(&attribute_names(&[language]))
            .section(None, vec![scheme_editor(&[language])])
    });
    [landing, general, defaults, debugger].into_iter().chain(languages).collect()
}

fn code_style() -> PageSpec {
    let page = editor_page("editor.codeStyle", "Code Style", 24.0)
        .keywords(&[
            "indent",
            "indentation",
            "tabs",
            "line separator",
            "editorconfig",
            "right margin",
            "scheme",
            "formatter",
        ])
        .section(None, vec![custom("Scheme", &[])])
        .section(
            None,
            vec![
                select(
                    "editor.codeStyle.lineSeparator",
                    "Line separator",
                    &[
                        ("system", "System-Dependent"),
                        ("lf", "Unix and macOS (\\n)"),
                        ("crlf", "Windows (\\r\\n)"),
                        ("cr", "Classic Mac OS (\\r)"),
                    ],
                ),
                number("editor.codeStyle.hardWrapAt", "Hard wrap at", 1.0, 999.0, Some("columns")),
                custom("Visual guides", &[]),
            ],
        )
        .section(
            Some("Indents Detection"),
            vec![toggle("editor.codeStyle.detectIndents", "Detect and use existing file indents for editing")],
        )
        .section(Some("EditorConfig"), vec![toggle("editor.codeStyle.editorconfig", "Enable EditorConfig support")])
        .section(
            Some("Formatter"),
            vec![
                custom("Do not format", &["glob", "exclude from formatting"]),
                toggle("editor.codeStyle.formatterTagsEnabled", "Turn formatter on/off with markers in code comments"),
                text("editor.codeStyle.formatterOffTag", "Off"),
                text("editor.codeStyle.formatterOnTag", "On"),
                toggle("editor.codeStyle.formatterTagsRegexp", "Enable regular expressions in formatter markers"),
            ],
        );
    scheme_page(page)
}

fn other_file_types() -> PageSpec {
    let page = editor_page("editor.codeStyle.other", "Other File Types", 1000.0)
        .keywords(&["tab size", "indent"])
        .section(None, vec![custom("Scheme", &[])])
        .section(
            Some("Tabs and Indents"),
            vec![custom("Use tab character", &[]), number("editor.tabSize", "Tab size", 1.0, 16.0, None)],
        );
    scheme_page(page)
}

fn file_encodings() -> PageSpec {
    editor_page("editor.fileEncodings", "File Encodings", 27.0)
        .project_scope()
        .keywords(&["encoding", "charset", "utf-8", "bom", "native-to-ascii", "properties"])
        .section(
            None,
            vec![
                select("editor.encodings.global", "Global Encoding", &ENCODINGS),
                select("editor.encodings.project", "Project Encoding", &ENCODINGS),
            ],
        )
        .section(
            Some("Properties Files (*.properties)"),
            vec![
                select("editor.encodings.properties", "Default encoding for properties files", &ENCODINGS),
                toggle("editor.encodings.nativeToAscii", "Transparent native-to-ascii conversion"),
            ],
        )
        .section(
            None,
            vec![select(
                "editor.encodings.utf8Bom",
                "Create UTF-8 files",
                &[
                    ("never", "with NO BOM"),
                    ("always", "with BOM"),
                    ("windows", "with BOM under Windows, with no BOM otherwise"),
                ],
            )],
        )
}

/// The Editor pages in the order they are registered, Inspections and Natural Languages
/// excepted.
pub(super) fn pages() -> Vec<PageSpec> {
    let general = vec![
        general(),
        auto_import(),
        appearance(),
        breadcrumbs(),
        code_completion(),
        code_folding(),
        editor_tabs(),
        gutter_icons(),
        inline_completion(),
        rich_text_copy(),
        smart_keys(),
        sticky_lines(),
        code_editing(),
        inlay_hints(),
        font(),
    ];
    let code_style = [code_style(), other_file_types(), file_encodings()];
    general.into_iter().chain(color_scheme_pages()).chain(code_style).collect()
}

/// Editor › Code Style › one page per language of the backend's language list (`(id, name)`),
/// placed before Other File Types. The ids are known only at run time, so these are
/// [`SettingsPage`]s; they share the Code Style scheme's scope.
pub fn code_style_language_pages(languages: &[(&str, &str)]) -> Vec<SettingsPage> {
    const KEYWORDS: [&str; 8] = [
        "Scheme",
        "Tabs and Indents",
        "Tab size",
        "Indent",
        "Use tab character",
        "Hard wrap at",
        "Comment Code",
        "Line comment at first column",
    ];
    const JAVA_KEYWORDS: [&str; 4] =
        ["Formatting profile", "IntelliJ IDEA", "Custom Eclipse formatter XML", "Eclipse formatter XML"];
    let styled = languages.iter().filter(|(id, _)| !NO_CODE_STYLE.contains(id));
    styled
        .map(|&(id, name)| {
            let java: &[&str] = if id == "java" { &JAVA_KEYWORDS } else { &[] };
            let keywords: Vec<&str> = KEYWORDS.iter().chain(java).copied().collect();
            let page = SettingsPage::new(&format!("{CODE_STYLE_SCOPE}.{id}"), name).group(EDITOR_GROUP).order(100);
            SettingsPage { project_scope: true, ..page.keywords(&keywords) }
        })
        .collect()
}
