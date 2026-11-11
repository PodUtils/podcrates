use std::path::{Path, PathBuf};

use super::*;
use crate::userland::{lib_dirs, takes_inhibit_cache};

fn image(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pod-enter-ldconf-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("etc/ld.so.conf.d")).unwrap();
    d
}

fn write(root: &Path, guest: &str, text: &str) {
    let host = root.join(guest.trim_start_matches('/'));
    std::fs::create_dir_all(host.parent().unwrap()).unwrap();
    std::fs::write(host, text).unwrap();
}

fn root_str(d: &Path) -> String {
    d.to_string_lossy().into_owned()
}

/// glibc's format: comments, `hwcap`, an old `DIR=TYPE`, a relative and an
/// absolute `include` with a glob, sorted, and each directory once.
#[test]
fn the_glibc_configuration_of_the_image_names_its_directories() {
    let d = image("glibc");
    write(&d, "/etc/ld.so.conf", "# the image's own\ninclude ld.so.conf.d/*.conf\n/opt/app/lib\nhwcap 0 x\n");
    write(&d, "/etc/ld.so.conf.d/b.conf", "/usr/local/lib/b # a comment\n/opt/app/lib\n");
    write(&d, "/etc/ld.so.conf.d/a.conf", "/usr/local/lib/a=libc6\ninclude /etc/extra/*.conf\n");
    write(&d, "/etc/ld.so.conf.d/c.txt", "/not/a/conf\n");
    write(&d, "/etc/ld.so.conf.d/.hidden.conf", "/hidden\n");
    write(&d, "/etc/extra/x.conf", "/srv/x, /srv/y:/srv/z\n");
    let dirs = configured_dirs(&root_str(&d), "/lib64/ld-linux-x86-64.so.2");
    assert_eq!(dirs, ["/usr/local/lib/a", "/srv/x", "/srv/y", "/srv/z", "/usr/local/lib/b", "/opt/app/lib"]);
    let _ = std::fs::remove_dir_all(&d);
}

/// musl's path file: one directory on each line or between colons.
#[test]
fn the_musl_path_file_of_the_image_names_its_directories() {
    let d = image("musl");
    write(&d, "/etc/ld-musl-x86_64.path", "/lib:/usr/local/lib\n/opt/lib\n");
    write(&d, "/etc/ld.so.conf", "/glibc/only\n");
    let dirs = configured_dirs(&root_str(&d), "/lib/ld-musl-x86_64.so.1");
    assert_eq!(dirs, ["/lib", "/usr/local/lib", "/opt/lib"]);
    let _ = std::fs::remove_dir_all(&d);
}

/// A configuration file that links out of the image reads the image's side
/// of the link, never the host's file.
#[test]
fn a_configuration_that_links_out_of_the_image_reads_nothing_of_the_host() {
    let d = image("link");
    let host_file = std::env::temp_dir().join(format!("pod-enter-ldconf-host-{}.conf", std::process::id()));
    std::fs::write(&host_file, "/host/only\n").unwrap();
    std::os::unix::fs::symlink(&host_file, d.join("etc/ld.so.conf")).unwrap();
    assert!(configured_dirs(&root_str(&d), "/lib64/ld-linux-x86-64.so.2").is_empty());
    let _ = std::fs::remove_file(&host_file);
    let _ = std::fs::remove_dir_all(&d);
}

/// A file that includes itself ends at the bound of the depth, and a
/// directory comes once.
#[test]
fn an_include_of_itself_ends_at_the_bound() {
    let d = image("loop");
    write(&d, "/etc/ld.so.conf", "include /etc/ld.so.conf\n/opt/lib\n");
    assert_eq!(configured_dirs(&root_str(&d), "/lib/ld-linux-aarch64.so.1"), ["/opt/lib"]);
    let _ = std::fs::remove_dir_all(&d);
}

/// `lib_dirs` resolves each directory in the image: `/lib` that links to
/// the absolute `/usr/lib` names the image's `usr/lib`, `/usr/local/lib`
/// counts, and a configured directory counts where it exists.
#[test]
fn lib_dirs_resolve_in_the_image_and_take_usr_local_and_the_configuration() {
    let d = image("dirs");
    std::fs::create_dir_all(d.join("usr/lib")).unwrap();
    std::fs::create_dir_all(d.join("usr/local/lib")).unwrap();
    std::fs::create_dir_all(d.join("opt/app/lib")).unwrap();
    std::os::unix::fs::symlink("/usr/lib", d.join("lib")).unwrap();
    write(&d, "/etc/ld-musl-x86_64.path", "/opt/app/lib\n/absent\n");
    let root = root_str(&d);
    let dirs = lib_dirs(&root, "/lib/ld-musl-x86_64.so.1");
    assert_eq!(dirs, [format!("{root}/usr/lib"), format!("{root}/usr/local/lib"), format!("{root}/opt/app/lib")]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_glob_matches_as_fnmatch_does_for_star_and_question_mark() {
    for (p, n, want) in [
        ("*.conf", "a.conf", true),
        ("*.conf", "a.conf.bak", false),
        ("x?.conf", "x1.conf", true),
        ("x?.conf", "x.conf", false),
        ("*", "", true),
        ("a*b*c", "aXbYc", true),
        ("a*b*c", "aXbY", false),
    ] {
        assert_eq!(matches(p.as_bytes(), n.as_bytes()), want, "{p} {n}");
    }
}

/// A glibc loader with a libc of 2.16 or later takes `--inhibit-cache`; a
/// musl loader and a loader with no libc beside it do not. The case of
/// glibc runs where this host has a glibc of its own to copy.
#[test]
fn inhibit_cache_needs_a_glibc_of_2_16_or_later() {
    let d = image("cache");
    let root = root_str(&d);
    assert!(!takes_inhibit_cache(&root, "/lib/ld-musl-x86_64.so.1", "/nonexistent"));
    assert!(!takes_inhibit_cache(&root, "/lib64/ld-linux-x86-64.so.2", "/nonexistent"));
    let (loader, libc) = (Path::new("/lib64/ld-linux-x86-64.so.2"), Path::new("/lib/x86_64-linux-gnu/libc.so.6"));
    if !(loader.is_file() && libc.is_file()) {
        pod_testkit::host::skip_line("this host has no x86_64 glibc to copy");
        let _ = std::fs::remove_dir_all(&d);
        return;
    }
    std::fs::create_dir_all(d.join("lib64")).unwrap();
    std::fs::copy(loader, d.join("lib64/ld-linux-x86-64.so.2")).unwrap();
    std::fs::copy(libc, d.join("lib64/libc.so.6")).unwrap();
    let loader_host = format!("{root}/lib64/ld-linux-x86-64.so.2");
    assert!(takes_inhibit_cache(&root, "/lib64/ld-linux-x86-64.so.2", &loader_host));
    let _ = std::fs::remove_dir_all(&d);
}
