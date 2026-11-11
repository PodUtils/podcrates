//! The confinement of a payload with no privilege: Landlock rules for the
//! paths and the TCP ports, a seccomp filter that denies a list of system
//! calls, and `no_new_privs`.
//!
//! The work has three steps, so that the child between its fork and its
//! exec only issues system calls on data that exists:
//!
//! 1. [`check`] (or [`probe`]) before the fork: the Landlock ABI of the
//!    kernel, measured in a disposable child that also restricts itself, so
//!    that a nested ruleset under the policy of the host is proven. The
//!    caller is not restricted by it.
//! 2. [`Spec::compile`] (host paths) or [`Spec::compile_in`] (paths of a
//!    guest root) before the fork: each path opens as `O_PATH`, a path that
//!    does not exist is a refusal by name, and the BPF program is built.
//! 3. [`apply`] in the child: `PR_SET_NO_NEW_PRIVS`, the ruleset, its
//!    rules, `landlock_restrict_self`, then the seccomp filter (mode 2). No
//!    allocation. A refusal ([`Refusal`]) names the call and the errno.
//!
//! There is no unconfined way through: [`Spec::compile`] refuses a kernel
//! with no Landlock or with an ABI under [`MIN_ABI`], and [`apply`] refuses
//! each failed call, so the caller does not exec.
//!
//! The crate reads no environment variable, prints nothing and keeps no
//! global state. A failure of a compile or of the probe is a
//! `pod_core::Failure` at the stage `host`. Linux only: on another target
//! the crate is empty.
#![cfg(target_os = "linux")]
#![forbid(unsafe_op_in_unsafe_fn)]

mod apply;
mod compile;
pub mod deny;
mod probe;
pub mod spec;

pub use apply::{Call, Refusal, apply};
pub use compile::{Compiled, MOST_RULES, Rule};
pub use deny::Deny;
pub use probe::{check, probe};
pub use spec::{Access, PathRule, Spec};

use pod_core::{Class, Failure, Stage};
use pod_hostprobe::sys::Errno;

/// The lowest Landlock ABI that [`Spec::compile`] takes. ABI 1 and 2 do not
/// handle `truncate(2)`: a payload could cut each file that its user can
/// write, also outside the write list.
pub const MIN_ABI: u32 = 3;

/// The lowest ABI with TCP rules (`LANDLOCK_ACCESS_NET_*`).
pub const NET_ABI: u32 = 4;

/// A failure of the host.
pub(crate) fn failure(class: Class, text: impl Into<String>) -> Failure {
    Failure::new(Stage::Host, class, text)
}

/// The class of a failure for an errno of the kernel.
pub(crate) fn errno_class(e: Errno) -> Class {
    match e.0 {
        2 | 20 => Class::NotFound,
        1 | 13 => Class::Refused,
        38 | 95 => Class::Unsupported,
        _ => Class::Unavailable,
    }
}

/// The name and the number of an errno: `EACCES (13)`.
pub(crate) fn errno_text(e: Errno) -> String {
    format!("{} ({})", e.name(), e.0)
}
