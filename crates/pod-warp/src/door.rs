//! The front door of a MASQUE session: the order of the doors of the device
//! record, or one door that the caller pins.

use std::net::SocketAddr;

use pod_core::Target;

/// Which front door a session dials.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Door {
    /// The doors of the device record, in the order of [`endpoint_order`].
    #[default]
    Record,
    /// One door, `HOST:PORT` or `ADDRESS:PORT`, and no other
    /// (PodUtils/podnet#413, ask 10; podnet's `--masque-endpoint`): a host
    /// with IPv6 alone pins the IPv6 door and pays no failed IPv4 connect.
    Pinned(Target),
}

/// The default routes of the host, as the caller knows them. A dial
/// through a proxy or a relay reaches both families.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Routes {
    pub v4: bool,
    pub v6: bool,
}

impl Default for Routes {
    /// Both families.
    fn default() -> Routes {
        Routes { v4: true, v6: true }
    }
}

/// The order of the front door addresses: the one that worked last first,
/// then the others; IPv6 only with an IPv6 route. With no door that worked
/// and no IPv4 route, the IPv6 doors go first: an IPv4 door can only fail
/// with `Network unreachable` (PodUtils/podnet@T-169). A door that carried
/// no packet (`bad`) goes last.
pub fn endpoint_order(
    all: Vec<SocketAddr>,
    won: Option<SocketAddr>,
    bad: Option<SocketAddr>,
    routes: Routes,
) -> Vec<SocketAddr> {
    let mut out: Vec<SocketAddr> = all.into_iter().filter(|a| a.is_ipv4() || routes.v6).collect();
    let won = won.filter(|w| Some(*w) != bad);
    if let Some(won) = won {
        if let Some(i) = out.iter().position(|a| *a == won) {
            out.swap(0, i);
        }
    } else if !routes.v4 {
        out.sort_by_key(|a| a.is_ipv4());
    }
    // PodUtils/podnet issue 133: a door that carried no packet goes last.
    if let Some(bad) = bad
        && let Some(i) = out.iter().position(|a| *a == bad)
    {
        let door = out.remove(i);
        out.push(door);
    }
    out
}

/// The doors that a session tries, in order: the pinned door alone, or the
/// doors of the record in [`endpoint_order`] (the record's first door when
/// the order leaves none).
pub fn doors(
    door: &Door,
    record: Vec<SocketAddr>,
    won: Option<SocketAddr>,
    bad: Option<SocketAddr>,
    routes: Routes,
) -> Vec<Target> {
    match door {
        Door::Pinned(target) => vec![target.clone()],
        Door::Record => {
            let first = record.first().copied();
            let order = endpoint_order(record, won, bad, routes);
            let order = if order.is_empty() { first.into_iter().collect() } else { order };
            order.into_iter().map(Target::Addr).collect()
        }
    }
}

/// A door in a door file is used for this time.
pub const DOOR_TTL: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 3600);

/// The door of the text of a door file (podnet's `warp-door.json`), when it
/// is recent (less than [`DOOR_TTL`] before `now`, Unix seconds) and one of
/// `candidates` (PodUtils/podnet issue 133). The file is the caller's.
pub fn door_from(text: &str, now: u64, candidates: &[SocketAddr]) -> Option<SocketAddr> {
    let doc: serde_json::Value = serde_json::from_str(text).ok()?;
    if doc["schema"] != 1 {
        return None;
    }
    let at = doc["at"].as_u64()?;
    if at > now || now - at >= DOOR_TTL.as_secs() {
        return None;
    }
    let door: SocketAddr = doc["door"].as_str()?.parse().ok()?;
    candidates.contains(&door).then_some(door)
}

