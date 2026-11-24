//! The device of a Unix kernel (`/dev/tpmrm0`): one write of a command, one
//! read of its response. A socket pair or a FIFO takes the place of the
//! device; the test of the device of this host skips with its reason when
//! the host has none.
#![cfg(all(feature = "device", unix))]

mod standin;

use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use pod_core::Class;
use pod_tpm::transport::{Device, Limits};
use pod_tpm::wire::MAX_RESPONSE;
use pod_tpm::{Transport, cmd, transact};
use standin::{Answer, Frame, ended, header_saying, response, serve};

const WAIT: Duration = Duration::from_secs(10);

fn fake(limits: Limits, answers: Vec<Answer>) -> (Device, std::sync::mpsc::Receiver<standin::Seen>) {
    let (ours, theirs) = UnixStream::pair().expect("socketpair");
    let device = Device::from_fd(OwnedFd::from(ours), "the fake device", limits).expect("a device");
    (device, serve(theirs, Frame::Raw, answers))
}

/// A directory of this test alone, short enough for a socket path.
fn scratch(name: &str) -> PathBuf {
    let dir = pod_testkit::host::socket_dir(&format!("pod-tpm-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map(|()| dir).expect("a directory")
}

fn mkfifo(path: &Path, mode: libc::mode_t) {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: a path that ends in NUL, for the length of the call.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), mode) }, 0, "mkfifo {}", path.display());
}

#[test]
fn a_device_takes_one_write_and_gives_one_read() {
    let command = cmd::flush_context(0x8000_0000).unwrap();
    let (mut device, rx) = fake(Limits::default(), vec![Answer::Whole(response(10)), Answer::Whole(response(10))]);
    transact(&mut device, &command).expect("a response").empty().expect("no parameter");
    transact(&mut device, &command).expect("a second response");
    drop(device);
    let seen = ended(&rx, WAIT);
    assert_eq!(seen.commands, vec![command.clone(), command]);
    assert!(seen.closed, "the device stays open after the drop");
}

#[test]
fn a_response_of_the_bound_is_read_whole_from_a_device() {
    let big = response(MAX_RESPONSE);
    let (mut device, _rx) = fake(Limits::default(), vec![Answer::Whole(big.clone())]);
    assert_eq!(device.submit(&cmd::flush_context(1).unwrap()).expect("a response"), big);
}

#[test]
fn a_response_over_the_bound_of_a_device_is_refused() {
    let over = (MAX_RESPONSE + 1) as u32;
    for bytes in [header_saying(over, MAX_RESPONSE + 1 - 10), response(MAX_RESPONSE + 64)] {
        let (mut device, rx) = fake(Limits::default(), vec![Answer::Bytes(bytes)]);
        let e = device.submit(&cmd::flush_context(1).unwrap()).map(|r| r.len()).expect_err("a response over the bound");
        assert_eq!(e.class, Class::Protocol, "{e}");
        let again = device.submit(&cmd::flush_context(1).unwrap()).expect_err("a broken device");
        assert_eq!(again.class, Class::Unavailable, "{again}");
        drop(device);
        drop(rx);
    }
}

#[test]
fn a_short_response_of_a_device_is_a_protocol_failure() {
    for bytes in [header_saying(20, 2), vec![0x80, 0x01, 0, 0]] {
        let (mut device, _rx) = fake(Limits::default(), vec![Answer::Bytes(bytes)]);
        let e = device.submit(&cmd::flush_context(1).unwrap()).expect_err("a short response");
        assert_eq!(e.class, Class::Protocol, "{e}");
    }
}

#[test]
fn a_late_device_is_a_timeout_and_is_closed() {
    let (mut device, rx) = fake(Limits { command: Duration::from_millis(200) }, vec![Answer::Silent]);
    let e = device.submit(&cmd::flush_context(1).unwrap()).expect_err("no answer");
    assert_eq!(e.class, Class::Timeout, "{e}");
    assert!(ended(&rx, WAIT).closed, "the device stays open after the time limit");
    let again = device.submit(&cmd::flush_context(1).unwrap()).expect_err("a broken device");
    assert_eq!(again.class, Class::Unavailable, "{again}");
}

#[test]
fn a_closed_device_is_unavailable() {
    let (mut device, _rx) = fake(Limits::default(), vec![Answer::Close]);
    let e = device.submit(&cmd::flush_context(1).unwrap()).expect_err("a closed device");
    assert_eq!(e.class, Class::Unavailable, "{e}");
}

#[test]
fn a_fifo_gives_back_the_command() {
    let dir = scratch("fifo");
    let path = dir.join("tpmrm0");
    mkfifo(&path, 0o600);
    let mut device = Device::open(&path, Limits::default()).expect("the FIFO opens");
    let command = cmd::flush_context(0x8000_0000).unwrap();
    assert_eq!(device.submit(&command).expect("the bytes come back"), command);
    drop(device);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_device_is_unavailable() {
    let dir = scratch("missing");
    let path = dir.join("tpmrm0");
    let e = Device::open(&path, Limits::default()).err().expect("no device");
    assert_eq!(e.class, Class::Unavailable, "{e}");
    assert!(e.text().contains(&path.display().to_string()), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_refused_device_is_a_permission_failure() {
    let dir = scratch("refused");
    let path = dir.join("tpmrm0");
    mkfifo(&path, 0);
    if std::fs::OpenOptions::new().read(true).write(true).open(&path).is_ok() {
        pod_testkit::host::skip_line("this user opens a file of mode 0 (root): no refusal to test");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let e = Device::open(&path, Limits::default()).err().expect("a refusal");
    assert_eq!(e.class, Class::Permission, "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_that_is_no_device_is_refused() {
    let dir = scratch("nodevice");
    let file = dir.join("tpmrm0");
    std::fs::write(&file, b"no device").unwrap();
    let socket = dir.join("swtpm.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).expect("bind");
    for path in [&file, &dir, &socket] {
        let e = Device::open(path, Limits::default()).err().expect("no device");
        assert_eq!(e.class, Class::Unavailable, "{e}");
        assert!(e.text().contains(&path.display().to_string()), "{e}");
    }
    let e = Device::open(&socket, Limits::default()).err().unwrap();
    assert!(e.text().contains("Socket"), "{e}");
    assert_eq!(std::fs::read(&file).unwrap(), b"no device");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_tpm_of_this_host_answers() {
    let path = Path::new("/dev/tpmrm0");
    if !path.exists() {
        pod_testkit::host::skip_line("this host has no /dev/tpmrm0 (no TPM, or a container)");
        return;
    }
    let mut device = match Device::open(path, Limits::default()) {
        Ok(device) => device,
        Err(e) if e.class == Class::Permission => {
            pod_testkit::host::skip_line(&format!("this user may not open /dev/tpmrm0: {e}"));
            return;
        }
        Err(e) => panic!("{e}"),
    };
    let caps = pod_tpm::flow::pcr::capabilities(&mut device).expect("the capabilities");
    assert!(caps.manufacturer.is_some(), "no manufacturer");
}
