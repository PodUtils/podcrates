use super::*;

/// Each errno of a refused `execve`, its name, and the exit code of the
/// payload: 126 for a file that was found and refused, 127 for a path
/// with no file (PodUtils/podvm#639). The name ties each number to its
/// errno on the target.
const ROWS: &[(i32, &str, i32)] = &[
    (1, "EPERM", CANNOT_INVOKE),
    (7, "E2BIG", CANNOT_INVOKE),
    (8, "ENOEXEC", CANNOT_INVOKE),
    (13, "EACCES", CANNOT_INVOKE),
    (21, "EISDIR", CANNOT_INVOKE),
    (22, "EINVAL", CANNOT_INVOKE),
    (26, "ETXTBSY", CANNOT_INVOKE),
    (80, "ELIBBAD", CANNOT_INVOKE),
    (2, "ENOENT", NOT_FOUND),
    (20, "ENOTDIR", NOT_FOUND),
    (36, "ENAMETOOLONG", NOT_FOUND),
    (40, "ELOOP", NOT_FOUND),
    (12, "ENOMEM", NOT_FOUND),
];

/// The three steps that exec the payload (the candidates of the chroot
/// and the namespace rungs, the memfd, the argv of the userland rung)
/// give the same code for each errno, and the text names the errno as
/// `NAME (N)`.
#[test]
fn each_errno_of_a_refused_execve_gives_the_code_of_the_shell() {
    let p = crate::tests::plain_plan();
    for &(n, name, code) in ROWS {
        let e = sys::Errno(n);
        assert_eq!(e.name(), name, "errno {n}");
        for step in [4u8, 5, 6] {
            let err = step_failure(&p, step, e, 0);
            assert_eq!(err.payload_code(), Some(code), "{name} at step {step}: {err}");
            assert!(err.to_string().contains(&format!("{name} ({n})")), "{name} at step {step}: {err}");
        }
    }
}