/// The text of a door file of `door` at `now` (Unix seconds).
pub fn door_text(door: SocketAddr, now: u64) -> String {
    serde_json::json!({"schema": 1, "door": door.to_string(), "at": now}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // PodUtils/podnet issue 133: the door file is used when it is recent and
    // names a door of the device.
    #[test]
    fn the_door_file() {
        let doors = [v4(), v6()];
        let doc = |door: &str, at: u64, schema: u64| {
            serde_json::json!({"schema": schema, "door": door, "at": at}).to_string()
        };
        let now = 1_800_000_000;
        assert_eq!(door_from(&doc("[2606:4700:103::2]:443", now - 60, 1), now, &doors), Some(v6()));
        assert_eq!(door_from(&doc("162.159.198.2:443", now - 8 * 86400, 1), now, &doors), None);
        assert_eq!(door_from(&doc("192.0.2.1:443", now - 60, 1), now, &doors), None);
        assert_eq!(door_from(&doc("162.159.198.2:443", now - 60, 2), now, &doors), None);
        assert_eq!(door_from("{", now, &doors), None);
        assert_eq!(door_from(&door_text(v4(), now - 5), now, &doors), Some(v4()));
    }

    const ALL: Routes = Routes { v4: true, v6: true };
    const NO_V6: Routes = Routes { v4: true, v6: false };
    const NO_V4: Routes = Routes { v4: false, v6: true };

    fn v4() -> SocketAddr {
        "162.159.198.2:443".parse().unwrap()
    }

    fn v6() -> SocketAddr {
        "[2606:4700:103::2]:443".parse().unwrap()
    }

    // PodUtils/podnet issue 124: the address that worked goes first; IPv6
    // only with a route.
    #[test]
    fn the_order_of_the_front_door_addresses() {
        let (v4, v6) = (v4(), v6());
        assert_eq!(endpoint_order(vec![v4, v6], None, None, ALL), [v4, v6]);
        assert_eq!(endpoint_order(vec![v4, v6], Some(v6), None, ALL), [v6, v4]);
        assert_eq!(endpoint_order(vec![v4, v6], None, None, NO_V6), [v4]);
        assert_eq!(endpoint_order(vec![v4, v6], Some(v6), None, NO_V6), [v4]);
        // PodUtils/podnet issue 133: a door that carried no packet goes last,
        // also when it won the handshake.
        assert_eq!(endpoint_order(vec![v4, v6], Some(v4), Some(v4), ALL), [v6, v4]);
        assert_eq!(endpoint_order(vec![v4, v6], None, Some(v4), ALL), [v6, v4]);
    }

    // PodUtils/podnet@T-169: with no IPv4 route, the IPv6 door goes first; a
    // door that won keeps its place.
    #[test]
    fn endpoint_order_puts_v6_first_without_a_v4_route() {
        let (v4, v6) = (v4(), v6());
        assert_eq!(endpoint_order(vec![v4, v6], None, None, NO_V4), [v6, v4]);
        assert_eq!(endpoint_order(vec![v4, v6], Some(v4), None, NO_V4), [v4, v6]);
        assert_eq!(endpoint_order(vec![v4, v6], None, None, ALL), [v4, v6]);
    }

    // PodUtils/podnet#413, ask 10: a pinned door is the only door, of a name
    // or of an address, and the order of the record does not apply; with no
    // pin, the order of the record.
    #[test]
    fn a_pinned_door_skips_the_order() {
        let (v4, v6) = (v4(), v6());
        let pinned = Door::Pinned(Target::Addr(v6));
        assert_eq!(doors(&pinned, vec![v4, v6], Some(v4), None, NO_V4), [Target::Addr(v6)]);
        let named: Target = "door.example:8443".parse().unwrap();
        assert_eq!(doors(&Door::Pinned(named.clone()), vec![v4, v6], None, None, ALL), [named]);
        assert_eq!(doors(&Door::Record, vec![v4, v6], None, None, NO_V4), [Target::Addr(v6), Target::Addr(v4)]);
        assert_eq!(doors(&Door::Record, vec![v4, v6], None, None, ALL), [Target::Addr(v4), Target::Addr(v6)]);
        // No door of the order (an IPv6 record on a host with no IPv6
        // route): the record's first door.
        assert_eq!(doors(&Door::Record, vec![v6], None, None, NO_V6), [Target::Addr(v6)]);
    }
}
