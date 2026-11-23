//! What an Allocate asks for beyond a UDP relay (RFC 8656, 7.1): the
//! address family, an even port and the reservation of the next one, a
//! reserved port, and DONT-FRAGMENT; and what its success gives back (7.3):
//! one or two relayed addresses, a token, and the family that the server
//! could not give.

use std::fmt;
use std::net::SocketAddr;

use crate::stun::{
    self, ADDITIONAL_ADDRESS_FAMILY, ADDRESS_ERROR_CODE, DONT_FRAGMENT, EVEN_PORT, REQUESTED_ADDRESS_FAMILY,
    RESERVATION_TOKEN, XOR_RELAYED_ADDRESS,
};

use super::Error;
use super::exchange::Value;

/// The families of an address attribute (RFC 8489, 14.1).
const IPV4: u8 = 0x01;
const IPV6: u8 = 0x02;

/// The address family of the relayed address.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Family {
    /// No attribute: the server's default, IPv4 (RFC 8656, 7.2).
    #[default]
    Server,
    /// REQUESTED-ADDRESS-FAMILY IPv4.
    Ipv4,
    /// REQUESTED-ADDRESS-FAMILY IPv6.
    Ipv6,
    /// ADDITIONAL-ADDRESS-FAMILY IPv6: an IPv4 and an IPv6 relayed address,
    /// or one of them and the [`AddressError`] of the other.
    Both,
}

impl Family {
    fn of(addr: &SocketAddr) -> Family {
        if addr.is_ipv4() { Family::Ipv4 } else { Family::Ipv6 }
    }
}

/// The port of the relayed address.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Port {
    /// No attribute: any port.
    #[default]
    Any,
    /// EVEN-PORT with the R bit 0: an even port.
    Even,
    /// EVEN-PORT with the R bit 1: an even port, and the server keeps the
    /// next port for a later Allocate with the token that it gives
    /// ([`crate::Client::reservation_token`]).
    EvenAndNext,
}

/// The transport between the server and the peer that the allocation
/// relays.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Relay {
    /// A UDP relay (RFC 8656).
    #[default]
    Udp,
    /// A TCP relay (RFC 6062): [`crate::Client::connect`] and
    /// [`crate::Client::accept`] carry TCP bytes through it.
    Tcp,
}

/// What an Allocate asks for beyond the relay itself. The default asks for
/// nothing more: a UDP relay, a relayed address of the server's family, any
/// port, no reservation, no DONT-FRAGMENT.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Allocation {
    /// The transport between the server and the peer.
    pub relay: Relay,
    pub family: Family,
    pub port: Port,
    /// RESERVATION-TOKEN: the port that the token of an earlier allocation
    /// keeps. It goes with no family and no even port (RFC 8656, 7.1).
    pub reservation: Option<[u8; 8]>,
    /// DONT-FRAGMENT in the Allocate and in each Send indication: the server
    /// sets the DF bit toward the peer. A server that cannot answers 420.
    pub dont_fragment: bool,
}

/// The token is a claim on a port of the server: `Debug` says only that
/// one is set.
impl fmt::Debug for Allocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Allocation")
            .field("relay", &self.relay)
            .field("family", &self.family)
            .field("port", &self.port)
            .field("reservation", &self.reservation.map(|_| "(set)"))
            .field("dont_fragment", &self.dont_fragment)
            .finish()
    }
}

impl Allocation {
    /// Refuse what RFC 8656, 7.1 forbids a client to send.
    pub(super) fn check(&self) -> Result<(), Error> {
        let usage = |why: &str| Err(Error::Usage(why.to_string()));
        if self.reservation.is_some() && self.port != Port::Any {
            return usage("a TURN reservation token goes with no even port");
        }
        if self.reservation.is_some() && self.family != Family::Server {
            return usage("a TURN reservation token goes with no address family");
        }
        if self.family == Family::Both && self.port == Port::EvenAndNext {
            return usage("a TURN allocation of both families cannot keep the next port");
        }
        if self.relay == Relay::Tcp {
            if self.port != Port::Any {
                return usage("a TCP relay has no even port");
            }
            if self.reservation.is_some() {
                return usage("a TCP relay has no reservation token");
            }
            if self.dont_fragment {
                return usage("a TCP relay has no DONT-FRAGMENT");
            }
        }
        Ok(())
    }

    /// The attributes of the Allocate.
    pub(super) fn attributes(&self) -> Vec<(u16, Value)> {
        let mut out = Vec::new();
        match self.family {
            Family::Server => {}
            Family::Ipv4 => out.push((REQUESTED_ADDRESS_FAMILY, Value::Bytes(vec![IPV4, 0, 0, 0]))),
            Family::Ipv6 => out.push((REQUESTED_ADDRESS_FAMILY, Value::Bytes(vec![IPV6, 0, 0, 0]))),
            Family::Both => out.push((ADDITIONAL_ADDRESS_FAMILY, Value::Bytes(vec![IPV6, 0, 0, 0]))),
        }
        match self.port {
            Port::Any => {}
            Port::Even => out.push((EVEN_PORT, Value::Bytes(vec![0]))),
            Port::EvenAndNext => out.push((EVEN_PORT, Value::Bytes(vec![0x80]))),
        }
        if let Some(token) = self.reservation {
            out.push((RESERVATION_TOKEN, Value::Bytes(token.to_vec())));
        }
        if self.dont_fragment {
            out.push((DONT_FRAGMENT, Value::Bytes(Vec::new())));
        }
        out
    }
}

