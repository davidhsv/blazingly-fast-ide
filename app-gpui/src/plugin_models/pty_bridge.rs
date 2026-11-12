//! Terminals a plugin's web frame (or log tab) runs through the host: the frame's `pty-*` messages
//! become spawns, writes, resizes and kills of terminals only that frame owns. Terminal ids never
//! cross into the frame; sessions are named by the frame and checked against strict limits, and a
//! plugin holds at most [`MAX_SESSIONS`] across all its frames ([`PtyQuota`], owned by the caller).
//!
//! Output is sent one chunk at a time: the next waits for the frame's `pty-ack`, and what is queued
//! or unacknowledged is bounded; an exit is reported once the output before it is drained.
//!
//! [`PtyBridge`] calls nothing: it returns [`PtyEffect`]s, and the backend's answers come back with
//! the ticket of the effect that asked.

use super::contribution::utf16_len;
use crate::platform::i18n::I18n;
use ide_core::TerminalId;
use ide_host::plugins::web::WebPtyCommand;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::LazyLock;

pub const MAX_SESSIONS: usize = 8;
/// Bounds in UTF-16 code units.
pub const MAX_INPUT: usize = 64 << 10;
pub const MAX_OUTPUT: usize = 256 << 10;
pub const MAX_MESSAGE: usize = 2 << 20;
/// Exits of terminals whose spawn has not been answered yet, kept for at most this many.
const MAX_EARLY_EXITS: usize = 32;

static SESSION_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.\-]{1,64}$").expect("the session id pattern is valid"));

/// Sessions per plugin, across every bridge of the plugin.
#[derive(Debug, Default)]
pub struct PtyQuota {
    counts: HashMap<String, usize>,
}

impl PtyQuota {
    pub fn count(&self, plugin_id: &str) -> usize {
        self.counts.get(plugin_id).copied().unwrap_or(0)
    }

    fn take(&mut self, plugin_id: &str) -> bool {
        let count = self.counts.entry(plugin_id.to_owned()).or_default();
        if *count >= MAX_SESSIONS {
            return false;
        }
        *count += 1;
        true
    }

    fn give_back(&mut self, plugin_id: &str) {
        if let Some(count) = self.counts.get_mut(plugin_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.counts.remove(plugin_id);
            }
        }
    }
}

/// What the caller does for the bridge.
#[derive(Clone, Debug)]
pub enum PtyEffect {
    /// Post this message to the frame.
    Send(Value),
    /// Start the terminal (the host checks the plugin's process grant); answer with
    /// [`PtyBridge::spawned`], and pass its output to [`PtyBridge::output`] with the same ticket.
    Spawn { ticket: u64, command: WebPtyCommand },
    /// Answer with [`PtyBridge::written`].
    Write { ticket: u64, terminal: TerminalId, data: String },
    /// Answer with [`PtyBridge::resized`].
    Resize { ticket: u64, terminal: TerminalId, cols: u16, rows: u16 },
    /// Kill the terminal; a failure changes nothing.
    Kill(TerminalId),
}

#[derive(Debug, Default)]
struct Session {
    sid: String,
    terminal: Option<TerminalId>,
    stopped: bool,
    released: bool,
    inputs: VecDeque<String>,
    input_size: usize,
    /// The write in flight: its ticket and size.
    writing: Option<(u64, usize)>,
    size: Option<(u16, u16)>,
    resizing: Option<u64>,
    output: VecDeque<String>,
    output_size: usize,
    awaiting: Option<u64>,
    sent_size: usize,
    sequence: u64,
    exit: Option<Option<i32>>,
}

/// The user-visible errors, in the UI language.
#[derive(Debug)]
struct Texts {
    invalid: String,
    too_many: String,
    input_limit: String,
    output_limit: String,
    write_failed: String,
    resize_failed: String,
}

