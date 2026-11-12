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
        [
            "docker-hub-registry",
            "ecr-public-registry",
            "gcr-registry",
            "ghcr-registry",
            "mcr-registry",
            "mirror-gcr-registry",
            "opensuse-registry",
            "quay-registry"
        ]
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

/// podvm's inventory gaps (PodUtils/podvm#263, podvm `1cb2fb1`): the
/// registry of `distroless-static`, the mirror of the measure workload, and
/// the sums of `MODULES` in `crates/podvm/src/machine/guest/kernel.rs`.
#[test]
fn the_gaps_of_podvm_s_inventory_come_from_the_files() {
    let gcr = service("gcr-registry");
    assert_eq!(gcr.endpoint("registry").unwrap().host(), "gcr.io");
    assert!(gcr.texts("images").unwrap()[0].starts_with("distroless/static-debian12:latest@sha256:"));
    let mirror = service("mirror-gcr-registry");
    assert_eq!(mirror.text("mirror-of"), Some("docker-hub-registry"));
    assert!(pod_services::get(mirror.text("mirror-of").unwrap()).is_some());
    let pin = format!("{}/{}", mirror.endpoint("registry").unwrap().host(), mirror.texts("images").unwrap()[0]);
    assert_eq!(
        pin, "mirror.gcr.io/library/alpine@sha256:3e9b4b680bfc9fb5269227cffbd6d42be39fbf7c0b908123913864aa4447e764",
        "ALPINE_MIRROR_REF"
    );
    let ecr = service("ecr-public-registry").text("guest-userland").unwrap();
    assert_eq!(pin.split_once('@').unwrap().1, ecr.split_once('@').unwrap().1, "the mirror holds the pin of ecr");
    let netboot = service("alpine-netboot");
    assert_eq!(netboot.text("kernel-release"), Some("6.12.110-0-virt"));
    for (module, sum) in [
        ("failover", "6abca214c939b70af47d83380251e0ea16a1d8e47490ddb0521e64abcad3e1a1"),
        ("net-failover", "0ede1d375546f9b177cc1d4250bc71d74e9a8444a83f5da7d1f92b7479b6c036"),
        ("virtio-net", "fd93a908f5cc37eeb446c7255d614af90ab13dcadd21ddd7bada67a77cb1d177"),
    ] {
        assert_eq!(netboot.text(&format!("module-{module}-sha256")), Some(sum), "MODULES {module}");
    }
}

/// The registry of podvm's profile `playwright-chromium`
/// (PodUtils/podvm#269): the pin of the profile, and no token.
#[test]
fn the_registry_of_playwright_holds_the_pin_of_podvm() {
    let mcr = service("mcr-registry");
    assert_eq!(mcr.endpoint("registry").unwrap().host(), "mcr.microsoft.com");
    assert!(mcr.fact("token-realm").is_none(), "/v2/ answers 200 with no challenge");
    let pin = format!("{}/{}", mcr.endpoint("registry").unwrap().host(), mcr.texts("images").unwrap()[0]);
    assert_eq!(
        pin,
        "mcr.microsoft.com/playwright:v1.65.0-noble@sha256:9beb546099e11f52d62e505537914870e01a67e0e6b00ae41da4775324e73dbe",
        "the image of playwright-chromium"
    );
}

/// The npm archive of workerd for podvm's profile `workerd-test`
/// (PodUtils/podvm#274): the pins of podvm, and the member of the binary.
#[test]
fn the_archive_of_workerd_holds_the_pins_of_podvm() {
    let w = service("cloudflare-workerd-npm");
    let archive = w.endpoint("archive").unwrap();
    assert_eq!(archive.host(), "registry.npmjs.org");
    assert!(archive.path().unwrap().ends_with(&format!("workerd-linux-64-{}.tgz", w.text("version").unwrap())));
    assert_eq!(w.text("archive-sha256"), Some("771936b71cbc88d01e5cbb2779ebabc91a5b79070b8f41d3d203973afb779c85"));
    assert_eq!(w.int("archive-bytes"), Some(43_615_013));
    assert!(w.text("archive-integrity").unwrap().starts_with("sha512-"));
    assert_eq!(w.text("bin-member"), Some("package/bin/workerd"));
    assert_eq!(w.text("bin-sha256"), Some("685acc339ac78bf7b605928c1845fef8d8a89e919b079556b571a3404b95db3e"));
    assert_eq!(w.int("bin-bytes"), Some(148_775_704));
}
