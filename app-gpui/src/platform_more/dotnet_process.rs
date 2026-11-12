//! One `dotnet` query run through the process host of the Run window: its output is collected
//! until the process exits, and the query can be cancelled at any moment, also before the host
//! has said which run it started.
//!
//! The caller starts the run ([`super::dotnet::dotnet_config`]), feeds what happens to a
//! [`DotnetQuery`] and stops the run when asked to. It listens for run exits from before it
//! starts the run until the query is finished: a fast process may exit before its run id is
//! known.

use crate::platform::i18n::{I18n, Param};
use ide_core::RunId;

/// Output beyond this many UTF-16 code units ends the query.
pub const OUTPUT_LIMIT: usize = 8_000_000;

/// Why a query has no output to give.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// The query was cancelled.
    Cancelled,
    /// The process printed more than [`OUTPUT_LIMIT`].
    OutputTooLarge,
    /// The process failed (`code` is `None` when a signal ended it); `output` is what it
    /// printed, trimmed.
    Exited { code: Option<i32>, output: String },
    /// The process could not be started.
    Start(String),
}

impl QueryError {
    /// The error as shown to the user; a cancelled query has nothing to show. A failed process
    /// is described by its own output when it printed any.
    pub fn message(&self, i18n: &mut I18n) -> Option<String> {
        Some(match self {
            Self::Cancelled => return None,
            Self::OutputTooLarge => i18n.tx(".NET output exceeded the size limit"),
            Self::Exited { output, .. } if !output.is_empty() => output.clone(),
            Self::Exited { code, .. } => {
                let code = code.map_or_else(|| Param::from("null"), Param::from);
                i18n.tx_built(&["dotnet exited with code ", ""], &[code])
            }
            Self::Start(message) => message.clone(),
        })
    }
}

/// What an event of a query asks of the caller.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[must_use]
pub struct QueryStep {
    /// Stop this run.
    pub stop: Option<RunId>,
    /// The query is over: its output, or why there is none. The caller stops listening.
    pub finished: Option<Result<String, QueryError>>,
}

/// The state of one query.
#[derive(Debug, Clone, Default)]
pub struct DotnetQuery {
    run: Option<RunId>,
    output: String,
    output_units: usize,
    overflow: bool,
    cancelled: bool,
    /// Exits seen before the run id was known.
    early_exits: Vec<(RunId, Option<i32>)>,
    finished: bool,
}

impl DotnetQuery {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the query is over; later events change nothing.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    fn stop(&self) -> Option<RunId> {
        self.run
    }

    fn finish(&mut self, code: Option<i32>) -> Option<Result<String, QueryError>> {
        self.finished = true;
        let output = std::mem::take(&mut self.output);
        Some(if self.cancelled {
            Err(QueryError::Cancelled)
        } else if self.overflow {
            Err(QueryError::OutputTooLarge)
        } else if code != Some(0) {
            Err(QueryError::Exited { code, output: output.trim().to_owned() })
        } else {
            Ok(output)
        })
    }

    /// The process printed `text`.
    pub fn output(&mut self, text: &str) -> QueryStep {
        if self.finished {
            return QueryStep::default();
        }
        let units = text.encode_utf16().count();
        if self.output_units + units > OUTPUT_LIMIT {
            self.overflow = true;
            return QueryStep { stop: self.stop(), finished: None };
        }
        self.output_units += units;
        self.output.push_str(text);
        QueryStep::default()
    }

    /// The caller cancels the query. A run that is not known yet is stopped as soon as it is.
    pub fn cancel(&mut self) -> QueryStep {
        if self.finished {
            return QueryStep::default();
        }
        self.cancelled = true;
        QueryStep { stop: self.stop(), finished: None }
    }

    /// The host started the process as `run`.
    pub fn started(&mut self, run: RunId) -> QueryStep {
        if self.finished {
            return QueryStep::default();
        }
        self.run = Some(run);
        let stop = (self.cancelled || self.overflow).then_some(run);
        let early = self.early_exits.iter().rev().find(|(exited, _)| *exited == run).map(|(_, code)| *code);
        self.early_exits.clear();
        QueryStep { stop, finished: early.and_then(|code| self.finish(code)) }
    }

