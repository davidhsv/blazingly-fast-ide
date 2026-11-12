//! The message composer: `@` / `/` completion, attachments, history recall, the settings it shows and
//! the meters beside it. Offsets and carets are UTF-16 code units.

use crate::find::utf16;
use crate::platform::i18n::{I18n, Param};
use ide_agent::{
    AgentCommand, AgentCommandSource, AgentConfigOption, AgentCost, AgentEntry, AgentImage, AgentLimit, AgentPrompt,
    AgentRetry, AgentUsage,
};
use ide_core::IpcPath;
use ide_index::FileMatch;
use regex::Regex;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

/// A file or folder referenced with `@`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRef {
    /// Absolute path (in the task's directory).
    pub path: String,
    /// As written after `@`.
    pub rel: String,
}

/// What a composer holds before it is sent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComposerDraft {
    pub text: String,
    pub files: Vec<FileRef>,
    pub images: Vec<AgentImage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Text,
    File,
    Command,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageToken {
    pub kind: TokenKind,
    pub value: String,
}

/// A message's text split into plain runs, `@file` references and `/command` skills. A command is a
/// `/word` at the start or after whitespace; a reference likewise after `@`.
pub fn message_tokens(text: &str) -> Vec<MessageToken> {
    static TOKEN: LazyLock<Regex> = LazyLock::new(|| pattern(r"(^|\s)([@/][^\s@]+)"));
    static COMMAND: LazyLock<Regex> = LazyLock::new(|| pattern(r"^/[A-Za-z][A-Za-z0-9_:.\-]*$"));
    let mut out = Vec::new();
    let mut last = 0;
    for m in TOKEN.captures_iter(text) {
        let Some(token) = m.get(2) else { continue };
        let command = COMMAND.is_match(token.as_str());
        if token.as_str().starts_with('/') && !command {
            continue;
        }
        if token.start() > last {
            out.push(MessageToken { kind: TokenKind::Text, value: text[last..token.start()].to_owned() });
        }
        let kind = if command { TokenKind::Command } else { TokenKind::File };
        out.push(MessageToken { kind, value: token.as_str().to_owned() });
        last = token.end();
    }
    if last < text.len() {
        out.push(MessageToken { kind: TokenKind::Text, value: text[last..].to_owned() });
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionKind {
    File,
    Command,
}

/// The `@file` or `/command` being typed: `from` is its `@` or `/`, `to` the caret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub kind: CompletionKind,
    pub query: String,
    pub from: u32,
    pub to: u32,
}

/// The `@file` or `/command` being typed at `caret`, if any; both may follow other text.
pub fn completion_at(text: &str, caret: u32) -> Option<Completion> {
    static WORD: LazyLock<Regex> = LazyLock::new(|| pattern(r"(^|\s)([@/])([^\s@]*)$"));
    let before = utf16::slice(text, 0, caret);
    let word = WORD.captures(before)?;
    let query = word.get(3).map_or("", |g| g.as_str());
    let command = word.get(2).is_some_and(|g| g.as_str() == "/");
    let kind = if command { CompletionKind::Command } else { CompletionKind::File };
    let to = utf16::len(before);
    Some(Completion { kind, query: query.to_owned(), from: to - utf16::len(query) - 1, to })
}

/// Text and caret after a completion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completed {
    pub text: String,
    pub caret: u32,
}

/// Replaces the completion with `insert` plus a space; returns the text and the caret after it.
pub fn apply_completion(text: &str, c: &Completion, insert: &str) -> Completed {
    let after = utf16::slice_from(text, c.to);
    let glue = if after.starts_with(' ') { "" } else { " " };
    let next = format!("{}{insert}{glue}{after}", utf16::slice(text, 0, c.from));
    Completed { text: next, caret: c.from + utf16::len(insert) + 1 }
}

/// Whether `text` holds the characters of `query` in order.
fn in_order(text: &str, query: &str) -> bool {
    let query: Vec<char> = query.chars().collect();
    let mut i = 0;
    for ch in text.chars() {
        if query.get(i) == Some(&ch) {
            i += 1;
        }
    }
    i >= query.len()
}

