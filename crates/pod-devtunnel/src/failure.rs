//! The named failures of the crate. Each failure is a [`pod_core::Failure`]
//! with its stage and class; its chain holds a [`Failed`] that names the
//! step that broke ([`step_of`] reads it), so that a consumer tells the
//! management API from the relay, SSH, a channel and a reconnect with no
//! parse of words.

use std::error::Error;
use std::fmt;

use pod_core::{Class, Failure, Stage};

/// The step of a connection that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The management API: the user's token, the tunnel, its connect
    /// information.
    Management,
    /// The relay WebSocket: the dial, TLS, the upgrade with the connect
    /// token.
    WebSocket,
    /// The SSH session to the host: the host key, the authentication.
    Ssh,
    /// A `direct-tcpip` channel to a port of the host.
    Channel,
    /// A reconnect that used each of its tries.
    Reconnect,
}

impl Step {
    /// The words of the step at the start of a failure's text.
    pub fn words(self) -> &'static str {
        match self {
            Step::Management => "the Dev Tunnels management API",
            Step::WebSocket => "the Dev Tunnels relay WebSocket",
            Step::Ssh => "the SSH session of the Dev Tunnels relay",
            Step::Channel => "the channel of the Dev Tunnels relay",
            Step::Reconnect => "the reconnect of the Dev Tunnels relay",
        }
    }

    /// The stage of a failure of this step that has no cause of its own.
    pub fn stage(self) -> Stage {
        match self {
            Step::Management | Step::Ssh | Step::Reconnect => Stage::Relay,
            Step::WebSocket => Stage::WebSocket,
            // The port of the host is the origin behind the tunnel.
            Step::Channel => Stage::Origin,
        }
    }
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.words())
    }
}

/// The mark of a step in the chain of a [`Failure`]: its text is the words
/// of the step, which the failure's text holds already, so
/// [`Failure::chain`] does not name them twice.
#[derive(Debug)]
pub struct Failed {
    pub step: Step,
    cause: Option<Failure>,
}

impl fmt::Display for Failed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.step.words())
    }
}

impl Error for Failed {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause.as_ref().map(|c| c as &(dyn Error + 'static))
    }
}

/// The first step that the chain of `failure` names: the outer one, as a
/// reconnect holds the failure of its last try.
pub fn step_of(failure: &Failure) -> Option<Step> {
    steps_of(failure).first().copied()
}

/// Each step that the chain of `failure` names, the outer first.
pub fn steps_of(failure: &Failure) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut next: Option<&(dyn Error + 'static)> = Some(failure);
    while let Some(e) = next {
        if let Some(failed) = e.downcast_ref::<Failed>() {
            steps.push(failed.step);
        }
        next = e.source();
    }
    steps
}

/// A failure of `step` with no cause: its stage, `class`, and `what`
/// after the words of the step.
pub(crate) fn fail(step: Step, class: Class, what: impl fmt::Display) -> Failure {
    Failure::caused(step.stage(), class, format!("{}: {what}", step.words()), Failed { step, cause: None })
}

/// A failure of `step` whose cause is `cause`: the stage and the class of
/// the cause, which knows where it broke.
pub(crate) fn wrap(step: Step, what: impl fmt::Display, cause: Failure) -> Failure {
    let (stage, class) = (cause.stage, cause.class);
    Failure::caused(stage, class, format!("{}: {what}", step.words()), Failed { step, cause: Some(cause) })
}

/// The failure of an SSH step: its class from the error of russh.
pub(crate) fn ssh(step: Step, what: impl fmt::Display, e: &russh::Error) -> Failure {
    let class = match e {
        russh::Error::UnknownKey | russh::Error::NotAuthenticated | russh::Error::NoAuthMethod => Class::Refused,
        russh::Error::ConnectionTimeout
        | russh::Error::KeepaliveTimeout
        | russh::Error::InactivityTimeout
        | russh::Error::Elapsed(_) => Class::Timeout,
        russh::Error::Disconnect
        | russh::Error::HUP
        | russh::Error::IO(_)
        | russh::Error::SendError
        | russh::Error::RecvError => Class::Unavailable,
        russh::Error::ChannelOpenFailure(why) => channel_class(why),
        _ => Class::Protocol,
    };
    fail(step, class, format!("{what}: {e}"))
}

/// The class of a refused channel (RFC 4254 section 5.1).
pub(crate) fn channel_class(why: &russh::ChannelOpenFailure) -> Class {
    match why {
        russh::ChannelOpenFailure::AdministrativelyProhibited => Class::Refused,
        russh::ChannelOpenFailure::ConnectFailed => Class::Unavailable,
        russh::ChannelOpenFailure::UnknownChannelType => Class::Unsupported,
        russh::ChannelOpenFailure::ResourceShortage => Class::TempFail,
        _ => Class::Protocol,
    }
}

/// True when an error of russh says that the session is gone, not that the
/// far end refused one request: the relay must connect again.
pub(crate) fn session_gone(e: &russh::Error) -> bool {
    matches!(
        e,
        russh::Error::Disconnect
            | russh::Error::HUP
            | russh::Error::IO(_)
            | russh::Error::SendError
            | russh::Error::RecvError
            | russh::Error::KeepaliveTimeout
            | russh::Error::InactivityTimeout
            | russh::Error::WrongChannel
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_names_its_step_once_and_keeps_the_stage_of_its_cause() {
        let cause = Failure::new(Stage::Tls, Class::Refused, "TLS with the relay failed: bad name");
        let failure = wrap(Step::WebSocket, "wss://relay.test/c", cause);
        assert_eq!((failure.stage, failure.class), (Stage::Tls, Class::Refused));
        assert_eq!(step_of(&failure), Some(Step::WebSocket));
        assert_eq!(
            failure.chain(),
            "the Dev Tunnels relay WebSocket: wss://relay.test/c: TLS with the relay failed: bad name"
        );
        let outer = wrap(Step::Reconnect, "3 tries failed", failure);
        assert_eq!(steps_of(&outer), [Step::Reconnect, Step::WebSocket]);
        assert_eq!(outer.class, Class::Refused);
        let alone = fail(Step::Channel, Class::Refused, "port 22 refused");
        assert_eq!((alone.stage, step_of(&alone)), (Stage::Origin, Some(Step::Channel)));
        assert_eq!(alone.chain(), "the channel of the Dev Tunnels relay: port 22 refused");
    }

    #[test]
    fn a_refused_channel_is_no_end_of_the_session() {
        let refused = russh::Error::ChannelOpenFailure(russh::ChannelOpenFailure::ConnectFailed);
        assert!(!session_gone(&refused));
        assert_eq!(ssh(Step::Channel, "port 9", &refused).class, Class::Unavailable);
        assert!(session_gone(&russh::Error::SendError));
        assert_eq!(ssh(Step::Ssh, "the host key", &russh::Error::UnknownKey).class, Class::Refused);
    }
}
