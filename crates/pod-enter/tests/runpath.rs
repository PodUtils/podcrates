//! The loader of an image loads a library that only a `RUNPATH` of the
//! program names (T-976, PodUtils/podvm#603): the loader of the host
//! runs by host path, as on the loader family, with `--inhibit-cache` and
//! the `--library-path` of `userland::loader_plan`.
//!
//! The fixture is built here: `cc` makes a library and two programs that
//! need it, and the image takes a copy of the loader and of the C library
//! of the host. A host with no `cc` or no glibc says so and passes as
//! skipped.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;

use pod_enter::abi::{Flavour, interp_of_file};
use pod_enter::userland::loader_plan;

/// Run `cc` with `args` in `dir`; `Err` with its output where it fails.
fn cc(dir: &Path, args: &[&str]) -> Result<(), String> {
    let out = Command::new("cc").current_dir(dir).args(args).output().map_err(|e| format!("cc: {e}"))?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).into_owned()) }
}

/// The image: the loader of the host at its guest path, the C library in
/// `/usr/lib`, `libpodrp.so` in `/opt/podrp/lib` and in
/// `/opt/app/lib/x`, and the programs `/opt/app/bin/abs`
/// (`RUNPATH=/opt/podrp/lib`) and `/opt/app/bin/origin`
/// (`RUNPATH=$ORIGIN/../lib/x`). Gives the image and the loader as a guest
/// path.
fn image(tag: &str) -> Result<(PathBuf, String), String> {
    let d = std::env::temp_dir().join(format!("pod-enter-runpath-e2e-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let src = d.join("src");
    std::fs::create_dir_all(&src).map_err(|e| e.to_string())?;
    std::fs::write(src.join("lib.c"), "#include <stdio.h>\nvoid podrp(void) { puts(\"runpath ok\"); }\n")
        .map_err(|e| e.to_string())?;
    std::fs::write(src.join("prog.c"), "void podrp(void);\nint main(void) { podrp(); return 0; }\n")
        .map_err(|e| e.to_string())?;
    cc(&src, &["-shared", "-fPIC", "-o", "libpodrp.so", "lib.c"])?;
    for (name, runpath) in [("abs", "/opt/podrp/lib"), ("origin", "$ORIGIN/../lib/x")] {
        let rpath = format!("-Wl,-rpath,{runpath}");
        cc(&src, &["-o", name, "prog.c", "-L.", "-lpodrp", &rpath, "-Wl,--enable-new-dtags"])?;
    }
    let interp = interp_of_file(&src.join("abs").display().to_string())
        .map_err(|e| e.to_string())?
        .filter(|i| Flavour::of_libc_name(i) == Flavour::Glibc)
        .ok_or("the programs of cc have no glibc loader")?;
    let libc = Command::new("cc").arg("-print-file-name=libc.so.6").output().map_err(|e| e.to_string())?;
    let libc = String::from_utf8_lossy(&libc.stdout).trim().to_string();
    let copy = |from: &Path, guest: &str| -> Result<(), String> {
        let to = d.join(guest.trim_start_matches('/'));
        std::fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::copy(from, &to).map(|_| ()).map_err(|e| format!("{}: {e}", from.display()))
    };
    copy(Path::new(&interp), &interp)?;
    copy(Path::new(&libc), "/usr/lib/libc.so.6")?;
    copy(&src.join("libpodrp.so"), "/opt/podrp/lib/libpodrp.so")?;
    copy(&src.join("libpodrp.so"), "/opt/app/lib/x/libpodrp.so")?;
    copy(&src.join("abs"), "/opt/app/bin/abs")?;
    copy(&src.join("origin"), "/opt/app/bin/origin")?;
    Ok((d, interp))
}

/// Run the program at `guest` of the image through the loader, as the
/// loader family runs it.
fn run(d: &Path, interp: &str, guest: &str) -> (bool, String) {
    let root = d.display().to_string();
    let payload = d.join(guest.trim_start_matches('/')).display().to_string();
    let plan = loader_plan(&root, interp, &payload).expect("the loader plan");
    let mut cmd = Command::new(&plan.loader_host);
    if plan.inhibit_cache {
        cmd.arg("--inhibit-cache");
    }
    let out = cmd.arg("--library-path").arg(plan.lib_dirs.join(":")).arg(&payload).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.success() && text.contains("runpath ok"), format!("{text} (library path {:?})", plan.lib_dirs))
}

/// `RUNPATH=/opt/podrp/lib`, as `systemctl` names `/usr/lib/systemd`: the
/// loader reads that directory on the host, so the library path must name
/// the one of the image.
#[test]
fn an_absolute_runpath_of_the_image_loads() {
    let (d, interp) = match image("abs") {
        Ok(x) => x,
        Err(why) => return pod_testkit::host::skip_line(&format!("no fixture of the loader: {why}")),
    };
    let (ok, text) = run(&d, &interp, "/opt/app/bin/abs");
    assert!(ok, "{text}");
    let _ = std::fs::remove_dir_all(&d);
}

/// `RUNPATH=$ORIGIN/../lib/x`, with the library only there.
#[test]
fn a_runpath_of_origin_loads() {
    let (d, interp) = match image("origin") {
        Ok(x) => x,
        Err(why) => return pod_testkit::host::skip_line(&format!("no fixture of the loader: {why}")),
    };
    let (ok, text) = run(&d, &interp, "/opt/app/bin/origin");
    assert!(ok, "{text}");
    let _ = std::fs::remove_dir_all(&d);
}
