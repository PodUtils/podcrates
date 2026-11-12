//! The bootstrap nodes of HyperDHT (PodUtils/podssh#204): podssh reads the
//! fact `nodes` and gives each text to HyperDHT as it is, so each text
//! is `IP@NAME:PORT` and names one endpoint of the file.

use pod_services::Service;

fn service() -> &'static Service {
    pod_services::get("hyperdht-bootstrap").expect("the service hyperdht-bootstrap")
}

#[test]
fn each_node_is_ip_at_name_colon_port() {
    let nodes = service().texts("nodes").expect("the fact nodes");
    assert!(!nodes.is_empty());
    for node in nodes {
        let (ip, rest) = node.split_once('@').unwrap_or_else(|| panic!("{node}: no @"));
        let (name, port) = rest.rsplit_once(':').unwrap_or_else(|| panic!("{node}: no port"));
        assert!(ip.parse::<std::net::Ipv4Addr>().is_ok(), "{node}: {ip} is no IPv4 address");
        assert!(!name.is_empty() && name.contains('.'), "{node}: {name} is no host name");
        assert!(port.parse::<u16>().is_ok_and(|p| p > 0), "{node}: {port} is no port");
    }
}

#[test]
fn each_node_is_an_endpoint_and_each_address_is_in_the_watch() {
    let s = service();
    let nodes = s.texts("nodes").unwrap();
    let addresses = s.texts("addresses").expect("the fact addresses");
    assert_eq!(nodes.len(), s.endpoints().len(), "one endpoint for each node");
    assert_eq!(nodes.len(), addresses.len(), "one address for each node");
    for e in s.endpoints() {
        assert_eq!((e.scheme(), e.role()), ("udp", Some("node")), "{}", e.name());
        let [ip] = e.addresses() else { panic!("{}: one address", e.name()) };
        let node = format!("{ip}@{}:{}", e.host(), e.port());
        assert!(nodes.iter().any(|n| n.as_ref() == node), "{node}: in the fact nodes");
        assert!(addresses.iter().any(|a| a == ip), "{ip}: in the fact addresses");
    }
}
