//! The calls of Windows of the dispatcher of the service control manager
//! (T-1026); `service.rs` holds the states and the exit codes.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use windows_sys::Win32::Foundation::{ERROR_CALL_NOT_IMPLEMENTED, ERROR_FAILED_SERVICE_CONTROLLER_CONNECT, NO_ERROR};
use windows_sys::Win32::System::Services::{
    RegisterServiceCtrlHandlerExW, SERVICE_ACCEPT_SHUTDOWN, SERVICE_ACCEPT_STOP, SERVICE_CONTROL_INTERROGATE,
    SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_STATUS, SERVICE_TABLE_ENTRYW, SERVICE_WIN32_OWN_PROCESS,
    SetServiceStatus, StartServiceCtrlDispatcherW,
};

use crate::model::usage;
use crate::service::{PANIC_CODE, Report, Signal, WAIT_HINT};

type Work = Box<dyn FnOnce(Service) -> i32 + Send>;

/// The name and the work of `serve` for the thread of the service: the
/// manager calls `service_main` with no pointer of the caller.
static WORK: Mutex<Option<(Vec<u16>, Work)>> = Mutex::new(None);

/// The end of the service: the exit code of the work, or why the work did
/// not run.
static END: Mutex<Option<Result<i32, String>>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The state that the handler of the manager and the work share.
struct Shared {
    handle: AtomicPtr<c_void>,
    report: Mutex<Report>,
    signal: Sender<Signal>,
}

impl Shared {
    /// Change the report and send it, under one lock: the reports of the
    /// handler and of the work reach the manager in their order.
    fn change(&self, next: impl FnOnce(Report) -> Option<Report>) -> Result<(), Failure> {
        let mut report = lock(&self.report);
        let Some(new) = next(*report) else { return Ok(()) };
        *report = new;
        let status = SERVICE_STATUS {
            dwServiceType: SERVICE_WIN32_OWN_PROCESS,
            dwCurrentState: new.state as u32,
            dwControlsAccepted: if new.accepts { SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN } else { 0 },
            dwWin32ExitCode: new.win32,
            dwServiceSpecificExitCode: new.specific,
            dwCheckPoint: new.checkpoint,
            dwWaitHint: new.wait_hint_ms,
        };
        // SAFETY: the handle comes from RegisterServiceCtrlHandlerExW and
        // stays valid for the life of the process; the status is a local.
        if unsafe { SetServiceStatus(self.handle.load(Ordering::Acquire), &status) } == 0 {
            let e = std::io::Error::last_os_error();
            return Err(Failure::new(Stage::Host, Class::Unavailable, format!("SetServiceStatus: {e}")));
        }
        Ok(())
    }
}

/// The side of the service that the work holds: its reports to the manager
/// and the channel of each stop.
pub struct Service {
    shared: &'static Shared,
    stop: Receiver<Signal>,
}

impl Service {
    /// The work is ready: the service is running, and the manager may stop
    /// it. A ready after a stop changes nothing.
    pub fn ready(&self) -> Result<(), Failure> {
        self.shared.change(Report::ready)
    }

    /// A long start goes on: the next report comes within `hint`.
    pub fn starting(&self, hint: Duration) -> Result<(), Failure> {
        self.shared.change(|r| r.starting(hint))
    }

    /// The work drains: the service reports a pending stop, and the next
    /// report comes within `hint`. Call it again for each step of a long
    /// drain, and also for a stop that the work decides itself.
    pub fn stopping(&self, hint: Duration) -> Result<(), Failure> {
        self.shared.change(|r| r.stopping(hint))
    }

    /// The channel of each stop and shutdown of the manager. The handler
    /// reports a pending stop before it sends.
    pub fn stop(&self) -> &Receiver<Signal> {
        &self.stop
    }
}

