//! pod-tpm: a command, and the reader of its response through a
//! transport that answers with a refusal of the policy.

pub fn tpm() -> String {
    let mut refuse = |_: &[u8]| Ok::<_, pod_core::Failure>(vec![0x80, 0x01, 0, 0, 0, 10, 0, 0, 0x09, 0x9D]);
    let line = pod_tpm::cmd::get_manufacturer()
        .and_then(|command| pod_tpm::transact(&mut refuse, &command))
        .map_or_else(|e| e.to_string(), |r| format!("{} bytes", r.params.len()));
    format!("pod-tpm: {line}")
}
