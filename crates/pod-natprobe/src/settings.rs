//! Configuration types for a NAT probe run. No environment variable is read by this crate.

use std::net::SocketAddr;

use crate::classify::COARSE_RUNGS_SECS;
use crate::report::Profile;
use crate::wire::AddrPolicy;

/// One resolved STUN server endpoint supplied by the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerEndpoint {
    pub name: String,
    pub addr: SocketAddr,
    /// Hint that this endpoint supports RFC 5780 (`OTHER-ADDRESS` and `CHANGE-REQUEST`).
    pub rfc5780: bool,
}

impl ServerEndpoint {
    /// Construct a server endpoint with a label and socket address.
    pub fn new(name: impl Into<String>, addr: SocketAddr) -> Self {
        Self { name: name.into(), addr, rfc5780: false }
    }

    /// Construct a server endpoint known to support RFC 5780 (`OTHER-ADDRESS`, `CHANGE-REQUEST`).
    pub fn rfc5780(name: impl Into<String>, addr: SocketAddr) -> Self {
        Self { name: name.into(), addr, rfc5780: true }
    }
}

/// Retransmission ladder selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LadderProfile {
    /// Fast adaptive ladder (`R = clamp(2 * srtt, 100, 500)`, up to 5 sends, new TID per send).
    #[default]
    Interactive,
    /// Conservative RFC 8489 ladder (`500 ms` doubling, 7 sends over `39.5 s`, same TID).
    Rfc8489,
}

/// Token bucket limits for new STUN transactions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BucketConfig {
    pub burst: u32,
    pub refill_per_sec: u32,
    pub max_inflight_per_server: usize,
}

impl BucketConfig {
    /// Fast profile bucket: burst 24, refill 10/s, max 10 outstanding per server IP.
    pub fn fast() -> Self {
        Self { burst: 24, refill_per_sec: 10, max_inflight_per_server: 10 }
    }

    /// Standard/Deep bucket obeying RFC 5780 section 5 (`10/s`, burst 10).
    pub fn standard() -> Self {
        Self { burst: 10, refill_per_sec: 10, max_inflight_per_server: 10 }
    }
}

/// Settings for opt-in UDP mapping lifetime measurement (RFC 5780 section 4.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifetimeSettings {
    /// Coarse idle durations in seconds (default `[5, 10, 20, 40, 80, 160, 240]`).
    pub coarse_rungs_secs: Vec<u64>,
    /// Number of fine geometric rungs inside `(low, high)` (0 disables fine pass).
    pub run_fine_rungs: bool,
    /// Number of repetitions (keeps maximum bracket per RFC 5780 section 4.6).
    pub repetitions: u8,
}

impl Default for LifetimeSettings {
    fn default() -> Self {
        Self { coarse_rungs_secs: COARSE_RUNGS_SECS.to_vec(), run_fine_rungs: true, repetitions: 1 }
    }
}

/// Complete caller-owned settings for one NAT probe run.
#[derive(Clone, Debug)]
pub struct Settings {
    pub profile: Profile,
    pub servers_v4: Vec<ServerEndpoint>,
    pub servers_v6: Vec<ServerEndpoint>,
    pub addr_policy: AddrPolicy,
    pub ladder: LadderProfile,
    pub bucket: BucketConfig,
    /// Hard wall-clock cap for the run in milliseconds.
    pub max_duration_ms: u32,
    /// Number of fresh allocation sockets (`F` counts as sample 0).
    pub alloc_samples: usize,
    /// Pacing between consecutive allocation sockets in milliseconds.
    pub alloc_pace_ms: u32,
    /// When true, hairpin probes send bidirectionally to open reverse filters.
    pub hairpin_punch_mode: bool,
    /// Enable UPnP IGD HTTP GET + read-only SOAP `GetExternalIPAddress`.
    pub run_upnp_http: bool,
    /// Enable RFC 7050 `ipv4only.arpa` DNS64/NAT64 query.
    pub run_nat64: bool,
    /// Enable single-socket 250 ms gateway portmapper probe.
    pub run_portmap: bool,
    /// Enable IPv6 STUN probe when IPv6 is routable and `servers_v6` is non-empty.
    pub run_ipv6: bool,
    /// Optional Deep-mode mapping lifetime probe.
    pub lifetime: Option<LifetimeSettings>,
    /// Optional SOFTWARE attribute string (omitted by default).
    pub software: Option<String>,
    /// Seed for retransmission jitter (`0` disables jitter for deterministic runs).
    pub jitter_seed: u64,
    /// Optional caller-supplied non-UDP online corroboration (`Some(true)` means TCP/HTTPS works).
    pub corroborated_online: Option<bool>,
}

