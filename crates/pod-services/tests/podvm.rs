//! The registries and the guest images of podvm: the constants that podvm
//! compiles in or pins in its experiments (PodUtils/podvm at `0249527`)
//! come back from the files, and each pin has the form of its kind.

use pod_services::Service;

fn service(id: &str) -> &'static Service {
    pod_services::get(id).unwrap_or_else(|| panic!("no service {id}"))
}

fn is_sha256(hex: &str) -> bool {
    hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The facts of a registry that hold images: one reference or a list.
fn images(registry: &Service) -> Vec<&str> {
    let mut all = Vec::new();
    for fact in registry.facts() {
        if let Some(list) = fact.value().as_texts() {
            all.extend(list.iter().map(|t| t.as_ref()).filter(|t| t.contains("@sha256:")));
        } else if let Some(text) = fact.value().as_text().filter(|t| t.contains("@sha256:")) {
            all.push(text);
        }
    }
    all
}

#[test]
fn each_registry_answers_at_v2_over_https() {
    let ids: Vec<&str> = pod_services::of_kind("oci-registry").map(|s| s.id()).collect();
    assert_eq!(
        ids,
        ["docker-hub-registry", "ecr-public-registry", "ghcr-registry", "opensuse-registry", "quay-registry"]
    );
    for id in ids {
        let registry = service(id).endpoint("registry").unwrap_or_else(|| panic!("{id}: no endpoint registry"));
        assert_eq!((registry.scheme(), registry.port(), registry.path()), ("https", 443, Some("/v2/")), "{id}");
        assert!(!images(service(id)).is_empty(), "{id}: no pinned image");
    }
}

#[test]
fn each_pinned_image_is_a_repository_and_a_sha256_digest() {
    for registry in pod_services::of_kind("oci-registry") {
        for image in images(registry) {
            let (name, digest) = image.split_once("@sha256:").unwrap();
            assert!(is_sha256(digest), "{image}");
            let repository = name.split_once(':').map_or(name, |(r, _)| r);
            assert!(!repository.is_empty() && !repository.contains("//"), "{image}");
            assert!(!repository.contains('.'), "{image}: the host is the endpoint, not a part of the name");
        }
    }
}

/// podvm `crates/podvm-image/src/reference.rs` and `crates/podvm/src/images/` (`images.rs` before podvm `0d4b943`).
#[test]
fn the_hub_of_podvm_s_references_is_the_docker_hub_file() {
    let hub = service("docker-hub-registry");
    assert_eq!(hub.text("domain"), Some("docker.io"), "DEFAULT_DOMAIN");
    assert_eq!(hub.endpoint("registry").unwrap().host(), "registry-1.docker.io", "DEFAULT_ENDPOINT");
    assert_eq!(hub.texts("aliases").map(|a| a.len()), Some(1));
    assert_eq!(hub.texts("aliases").unwrap()[0], "index.docker.io");
    assert_eq!(hub.text("official-prefix"), Some("library"), "OFFICIAL_PREFIX");
    assert_eq!(hub.text("credential-key"), Some("https://index.docker.io/v1/"), "the default of podvm login");
}

/// podvm `crates/podvm-measure/src/workload.rs` `ALPINE_REF`, `KURL`,
/// `KERNEL_SHA256`.
#[test]
fn the_inputs_of_podvm_measure_come_from_the_files() {
    let ecr = service("ecr-public-registry");
    let userland = format!("{}/{}", ecr.endpoint("registry").unwrap().host(), ecr.text("guest-userland").unwrap());
    assert_eq!(
        userland,
        "public.ecr.aws/docker/library/alpine@sha256:3e9b4b680bfc9fb5269227cffbd6d42be39fbf7c0b908123913864aa4447e764"
    );
    let netboot = service("alpine-netboot");
    let kernel = format!("{}vmlinuz-virt", netboot.endpoint("netboot").unwrap().url());
    assert_eq!(kernel, "https://dl-cdn.alpinelinux.org/alpine/v3.22/releases/x86_64/netboot/vmlinuz-virt");
    assert_eq!(
        netboot.text("vmlinuz-virt-sha256"),
        Some("6b58e5d779e44e57c9efa20232da18650415eceb9d4f544e5c165c1f392c5d51")
    );
    let archive = netboot.endpoint("archive").unwrap().path().unwrap();
    assert!(archive.ends_with(&format!("alpine-netboot-{}-x86_64.tar.gz", netboot.text("release").unwrap())));
}

#[test]
fn each_sum_of_a_guest_image_is_a_sha256() {
    let ids: Vec<&str> = pod_services::of_kind("guest-image").map(|s| s.id()).collect();
    assert_eq!(ids, ["alpine-netboot", "freedos-image", "microsoft-validationos"]);
    for id in ids {
        let sums: Vec<&str> = service(id)
            .facts()
            .iter()
            .filter(|f| f.name().ends_with("sha256"))
            .filter_map(|f| f.value().as_text())
            .collect();
        assert!(!sums.is_empty(), "{id}: no sum");
        assert!(sums.iter().all(|s| is_sha256(s)), "{id}");
    }
}

/// podvm `experiments/371-validationos-stream.sh`: the VHDX is one extent
/// inside the ISO.
#[test]
fn the_vhdx_extent_lies_inside_the_iso() {
    let vos = service("microsoft-validationos");
    let fact = |name| vos.int(name).unwrap_or_else(|| panic!("no {name}"));
    let start = fact("vhdx-lba") * fact("sector-bytes");
    assert_eq!(start, 2_068_480);
    assert!(start + fact("vhdx-bytes") <= fact("iso-bytes"));
    let dos = service("freedos-image");
    assert_eq!(dos.int("image-bytes"), Some(32 << 20), "FD_IMG_BYTES");
}