/// Commands matching `query`, the agent's commands before skills: prefix matches first (a plugin's
/// "plugin:name" also by its name), then substrings, then names holding its letters in order
/// ("trig" → "triage").
pub fn match_commands<'a>(commands: &'a [AgentCommand], query: &str) -> Vec<&'a AgentCommand> {
    let q = query.to_lowercase();
    let rank = |name: &str| {
        let n = name.to_lowercase();
        let short = n.rsplit_once(':').map_or(n.as_str(), |(_, short)| short);
        if n.starts_with(&q) || short.starts_with(&q) {
            Some(0)
        } else if n.contains(&q) {
            Some(1)
        } else {
            in_order(&n, &q).then_some(2)
        }
    };
    let mut ranked: Vec<(bool, u8, &AgentCommand)> =
        commands.iter().filter_map(|c| rank(&c.name).map(|r| (c.source == AgentCommandSource::Skill, r, c))).collect();
    ranked.sort_by_key(|(skill, rank, _)| (*skill, *rank));
    ranked.into_iter().map(|(_, _, c)| c).collect()
}

/// Whether an `@` query is a typed-out path (`../x`, `./x`, `/x`) rather than a project file search.
pub fn is_path_query(query: &str) -> bool {
    static PATH: LazyLock<Regex> = LazyLock::new(|| pattern(r"^(\.\.?(/|$)|/)"));
    PATH.is_match(query)
}

fn is_drive(part: Option<&&str>) -> bool {
    part.is_some_and(|p| p.len() == 2 && p.as_bytes()[0].is_ascii_alphabetic() && p.as_bytes()[1] == b':')
}

/// `rel` (as typed after `@`) resolved against `root`, `.` and `..` folded lexically.
pub fn resolve_typed(root: &str, rel: &str) -> String {
    let path = if rel.starts_with('/') { rel.to_owned() } else { format!("{}/{rel}", root.replace('\\', "/")) };
    let mut out: Vec<&str> = Vec::new();
    for part in path.split('/') {
        if part == ".." {
            // Never fold away a Windows drive (`C:`).
            if out.len() > 1 || !is_drive(out.first()) {
                out.pop();
            }
        } else if !part.is_empty() && part != "." {
            out.push(part);
        }
    }
    if is_drive(out.first()) { out.join("/") } else { format!("/{}", out.join("/")) }
}

/// A file or folder the `@` popup offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathMatch {
    pub path: String,
    pub rel: String,
    pub dir: bool,
    pub score: u32,
}

/// Index file and folder matches as one list, best first (files first on a tie); folders' `rel` loses
/// the trailing `/` the index gives them.
pub fn merge_matches(files: &[FileMatch], dirs: &[FileMatch], limit: usize) -> Vec<PathMatch> {
    let entry = |m: &FileMatch, dir: bool| PathMatch {
        path: ide_core::ipc_path::to_ipc(&m.path),
        rel: if dir { m.rel_path.strip_suffix('/').unwrap_or(&m.rel_path).to_owned() } else { m.rel_path.clone() },
        dir,
        score: m.score,
    };
    let mut out: Vec<PathMatch> =
        files.iter().map(|f| entry(f, false)).chain(dirs.iter().map(|d| entry(d, true))).collect();
    out.sort_by(|a, b| b.score.cmp(&a.score).then(a.dir.cmp(&b.dir)));
    out.truncate(limit);
    out
}

/// 1-based lines of a reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineRange {
    pub from: u32,
    pub to: u32,
}

/// `@rel` with a line range suffix when given ("@src/a.rs:10-20").
pub fn mention_of(rel: &str, lines: Option<LineRange>) -> String {
    match lines {
        None => format!("@{rel}"),
        Some(l) if l.from == l.to => format!("@{rel}:{}", l.from),
        Some(l) => format!("@{rel}:{}-{}", l.from, l.to),
    }
}

/// Whether `text` still mentions `rel`: `@rel` not followed by more of a path, or `@rel:` (a line).
fn mentions(text: &str, rel: &str) -> bool {
    let mention = format!("@{rel}");
    text.match_indices(&mention).any(|(at, _)| match text[at + mention.len()..].chars().next() {
        None | Some(':') => true,
        Some(c) => !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '-')),
    })
}

