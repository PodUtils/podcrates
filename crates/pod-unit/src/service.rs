//! The dispatcher of the service control manager of Windows (T-1026): the
//! part of a Windows service that the program runs. The manager starts the
//! program and waits for its call of `StartServiceCtrlDispatcherW`; it ends
//! a program that does not call it with error 1053 after 30 s.
//! `windows::serve` makes the call on the thread of its caller, runs the
//! work of the program on the thread of the service, and gives each stop of
//! the manager to the work as a [`Signal`] on a channel: the work drains,
//! as with `KillMode=mixed` on systemd.
//!
//! This file holds the part with no call of Windows: the states that the
//! service reports, and the exit codes. It compiles and its tests run on
//! each host; `service_scm.rs` makes the calls.

use std::time::Duration;

/// A stop of the service control manager, which the work receives on the
/// channel of `Service::stop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// `sc.exe stop`, or a stop from the console of the services.
    Stop,
    /// The host shuts down: the manager waits for a short time only.
    Shutdown,
}

/// The time that the manager waits for the next report of a start or of a
/// stop (its wait hint), until the work gives its own.
pub const WAIT_HINT: Duration = Duration::from_secs(30);

/// The Win32 exit code of a service that ends with a code of its own
/// (`ERROR_SERVICE_SPECIFIC_ERROR`).
pub const SPECIFIC_ERROR: u32 = 1066;

/// The exit code that a work which panics gives: the code of a Rust
/// program that panics.
pub const PANIC_CODE: i32 = 101;

/// The two exit codes of a stopped service (`dwWin32ExitCode`,
/// `dwServiceSpecificExitCode`) for the exit code `code` of the work: 0 is
/// a clean end; each other code is a code of the service, which stays as
/// it is (78 stays 78). With `failureflag 1` of `windows::service_commands`,
/// the manager takes each code that is not 0 as a failure.
pub fn exit_codes(code: i32) -> (u32, u32) {
    if code == 0 { (0, 0) } else { (SPECIFIC_ERROR, code as u32) }
}

/// The exit code of the work from the two exit codes of a stopped service,
/// as `sc.exe query` or `QueryServiceStatusEx` give them. A Win32 code
/// that is not `SPECIFIC_ERROR` is a code of the manager (1067: the process
/// ended with no report).
pub fn program_code(win32: u32, specific: u32) -> i32 {
    if win32 == SPECIFIC_ERROR { specific as i32 } else { win32 as i32 }
}

/// The state of a service, with the values of `SERVICE_STATUS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum State {
    Stopped = 1,
    StartPending = 2,
    StopPending = 3,
    Running = 4,
}

/// What the service reports to the manager: the fields of
/// `SERVICE_STATUS` with no type of Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Report {
    pub(crate) state: State,
    /// True: the manager may send a stop and a shutdown.
    pub(crate) accepts: bool,
    pub(crate) win32: u32,
    pub(crate) specific: u32,
    pub(crate) checkpoint: u32,
    pub(crate) wait_hint_ms: u32,
}

impl Report {
    /// The first report: the start is pending, and no control is accepted.
    pub(crate) fn start() -> Report {
        Report {
            state: State::StartPending,
            accepts: false,
            win32: 0,
            specific: 0,
            checkpoint: 1,
            wait_hint_ms: ms(WAIT_HINT),
        }
    }

    /// The work is ready: running, with stop and shutdown accepted. None
    /// outside a pending start: a ready after a stop changes nothing.
    pub(crate) fn ready(self) -> Option<Report> {
        (self.state == State::StartPending).then_some(Report {
            state: State::Running,
            accepts: true,
            checkpoint: 0,
            wait_hint_ms: 0,
            ..self
        })
    }

    /// The start goes on: the next checkpoint, and the time to the next
    /// report. None outside a pending start.
    pub(crate) fn starting(self, hint: Duration) -> Option<Report> {
        (self.state == State::StartPending).then_some(Report {
            checkpoint: self.checkpoint + 1,
            wait_hint_ms: ms(hint),
            ..self
        })
    }

    /// The work drains: a pending stop with no control accepted, the next
    /// checkpoint and the time to the next report. None after the end.
    pub(crate) fn stopping(self, hint: Duration) -> Option<Report> {
        let checkpoint = match self.state {
            State::Stopped => return None,
            State::StopPending => self.checkpoint + 1,
            State::StartPending | State::Running => 1,
        };
        Some(Report { state: State::StopPending, accepts: false, checkpoint, wait_hint_ms: ms(hint), ..self })
    }

    /// The last report: stopped, with the exit codes of `code`.
    pub(crate) fn stopped(self, code: i32) -> Report {
        let (win32, specific) = exit_codes(code);
        Report { state: State::Stopped, accepts: false, win32, specific, checkpoint: 0, wait_hint_ms: 0 }
    }
}

fn ms(hint: Duration) -> u32 {
    u32::try_from(hint.as_millis()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exit_code_of_the_work_stays_the_code_of_the_service() {
        assert_eq!(exit_codes(0), (0, 0));
        assert_eq!(exit_codes(78), (SPECIFIC_ERROR, 78));
        assert_eq!(exit_codes(64), (SPECIFIC_ERROR, 64));
        for code in [0, 1, 64, 69, 78, PANIC_CODE, -1, i32::MAX] {
            let (win32, specific) = exit_codes(code);
            assert_eq!(program_code(win32, specific), code, "{code}");
        }
        // The process ended with no report: a code of the manager.
        assert_eq!(program_code(1067, 0), 1067);
    }

    #[test]
    fn a_service_reports_running_after_ready_and_a_pending_stop_while_it_drains() {
        let start = Report::start();
        assert_eq!(
            (start.state, start.accepts, start.checkpoint, start.wait_hint_ms),
            (State::StartPending, false, 1, 30_000)
        );
        let longer = start.starting(Duration::from_secs(5)).unwrap();
        assert_eq!((longer.checkpoint, longer.wait_hint_ms), (2, 5000));
        let running = longer.ready().unwrap();
        assert_eq!(
            (running.state, running.accepts, running.checkpoint, running.wait_hint_ms),
            (State::Running, true, 0, 0)
        );
        assert_eq!(running.starting(Duration::from_secs(1)), None);
        let stopping = running.stopping(WAIT_HINT).unwrap();
        assert_eq!((stopping.state, stopping.accepts, stopping.checkpoint), (State::StopPending, false, 1));
        // A ready after the stop does not accept a control again.
        assert_eq!(stopping.ready(), None);
        let drain = stopping.stopping(Duration::from_millis(7500)).unwrap();
        assert_eq!((drain.checkpoint, drain.wait_hint_ms), (2, 7500));
        let end = drain.stopped(78);
        assert_eq!((end.state, end.accepts, end.win32, end.specific), (State::Stopped, false, SPECIFIC_ERROR, 78));
        assert_eq!(end.stopping(WAIT_HINT), None);
        assert_eq!(end.ready(), None);
    }

    #[test]
    fn a_work_that_ends_in_its_start_stops_with_its_code() {
        let end = Report::start().stopped(78);
        assert_eq!((end.state, end.win32, end.specific), (State::Stopped, SPECIFIC_ERROR, 78));
        let clean = Report::start().stopping(WAIT_HINT).unwrap().stopped(0);
        assert_eq!((clean.win32, clean.specific), (0, 0));
        assert_eq!(ms(Duration::from_secs(u64::MAX)), u32::MAX);
    }
}
