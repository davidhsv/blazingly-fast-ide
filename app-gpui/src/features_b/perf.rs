//! The memory indicator, "Clear caches" and the dev performance overlay: IDEA-style compact numbers,
//! a rolling window of latency samples with percentiles, keystroke latencies from event timings,
//! polling that pauses while the window is hidden, and the memory breakdown of the indicator's popup.
//!
//! Nothing here calls the backend: the caller fetches the metrics and hands them to [`PerfStore`];
//! times are passed in.

use crate::platform::i18n::{I18n, Param};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_language::SyntaxCacheStats;
use ide_workspace::memory::{MemoryCategory, ProcessMemory};
use serde::Deserialize;

const KB: f64 = 1024.0;
const MB: f64 = KB * 1024.0;
const GB: f64 = MB * 1024.0;

/// Shown for a value that is unknown.
pub const UNKNOWN: &str = "–";

/// One decimal, without a trailing `.0`.
fn trim(n: f64) -> String {
    let fixed = format!("{n:.1}");
    match fixed.strip_suffix(".0") {
        Some(whole) => whole.to_owned(),
        None => fixed,
    }
}

/// Rounds half up, as the formatting of the other views does (`-2.5` gives `-2`).
fn round(n: f64) -> f64 {
    (n + 0.5).floor()
}

fn known(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite())
}

/// `512B`, `12K`, `412M`, `1.9G`, `2G` (binary units, like IDEA's indicator).
pub fn format_bytes(bytes: Option<f64>) -> String {
    let Some(bytes) = known(bytes).filter(|b| *b >= 0.0) else { return UNKNOWN.to_owned() };
    if bytes < KB {
        return format!("{}B", round(bytes));
    }
    if bytes < MB {
        return format!("{}K", round(bytes / KB));
    }
    // Rounded first so 1023.6M shows as 1G, not 1024M.
    let m = round(bytes / MB);
    if m < 1024.0 { format!("{m}M") } else { format!("{}G", trim(bytes / GB)) }
}

/// `560 MB`, `1.1 GB`, `12 KB` (binary units, as Activity Monitor and the Task Manager show them).
pub fn format_size(bytes: Option<f64>) -> String {
    let Some(bytes) = known(bytes).filter(|b| *b >= 0.0) else { return UNKNOWN.to_owned() };
    if bytes < KB {
        return format!("{} B", round(bytes));
    }
    if bytes < MB {
        return format!("{} KB", round(bytes / KB));
    }
    let m = round(bytes / MB);
    if m < 1024.0 { format!("{m} MB") } else { format!("{} GB", trim(bytes / GB)) }
}

/// [`format_size`] of a byte count the backend reports.
pub fn format_size_of(bytes: Option<u64>) -> String {
    format_size(bytes.map(|b| b as f64))
}

/// `0.4 ms`, `16 ms`, `2.3 s`.
pub fn format_ms(ms: Option<f64>) -> String {
    let Some(ms) = known(ms) else { return UNKNOWN.to_owned() };
    if ms < 10.0 {
        format!("{} ms", trim(ms))
    } else if ms < 1000.0 {
        format!("{} ms", round(ms))
    } else {
        format!("{} s", trim(ms / 1000.0))
    }
}

/// `42s`, `5m 03s`, `2h 07m`, `3d 4h`.
pub fn format_uptime(ms: Option<f64>) -> String {
    let Some(ms) = known(ms).filter(|ms| *ms >= 0.0) else { return UNKNOWN.to_owned() };
    let s = (ms / 1000.0).floor() as u64;
    if s < 60 {
        return format!("{s}s");
    }
    let m = s / 60;
    if m < 60 {
        return format!("{m}m {:02}s", s % 60);
    }
    let h = m / 60;
    if h < 24 {
        return format!("{h}h {:02}m", m % 60);
    }
    format!("{}d {}h", h / 24, h % 24)
}

/// `1,234` style grouping, the same in every locale.
pub fn format_count(n: Option<f64>) -> String {
    let Some(n) = known(n) else { return UNKNOWN.to_owned() };
    let rounded = round(n);
    let digits = format!("{}", rounded.abs());
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    if rounded < 0.0 { format!("-{grouped}") } else { grouped }
}