/// The terminals of one frame or log tab.
#[derive(Debug)]
pub struct PtyBridge {
    plugin_id: String,
    /// By spawn ticket: live sessions, and stopped ones whose spawn is not answered yet.
    sessions: HashMap<u64, Session>,
    /// Live session ids → their spawn ticket.
    by_sid: HashMap<String, u64>,
    early_exits: HashMap<TerminalId, Option<i32>>,
    closed: bool,
    next: u64,
    texts: Texts,
}

/// Whether `cols` and `rows` are whole numbers within a terminal's bounds.
fn dimensions(cols: Option<&Value>, rows: Option<&Value>) -> Option<(u16, u16)> {
    let whole = |v: Option<&Value>| v.and_then(Value::as_f64).filter(|n| n.fract() == 0.0);
    let (cols, rows) = (whole(cols)?, whole(rows)?);
    ((2.0..=500.0).contains(&cols) && (1.0..=500.0).contains(&rows)).then_some((cols as u16, rows as u16))
}

/// The command of a `pty-spawn`, when every field is within bounds.
fn spawn_command(m: &Map<String, Value>) -> Option<WebPtyCommand> {
    if m.contains_key("cwd") {
        return None;
    }
    let program = m.get("program")?.as_str()?;
    if program.is_empty()
        || utf16_len(program) > 256
        || program.trim() != program
        || program.contains(['\0', '\r', '\n'])
    {
        return None;
    }
    let empty = Value::Array(Vec::new());
    let args = m.get("args").unwrap_or(&empty).as_array()?;
    if args.len() > 256 {
        return None;
    }
    let args = args
        .iter()
        .map(|a| a.as_str().filter(|a| utf16_len(a) <= 4096 && !a.contains('\0')).map(str::to_owned))
        .collect::<Option<Vec<String>>>()?;
    let no_env = Value::Object(Map::new());
    let env = m.get("env").unwrap_or(&no_env).as_object()?;
    if env.len() > 64 {
        return None;
    }
    let env = env
        .iter()
        .map(|(k, v)| {
            let valid_key = !k.is_empty() && utf16_len(k) <= 256 && !k.contains(['=', '\0']);
            let value = v.as_str().filter(|v| utf16_len(v) <= 4096 && !v.contains('\0'))?;
            valid_key.then(|| (k.clone(), value.to_owned()))
        })
        .collect::<Option<BTreeMap<String, String>>>()?;
    let cols = m.get("cols").cloned().unwrap_or_else(|| json!(100));
    let rows = m.get("rows").cloned().unwrap_or_else(|| json!(28));
    let (cols, rows) = dimensions(Some(&cols), Some(&rows))?;
    Some(WebPtyCommand { program: program.to_owned(), args, env, cols, rows })
}

impl PtyBridge {
    pub fn new(plugin_id: &str, i18n: &I18n) -> Self {
        Self {
            plugin_id: plugin_id.to_owned(),
            sessions: HashMap::new(),
            by_sid: HashMap::new(),
            early_exits: HashMap::new(),
            closed: false,
            next: 0,
            texts: Texts {
                invalid: i18n.tx("Invalid PTY request"),
                too_many: i18n.tx("Too many PTY sessions"),
                input_limit: i18n.tx("PTY input limit exceeded"),
                output_limit: i18n.tx("PTY output limit exceeded"),
                write_failed: i18n.tx("Cannot write to PTY"),
                resize_failed: i18n.tx("Cannot resize PTY"),
            },
        }
    }

    fn error(sid: &str, message: &str) -> PtyEffect {
        PtyEffect::Send(json!({ "op": "pty-error", "sessionId": sid, "error": message }))
    }

