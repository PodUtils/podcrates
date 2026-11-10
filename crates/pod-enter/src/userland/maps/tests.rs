use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use super::*;
use crate::abi::{ResolveKind, resolve_dir_in, resolve_in};
use crate::userland::{invocation_path, loader_plan};
use crate::{Fds, Plan, RootDir, check_workdir, ladder, spawn_userland};

const N: Names = Names::DEFAULT;

fn vol(host: &str, guest: &str) -> Volume {
    Volume { guest: guest.to_string(), host: host.to_string() }
}

/// A host tree for one test: `rootfs/` (the image), `vol/` and
/// `vol/sub/inner/` (the sources of two volumes), and `secret` beside
/// them, which no guest path may reach.
fn tree(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pod-enter-maps-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    for dir in ["rootfs/bin", "rootfs/lib", "rootfs/etc", "rootfs/work", "vol/bin", "vol/work", "vol/sub/inner/bin"] {
        std::fs::create_dir_all(d.join(dir)).unwrap();
    }
    std::fs::write(d.join("secret"), b"host").unwrap();
    std::fs::write(d.join("rootfs/etc/marker"), b"image").unwrap();
    for f in ["rootfs/bin/prog", "vol/bin/prog", "vol/sub/inner/bin/prog"] {
        std::fs::write(d.join(f), b"\x7fELF").unwrap();
        std::fs::set_permissions(d.join(f), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    d
}

fn s(p: &Path) -> String {
    p.to_str().unwrap().to_string()
}

/// The table of `tree`: the volume `vol` at `/data`, and `vol/sub/inner`
/// at `/data/sub`, a longer guest prefix inside the first volume.
fn maps(d: &Path) -> Maps {
    Maps::new(
        &s(&d.join("rootfs")),
        &[vol(&s(&d.join("vol")), "/data/"), vol(&s(&d.join("vol/sub/inner")), "/data/sub")],
    )
    .unwrap()
}

/// The root pair comes first, then one pair for each volume, each side
/// in its normal form; the volume table has no root pair.
#[test]
fn the_table_holds_the_root_pair_then_each_volume() {
    let m = Maps::new("/store/rootfs/", &[vol("/host/dir/", "/data/"), vol("/", "/hostroot")]).unwrap();
    assert_eq!(userland_maps_table(&N, &m).as_deref(), Ok("/:/store/rootfs,/data:/host/dir,/hostroot:/"));
    assert_eq!(userland_volume_table(&N, &m).as_deref(), Ok("/data:/host/dir,/hostroot:/"));
    assert_eq!(userland_maps_table(&N, &Maps::of_root("/store/rootfs")).as_deref(), Ok("/:/store/rootfs"));
    assert_eq!(userland_volume_table(&N, &Maps::of_root("/store/rootfs")).as_deref(), Ok(""));
}

/// Each side of a volume that the table cannot spell is refused with the
/// name of that side, not with the name of the root.
#[test]
fn each_volume_side_is_refused_by_its_own_name() {
    let long = format!("/{}", "h".repeat(MAX_MAP_SIDE));
    let table = |v: Volume| userland_maps_table(&N, &Maps::new("/r", &[v]).unwrap()).unwrap_err();
    let e = table(vol(&long, "/data"));
    assert!(e.contains("the host side is 4097 bytes, over the 4096-byte table side"), "{e}");
    assert!(e.contains("the volume /hhh") && e.contains("...:/data"), "{e}");
    assert!(!e.contains("rootfs"), "{e}");
    let e = table(vol("/a", &format!("/{}", "g".repeat(MAX_MAP_SIDE))));
    assert!(e.contains("the guest side is 4097 bytes"), "{e}");
    let e = table(vol("/a,b", "/data"));
    assert!(e.contains("the host side \"/a,b\" cannot be spelled"), "{e}");
    let e = table(vol("/a", "/d:e"));
    assert!(e.contains("the guest side \"/d:e\" cannot be spelled"), "{e}");
    let many: Vec<Volume> = (0..MAX_MAP_PAIRS).map(|i| vol("/a", &format!("/v{i}"))).collect();
    let e = userland_maps_table(&N, &Maps::new("/r", &many).unwrap()).unwrap_err();
    assert!(e.contains("33 pairs, over the 32"), "{e}");
    assert!(userland_maps_table(&N, &Maps::new("/r", &many[1..]).unwrap()).is_ok());
    assert!(userland_volume_table(&N, &Maps::new("/r", &many).unwrap()).is_ok());
}

/// A volume that no table could serve is refused when the table is made.
#[test]
fn a_volume_the_resolver_cannot_match_is_refused() {
    let refused = |host: &str, guest: &str, want: &str| {
        let e = Maps::new("/r", &[vol(host, guest)]).unwrap_err();
        assert!(e.contains(want), "{host}:{guest}: {e}");
    };
    refused("/a", "data", "the guest side must be an absolute path");
    refused("/a", "/", "cannot replace the guest root");
    refused("/a", "/x/../etc", "normal path");
    refused("/a", "/x//y", "normal path");
    refused("/a", "/./y", "normal path");
    refused("a", "/data", "the host side must be an absolute path");
    refused("/a\0", "/data", "NUL");
    let e = Maps::new("/r", &[vol("/a", "/data"), vol("/b", "/data/")]).unwrap_err();
    assert!(e.contains("a second volume at the guest path /data"), "{e}");
}

/// The longest guest prefix wins, at a component boundary only, as the
/// interposer matches it.
#[test]
fn the_longest_guest_prefix_serves_a_path() {
    let d = tree("longest");
    let m = maps(&d);
    assert_eq!(m.volume_at("/data/sub/bin/prog").map(|v| v.guest.as_str()), Some("/data/sub"));
    assert_eq!(m.volume_at("/data/bin/prog").map(|v| v.guest.as_str()), Some("/data"));
    assert_eq!(m.volume_at("/database"), None);
    assert_eq!(m.volume_at("/bin/prog"), None);
    assert_eq!(resolve_in(&m, "/data/sub/bin/prog"), Ok(d.join("vol/sub/inner/bin/prog")));
    assert_eq!(resolve_in(&m, "/data/bin/prog"), Ok(d.join("vol/bin/prog")));
    assert_eq!(resolve_in(&m, "/bin/prog"), Ok(d.join("rootfs/bin/prog")));
    let _ = std::fs::remove_dir_all(&d);
}

/// A working directory in a volume passes the check through the table,
/// and fails it through the root alone: the image has no `/data`.
#[test]
fn a_workdir_in_a_volume_resolves_to_the_volume() {
    let d = tree("workdir");
    let m = maps(&d);
    assert!(check_workdir(&m, "/data/work", "img").is_ok());
    assert_eq!(resolve_dir_in(&m, "/data/work"), Ok(d.join("vol/work")));
    assert_eq!(resolve_dir_in(&m, "/data/sub"), Ok(d.join("vol/sub/inner")));
    assert!(check_workdir(m.rootfs(), "/data/work", "img").is_err());
    // The root pair alone is the old check.
    assert!(check_workdir(&Maps::of_root(m.rootfs()), "/work", "img").is_ok());
    let _ = std::fs::remove_dir_all(&d);
}

/// The first program in a volume is the file of the volume: by path, along
/// `PATH`, for the head of the decision, for the guest path and for the
/// loader.
#[test]
fn a_first_program_in_a_volume_resolves_to_the_volume() {
    let d = tree("program");
    let m = maps(&d);
    let path = ["/data/bin".to_string()];
    let host = s(&d.join("vol/bin/prog"));
    assert_eq!(ladder::resolve_payload(&m, "/data/bin/prog", &[]).unwrap(), host);
    assert_eq!(ladder::resolve_payload(&m, "prog", &path).unwrap(), host);
    assert_eq!(ladder::payload_head(&m, "prog", &path).unwrap().path, host);
    assert_eq!(ladder::guest_exe(&m, "prog", &path).as_deref(), Some("/data/bin/prog"));
    assert_eq!(ladder::guest_invoked(&m, "prog", &path).as_deref(), Some("/data/bin/prog"));
    assert_eq!(invocation_path(&m, "prog", &path, &host), host);
    // A loader in a volume resolves there too.
    let plan = loader_plan(&m, "/data/bin/prog", &host).unwrap();
    assert_eq!(plan.loader_host, host);
    // Through the root alone the program is absent.
    assert!(ladder::resolve_payload(m.rootfs(), "/data/bin/prog", &[]).is_err());
    let _ = std::fs::remove_dir_all(&d);
}

/// A link in a volume that points out of it never reaches the host tree
/// beside the volume: a relative target past the volume root goes up the
/// guest path (into the image), an absolute target is a guest path (into
/// the image or another volume), and a target past the guest root is an
/// escape, as for the root alone.
#[test]
fn a_link_out_of_a_volume_is_held_inside_the_table() {
    let d = tree("escape");
    symlink("../secret", d.join("vol/up")).unwrap();
    symlink("../../../../../../secret", d.join("vol/far")).unwrap();
    symlink("/etc/marker", d.join("vol/abs")).unwrap();
    symlink("/data/bin", d.join("vol/sub/inner/back")).unwrap();
    symlink("..", d.join("vol/parent")).unwrap();
    let m = maps(&d);
    // `vol/../secret` is a host file; `/data/../secret` is the guest
    // `/secret`, which the image does not hold.
    assert_eq!(resolve_in(&m, "/data/up"), Err(ResolveKind::Absent));
    assert_eq!(resolve_in(&m, "/data/far"), Err(ResolveKind::Escapes));
    assert_eq!(resolve_in(&m, "/data/abs"), Ok(d.join("rootfs/etc/marker")));
    assert_eq!(resolve_dir_in(&m, "/data/sub/back"), Ok(d.join("vol/bin")));
    assert_eq!(resolve_dir_in(&m, "/data/parent"), Ok(d.join("rootfs")));
    assert!(check_workdir(&m, "/data/parent/data/work", "img").is_ok());
    let _ = std::fs::remove_dir_all(&d);
}

/// The entry with no chroot starts the payload in the volume that serves
/// its working directory, and in the image where the root pair serves it.
#[test]
fn the_userland_entry_enters_a_workdir_in_a_volume() {
    pod_testkit::need_tool!("/bin/sh");
    let d = tree("spawn");
    let m = maps(&d);
    let root = RootDir::open(m.rootfs()).unwrap();
    for (wd, want) in [("/data/work", d.join("vol/work")), ("/work", d.join("rootfs/work"))] {
        let out = d.join("pwd.txt");
        let plan = Plan {
            argv: vec!["sh".to_string()],
            env: vec!["PATH=/bin:/usr/bin".to_string()],
            working_dir: wd.to_string(),
            fds: Fds::default(),
            banner: String::new(),
            path_dirs: vec!["/bin".to_string()],
            names: N,
        };
        let argv = vec!["/bin/sh".to_string(), "-c".to_string(), format!("pwd -P > {}", s(&out))];
        let child = spawn_userland(&root, &plan, &m, argv, "userland", None, &mut std::io::sink()).unwrap();
        assert_eq!(child.wait().unwrap(), 0);
        let got = std::fs::read_to_string(&out).unwrap();
        assert_eq!(got.trim_end(), s(&std::fs::canonicalize(&want).unwrap()), "{wd}");
    }
    // A table of another root is refused before the fork.
    let plan = Plan {
        argv: vec!["sh".to_string()],
        env: Vec::new(),
        working_dir: "/".to_string(),
        fds: Fds::default(),
        banner: String::new(),
        path_dirs: vec!["/bin".to_string()],
        names: N,
    };
    let other = Maps::of_root("/elsewhere");
    let e = spawn_userland(&root, &plan, &other, vec!["/bin/sh".to_string()], "userland", None, &mut std::io::sink())
        .unwrap_err();
    assert!(format!("{e}").contains("the table of path maps has the root"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}