// ---------------------------------------------------------------------------
// Latency samples.

/// Percentiles of a [`RollingWindow`].
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub count: usize,
    pub last: Option<f64>,
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub max: Option<f64>,
    /// Samples above the budget passed to [`RollingWindow::summary`].
    pub over_budget: usize,
}

/// Nearest-rank percentile of `sorted` (ascending); `None` when empty.
pub fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (p.clamp(0.0, 100.0) / 100.0 * sorted.len() as f64).ceil() as usize;
    sorted.get(rank.max(1) - 1).copied()
}

/// The last `capacity` samples, and how many were ever pushed.
#[derive(Debug, Clone, PartialEq)]
pub struct RollingWindow {
    capacity: usize,
    buf: Vec<f64>,
    next: usize,
    total: usize,
    last: Option<f64>,
}

impl Default for RollingWindow {
    fn default() -> Self {
        Self::new(200)
    }
}

impl RollingWindow {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self { capacity, buf: Vec::with_capacity(capacity), next: 0, total: 0, last: None }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Adds a sample; values that are not finite are ignored.
    pub fn push(&mut self, value: f64) {
        if !value.is_finite() {
            return;
        }
        self.total += 1;
        self.last = Some(value);
        if self.buf.len() < self.capacity {
            self.buf.push(value);
        } else {
            self.buf[self.next] = value;
        }
        self.next = (self.next + 1) % self.capacity;
    }

    /// Samples ever pushed (not just those retained).
    pub fn pushed(&self) -> usize {
        self.total
    }

    pub fn size(&self) -> usize {
        self.buf.len()
    }

    pub fn last(&self) -> Option<f64> {
        self.last
    }

    pub fn values(&self) -> Vec<f64> {
        self.buf.clone()
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.next = 0;
        self.total = 0;
        self.last = None;
    }

    /// The window's percentiles; `budget` counts the samples above it (`f64::INFINITY`: none).
    pub fn summary(&self, budget: f64) -> Summary {
        let mut sorted = self.buf.clone();
        sorted.sort_by(f64::total_cmp);
        Summary {
            count: sorted.len(),
            last: self.last,
            p50: percentile(&sorted, 50.0),
            p95: percentile(&sorted, 95.0),
            max: sorted.last().copied(),
            over_budget: sorted.iter().filter(|v| **v > budget).count(),
        }
    }
}

// ---------------------------------------------------------------------------
// Keystroke → paint latency.

/// Input events that make up typing in an editor.
pub const TYPING_EVENTS: [&str; 4] = ["keydown", "keypress", "beforeinput", "input"];

/// Frame budget (60 Hz) used for the "over budget" count.
pub const FRAME_BUDGET_MS: f64 = 16.0;

/// An event timing: its event name, the input-to-next-frame duration, whether its target is inside a
/// text editor (a text node counts by its parent), and the interaction it belongs to (0: none).
#[derive(Debug, Clone, PartialEq)]
pub struct EventEntry {
    pub name: String,
    pub duration: f64,
    pub in_editor: bool,
    pub interaction_id: u32,
}

/// Whether an event timing is a keystroke in a text editor.
pub fn is_editor_typing_entry(entry: &EventEntry) -> bool {
    TYPING_EVENTS.contains(&entry.name.as_str()) && entry.in_editor
}

/// Collapses the entries of one keystroke (keydown, beforeinput and input share an interaction) into one
/// sample: the longest duration per interaction, in the order interactions first appear. Entries without
/// an interaction are kept individually, after them.
pub fn keystroke_latencies(entries: &[EventEntry]) -> Vec<f64> {
    let mut by_interaction: Vec<(u32, f64)> = Vec::new();
    let mut loose = Vec::new();
    for entry in entries.iter().filter(|entry| is_editor_typing_entry(entry)) {
        if entry.interaction_id == 0 {
            loose.push(entry.duration);
            continue;
        }
        match by_interaction.iter_mut().find(|(id, _)| *id == entry.interaction_id) {
            Some((_, longest)) => *longest = longest.max(entry.duration),
            None => by_interaction.push((entry.interaction_id, entry.duration.max(0.0))),
        }
    }
    by_interaction.into_iter().map(|(_, duration)| duration).chain(loose).collect()
}