    fn ticket(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    fn release(&mut self, key: u64, quota: &mut PtyQuota) {
        if let Some(session) = self.sessions.get_mut(&key)
            && !session.released
        {
            session.released = true;
            quota.give_back(&self.plugin_id);
        }
    }

    /// Stops a session: its terminal is killed and its slot given back; a session whose spawn is
    /// still pending keeps its slot until the terminal it gets is killed.
    fn stop(&mut self, key: u64, quota: &mut PtyQuota, effects: &mut Vec<PtyEffect>) {
        let Some(session) = self.sessions.get_mut(&key) else { return };
        if session.stopped {
            return;
        }
        session.stopped = true;
        session.inputs.clear();
        session.output.clear();
        if self.by_sid.get(&session.sid) == Some(&key) {
            self.by_sid.remove(&session.sid);
        }
        if let Some(terminal) = session.terminal {
            effects.push(PtyEffect::Kill(terminal));
            self.release(key, quota);
            self.sessions.remove(&key);
        }
    }

    fn flush(&mut self, key: u64, quota: &mut PtyQuota, effects: &mut Vec<PtyEffect>) {
        let closed = self.closed;
        let Some(session) = self.sessions.get_mut(&key) else { return };
        if closed || session.stopped || session.awaiting.is_some() {
            return;
        }
        if let Some(data) = session.output.pop_front() {
            session.sent_size = utf16_len(&data);
            session.sequence += 1;
            session.awaiting = Some(session.sequence);
            let (sid, sequence) = (&session.sid, session.sequence);
            let message = json!({ "op": "pty-data", "sessionId": sid, "data": data, "sequence": sequence });
            effects.push(PtyEffect::Send(message));
        } else if let Some(code) = session.exit {
            effects.push(PtyEffect::Send(json!({ "op": "pty-exit", "sessionId": session.sid, "exitCode": code })));
            session.stopped = true;
            let sid = session.sid.clone();
            if self.by_sid.get(&sid) == Some(&key) {
                self.by_sid.remove(&sid);
            }
            self.release(key, quota);
            self.sessions.remove(&key);
        }
    }

    fn pump_writes(&mut self, key: u64, effects: &mut Vec<PtyEffect>) {
        let ticket = self.next + 1;
        let Some(session) = self.sessions.get_mut(&key) else { return };
        let idle = session.writing.is_none() && !session.stopped;
        let Some(terminal) = session.terminal.filter(|_| idle) else { return };
        let Some(data) = session.inputs.pop_front() else { return };
        session.writing = Some((ticket, utf16_len(&data)));
        self.next = ticket;
        effects.push(PtyEffect::Write { ticket, terminal, data });
    }

    fn pump_resizes(&mut self, key: u64, effects: &mut Vec<PtyEffect>) {
        let ticket = self.next + 1;
        let Some(session) = self.sessions.get_mut(&key) else { return };
        let idle = session.resizing.is_none() && !session.stopped;
        let Some(terminal) = session.terminal.filter(|_| idle) else { return };
        let Some((cols, rows)) = session.size.take() else { return };
        session.resizing = Some(ticket);
        self.next = ticket;
        effects.push(PtyEffect::Resize { ticket, terminal, cols, rows });
    }

    /// A message from the frame. Returns whether it was a PTY message (handled here) and what to do.
    pub fn handle(&mut self, text: &str, quota: &mut PtyQuota) -> (bool, Vec<PtyEffect>) {
        let Ok(Value::Object(m)) = serde_json::from_str::<Value>(text) else { return (false, Vec::new()) };
        let Some(op) = m.get("op").and_then(Value::as_str).filter(|op| op.starts_with("pty-")) else {
            return (false, Vec::new());
        };
        let mut effects = Vec::new();
        if self.closed {
            return (true, effects);
        }
        let sid = m.get("sessionId").and_then(Value::as_str).unwrap_or_default();
        if utf16_len(text) > MAX_MESSAGE || !SESSION_ID.is_match(sid) {
            effects.push(Self::error("", &self.texts.invalid));
            return (true, effects);
        }
        let key = self.by_sid.get(sid).copied();
        let live = key.filter(|key| self.sessions.get(key).is_some_and(|s| s.exit.is_none()));
        match op {
            "pty-spawn" => {
                let Some(command) = spawn_command(&m).filter(|_| key.is_none()) else {
                    effects.push(Self::error(sid, &self.texts.invalid));
                    return (true, effects);
                };
                if !quota.take(&self.plugin_id) {
                    effects.push(Self::error(sid, &self.texts.too_many));
                    return (true, effects);
                }
                let ticket = self.ticket();
                self.sessions.insert(ticket, Session { sid: sid.to_owned(), ..Session::default() });
                self.by_sid.insert(sid.to_owned(), ticket);
                effects.push(PtyEffect::Spawn { ticket, command });
            }
            "pty-input" => {
                let Some(data) = m.get("data").and_then(Value::as_str).filter(|d| utf16_len(d) <= MAX_INPUT) else {
                    effects.push(Self::error(sid, &self.texts.invalid));
                    return (true, effects);
                };
                if let Some(key) = live.filter(|_| !data.is_empty())
                    && let Some(session) = self.sessions.get_mut(&key)
                {
                    session.input_size += utf16_len(data);
                    if session.input_size > MAX_INPUT {
                        effects.push(Self::error(sid, &self.texts.input_limit));
                        self.stop(key, quota, &mut effects);
                    } else {
                        session.inputs.push_back(data.to_owned());
                        self.pump_writes(key, &mut effects);
                    }
                }
            }
            "pty-resize" => {
                let Some(size) = dimensions(m.get("cols"), m.get("rows")) else {
                    effects.push(Self::error(sid, &self.texts.invalid));
                    return (true, effects);
                };
                if let Some(key) = live {
                    if let Some(session) = self.sessions.get_mut(&key) {
                        session.size = Some(size);
                    }
                    self.pump_resizes(key, &mut effects);
                }
            }
            "pty-kill" => {
                if let Some(key) = key {
                    self.stop(key, quota, &mut effects);
                }
            }
            "pty-ack" => {
                let sequence = m.get("sequence").and_then(Value::as_u64);
                if let Some(key) = key
                    && let Some(session) = self.sessions.get_mut(&key)
                    && sequence.is_some()
                    && session.awaiting == sequence
                {
                    // The chunk in flight counted against the same bound as the queue.
                    session.output_size = session.output_size.saturating_sub(session.sent_size);
                    session.awaiting = None;
                    self.flush(key, quota, &mut effects);
                }
            }
            _ => effects.push(Self::error(sid, &self.texts.invalid)),
        }
        (true, effects)
    }

    /// The answer to [`PtyEffect::Spawn`] `ticket`: the terminal, or why it could not start.
    pub fn spawned(&mut self, ticket: u64, result: Result<TerminalId, String>, quota: &mut PtyQuota) -> Vec<PtyEffect> {
        let mut effects = Vec::new();
        let closed = self.closed;
        let Some(session) = self.sessions.get_mut(&ticket) else { return effects };
        match result {
            Ok(terminal) => {
                session.terminal = Some(terminal);
                if closed || session.stopped {
                    effects.push(PtyEffect::Kill(terminal));
                    self.release(ticket, quota);
                    self.sessions.remove(&ticket);
                    return effects;
                }
                if let Some(code) = self.early_exits.remove(&terminal) {
                    session.exit = Some(code);
                    self.flush(ticket, quota, &mut effects);
                } else {
                    self.pump_writes(ticket, &mut effects);
                    self.pump_resizes(ticket, &mut effects);
                }
                if self.sessions.values().all(|s| s.terminal.is_some()) {
                    self.early_exits.clear();
                }
            }
            Err(error) => {
                if !closed && !session.stopped {
                    effects.push(Self::error(&session.sid.clone(), &error));
                }
                self.release(ticket, quota);
                self.stop(ticket, quota, &mut effects);
                self.sessions.remove(&ticket);
            }
        }
        effects
    }

    /// Output of the terminal spawned for `ticket` (it may arrive before the spawn is answered).
    pub fn output(&mut self, ticket: u64, data: &str, quota: &mut PtyQuota) -> Vec<PtyEffect> {
        let mut effects = Vec::new();
        let closed = self.closed;
        let Some(session) = self.sessions.get_mut(&ticket) else { return effects };
        if closed || session.stopped || data.is_empty() {
            return effects;
        }
        session.output_size += utf16_len(data);
        if session.output_size > MAX_OUTPUT {
            effects.push(Self::error(&session.sid.clone(), &self.texts.output_limit));
            self.stop(ticket, quota, &mut effects);
            return effects;
        }
        session.output.push_back(data.to_owned());
        self.flush(ticket, quota, &mut effects);
        effects
    }

    /// A terminal exited (any terminal of the app: those of other frames are ignored).
    pub fn exited(&mut self, terminal: TerminalId, code: Option<i32>, quota: &mut PtyQuota) -> Vec<PtyEffect> {
        let mut effects = Vec::new();
        let owner = self.sessions.iter().find(|(_, s)| s.terminal == Some(terminal) && !s.stopped).map(|(k, _)| *k);
        match owner {
            Some(key) => {
                if let Some(session) = self.sessions.get_mut(&key) {
                    session.exit = Some(code);
                }
                self.flush(key, quota, &mut effects);
            }
            None if self.early_exits.len() < MAX_EARLY_EXITS
                && self.sessions.values().any(|s| s.terminal.is_none()) =>
            {
                self.early_exits.insert(terminal, code);
            }
            None => {}
        }
        effects
    }

    /// The answer to [`PtyEffect::Write`]; the next queued input follows.
    pub fn written(&mut self, ticket: u64, result: Result<(), String>, quota: &mut PtyQuota) -> Vec<PtyEffect> {
        let mut effects = Vec::new();
        let Some((&key, session)) = self.sessions.iter_mut().find(|(_, s)| s.writing.is_some_and(|(t, _)| t == ticket))
        else {
            return effects;
        };
        let (_, size) = session.writing.take().unwrap_or_default();
        match result {
            Ok(()) => {
                session.input_size = session.input_size.saturating_sub(size);
                self.pump_writes(key, &mut effects);
            }
            Err(_) => {
                if !session.stopped {
                    effects.push(Self::error(&session.sid.clone(), &self.texts.write_failed));
                }
                self.stop(key, quota, &mut effects);
            }
        }
        effects
    }

    /// The answer to [`PtyEffect::Resize`]; the latest size asked for meanwhile follows.
    pub fn resized(&mut self, ticket: u64, result: Result<(), String>, quota: &mut PtyQuota) -> Vec<PtyEffect> {
        let mut effects = Vec::new();
        let Some((&key, session)) = self.sessions.iter_mut().find(|(_, s)| s.resizing == Some(ticket)) else {
            return effects;
        };
        session.resizing = None;
        match result {
            Ok(()) => self.pump_resizes(key, &mut effects),
            Err(_) => {
                if !session.stopped {
                    effects.push(Self::error(&session.sid.clone(), &self.texts.resize_failed));
                }
                self.stop(key, quota, &mut effects);
            }
        }
        effects
    }

    /// The frame went away: every terminal is killed. Closing again does nothing.
    pub fn close(&mut self, quota: &mut PtyQuota) -> Vec<PtyEffect> {
        let mut effects = Vec::new();
        if self.closed {
            return effects;
        }
        self.closed = true;
        let keys: Vec<u64> = self.sessions.keys().copied().collect();
        for key in keys {
            self.stop(key, quota, &mut effects);
        }
        self.early_exits.clear();
        effects
    }
}

/// Whether plugin `plugin_id` may start `program` now: it is enabled and its `process:<program>`
/// grant is current. Checked again before every spawn, of frame terminals and log tabs alike.
pub fn check_process_grant(
    plugins: &[ide_host::plugins::PluginInfo],
    plugin_id: &str,
    program: &str,
    i18n: &mut I18n,
) -> Result<(), String> {
    let wanted = format!("process:{program}");
    let granted = plugins
        .iter()
        .find(|p| p.id == plugin_id)
        .is_some_and(|p| p.enabled && p.capabilities.iter().any(|c| c.id == wanted && c.granted));
    if granted {
        return Ok(());
    }
    let strings = ["Cannot start PTY: process:", " capability not granted."];
    Err(i18n.tx_built(&strings, &[crate::platform::i18n::Param::Text(program.to_owned())]))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Frame {
        bridge: PtyBridge,
        sent: Vec<Value>,
        spawns: Vec<u64>,
        writes: Vec<(u64, TerminalId, String)>,
        resizes: Vec<(u64, TerminalId, u16, u16)>,
        kills: Vec<TerminalId>,
    }

    impl Frame {
        fn new(plugin: &str) -> Self {
            Self {
                bridge: PtyBridge::new(plugin, &I18n::default()),
                sent: Vec::new(),
                spawns: Vec::new(),
                writes: Vec::new(),
                resizes: Vec::new(),
                kills: Vec::new(),
            }
        }

        fn take(&mut self, effects: Vec<PtyEffect>) {
            for effect in effects {
                match effect {
                    PtyEffect::Send(message) => self.sent.push(message),
                    PtyEffect::Spawn { ticket, .. } => self.spawns.push(ticket),
                    PtyEffect::Write { ticket, terminal, data } => self.writes.push((ticket, terminal, data)),
                    PtyEffect::Resize { ticket, terminal, cols, rows } => {
                        self.resizes.push((ticket, terminal, cols, rows));
                    }
                    PtyEffect::Kill(terminal) => self.kills.push(terminal),
                }
            }
        }

        fn raw(&mut self, text: &str, quota: &mut PtyQuota) -> bool {
            let (handled, effects) = self.bridge.handle(text, quota);
            self.take(effects);
            handled
        }

        /// Sends `message` for session `s` unless it names its own.
        fn send(&mut self, message: Value, quota: &mut PtyQuota) {
            let mut m = json!({ "sessionId": "s" });
            m.as_object_mut().expect("an object").extend(message.as_object().expect("an object").clone());
            self.raw(&m.to_string(), quota);
        }

        fn spawn(&mut self, quota: &mut PtyQuota) {
            self.send(json!({ "op": "pty-spawn", "program": "bash", "args": ["-i"] }), quota);
        }

        fn spawned(&mut self, ticket: u64, terminal: u32, quota: &mut PtyQuota) {
            let effects = self.bridge.spawned(ticket, Ok(TerminalId(terminal)), quota);
            self.take(effects);
        }

        fn output(&mut self, ticket: u64, data: &str, quota: &mut PtyQuota) {
            let effects = self.bridge.output(ticket, data, quota);
            self.take(effects);
        }

        fn last_error(&self) -> Option<&str> {
            self.sent.last().and_then(|m| m.get("error")).and_then(Value::as_str)
        }
    }

    #[test]
    fn rejects_malformed_types_sizes_paths_and_dimensions_without_starting_a_process() {
        let mut quota = PtyQuota::default();
        let mut f = Frame::new("acme.pty");
        let long = "x".repeat(4097);
        let bad = [
            json!({ "program": "\0" }),
            json!({ "program": " bash" }),
            json!({ "args": [1] }),
            json!({ "args": [long] }),
            json!({ "args": vec!["x"; 257] }),
            json!({ "args": ["\0"] }),
            json!({ "cols": -1 }),
            json!({ "rows": 501 }),
            json!({ "cols": 2.5 }),
            json!({ "cols": "80" }),
            json!({ "env": [] }),
            json!({ "env": { "PATH": 1 } }),
            json!({ "env": { "a=b": "v" } }),
            json!({ "env": { "a": "\0" } }),
            json!({ "cwd": "/" }),
            json!({ "sessionId": "x".repeat(65) }),
        ];
        for fields in bad {
            let mut message = json!({ "op": "pty-spawn", "program": "bash" });
            message.as_object_mut().expect("an object").extend(fields.as_object().expect("an object").clone());
            f.send(message, &mut quota);
        }
        assert!(f.spawns.is_empty());
        assert!(!f.raw("null", &mut quota));
        assert!(!f.raw("[1]", &mut quota));
        assert!(!f.raw("garbage", &mut quota));
        let state = json!({ "op": "plugin-state", "data": "x".repeat(3 << 20) }).to_string();
        assert!(!f.raw(&state, &mut quota));
        f.send(json!({ "op": "pty-spawn", "program": "bash", "padding": "x".repeat(3 << 20) }), &mut quota);
        assert!(f.spawns.is_empty());
        assert!(f.sent.iter().all(|m| m["op"] == "pty-error"));
        assert_eq!(f.sent.len(), 17);
    }

    #[test]
    fn caps_sessions_across_frames_of_one_plugin_including_cancelled_pending_starts() {
        let mut quota = PtyQuota::default();
        let (mut a, mut b, mut other) = (Frame::new("acme.pty"), Frame::new("acme.pty"), Frame::new("other.pty"));
        for i in 0..8 {
            a.send(json!({ "op": "pty-spawn", "program": "bash", "sessionId": format!("s{i}") }), &mut quota);
        }
        b.spawn(&mut quota);
        assert!(b.spawns.is_empty());
        a.send(json!({ "op": "pty-kill", "sessionId": "s7" }), &mut quota);
        b.spawn(&mut quota);
        assert!(b.spawns.is_empty(), "a cancelled pending start keeps its slot");
        other.spawn(&mut quota);
        assert_eq!(other.spawns.len(), 1);
        a.spawned(a.spawns[7], 77, &mut quota);
        assert_eq!(a.kills, vec![TerminalId(77)]);
        b.spawn(&mut quota);
        assert_eq!(b.spawns.len(), 1);
    }

    #[test]
    fn serializes_bounded_pending_and_in_flight_input_and_coalesces_resizes() {
        let mut quota = PtyQuota::default();
        let mut f = Frame::new("acme.pty");
        f.spawn(&mut quota);
        f.send(json!({ "op": "pty-input", "data": "hello" }), &mut quota);
        f.send(json!({ "op": "pty-resize", "cols": 90, "rows": 30 }), &mut quota);
        f.send(json!({ "op": "pty-resize", "cols": 100, "rows": 40 }), &mut quota);
        f.spawned(f.spawns[0], 1, &mut quota);
        let writes: Vec<_> = f.writes.iter().map(|(_, t, d)| (*t, d.as_str())).collect();
        assert_eq!(writes, vec![(TerminalId(1), "hello")]);
        let resizes: Vec<_> = f.resizes.iter().map(|(_, t, c, r)| (*t, *c, *r)).collect();
        assert_eq!(resizes, vec![(TerminalId(1), 100, 40)]);
        let effects = f.bridge.written(f.writes[0].0, Ok(()), &mut quota);
        f.take(effects);
        f.send(json!({ "op": "pty-input", "data": "x".repeat(64 << 10) }), &mut quota);
        f.send(json!({ "op": "pty-input", "data": "x" }), &mut quota);
        assert_eq!(f.kills, vec![TerminalId(1)]);
        assert_eq!(f.last_error(), Some("PTY input limit exceeded"));
    }

    #[test]
    fn never_lets_one_frame_write_resize_or_kill_another_frames_terminals() {
        let mut quota = PtyQuota::default();
        let (mut a, mut b) = (Frame::new("ownership.pty"), Frame::new("ownership.pty"));
        a.spawn(&mut quota);
        a.spawned(a.spawns[0], 1, &mut quota);
        b.send(json!({ "op": "pty-input", "data": "bad" }), &mut quota);
        b.send(json!({ "op": "pty-resize", "cols": 80, "rows": 24 }), &mut quota);
        b.send(json!({ "op": "pty-kill" }), &mut quota);
        assert!(b.writes.is_empty() && b.resizes.is_empty() && b.kills.is_empty());
        assert!(a.writes.is_empty() && a.kills.is_empty());
        a.send(json!({ "op": "pty-input", "data": "yes" }), &mut quota);
        assert_eq!(a.writes[0].2, "yes");
    }

    #[test]
    fn allows_one_unacknowledged_chunk_and_caps_all_outstanding_output() {
        let mut quota = PtyQuota::default();
        let mut f = Frame::new("output.pty");
        f.spawn(&mut quota);
        let ticket = f.spawns[0];
        f.spawned(ticket, 1, &mut quota);
        f.output(ticket, "hello", &mut quota);
        f.output(ticket, "world", &mut quota);
        assert_eq!(f.sent.len(), 1);
        f.send(json!({ "op": "pty-ack", "sequence": 999 }), &mut quota);
        assert_eq!(f.sent.len(), 1);
        f.send(json!({ "op": "pty-ack", "sequence": 1 }), &mut quota);
        let last = f.sent.last().expect("a message");
        assert_eq!((last["data"].as_str(), last["sequence"].as_u64()), (Some("world"), Some(2)));
        f.output(ticket, &"x".repeat(256 << 10), &mut quota);
        assert_eq!(f.kills, vec![TerminalId(1)]);
        assert_eq!(f.last_error(), Some("PTY output limit exceeded"));
    }

    #[test]
    fn handles_output_and_exit_before_spawn_resolves_and_drains_before_reporting_exit() {
        let mut quota = PtyQuota::default();
        let mut f = Frame::new("early.pty");
        f.spawn(&mut quota);
        let ticket = f.spawns[0];
        f.output(ticket, "early", &mut quota);
        let effects = f.bridge.exited(TerminalId(42), Some(7), &mut quota);
        f.take(effects);
        f.spawned(ticket, 42, &mut quota);
        assert_eq!(f.sent.len(), 1);
        f.send(json!({ "op": "pty-ack", "sequence": 1 }), &mut quota);
        assert_eq!(f.sent.last(), Some(&json!({ "op": "pty-exit", "sessionId": "s", "exitCode": 7 })));
        f.spawn(&mut quota);
        assert_eq!(f.spawns.len(), 2);
        assert_eq!(quota.count("early.pty"), 1);
    }

    #[test]
    fn cleans_failed_and_late_spawns_catches_operation_failures_and_closes_idempotently() {
        let mut quota = PtyQuota::default();
        let mut f = Frame::new("cleanup.pty");
        f.spawn(&mut quota);
        let effects = f.bridge.spawned(f.spawns[0], Err("denied".to_owned()), &mut quota);
        f.take(effects);
        assert_eq!(f.last_error(), Some("denied"));
        f.spawn(&mut quota);
        assert_eq!(f.spawns.len(), 2);
        f.spawned(f.spawns[1], 1, &mut quota);
        f.send(json!({ "op": "pty-resize", "cols": 80, "rows": 24 }), &mut quota);
        let effects = f.bridge.resized(f.resizes[0].0, Err("gone".to_owned()), &mut quota);
        f.take(effects);
        assert_eq!(f.kills, vec![TerminalId(1)]);
        f.spawn(&mut quota);
        let effects = f.bridge.close(&mut quota);
        f.take(effects);
        let effects = f.bridge.close(&mut quota);
        f.take(effects);
        f.spawned(f.spawns[2], 2, &mut quota);
        assert_eq!(f.kills, vec![TerminalId(1), TerminalId(2)]);
        f.spawn(&mut quota);
        assert_eq!(f.spawns.len(), 3);
        assert_eq!(quota.count("cleanup.pty"), 0);
    }

    #[test]
    fn uses_the_plugin_scoped_backend_for_both_log_and_frame_streams_rechecking_the_current_grant() {
        use crate::plugin_models::test_support::{capability, plugin as listed};
        let plugin = |enabled: bool, granted: bool| {
            let capabilities = if granted { vec![capability("process:bash", true)] } else { Vec::new() };
            listed("probe", json!({ "enabled": enabled, "capabilities": capabilities }))
        };
        let mut i18n = I18n::default();
        assert_eq!(check_process_grant(&[plugin(true, true)], "probe", "bash", &mut i18n), Ok(()));
        for current in [plugin(true, false), plugin(false, true)] {
            let refused = check_process_grant(std::slice::from_ref(&current), "probe", "bash", &mut i18n);
            assert!(refused.is_err_and(|e| e.contains("capability not granted")));
        }
    }
}
