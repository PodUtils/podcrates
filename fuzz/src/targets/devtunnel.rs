//! The connect information of `pod-devtunnel` (T-378).

/// An answer of the management API of Dev Tunnels
/// (`pod_devtunnel::ConnectInfo::parse`; new in `pod-devtunnel`, T-378).
/// An answer that parses has a connect token and at least one relay
/// endpoint, each with a `wss://` URI or a `ws://` one to the loopback, and
/// at most the bounded counts of endpoints and keys; its `Debug` and the
/// text of a failure hold no token of the answer and no line break.
pub fn devtunnel_connect_info(data: &[u8]) {
    use pod_devtunnel::info::{ConnectInfo, ENDPOINTS_MOST, KEYS_MOST};
    const TOKEN: &str = "made-up-connect-token";
    match ConnectInfo::parse(data, "connect") {
        Ok(info) => {
            assert!(!info.endpoints.is_empty());
            assert!(info.endpoints.len() <= ENDPOINTS_MOST);
            for endpoint in &info.endpoints {
                assert!(endpoint.host_keys.len() <= KEYS_MOST);
                assert!(endpoint.relay.tls || pod_devtunnel::settings::is_loopback(&endpoint.relay.host));
            }
            let shown = format!("{info:?}");
            assert!(!shown.contains(TOKEN), "{shown}");
        }
        Err(failure) => {
            let chain = failure.chain();
            assert!(!chain.contains(TOKEN) && !chain.contains('\n'), "{chain:?}");
        }
    }
}
