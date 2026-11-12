//! The settings of the stack over a session: the MTU, the TCP buffers, the
//! congestion controller (podnet's `--cc` and `PODNET_WARP_CC`), the TCP
//! burst (`PODNET_WARP_TCP_BURST`), the TCP timeout, keepalive, linger and
//! Nagle, the counters of the links, the check of the data path and the
//! budget of a relay. The consumer keeps its flags and
//! its variables, parses them with [`parse_cc`] and [`parse_tcp_burst`], and
//! fills the fields.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use pod_netstack::device::DeviceStats;
use pod_netstack::smoltcp::socket::tcp::CongestionControl;
use pod_netstack::{StackConfig, Subnet};

use crate::check::DataPathCheck;
use crate::warp::MTU;

/// The send and the receive buffer of each TCP connection (PodUtils/podnet
/// issue 93: 128 KiB gave at most 2.8 MB/s at an RTT of 46 ms).
pub const TCP_BUFFER: usize = 512 * 1024;

/// The time limit of each TCP connection with no packet of the peer: a
/// connect, data with no ACK, or a keepalive with no answer (PodUtils/podnet
/// issue 125).
pub const TCP_TIMEOUT: Duration = Duration::from_secs(60);
/// The interval of the keepalive of an idle TCP connection (PodUtils/podnet
/// issue 125); shorter than [`TCP_TIMEOUT`], so that an idle connection to a
/// live peer stays.
pub const TCP_KEEPALIVE: Duration = Duration::from_secs(20);
/// The time that a closed TCP connection with no progress keeps its socket;
/// then the stack aborts it (PodUtils/podnet issue 125).
pub const TCP_LINGER: Duration = Duration::from_secs(30);

/// The largest TCP burst of [`TCP_BUFFER`] at [`MTU`]: more segments do not
/// fit in the receive buffer.
pub const TCP_BURST_MAX: usize = tcp_burst_max(TCP_BUFFER, MTU);

/// The largest TCP burst, in segments, of a buffer of `tcp_buffer` bytes at
/// the MTU `mtu`.
pub const fn tcp_burst_max(tcp_buffer: usize, mtu: usize) -> usize {
    let segment = if mtu > 60 { mtu - 60 } else { 1 };
    tcp_buffer.div_ceil(segment)
}

/// Whether the budget of the relay that carries the session is spent: a
/// function of the caller (podnet: `RelayBudget::spent` of `pod-relay`).
pub type Renew = Arc<dyn Fn() -> bool + Send + Sync>;

/// The settings of a [`crate::WarpNet`].
#[derive(Clone)]
pub struct NetSettings {
    /// The MTU of the tunnel. 1280 is the minimum of IPv6 and what the WARP
    /// client uses; the front door drops a larger packet.
    pub mtu: usize,
    /// The send and the receive buffer of each TCP connection.
    pub tcp_buffer: usize,
    /// The congestion controller of each TCP connection (PodUtils/podnet
    /// issue 134). None by default: the link holds the stack when its queue
    /// is full.
    pub congestion_control: CongestionControl,
    /// The TCP window in segments of the MTU, from 1 to
    /// [`tcp_burst_max`] (PodUtils/podnet issue 100). None: no limit, the
    /// default (with smoltcp 0.12 and later no window stalls).
    pub tcp_burst: Option<usize>,
    /// The time limit of each TCP connection with no packet of the peer
    /// ([`pod_netstack::StackConfig::tcp_timeout`]).
    pub tcp_timeout: Option<Duration>,
    /// The interval of the TCP keepalive of an idle connection.
    pub tcp_keepalive: Option<Duration>,
    /// The time that a closed TCP connection with no progress keeps its
    /// socket ([`pod_netstack::StackConfig::linger`]).
    pub tcp_linger: Duration,
    /// No Nagle on each TCP connection (podssh: a keystroke of SSH goes at
    /// once, with no wait of one round trip through WARP for the ACK of
    /// the data before it).
    pub nodelay: bool,
    /// The counters of the link, shared with the other sessions of the
    /// caller (podnet adds each session of a process, issue 134). None: the
    /// counters of this session only.
    pub link_stats: Option<Arc<DeviceStats>>,
    /// The check that a packet crosses a new session
    /// ([`crate::Warp::connect`], PodUtils/podnet issue 120). None: no check
    /// (podnet's `PODNET_WARP_NO_CHECK`).
    pub check: Option<DataPathCheck>,
    /// The budget of the relay that carries the session, if any
    /// ([`crate::WarpNet::renew_due`]).
    pub renew: Option<Renew>,
}