/// The prompt to send: files still mentioned in the text (each once), and the images.
pub fn to_prompt(draft: &ComposerDraft) -> AgentPrompt {
    let mut files: Vec<String> = Vec::new();
    for f in &draft.files {
        if !files.contains(&f.path) && mentions(&draft.text, &f.rel) {
            files.push(f.path.clone());
        }
    }
    AgentPrompt {
        text: draft.text.trim().to_owned(),
        files: files.into_iter().map(|p| IpcPath(PathBuf::from(p))).collect(),
        images: draft.images.clone(),
    }
}

pub fn can_send(draft: &ComposerDraft) -> bool {
    !draft.text.trim().is_empty() || !draft.images.is_empty()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

/// ↑ / ↓ through sent messages (newest first). Index −1 is the unsent draft. Returns the new index, or
/// `None` when the key should move the caret instead.
pub fn recall(history_len: usize, index: i64, dir: Direction) -> Option<i64> {
    let next = index + if dir == Direction::Up { 1 } else { -1 };
    (next >= -1 && next < history_len as i64).then_some(next)
}

/// Whether ↑ / ↓ recalls history: the caret is on the first / last line.
pub fn at_edge_line(text: &str, caret: u32, dir: Direction) -> bool {
    match dir {
        Direction::Up => !utf16::slice(text, 0, caret).contains('\n'),
        Direction::Down => !utf16::slice_from(text, caret).contains('\n'),
    }
}

fn category(option: &AgentConfigOption) -> &str {
    option.category.as_deref().unwrap_or_default()
}

/// The settings shown in the composer: model, mode, permissions, approvals, effort (the first of each).
pub fn main_options(config: &[AgentConfigOption]) -> Vec<&AgentConfigOption> {
    ["model", "mode", "permissions", "approval", "thought_level"]
        .iter()
        .filter_map(|c| config.iter().find(|o| category(o) == *c))
        .collect()
}

pub fn mode_option(config: &[AgentConfigOption]) -> Option<&AgentConfigOption> {
    config.iter().find(|o| category(o) == "mode")
}

/// An icon per permission mode, by what it lets the agent do (mode ids differ per agent: Claude
/// `acceptEdits`, Gemini `auto_edit`, Codex `full-access`…). Unknown modes ask.
pub fn mode_icon(mode: &str) -> &'static str {
    let lower = mode.to_lowercase();
    let last = lower.rsplit(['#', '/']).next().unwrap_or_default();
    let m: String = last.chars().filter(char::is_ascii_lowercase).collect();
    if m.contains("plan") || m == "architect" {
        "todo"
    } else if m.contains("bypass") || m == "yolo" || m.contains("fullaccess") || m == "dangerous" {
        "unlock"
    } else if m.contains("readonly") || m == "ask" || m == "chat" {
        "eye"
    } else if m.contains("edit") || m == "code" {
        "edit"
    } else if m == "auto" || m == "agent" || m.contains("autopilot") {
        "run"
    } else {
        "lock"
    }
}

/// The settings that approve everything without asking (a bypass mode, or approvals), by option id.
pub fn bypass_defaults(config: &[AgentConfigOption]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(mode) = mode_option(config)
        && let Some(unlock) = mode.values.iter().find(|v| mode_icon(&v.value) == "unlock")
    {
        out.insert(mode.id.clone(), unlock.value.clone());
    }
    if let Some(permissions) = config.iter().find(|o| category(o) == "permissions")
        && let Some(all) = permissions.values.iter().find(|v| matches!(v.value.to_lowercase().as_str(), "on" | "true"))
    {
        out.insert(permissions.id.clone(), all.value.clone());
    }
    if let Some(approval) = config.iter().find(|o| category(o) == "approval")
        && approval.values.iter().any(|v| v.value == "bypass")
    {
        out.insert(approval.id.clone(), "bypass".to_owned());
    }
    out
}

/// Shift+Tab: the value after the option's current one.
pub fn next_value(option: &AgentConfigOption) -> Option<&str> {
    next_value_after(option, option.current.as_deref())
}

