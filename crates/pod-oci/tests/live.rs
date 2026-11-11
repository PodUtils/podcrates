//! A live pull from a public registry (T-142): the image `guest-userland`
//! of the service `ecr-public-registry` of `pod-services`, pinned by the
//! digest of its index, for linux/amd64 and linux/arm64. The client checks
//! the index, each manifest and each config blob against its digest while
//! it reads them; the test checks that the index is the pinned one and that
//! each config names its platform. A rate limit of the registry (429) is a
//! skip, not a failure. `live.yml` runs it each day; the checks of a push
//! need no network.

mod common;

use pod_oci::{Platform, Reference, pull};

/// The proxy settings of the process: the runner of `live.yml` has none.
fn env_of_process() -> pod_proxy::Environment {
    let names =
        ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy", "NO_PROXY", "no_proxy"];
    let pairs: Vec<(String, String)> =
        names.iter().filter_map(|n| std::env::var(n).ok().map(|v| (n.to_string(), v))).collect();
    let pairs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    common::client::environment(&pairs)
}

#[test]
#[ignore = "a live registry: live.yml runs it"]
fn the_pinned_guest_userland_resolves_for_two_platforms_and_each_config_checks() {
    let ecr = pod_services::get("ecr-public-registry").expect("the service ecr-public-registry");
    let host = ecr.endpoint("registry").expect("the endpoint registry").host();
    let image = ecr.text("guest-userland").expect("the fact guest-userland");
    let pinned = image.split_once('@').map(|(_, d)| d).expect("the image is pinned by a digest");
    let reference = Reference::parse(&format!("{host}/{image}")).expect("a reference");
    let mut client = common::client::client(env_of_process(), &[]);
    for platform in ["linux/amd64", "linux/arm64"] {
        let platform = Platform::parse(platform).expect("a platform");
        let resolved = match pull::resolve(&mut client, &reference, &platform) {
            Ok(resolved) => resolved,
            Err(f) if f.to_string().contains("429") => {
                eprintln!("skip: {host} gave 429 (a rate limit): {f}");
                return;
            }
            Err(f) => panic!("{reference:?} for {platform:?}: {f}"),
        };
        assert_eq!(resolved.top.digest.to_string(), pinned, "the index is not the pinned one");
        assert_ne!(resolved.manifest_digest.to_string(), pinned, "{platform:?}: no manifest of the platform");
        let config = &resolved.manifest.config;
        let digest = config.parsed_digest().expect("the digest of the config");
        let mut out = Vec::new();
        let bytes = match client.blob(
            reference.endpoint(),
            &reference.repository,
            &digest,
            Some(config.size),
            Vec::new(),
            &mut out,
        ) {
            Ok(bytes) => bytes,
            Err(f) if f.to_string().contains("429") => {
                eprintln!("skip: {host} gave 429 (a rate limit): {f}");
                return;
            }
            Err(f) => panic!("the config {digest} of {platform:?}: {f}"),
        };
        assert_eq!(bytes.len() as u64, config.size, "{platform:?}");
        let config: serde_json::Value = serde_json::from_slice(&bytes).expect("a config is JSON");
        assert_eq!(config["architecture"], platform.arch.as_str(), "{platform:?}");
        assert_eq!(config["os"], "linux", "{platform:?}");
        eprintln!("{platform:?}: manifest {}, config {digest}, {} bytes", resolved.manifest_digest, bytes.len());
    }
}
