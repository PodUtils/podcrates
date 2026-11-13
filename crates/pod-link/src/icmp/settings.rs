//! The settings of the ICMP echo carrier. podnet gives the payload (1400
//! bytes), 16 echoes in flight, 10 a second and a reply timeout of 1 s
//! (PodUtils/podnet#469, row E3-2). The count in flight is the ARQ window,
//! and the pace is the ARQ's, so the carrier keeps the payload alone and the
//! rest is the ARQ's (call C277).

/// The largest payload of one echo message by default: podnet's 1400 bytes.
pub const PAYLOAD: usize = 1400;

/// What the ICMP carrier sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IcmpSettings {
    /// The largest payload of one echo message (the ARQ datagram with the
    /// session), in bytes. 48 to 65407.
    pub payload: usize,
}

impl Default for IcmpSettings {
    fn default() -> IcmpSettings {
        IcmpSettings { payload: PAYLOAD }
    }
}

impl IcmpSettings {
    /// Each value against its limits.
    pub fn check(&self) -> Result<(), IcmpSettingsError> {
        if !(48..=65407).contains(&self.payload) {
            return Err(IcmpSettingsError::Payload(self.payload));
        }
        Ok(())
    }
}

/// A setting of the ICMP carrier out of its limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IcmpSettingsError {
    Payload(usize),
}

impl std::fmt::Display for IcmpSettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IcmpSettingsError::Payload(v) => write!(f, "a payload of {v} bytes: give 48 to 65407"),
        }
    }
}

impl std::error::Error for IcmpSettingsError {}