/// Run `work` as the Windows service `name`, which the manager started:
/// the dispatcher of the manager on the thread of the caller, and `work` on
/// the thread of the service. The service reports a pending start, then
/// `Service::ready` gives running; each stop of the manager comes as a
/// `Signal`, and the end of `work` is the stop of the service, with the
/// exit code of `work` (`exit_codes`; a panic is `PANIC_CODE`). `serve`
/// returns that code when the dispatcher ends; the caller exits with it.
///
/// A program calls `serve` when an argument says that the manager started
/// it: put that word in the arguments of the unit of the service form. A
/// program that the manager did not start gets `Class::Usage`.
pub fn serve<F>(name: &str, work: F) -> Result<i32, Failure>
where
    F: FnOnce(Service) -> i32 + Send + 'static,
{
    if name.is_empty() || name.contains('\0') {
        return Err(usage("the name of a service is empty or holds a NUL"));
    }
    let mut wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    {
        let mut slot = lock(&WORK);
        if slot.is_some() {
            return Err(usage("serve runs on one thread of a process at a time"));
        }
        *slot = Some((wide.clone(), Box::new(work)));
    }
    *lock(&END) = None;
    let table = [
        SERVICE_TABLE_ENTRYW { lpServiceName: wide.as_mut_ptr(), lpServiceProc: Some(service_main) },
        SERVICE_TABLE_ENTRYW { lpServiceName: null_mut(), lpServiceProc: None },
    ];
    // SAFETY: the table ends with an empty entry, and the name lives past
    // the call, which returns after the service stopped.
    let dispatched = unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } != 0;
    let e = std::io::Error::last_os_error();
    // A work that the manager did not run.
    lock(&WORK).take();
    if !dispatched {
        if e.raw_os_error() == Some(ERROR_FAILED_SERVICE_CONTROLLER_CONNECT as i32) {
            return Err(usage(format!(
                "the service control manager did not start this program: start the service {name} with sc.exe \
                 start, or run the program with no argument of a service"
            )));
        }
        return Err(Failure::new(Stage::Host, Class::Unavailable, format!("StartServiceCtrlDispatcherW: {e}")));
    }
    match lock(&END).take() {
        Some(Ok(code)) => Ok(code),
        Some(Err(why)) => Err(Failure::new(Stage::Host, Class::Unavailable, why)),
        None => Err(Failure::new(Stage::Host, Class::Unavailable, "the dispatcher ended before the service ran")),
    }
}

/// The main function of the service, on a thread of the dispatcher.
unsafe extern "system" fn service_main(_argc: u32, _argv: *mut windows_sys::core::PWSTR) {
    let Some((name, work)) = lock(&WORK).take() else { return };
    let (signal, stop) = channel();
    // Leaked: the manager can call the handler with this pointer until the
    // process ends. One small value for each process.
    let shared: &'static Shared =
        Box::leak(Box::new(Shared { handle: AtomicPtr::new(null_mut()), report: Mutex::new(Report::start()), signal }));
    // SAFETY: the name ends with a 0; the context lives for the process.
    let handle =
        unsafe { RegisterServiceCtrlHandlerExW(name.as_ptr(), Some(handler), std::ptr::from_ref(shared).cast()) };
    if handle.is_null() {
        *lock(&END) = Some(Err(format!("RegisterServiceCtrlHandlerExW: {}", std::io::Error::last_os_error())));
        return;
    }
    shared.handle.store(handle, Ordering::Release);
    if let Err(f) = shared.change(|_| Some(Report::start())) {
        *lock(&END) = Some(Err(f.to_string()));
        return;
    }
    let code = catch_unwind(AssertUnwindSafe(|| work(Service { shared, stop }))).unwrap_or(PANIC_CODE);
    // The end comes first: the process can end at any time after the last
    // report.
    *lock(&END) = Some(Ok(code));
    let _ = shared.change(|r| Some(r.stopped(code)));
}

/// The handler of the controls of the manager, on the thread of the
/// dispatcher. It does not block: a stop is a report and a send.
unsafe extern "system" fn handler(control: u32, _event: u32, _data: *mut c_void, context: *mut c_void) -> u32 {
    // SAFETY: the context is the leaked `Shared` of `service_main`.
    let shared = unsafe { &*(context as *const Shared) };
    let signal = match control {
        SERVICE_CONTROL_STOP => Signal::Stop,
        SERVICE_CONTROL_SHUTDOWN => Signal::Shutdown,
        SERVICE_CONTROL_INTERROGATE => return NO_ERROR,
        _ => return ERROR_CALL_NOT_IMPLEMENTED,
    };
    // The manager sees the stop at once; the work gives its own hint as it
    // drains.
    let _ = shared.change(|r| r.stopping(WAIT_HINT));
    // A work that ended dropped its receiver: the send then fails, and the
    // stop is already there.
    let _ = shared.signal.send(signal);
    NO_ERROR
}
