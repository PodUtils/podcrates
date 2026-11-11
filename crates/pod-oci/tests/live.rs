//! A live pull from a public registry (T-142), from the service
//! `ecr-public-registry` of `pod-services`: the first image of its fact
//! `images` (a tag pinned by the digest of its index) for linux/amd64 and
//! linux/arm64, and its `guest-userland` (pinned by the digest of one
//! manifest). The client checks each index, manifest and config blob
//! against its digest while it reads them; the test checks that the top
//! document is the pinned one and that each config names its platform. A
//! rate limit of the registry (429) is a skip, not a failure. `live.yml`
//! runs it each day; the checks of a push need no network.

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

/// The resolve of `image` (`REPOSITORY[:TAG]@DIGEST` on `host`) for
/// `platform`, and its config as JSON; `None` after a 429.
fn pull_config(
    client: &mut pod_oci::Client,
    host: &str,
    image: &str,
    platform: &Platform,
) -> Option<(pull::Resolved, serde_json::Value)> {
    let reference = Reference::parse(&format!("{host}/{image}")).expect("a reference");
    let resolved = match pull::resolve(client, &reference, platform) {
        Ok(resolved) => resolved,
        Err(f) if f.to_string().contains("429") => {
            eprintln!("skip: {host} gave 429 (a rate limit): {f}");
            return None;
        }
        Err(f) => panic!("{image} for {platform:?}: {f}"),
    };
    let config = resolved.manifest.config.clone();
    let digest = config.parsed_digest().expect("the digest of the config");
    let mut out = Vec::new();
    let sink = Vec::new();
    let bytes =
        match client.blob(reference.endpoint(), &reference.repository, &digest, Some(config.size), sink, &mut out) {
            Ok(bytes) => bytes,
            Err(f) if f.to_string().contains("429") => {
                eprintln!("skip: {host} gave 429 (a rate limit): {f}");
                return None;
            }
            Err(f) => panic!("the config {digest} of {image} for {platform:?}: {f}"),
        };
    assert_eq!(bytes.len() as u64, config.size, "{image} {platform:?}");
    eprintln!("{image} {platform:?}: manifest {}, config {digest}", resolved.manifest_digest);
    Some((resolved, serde_json::from_slice(&bytes).expect("a config is JSON")))
}

#[test]
#[ignore = "a live registry: live.yml runs it"]
fn a_pinned_index_resolves_for_two_platforms_and_a_pinned_manifest_for_one() {
    let ecr = pod_services::get("ecr-public-registry").expect("the service ecr-public-registry");
    let host = ecr.endpoint("registry").expect("the endpoint registry").host();
    let mut client = common::client::client(env_of_process(), &[]);
    let images = ecr.texts("images").expect("the fact images");
    let index: &str = &images[0];
    let pinned = index.split_once('@').map(|(_, d)| d).expect("pinned by a digest");
    for platform in ["linux/amd64", "linux/arm64"] {
        let platform = Platform::parse(platform).expect("a platform");
        let Some((resolved, config)) = pull_config(&mut client, host, index, &platform) else { return };
        assert_eq!(resolved.top.digest.to_string(), pinned, "the index is not the pinned one");
        assert_ne!(resolved.manifest_digest.to_string(), pinned, "{platform:?}: no manifest of the platform");
        assert_eq!(config["architecture"], platform.arch.as_str(), "{platform:?}");
        assert_eq!(config["os"], "linux", "{platform:?}");
    }
    let userland = ecr.text("guest-userland").expect("the fact guest-userland");
    let pinned = userland.split_once('@').map(|(_, d)| d).expect("pinned by a digest");
    let platform = Platform::parse("linux/amd64").expect("a platform");
    let Some((resolved, config)) = pull_config(&mut client, host, userland, &platform) else { return };
    assert_eq!(resolved.manifest_digest.to_string(), pinned, "the manifest is not the pinned one");
    assert_eq!(config["os"], "linux");
}