/// Whether the timing source supports `entry_type`; `supported` is `None` when there is no timing source.
pub fn supports_entry_type(supported: Option<&[&str]>, entry_type: &str) -> bool {
    supported.is_some_and(|types| types.contains(&entry_type))
}

// ---------------------------------------------------------------------------
// Polling.

/// Interval polling that pauses while the window is hidden: a tick at once when it starts or shows
/// again, then one every `interval_ms`. The caller asks [`Polling::advance`] how many ticks are due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Polling {
    interval_ms: u64,
    /// When the next tick is due; `None` while paused or stopped.
    next_due: Option<u64>,
    stopped: bool,
}

impl Polling {
    /// Starts polling; returns whether to tick now (the window is visible).
    pub fn start(interval_ms: u64, visible: bool, now: u64) -> (Self, bool) {
        let mut polling = Self { interval_ms: interval_ms.max(1), next_due: None, stopped: false };
        let tick = visible && polling.resume(now);
        (polling, tick)
    }

    fn resume(&mut self, now: u64) -> bool {
        if self.stopped || self.next_due.is_some() {
            return false;
        }
        self.next_due = Some(now + self.interval_ms);
        true
    }

    /// The window was shown or hidden; returns whether to tick now.
    pub fn visibility_changed(&mut self, visible: bool, now: u64) -> bool {
        if visible {
            self.resume(now)
        } else {
            self.next_due = None;
            false
        }
    }

    /// The ticks due by `now`.
    pub fn advance(&mut self, now: u64) -> u32 {
        let mut ticks = 0;
        while let Some(due) = self.next_due.filter(|due| *due <= now) {
            ticks += 1;
            self.next_due = Some(due + self.interval_ms);
        }
        ticks
    }

    /// When the next tick is due (for the caller's timer); `None` while paused or stopped.
    pub fn next_due(&self) -> Option<u64> {
        self.next_due
    }

    pub fn stop(&mut self) {
        self.stopped = true;
        self.next_due = None;
    }
}

// ---------------------------------------------------------------------------
// Memory breakdown.

/// Start-up timings of the backend.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupMetrics {
    pub process_to_main_ms: Option<f64>,
    pub process_to_setup_ms: Option<f64>,
    pub setup_ms: f64,
    pub first_ipc_ms: Option<f64>,
}

/// Process memory and subsystem sizes, as the backend's `app_metrics` reports them.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppMetrics {
    /// Resident memory of the backend process.
    pub rss_bytes: Option<u64>,
    /// Bytes allocated through malloc in the backend process (macOS only).
    pub heap_bytes: Option<u64>,
    pub total_memory_bytes: Option<u64>,
    pub uptime_ms: f64,
    pub startup: StartupMetrics,
    pub projects: u32,
    pub open_documents: u32,
    pub index_files: u32,
    pub index_symbols: u32,
    pub indexing: bool,
    pub language_servers: u32,
    pub syntax: SyntaxCacheStats,
    /// Footprint per process group.
    pub memory: Vec<ProcessMemory>,
}

/// What "Clear caches" freed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearCachesResult {
    pub syntax_trees: u32,
    pub rss_before_bytes: Option<u64>,
    pub rss_after_bytes: Option<u64>,
}

/// Memory totals per group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySummary {
    /// The backend and its own processes.
    pub app: Option<u64>,
    pub language_servers: Option<u64>,
    pub processes: Option<u64>,
    pub total: Option<u64>,
}

/// The sum of the rows whose size is known; `None` when none is.
fn sum<'a>(rows: impl IntoIterator<Item = &'a ProcessMemory>) -> Option<u64> {
    rows.into_iter().filter_map(|row| row.bytes).reduce(|a, b| a + b)
}