    /// The host could not start the process.
    pub fn start_failed(&mut self, message: &str) -> QueryStep {
        if self.finished {
            return QueryStep::default();
        }
        self.finished = true;
        let error = if self.cancelled { QueryError::Cancelled } else { QueryError::Start(message.to_owned()) };
        QueryStep { stop: None, finished: Some(Err(error)) }
    }

    /// Some run exited. Only the query's own run ends it; other runs of the window are ignored.
    pub fn run_exited(&mut self, run: RunId, code: Option<i32>) -> QueryStep {
        if self.finished {
            return QueryStep::default();
        }
        match self.run {
            None => {
                self.early_exits.push((run, code));
                QueryStep::default()
            }
            Some(own) if own == run => QueryStep { stop: None, finished: self.finish(code) },
            Some(_) => QueryStep::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: RunId = RunId(7);
    const IDLE: QueryStep = QueryStep { stop: None, finished: None };

    // .NET query lifecycle › captures a fast exit before runStart resolves, ignoring another run
    #[test]
    fn captures_a_fast_exit_before_run_start_resolves_ignoring_another_run() {
        let mut query = DotnetQuery::new();
        assert_eq!(query.output(r#"{"ok":true}"#), IDLE);
        assert_eq!(query.run_exited(RunId(99), Some(1)), IDLE);
        assert_eq!(query.run_exited(OWN, Some(0)), IDLE);
        let step = query.started(OWN);
        assert_eq!(step, QueryStep { stop: None, finished: Some(Ok(r#"{"ok":true}"#.to_owned())) });
        assert!(query.is_finished());
    }

    // .NET query lifecycle › reports process failures and releases its event subscription
    #[test]
    fn reports_process_failures_and_releases_its_event_subscription() {
        let mut query = DotnetQuery::new();
        assert_eq!(query.run_exited(OWN, Some(1)), IDLE);
        let error = query.started(OWN).finished.and_then(Result::err).expect("the query failed");
        assert_eq!(error, QueryError::Exited { code: Some(1), output: String::new() });
        assert_eq!(error.message(&mut I18n::default()).as_deref(), Some("dotnet exited with code 1"));
        assert!(query.is_finished());
    }

    // .NET query lifecycle › stops a process cancelled while runStart is pending
    #[test]
    fn stops_a_process_cancelled_while_run_start_is_pending() {
        let mut query = DotnetQuery::new();
        // Nothing to stop yet: the host has not said which run it started.
        assert_eq!(query.cancel(), IDLE);
        assert_eq!(query.started(OWN), QueryStep { stop: Some(OWN), finished: None });
        let step = query.run_exited(OWN, None);
        assert_eq!(step, QueryStep { stop: None, finished: Some(Err(QueryError::Cancelled)) });
    }

    // .NET query lifecycle › releases its subscription if starting the process fails
    #[test]
    fn releases_its_subscription_if_starting_the_process_fails() {
        let mut query = DotnetQuery::new();
        let step = query.start_failed("SDK missing");
        assert_eq!(step.finished, Some(Err(QueryError::Start("SDK missing".to_owned()))));
        assert!(query.is_finished());
        assert_eq!(query.run_exited(OWN, Some(0)), IDLE);
    }

    // Not from a spec: a failed process is described by its output, another run's exit after
    // the start is ignored, and output past the limit stops the run and fails the query.
    #[test]
    fn fails_with_the_output_of_the_process_and_on_too_much_output() {
        let mut query = DotnetQuery::new();
        assert_eq!(query.started(OWN), IDLE);
        assert_eq!(query.output("  error MSB1009: Project file does not exist.\n"), IDLE);
        assert_eq!(query.run_exited(RunId(8), Some(0)), IDLE);
        let error = query.run_exited(OWN, Some(1)).finished.and_then(Result::err).expect("the query failed");
        let shown = error.message(&mut I18n::default());
        assert_eq!(shown.as_deref(), Some("error MSB1009: Project file does not exist."));
        assert_eq!(QueryError::Cancelled.message(&mut I18n::default()), None);

        let mut flood = DotnetQuery::new();
        assert_eq!(flood.started(OWN), IDLE);
        let chunk = "x".repeat(OUTPUT_LIMIT / 2 + 1);
        assert_eq!(flood.output(&chunk), IDLE);
        assert_eq!(flood.output(&chunk), QueryStep { stop: Some(OWN), finished: None });
        let step = flood.run_exited(OWN, None);
        assert_eq!(step.finished, Some(Err(QueryError::OutputTooLarge)));
    }
}