impl Default for NetSettings {
    /// [`MTU`], [`TCP_BUFFER`], no congestion controller, no burst limit,
    /// [`TCP_TIMEOUT`], [`TCP_KEEPALIVE`], [`TCP_LINGER`], no Nagle,
    /// counters of the session, the check of [`DataPathCheck::default`], and
    /// no relay.
    fn default() -> NetSettings {
        NetSettings {
            mtu: MTU,
            tcp_buffer: TCP_BUFFER,
            congestion_control: CongestionControl::None,
            tcp_burst: None,
            tcp_timeout: Some(TCP_TIMEOUT),
            tcp_keepalive: Some(TCP_KEEPALIVE),
            tcp_linger: TCP_LINGER,
            nodelay: true,
            link_stats: None,
            check: Some(DataPathCheck::default()),
            renew: None,
        }
    }
}

/// The address of a family that the device lacks: the documentation
/// addresses, which the stack owns and never reaches.
pub(crate) const NO_V4: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 2);
pub(crate) const NO_V6: Ipv6Addr = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 2);

impl NetSettings {
    /// The settings of the stack of `pod-netstack` for a device with the
    /// addresses `v4` and `v6` (one at least). A TCP burst out of its range
    /// is a `Config` failure (exit 78).
    pub fn stack_config(&self, v4: Option<Ipv4Addr>, v6: Option<Ipv6Addr>) -> Result<StackConfig, Failure> {
        if v4.is_none() && v6.is_none() {
            return Err(Failure::new(Stage::Warp, Class::Protocol, "the tunnel assigned no address"));
        }
        let max = tcp_burst_max(self.tcp_buffer, self.mtu);
        if let Some(burst) = self.tcp_burst
            && !(1..=max).contains(&burst)
        {
            let text = format!("a TCP burst of {burst} segments: give a number from 1 to {max}, or none");
            return Err(Failure::new(Stage::Warp, Class::Config, text));
        }
        let local_v4 = v4.unwrap_or(NO_V4);
        let local_v6 = v6.unwrap_or(NO_V6);
        Ok(StackConfig {
            subnet: Subnet { v4: local_v4, v4_prefix: 0, v6: local_v6, v6_prefix: 0 },
            local_v4,
            local_v6,
            mtu: self.mtu,
            tcp_buffer: self.tcp_buffer,
            congestion_control: self.congestion_control,
            max_burst: self.tcp_burst,
            tcp_timeout: self.tcp_timeout,
            tcp_keepalive: self.tcp_keepalive,
            linger: self.tcp_linger,
            nodelay: self.nodelay,
            link_stats: self.link_stats.clone(),
            ..StackConfig::default()
        })
    }

    /// The addresses of the device for `v4` and `v6`.
    pub(crate) fn ours(v4: Option<Ipv4Addr>, v6: Option<Ipv6Addr>) -> Vec<IpAddr> {
        v4.map(IpAddr::V4).into_iter().chain(v6.map(IpAddr::V6)).collect()
    }
}

fn config(text: String) -> Failure {
    Failure::new(Stage::Warp, Class::Config, text)
}

/// `none` (also an empty value), `reno` (RFC 5681) or `cubic` (RFC 9438),
/// in any case; another value is a `Config` failure (exit 78).
pub fn parse_cc(value: &str) -> Result<CongestionControl, Failure> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "none" => Ok(CongestionControl::None),
        "reno" => Ok(CongestionControl::Reno),
        "cubic" => Ok(CongestionControl::Cubic),
        other => Err(config(format!("the congestion controller {other:?}: give none, reno or cubic"))),
    }
}

