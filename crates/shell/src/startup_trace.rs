//! PERF-12: instrumentation of the fixed cost a Lumen process pays before it
//! does any page work.
//!
//! # Why this module exists
//!
//! The 2026-08-18 startup census (`docs/tasks/perf-startup-census.md`) found
//! that a `--dump-source` of a page containing a single empty `<div>` — nothing
//! to fetch, nothing to lay out — costs as much as the whole rest of the
//! pipeline put together, and that the cost is a black box: [`lumen_core::trace`]
//! had no spans anywhere between the process entry point and the first fetch,
//! and `--trace-nav` only switches the tracer on once the CLI mode is already
//! known, which is *after* the stretch in question.
//!
//! Two separate blind spots follow from that, and this module closes both:
//!
//! - **Before `main`.** The OS loader maps the ~80 MB binary and the CRT runs
//!   its static initialisers before a single line of ours executes. No `Instant`
//!   taken inside the process can see that, so [`Startup::begin`] asks the OS
//!   when the process was created and backfills the stretch onto the timeline.
//! - **Inside `main`, before dispatch.** Config load, argument parsing and
//!   per-flag service spawns all run before any `CliMode` is chosen. These are
//!   ordinary spans, they just had nobody opening them.
//!
//! # Reading the output
//!
//! Under `--trace-nav` the phases appear on the normal Chrome-trace timeline in
//! the `startup` category, ahead of the `navigation` span. Because most startup
//! modes never produce a trace file (an argument error exits long before one is
//! written, and `--dump-*` is not a trace mode at all), the same breakdown is
//! also available as stderr lines under `LUMEN_STARTUP_LOG=1`, which works for
//! every mode including the ones that exit early.
//!
//! # Caveat on the absolute numbers
//!
//! A large part of what a shell's stopwatch attributes to "starting Lumen" is
//! the OS's per-process constant, which any executable pays — measured against a
//! 15 KB system binary on the census machine it was 86 ms of a 147 ms total. Do
//! not read the `total` line as "Lumen's overhead"; read `pre-main` and the
//! phases, which are ours.

use std::time::{Duration, Instant};

/// Category all startup spans are filed under in the Chrome-trace output.
const CAT: &str = "startup";

/// Time from OS process creation to now, or `None` where the platform cannot
/// report it (everything except Windows, currently).
///
/// Windows keeps a creation `FILETIME` per process; the delta against the
/// current system time is the process's whole lifetime so far, which is the only
/// way to see the loader and CRT work that precedes `main`.
#[cfg(target_os = "windows")]
fn since_process_creation() -> Option<Duration> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    const ZERO: FILETIME = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut creation, mut exit, mut kernel, mut user, mut now) = (ZERO, ZERO, ZERO, ZERO, ZERO);

    // SAFETY: `GetCurrentProcess` returns a pseudo-handle that is always valid
    // for the calling process and must not be closed. All five `FILETIME`
    // out-params are live, correctly aligned locals that outlive the calls, and
    // both functions only write through them — `GetProcessTimes` requires all
    // four to be non-null, so none are elided.
    let ok = unsafe {
        let ok = GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        );
        GetSystemTimeAsFileTime(&mut now);
        ok
    };
    if ok == 0 {
        return None;
    }

    let ticks = |ft: &FILETIME| (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime);
    // Both values are 100 ns ticks since 1601-01-01 UTC, so the epoch cancels.
    // `checked_sub` guards against a clock adjustment moving system time behind
    // the recorded creation stamp, which would otherwise wrap into ~584 years.
    let elapsed = ticks(&now).checked_sub(ticks(&creation))?;
    Some(Duration::from_nanos(elapsed.saturating_mul(100)))
}

/// Non-Windows stub: the pre-`main` stretch is simply not reported, and callers
/// fall back to measuring from process entry, as they did before this module.
#[cfg(not(target_os = "windows"))]
fn since_process_creation() -> Option<Duration> {
    None
}

/// Fixed-startup stopwatch, created on the first line of `run_cli` and consulted
/// until the CLI mode is dispatched.
pub struct Startup {
    /// When `run_cli` was entered — the boundary between "before `main`" and
    /// "our code".
    entry: Instant,
    /// How long the process had already existed at [`Self::entry`], when the OS
    /// can say.
    pre_main: Option<Duration>,
    /// Whether `LUMEN_STARTUP_LOG=1` asked for the stderr breakdown.
    logging: bool,
}

