//! Start-up failures: whatever goes wrong before the first window is up is shown instead of a
//! splash that never goes away, with details to copy into an issue. The first failure is the
//! one reported, a start-up that does not finish within 30 seconds counts as one, and once the
//! app is up nothing is intercepted any more.
//!
//! The texts are built in rather than taken from the message catalogs: the catalogs are part
//! of what may have failed to load.

/// How long start-up may take before it is reported as stuck, in milliseconds.
pub const BOOT_TIMEOUT_MS: u64 = 30_000;
/// Where "Report an issue" leads.
pub const REPORT_ISSUE_URL: &str = "https://github.com/l0ng-ai/openidea/issues/new?template=bug_report.yml";

/// The texts of the failure screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootText {
    pub title: &'static str,
    pub report: &'static str,
    pub link: &'static str,
    pub copy: &'static str,
    pub copied: &'static str,
    pub manual: &'static str,
    /// Accessible name of the details field.
    pub details: &'static str,
    pub timeout: &'static str,
}

const ENGLISH: BootText = BootText {
    title: "OpenIDEA could not start",
    report: "Copy the details below and include them in a GitHub issue.",
    link: "Report an issue",
    copy: "Copy details",
    copied: "Copied",
    manual: "Could not copy. Select the details below and copy them.",
    details: "Startup error details",
    timeout: "Startup did not finish within 30 seconds.",
};

const CHINESE: BootText = BootText {
    title: "OpenIDEA 无法启动",
    report: "请复制下方详情，并附在 GitHub 问题中。",
    link: "报告问题",
    copy: "复制详情",
    copied: "已复制",
    manual: "无法复制。请选择下方详情并手动复制。",
    details: "启动错误详情",
    timeout: "启动未在 30 秒内完成。",
};

/// The texts for the system language tag `language` (`zh-CN`, `en-US`): Chinese for Chinese,
/// English for everything else.
pub fn boot_text(language: &str) -> &'static BootText {
    if language.to_lowercase().starts_with("zh") { &CHINESE } else { &ENGLISH }
}

/// An error as the details show it: its trace, with its message in front unless the trace
/// already says it (some traces only list locations).
pub fn describe(message: &str, trace: Option<&str>) -> String {
    match trace {
        Some(trace) if trace.contains(message) => trace.to_owned(),
        Some(trace) => format!("{message}\n{trace}"),
        None => message.to_owned(),
    }
}

/// What the failure screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootReport {
    pub text: &'static BootText,
    /// The read-only details to copy: a heading, the system the app runs on, and the failure.
    pub details: String,
}

/// What the screen says after Copy details was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyStatus {
    pub message: &'static str,
    /// Copying failed: the details are focused and selected for copying by hand.
    pub select_details: bool,
}

/// Watches one start-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootWatch {
    text: &'static BootText,
    system: String,
    failed: bool,
    ready: bool,
}

impl BootWatch {
    /// `language` is the system's language tag and `system` a line that tells the app version
    /// and the OS, for the details. The caller arms a timer of [`BOOT_TIMEOUT_MS`] that calls
    /// [`BootWatch::timed_out`].
    pub fn new(language: &str, system: &str) -> Self {
        Self { text: boot_text(language), system: system.to_owned(), failed: false, ready: false }
    }

    /// Whether start-up is still being watched.
    pub fn is_watching(&self) -> bool {
        !self.failed && !self.ready
    }

    /// Something failed (`reason` as [`describe`] gives it). The first failure is reported;
    /// later ones, and anything after start-up finished, are not.
    pub fn fail(&mut self, reason: &str) -> Option<BootReport> {
        if !self.is_watching() {
            return None;
        }
        self.failed = true;
        let details = format!("OpenIDEA startup failure\n{}\n\n{reason}", self.system);
        Some(BootReport { text: self.text, details })
    }

    /// The timer ran out: a start-up that is neither up nor failed by now is reported as stuck.
    pub fn timed_out(&mut self) -> Option<BootReport> {
        self.fail(self.text.timeout)
    }

