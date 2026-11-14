//! The whole completion on two layers against the completion of one root
//! that holds the same tree.

use super::*;
use crate::reach::Fixed;
use crate::{HostBundle, Report, Settings, complete};

const PEM: &str = "-----BEGIN CERTIFICATE-----\nIMAGE\n-----END CERTIFICATE-----\n";
const HOST: &str = "-----BEGIN CERTIFICATE-----\nHOST\n-----END CERTIFICATE-----\n";

/// An image that each step reads: a read-only passwd, links into `/proc`
/// and out of the root, a symlinked `lib` and trust store, a hash-indexed
/// CA directory, `http://` sources in a file and a directory, and a link
/// at a device path.
fn image(lower: &Path) {
    put(lower, "etc/passwd", "root:x:0:0:root:/root:/bin/sh\n", 0o444);
    put(lower, "etc/hosts", "127.0.0.1 localhost\n", 0o644);
    put(lower, "etc/nsswitch.conf", "passwd: compat\ngroup: compat\nhosts: files dns\n", 0o644);
    link(lower, "etc/mtab", "/proc/self/mounts");
    link(lower, "etc/resolv.conf", "../run/systemd/resolve/stub-resolv.conf");
    link(lower, "lib", "usr/lib");
    put(lower, "usr/lib/x86_64-linux-gnu/libc.so.6", "libc\n", 0o755);
    put(lower, "usr/bin/openssl", "openssl\n", 0o755);
    put(lower, "etc/apt/sources.list", "deb http://deb.debian.org/debian bookworm main\n", 0o644);
    put(lower, "etc/apt/sources.list.d/a.list", "deb http://deb.debian.org/debian bookworm-updates main\n", 0o644);
    std::fs::set_permissions(lower.join("etc/apt"), std::fs::Permissions::from_mode(0o750)).unwrap();
    put(lower, "var/lib/ca/pem/ca-certificates.crt", PEM, 0o644);
    put(lower, "var/lib/ca/pem/image-root.pem", PEM, 0o644);
    link(lower, "var/lib/ca/pem/abcdef01.0", "image-root.pem");
    link(lower, "etc/ssl/certs", "/var/lib/ca/pem");
    std::fs::set_permissions(lower.join("etc/ssl"), std::fs::Permissions::from_mode(0o751)).unwrap();
    link(lower, "dev/null", "/nowhere");
}

fn settings(d: &Scratch, originals: &str) -> Settings {
    std::fs::write(d.at("resolv.conf"), "nameserver 10.0.0.53\n").unwrap();
    std::fs::write(d.at("mounts"), "/dev/root / ext4 rw 0 0\n").unwrap();
    Settings {
        container_name: Some("box".to_string()),
        add_hosts: vec!["svc:10.0.0.7".to_string()],
        host_bundle: Some(HostBundle {
            path: "/host/ca.pem".to_string(),
            announced_by: Some("SSL_CERT_FILE".to_string()),
            bytes: HOST.as_bytes().to_vec(),
        }),
        host_resolv_conf: d.at("resolv.conf"),
        mounts: d.at("mounts"),
        originals: Some(d.at(originals)),
        ..Settings::default()
    }
}

/// A copy of `from` over `to`: directories with their modes, files with
/// their bytes and modes, links as links.
fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let p = e.unwrap().path();
        let q = to.join(p.file_name().unwrap());
        let m = std::fs::symlink_metadata(&p).unwrap();
        if m.file_type().is_symlink() {
            let _ = std::fs::remove_file(&q);
            symlink(std::fs::read_link(&p).unwrap(), &q).unwrap();
        } else if m.is_dir() {
            copy(&p, &q);
            std::fs::set_permissions(&q, m.permissions()).unwrap();
        } else {
            let _ = std::fs::remove_file(&q);
            std::fs::copy(&p, &q).unwrap();
            std::fs::set_permissions(&q, m.permissions()).unwrap();
        }
    }
}

/// The digest with the bytes of the random shims left out: each run draws
/// new ones.
fn comparable(mut m: BTreeMap<String, String>) -> BTreeMap<String, String> {
    for k in ["dev/random", "dev/urandom"] {
        if let Some(v) = m.get_mut(k)
            && let Some(i) = v.find(" file ")
        {
            v.truncate(i);
        }
    }
    m
}

/// The rows of a report, less the details (a detail names a path).
fn rows(r: &Report) -> Vec<String> {
    let mut out: Vec<String> =
        r.fixups.iter().map(|f| format!("{} {} {} {}", f.id, f.path, f.action.word(), f.degraded)).collect();
    out.extend(r.steps.iter().map(|s| format!("step {} {:?}", s.id, s.argv)));
    out.push(format!("{:?}", r.libc));
    out
}

/// The full completion of two layers: the merged view is the tree that the
/// completion of one root gives, the reports agree, the lower layer keeps
/// its digest, the upper layer holds no whiteout and no unchanged copy, and
/// a second run changes nothing. The upper layer holds a source of an
/// earlier run, so a listing of both layers is part of it.
#[test]
fn two_layers_complete_as_one_root_and_the_lower_layer_keeps_its_digest() {
    if crate::testing::no_in_root() {
        return;
    }
    let (d, lower, upper) = two("l-complete");
    image(&lower);
    put(&upper, "etc/apt/sources.list.d/b.list", "deb http://deb.debian.org/debian bookworm-backports main\n", 0o644);
    let single = d.at("single");
    copy(&lower, &single);
    copy(&upper, &single);
    let before = digest(&lower, true);

    let one = complete(&single, &settings(&d, "orig-one"), &mut Fixed(true)).unwrap();
    let s2 = Settings { upper: Some(upper.clone()), ..settings(&d, "orig-two") };
    let two_layers = complete(&lower, &s2, &mut Fixed(true)).unwrap();

    assert_eq!(digest(&lower, true), before, "the completion changed the lower layer");
    assert_eq!(rows(&two_layers), rows(&one));
    assert_eq!(two_layers.failed(), 0, "{two_layers:#?}");
    let up = digest(&upper, false);
    let low = digest(&lower, false);
    for (path, line) in &up {
        assert!(!line.contains("node 20000 0"), "{path}: a whiteout in the upper layer");
        if line.contains(" file ") {
            assert_ne!(low.get(path), Some(line), "{path}: copied up with no change");
        }
    }
    let mut merged = low;
    merged.extend(up);
    let (want, have) = (comparable(digest(&single, false)), comparable(merged));
    for (path, w) in &want {
        assert_eq!(have.get(path), Some(w), "{path}: the merged view differs from the one-root completion");
    }
    for path in have.keys() {
        assert!(want.contains_key(path), "{path}: in the merged view, not in the one-root completion");
    }
    // Each step that the ask names changed the merged view.
    for (path, what) in [
        ("etc/passwd", "644"),
        ("etc/group", "file"),
        ("etc/hosts", "svc"),
        ("var/lib/ca/pem/ca-certificates.crt", "HOST"),
        ("etc/apt/sources.list.d/a.list", "https"),
        ("etc/apt/sources.list.d/b.list", "https"),
        ("etc/mtab", "ext4"),
        ("dev/fd", "proc"),
        ("etc/apt/apt.conf.d/99pod-rootfs", "Sandbox"),
    ] {
        assert!(have[path].contains(what), "{path}: {}", have[path]);
    }

    let again = complete(&lower, &s2, &mut Fixed(true)).unwrap();
    assert_eq!(again.changed(), 0, "{again:#?}");
    assert_eq!(digest(&lower, true), before, "the second completion changed the lower layer");
}
