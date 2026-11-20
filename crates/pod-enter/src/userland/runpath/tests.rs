//! The table test of the `RUNPATH` directories of the loader family
//! (T-976, PodUtils/podvm#603): an image in a temporary directory, with
//! objects whose `.dynamic` the test writes.

use std::path::{Path, PathBuf};

use super::*;
use crate::abi::tests::dynamic::dyn_object;
use crate::userland::{Maps, Volume, lib_dirs, lib_dirs_for, loader_plan};

const X86_64: u16 = 62;
const AARCH64: u16 = 183;
const LOADER: &str = "/lib64/ld-linux-x86-64.so.2";

/// An image with an executable loader, `/lib` and `/usr/lib`.
fn image(tag: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let d = std::env::temp_dir().join(format!("pod-enter-runpath-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    for dir in ["lib", "lib64", "usr/lib", "usr/bin"] {
        std::fs::create_dir_all(d.join(dir)).unwrap();
    }
    put(&d, LOADER, &dyn_object(X86_64, &[], None, None));
    std::fs::set_permissions(d.join(&LOADER[1..]), std::fs::Permissions::from_mode(0o755)).unwrap();
    d
}

/// Write `bytes` at the guest path `guest` of the image `d`.
fn put(d: &Path, guest: &str, bytes: &[u8]) {
    let p = d.join(guest.trim_start_matches('/'));
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

fn host(d: &Path, guest: &str) -> String {
    d.join(guest.trim_start_matches('/')).display().to_string()
}

/// `RUNPATH=$ORIGIN/../lib/x`, with the library only there: the directory
/// of the image leads the library path of the loader, before the
/// directory of the loader and the standard ones.
#[test]
fn a_runpath_of_origin_leads_the_library_path() {
    let d = image("origin");
    put(&d, "/opt/app/bin/prog", &dyn_object(X86_64, &["libx.so.1", "libc.so.6"], Some("$ORIGIN/../lib/x"), None));
    put(&d, "/opt/app/lib/x/libx.so.1", &dyn_object(X86_64, &[], None, None));
    let root = d.display().to_string();
    let plan = loader_plan(&root, LOADER, &host(&d, "/opt/app/bin/prog")).unwrap();
    assert_eq!(plan.lib_dirs[0], host(&d, "/opt/app/lib/x"), "{plan:?}");
    assert_eq!(plan.lib_dirs[1..], lib_dirs(&root, LOADER)[..], "{plan:?}");
    let _ = std::fs::remove_dir_all(&d);
}

/// `systemctl` of systemd: `RUNPATH=/usr/lib/systemd` names the directory
/// of the image, never the one of the host.
#[test]
fn an_absolute_runpath_resolves_inside_the_image() {
    let d = image("systemd");
    put(&d, "/usr/bin/systemctl", &dyn_object(X86_64, &["libsystemd-shared-261.so"], Some("/usr/lib/systemd"), None));
    put(&d, "/usr/lib/systemd/libsystemd-shared-261.so", &dyn_object(X86_64, &["libc.so.6"], None, None));
    let root = d.display().to_string();
    let dirs = lib_dirs_for(&root, LOADER, "/usr/bin/systemctl");
    assert_eq!(dirs[0], host(&d, "/usr/lib/systemd"), "{dirs:?}");
    let _ = std::fs::remove_dir_all(&d);
}

/// The program first, then each library that it loads, breadth first; a
/// library of the program's `RUNPATH` gives its own `RUNPATH` too, and its
/// `$ORIGIN` is its own directory.
#[test]
fn the_runpath_of_a_library_follows_the_one_of_the_program() {
    let d = image("chain");
    put(&d, "/usr/bin/prog", &dyn_object(X86_64, &["liba.so", "libb.so"], Some("/opt/a"), None));
    put(&d, "/opt/a/liba.so", &dyn_object(X86_64, &["libc.so.6"], Some("$ORIGIN/inner"), None));
    put(&d, "/opt/a/inner/libinner.so", &dyn_object(X86_64, &[], None, None));
    // libb is in a standard directory, and names an RPATH only.
    put(&d, "/usr/lib/libb.so", &dyn_object(X86_64, &[], None, Some("/opt/b")));
    std::fs::create_dir_all(d.join("opt/b")).unwrap();
    let root = d.display().to_string();
    assert_eq!(runpath_dirs(&root, LOADER, "/usr/bin/prog"), ["/opt/a", "/opt/a/inner", "/opt/b"]);
    let _ = std::fs::remove_dir_all(&d);
}

/// A `RUNPATH` that leaves the image, a directory that the image does not
/// hold, a relative entry and the `RPATH` beside a `RUNPATH` are not added.
#[test]
fn a_runpath_that_leaves_the_image_is_not_added() {
    let d = image("escape");
    std::fs::create_dir_all(d.join("opt/rpath")).unwrap();
    let list = "$ORIGIN/../../../../../../../..:/no/such/dir:rel/dir:/usr/lib";
    put(&d, "/usr/bin/prog", &dyn_object(X86_64, &[], Some(list), Some("/opt/rpath")));
    let root = d.display().to_string();
    assert_eq!(runpath_dirs(&root, LOADER, "/usr/bin/prog"), ["/usr/lib"]);
    // With no RUNPATH, the RPATH counts.
    put(&d, "/usr/bin/old", &dyn_object(X86_64, &[], None, Some("/opt/rpath")));
    assert_eq!(runpath_dirs(&root, LOADER, "/usr/bin/old"), ["/opt/rpath"]);
    // An absolute link of the image stays in the image.
    std::os::unix::fs::symlink("/opt/rpath", d.join("opt/link")).unwrap();
    put(&d, "/usr/bin/linked", &dyn_object(X86_64, &[], Some("/opt/link"), None));
    assert_eq!(lib_dirs_for(&root, LOADER, "/usr/bin/linked")[0], host(&d, "/opt/rpath"));
    // A program that does not read gives no directory.
    assert!(runpath_dirs(&root, LOADER, "/usr/bin/absent").is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

/// The loader passes over a library of another machine, and so does the
/// walk: its `RUNPATH` is not read.
#[test]
fn a_library_of_another_machine_gives_no_runpath() {
    let d = image("machine");
    put(&d, "/usr/bin/prog", &dyn_object(X86_64, &["liba.so"], None, None));
    put(&d, "/lib64/liba.so", &dyn_object(AARCH64, &[], Some("/opt/arm"), None));
    put(&d, "/usr/lib/liba.so", &dyn_object(X86_64, &[], Some("/opt/x86"), None));
    std::fs::create_dir_all(d.join("opt/arm")).unwrap();
    std::fs::create_dir_all(d.join("opt/x86")).unwrap();
    let root = d.display().to_string();
    assert_eq!(runpath_dirs(&root, LOADER, "/usr/bin/prog"), ["/opt/x86"]);
    let _ = std::fs::remove_dir_all(&d);
}

/// A program in a volume: its `$ORIGIN` is a guest directory of the
/// volume, and the directory resolves in the host side of the volume.
#[test]
fn a_runpath_of_a_program_in_a_volume_resolves_in_the_volume() {
    let d = image("volume");
    let vol = std::env::temp_dir().join(format!("pod-enter-runpath-vol-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&vol);
    put(&vol, "/bin/tool", &dyn_object(X86_64, &["libt.so"], Some("$ORIGIN/../lib"), None));
    put(&vol, "/lib/libt.so", &dyn_object(X86_64, &[], None, None));
    let names = crate::Names::DEFAULT;
    let v = Volume { guest: "/work".to_string(), host: vol.display().to_string() };
    let maps = Maps::new(&names, &d.display().to_string(), &[v]).unwrap();
    assert_eq!(maps.guest_of(&vol.join("bin/tool")).as_deref(), Some("/work/bin/tool"));
    assert_eq!(maps.guest_of(&d.join("usr/bin")).as_deref(), Some("/usr/bin"));
    assert_eq!(maps.guest_of(Path::new("/elsewhere")), None);
    let plan = loader_plan(&maps, LOADER, &vol.join("bin/tool").display().to_string()).unwrap();
    assert_eq!(plan.lib_dirs[0], vol.join("lib").display().to_string(), "{plan:?}");
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&vol);
}

/// The tokens: `$ORIGIN` and `${ORIGIN}` for each C library; a `$` that
/// starts no token stays; musl has no `$LIB` or `$PLATFORM`; glibc's `$LIB`
/// is the directory of its C library.
#[test]
fn the_tokens_expand_as_the_loader_of_the_image_expands_them() {
    let d = image("tokens");
    let root = d.display().to_string();
    let musl = Tokens::new(&root, "/lib/ld-musl-x86_64.so.1");
    assert_eq!(musl.expand("$ORIGIN/../lib", "/a/b").as_deref(), Some("/a/b/../lib"));
    assert_eq!(musl.expand("${ORIGIN}/x", "/a").as_deref(), Some("/a/x"));
    assert_eq!(musl.expand("/x/$ORIGINAL/y", "/a").as_deref(), Some("/x/$ORIGINAL/y"));
    assert_eq!(musl.expand("/x/${ORIGIN", "/a").as_deref(), Some("/x/${ORIGIN"));
    assert_eq!(musl.expand("/x/$LIB", "/a"), None);
    assert_eq!(musl.expand("/x/${PLATFORM}", "/a"), None);
    // Debian's layout: the C library in a multiarch directory.
    put(&d, "/lib/x86_64-linux-gnu/libc.so.6", &dyn_object(X86_64, &[], None, None));
    let debian = Tokens::new(&root, LOADER);
    assert_eq!(debian.expand("/opt/$LIB/x", "/a").as_deref(), Some("/opt/lib/x86_64-linux-gnu/x"));
    // Fedora's layout: the C library beside the loader.
    put(&d, "/lib64/libc.so.6", &dyn_object(X86_64, &[], None, None));
    let fedora = Tokens::new(&root, LOADER);
    assert_eq!(fedora.expand("/opt/${LIB}", "/a").as_deref(), Some("/opt/lib64"));
    let _ = std::fs::remove_dir_all(&d);
}

/// glibc's `$PLATFORM`: `haswell` for an Intel x86-64 CPU with the seven
/// features and glibc 2.26 or later; else `AT_PLATFORM`.
#[test]
fn the_platform_of_glibc_follows_its_rule() {
    fn kernel() -> Option<String> {
        Some("x86_64".to_string())
    }
    let intel = "vendor_id\t: GenuineIntel\nflags\t\t: fpu popcnt movbe avx2 fma bmi1 bmi2 abm\n";
    let amd = "vendor_id\t: AuthenticAMD\nflags\t\t: fpu popcnt movbe avx2 fma bmi1 bmi2 abm\n";
    let old = "vendor_id\t: GenuineIntel\nflags\t\t: fpu popcnt avx2 fma bmi1 bmi2 abm\n";
    let x86 = cfg!(target_arch = "x86_64");
    let want = |w: &str| Some(if x86 { w } else { "x86_64" }.to_string());
    assert_eq!(glibc_platform(intel, true, kernel), want("haswell"));
    assert_eq!(glibc_platform(intel, false, kernel), want("x86_64"));
    assert_eq!(glibc_platform(amd, true, kernel), want("x86_64"));
    assert_eq!(glibc_platform(old, true, kernel), want("x86_64"));
    // The kernel's own string of this host, read from the auxiliary vector.
    let arch = std::env::consts::ARCH;
    if arch == "x86_64" || arch == "aarch64" {
        assert_eq!(at_platform().as_deref(), Some(arch));
    }
}

/// `$LIB` and `$PLATFORM` of the walk agree with the loader of the host,
/// read with the host as the image: `LD_DEBUG=libs` prints the search path
/// with each token expanded.
#[test]
fn the_tokens_agree_with_the_loader_of_the_host() {
    let Ok(interp) = crate::abi::interp_of_file("/bin/true") else { return };
    let Some(interp) = interp.filter(|i| Flavour::of_libc_name(i) == Flavour::Glibc) else {
        pod_testkit::host::skip_line("no glibc loader on this host");
        return;
    };
    // `$PLATFORM` is the one of this process. An emulated test binary (or
    // i686 on x86_64) runs a host `/bin/true` of another machine.
    let machine = |p: &str| crate::abi::Search::read(p).map(|s| s.machine).ok();
    let exe = std::env::current_exe().ok().and_then(|p| machine(&p.to_string_lossy()));
    if exe.is_none() || exe != machine("/bin/true") {
        pod_testkit::host::skip_line("/bin/true is of another machine than this test binary");
        return;
    }
    let out = std::process::Command::new("/bin/true")
        .env("LD_DEBUG", "libs")
        .env("LD_LIBRARY_PATH", "/pod-enter-n1/$LIB/m1:/pod-enter-n2/$PLATFORM/m2")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stderr);
    let between = |a: &str, b: &str| text.split(a).nth(1).and_then(|r| r.split(b).next()).map(str::to_string);
    let tokens = Tokens::new("/", &interp);
    let mut checked = 0;
    for (token, a, b) in [("$LIB", "/pod-enter-n1/", "/m1"), ("$PLATFORM", "/pod-enter-n2/", "/m2")] {
        // A loader that drops an entry with no value prints nothing for it.
        let Some(loader) = between(a, b) else { continue };
        assert_eq!(tokens.expand(token, "/").as_deref(), Some(loader.as_str()), "{token}: {text}");
        checked += 1;
    }
    assert!(checked > 0 || text.is_empty(), "the loader printed no search path: {text}");
}
