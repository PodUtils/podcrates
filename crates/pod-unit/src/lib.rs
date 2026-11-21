//! Hardened service units with no secret in them (T-102). A program
//! describes its service once as a [`Unit`]; a renderer gives each form:
//!
//! * [`systemd`]: a user unit, or a unit of the system that runs as a named
//!   user or as an account of the service alone (`DynamicUser=yes`), with
//!   the hardening of [`Hardening`] and the credentials of systemd;
//! * [`launchd`]: a LaunchAgent, or a LaunchDaemon of a named user;
//! * [`windows`]: a task at the logon of the user, or a Windows service of a
//!   virtual account with a restricted SID (the commands of `sc.exe`);
//! * [`cron`]: the lines of the crontab of the user.
//!
//! No word of a unit is a secret ([`secret`]): a secret is a private file
//! that the host resolves at the start ([`Secret`]), or a placeholder that
//! the broker swaps ([`Env::placeholder`]). [`install`] installs and
//! removes a unit of the user, and, as root or an administrator, a unit of
//! the system, each an explicit call. The command line
//! (`--print-unit`, `--install`) stays in each program.
//!
//! The crate prints nothing and does not exit. Each failure is a
//! [`pod_core::Failure`] of the stage `host`.

pub mod cron;
pub mod hardening;
pub mod install;
mod install_system;
mod install_ways;
pub mod launchd;
mod model;
pub mod quote;
pub mod secret;
pub mod systemd;
pub mod windows;

pub use hardening::Hardening;
pub use install::{FileMeta, Form, Host, Places, Removed, System, SystemPlaces};
pub use model::{Env, Restart, Scope, Secret, Start, Stop, Unit};
