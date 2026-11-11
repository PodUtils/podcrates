//! pod-hold: the hold of each connect of a payload, with no root.
//!
//! Two holds, each with its level ([`level::Level`]) and a proof:
//!
//! * The seccomp hold (`advisory`): a filter that returns
//!   `SECCOMP_RET_USER_NOTIF` stops the socket calls of the payload
//!   ([`filter`], [`spawn`]). The supervisor takes each stopped call
//!   ([`listen`]), reads the memory of its task ([`mem`], [`sockaddr`]),
//!   keeps the sockets of the payload in a table with a flow budget
//!   ([`table`]), and answers ([`reply`]: the seam; [`reply::Recorded`]
//!   stands in for the kernel in a test, and a memfd for the memory).
//! * The netns hold (`enforced`): the payload runs in a new network
//!   namespace whose only route is a TUN that the supervisor holds
//!   ([`netns`]).
//!
//! The decisions (where a connect goes, the rules, the relays, the stack
//! on the TUN) are the consumer's. The crate reads no environment
//! variable, prints nothing and keeps no global state.
//!
//! Linux only: on another target the crate is empty.

#![cfg(target_os = "linux")]

pub mod abi;
pub mod connect;
mod failure;
pub mod filter;
pub mod level;
pub mod listen;
pub mod mem;
pub mod netns;
pub mod packet;
pub mod reply;
pub mod sockaddr;
pub mod spawn;
pub mod table;

pub use level::{Hold, Level};