/// The family that the server did not give in an allocation of both
/// families, and why (ADDRESS-ERROR-CODE, RFC 8656, 18.12): 440 (Address
/// Family not Supported) or 508 (Insufficient Capacity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressError {
    /// [`Family::Ipv4`] or [`Family::Ipv6`].
    pub family: Family,
    pub code: u16,
    pub reason: String,
}

/// What an Allocate success gave.
pub(super) struct Granted {
    pub relayed: Vec<SocketAddr>,
    pub reservation: Option<[u8; 8]>,
    pub address_error: Option<AddressError>,
}

/// Read the success of an Allocate that asked for `asked`: each relayed
/// address must be of a family that was asked for, and of an even port
/// when one was asked for (RFC 8656, 7.3). An unspecified address is no
/// relayed address: coturn writes `0.0.0.0:0` in place of the family that
/// its ADDRESS-ERROR-CODE names.
pub(super) fn granted(read: &stun::Message<'_>, asked: &Allocation) -> Result<Granted, Error> {
    let kind = read.kind;
    let protocol = |why: String| Error::Protocol { kind, why };
    let relayed = read
        .attributes
        .iter()
        .filter(|(k, _)| *k == XOR_RELAYED_ADDRESS)
        .map(|(_, v)| {
            stun::read_address(v, Some(&read.id)).map_err(|why| protocol(format!("XOR-RELAYED-ADDRESS: {why}")))
        })
        .filter(|got| got.as_ref().map_or(true, |addr| !addr.ip().is_unspecified()))
        .collect::<Result<Vec<_>, _>>()?;
    let address_error = read.attribute(ADDRESS_ERROR_CODE).map(address_error).transpose().map_err(protocol)?;
    let families: Vec<Family> = relayed.iter().map(Family::of).collect();
    let fits = match (asked.family, families.as_slice()) {
        (Family::Server, [_]) => true,
        (Family::Ipv4 | Family::Ipv6, [one]) => *one == asked.family,
        (Family::Both, [a, b]) => a != b,
        // coturn 4.6 gives one family and no ADDRESS-ERROR-CODE.
        (Family::Both, [one]) => address_error.as_ref().is_none_or(|e| e.family != *one),
        _ => false,
    };
    if !fits {
        let why = format!("relayed addresses of the families {families:?} for an Allocate of {:?}", asked.family);
        return Err(protocol(why));
    }
    if asked.port != Port::Any
        && let Some(odd) = relayed.iter().find(|a| a.port() % 2 == 1)
    {
        return Err(protocol(format!("the relayed address {odd} for an Allocate of an even port")));
    }
    let reservation = match read.attribute(RESERVATION_TOKEN) {
        None => None,
        Some(value) => Some(
            <[u8; 8]>::try_from(value)
                .map_err(|_| protocol(format!("a RESERVATION-TOKEN of {} bytes", value.len())))?,
        ),
    };
    Ok(Granted { relayed, reservation, address_error })
}