fn by_category(rows: &[ProcessMemory], category: MemoryCategory) -> impl Iterator<Item = &ProcessMemory> {
    rows.iter().filter(move |row| row.category == category)
}

/// Totals of `memory`; without an app breakdown the backend's resident memory stands for the app.
pub fn summarize_memory(memory: &[ProcessMemory], rss_bytes: Option<u64>) -> MemorySummary {
    let app = sum(by_category(memory, MemoryCategory::App)).or(rss_bytes);
    let language_servers = sum(by_category(memory, MemoryCategory::LanguageServer));
    let processes = sum(by_category(memory, MemoryCategory::Process));
    let total = [app, language_servers, processes].into_iter().flatten().reduce(|a, b| a + b);
    MemorySummary { app, language_servers, processes, total }
}

/// [`summarize_memory`] of fetched metrics (`None`: not fetched yet).
pub fn summarize_metrics(metrics: Option<&AppMetrics>) -> MemorySummary {
    metrics.map_or_else(|| summarize_memory(&[], None), |m| summarize_memory(&m.memory, m.rss_bytes))
}

/// Status bar text: `OpenIDEA 560M` (no denominator: there is no heap limit to compare with).
pub fn indicator_label(summary: &MemorySummary) -> String {
    match summary.app {
        None => format!("OpenIDEA {UNKNOWN}"),
        Some(app) => format!("OpenIDEA {}", format_bytes(Some(app as f64))),
    }
}

/// Status bar tooltip: `OpenIDEA 560 MB · language servers 1.1 GB · processes 80 MB`.
pub fn indicator_tooltip(summary: &MemorySummary) -> String {
    let mut parts = vec![format!("OpenIDEA {}", format_size_of(summary.app))];
    if let Some(bytes) = summary.language_servers {
        parts.push(format!("language servers {}", format_size_of(Some(bytes))));
    }
    if let Some(bytes) = summary.processes {
        parts.push(format!("processes {}", format_size_of(Some(bytes))));
    }
    parts.join(" · ")
}

/// A row of the popup's breakdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakdownRow {
    pub key: String,
    pub label: String,
    /// Project, windows, run kind…
    pub detail: Option<String>,
    pub value: String,
    pub processes: u32,
}

/// A section of the popup's breakdown; `title` is English source text for the view to translate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakdownSection {
    pub title: &'static str,
    pub total: String,
    pub rows: Vec<BreakdownRow>,
}

const SECTIONS: [(MemoryCategory, &str, &str); 3] = [
    (MemoryCategory::App, "app", "OpenIDEA"),
    (MemoryCategory::LanguageServer, "languageServer", "Language servers"),
    (MemoryCategory::Process, "process", "Debug adapters & processes"),
];

