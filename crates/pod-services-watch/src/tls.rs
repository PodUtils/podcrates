//! `tls-name` and `tls-names`: a verified TLS handshake with an endpoint.

use pod_services::{Service, Watch};

use crate::args::Args;
use crate::{Checked, Finding, Net, Outcome};

/// `tls-name`: a TLS handshake with each endpoint (of `endpoint` or
/// `role`) verifies its server name.
pub async fn name(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let mut out = Outcome::default();
    for endpoint in Args::new(service, watch).endpoints()? {
        match net.tls(endpoint.host(), endpoint.port(), endpoint.server_name()).await {
            Ok(_) => out.saw(format!("{} verified at {}", endpoint.server_name(), endpoint.authority())),
            Err(finding) => out.found(finding),
        }
    }
    Ok(out)
}

/// `tls-names`: at each address of each endpoint (of `endpoint` or
/// `role`), the certificate names the host and the address: one handshake
/// verifies the host, and a second one the address.
pub async fn names(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let mut out = Outcome::default();
    for endpoint in Args::new(service, watch).endpoints()? {
        let addresses = endpoint.ip_addrs();
        if addresses.is_empty() {
            out.found(Finding::failed(format!("the endpoint {:?} has no address", endpoint.name())));
            continue;
        }
        let host = endpoint.server_name().trim_end_matches('.');
        for ip in addresses {
            let at = ip.to_string();
            let mut named = 0;
            for name in [host.to_string(), at.clone()] {
                match net.tls(&at, endpoint.port(), &name).await {
                    Ok(_) => named += 1,
                    Err(Finding::Failed(why)) => out.found(Finding::failed(format!("no handshake for {name}: {why}"))),
                    Err(other) => out.found(other),
                }
            }
            if named == 2 {
                out.saw(format!("the certificate at {at} names {host} and {at}"));
            }
        }
    }
    Ok(out)
}
