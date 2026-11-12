//! `~/.openideavimrc` / `~/.ideavimrc`: the safe subset kept in an rc file — `set` options, `map`
//! variants, `let mapleader` — plus `:action <id>` / `<Action>(id)` mappings, which run IDE
//! actions. `source` reads another file by the same rules. Everything else is reported as
//! ignored, never executed.
//!
//! Reading the files is the caller's: [`SourcedFiles`] names the files `source` lines reach, one at
//! a time, and [`parse_vimrc`] takes their text.

use regex::{Captures, NoExpand, Regex};
use std::collections::HashMap;
use std::sync::LazyLock;

/// The mapping context; `None` is Normal, Visual and Operator-pending.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MapMode {
    Normal,
    Insert,
    Visual,
    OperatorPending,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetValue {
    Text(String),
    Flag(bool),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RcCommand {
    Set {
        name: String,
        value: SetValue,
    },
    Map {
        mode: Option<MapMode>,
        noremap: bool,
        lhs: String,
        rhs: String,
    },
    /// A key mapped to one Ex command, e.g. `<Action>(ReformatCode)` → `action ReformatCode`.
    MapEx {
        mode: Option<MapMode>,
        lhs: String,
        ex: String,
    },
}

/// A line that was not applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RcIgnored {
    /// The sourced file it came from, as written after `source`; `None` for the rc file itself.
    pub file: Option<String>,
    /// 1-based line number.
    pub line: usize,
    pub text: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RcParseResult {
    pub commands: Vec<RcCommand>,
    pub ignored: Vec<RcIgnored>,
}

pub const RC_FILES: [&str; 2] = [".openideavimrc", ".ideavimrc"];

macro_rules! re {
    ($name:ident, $pattern:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pattern).expect("valid regex"));
    };
}

re!(SOURCE, r"^so(?:urce)?\s+(.+)$");
re!(MAP, r"^(n|v|x|i|o)?(map|noremap)$");
re!(KEY_NOTATION, r"<([^<>\s]+)>");
re!(KEY_PARTS, r"(?i)^((?:[csamd]-)*)(.+)$");
re!(FUNCTION_KEY, r"^f(?:[1-9]|1[0-2])$");
re!(MAP_FLAG, r"(?i)^<(silent|unique|nowait|buffer|expr|script)>$");
re!(SPACE_LEADER, r"(?i)^\\?<space>$");
re!(ESCAPED_KEY, r"\\(<[^>]+>)");
re!(LET_LEADER, r"^let\s+(g:)?mapleader\s*(=.*)$");
re!(OPTION_VALUE, r"^([a-zA-Z]+)=(.*)$");
re!(OPTION_NAME, r"^[a-zA-Z]+$");
re!(LEADER, r"(?i)<leader>");
re!(ACTION_RHS, r"(?i)^<Action>\(([^)\s]+)\)$");
re!(EX_RHS, r"(?i)^:(?:<C-u>)?([^<]+)<(?:CR|Enter|Return)>$");

/// Special-key names are case-insensitive (`<esc>`, `<CR>`…) but the engine compares the exact
/// canonical spelling: canonical names by lower-cased name.
fn special_key(lower: &str) -> Option<&'static str> {
    Some(match lower {
        "esc" => "Esc",
        "return" | "enter" | "cr" => "CR",
        "bs" | "backspace" => "BS",
        "del" | "delete" => "Del",
        "ins" | "insert" => "Ins",
        "space" => "Space",
        "tab" => "Tab",
        "lt" => "lt",
        "bar" => "|",
        "bslash" => "\\",
        "left" => "Left",
        "right" => "Right",
        "up" => "Up",
        "down" => "Down",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        _ => return None,
    })
}

