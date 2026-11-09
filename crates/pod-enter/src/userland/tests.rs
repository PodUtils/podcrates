use super::*;

fn tree(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("podvm-userland-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("lib")).unwrap();
    std::fs::create_dir_all(d.join("usr/lib")).unwrap();
    d
}

/// The loader's directory leads, the standard ones follow, and only
/// directories that exist are named.
#[test]
fn lib_dirs_lead_with_the_loader_and_skip_the_absent() {
    let d = tree("libs");
    let root = d.to_string_lossy().to_string();
    let dirs = lib_dirs(&root, "/lib64/ld-linux-x86-64.so.2");
    assert_eq!(
        dirs,
        vec![format!("{root}/lib"), format!("{root}/usr/lib"),],
        "{dirs:?}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// A loader that is not executable by host path refuses naming it,
/// rather than failing inside the loader later.
#[test]
fn a_missing_loader_is_a_sentence() {
    let d = tree("noloader");
    let root = d.to_string_lossy().to_string();
    let e = loader_plan(&root, "/lib/ld-musl-x86_64.so.1", &format!("{root}/bin/sh")).unwrap_err();
    assert!(e.contains("not an executable file"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

/// An executable loader with library directories resolves whole.
#[test]
fn an_executable_loader_resolves_whole() {
    let d = tree("loader");
    std::fs::write(d.join("lib/ld.so"), b"\x7fELF").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(d.join("lib/ld.so"))
            .unwrap()
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(d.join("lib/ld.so"), perms).unwrap();
    }
    let root = d.to_string_lossy().to_string();
    let plan = loader_plan(&root, "/lib/ld.so", &format!("{root}/bin/sh")).unwrap();
    assert_eq!(plan.loader_host, format!("{root}/lib/ld.so"));
    assert!(plan.lib_dirs.contains(&format!("{root}/lib")), "{plan:?}");
    let _ = std::fs::remove_dir_all(&d);
}

/// TODO/enter.md T-1408: Debian's `/lib64/ld-linux-*.so.2` is an
/// absolute symlink whose target exists only in the image. The naive
/// host join dangles there, so the loader must resolve inside the
/// rootfs first: the resolved file is what opens, and the refusal where
/// it still fails names the guest path.
#[test]
#[cfg(unix)]
fn an_absolute_link_loader_resolves_inside_the_rootfs() {
    let d = tree("absloader");
    std::fs::write(d.join("lib/real-ld.so"), b"\x7fELF").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(d.join("lib/real-ld.so"))
            .unwrap()
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(d.join("lib/real-ld.so"), perms).unwrap();
    }
    std::fs::create_dir_all(d.join("lib64")).unwrap();
    // Absolute as the guest kernel reads it: under the root it names the
    // real loader, on the host it dangles.
    std::os::unix::fs::symlink("/lib/real-ld.so", d.join("lib64/ld-linux-x86-64.so.2")).unwrap();
    let root = d.to_string_lossy().to_string();
    let plan = loader_plan(
        &root,
        "/lib64/ld-linux-x86-64.so.2",
        &format!("{root}/bin/sh"),
    )
    .unwrap();
    assert_eq!(
        plan.loader_host,
        format!("{root}/lib/real-ld.so"),
        "{plan:?}"
    );
    // And the refusal where nothing resolves still names the guest path.
    let e = loader_plan(&root, "/lib64/absent-ld.so.2", &format!("{root}/bin/sh")).unwrap_err();
    assert!(e.contains("/lib64/absent-ld.so.2"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

/// The guest preload path becomes the host path; the library path is
/// set; a caller value loses to the image's.
#[test]
fn host_env_rewrites_preload_and_sets_lib_path() {
    let env = [
        "LD_PRELOAD=/.podvm/interpose.so /mine.so".to_string(),
        "LD_LIBRARY_PATH=/caller".to_string(),
        "PATH=/bin".to_string(),
    ];
    let (got, dropped) = host_env(
        "/store/rootfs",
        "/.podvm/interpose.so",
        &env,
        &["/store/rootfs/lib".to_string()],
    );
    assert!(
        got.contains(&"LD_PRELOAD=/store/rootfs/.podvm/interpose.so /mine.so".to_string()),
        "{got:?}"
    );
    assert!(
        got.contains(&"LD_LIBRARY_PATH=/store/rootfs/lib".to_string()),
        "{got:?}"
    );
    assert!(
        !got.iter().any(|e| e == "LD_LIBRARY_PATH=/caller"),
        "{got:?}"
    );
    assert!(got.contains(&"PATH=/bin".to_string()), "{got:?}");
    assert_eq!(dropped, None, "{dropped:?}");
}

/// Where the tier declined, no preload is rewritten and the library
/// path is still set.
#[test]
fn host_env_without_a_tier_sets_only_the_library_path() {
    let env = ["PATH=/bin".to_string()];
    let (got, dropped) = host_env(
        "/store/rootfs",
        "/.podvm/interpose.so",
        &env,
        &["/store/rootfs/lib".to_string()],
    );
    assert!(!got.iter().any(|e| e.starts_with("LD_PRELOAD=")), "{got:?}");
    assert!(
        got.contains(&"LD_LIBRARY_PATH=/store/rootfs/lib".to_string()),
        "{got:?}"
    );
    assert_eq!(dropped, None, "{dropped:?}");
}

/// A caller-only preload is dropped and reported, because guest paths
/// cannot resolve without a chroot.
#[test]
fn host_env_drops_a_caller_only_preload_and_reports_it() {
    let env = ["LD_PRELOAD=/mine.so".to_string()];
    let (got, dropped) = host_env(
        "/store/rootfs",
        "/.podvm/interpose.so",
        &env,
        &["/store/rootfs/lib".to_string()],
    );
    assert!(!got.iter().any(|e| e.starts_with("LD_PRELOAD=")), "{got:?}");
    assert_eq!(dropped, Some("/mine.so".to_string()), "{dropped:?}");
}

/// The invocation keeps the invoked name: `sh` through the search path
/// stays `sh`, not the resolved `busybox` file, so applet dispatch
/// survives the loader invocation.
#[test]
fn invocation_keeps_the_invoked_name() {
    let d = tree("invocation");
    let root = d.to_string_lossy().to_string();
    std::fs::create_dir_all(d.join("bin")).unwrap();
    std::fs::write(d.join("bin/busybox"), b"\x7fELF").unwrap();
    std::fs::hard_link(d.join("bin/busybox"), d.join("bin/sh")).unwrap_or_else(|_| {
        std::fs::write(d.join("bin/sh"), b"\x7fELF").unwrap();
    });
    let resolved = format!("{root}/bin/busybox");
    let got = invocation_path(&root, "sh", &["/bin".to_string()], &resolved);
    assert_eq!(got, format!("{root}/bin/sh"), "{got}");
    let slash = invocation_path(&root, "/bin/sh", &[], &resolved);
    assert_eq!(slash, format!("{root}/bin/sh"), "{slash}");
    let missing = invocation_path(&root, "absent-tool", &["/bin".to_string()], &resolved);
    assert_eq!(missing, resolved, "{missing}");
    let _ = std::fs::remove_dir_all(&d);
}

/// An absolute link to nowhere on the host dangles there (the alpine
/// shape the lane measured: `/bin/sh` pointing at the absolute
/// `/bin/busybox`, absent from the host root): the resolved file is
/// what opens, and the invoked name is carried by
/// [`loader_argv_for`], not by the path.
#[test]
#[cfg(unix)]
fn invocation_falls_back_where_an_absolute_link_dangles() {
    let d = tree("abslink");
    let root = d.to_string_lossy().to_string();
    std::fs::create_dir_all(d.join("bin")).unwrap();
    std::fs::write(d.join("bin/busybox"), b"\x7fELF").unwrap();
    std::os::unix::fs::symlink("/nonexistent-podvm-target/busybox", d.join("bin/sh")).unwrap();
    let resolved = format!("{root}/bin/busybox");
    let got = invocation_path(&root, "sh", &["/bin".to_string()], &resolved);
    assert_eq!(got, resolved, "{got}");
    let _ = std::fs::remove_dir_all(&d);
}

/// `sh` through a busybox file runs as `busybox sh`: the applet name
/// rides beside the opened path.
#[test]
fn loader_argv_inserts_the_applet_name_for_busybox() {
    let rest = ["-c".to_string(), "echo hi".to_string()];
    let (argv, inserted) = loader_argv_for("/root/lib/ld.so", "/root/bin/busybox", "sh", &rest);
    assert!(inserted);
    assert_eq!(
        argv,
        vec![
            "/root/lib/ld.so",
            "/root/bin/busybox",
            "sh",
            "-c",
            "echo hi",
        ],
        "{argv:?}"
    );
}

/// An absolute invoked spelling inserts the same way: the link the
/// kernel would exec is what selects the applet.
#[test]
fn loader_argv_inserts_the_applet_name_for_an_absolute_spelling() {
    let (argv, inserted) = loader_argv_for("/root/lib/ld.so", "/root/bin/busybox", "/bin/sh", &[]);
    assert!(inserted);
    assert_eq!(
        argv,
        vec!["/root/lib/ld.so", "/root/bin/busybox", "sh"],
        "{argv:?}"
    );
}

/// `busybox` named as itself passes through: the caller's own
/// arguments already select the applet.
#[test]
fn loader_argv_leaves_explicit_busybox_alone() {
    let rest = ["sh".to_string()];
    let (argv, inserted) =
        loader_argv_for("/root/lib/ld.so", "/root/bin/busybox", "busybox", &rest);
    assert!(!inserted);
    assert_eq!(
        argv,
        vec!["/root/lib/ld.so", "/root/bin/busybox", "sh"],
        "{argv:?}"
    );
}

/// An ordinary payload passes through untouched.
#[test]
fn loader_argv_leaves_ordinary_payloads_alone() {
    let rest = ["-c".to_string()];
    let (argv, inserted) = loader_argv_for("/root/lib/ld.so", "/root/bin/sh", "sh", &rest);
    assert!(!inserted);
    assert_eq!(
        argv,
        vec!["/root/lib/ld.so", "/root/bin/sh", "-c"],
        "{argv:?}"
    );
}

/// TODO/enter.md T-1407: guest `/` anchors on the rootfs host path, and
/// the variable that carries it is `PODVM_MAPS`.
#[test]
fn the_userland_table_anchors_root_on_the_rootfs() {
    assert_eq!(MAPS_VAR, "PODVM_MAPS");
    assert_eq!(
        userland_maps_table("/store/rootfs").as_deref(),
        Ok("/:/store/rootfs")
    );
    // A trailing slash trims rather than doubling the separator.
    assert_eq!(
        userland_maps_table("/store/rootfs/").as_deref(),
        Ok("/:/store/rootfs")
    );
}

/// TODO/enter.md T-1407: a rootfs the table cannot spell refuses rather
/// than rewriting nothing, which on this rung would be a silent host
/// write. The exclusions themselves live interposer-side and never in
/// the built value: no `/.podvm`, `/proc` or exclude list rides here.
#[test]
fn an_unspellable_rootfs_is_a_refusal_not_an_empty_table() {
    for bad in ["", "/", "relative/root", "/tmp/a:b", "/tmp/a,b", "/tmp/\0x"] {
        assert!(userland_maps_table(bad).is_err(), "{bad:?} was accepted");
    }
    let long = format!("/{}", "a".repeat(MAX_MAP_SIDE));
    assert!(
        userland_maps_table(&long).is_err(),
        "an overlong root was accepted"
    );
    let table = userland_maps_table("/store/rootfs").unwrap();
    assert!(!table.contains(".podvm"), "{table}");
    assert!(!table.contains("/proc"), "{table}");
}
