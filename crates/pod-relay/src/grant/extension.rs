//! The extensions of a user certificate, as OpenSSH's PROTOCOL.certkeys
//! names them. A stock sshd that takes a certificate by `TrustedUserCAKeys`
//! gives a pty, a forward, the agent, X11 and `~/.ssh/rc` only for the
//! extension of each. Each extension of OpenSSH is a flag with empty data,
//! and sshd refuses a certificate whose flag has data, so a variant writes
//! the name and its data together.

/// One extension of a user certificate, which [`super::Mint::sign_with`]
/// writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Extension<'a> {
    /// `permit-X11-forwarding`: X11 forwarding.
    PermitX11Forwarding,
    /// `permit-agent-forwarding`: agent forwarding.
    PermitAgentForwarding,
    /// `permit-port-forwarding`: each port forward, local and remote.
    PermitPortForwarding,
    /// `permit-pty`: a pty, which an interactive shell needs.
    PermitPty,
    /// `permit-user-rc`: sshd runs `~/.ssh/rc`.
    PermitUserRc,
    /// `no-touch-required`: a signature of a FIDO key needs no touch.
    NoTouchRequired,
    /// Another extension: its name (`NAME@DOMAIN` for a custom one, as
    /// PROTOCOL.certkeys asks) and its data, written as they are given. A
    /// reader that does not know the name ignores it.
    Other(&'a str, &'a [u8]),
}

impl<'a> Extension<'a> {
    /// What `ssh-keygen -s` writes when no `-O` changes it, in the order of
    /// the names: the five permits.
    pub const OPENSSH_DEFAULT: [Extension<'static>; 5] = [
        Extension::PermitX11Forwarding,
        Extension::PermitAgentForwarding,
        Extension::PermitPortForwarding,
        Extension::PermitPty,
        Extension::PermitUserRc,
    ];

    /// Each extension of OpenSSH, with its name.
    const KNOWN: [(Extension<'static>, &'static str); 6] = [
        (Extension::PermitX11Forwarding, "permit-X11-forwarding"),
        (Extension::PermitAgentForwarding, "permit-agent-forwarding"),
        (Extension::PermitPortForwarding, "permit-port-forwarding"),
        (Extension::PermitPty, "permit-pty"),
        (Extension::PermitUserRc, "permit-user-rc"),
        (Extension::NoTouchRequired, "no-touch-required"),
    ];

    /// The name, as the certificate holds it.
    pub fn name(&self) -> &'a str {
        match self {
            Extension::Other(name, _) => name,
            known => Self::KNOWN.iter().find(|(e, _)| e == known).map_or("", |(_, name)| name),
        }
    }

    /// The data: empty for each extension of OpenSSH.
    pub fn data(&self) -> &'a [u8] {
        match self {
            Extension::Other(_, data) => data,
            _ => &[],
        }
    }

    /// The extension of OpenSSH that `name` names, if any: so a reader of a
    /// configuration gives the typed form.
    pub fn known(name: &str) -> Option<Extension<'static>> {
        Self::KNOWN.iter().find(|(_, n)| *n == name).map(|(e, _)| *e)
    }
}

#[cfg(test)]
#[path = "extension_tests.rs"]
mod tests;
