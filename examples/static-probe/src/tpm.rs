//! pod-tpm: a command, and the reader of its response through a
//! transport that answers with a refusal of the policy; the transports of
//! the features `device` and `tbs`, each opened where no TPM answers.

pub fn tpm() -> String {
    let mut refuse = |_: &[u8]| Ok::<_, pod_core::Failure>(vec![0x80, 0x01, 0, 0, 0, 10, 0, 0, 0x09, 0x9D]);
    let line = pod_tpm::cmd::get_manufacturer()
        .and_then(|command| pod_tpm::transact(&mut refuse, &command))
        .map_or_else(|e| e.to_string(), |r| format!("{} bytes", r.params.len()));
    format!("pod-tpm: {line}; {}; {}", transport(), tbs())
}

/// The device and the socket, where no TPM is.
#[cfg(unix)]
fn transport() -> String {
    use pod_tpm::transport::{Device, Limits, Socket};
    let path = std::path::Path::new("/nonexistent/pod-tpm/tpmrm0");
    let device = Device::open(path, Limits::default()).map_or_else(|e| e.class.to_string(), |_| "a device".into());
    let socket =
        Socket::connect_unix(path, Limits::default()).map_or_else(|e| e.class.to_string(), |_| "a socket".into());
    format!("device {device}, socket {socket}")
}

#[cfg(not(unix))]
fn transport() -> String {
    // A port of the loopback where a closed socket was: no TPM answers.
    let gone = std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr());
    let limits = pod_tpm::transport::Limits::default();
    gone.map_or_else(
        |e| e.to_string(),
        |addr| {
            pod_tpm::transport::Socket::connect_tcp(addr, limits)
                .map_or_else(|e| format!("socket {}", e.class), |_| "a socket".into())
        },
    )
}

/// The TBS of Windows; on another target, the map of its codes.
#[cfg(windows)]
fn tbs() -> String {
    pod_tpm::transport::tbs::Tbs::open().map_or_else(|e| format!("tbs {}", e.class), |_| "a TBS context".into())
}

#[cfg(not(windows))]
fn tbs() -> String {
    pod_tpm::transport::tbs::failure("Tbsi_Context_Create", pod_tpm::transport::tbs::TBS_E_TPM_NOT_FOUND).to_string()
}
