//! The acceptance test of podcrates@T-567 (PodUtils/podvm#326, PodUtils/podcrates#146):
//! the completion of a root of two layers.
//!
//! The oracle is the completion of one layer, which exists today: a full
//! copy of the image, completed with `pod_rootfs::complete`. The same image
//! as a lower layer, with an empty upper layer, completed in the two-layer
//! form, must give:
//!
//! 1. a merged view (`upper` over `lower`) that is the same as the copy
//!    completed with one layer: each path, its type, mode, size, link
//!    target and content;
//! 2. a lower layer that is the same, byte for byte, before and after;
//! 3. an upper layer that holds only what the completion changed or made:
//!    no regular file that is the same as the lower one.
//!
//! The settings turn on most steps: a container name and more hosts, a host
//! CA bundle that a variable announced, an `http://` package source with a
//! mirror that speaks HTTPS, a resolver and a mount table of the test.
//!
//! ⚠ One adapter: [`complete_layers`] calls the API that T-567 adds. Each
//! other line is the acceptance and stays as it is.
#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use pod_rootfs::reach::Fixed;
use pod_rootfs::{HostBundle, Report, Settings};

/// ⚠ THE ADAPTER. Complete the root of the two layers `lower` (read-only)
/// and `upper` (each write). Wire it to the call of T-567.
fn complete_layers(lower: &Path, upper: &Path, s: &Settings) -> Result<Report, String> {
    let s = Settings { upper: Some(upper.to_path_buf()), ..s.clone() };
    pod_rootfs::complete(lower, &s, &mut Fixed(true)).map_err(|e| e.to_string())
}

/// A scratch directory of this test.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("t567-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn write(root: &Path, rel: &str, body: &str, mode: u32) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, body).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(mode)).unwrap();
}

/// The image of the test: a Debian-like tree with users, hosts, a trust
/// store, an `http://` source, a libc marker and a link.
fn image(lower: &Path) {
    write(lower, "etc/passwd", "root:x:0:0:root:/root:/bin/sh\nada:x:1000:1000::/home/ada:/bin/sh\n", 0o644);
    write(lower, "etc/group", "root:x:0:\nada:x:1000:\n", 0o644);
    write(lower, "etc/hosts", "127.0.0.1 localhost\n", 0o644);
    write(lower, "etc/os-release", "ID=debian\nVERSION_ID=12\n", 0o644);
    write(lower, "etc/apt/sources.list", "deb http://deb.debian.org/debian bookworm main\n", 0o644);
    write(
        lower,
        "etc/ssl/certs/ca-certificates.crt",
        "-----BEGIN CERTIFICATE-----\nIMAGE\n-----END CERTIFICATE-----\n",
        0o644,
    );
    write(
        lower,
        "etc/ssl/certs/image-root.pem",
        "-----BEGIN CERTIFICATE-----\nIMAGE\n-----END CERTIFICATE-----\n",
        0o644,
    );
    write(lower, "lib/x86_64-linux-gnu/libc.so.6", "not a real libc\n", 0o755);
    write(lower, "usr/share/doc/keep", "only read\n", 0o444);
    fs::create_dir_all(lower.join("dev")).unwrap();
    fs::create_dir_all(lower.join("tmp")).unwrap();
    symlink("../usr/share/doc/keep", lower.join("etc/keep-link")).unwrap();
}

/// The settings of both completions: the same, but for the directory of the
/// originals, which each run keeps outside its root.
fn settings(work: &Path, originals: &str) -> Settings {
    write(work, "host/resolv.conf", "nameserver 10.0.0.53\n", 0o644);
    write(work, "host/mounts", "/dev/root / ext4 rw 0 0\n", 0o644);
    fs::create_dir_all(work.join(originals)).unwrap();
    Settings {
        container_name: Some("t567".to_string()),
        add_hosts: vec!["svc:10.0.0.7".to_string()],
        host_bundle: Some(HostBundle {
            path: "/host/ca.pem".to_string(),
            announced_by: Some("SSL_CERT_FILE".to_string()),
            bytes: b"-----BEGIN CERTIFICATE-----\nHOST\n-----END CERTIFICATE-----\n".to_vec(),
        }),
        host_resolv_conf: work.join("host/resolv.conf"),
        mounts: work.join("host/mounts"),
        originals: Some(work.join(originals)),
        steps: false,
        ..Settings::default()
    }
}