impl Startup {
    /// Starts the stopwatch. Call as the first statement of `run_cli`, before
    /// any config or argument work, so that everything it measures is really in
    /// front of it.
    ///
    /// Switches the tracer on here — rather than in `run_trace_nav`, where it
    /// used to start — whenever `--trace-nav` appears anywhere in the raw
    /// argument list. The scan deliberately does not reuse the `extract_*`
    /// parsers below it in `run_cli`: those run inside the stretch being
    /// measured, and using them would put the parse outside its own span.
    pub fn begin() -> Self {
        let entry = Instant::now();
        let pre_main = since_process_creation();
        let logging = std::env::var_os("LUMEN_STARTUP_LOG").is_some();

        if std::env::args().any(|a| a == "--trace-nav") {
            // Anchor the timeline at process creation when known, so `pre-main`
            // and every later span share one origin.
            let origin = pre_main
                .and_then(|d| entry.checked_sub(d))
                .unwrap_or(entry);
            lumen_core::trace::enable_at(origin);
            if origin != entry {
                lumen_core::trace::record_span("pre-main", CAT, origin, entry);
            }
        }

        if logging {
            match pre_main {
                Some(d) => eprintln!(
                    "[startup] pre-main {:.1}ms  (OS loader + CRT, before run_cli)",
                    d.as_secs_f64() * 1000.0
                ),
                None => eprintln!("[startup] pre-main n/a on this platform"),
            }
        }
        Self {
            entry,
            pre_main,
            logging,
        }
    }

    /// Opens a named startup phase. The phase ends when the returned guard is
    /// dropped, recording a trace span and — under `LUMEN_STARTUP_LOG=1` — a
    /// stderr line.
    pub fn phase(&self, name: &'static str) -> Phase<'_> {
        Phase {
            startup: self,
            name,
            begin: Instant::now(),
            span: lumen_core::trace::span(name, CAT),
        }
    }

    /// Closes the fixed-startup accounting at the moment the CLI mode is known
    /// and real work is about to begin. `mode` names the chosen mode, so a
    /// timeline or log can be read without re-deriving it from the arguments.
    pub fn dispatch(&self, mode: &str) {
        let now = Instant::now();
        let origin = self
            .pre_main
            .and_then(|d| self.entry.checked_sub(d))
            .unwrap_or(self.entry);
        // One enclosing band over every phase above, including `pre-main`; the
        // mode goes in the name because the span is backfilled and so cannot
        // carry args through a guard.
        lumen_core::trace::record_span(format!("startup ({mode})"), CAT, origin, now);
        if self.logging {
            eprintln!(
                "[startup] total {:.1}ms to dispatch {mode}  \
                 (incl. pre-main; a bare system exe costs ~85ms on Windows)",
                now.saturating_duration_since(origin).as_secs_f64() * 1000.0
            );
        }
    }
}

/// Reports the process's whole lifetime, as the OS sees it, on the way out of
/// `main` — under `LUMEN_STARTUP_LOG=1` only.
///
/// Without this line the accounting does not close: `Startup::dispatch` stops at
/// the moment page work begins, so everything after it — the work itself, plus
/// process teardown, which for a binary this size is not free — would have to be
/// inferred by subtracting an externally timed run, and an external stopwatch
/// also charges Lumen for the shell's own fork/exec.
pub fn log_exit() {
    if std::env::var_os("LUMEN_STARTUP_LOG").is_none() {
        return;
    }
    match since_process_creation() {
        Some(d) => eprintln!(
            "[startup] process lifetime at exit {:.1}ms (OS creation -> end of main)",
            d.as_secs_f64() * 1000.0
        ),
        None => eprintln!("[startup] process lifetime n/a on this platform"),
    }
}

/// One named startup phase; see [`Startup::phase`].
#[must_use = "the phase ends when this guard is dropped — bind it to a name, not `_`"]
pub struct Phase<'a> {
    /// Owner, consulted for the logging flag on drop.
    startup: &'a Startup,
    /// Phase name, shared by the trace span and the stderr line.
    name: &'static str,
    /// When the phase opened.
    begin: Instant,
    /// Timeline span closed together with this guard.
    span: lumen_core::trace::SpanGuard,
}

impl Drop for Phase<'_> {
    fn drop(&mut self) {
        if self.startup.logging {
            eprintln!(
                "[startup]   {} {:.1}ms",
                self.name,
                self.begin.elapsed().as_secs_f64() * 1000.0
            );
        }
        // `span` closes right after this, on the field's own drop.
        let _ = &self.span;
    }
}
