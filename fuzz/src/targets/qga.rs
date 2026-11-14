//! The answers of the guest agent of `pod-qga` (T-594).

use pod_qga::wire::{self, Answer, Chunk, ExecStatus};

/// One answer line of the QEMU Guest Agent (`pod_qga::wire::answer`; new
/// in `pod-qga`, T-594), and each typed return of it: the same answer
/// twice; no typed return panics; the bytes of an exec status or of a
/// chunk are not more than the bytes of the line; a chunk holds as many
/// bytes as its count; the text of a refusal comes from the line.
pub fn qga_answer(data: &[u8]) {
    let first = wire::answer(data);
    assert_eq!(first, wire::answer(data));
    match first {
        Ok(Answer::Return(v)) => {
            let _ = (wire::sync_id(&v), wire::exec_pid(&v), wire::file_handle(&v), wire::written(&v));
            if let Ok(s) = ExecStatus::from_return(&v) {
                assert!(s.stdout.len() + s.stderr.len() <= data.len());
            }
            if let Ok(c) = Chunk::from_return(&v) {
                assert!(c.data.len() <= data.len());
                assert_eq!(v["count"].as_u64(), Some(c.data.len() as u64));
            }
        }
        Ok(Answer::Error { class, desc }) => assert!(class.len() + desc.len() <= data.len()),
        Err(why) => assert!(!why.is_empty()),
    }
}
