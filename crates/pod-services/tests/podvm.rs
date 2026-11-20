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
    assert_eq!(
        ids,
        [
            "alpine-netboot",
            "alpine-netboot-aarch64",
            "fedora-virtio-win",
            "freedos-image",
            "microsoft-validationos",
            "microsoft-windows-eval"
        ]
    );
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

/// podvm `crates/podvm-windows/src/catalog.rs`, the entry `2019`
/// (PodUtils/podvm#361): the media of `windows:2019`, and the range of the
/// watch inside the ISO.
#[test]
fn the_windows_evaluation_media_hold_the_pin_of_podvm() {
    let eval = service("microsoft-windows-eval");
    let iso = eval.endpoint("server-2019").unwrap();
    assert_eq!(
        iso.url(),
        "https://software-download.microsoft.com/download/pr/\
         17763.737.190906-2324.rs5_release_svc_refresh_SERVER_EVAL_x64FRE_en-us_1.iso"
    );
    assert_eq!(eval.int("server-2019-bytes"), Some(5_296_713_728));
    assert_eq!(
        eval.text("server-2019-sha256"),
        Some("549bca46c055157291be6c22a3aaaed8330e78ef4382c99ee82c896426a1cee1")
    );
    let fact = |name| eval.int(name).unwrap_or_else(|| panic!("no {name}"));
    let start = fact("pvd-lba") * fact("sector-bytes");
    assert_eq!(start, 32_768);
    assert!(start + fact("pvd-bytes") <= fact("server-2019-bytes"));
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

/// The release archive of wasmtime for podvm's profile `wasmtime-wasi`
/// (PodUtils/podvm#275): the pins of podvm, and the member of the binary.
#[test]
fn the_archive_of_wasmtime_holds_the_pins_of_podvm() {
    let w = service("wasmtime-release");
    let version = w.text("version").unwrap();
    let archive = w.endpoint("archive").unwrap();
    assert_eq!(archive.host(), "github.com");
    assert!(archive.path().unwrap().ends_with(&format!("/v{version}/wasmtime-v{version}-x86_64-linux.tar.xz")));
    assert_eq!(w.text("archive-sha256"), Some("685466455dd42a14774a86c36c8f2e45cbb9bd6d92a850139315df60a75498a5"));
    assert_eq!(w.int("archive-bytes"), Some(11_577_424));
    assert_eq!(w.text("bin-member"), Some(format!("wasmtime-v{version}-x86_64-linux/wasmtime").as_str()));
    assert_eq!(w.text("bin-sha256"), Some("983e57573a320cf44c93ccaa571a25640cf38b15e8e14160aa7bb919336caae8"));
    assert_eq!(w.int("bin-bytes"), Some(67_964_264));
}

/// The static QEMU of podvm's store (PodUtils/podvm#389, PodUtils/podvm#618):
/// QEMU 11.1.3 of PodUtils/static-pkgs, which migrates an NVMe device (the
/// warm save of the plan line, T-1035) and holds the fix of the
/// use-after-free of `address_space_translate_for_iotlb`; each archive and
/// each binary in it by its SHA-256 and its length.
#[test]
fn the_static_qemu_is_11_1_3_with_the_migration_of_nvme() {
    let q = service("static-pkgs-qemu");
    assert_eq!(q.party(), "PodUtils");
    assert_eq!(q.text("version"), Some("11.1.3"));
    assert_eq!(q.text("release"), Some("qemu-11.1.3"));
    assert_eq!(q.text("nvme-commit"), Some("9d378cab537d70d9e02edc11ff6ded7266c1078e"));
    assert_eq!(q.text("fix-commit"), Some("2865bf1c5795371ef441e9029bc22566ccff8299"));
    let base = "/PodUtils/static-pkgs/releases/download/qemu-11.1.3/";
    // The name of an endpoint is a word: `_` becomes `-`.
    for (file, sha, bytes, bin_sha, bin_bytes) in [
        (
            "qemu-system-x86_64",
            "368da02758b8b474448a7a37fed601256ec9cd7e11c09656dbb85276e778c888",
            6_814_799,
            "5b5c3a2c181ae3381d1881d615ab923ec1ae35188d0274246ef14863b71c14d4",
            33_571_760,
        ),
        (
            "qemu-system-aarch64",
            "d8a2cd808f1a16c59be6805aef1bbe24d9dce45b66da49c18d6af3f52e735eb4",
            8_211_695,
            "9fba494bd2b25efa022b36d056b840c3c00901d3a65152ed54595e844e98b678",
            38_909_904,
        ),
        (
            "qemu-img",
            "cffa14e62afc745f5487beb70b5caa6a4814853f5bba92a03e3eeddab53c43a5",
            1_430_921,
            "e88a84357dd7f86d2f6dd7159deff4a929c6d9f030b691f378d9a59c0b7cc0ea",
            3_209_680,
        ),
    ] {
        let name = file.replace('_', "-");
        let e = q.endpoint(&name).unwrap();
        assert_eq!(e.host(), "github.com");
        assert_eq!(e.path(), Some(format!("{base}{file}-11.1.3-linux-amd64.tar.gz").as_str()), "{name}");
        assert_eq!(q.text(&format!("{name}-sha256")), Some(sha), "{name}");
        assert_eq!(q.int(&format!("{name}-bytes")), Some(bytes), "{name}");
        assert_eq!(q.text(&format!("{name}-member")), Some(format!("./bin/{file}").as_str()), "{name}");
        assert_eq!(q.text(&format!("{name}-bin-sha256")), Some(bin_sha), "{name}");
        assert_eq!(q.int(&format!("{name}-bin-bytes")), Some(bin_bytes), "{name}");
    }
    let data = q.endpoint("data").unwrap();
    assert_eq!(data.path(), Some(format!("{base}qemu-data-11.1.3-linux-amd64.tar.gz").as_str()));
    assert_eq!(q.text("data-sha256"), Some("b11bff496ff7e345c9e4931306ccc3074e55d5eeb62d4a25f9efb70a3500f66e"));
    assert_eq!(q.int("data-bytes"), Some(18_171_081));
    assert!(pod_services::get("hermeticbuild-qemu").is_none(), "QEMU 11.0.0 cannot save an NVMe device");
    assert!(pod_services::get("pkgforge-qemu").is_none(), "QEMU 9.2.0 has no fix of the use-after-free");
}

/// The aarch64 netboot of Alpine for podvm's arm64 guest
/// (PodUtils/podvm#394): the release of the x86_64 pin and podvm's sums.
#[test]
fn the_aarch64_netboot_holds_the_pins_of_podvm() {
    let arm = service("alpine-netboot-aarch64");
    assert_eq!(arm.text("release"), service("alpine-netboot").text("release"));
    let archive = arm.endpoint("archive").unwrap().path().unwrap();
    assert!(archive.ends_with(&format!("/aarch64/alpine-netboot-{}-aarch64.tar.gz", arm.text("release").unwrap())));
    assert_eq!(
        arm.text("vmlinuz-virt-sha256"),
        Some("dd37c4460f933ec909ff022c34944ff7e1a5300aff99750e3bdc02cd4074c332")
    );
    assert_eq!(
        arm.text("modloop-virt-sha256"),
        Some("da142869626ec9d4543ab46482af10b5ff2c8c1c2e01068ddf32360316a06218")
    );
    assert_eq!(arm.int("archive-bytes"), Some(341_396_208));
}
