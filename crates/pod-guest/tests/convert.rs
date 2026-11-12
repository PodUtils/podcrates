//! The steps of `qemu-img` that a provision runs, with the `qemu-img` of
//! the host (PodUtils/podvm#277): a check of the argv alone passed while
//! `qemu-img commit` refused a second file name.
//!
//! The test needs `qemu-img` and `qemu-io` (`qemu-utils`). With no
//! `qemu-img` it says so and passes, unless `POD_GUEST_TEST_QEMU=required`,
//! which `guest.yml` and the plant set: then the absence is a failure.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;

use pod_guest::{convert_argv, overlay_argv};

const MIB: usize = 1 << 20;

fn required() -> bool {
    std::env::var("POD_GUEST_TEST_QEMU").is_ok_and(|v| v == "required")
}

/// The program, or `None` when the host has none.
fn tool(name: &str) -> Option<PathBuf> {
    let found = Command::new(name).arg("--version").output().is_ok_and(|o| o.status.success());
    if !found {
        assert!(!required(), "POD_GUEST_TEST_QEMU=required: no {name}");
        eprintln!("skipped: no {name} on this host");
    }
    found.then(|| PathBuf::from(name))
}

fn run(argv: &[String]) {
    let out = Command::new(&argv[0]).args(&argv[1..]).output().expect("start the step");
    assert!(out.status.success(), "{argv:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|w| w.to_string()).collect()
}

fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// A raw base of 4 MiB, an overlay over it, a write of the guest into the
/// overlay, then the convert: the new image has no backing file, holds the
/// base and the write, and the base keeps each byte.
#[test]
fn the_convert_writes_a_standalone_image_and_leaves_the_base() {
    let (Some(qemu_img), Some(qemu_io)) = (tool("qemu-img"), tool("qemu-io")) else { return };
    let dir = std::env::temp_dir().join(format!("pod-guest-convert-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (base, overlay, out) = (dir.join("base.img"), dir.join("run.qcow2"), dir.join("out.qcow2"));
    let before: Vec<u8> = (0..4 * MIB).map(|i| (i % 251) as u8).collect();
    std::fs::write(&base, &before).unwrap();

    run(&overlay_argv(&qemu_img, &base, &overlay));
    let write = ["-f", "qcow2", "-c", "write -P 0x5a 1M 64k", &path(&overlay)];
    run(&[vec![path(&qemu_io)], words(&write)].concat());
    run(&convert_argv(&qemu_img, &overlay, &out));

    let info = Command::new(&qemu_img).args(["info", "--output=json", &path(&out)]).output().unwrap();
    let info = String::from_utf8_lossy(&info.stdout);
    assert!(info.contains("\"format\": \"qcow2\""), "{info}");
    assert!(!info.contains("backing-filename"), "the image stands alone: {info}");
    assert_eq!(std::fs::read(&base).unwrap(), before, "the base keeps each byte");

    let raw = dir.join("out.raw");
    run(&words(&[&path(&qemu_img), "convert", "-f", "qcow2", "-O", "raw", &path(&out), &path(&raw)]));
    let after = std::fs::read(&raw).unwrap();
    assert_eq!(after.len(), before.len());
    assert!(after[MIB..MIB + 65536].iter().all(|&b| b == 0x5a), "the write of the guest is in the image");
    assert_eq!(after[..MIB], before[..MIB], "the base is in the image");
    assert_eq!(after[MIB + 65536..], before[MIB + 65536..]);
    std::fs::remove_dir_all(&dir).unwrap();
}
