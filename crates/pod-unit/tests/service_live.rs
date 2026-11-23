//! A Windows service of this test program (T-1026). `install::install_system`
//! makes it with the commands of `windows::service_commands` and starts it;
//! the program answers the service control manager with `windows::serve`,
//! reports running, receives the stop of the manager, reports its drain, and
//! stops with the exit code of its work (78, then 0); `install::remove_system`
//! removes it. A program that the manager did not start gets
//! `Class::Usage`. The steps of a service need an administrator, and skip
//! with the reason elsewhere; the Windows job of `nightly.yml` runs them, and
//! a skip fails there.
//!
//! No libtest (`harness = false`): the manager runs this binary again as
//! the service (`service CODE DIR`), and the dispatcher takes the thread of
//! `main`.

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("service") && args.len() == 4 {
        std::process::exit(live::be_the_service(&args[2], std::path::Path::new(&args[3])));
    }
    // `cargo test -- --list` lists no test of this binary.
    if args.iter().any(|a| a == "--list") {
        return;
    }
    live::run();
}

#[cfg(not(windows))]
fn main() {
    pod_testkit::host::skip_line("a Windows service needs Windows");
}

#[cfg(windows)]
mod live {
    use std::path::Path;
    use std::process::Command;
    use std::time::{Duration, Instant};

    use pod_core::Class;
    use pod_unit::install::{self, Form, Host, Removed, SystemPlaces};
    use pod_unit::windows::{self as win, Signal};
    use pod_unit::{System, Unit};
    use windows_sys::Win32::Foundation::ERROR_SERVICE_DOES_NOT_EXIST;
    use windows_sys::Win32::System::Services::{
        CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, QueryServiceStatus, SC_HANDLE,
        SC_MANAGER_CONNECT, SERVICE_ACCEPT_STOP, SERVICE_CONTROL_STOP, SERVICE_QUERY_STATUS, SERVICE_RUNNING,
        SERVICE_STATUS, SERVICE_STOP, SERVICE_STOP_PENDING, SERVICE_STOPPED,
    };

    /// The bound of each wait of the test and of the service.
    const LIMIT: Duration = Duration::from_secs(60);
    /// The hint of the drain of the service, which the test reads back from
    /// the manager.
    const DRAIN: Duration = Duration::from_millis(7250);
    /// The file whose presence ends the drain of the service.
    const DRAINED: &str = "drained";

    /// The work of the service: ready, then a drain at the stop of the
    /// manager until the test makes the file `DRAINED`, then `code`.
    pub fn be_the_service(code: &str, dir: &Path) -> i32 {
        let Ok(code) = code.parse::<i32>() else { return 64 };
        let dir = dir.to_path_buf();
        let served = win::serve("pod-unit-test", move |service| {
            if service.ready().is_err() {
                return 70;
            }
            if service.stop().recv_timeout(LIMIT) != Ok(Signal::Stop) {
                return 75;
            }
            if service.stopping(DRAIN).is_err() {
                return 70;
            }
            if !until(|| dir.join(DRAINED).exists()) {
                return 75;
            }
            code
        });
        served.unwrap_or(70)
    }

    pub fn run() {
        a_program_that_the_manager_did_not_start_gets_a_usage_failure();
        if !System::default().admin() {
            pod_testkit::host::skip_line("a Windows service needs an administrator: this token may not create one");
            return;
        }
        a_stop_of_the_manager_reaches_the_drain_and_the_code_of_the_work_is_the_code_of_the_service(78);
        a_stop_of_the_manager_reaches_the_drain_and_the_code_of_the_work_is_the_code_of_the_service(0);
        println!("service_live: 3 steps passed");
    }

    fn a_program_that_the_manager_did_not_start_gets_a_usage_failure() {
        // Three calls: a failed call leaves no work behind it, and Windows
        // lets a process call the dispatcher one time, so each later call
        // gets the failure of the first.
        let mut first = None;
        for _ in 0..3 {
            let failure = win::serve("pod-unit-test", |_| unreachable!("the manager runs no work")).unwrap_err();
            assert_eq!(failure.class, Class::Usage, "{failure}");
            assert!(failure.text().contains("did not start this program"), "{failure}");
            let first = first.get_or_insert_with(|| failure.to_string());
            assert_eq!(*first, failure.to_string());
        }
        assert_eq!(win::serve("", |_| 0).unwrap_err().class, Class::Usage);
    }

    fn a_stop_of_the_manager_reaches_the_drain_and_the_code_of_the_work_is_the_code_of_the_service(code: i32) {
        let name = format!("pod-unit-test-{}-{code}", std::process::id());
        let dir = tempfile::Builder::new().prefix("pod-unit-service-").tempdir().expect("a directory");
        // Each process of a service holds the group SERVICE (S-1-5-6): it
        // reads the directory and the copy of the program. The privilege
        // SeChangeNotifyPrivilege of the unit crosses the directories above.
        let granted = Command::new("icacls").arg(dir.path()).args(["/grant", "*S-1-5-6:(OI)(CI)RX"]).output();
        assert!(granted.as_ref().is_ok_and(|o| o.status.success()), "icacls: {granted:?}");
        let program = dir.path().join("service_live.exe");
        std::fs::copy(std::env::current_exe().expect("this program"), &program).expect("a copy of the program");
        let mut unit = Unit::new(&name, &program, "pod-unit tests/service_live.rs").system(None).args([
            "service".to_string(),
            code.to_string(),
            dir.path().display().to_string(),
        ]);
        unit.description = "pod-unit: a service of a test".into();
        // No start again within the test after the code 78.
        unit.restart.delay_s = 3600;
        let (host, places) = (System::default(), SystemPlaces::default());

        let done = install::install_system(&unit, Form::Service, &places, &host);
        let seen = done.as_ref().map_err(ToString::to_string).and_then(|_| stop_with_a_drain(&name, dir.path()));
        let removed = install::remove_system(&unit, Form::Service, &places, &host);
        let done = done.expect("an install");
        assert!(done[0].contains("made and started"), "{done:?}");
        let (win32, specific) = seen.expect("a stop with a drain");
        assert_eq!((win32, specific), win::exit_codes(code));
        assert_eq!(win::program_code(win32, specific), code);
        assert!(matches!(removed, Ok(Removed::Done(_))), "{removed:?}");
        let gone = until(|| query(&name).err() == Some(ERROR_SERVICE_DOES_NOT_EXIST));
        assert!(gone, "the removed service: {:?}", query(&name).map(|s| text(&s)));
    }