/// The value after `current` (the first one when `current` is not among the values).
pub fn next_value_after<'a>(option: &'a AgentConfigOption, current: Option<&str>) -> Option<&'a str> {
    let len = option.values.len();
    if len == 0 {
        return None;
    }
    let next = option.values.iter().position(|v| Some(v.value.as_str()) == current).map_or(0, |i| (i + 1) % len);
    Some(option.values[next].value.as_str())
}

/// A pending API retry: "3/10" (or "3" when the agent gives no limit) and the seconds left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryParts {
    pub attempt: String,
    pub wait: u64,
}

pub fn retry_parts(r: &AgentRetry, now: f64) -> RetryParts {
    let attempt = match r.max_retries {
        Some(max) => format!("{}/{max}", r.attempt),
        None => r.attempt.to_string(),
    };
    RetryParts { attempt, wait: ((r.retry_at_ms - now) / 1000.0).ceil().max(0.0) as u64 }
}

/// A meter's text and its tooltip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeterLabel {
    pub short: String,
    pub detail: String,
}

/// `x` with up to `digits` decimals, trailing zeros dropped.
fn trimmed(x: f64, digits: usize) -> String {
    let s = format!("{x:.digits$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
}

/// `850`, `83.1k`, `1M`.
fn tokens(n: f64) -> String {
    if n >= 1_000_000.0 {
        format!("{}M", trimmed(n / 1_000_000.0, 1))
    } else if n >= 1000.0 {
        format!("{}k", trimmed(n / 1000.0, if n >= 100_000.0 { 0 } else { 1 }))
    } else {
        format!("{}", n.round())
    }
}

/// "12%" of the context window and a tooltip with the numbers.
pub fn usage_label(u: &AgentUsage, i18n: &mut I18n) -> MeterLabel {
    let pct = (f64::from(u.used) / f64::from(u.size.max(1)) * 100.0).round().min(100.0);
    let detail =
        i18n.tx_built(&["", " / ", " tokens"], &[tokens(f64::from(u.used)).into(), tokens(f64::from(u.size)).into()]);
    MeterLabel { short: format!("{pct}%"), detail }
}

/// The usage-limit windows agents report, by the name they give them: the short and the full name.
const LIMIT_WINDOWS: [(&str, &str, &str); 4] = [
    ("five_hour", "5h", "5-hour limit"),
    ("seven_day", "7d", "Weekly limit"),
    ("seven_day_opus", "7d Opus", "Weekly Opus limit"),
    ("seven_day_sonnet", "7d Sonnet", "Weekly Sonnet limit"),
];

/// A usage-limit meter: "5h 19%", the share used, and a tooltip saying when it resets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LimitLabel {
    pub short: String,
    pub percent: u32,
    pub detail: String,
}

/// `45m`, `2h 51m`, `5d 3h`.
fn until_reset(ms: f64) -> String {
    let m = (ms / 60_000.0).round() as u64;
    if m < 60 {
        return format!("{m}m");
    }
    let h = m / 60;
    if h < 24 {
        return format!("{h}h {:02}m", m % 60);
    }
    format!("{}d {}h", h / 24, h % 24)
}

pub fn limit_label(l: &AgentLimit, now: f64, i18n: &mut I18n) -> LimitLabel {
    let known = LIMIT_WINDOWS.iter().find(|(window, _, _)| *window == l.window);
    let percent = l.used_percent.round().clamp(0.0, 100.0) as u32;
    let name = known.map_or_else(|| l.window.clone(), |(_, _, name)| i18n.tx(name));
    let short = format!("{} {percent}%", known.map_or(l.window.as_str(), |(_, short, _)| *short));
    let used = i18n.tx_built(&["", ": ", "% used"], &[Param::from(name), Param::Number(f64::from(percent))]);
    let detail = match l.resets_ms {
        None => used,
        Some(resets) => {
            let reset = until_reset((resets - now).max(0.0));
            i18n.tx_built(&["", ", resets in ", ""], &[Param::from(used), Param::from(reset)])
        }
    };
    LimitLabel { short, percent, detail }
}