    /// The first window is up: failures from here on are the running app's to report.
    pub fn ready(&mut self) {
        self.ready = true;
    }

    /// The outcome of copying the details to the clipboard.
    pub fn copy_status(&self, copied: bool) -> CopyStatus {
        if copied {
            CopyStatus { message: self.text.copied, select_details: false }
        } else {
            CopyStatus { message: self.text.manual, select_details: true }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYSTEM: &str = "OpenIDEA 0.1.40 (macOS 14.5)";

    fn watch() -> BootWatch {
        BootWatch::new("en-US", SYSTEM)
    }

    // surfaces module evaluation errors before React is available
    #[test]
    fn surfaces_errors_before_the_first_window_is_available() {
        let mut watch = watch();
        let report = watch.fail("SyntaxError: Invalid regular expression").expect("the failure is reported");
        assert!(report.text.title.contains("OpenIDEA could not start"));
        assert!(report.details.contains("Invalid regular expression"));
        assert!(report.details.contains(SYSTEM));
        assert!(!watch.is_watching());
    }

    // handles startup rejections and preserves the first failure
    #[test]
    fn handles_startup_rejections_and_preserves_the_first_failure() {
        let mut watch = watch();
        let report = watch.fail("startup rejected").expect("the failure is reported");
        assert_eq!(watch.fail("later failure"), None);
        assert!(report.details.contains("startup rejected") && !report.details.contains("later failure"));
    }

    // surfaces initial React render failures through the boot error event
    #[test]
    fn surfaces_initial_render_failures() {
        let report = watch().fail(&describe("render failed", None));
        assert!(report.is_some_and(|report| report.details.ends_with("\n\nrender failed")));
    }

    // reports an unresolved startup instead of waiting forever
    #[test]
    fn reports_an_unresolved_startup_instead_of_waiting_forever() {
        let report = watch().timed_out().expect("a stuck start-up is reported");
        assert!(report.details.contains("Startup did not finish within 30 seconds."));
        assert_eq!(BOOT_TIMEOUT_MS, 30_000);
    }

    // stops intercepting failures after a successful startup
    #[test]
    fn stops_intercepting_failures_after_a_successful_startup() {
        let mut watch = watch();
        watch.ready();
        assert_eq!(watch.fail("runtime failure"), None);
        assert_eq!(watch.timed_out(), None);
    }

    // copies diagnostics and offers manual copying when clipboard access fails
    #[test]
    fn copies_diagnostics_and_offers_manual_copying_when_clipboard_access_fails() {
        let mut watch = watch();
        let report = watch.fail("<script>unsafe</script>").expect("the failure is reported");
        // The details are text, whatever they look like.
        assert!(report.details.contains("<script>unsafe</script>"));
        let status = watch.copy_status(false);
        assert!(status.message.contains("Select") && status.select_details);
    }

    // uses Chinese boot text without loading the application catalog
    #[test]
    fn uses_chinese_boot_text_without_loading_the_application_catalog() {
        let report = BootWatch::new("zh-CN", SYSTEM).fail("failure").expect("the failure is reported");
        assert!(report.text.title.contains("OpenIDEA 无法启动"));
        assert_eq!(boot_text("ZH-hant").copy, "复制详情");
        assert_eq!(boot_text("pt-BR"), boot_text("en"));
    }

    // confirms a successful copy
    #[test]
    fn confirms_a_successful_copy() {
        assert_eq!(watch().copy_status(true), CopyStatus { message: "Copied", select_details: false });
    }

    // preserves Safari error messages when its stack only lists locations
    #[test]
    fn preserves_error_messages_when_the_trace_only_lists_locations() {
        let message = "SyntaxError: Unsupported regular expression";
        let trace = "global code@tauri://localhost/assets/entry.js:1:20";
        let described = describe(message, Some(trace));
        assert!(described.contains(message) && described.contains(trace));
        // A trace that says the message is not given it twice.
        let full = format!("{message}\n    at entry.js:1:20");
        assert_eq!(describe(message, Some(&full)), full);
    }
}
