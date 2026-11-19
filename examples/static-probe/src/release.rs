//! pod-release: the compiled roots of Sigstore and a check of a bundle that
//! is no bundle, so that x509-parser and the verifiers of P-256 and P-384
//! link in; a fetcher over the client of pod-proxy, which refuses a URL
//! that is not HTTPS before a dial.

pub fn release(client: pod_proxy::Client) -> String {
    let roots = pod_release::Roots::sigstore();
    let who = pod_release::Identity::github("PodUtils/podcrates", "release.yml", "v0.0.0");
    let refused = pod_release::verify_release(b"", b"{}", &who, &roots).map_or_else(|e| e.to_string(), |_| "ok".into());
    let fetcher = pod_release::fetch::Fetcher::new(client);
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
    let plain = runtime
        .block_on(fetcher.get("http://probe.example/SHA256SUMS", "*/*", 1, std::time::Duration::from_secs(1)))
        .map_or_else(|e| e.to_string(), |b| format!("{} bytes", b.len()));
    format!("pod-release: {} certificates of Fulcio; {refused}; {plain}", roots.fulcio.len())
}