/// Symbols and decimals of the currencies agents bill in, as `en-US` writes them.
const CURRENCIES: [(&str, &str, usize); 12] = [
    ("USD", "$", 2),
    ("EUR", "€", 2),
    ("GBP", "£", 2),
    ("JPY", "¥", 0),
    ("CNY", "CN¥", 2),
    ("CAD", "CA$", 2),
    ("AUD", "A$", 2),
    ("INR", "₹", 2),
    ("KRW", "₩", 0),
    ("BRL", "R$", 2),
    ("MXN", "MX$", 2),
    ("CHF", "CHF\u{a0}", 2),
];

/// `1234.5` with `digits` decimals and thousands separators: `1,234.50`.
fn grouped(amount: f64, digits: usize) -> String {
    let fixed = format!("{:.digits$}", amount.abs());
    let (int, frac) = fixed.split_once('.').map_or((fixed.as_str(), None), |(i, f)| (i, Some(f)));
    let mut out = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if let Some(frac) = frac {
        out.push('.');
        out.push_str(frac);
    }
    out
}

/// "$0.42": what the session has cost so far. A code that is not ISO 4217 is written after the amount.
pub fn cost_label(c: &AgentCost) -> String {
    let code = c.currency.to_uppercase();
    let sign = if c.amount < 0.0 { "-" } else { "" };
    if c.currency.len() != 3 || !c.currency.bytes().all(|b| b.is_ascii_alphabetic()) {
        return format!("{:.2} {}", c.amount, c.currency);
    }
    match CURRENCIES.iter().find(|(iso, _, _)| *iso == code) {
        Some((_, symbol, digits)) => format!("{sign}{symbol}{}", grouped(c.amount, *digits)),
        None => format!("{sign}{code}\u{a0}{}", grouped(c.amount, 2)),
    }
}

/// Time the agent spent on the task's turns; a running one counts up to `now` (`None` when idle).
pub fn work_time(entries: &[AgentEntry], now: Option<f64>) -> f64 {
    let users: Vec<(Option<f64>, Option<f64>)> = entries
        .iter()
        .filter_map(|e| match e {
            AgentEntry::User { sent_ms, done_ms, .. } => Some((*sent_ms, *done_ms)),
            _ => None,
        })
        .collect();
    let last = users.len().saturating_sub(1);
    users
        .iter()
        .enumerate()
        .filter_map(|(i, (sent, done))| {
            let sent = (*sent)?;
            let end = done.or(if i == last { now } else { None })?;
            (end > sent).then_some(end - sent)
        })
        .sum()
}

/// `data:image/png;base64,AAA` → an image attachment.
pub fn image_from_data_url(url: &str, name: Option<&str>) -> Option<AgentImage> {
    static DATA: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?s)^data:([^;,]+);base64,(.*)$"));
    let m = DATA.captures(url)?;
    let mime = m.get(1)?.as_str();
    mime.starts_with("image/").then(|| AgentImage {
        mime_type: mime.to_owned(),
        data: m.get(2).map_or("", |g| g.as_str()).to_owned(),
        name: name.map(str::to_owned),
    })
}

/// Whether a file name is an image the composer attaches.
pub fn is_image_file(name: &str) -> bool {
    let lower = name.to_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp"].iter().any(|ext| lower.ends_with(ext))
}

