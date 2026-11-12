//! The facts of WARP: the service `cloudflare-warp` of `pod-services` (its
//! API, its front doors, its versions and its Terms), and the resolver in
//! the tunnel of the service `cloudflare-dns`. A caller can give other
//! values in [`crate::Settings`]; these are the defaults.

use std::net::SocketAddr;

use pod_services::{Endpoint, Service};

/// The service `cloudflare-warp`.
pub fn service() -> &'static Service {
    pod_services::get("cloudflare-warp").expect("pod-services holds the service cloudflare-warp")
}

fn endpoint(name: &str) -> &'static Endpoint {
    service().endpoint(name).unwrap_or_else(|| panic!("cloudflare-warp of pod-services has the endpoint {name}"))
}

fn text(name: &str) -> &'static str {
    service().text(name).unwrap_or_else(|| panic!("cloudflare-warp of pod-services holds the fact {name}"))
}

/// The API host, on TCP 443: the endpoint `api`.
pub fn api_base() -> String {
    endpoint("api").url()
}

/// A second name of the same API, tried after a dial, TLS or 5xx failure
/// of the first: a proxy allowlist can have one name and not the other.
/// The endpoint `api-second`.
pub fn api_second() -> String {
    endpoint("api-second").url()
}

/// The version segment of the API. It must agree with the build number of
/// [`client_version`]; the service refuses a pair that does not. The fact
/// `api-version`.
pub fn api_version() -> &'static str {
    text("api-version")
}

/// The value of `CF-Client-Version`: the fact `client-version`.
pub fn client_version() -> &'static str {
    text("client-version")
}

/// The `User-Agent` of the Android client, which the service expects: the
/// fact `user-agent`.
pub fn user_agent() -> &'static str {
    text("user-agent")
}

/// The TLS name of the MASQUE front door (the endpoint `masque-v4`).
pub fn masque_sni() -> &'static str {
    endpoint("masque-v4").server_name()
}

/// The MASQUE port of the front door: 443. The port of a peer in the
/// device record is QUIC's or WireGuard's, or 0.
pub fn masque_port() -> u16 {
    endpoint("masque-v4").port()
}

/// The front doors of the data of `pod-services`: `masque-v4`, then
/// `masque-v6`. A device record gives its own; these are for a record with
/// none.
pub fn masque_doors() -> Vec<SocketAddr> {
    ["masque-v4", "masque-v6"].iter().flat_map(|name| endpoint(name).socket_addrs()).collect()
}

/// The `:authority` of the CONNECT-IP request. Only the host matters:
/// Cloudflare reads it as a hint of routing. The fact `connect-uri`.
pub fn connect_uri() -> &'static str {
    text("connect-uri")
}

/// The Terms that a registration accepts, which the user accepts first:
/// the fact `terms-url`.
pub fn terms_url() -> &'static str {
    text("terms-url")
}

/// The service `cloudflare-dns`: the resolver inside the tunnel.
fn dns() -> &'static Service {
    pod_services::get("cloudflare-dns").expect("pod-services holds the service cloudflare-dns")
}

/// The resolver that the check of the data path asks through the tunnel:
/// the endpoint `udp-1` of `cloudflare-dns` (1.1.1.1, port 53).
pub fn tunnel_resolver() -> SocketAddr {
    let endpoint = dns().endpoint("udp-1").expect("cloudflare-dns of pod-services has the endpoint udp-1");
    *endpoint.socket_addrs().first().expect("udp-1 of pod-services is an address")
}

/// The name of the question of the check of the data path: the fact
/// `check-name` of `cloudflare-dns`.
pub fn check_name() -> &'static str {
    dns().text("check-name").expect("cloudflare-dns of pod-services holds the fact check-name")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The facts that podnet and podssh read are in the data, and the two
    /// versions agree in their build number.
    #[test]
    fn the_facts_of_warp_are_in_the_data() {
        assert_eq!(api_base(), "https://api.cloudflareclient.com");
        assert_eq!(api_second(), "https://api.devices.cloudflare.com");
        assert_eq!(masque_port(), 443);
        assert_eq!(masque_sni(), "consumer-masque.cloudflareclient.com");
        let build = |v: &str| v.rsplit(|c: char| !c.is_ascii_digit()).next().unwrap_or("").to_string();
        assert_eq!(build(api_version()), build(client_version()));
        let doors = masque_doors();
        assert_eq!(doors.len(), 2);
        assert!(doors[0].is_ipv4() && doors[1].is_ipv6(), "{doors:?}");
        assert!(connect_uri().starts_with("https://"));
        assert!(!user_agent().is_empty() && terms_url().starts_with("https://"));
        assert_eq!(tunnel_resolver(), "1.1.1.1:53".parse().unwrap());
        assert_eq!(check_name(), "cloudflare.com");
    }
}