    /// Running, then a stop of the manager: the pending stop with the hint
    /// of the drain of the work, then the file that ends the drain, then the
    /// two exit codes of the stopped service.
    fn stop_with_a_drain(name: &str, dir: &Path) -> Result<(u32, u32), String> {
        let running = wait(name, |s| s.dwCurrentState == SERVICE_RUNNING)?;
        if running.dwControlsAccepted & SERVICE_ACCEPT_STOP == 0 {
            return Err(format!("a running service accepts no stop: {}", text(&running)));
        }
        let first = control_stop(name)?;
        if first.dwCurrentState != SERVICE_STOP_PENDING {
            return Err(format!("no pending stop after the stop: {}", text(&first)));
        }
        let hint = DRAIN.as_millis() as u32;
        let drain = wait(name, |s| s.dwCurrentState == SERVICE_STOP_PENDING && s.dwWaitHint == hint)?;
        // The handler gives the first checkpoint, the drain of the work the
        // second.
        if drain.dwCheckPoint < 2 || drain.dwControlsAccepted != 0 {
            return Err(format!("the drain: {}", text(&drain)));
        }
        std::fs::write(dir.join(DRAINED), b"").map_err(|e| format!("the file of the drain: {e}"))?;
        let end = wait(name, |s| s.dwCurrentState == SERVICE_STOPPED)?;
        Ok((end.dwWin32ExitCode, end.dwServiceSpecificExitCode))
    }

    fn text(s: &SERVICE_STATUS) -> String {
        format!(
            "state {}, accepts {}, exit {} {}, checkpoint {}, hint {} ms",
            s.dwCurrentState,
            s.dwControlsAccepted,
            s.dwWin32ExitCode,
            s.dwServiceSpecificExitCode,
            s.dwCheckPoint,
            s.dwWaitHint
        )
    }

    /// Poll `test` every 20 ms until it is true, at most `LIMIT`.
    fn until(mut test: impl FnMut() -> bool) -> bool {
        let end = Instant::now() + LIMIT;
        while Instant::now() < end {
            if test() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        test()
    }

    /// The status of the service when `test` accepts it, within `LIMIT`. A
    /// stopped service that `test` does not accept fails at once.
    fn wait(name: &str, test: impl Fn(&SERVICE_STATUS) -> bool) -> Result<SERVICE_STATUS, String> {
        let mut last = Err("no query".to_string());
        let found = until(|| {
            last = query(name).map_err(|e| format!("the query of the service: Win32 error {e}"));
            match &last {
                Ok(s) => test(s) || s.dwCurrentState == SERVICE_STOPPED,
                Err(_) => false,
            }
        });
        let status = last?;
        if found && test(&status) {
            return Ok(status);
        }
        Err(format!("the service did not reach the state of the test: {}", text(&status)))
    }

    /// Run `call` on the service `name` opened with `access`.
    fn with_service<T>(name: &str, access: u32, call: impl FnOnce(SC_HANDLE) -> Result<T, u32>) -> Result<T, u32> {
        let last = || std::io::Error::last_os_error().raw_os_error().unwrap_or(-1) as u32;
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        // SAFETY: null names give the manager of this machine.
        let manager = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
        if manager.is_null() {
            return Err(last());
        }
        // SAFETY: the manager is open, and the name ends with a 0.
        let service = unsafe { OpenServiceW(manager, wide.as_ptr(), access) };
        let out = if service.is_null() { Err(last()) } else { call(service) };
        // SAFETY: each handle is open, and this is its one close.
        unsafe {
            if !service.is_null() {
                CloseServiceHandle(service);
            }
            CloseServiceHandle(manager);
        }
        out
    }

    fn query(name: &str) -> Result<SERVICE_STATUS, u32> {
        with_service(name, SERVICE_QUERY_STATUS, |service| {
            let mut status = SERVICE_STATUS::default();
            // SAFETY: the handle is open with SERVICE_QUERY_STATUS.
            if unsafe { QueryServiceStatus(service, &mut status) } == 0 {
                return Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(-1) as u32);
            }
            Ok(status)
        })
    }

    fn control_stop(name: &str) -> Result<SERVICE_STATUS, String> {
        with_service(name, SERVICE_STOP, |service| {
            let mut status = SERVICE_STATUS::default();
            // SAFETY: the handle is open with SERVICE_STOP.
            if unsafe { ControlService(service, SERVICE_CONTROL_STOP, &mut status) } == 0 {
                return Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(-1) as u32);
            }
            Ok(status)
        })
        .map_err(|e| format!("the stop of the service: Win32 error {e}"))
    }
}
