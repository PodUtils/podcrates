//! The offline tests of each watch kind, each against a stand-in of
//! `pod-testkit` on the loopback: a file that the live service matches
//! holds, and a file that it does not match gives a finding.

mod apis;
mod common;
mod data;
mod dns;
mod files;
mod http;
mod ssh;
mod ws;
mod xhttp;
