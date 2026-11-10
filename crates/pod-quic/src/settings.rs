//! The settings of a QUIC connection. The default is cloudflared's
//! (`quic/constants.go`, `connection/quic_config.go`): 5 s for the handshake
//! and for a silence, a keepalive each second, first packets of 1232 bytes
//! over IPv4 and 1252 over IPv6, 256 streams, and datagrams on. A consumer
//! changes a field for its own peer (podnet's edge: a silence of 30 s; its
//! tunnel under WARP: a ceiling of the MTU).

use std::time::Duration;

use noq::{IdleTimeout, MtuDiscoveryConfig, TransportConfig, VarInt};

/// The settings of the transport and of the handshake.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// The wait for the handshake (cloudflared's `HandshakeIdleTimeout`).
    pub handshake_wait: Duration,
    /// A silence that ends the connection (`MaxIdleTimeout`). A value that
    /// QUIC cannot carry (more than 2^62 ms) sets no limit.
    pub silence: Duration,
    /// The interval of the keepalive (`MaxIdlePingPeriod`); `None` sends
    /// none.
    pub keepalive: Option<Duration>,
    /// The first packets toward an IPv4 address, for paths whose MTU is
    /// 1280.
    pub first_packet_v4: u16,
    /// The first packets toward an IPv6 address.
    pub first_packet_v6: u16,
    /// The bidirectional streams that the peer may open at once: as
    /// HTTP/2's, as a quick tunnel takes 200 requests in flight.
    pub max_streams: u32,
    /// The bytes of datagrams that may wait to be read. `None` keeps the
    /// default of noq.
    pub datagram_buffer: Option<usize>,
    /// The ceiling of the discovery of the MTU: the payload of the largest
    /// UDP datagram that the path carries (podnet: its tunnel of 1280
    /// bytes, less an IPv4 and a UDP header). `None` keeps the default of
    /// noq.
    pub mtu_ceiling: Option<u16>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            handshake_wait: HANDSHAKE_WAIT,
            silence: SILENCE,
            keepalive: Some(KEEPALIVE),
            first_packet_v4: FIRST_PACKET_V4,
            first_packet_v6: FIRST_PACKET_V6,
            max_streams: MAX_STREAMS,
            datagram_buffer: Some(DATAGRAMS),
            mtu_ceiling: None,
        }
    }
}

/// cloudflared's `HandshakeIdleTimeout`, `MaxIdleTimeout` and
/// `MaxIdlePingPeriod`.
pub const HANDSHAKE_WAIT: Duration = Duration::from_secs(5);
pub const SILENCE: Duration = Duration::from_secs(5);
pub const KEEPALIVE: Duration = Duration::from_secs(1);
/// cloudflared's first packets, for paths whose MTU is 1280.
pub const FIRST_PACKET_V4: u16 = 1232;
pub const FIRST_PACKET_V6: u16 = 1252;
/// The streams that the peer may open at once.
pub const MAX_STREAMS: u32 = 256;
/// The datagrams that may wait: podssh routes no private network and reads
/// none, but takes them as cloudflared does.
pub const DATAGRAMS: usize = 64 * 1024;

impl Settings {
    /// The first packets toward an address of the family `v6`.
    pub fn first_packet(&self, v6: bool) -> u16 {
        if v6 { self.first_packet_v6 } else { self.first_packet_v4 }
    }

    /// The configuration of the transport toward an address of the family
    /// `v6`.
    pub fn transport(&self, v6: bool) -> TransportConfig {
        let mut transport = TransportConfig::default();
        transport
            .max_idle_timeout(IdleTimeout::try_from(self.silence).ok())
            .keep_alive_interval(self.keepalive)
            .initial_mtu(self.first_packet(v6))
            .max_concurrent_bidi_streams(VarInt::from_u32(self.max_streams));
        if let Some(buffer) = self.datagram_buffer {
            transport.datagram_receive_buffer_size(Some(buffer));
        }
        if let Some(ceiling) = self.mtu_ceiling {
            let mut discovery = MtuDiscoveryConfig::default();
            discovery.upper_bound(ceiling);
            transport.mtu_discovery_config(Some(discovery));
        }
        transport
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_cloudflared_s() {
        let s = Settings::default();
        assert_eq!((s.handshake_wait, s.silence, s.keepalive), (HANDSHAKE_WAIT, SILENCE, Some(KEEPALIVE)));
        assert_eq!((s.first_packet(false), s.first_packet(true)), (1232, 1252));
        assert_eq!((s.max_streams, s.datagram_buffer, s.mtu_ceiling), (256, Some(64 * 1024), None));
    }

    /// A ceiling below the first packets is a configuration that noq
    /// takes: the discovery does not go past it.
    #[test]
    fn a_transport_with_a_ceiling_and_a_long_silence_builds() {
        let s = Settings { silence: Duration::from_secs(30), mtu_ceiling: Some(1252), ..Settings::default() };
        let _ = s.transport(false);
        let s = Settings { silence: Duration::MAX, keepalive: None, datagram_buffer: None, ..Settings::default() };
        let _ = s.transport(true);
    }
}