impl Settings {
    /// Construct settings for `profile` with the given IPv4 STUN servers.
    pub fn for_profile(profile: Profile, servers_v4: Vec<ServerEndpoint>) -> Self {
        match profile {
            Profile::Fast => Self {
                profile,
                servers_v4,
                servers_v6: Vec::new(),
                addr_policy: AddrPolicy::Internet,
                ladder: LadderProfile::Interactive,
                bucket: BucketConfig::fast(),
                max_duration_ms: 600,
                alloc_samples: 3,
                alloc_pace_ms: 25,
                hairpin_punch_mode: false,
                run_upnp_http: false,
                run_nat64: true,
                run_portmap: true,
                run_ipv6: true,
                lifetime: None,
                software: None,
                jitter_seed: 0,
                corroborated_online: None,
            },
            Profile::Standard => Self {
                profile,
                servers_v4,
                servers_v6: Vec::new(),
                addr_policy: AddrPolicy::Internet,
                ladder: LadderProfile::Interactive,
                bucket: BucketConfig::standard(),
                max_duration_ms: 15_000,
                alloc_samples: 6,
                alloc_pace_ms: 100,
                hairpin_punch_mode: true,
                run_upnp_http: true,
                run_nat64: true,
                run_portmap: true,
                run_ipv6: true,
                lifetime: None,
                software: None,
                jitter_seed: 0,
                corroborated_online: None,
            },
            Profile::Deep => Self {
                profile,
                servers_v4,
                servers_v6: Vec::new(),
                addr_policy: AddrPolicy::Internet,
                ladder: LadderProfile::Interactive,
                bucket: BucketConfig::standard(),
                max_duration_ms: 600_000,
                alloc_samples: 8,
                alloc_pace_ms: 100,
                hairpin_punch_mode: true,
                run_upnp_http: true,
                run_nat64: true,
                run_portmap: true,
                run_ipv6: true,
                lifetime: Some(LifetimeSettings::default()),
                software: None,
                jitter_seed: 0,
                corroborated_online: None,
            },
        }
    }

    /// Attach IPv6 STUN server endpoints.
    pub fn with_servers_v6(mut self, servers_v6: Vec<ServerEndpoint>) -> Self {
        self.servers_v6 = servers_v6;
        self
    }

    /// Set the address classification policy (`Internet` or `Lab`).
    pub fn with_addr_policy(mut self, policy: AddrPolicy) -> Self {
        self.addr_policy = policy;
        self
    }

    /// Validate settings before starting a probe.
    pub fn validate(&self) -> Result<(), String> {
        if self.servers_v4.is_empty() && self.servers_v6.is_empty() {
            return Err("give one STUN server or more".to_string());
        }
        if self.servers_v4.iter().any(|s| !s.addr.is_ipv4()) {
            return Err("each address of servers_v4 must be an IPv4 address".to_string());
        }
        if self.servers_v6.iter().any(|s| !s.addr.is_ipv6()) {
            return Err("each address of servers_v6 must be an IPv6 address".to_string());
        }
        if self.alloc_samples > 32 {
            return Err("alloc_samples must be 32 or less".to_string());
        }
        if self.max_duration_ms == 0 {
            return Err("max_duration_ms must be more than zero".to_string());
        }
        if self.bucket.burst == 0 || self.bucket.refill_per_sec == 0 {
            return Err("the burst and the refill rate of the token bucket must be more than zero".to_string());
        }
        if let Some(ref sw) = self.software
            && (sw.is_empty() || sw.len() > 32)
        {
            return Err("the length of software must be 1 to 32 bytes".to_string());
        }
        Ok(())
    }
}
