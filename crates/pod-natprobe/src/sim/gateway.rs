//! Simulated LAN gateway responding to NAT-PMP, PCP and UPnP IGD SSDP/HTTP/SOAP.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use crate::wire::pcp::{HEADER_LEN, OP_ANNOUNCE, PCP_VERSION};
use crate::wire::pmp::{OP_EXTERNAL_ADDR, PMP_VERSION, RESPONSE_BIT};

/// Simulated NAT-PMP responder mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimPmpMode {
    Silent,
    ExternalIp(Ipv4Addr),
    Disabled,
}

/// Simulated PCP responder mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimPcpMode {
    Silent,
    Ok,
    NotAuthorized,
    AddressMismatch,
    /// NAT-PMP-only gateway: replies to PCP with an 8-byte version-0 UnsupportedVersion packet.
    NatPmpOnly8Byte,
}

/// Simulated UPnP IGD mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimUpnpMode {
    pub enabled: bool,
    pub reply_from_ephemeral_port: bool,
    pub location: String,
    pub external_ip: Option<Ipv4Addr>,
}

impl Default for SimUpnpMode {
    fn default() -> Self {
        Self {
            enabled: false,
            reply_from_ephemeral_port: false,
            location: "http://192.168.1.1:5000/rootDesc.xml".to_string(),
            external_ip: None,
        }
    }
}

/// Simulated LAN gateway attached to a [`super::SimWorld`].
#[derive(Clone, Debug)]
pub struct SimGateway {
    pub ip: IpAddr,
    pub pmp_port: u16,
    pub ssdp_port: u16,
    pub pmp: SimPmpMode,
    pub pcp: SimPcpMode,
    pub upnp: SimUpnpMode,
    /// Optional spoofed sender IP (tests that non-gateway 5351 replies are dropped).
    pub spoof_src_ip: Option<IpAddr>,
}

impl SimGateway {
    pub fn silent(ip: IpAddr) -> Self {
        Self {
            ip,
            pmp_port: 5351,
            ssdp_port: 1900,
            pmp: SimPmpMode::Silent,
            pcp: SimPcpMode::Silent,
            upnp: SimUpnpMode::default(),
            spoof_src_ip: None,
        }
    }

    /// Handle a UDP datagram sent to this gateway or SSDP multicast.
    pub fn handle_udp(&self, dst: SocketAddr, bytes: &[u8]) -> Vec<(SocketAddr, Vec<u8>)> {
        let mut out = Vec::new();
        let src_ip = self.spoof_src_ip.unwrap_or(self.ip);
        if dst.ip() == self.ip && dst.port() == self.pmp_port {
            if bytes.len() == 2 && bytes[0] == PMP_VERSION && bytes[1] == OP_EXTERNAL_ADDR {
                match self.pmp {
                    SimPmpMode::Silent => {}
                    SimPmpMode::ExternalIp(wan) => {
                        let mut r = [0u8; 12];
                        r[0] = PMP_VERSION;
                        r[1] = RESPONSE_BIT | OP_EXTERNAL_ADDR;
                        r[4..8].copy_from_slice(&100u32.to_be_bytes());
                        r[8..12].copy_from_slice(&wan.octets());
                        out.push((SocketAddr::new(src_ip, self.pmp_port), r.to_vec()));
                    }
                    SimPmpMode::Disabled => {
                        let mut r = [0u8; 12];
                        r[0] = PMP_VERSION;
                        r[1] = RESPONSE_BIT | OP_EXTERNAL_ADDR;
                        r[2..4].copy_from_slice(&2u16.to_be_bytes());
                        out.push((SocketAddr::new(src_ip, self.pmp_port), r.to_vec()));
                    }
                }
            } else if bytes.len() >= HEADER_LEN && bytes[0] == PCP_VERSION && bytes[1] == OP_ANNOUNCE {
                match self.pcp {
                    SimPcpMode::Silent => {}
                    SimPcpMode::NatPmpOnly8Byte => {
                        let mut r = [0u8; 8];
                        r[0] = PMP_VERSION;
                        r[1] = RESPONSE_BIT;
                        r[2..4].copy_from_slice(&1u16.to_be_bytes());
                        r[4..8].copy_from_slice(&100u32.to_be_bytes());
                        out.push((SocketAddr::new(src_ip, self.pmp_port), r.to_vec()));
                    }
                    SimPcpMode::Ok | SimPcpMode::NotAuthorized | SimPcpMode::AddressMismatch => {
                        let code = match self.pcp {
                            SimPcpMode::Ok => 0u8,
                            SimPcpMode::NotAuthorized => 2u8,
                            SimPcpMode::AddressMismatch => 12u8,
                            _ => 0,
                        };
                        let mut r = [0u8; HEADER_LEN];
                        r[0] = PCP_VERSION;
                        r[1] = 0x80 | OP_ANNOUNCE;
                        r[3] = code;
                        r[8..12].copy_from_slice(&100u32.to_be_bytes());
                        out.push((SocketAddr::new(src_ip, self.pmp_port), r.to_vec()));
                    }
                }
            }
            return out;
        }
        if self.upnp.enabled && bytes.starts_with(b"M-SEARCH ") {
            let reply_port = if self.upnp.reply_from_ephemeral_port { 49152 } else { self.ssdp_port };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nLOCATION: {}\r\nST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\r\n",
                self.upnp.location
            );
            out.push((SocketAddr::new(self.ip, reply_port), resp.into_bytes()));
        }
        out
    }

    /// Handle a simulated UPnP HTTP GET (device XML) or SOAP POST (`GetExternalIPAddress`).
    pub fn handle_http_get(&self, url: &str) -> Result<String, String> {
        if self.upnp.enabled && url == self.upnp.location {
            Ok("<root><device><serviceList><service>\
                <serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>\
                <controlURL>/ctl/IPConn</controlURL>\
                </service></serviceList></device></root>"
                .to_string())
        } else {
            Err("404".to_string())
        }
    }

    pub fn handle_http_soap(&self, _url: &str, _action: &str) -> Result<String, String> {
        if let Some(ip) = self.upnp.external_ip {
            Ok(format!(
                "<s:Envelope><s:Body><u:GetExternalIPAddressResponse>\
                <NewExternalIPAddress>{ip}</NewExternalIPAddress>\
                </u:GetExternalIPAddressResponse></s:Body></s:Envelope>"
            ))
        } else {
            Err("500".to_string())
        }
    }
}