/// The value of ADDRESS-ERROR-CODE: the family, the class and the number,
/// and the reason.
fn address_error(value: &[u8]) -> Result<AddressError, String> {
    if value.len() < 4 {
        return Err(format!("an ADDRESS-ERROR-CODE of {} bytes", value.len()));
    }
    let family = match value[0] {
        IPV4 => Family::Ipv4,
        IPV6 => Family::Ipv6,
        other => return Err(format!("an ADDRESS-ERROR-CODE of the family {other}")),
    };
    let (class, number) = (u16::from(value[2] & 0x07), u16::from(value[3]));
    if !(3..=6).contains(&class) || number > 99 {
        return Err(format!("an ADDRESS-ERROR-CODE of class {class} and number {number}"));
    }
    Ok(AddressError { family, code: class * 100 + number, reason: String::from_utf8_lossy(&value[4..]).into_owned() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stun::{ALLOCATE, Class, message, message_type, write_address};

    const ID: [u8; 12] = [9; 12];

    fn success(attributes: &[(u16, &[u8])]) -> Vec<u8> {
        message(message_type(ALLOCATE, Class::Success), &ID, attributes)
    }

    fn read(bytes: &[u8]) -> stun::Message<'_> {
        stun::decode(bytes).unwrap()
    }

    #[test]
    fn the_attributes_of_each_ask() {
        let ask = Allocation {
            relay: Relay::Udp,
            family: Family::Both,
            port: Port::Even,
            reservation: None,
            dont_fragment: true,
        };
        let kinds: Vec<u16> = ask.attributes().iter().map(|(k, _)| *k).collect();
        assert_eq!(kinds, [ADDITIONAL_ADDRESS_FAMILY, EVEN_PORT, DONT_FRAGMENT]);
        let reserved = Allocation { reservation: Some([7; 8]), ..Allocation::default() };
        assert!(matches!(reserved.attributes()[..], [(RESERVATION_TOKEN, Value::Bytes(ref t))] if t == &[7; 8]));
        assert!(Allocation::default().attributes().is_empty());
        assert!(format!("{reserved:?}").contains("(set)") && !format!("{reserved:?}").contains('7'));
    }

    #[test]
    fn an_ask_that_rfc_8656_forbids_is_refused() {
        let token = Some([1; 8]);
        for ask in [
            Allocation { reservation: token, port: Port::Even, ..Allocation::default() },
            Allocation { reservation: token, family: Family::Ipv4, ..Allocation::default() },
            Allocation { family: Family::Both, port: Port::EvenAndNext, ..Allocation::default() },
        ] {
            assert!(matches!(ask.check(), Err(Error::Usage(_))), "{ask:?}");
        }
        let both = Allocation { family: Family::Both, port: Port::Even, ..Allocation::default() };
        assert!(both.check().is_ok());
        for ask in [
            Allocation { relay: Relay::Tcp, port: Port::Even, ..Allocation::default() },
            Allocation { relay: Relay::Tcp, reservation: token, ..Allocation::default() },
            Allocation { relay: Relay::Tcp, dont_fragment: true, ..Allocation::default() },
        ] {
            assert!(matches!(ask.check(), Err(Error::Usage(_))), "{ask:?}");
        }
        assert!(Allocation { relay: Relay::Tcp, ..Allocation::default() }.check().is_ok());
    }

    #[test]
    fn a_success_gives_its_addresses_its_token_and_the_family_it_did_not_give() {
        let (v4, v6): (SocketAddr, SocketAddr) =
            ("192.0.2.15:50000".parse().unwrap(), "[2001:db8::1]:50002".parse().unwrap());
        let (a, b) = (write_address(v4, Some(&ID)), write_address(v6, Some(&ID)));
        let token = [5u8; 8];
        let both = Allocation { family: Family::Both, ..Allocation::default() };
        let bytes = success(&[(XOR_RELAYED_ADDRESS, &a), (XOR_RELAYED_ADDRESS, &b), (RESERVATION_TOKEN, &token)]);
        let got = granted(&read(&bytes), &both).unwrap();
        assert_eq!((got.relayed, got.reservation, got.address_error), (vec![v4, v6], Some(token), None));

        let error = [[IPV6, 0, 4, 40].as_slice(), b"no IPv6"].concat();
        let bytes = success(&[(XOR_RELAYED_ADDRESS, &a), (ADDRESS_ERROR_CODE, &error)]);
        let got = granted(&read(&bytes), &both).unwrap();
        let wanted = AddressError { family: Family::Ipv6, code: 440, reason: "no IPv6".into() };
        assert_eq!((got.relayed, got.address_error), (vec![v4], Some(wanted.clone())));

        // coturn: 0.0.0.0:0 in place of the family that it could not give.
        let any = write_address("0.0.0.0:0".parse().unwrap(), Some(&ID));
        let bytes = success(&[(XOR_RELAYED_ADDRESS, &a), (XOR_RELAYED_ADDRESS, &any), (ADDRESS_ERROR_CODE, &error)]);
        let got = granted(&read(&bytes), &both).unwrap();
        assert_eq!((got.relayed, got.address_error), (vec![v4], Some(wanted)));
    }

    #[test]
    fn a_success_of_another_family_or_an_odd_port_is_refused() {
        let v4: SocketAddr = "192.0.2.15:50001".parse().unwrap();
        let a = write_address(v4, Some(&ID));
        let bytes = success(&[(XOR_RELAYED_ADDRESS, &a)]);
        let refused = |ask: Allocation| match granted(&read(&bytes), &ask) {
            Err(Error::Protocol { why, .. }) => why,
            other => panic!("{:?}", other.map(|g| g.relayed)),
        };
        assert!(refused(Allocation { family: Family::Ipv6, ..Allocation::default() }).contains("families"));
        let ipv4_error = [[IPV4, 0, 4, 40].as_slice(), b"no IPv4"].concat();
        let wrong = success(&[(XOR_RELAYED_ADDRESS, &a), (ADDRESS_ERROR_CODE, &ipv4_error)]);
        let both = Allocation { family: Family::Both, ..Allocation::default() };
        assert!(matches!(granted(&read(&wrong), &both), Err(Error::Protocol { .. })), "IPv4 given and refused");
        assert!(granted(&read(&bytes), &both).is_ok(), "one family and no ADDRESS-ERROR-CODE, as coturn 4.6");
        assert!(refused(Allocation { port: Port::Even, ..Allocation::default() }).contains("even port"));
        assert!(granted(&read(&bytes), &Allocation::default()).is_ok());
        assert!(granted(&read(&success(&[])), &Allocation::default()).is_err(), "no relayed address");
        let short = success(&[(XOR_RELAYED_ADDRESS, &a), (RESERVATION_TOKEN, &[1, 2, 3, 4])]);
        assert!(granted(&read(&short), &Allocation::default()).is_err(), "a token of 4 bytes");
    }
}
