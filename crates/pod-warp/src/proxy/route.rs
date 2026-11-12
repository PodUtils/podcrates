//! The way of each destination of a proxy by its rules (PodUtils/podnet
//! issues 132 and 136): through the tunnel, directly from the host, or not
//! at all.

use std::sync::Arc;

use pod_core::{BoxedIo, Dial, Failure, Target};

use super::Proxy;
use super::names::{Inside, connect_ips};
use crate::rules::{Action, Host, Refused};

/// Connect to `target` by the rules of `proxy` through `net`. Each decision
/// counts in the counters of the rules and in the decision `warp.rule`.
pub(super) async fn route(proxy: &Proxy, net: Arc<dyn Dial>, target: Target) -> Result<BoxedIo, Failure> {
    let table = proxy.rules.current();
    let inside = Inside { names: &proxy.names, net: net.as_ref() };
    let (action, addresses) = table.decide(&inside, &Host::of(&target)).await?;
    proxy.rules.counters.count(action);
    proxy.report.decision("warp.rule", action.name());
    match action {
        Action::Reject => {
            tracing::debug!(%target, "WARP proxy: refused by the rules");
            Err(Refused(target.to_string()).failure())
        }
        Action::Direct => {
            tracing::debug!(%target, "WARP proxy: direct by the rules");
            let deadline = tokio::time::Instant::now() + proxy.connect_timeout;
            Ok(proxy.direct.dial(&target, deadline).await?.io)
        }
        Action::Tunnel => {
            let connected = match addresses {
                Some(addresses) => connect_ips(net.as_ref(), addresses, target.port(), proxy.connect_timeout).await,
                None => proxy.names.connect(net.as_ref(), &target, proxy.connect_timeout).await,
            };
            Ok(connected?.io)
        }
    }
}