/// Larger images are refused (agents reject them anyway).
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::fixtures::option;

    fn command(name: &str, source: AgentCommandSource) -> AgentCommand {
        AgentCommand { name: name.to_owned(), description: String::new(), hint: None, source }
    }

    fn file_match(path: &str, rel: &str, score: u32) -> FileMatch {
        FileMatch { path: IpcPath(PathBuf::from(path)), rel_path: rel.to_owned(), score, indices: Vec::new() }
    }

    fn names<'a>(commands: &[&'a AgentCommand]) -> Vec<&'a str> {
        commands.iter().map(|c| c.name.as_str()).collect()
    }

    fn files(prompt: &AgentPrompt) -> Vec<String> {
        prompt.files.iter().map(|p| ide_core::ipc_path::to_ipc(p)).collect()
    }

    #[test]
    fn ranks_folders_among_files_by_score_files_first_on_a_tie() {
        let merged = merge_matches(
            &[file_match("/p/src/agents.ts", "src/agents.ts", 50), file_match("/p/x.ts", "x.ts", 10)],
            &[file_match("/p/src/agents", "src/agents/", 50), file_match("/p/docs", "docs/", 70)],
            3,
        );
        let m = |path: &str, rel: &str, dir, score| PathMatch { path: path.into(), rel: rel.into(), dir, score };
        assert_eq!(
            merged,
            vec![
                m("/p/docs", "docs", true, 70),
                m("/p/src/agents.ts", "src/agents.ts", false, 50),
                m("/p/src/agents", "src/agents", true, 50),
            ]
        );
    }

    #[test]
    fn completes_files_and_commands_anywhere_at_a_word_boundary() {
        let c = |kind, query: &str, from, to| Some(Completion { kind, query: query.into(), from, to });
        assert_eq!(completion_at("fix @src/le", 11), c(CompletionKind::File, "src/le", 4, 11));
        assert_eq!(completion_at("/rev", 4), c(CompletionKind::Command, "rev", 0, 4));
        assert_eq!(completion_at("run /rev", 8), c(CompletionKind::Command, "rev", 4, 8));
        assert_eq!(completion_at("a@b", 3), None);
        let at = completion_at("see @le and", 7).expect("a completion");
        assert_eq!(
            apply_completion("see @le and", &at, "@src/lexer.rs"),
            Completed { text: "see @src/lexer.rs and".into(), caret: 18 }
        );
        let cmds: Vec<AgentCommand> =
            ["init", "review", "pr-review"].iter().map(|n| command(n, AgentCommandSource::Command)).collect();
        assert_eq!(names(&match_commands(&cmds, "rev")), vec!["review", "pr-review"]);
    }

    #[test]
    fn matches_commands_by_prefix_plugin_name_substring_then_letters_in_order() {
        let cmds: Vec<AgentCommand> =
            ["triage", "verify", "issue-fix", "brooks-lint:brooks-review", "pr-review", "init"]
                .iter()
                .map(|n| command(n, AgentCommandSource::Command))
                .collect();
        let found = |q: &str| names(&match_commands(&cmds, q)).into_iter().map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(found("trig"), vec!["triage"]);
        assert_eq!(found("brooks-r"), vec!["brooks-lint:brooks-review"]);
        assert_eq!(found("rev"), vec!["brooks-lint:brooks-review", "pr-review"]);
        assert_eq!(found("if"), vec!["verify", "issue-fix"]);
        assert!(found("xyz").is_empty());
        assert_eq!(found("").len(), cmds.len());
    }

    #[test]
    fn keeps_all_commands_and_skills_and_ranks_matches_within_each_source() {
        let source = |i: usize| if i % 2 == 1 { AgentCommandSource::Command } else { AgentCommandSource::Skill };
        let cmds: Vec<AgentCommand> = (0..600).map(|i| command(&format!("item-{i}"), source(i))).collect();
        let all = match_commands(&cmds, "");
        assert_eq!(all.len(), 600);
        let commands: Vec<&AgentCommand> = cmds.iter().filter(|c| c.source == AgentCommandSource::Command).collect();
        let skills: Vec<&AgentCommand> = cmds.iter().filter(|c| c.source == AgentCommandSource::Skill).collect();
        assert_eq!(all[..300], commands[..]);
        assert_eq!(all[300..], skills[..]);
        let mixed = [
            command("pre-review", source(0)),
            command("pre-review", source(1)),
            command("review", source(0)),
            command("review", source(1)),
        ];
        assert_eq!(match_commands(&mixed, "rev"), vec![&mixed[3], &mixed[1], &mixed[2], &mixed[0]]);
        assert!(match_commands(&mixed, "missing").is_empty());
    }

    fn file(path: &str, rel: &str) -> FileRef {
        FileRef { path: path.into(), rel: rel.into() }
    }

    #[test]
    fn sends_only_the_files_still_mentioned() {
        let prompt = to_prompt(&ComposerDraft {
            text: " look at @src/a.rs:3-5 and @b.rs ".into(),
            files: vec![
                file("/w/src/a.rs", "src/a.rs"),
                file("/w/b.rs", "b.rs"),
                file("/w/b.rs", "b.rs"),
                file("/w/gone.rs", "gone.rs"),
            ],
            images: Vec::new(),
        });
        assert_eq!(prompt.text, "look at @src/a.rs:3-5 and @b.rs");
        assert_eq!(files(&prompt), vec!["/w/src/a.rs", "/w/b.rs"]);
        assert!(prompt.images.is_empty());
        assert_eq!(mention_of("a.rs", Some(LineRange { from: 3, to: 3 })), "@a.rs:3");
        // A folder counts while `@src/` is left as typed, not once a file inside it was picked.
        let folder =
            |text: &str| ComposerDraft { text: text.into(), files: vec![file("/w/src", "src/")], images: Vec::new() };
        assert_eq!(files(&to_prompt(&folder("explain @src/ please"))), vec!["/w/src"]);
        assert_eq!(files(&to_prompt(&folder("explain @src/"))), vec!["/w/src"]);
        assert!(files(&to_prompt(&folder("explain @src/a.rs"))).is_empty());
        assert!(!can_send(&ComposerDraft { text: " ".into(), ..ComposerDraft::default() }));
    }

    #[test]
    fn recalls_history_from_the_edge_lines_only() {
        assert_eq!(recall(2, -1, Direction::Up), Some(0));
        assert_eq!(recall(2, 0, Direction::Up), Some(1));
        assert_eq!(recall(2, 1, Direction::Up), None);
        assert_eq!(recall(2, 0, Direction::Down), Some(-1));
        assert_eq!(recall(2, -1, Direction::Down), None);
        assert!(at_edge_line("a\nb", 1, Direction::Up));
        assert!(!at_edge_line("a\nb", 3, Direction::Up));
        assert!(!at_edge_line("a\nb", 1, Direction::Down));
    }

    #[test]
    fn picks_the_main_settings_and_cycles_the_mode() {
        let config = vec![
            option("fast", &["on"], None),
            option("mode", &["ask", "code"], Some("code")),
            option("model", &["m"], Some("m")),
        ];
        let ids = |options: Vec<&AgentConfigOption>| options.iter().map(|o| o.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(main_options(&config)), vec!["model", "mode"]);
        assert_eq!(next_value(&config[1]), Some("ask"));
        let mut permissions = option("allow_all", &["off", "on"], Some("off"));
        permissions.category = Some("permissions".into());
        let with_permissions = [config, vec![permissions]].concat();
        assert_eq!(ids(main_options(&with_permissions)), vec!["model", "mode", "allow_all"]);
        let mut approvals = option("openidea.cursor.approval", &["ask", "bypass"], Some("ask"));
        approvals.category = Some("approval".into());
        let with_approvals = [with_permissions.clone(), vec![approvals]].concat();
        assert_eq!(ids(main_options(&with_approvals)), vec!["model", "mode", "allow_all", "openidea.cursor.approval"]);
        let modes = [
            "default",
            "acceptEdits",
            "plan",
            "auto",
            "bypassPermissions",
            "auto_edit",
            "read-only",
            "x",
            "https://agentclientprotocol.com/protocol/session-modes#agent",
            "https://agentclientprotocol.com/protocol/session-modes#autopilot",
        ];
        assert_eq!(
            modes.map(mode_icon),
            ["lock", "edit", "todo", "run", "unlock", "edit", "eye", "lock", "run", "run"]
        );
        assert_eq!(bypass_defaults(&with_permissions), BTreeMap::from([("allow_all".into(), "on".into())]));
        let mut i18n = I18n::default();
        let usage = AgentUsage { used: 24_000, size: 200_000, cost: None };
        assert_eq!(
            usage_label(&usage, &mut i18n),
            MeterLabel { short: "12%".into(), detail: "24k / 200k tokens".into() }
        );
        assert_eq!(
            image_from_data_url("data:image/png;base64,AAA", Some("a.png")),
            Some(AgentImage { mime_type: "image/png".into(), data: "AAA".into(), name: Some("a.png".into()) })
        );
        assert_eq!(image_from_data_url("data:text/plain;base64,AAA", None), None);
    }

    #[test]
    fn formats_token_counts() {
        let usage = AgentUsage { used: 83_100, size: 1_000_000, cost: None };
        assert_eq!(usage_label(&usage, &mut I18n::default()).detail, "83.1k / 1M tokens");
    }

    #[test]
    fn names_usage_limit_windows_and_says_when_they_reset() {
        let hour = 3_600_000.0;
        let mut i18n = I18n::default();
        let limit =
            |window: &str, used_percent, resets_ms| AgentLimit { window: window.into(), used_percent, resets_ms };
        assert_eq!(
            limit_label(&limit("five_hour", 19.4, Some(2.0 * hour + 51.0 * 60_000.0)), 0.0, &mut i18n),
            LimitLabel {
                short: "5h 19%".into(),
                percent: 19,
                detail: "5-hour limit: 19% used, resets in 2h 51m".into()
            }
        );
        assert_eq!(
            limit_label(&limit("seven_day", 120.0, Some(5.0 * 24.0 * hour + 3.0 * hour)), 0.0, &mut i18n).detail,
            "Weekly limit: 100% used, resets in 5d 3h"
        );
        assert_eq!(
            limit_label(&limit("spend_limit", 7.0, None), 0.0, &mut i18n),
            LimitLabel { short: "spend_limit 7%".into(), percent: 7, detail: "spend_limit: 7% used".into() }
        );
    }

    #[test]
    fn formats_the_session_cost() {
        assert_eq!(cost_label(&AgentCost { amount: 0.4231, currency: "USD".into() }), "$0.42");
        assert_eq!(cost_label(&AgentCost { amount: 1.5, currency: "credits".into() }), "1.50 credits");
    }

    #[test]
    fn adds_up_the_time_spent_on_turns() {
        let user = |sent_ms: f64, done_ms: Option<f64>| AgentEntry::User {
            text: String::new(),
            attachments: Vec::new(),
            images: Vec::new(),
            sent_ms: Some(sent_ms),
            done_ms,
            steered: false,
        };
        let entries = [user(0.0, Some(5_000.0)), AgentEntry::Agent { text: String::new() }, user(10_000.0, None)];
        assert_eq!(work_time(&entries, Some(12_000.0)), 7_000.0);
        assert_eq!(work_time(&entries, None), 5_000.0);
    }

    #[test]
    fn counts_down_to_the_next_attempt_and_shows_the_limit_when_known() {
        let error = Some("Connection error.".to_owned());
        let retry = AgentRetry { attempt: 3, max_retries: Some(10), retry_at_ms: 5_400.0, error };
        assert_eq!(retry_parts(&retry, 1_000.0), RetryParts { attempt: "3/10".into(), wait: 5 });
        let unlimited = AgentRetry { max_retries: None, ..retry };
        assert_eq!(retry_parts(&unlimited, 9_000.0), RetryParts { attempt: "3".into(), wait: 0 });
    }

    #[test]
    fn tells_a_path_from_a_search() {
        for q in ["../", "..", "./a", "/etc", "../x/y"] {
            assert!(is_path_query(q), "{q}");
        }
        for q in ["", "src/le", ".gitignore", "a/../b"] {
            assert!(!is_path_query(q), "{q}");
        }
    }

    #[test]
    fn resolves_against_the_task_root() {
        assert_eq!(resolve_typed("/r/idea", "../kalo/a.ts"), "/r/kalo/a.ts");
        assert_eq!(resolve_typed("/r/idea", "./src"), "/r/idea/src");
        assert_eq!(resolve_typed("/r/idea", "/etc/hosts"), "/etc/hosts");
        assert_eq!(resolve_typed("C:\\r\\idea", "../kalo/a.ts"), "C:/r/kalo/a.ts");
        assert_eq!(resolve_typed("C:\\r", "../../x"), "C:/x");
    }

    // Not from a spec: the tags composerCommands.test.tsx and sessionState.test.tsx look for.
    #[test]
    fn splits_a_message_into_text_references_and_commands() {
        let tokens = message_tokens("fix @src/a.ts and run /triage");
        let kinds: Vec<(TokenKind, &str)> = tokens.iter().map(|t| (t.kind, t.value.as_str())).collect();
        assert_eq!(
            kinds,
            vec![
                (TokenKind::Text, "fix "),
                (TokenKind::File, "@src/a.ts"),
                (TokenKind::Text, " and run "),
                (TokenKind::Command, "/triage"),
            ]
        );
    }
}