/// One `<…>` key notation in canonical spelling, or `None` to keep it as written.
fn canonical_key(inner: &str, rhs: bool) -> Option<String> {
    let caps = KEY_PARTS.captures(inner)?;
    let prefixes: Vec<String> =
        caps.get(1).map_or("", |m| m.as_str()).to_uppercase().split('-').map(str::to_owned).collect();
    let name = caps.get(2).map_or("", |m| m.as_str());
    let lower = name.to_lowercase();
    let has = |p: &str| prefixes.iter().any(|x| x == p);
    let single = |s: &str| s.encode_utf16().count() == 1;
    let mut canonical = if let Some(special) = special_key(&lower) {
        special.to_owned()
    } else if FUNCTION_KEY.is_match(&lower) {
        name.to_uppercase()
    } else if prefixes.len() > 1 && single(name) {
        name.to_owned()
    } else {
        return None;
    };
    if canonical == "lt" {
        canonical = if rhs { "<lt>".to_owned() } else { "<".to_owned() };
    }
    // M- aliases Alt; D- is Command (the engine's M-).
    let mods = format!(
        "{}{}{}",
        if has("C") { "C-" } else { "" },
        if has("A") || has("M") { "A-" } else { "" },
        if has("D") { "M-" } else { "" }
    );
    let shift = has("S");
    if shift && single(&canonical) {
        canonical = canonical.to_uppercase();
    }
    if mods.is_empty() && (single(&canonical) || canonical == "<lt>") {
        return Some(canonical);
    }
    Some(format!("<{mods}{}{canonical}>", if shift { "S-" } else { "" }))
}

/// Rewrites `<esc>`/`<cr>`/`<c-c>`… to the exact `<Esc>`/`<CR>`/`<C-c>` the engine matches.
fn normalize_vim_keys(keys: &str, rhs: bool) -> String {
    KEY_NOTATION
        .replace_all(keys, |caps: &Captures| {
            let whole = caps.get(0).map_or("", |m| m.as_str());
            caps.get(1).and_then(|inner| canonical_key(inner.as_str(), rhs)).unwrap_or_else(|| whole.to_owned())
        })
        .into_owned()
}

/// Options the engine implements (others are reported as ignored).
const SUPPORTED_OPTIONS: [&str; 6] = ["textwidth", "tw", "pcre", "insertModeEscKeysTimeout", "langmap", "lmap"];

fn supported(option: &str) -> bool {
    SUPPORTED_OPTIONS.contains(&option)
}

fn leader_keys(value: &str) -> String {
    if value == " " || SPACE_LEADER.is_match(value) { "<Space>".to_owned() } else { value.to_owned() }
}

/// `= ","` / `= "\<Space>"` / `= ' '`: the quoted value, or `None`.
fn parse_let_value(rest: &str) -> Option<String> {
    let after = rest.trim().strip_prefix('=')?.trim_start();
    let quote = after.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let value = after[1..].trim_end().strip_suffix(quote)?;
    if quote == '"' { Some(ESCAPED_KEY.replace_all(value, "$1").replace("\\\\", "\\")) } else { Some(value.to_owned()) }
}