/// A copy of `from` at `to`: directories with their modes, files with
/// their bytes and modes, links as links.
fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let p = e.unwrap().path();
        let q = to.join(p.file_name().unwrap());
        let m = fs::symlink_metadata(&p).unwrap();
        if m.file_type().is_symlink() {
            symlink(fs::read_link(&p).unwrap(), &q).unwrap();
        } else if m.is_dir() {
            copy(&p, &q);
            fs::set_permissions(&q, m.permissions()).unwrap();
        } else {
            fs::copy(&p, &q).unwrap();
            fs::set_permissions(&q, m.permissions()).unwrap();
        }
    }
}

/// Each path under `root`: its type, mode, size, link target and the bytes
/// of a regular file.
fn digest(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            let m = fs::symlink_metadata(&p).unwrap();
            let rel = p.strip_prefix(root).unwrap().display().to_string();
            let kind = if m.file_type().is_symlink() {
                format!("link {}", fs::read_link(&p).unwrap().display())
            } else if m.is_dir() {
                stack.push(p.clone());
                "dir".to_string()
            } else if m.is_file() {
                format!("file {}", String::from_utf8_lossy(&fs::read(&p).unwrap()))
            } else {
                format!("node {:o}", m.mode() & 0o170000)
            };
            let size = if m.is_dir() { 0 } else { m.size() };
            out.insert(rel, format!("{:o} {size} {kind}", m.mode() & 0o7777));
        }
    }
    out
}

/// The merged view: each path of `upper` over the same path of `lower`.
/// T-567 refuses a remove of a lower entry, so there is no whiteout to read.
fn merged(lower: &Path, upper: &Path) -> BTreeMap<String, String> {
    let mut m = digest(lower);
    m.extend(digest(upper));
    m
}

/// The three runs of one fixture: the one-layer oracle, the two layers.
struct Runs {
    d: PathBuf,
    lower: PathBuf,
    upper: PathBuf,
    single: PathBuf,
    lower_before: BTreeMap<String, String>,
}

fn runs(name: &str) -> Result<Runs, String> {
    let d = scratch(name);
    let (lower, upper, single) = (d.join("lower"), d.join("upper"), d.join("single"));
    image(&lower);
    copy(&lower, &single);
    fs::create_dir_all(&upper).unwrap();
    let lower_before = digest(&lower);
    pod_rootfs::complete(&single, &settings(&d, "orig-one"), &mut Fixed(true)).expect("the one-layer completion");
    complete_layers(&lower, &upper, &settings(&d, "orig-two"))?;
    Ok(Runs { d, lower, upper, single, lower_before })
}

fn wired(r: Result<Runs, String>) -> Runs {
    match r {
        Ok(r) => r,
        Err(why) => panic!("{why}"),
    }
}

#[test]
fn two_layers_give_the_root_of_one_layer() {
    let r = wired(runs("same"));
    let (want, have) = (digest(&r.single), merged(&r.lower, &r.upper));
    // A host that refuses mknod(2) gets shims of dev/random, and each run
    // draws new bytes for them: the two roots cannot be the same there.
    if want.get("dev/random").is_some_and(|e| e.contains(" file ")) {
        eprintln!("SKIP: mknod(2) is refused here, and the shims of dev/random hold the random bytes of each run");
        let _ = fs::remove_dir_all(&r.d);
        return;
    }
    for (path, w) in &want {
        assert_eq!(have.get(path), Some(w), "{path}: the merged view differs from the one-layer root");
    }
    for path in have.keys() {
        assert!(want.contains_key(path), "{path}: in the merged view, not in the one-layer root");
    }
    let _ = fs::remove_dir_all(&r.d);
}

#[test]
fn the_lower_layer_stays_the_same() {
    let r = wired(runs("lower"));
    assert_eq!(digest(&r.lower), r.lower_before, "the completion changed the lower layer");
    let _ = fs::remove_dir_all(&r.d);
}

#[test]
fn the_upper_layer_holds_only_what_the_completion_changed() {
    let r = wired(runs("upper"));
    let lower = digest(&r.lower);
    for (path, entry) in digest(&r.upper) {
        if entry.contains(" dir") || entry.ends_with("dir") {
            continue; // a parent of a copy-up
        }
        assert_ne!(lower.get(&path), Some(&entry), "{path}: copied up with no change");
    }
    let _ = fs::remove_dir_all(&r.d);
}