/// The TCP burst of a value: a number of segments from 1 to
/// [`TCP_BURST_MAX`], or `none` for no limit; no value is no limit.
/// Another value is a `Config` failure (exit 78), not the default
/// (PodUtils/podnet issue 111: a measurement must be trusted).
pub fn parse_tcp_burst(value: Option<&str>) -> Result<Option<usize>, Failure> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value == "none" {
        return Ok(None);
    }
    match value.parse::<usize>() {
        Ok(n) if (1..=TCP_BURST_MAX).contains(&n) => Ok(Some(n)),
        _ => Err(config(format!(
            "the TCP burst {value:?}: give a number of segments from 1 to {TCP_BURST_MAX}, or none"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // PodUtils/podnet issue 111: a bad value is an error, not the default.
    #[test]
    fn the_congestion_controller() {
        assert_eq!(parse_cc("none").unwrap(), CongestionControl::None);
        assert_eq!(parse_cc(" Reno ").unwrap(), CongestionControl::Reno);
        assert_eq!(parse_cc("cubic").unwrap(), CongestionControl::Cubic);
        let err = parse_cc("bbr").unwrap_err();
        assert_eq!(err.class.exit_code(), 78);
        // No setting: none, as before smoltcp 0.12.
        let settings = NetSettings::default();
        assert_eq!(settings.congestion_control, CongestionControl::None);
        let reno = NetSettings { congestion_control: CongestionControl::Reno, ..NetSettings::default() };
        let stack = reno.stack_config(Some(Ipv4Addr::new(172, 16, 0, 2)), None).unwrap();
        assert_eq!(stack.congestion_control, CongestionControl::Reno);
    }

    #[test]
    fn the_tcp_burst_variable_is_checked() {
        assert_eq!(parse_tcp_burst(None).unwrap(), None);
        assert_eq!(parse_tcp_burst(Some("none")).unwrap(), None);
        assert_eq!(parse_tcp_burst(Some(" 8 ")).unwrap(), Some(8));
        assert_eq!(parse_tcp_burst(Some("430")).unwrap(), Some(430));
        for bad in ["", "0", "abc", "-1", "431", "999999999", "8 segments"] {
            let err = parse_tcp_burst(Some(bad)).unwrap_err();
            assert_eq!(err.class.exit_code(), 78, "{bad:?}");
            assert!(err.text().contains("TCP burst"), "{}", err.text());
        }
    }

    /// The burst of the settings is the burst limit of the stack, in its
    /// range for the buffer and the MTU; out of it is a `Config` failure.
    #[test]
    fn the_stack_takes_the_burst_the_buffer_and_the_counters() {
        let ip = Ipv4Addr::new(172, 16, 0, 2);
        let v4 = Some(ip);
        let shared = Arc::new(DeviceStats::default());
        let settings = NetSettings { tcp_burst: Some(4), link_stats: Some(shared.clone()), ..NetSettings::default() };
        let stack = settings.stack_config(v4, None).unwrap();
        assert_eq!(stack.max_burst, Some(4));
        assert_eq!(stack.tcp_buffer, TCP_BUFFER);
        assert!(stack.link_stats.is_some_and(|s| Arc::ptr_eq(&s, &shared)));
        assert_eq!((stack.local_v4, stack.local_v6, stack.mtu), (ip, NO_V6, MTU));
        for burst in [0, TCP_BURST_MAX + 1] {
            let bad = NetSettings { tcp_burst: Some(burst), ..NetSettings::default() };
            assert_eq!(bad.stack_config(v4, None).err().unwrap().class, Class::Config, "{burst}");
        }
        let small = NetSettings { tcp_buffer: 64 * 1024, tcp_burst: Some(60), ..NetSettings::default() };
        assert!(small.stack_config(v4, None).is_err(), "60 segments do not fit in 64 KiB");
        assert!(NetSettings::default().stack_config(None, None).is_err(), "no address");
    }

    /// PodUtils/podnet issue 125 and podssh: the stack of a session has
    /// podnet's TCP timeout, keepalive and linger, and no Nagle.
    #[test]
    fn the_stack_has_the_tcp_timeout_the_keepalive_and_no_nagle_of_podnet() {
        let stack = NetSettings::default().stack_config(Some(Ipv4Addr::new(172, 16, 0, 2)), None).unwrap();
        assert_eq!(stack.tcp_timeout, Some(Duration::from_secs(60)));
        assert_eq!(stack.tcp_keepalive, Some(Duration::from_secs(20)));
        assert_eq!(stack.linger, Duration::from_secs(30));
        assert!(stack.nodelay);
        assert!(TCP_KEEPALIVE < TCP_TIMEOUT, "an idle connection to a live peer must stay");
    }
}