fn split_args(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

/// `<Action>(id)` or `:action id<CR>` (also `:<C-u>action`) as an Ex command line.
pub fn ex_of_rhs(rhs: &str) -> Option<String> {
    if let Some(action) = ACTION_RHS.captures(rhs).and_then(|c| c.get(1)) {
        return Some(format!("action {}", action.as_str()));
    }
    EX_RHS.captures(rhs).and_then(|c| c.get(1)).map(|m| m.as_str().trim().to_owned())
}

/// What `source` lines name, for reading them ahead of the parse.
pub fn sourced_paths(text: &str) -> Vec<String> {
    text.split('\n')
        .filter_map(|raw| SOURCE.captures(raw.trim()).and_then(|c| c.get(1)).map(|m| m.as_str().trim().to_owned()))
        .collect()
}

/// The text of sourced files by the path written in the rc file; `None` is unreadable.
pub type Sources = HashMap<String, Option<String>>;

struct RcLine {
    file: Option<String>,
    line: usize,
    text: String,
}

/// The rc file's lines with each readable `source` line replaced by that file's lines.
fn expand_sources(
    text: &str,
    sources: Option<&Sources>,
    ignored: &mut Vec<RcIgnored>,
    file: Option<&str>,
    active: &[String],
) -> Vec<RcLine> {
    let mut out = Vec::new();
    for (i, raw) in text.split('\n').enumerate() {
        let line = raw.trim();
        let Some(path) = SOURCE.captures(line).and_then(|c| c.get(1)).map(|m| m.as_str().trim()) else {
            out.push(RcLine { file: file.map(str::to_owned), line: i + 1, text: line.to_owned() });
            continue;
        };
        let sourced = sources.and_then(|s| s.get(path)).and_then(Option::as_deref);
        let reason = if active.iter().any(|a| a == path) {
            Some("file sources itself")
        } else if sourced.is_none() {
            Some("cannot read file")
        } else {
            None
        };
        match (reason, sourced) {
            (None, Some(sourced)) => {
                let mut nested = active.to_vec();
                nested.push(path.to_owned());
                out.extend(expand_sources(sourced, sources, ignored, Some(path), &nested));
            }
            (reason, _) => ignored.push(RcIgnored {
                file: file.map(str::to_owned),
                line: i + 1,
                text: line.to_owned(),
                reason: reason.unwrap_or("cannot read file").to_owned(),
            }),
        }
    }
    out
}

fn map_mode(prefix: &str) -> Option<MapMode> {
    match prefix {
        "n" => Some(MapMode::Normal),
        "v" | "x" => Some(MapMode::Visual),
        "i" => Some(MapMode::Insert),
        "o" => Some(MapMode::OperatorPending),
        _ => None,
    }
}

/// Parses an rc file; `sources` holds the text of sourced files by the path written in it.
pub fn parse_vimrc(text: &str, sources: Option<&Sources>) -> RcParseResult {
    let mut commands = Vec::new();
    let mut ignored = Vec::new();
    let mut leader = "\\".to_owned();
    let lines = expand_sources(text, sources, &mut ignored, None, &[]);
    for RcLine { file, line: number, text: line } in lines {
        if line.is_empty() || line.starts_with('"') {
            continue;
        }
        let mut ignore = |reason: String| {
            ignored.push(RcIgnored { file: file.clone(), line: number, text: line.clone(), reason });
        };
        let words = split_args(&line);
        let word = words.first().copied().unwrap_or_default();
        let rest = line.find(word).map_or("", |at| &line[at + word.len()..]);

        if word == "set" || word == "se" {
            if words.len() < 2 {
                ignore("set without options".to_owned());
                continue;
            }
            for opt in &words[1..] {
                let cmd = if let Some(eq) = OPTION_VALUE.captures(opt) {
                    let name = eq.get(1).map_or("", |m| m.as_str()).to_owned();
                    Some((name, SetValue::Text(eq.get(2).map_or("", |m| m.as_str()).to_owned())))
                } else if OPTION_NAME.is_match(opt) {
                    match opt.strip_prefix("no") {
                        Some(name) if !supported(opt) && supported(name) => {
                            Some((name.to_owned(), SetValue::Flag(false)))
                        }
                        _ => Some(((*opt).to_owned(), SetValue::Flag(true))),
                    }
                } else {
                    None
                };
                match cmd {
                    None => ignore(format!("unsupported option syntax: {opt}")),
                    Some((name, _)) if !supported(&name) => ignore(format!("option not supported: {name}")),
                    Some((name, value)) => commands.push(RcCommand::Set { name, value }),
                }
            }
            continue;
        }

        if word == "let" {
            let value = LET_LEADER.captures(&line).and_then(|c| c.get(2)).and_then(|m| parse_let_value(m.as_str()));
            match value.filter(|v| !v.is_empty()) {
                Some(value) => leader = leader_keys(&value),
                None => ignore("only `let mapleader` is supported".to_owned()),
            }
            continue;
        }

        if let Some(map) = MAP.captures(word) {
            let args: Vec<&str> = split_args(rest).into_iter().filter(|a| !MAP_FLAG.is_match(a)).collect();
            if args.len() < 2 {
                ignore("mapping needs a left and right side".to_owned());
                continue;
            }
            let with_leader = |keys: &str| LEADER.replace_all(keys, NoExpand(&leader)).into_owned();
            let lhs = normalize_vim_keys(&with_leader(args[0]), false);
            let raw_rhs = with_leader(&args[1..].join(" "));
            let mode = map_mode(map.get(1).map_or("", |m| m.as_str()));
            match ex_of_rhs(&raw_rhs) {
                Some(ex) => commands.push(RcCommand::MapEx { mode, lhs, ex }),
                None => commands.push(RcCommand::Map {
                    mode,
                    noremap: map.get(2).is_some_and(|m| m.as_str() != "map"),
                    lhs,
                    rhs: normalize_vim_keys(&raw_rhs, true),
                }),
            }
            continue;
        }

        ignore(format!("unsupported command: {word}"));
    }
    RcParseResult { commands, ignored }
}

/// At most this many files are read through `source` lines.
pub const MAX_SOURCED_FILES: usize = 32;

/// Reading the files `source` lines reach, by the path as written, each once. `~` at the start is
/// the home directory; other paths stay as given.
///
/// [`SourcedFiles::pending`] names the next file to read (as the OS should see it);
/// [`SourcedFiles::answer`] takes its text, `None` when it cannot be read.
#[derive(Clone, Debug)]
pub struct SourcedFiles {
    home: String,
    queue: Vec<String>,
    sources: Vec<(String, Option<String>)>,
}

impl SourcedFiles {
    pub fn new(text: &str, home: &str) -> Self {
        let mut files = Self { home: home.to_owned(), queue: sourced_paths(text), sources: Vec::new() };
        files.skip_known();
        files
    }

    fn skip_known(&mut self) {
        while self.queue.first().is_some_and(|p| self.sources.iter().any(|(known, _)| known == p)) {
            self.queue.remove(0);
        }
    }

    fn expand_home(&self, path: &str) -> String {
        match path.strip_prefix('~') {
            Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => format!("{}{rest}", self.home),
            _ => path.to_owned(),
        }
    }

    /// The file to read next; `None` when every reachable file is read.
    pub fn pending(&self) -> Option<String> {
        if self.sources.len() >= MAX_SOURCED_FILES {
            return None;
        }
        self.queue.first().map(|p| self.expand_home(p))
    }

    /// The pending file's text.
    pub fn answer(&mut self, text: Option<&str>) {
        if self.queue.is_empty() {
            return;
        }
        let path = self.queue.remove(0);
        if let Some(text) = text {
            self.queue.extend(sourced_paths(text));
        }
        self.sources.push((path, text.map(str::to_owned)));
        self.skip_known();
    }

    /// The files read, in reading order.
    pub fn read(&self) -> &[(String, Option<String>)] {
        &self.sources
    }

    pub fn finish(self) -> Sources {
        self.sources.into_iter().collect()
    }
}

/// IntelliJ action ids → OpenIDEA action ids, for `:action` and `<Action>()`. OpenIDEA's own ids
/// (`code.reformat`) work as well.
pub const IDEA_ACTIONS: [(&str, &str); 94] = [
    ("GotoDeclaration", "navigate.declaration"),
    ("GotoImplementation", "navigate.implementation"),
    ("GotoTypeDeclaration", "navigate.typeDeclaration"),
    ("GotoSuperMethod", "navigate.superMethod"),
    ("GotoTest", "navigate.gotoTest"),
    ("GotoClass", "navigate.gotoClass"),
    ("GotoFile", "navigate.gotoFile"),
    ("GotoSymbol", "navigate.gotoSymbol"),
    ("GotoAction", "navigate.findAction"),
    ("GotoLine", "navigate.gotoLine"),
    ("SearchEverywhere", "navigate.searchEverywhere"),
    ("RecentFiles", "navigate.recentFiles"),
    ("RecentLocations", "navigate.recentLocations"),
    ("Back", "navigate.back"),
    ("Forward", "navigate.forward"),
    ("FileStructurePopup", "navigate.fileStructure"),
    ("GotoNextError", "navigate.nextError"),
    ("GotoPreviousError", "navigate.prevError"),
    ("MethodDown", "navigate.nextMethod"),
    ("MethodUp", "navigate.previousMethod"),
    ("CallHierarchy", "navigate.callHierarchy"),
    ("TypeHierarchy", "navigate.typeHierarchy"),
    ("ToggleBookmark", "navigate.toggleBookmark"),
    ("ShowBookmarks", "navigate.showBookmarks"),
    ("SelectIn", "navigate.selectIn"),
    ("Find", "find.inFile"),
    ("Replace", "find.replaceInFile"),
    ("FindInPath", "find.inPath"),
    ("ReplaceInPath", "find.replaceInPath"),
    ("FindUsages", "find.usages"),
    ("ShowUsages", "find.showUsages"),
    ("HighlightUsagesInFile", "find.highlightUsages"),
    ("ReformatCode", "code.reformat"),
    ("OptimizeImports", "code.optimizeImports"),
    ("CommentByLineComment", "code.commentLine"),
    ("CommentByBlockComment", "code.commentBlock"),
    ("ShowIntentionActions", "code.showIntentions"),
    ("QuickJavaDoc", "code.quickDocumentation"),
    ("QuickImplementations", "code.quickDefinition"),
    ("ParameterInfo", "code.parameterInfo"),
    ("CodeCompletion", "code.completion"),
    ("Generate", "code.generate"),
    ("SurroundWith", "code.surroundWith"),
    ("EditorSelectWord", "code.extendSelection"),
    ("EditorUnSelectWord", "code.shrinkSelection"),
    ("CollapseRegion", "code.fold"),
    ("ExpandRegion", "code.unfold"),
    ("CollapseAllRegions", "code.foldAll"),
    ("ExpandAllRegions", "code.unfoldAll"),
    ("RenameElement", "refactor.rename"),
    ("Refactorings.QuickListPopupAction", "refactor.thisMenu"),
    ("ChangeSignature", "refactor.changeSignature"),
    ("ExtractMethod", "refactor.extractMethod"),
    ("IntroduceVariable", "refactor.extractVariable"),
    ("IntroduceConstant", "refactor.extractConstant"),
    ("IntroduceField", "refactor.extractField"),
    ("IntroduceParameter", "refactor.extractParameter"),
    ("Inline", "refactor.inline"),
    ("Move", "refactor.move"),
    ("SafeDelete", "refactor.safeDelete"),
    ("MembersPullUp", "refactor.pullMembersUp"),
    ("MemberPushDown", "refactor.pushMembersDown"),
    ("Run", "run.run"),
    ("Debug", "run.debug"),
    ("RunClass", "run.runContext"),
    ("DebugClass", "run.debugContext"),
    ("Stop", "run.stop"),
    ("Rerun", "run.rerun"),
    ("ChooseRunConfiguration", "run.chooseConfig"),
    ("ToggleLineBreakpoint", "debug.toggleBreakpoint"),
    ("SaveAll", "file.save"),
    ("ShowSettings", "file.settings"),
    ("NewScratchFile", "file.newScratchFile"),
    ("CloseContent", "editor.closeTab"),
    ("CloseEditor", "editor.closeTab"),
    ("CloseAllEditors", "editor.closeAllTabs"),
    ("ReopenClosedTab", "editor.reopenClosedTab"),
    ("NextTab", "editor.nextTab"),
    ("PreviousTab", "editor.prevTab"),
    ("SplitVertically", "editor.splitRight"),
    ("SplitHorizontally", "editor.splitDown"),
    ("Unsplit", "editor.unsplit"),
    ("ToggleDistractionFreeMode", "editor.toggleZen"),
    ("ActivateProjectToolWindow", "toolwindow.project"),
    ("HideAllWindows", "toolwindow.hideAll"),
    ("Vcs.QuickListPopupAction", "vcs.operations"),
    ("CheckinProject", "vcs.commit"),
    ("Vcs.Push", "vcs.push"),
    ("Vcs.UpdateProject", "vcs.update"),
    ("Git.Branches", "vcs.branches"),
    ("Annotate", "vcs.annotate"),
    ("VcsShowNextChangeMarker", "vcs.nextChange"),
    ("VcsShowPrevChangeMarker", "vcs.prevChange"),
    ("Compare.SameVersion", "vcs.showDiff"),
];

/// The OpenIDEA action for an `:action` argument: an IntelliJ id, or one of OpenIDEA's as is.
pub fn resolve_action_id(id: &str) -> &str {
    IDEA_ACTIONS.iter().find(|(idea, _)| *idea == id).map_or(id, |(_, ours)| *ours)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::keymap::{PRESET_TITLES, preset};
    use std::collections::HashSet;

    const RC: &str = r#"
" OpenIDEA vimrc
set textwidth=100 nopcre
set ideajoin surround
let mapleader = " "
nnoremap <leader>f :action FindInPath<CR>
nmap <silent> gd <Action>(GotoDeclaration)
map <leader>r <Action>(ReformatCode)
imap jk <Esc>
vnoremap < <gv
noremap H ^
xmap <leader>c <Action>(code.commentLine)
let g:mapleader = ","
nmap <leader>w :w<CR>
source ~/.vimrc
nnoremap
Plug 'tpope/vim-surround'
"#;

    fn parse(text: &str) -> RcParseResult {
        parse_vimrc(text, None)
    }

    fn first(text: &str) -> RcCommand {
        parse(text).commands.remove(0)
    }

    /// The two sides of a mapping (the lhs and the Ex line of an Ex mapping).
    fn sides(command: &RcCommand) -> (&str, &str) {
        match command {
            RcCommand::Map { lhs, rhs, .. } => (lhs, rhs),
            RcCommand::MapEx { lhs, ex, .. } => (lhs, ex),
            RcCommand::Set { name, .. } => (name, ""),
        }
    }

    fn map(mode: Option<MapMode>, noremap: bool, lhs: &str, rhs: &str) -> RcCommand {
        RcCommand::Map { mode, noremap, lhs: lhs.to_owned(), rhs: rhs.to_owned() }
    }

    fn map_ex(mode: Option<MapMode>, lhs: &str, ex: &str) -> RcCommand {
        RcCommand::MapEx { mode, lhs: lhs.to_owned(), ex: ex.to_owned() }
    }

    fn set(name: &str, value: SetValue) -> RcCommand {
        RcCommand::Set { name: name.to_owned(), value }
    }

    fn ignored(file: Option<&str>, line: usize, text: &str, reason: &str) -> RcIgnored {
        RcIgnored { file: file.map(str::to_owned), line, text: text.to_owned(), reason: reason.to_owned() }
    }

    #[test]
    fn parses_set_options_keeping_only_ones_the_engine_supports() {
        let result = parse(RC);
        let sets: Vec<&RcCommand> = result.commands.iter().filter(|c| matches!(c, RcCommand::Set { .. })).collect();
        assert_eq!(sets, [&set("textwidth", SetValue::Text("100".to_owned())), &set("pcre", SetValue::Flag(false))]);
        let reasons: Vec<&str> = result.ignored.iter().map(|i| i.reason.as_str()).collect();
        assert!(reasons.contains(&"option not supported: ideajoin"));
        assert!(reasons.contains(&"option not supported: surround"));
    }

    #[test]
    fn parses_map_variants_with_modes_noremap_and_the_leader_in_effect_at_that_line() {
        let maps: Vec<RcCommand> =
            parse(RC).commands.into_iter().filter(|c| !matches!(c, RcCommand::Set { .. })).collect();
        assert_eq!(
            maps,
            [
                map_ex(Some(MapMode::Normal), "<Space>f", "action FindInPath"),
                map_ex(Some(MapMode::Normal), "gd", "action GotoDeclaration"),
                map_ex(None, "<Space>r", "action ReformatCode"),
                map(Some(MapMode::Insert), false, "jk", "<Esc>"),
                map(Some(MapMode::Visual), true, "<", "<gv"),
                map(None, true, "H", "^"),
                map_ex(Some(MapMode::Visual), "<Space>c", "action code.commentLine"),
                map_ex(Some(MapMode::Normal), ",w", "w"),
            ]
        );
    }

    #[test]
    fn canonicalises_special_key_names_in_mappings_whatever_their_case() {
        assert_eq!(
            parse("inoremap jk <esc>\nmap <c-c> <C-C>\nimap <CR> <cr>").commands,
            [
                map(Some(MapMode::Insert), true, "jk", "<Esc>"),
                map(None, false, "<C-c>", "<C-C>"),
                map(Some(MapMode::Insert), false, "<CR>", "<CR>"),
            ]
        );
    }

    #[test]
    fn normalizes_special_keys_on_both_sides_in_any_case() {
        let mut table: Vec<(String, String)> = [
            ("esc", "<Esc>"),
            ("cr", "<CR>"),
            ("enter", "<CR>"),
            ("return", "<CR>"),
            ("tab", "<Tab>"),
            ("bs", "<BS>"),
            ("backspace", "<BS>"),
            ("space", "<Space>"),
            ("del", "<Del>"),
            ("delete", "<Del>"),
            ("up", "<Up>"),
            ("down", "<Down>"),
            ("left", "<Left>"),
            ("right", "<Right>"),
            ("home", "<Home>"),
            ("end", "<End>"),
            ("pageup", "<PageUp>"),
            ("pagedown", "<PageDown>"),
            ("ins", "<Ins>"),
            ("insert", "<Ins>"),
            ("bar", "|"),
            ("bslash", "\\"),
        ]
        .iter()
        .map(|(n, c)| ((*n).to_owned(), (*c).to_owned()))
        .collect();
        table.extend((1..=12).map(|i| (format!("f{i}"), format!("<F{i}>"))));
        for (name, canonical) in table {
            let capitalized = format!("{}{}", name[..1].to_uppercase(), &name[1..]);
            for spelling in [name.clone(), name.to_uppercase(), capitalized] {
                let command = first(&format!("nmap <{spelling}> <{spelling}>"));
                assert_eq!(sides(&command), (canonical.as_str(), canonical.as_str()), "{spelling}");
            }
        }
    }

    #[test]
    fn normalizes_modifiers_and_preserves_unknown_tokens() {
        for (keys, canonical) in [
            ("<c-f>", "<C-f>"),
            ("<s-tab>", "<S-Tab>"),
            ("<s-cr>", "<S-CR>"),
            ("<a-x>", "<A-x>"),
            ("<m-x>", "<A-x>"),
            ("<d-x>", "<M-x>"),
            ("<s-c-x>", "<C-S-X>"),
            ("<C-S-x>", "<C-S-X>"),
            ("<s-x>", "X"),
            ("<d-a-c-s-x>", "<C-A-M-S-X>"),
            ("<c-C>", "<C-C>"),
            ("<frob>", "<frob>"),
            ("<c-Frob>", "<c-Frob>"),
            ("<z-esc>", "<z-esc>"),
            ("<constructor>", "<constructor>"),
            ("<toString>", "<toString>"),
            ("<", "<"),
            ("<<esc>", "<<Esc>"),
            ("<c->", "<c->"),
        ] {
            let command = first(&format!("nmap {keys} {keys}"));
            assert_eq!(sides(&command), (canonical, canonical), "{keys}");
        }
    }

    #[test]
    fn keeps_escaped_less_than_separate_from_following_key_names() {
        assert_eq!(sides(&first("nmap <LT> <Lt>Esc>")), ("<", "<lt>Esc>"));
        assert_eq!(sides(&first("nmap <c-f> <pageup><lt>x")), ("<C-f>", "<PageUp><lt>x"));
    }

    #[test]
    fn expands_mixed_case_leaders_on_both_sides_without_replacement_string_interpolation() {
        assert_eq!(sides(&first("let mapleader=\"$&\"\nnmap <LeAdEr>a <LEADER>b")), ("$&a", "$&b"));
        assert_eq!(sides(&first("let mapleader=\"\\<tAb>\"\nnmap <LEADER>a <leader>b")), ("<Tab>a", "<Tab>b"));
    }

    #[test]
    fn normalizes_ex_mapping_lhs_while_preserving_command_and_action_text() {
        assert_eq!(
            parse("nmap <f1> <Action>(GotoDeclaration)\nnmap <s-tab> :echo <cr>").commands,
            [
                map_ex(Some(MapMode::Normal), "<F1>", "action GotoDeclaration"),
                map_ex(Some(MapMode::Normal), "<S-Tab>", "echo"),
            ]
        );
    }

    #[test]
    fn recognizes_ex_terminator() {
        for enter in ["cr", "CR", "enter", "ENTER", "return", "RETURN"] {
            assert_eq!(first(&format!("nmap <f1> :w<{enter}>")), map_ex(Some(MapMode::Normal), "<F1>", "w"), "{enter}");
        }
    }

    #[test]
    fn reports_everything_else_as_ignored_never_executed() {
        let reasons: Vec<String> = parse(RC).ignored.iter().map(|i| format!("{}:{}", i.line, i.reason)).collect();
        assert!(reasons.contains(&"15:cannot read file".to_owned()));
        assert!(reasons.contains(&"16:mapping needs a left and right side".to_owned()));
        assert!(reasons.contains(&"17:unsupported command: Plug".to_owned()));
    }

    #[test]
    fn recognises_action_and_action_function_right_hand_sides() {
        assert_eq!(ex_of_rhs("<Action>(GotoDeclaration)").as_deref(), Some("action GotoDeclaration"));
        assert_eq!(ex_of_rhs(":action ReformatCode<CR>").as_deref(), Some("action ReformatCode"));
        assert_eq!(ex_of_rhs(":<C-u>action Back<cr>").as_deref(), Some("action Back"));
        assert_eq!(ex_of_rhs("dd"), None);
        assert_eq!(ex_of_rhs(":action A<CR>j"), None);
    }

    #[test]
    fn parses_a_leader_written_as_escaped_space_and_a_comma() {
        assert_eq!(
            parse("let mapleader=\"\\<Space>\"\nnmap <Leader>a b").commands,
            [map(Some(MapMode::Normal), false, "<Space>a", "b")]
        );
        assert_eq!(sides(&first("nmap <leader>a b")).0, "\\a");
    }

    #[test]
    fn maps_idea_ids_to_openidea_ids_and_passes_our_own_ids_through() {
        assert_eq!(resolve_action_id("GotoDeclaration"), "navigate.declaration");
        assert_eq!(resolve_action_id("ReformatCode"), "code.reformat");
        assert_eq!(resolve_action_id("FindInPath"), "find.inPath");
        assert_eq!(resolve_action_id("code.commentLine"), "code.commentLine");
    }

    #[test]
    fn targets_actions_that_exist() {
        let mut known: HashSet<&str> = PRESET_TITLES
            .iter()
            .filter_map(|(id, _)| preset(id))
            .flat_map(|keymap| keymap.keys().map(String::as_str))
            .collect();
        known.extend(["editor.closeAllTabs", "editor.unsplit", "editor.splitDown", "vcs.update"]);
        let missing: Vec<&(&str, &str)> = IDEA_ACTIONS.iter().filter(|(_, id)| !known.contains(id)).collect();
        assert!(missing.is_empty(), "{missing:?}");
    }

    #[test]
    fn applies_the_sourced_file_in_place_sharing_the_leader() {
        let sources = Sources::from([(
            "~/.vimrc".to_owned(),
            Some("let mapleader = \",\"\nset textwidth=80\nset number\nnmap <leader>a b".to_owned()),
        )]);
        let result = parse_vimrc("nmap x y\nsource ~/.vimrc\nnmap <leader>c d", Some(&sources));
        assert_eq!(
            result.commands,
            [
                map(Some(MapMode::Normal), false, "x", "y"),
                set("textwidth", SetValue::Text("80".to_owned())),
                map(Some(MapMode::Normal), false, ",a", "b"),
                map(Some(MapMode::Normal), false, ",c", "d"),
            ]
        );
        assert_eq!(result.ignored, [ignored(Some("~/.vimrc"), 3, "set number", "option not supported: number")]);
    }

    #[test]
    fn reports_unreadable_files_and_files_that_source_themselves() {
        let sources = Sources::from([
            ("a".to_owned(), Some("source b".to_owned())),
            ("b".to_owned(), Some("source a".to_owned())),
            ("gone".to_owned(), None),
        ]);
        assert_eq!(
            parse_vimrc("so gone\nsource a", Some(&sources)).ignored,
            [ignored(None, 1, "so gone", "cannot read file"), ignored(Some("b"), 1, "source a", "file sources itself")]
        );
    }

    #[test]
    fn reads_nested_files_once_expanding_home_only_at_the_start() {
        let files: HashMap<&str, &str> =
            HashMap::from([("/home/me/.vimrc", "source /etc/extra\nsource ~/.vimrc"), ("/etc/extra", "")]);
        let mut reading = SourcedFiles::new("source ~/.vimrc\nsource ~other/x", "/home/me");
        let mut asked = Vec::new();
        while let Some(path) = reading.pending() {
            asked.push(path.clone());
            reading.answer(files.get(path.as_str()).copied());
        }
        assert_eq!(asked, ["/home/me/.vimrc", "~other/x", "/etc/extra"]);
        assert_eq!(
            reading.read(),
            [
                ("~/.vimrc".to_owned(), Some("source /etc/extra\nsource ~/.vimrc".to_owned())),
                ("~other/x".to_owned(), None),
                ("/etc/extra".to_owned(), Some(String::new())),
            ]
        );
    }
}
