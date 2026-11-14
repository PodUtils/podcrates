//! pod-qga: the reader of an answer on each target, and on Unix a connect
//! to a socket where no agent answers.

pub fn qga() -> String {
    let status = match pod_qga::wire::answer(br#"{"return": {"exited": true, "exitcode": 0, "out-data": "b2s="}}"#) {
        Ok(pod_qga::Answer::Return(v)) => {
            pod_qga::ExecStatus::from_return(&v).map_or_else(|e| e, |s| String::from_utf8_lossy(&s.stdout).into_owned())
        }
        other => format!("{other:?}"),
    };
    format!("pod-qga: {status}, {}", connect())
}

#[cfg(unix)]
fn connect() -> String {
    pod_qga::Client::connect(pod_qga::Settings::new("/nonexistent/qga.sock"))
        .map_or_else(|e| e.to_string(), |_| "an agent".to_string())
}

#[cfg(not(unix))]
fn connect() -> String {
    "no client on this target".to_string()
}