/// Popup sections, one per non-empty category, rows in backend order.
pub fn memory_sections(rows: &[ProcessMemory]) -> Vec<BreakdownSection> {
    SECTIONS
        .iter()
        .filter_map(|(category, key, title)| {
            let items: Vec<&ProcessMemory> = by_category(rows, *category).collect();
            if items.is_empty() {
                return None;
            }
            Some(BreakdownSection {
                title,
                total: format_size_of(sum(items.iter().copied())),
                rows: items
                    .iter()
                    .enumerate()
                    .map(|(i, row)| BreakdownRow {
                        key: format!("{key}:{i}:{}", row.label),
                        label: row.label.clone(),
                        detail: row.detail.clone(),
                        value: format_size_of(row.bytes),
                        processes: row.processes,
                    })
                    .collect(),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Store and actions.

/// Poll interval of the memory indicator.
pub const METRICS_INTERVAL_MS: u64 = 5000;

/// Ids of the feature's actions, in registration order.
pub const ACTION_IDS: [&str; 4] =
    ["perf.clearCaches", "perf.showMemory", "perf.toggleMemoryIndicator", "perf.toggleOverlay"];

/// Id of the memory indicator in the status bar (hidden by default, far right).
pub const MEMORY_WIDGET: &str = "perf.memory";

const CLEAR_CACHES_KEY: &str = "perf.clearCaches";

/// Metrics shared by the status item, its popup and the dev overlay.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PerfStore {
    pub metrics: Option<AppMetrics>,
    /// The last metrics fetch failed (shown in the popup).
    pub error: Option<String>,
    pub popup_open: bool,
    pub overlay_open: bool,
}

impl PerfStore {
    /// The answer to a metrics fetch: new metrics clear the error; a failure keeps the last metrics.
    pub fn metrics_loaded(&mut self, answer: Result<AppMetrics, String>) {
        match answer {
            Ok(metrics) => {
                self.metrics = Some(metrics);
                self.error = None;
            }
            Err(message) => self.error = Some(message),
        }
    }

    /// "Show Memory Indicator Details": after the indicator is shown and the metrics refreshed.
    pub fn open_popup(&mut self) {
        self.popup_open = true;
    }

    pub fn close_popup(&mut self) {
        self.popup_open = false;
    }

    /// "Toggle Performance Overlay (Dev)".
    pub fn toggle_overlay(&mut self) {
        self.overlay_open = !self.overlay_open;
    }

    /// The memory totals of the last metrics.
    pub fn summary(&self) -> MemorySummary {
        summarize_metrics(self.metrics.as_ref())
    }
}

/// The notification "Clear caches" ends with: `Ok(None)` when there is no backend with caches to clear.
/// The caller refreshes the metrics after a successful clear.
pub fn clear_caches_notification(
    answer: Result<Option<ClearCachesResult>, String>,
    i18n: &mut I18n,
) -> NotificationRequest {
    let mut request = match answer {
        Ok(None) => NotificationRequest::new(NotificationLevel::Info, "No backend caches to clear"),
        Ok(Some(result)) => {
            let memory = match (result.rss_before_bytes, result.rss_after_bytes) {
                (Some(before), Some(after)) => i18n.tx_built(
                    &[" Memory ", " → ", "."],
                    &[Param::from(format_size_of(Some(before))), Param::from(format_size_of(Some(after)))],
                ),
                _ => String::new(),
            };
            let strings: [&str; 3] = if result.syntax_trees == 1 {
                ["", " syntax tree dropped, index compacted.", ""]
            } else {
                ["", " syntax trees dropped, index compacted.", ""]
            };
            let body = i18n.tx_built(&strings, &[Param::from(f64::from(result.syntax_trees)), Param::from(memory)]);
            let mut request = NotificationRequest::new(NotificationLevel::Info, "Caches cleared");
            request.body = Some(body);
            request
        }
        Err(message) => {
            let mut request = NotificationRequest::new(NotificationLevel::Error, "Could not clear caches");
            request.body = Some(message);
            request
        }
    };
    request.key = Some(CLEAR_CACHES_KEY.to_owned());
    request
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mb(n: f64) -> f64 {
        n * MB
    }

    #[test]
    fn formats_bytes_like_ideas_memory_indicator() {
        assert_eq!(format_bytes(Some(mb(412.0))), "412M");
        assert_eq!(format_bytes(Some(2.0 * GB)), "2G");
        assert_eq!(format_bytes(Some(1.94 * GB)), "1.9G");
        assert_eq!(format_bytes(Some(mb(1023.7))), "1G");
        assert_eq!(format_bytes(Some(512.0)), "512B");
        assert_eq!(format_bytes(Some(12.0 * 1024.0)), "12K");
        assert_eq!(format_bytes(None), "–");
        assert_eq!(format_bytes(Some(-1.0)), "–");
    }

    #[test]
    fn formats_sizes_for_tooltips_and_the_breakdown() {
        assert_eq!(format_size(Some(mb(560.0))), "560 MB");
        assert_eq!(format_size(Some(1.1 * GB)), "1.1 GB");
        assert_eq!(format_size(Some(12.0 * 1024.0)), "12 KB");
        assert_eq!(format_size(None), "–");
    }

    #[test]
    fn formats_durations_and_counts() {
        assert_eq!(format_ms(Some(0.42)), "0.4 ms");
        assert_eq!(format_ms(Some(16.4)), "16 ms");
        assert_eq!(format_ms(Some(2300.0)), "2.3 s");
        assert_eq!(format_ms(None), "–");
        assert_eq!(format_uptime(Some(42_000.0)), "42s");
        assert_eq!(format_uptime(Some(303_000.0)), "5m 03s");
        assert_eq!(format_uptime(Some(f64::from((2 * 60 + 7) * 60_000))), "2h 07m");
        assert_eq!(format_uptime(Some(76.0 * 3_600_000.0)), "3d 4h");
        assert_eq!(format_count(Some(1_234_567.0)), "1,234,567");
    }

    #[test]
    fn computes_nearest_rank_percentiles() {
        let sorted: Vec<f64> = (1..=10).map(f64::from).collect();
        assert_eq!(percentile(&sorted, 50.0), Some(5.0));
        assert_eq!(percentile(&sorted, 95.0), Some(10.0));
        assert_eq!(percentile(&sorted, 0.0), Some(1.0));
        assert_eq!(percentile(&[], 50.0), None);
    }

    #[test]
    fn keeps_a_rolling_window() {
        let mut window = RollingWindow::new(3);
        assert_eq!(window.summary(f64::INFINITY).last, None);
        for value in [5.0, 1.0, 9.0, 30.0] {
            window.push(value);
        }
        window.push(f64::NAN);
        assert_eq!(window.size(), 3);
        assert_eq!(window.pushed(), 4);
        let mut values = window.values();
        values.sort_by(f64::total_cmp);
        assert_eq!(values, [1.0, 9.0, 30.0]);
        let summary = window.summary(16.0);
        assert_eq!(summary.count, 3);
        assert_eq!(summary.last, Some(30.0));
        assert_eq!(summary.p50, Some(9.0));
        assert_eq!(summary.max, Some(30.0));
        assert_eq!(summary.over_budget, 1);
        window.clear();
        assert_eq!(window.summary(f64::INFINITY).count, 0);
    }

    fn entry(name: &str, duration: f64, in_editor: bool, interaction_id: u32) -> EventEntry {
        EventEntry { name: name.to_owned(), duration, in_editor, interaction_id }
    }

    #[test]
    fn keeps_only_typing_events_inside_codemirror() {
        assert!(is_editor_typing_entry(&entry("keydown", 20.0, true, 0)));
        assert!(is_editor_typing_entry(&entry("input", 20.0, true, 0)));
        assert!(!is_editor_typing_entry(&entry("keydown", 20.0, false, 0)));
        assert!(!is_editor_typing_entry(&entry("click", 20.0, true, 0)));
    }

    #[test]
    fn collapses_one_keystrokes_entries_into_its_slowest() {
        let mut latencies = keystroke_latencies(&[
            entry("keydown", 18.0, true, 7),
            entry("beforeinput", 24.0, true, 7),
            entry("keydown", 40.0, true, 8),
            entry("input", 17.0, true, 0),
            entry("pointerdown", 90.0, true, 9),
        ]);
        latencies.sort_by(f64::total_cmp);
        assert_eq!(latencies, [17.0, 24.0, 40.0]);
    }

    #[test]
    fn feature_detects_entry_types() {
        assert!(supports_entry_type(Some(&["event"][..]), "event"));
        assert!(!supports_entry_type(Some(&["event"][..]), "longtask"));
        assert!(!supports_entry_type(None, "event"));
    }

    #[test]
    fn pauses_while_the_document_is_hidden() {
        let (mut polling, tick) = Polling::start(1000, true, 0);
        let mut ticks = u32::from(tick);
        assert_eq!(ticks, 1);
        ticks += polling.advance(2000);
        assert_eq!(ticks, 3);
        ticks += u32::from(polling.visibility_changed(false, 2000));
        ticks += polling.advance(7000);
        assert_eq!(ticks, 3);
        ticks += u32::from(polling.visibility_changed(true, 7000));
        assert_eq!(ticks, 4);
        polling.stop();
        ticks += polling.advance(12_000);
        assert_eq!(ticks, 4);
        assert_eq!(polling.next_due(), None);
        assert!(!polling.visibility_changed(true, 12_000));
    }

    fn row(category: MemoryCategory, label: &str, mbs: Option<u64>, detail: Option<&str>) -> ProcessMemory {
        ProcessMemory {
            category,
            label: label.to_owned(),
            detail: detail.map(str::to_owned),
            processes: 1,
            bytes: mbs.map(|n| n * 1024 * 1024),
        }
    }

    fn rows() -> Vec<ProcessMemory> {
        vec![
            row(MemoryCategory::App, "Backend", Some(60), None),
            row(MemoryCategory::App, "Web content", Some(400), Some("idea")),
            row(MemoryCategory::App, "GPU", Some(40), None),
            row(MemoryCategory::App, "Networking", Some(20), None),
            row(MemoryCategory::LanguageServer, "jdtls", Some(900), Some("idea")),
            ProcessMemory {
                processes: 2,
                ..row(MemoryCategory::LanguageServer, "rust-analyzer", Some(226), Some("idea"))
            },
            row(MemoryCategory::Process, "zsh", None, Some("Terminal")),
        ]
    }

    const MIB: u64 = 1024 * 1024;

    #[test]
    fn sums_per_group_and_labels_the_indicator_without_a_fake_denominator() {
        let summary = summarize_memory(&rows(), Some(1));
        assert_eq!(
            summary,
            MemorySummary {
                app: Some(520 * MIB),
                language_servers: Some(1126 * MIB),
                processes: None,
                total: Some(1646 * MIB),
            }
        );
        assert_eq!(indicator_label(&summary), "OpenIDEA 520M");
        assert_eq!(indicator_tooltip(&summary), "OpenIDEA 520 MB · language servers 1.1 GB");
    }

    #[test]
    fn falls_back_to_the_backends_resident_memory_without_a_breakdown() {
        let summary = summarize_memory(&[], Some(125 * MIB));
        assert_eq!(indicator_label(&summary), "OpenIDEA 125M");
        assert_eq!(indicator_tooltip(&summary), "OpenIDEA 125 MB");
        assert_eq!(indicator_label(&summarize_metrics(None)), "OpenIDEA –");
    }

    #[test]
    fn builds_popup_sections_in_category_order() {
        let sections = memory_sections(&rows());
        let shape: Vec<(&str, &str, usize)> =
            sections.iter().map(|s| (s.title, s.total.as_str(), s.rows.len())).collect();
        assert_eq!(
            shape,
            [("OpenIDEA", "520 MB", 4), ("Language servers", "1.1 GB", 2), ("Debug adapters & processes", "–", 1)]
        );
        let rust_analyzer = &sections[1].rows[1];
        assert_eq!(rust_analyzer.label, "rust-analyzer");
        assert_eq!(rust_analyzer.detail.as_deref(), Some("idea"));
        assert_eq!(rust_analyzer.value, "226 MB");
        assert_eq!(rust_analyzer.processes, 2);
        assert!(memory_sections(&[]).is_empty());
    }

    #[test]
    fn reports_what_clear_caches_freed() {
        let mut i18n = I18n::default();
        let result =
            ClearCachesResult { syntax_trees: 3, rss_before_bytes: Some(100 * MIB), rss_after_bytes: Some(50 * MIB) };
        let cleared = clear_caches_notification(Ok(Some(result)), &mut i18n);
        assert_eq!(cleared.title, "Caches cleared");
        assert_eq!(cleared.body.as_deref(), Some("3 syntax trees dropped, index compacted. Memory 100 MB → 50 MB."));
        assert_eq!(cleared.key.as_deref(), Some("perf.clearCaches"));
        let one = ClearCachesResult { syntax_trees: 1, ..ClearCachesResult::default() };
        let cleared = clear_caches_notification(Ok(Some(one)), &mut i18n);
        assert_eq!(cleared.body.as_deref(), Some("1 syntax tree dropped, index compacted."));
        assert_eq!(clear_caches_notification(Ok(None), &mut i18n).title, "No backend caches to clear");
        let failed = clear_caches_notification(Err("busy".to_owned()), &mut i18n);
        assert_eq!((failed.level, failed.body.as_deref()), (NotificationLevel::Error, Some("busy")));
    }
}
