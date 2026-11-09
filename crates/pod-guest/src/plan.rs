//! The emulator invocation for a Windows guest. `TODO/milestones.md` T-1112.
//!
//! ⛔ **Acceleration is chosen from the probe's machine profile and named
//! out loud.** `kvm` where the machine tier's `full` profile holds, `tcg`
//! where only `tcg` does, and a refusal where neither does, the same
//! three-way decision the machine tier already makes, reused rather than
//! re-derived, so a machine that podvm refuses to boot a Linux guest on is
//! not quietly a machine it boots a Windows guest on.
//!
//! ⚠ **`-accel tcg` is slower by a lot and is not a reason to refuse.** The
//! reference implementation refuses TCG outright; that refusal is why this
//! milestone was blocked, because the whole point of podvm's machine tier is
//! that it runs where there is no `/dev/kvm`. The driver runs TCG and the
//! banner says which one ran.
//!
//! ⚠ **The firmware is UEFI, and both OVMF files are needed.** Windows
//! refuses to boot from the legacy BIOS path here, and the variable store is
//! a per-run copy: a vars file shared between runs is a boot entry from the
//! last run leaking into this one.

use std::path::{Path, PathBuf};

use podvm_probe::machine::Profile;

/// Which accelerator to ask the emulator for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accel {
    /// `/dev/kvm` holds: real hardware virtualisation.
    Kvm,
    /// The emulator's own translator. Slower, and portable by construction.
    Tcg,
}

impl Accel {
    pub fn word(self) -> &'static str {
        match self {
            Accel::Kvm => "kvm",
            Accel::Tcg => "tcg",
        }
    }

    /// The cpu model that goes with the accelerator. `host` is only valid
    /// under KVM; under TCG it is refused by the emulator, so this is a
    /// decision the pair makes together rather than two independent ones.
    pub fn cpu(self) -> &'static str {
        match self {
            Accel::Kvm => "host",
            Accel::Tcg => "max",
        }
    }
}

/// The accelerator the machine profile allows, or `None` where the machine
/// tier would refuse. `None` is a refusal and never a default.
pub fn accel_for(profile: Option<Profile>) -> Option<Accel> {
    match profile {
        Some(Profile::Full) => Some(Accel::Kvm),
        Some(Profile::Tcg) => Some(Accel::Tcg),
        None => None,
    }
}

/// Everything one guest run needs to be described to the emulator.
#[derive(Debug, Clone)]
pub struct Plan {
    /// The emulator binary. On this host it is `qemu-system-x86_64`.
    pub emulator: PathBuf,
    pub accel: Accel,
    /// Where the emulator finds its firmware and option ROMs.
    pub share: PathBuf,
    /// OVMF's code image, opened read-only.
    pub firmware_code: PathBuf,
    /// OVMF's variable store. Copied per run, so the copy is what changes.
    pub firmware_vars: PathBuf,
    /// The emulator's root disk, and the field carries both roles.
    ///
    /// ⛔ **On the `run` path it is the disposable per-run overlay**, created
    /// by [`overlay_argv`] with `-b` at the read-only base image, so nothing
    /// a run writes survives it. **On the `setup` path it is the disposable
    /// scratch overlay** the provisioning boot writes its installer into, and
    /// it is created by [`overlay_argv`] and then discarded: the installer's
    /// writes are committed by [`commit_argv`] into a separate standalone
    /// image that carries no backing pointer. It is never the vendor image.
    pub root: PathBuf,
    /// The raw FAT mailbox volume.
    pub mailbox: PathBuf,
    /// Where the guest's serial output goes.
    pub serial: PathBuf,
    /// The QMP socket. It is how provisioning types into the console and how
    /// a stuck run is stopped. ⚠ It is inside a mode-0700 directory: whoever
    /// can connect to it can type into the guest.
    pub monitor: PathBuf,
    /// Guest memory in MiB.
    pub memory_mib: u64,
    pub cpus: u32,
    /// Extra emulator arguments, appended last so a caller can override
    /// anything above. Empty for every field the driver sets itself.
    pub emu_args: Vec<String>,
}

/// The emulator's argv, in order.
///
/// ⚠ **The mailbox is a second NVMe device with `cache=writethrough`.** The
/// host writes the command into the image file and reads the result back out
/// of it; a write-back cache would let the guest see the command and the host
/// see a result that the guest's page cache had not yet flushed. Writethrough
/// is what makes the file the mailbox rather than a copy of it.
///
/// ⚠ **`-nic none`, and the guest gets no network.** A disposable guest that
/// has no business talking to anything should not be one emulator argument
/// away from talking to everything; a caller who wants user-mode networking
/// asks for it explicitly through the emulator argument passthrough.
pub fn argv(p: &Plan) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    let mut push = |s: &str| a.push(s.to_string());
    push("-M");
    push("q35");
    push("-accel");
    push(p.accel.word());
    push("-cpu");
    push(p.accel.cpu());
    push("-smp");
    push(&p.cpus.to_string());
    push("-m");
    push(&p.memory_mib.to_string());
    push("-L");
    push(&p.share.to_string_lossy());
    push("-drive");
    push(&format!(
        "if=pflash,format=raw,readonly=on,file={}",
        p.firmware_code.display()
    ));
    push("-drive");
    push(&format!(
        "if=pflash,format=raw,file={}",
        p.firmware_vars.display()
    ));
    push("-drive");
    push(&format!(
        "if=none,id=root,file={},format=qcow2",
        p.root.display()
    ));
    push("-device");
    push("nvme,drive=root,serial=wqroot");
    push("-drive");
    push(&format!(
        "if=none,id=mbox,file={},format=raw,cache=writethrough",
        p.mailbox.display()
    ));
    push("-device");
    push("nvme,drive=mbox,serial=wqmbox");
    push("-vga");
    push("std");
    push("-display");
    push("none");
    push("-rtc");
    push("base=localtime");
    // A guest that reboots itself is a run that never ends; the agent powers
    // the machine off, and anything else is a timeout the host enforces.
    push("-no-reboot");
    push("-nic");
    push("none");
    push("-serial");
    push(&format!("file:{}", p.serial.display()));
    // ⭐ QMP, not HMP: the documented request/response interface, so a
    // refusal is an `"error"` member rather than a line on a debug console.
    push("-qmp");
    push(&format!("unix:{},server=on,wait=off", p.monitor.display()));
    // ⭐ Last, so a caller's own argument wins over the driver's default. This
    // is the only way to reach an emulator knob the driver does not name, and
    // it is deliberately the tail of the line rather than spliced in.
    a.extend(p.emu_args.iter().cloned());
    a
}

/// The `qemu-img` format name for a base image. ⛔ **Not the extension.**
/// `.img`, `.bin` and `.iso` are all `raw`, and `qemu-img -F img` is a
/// refusal rather than a guess: a defect found in review, where the first
/// revision passed the extension through and would have failed on exactly
/// the disk images a caller is most likely to have.
pub fn base_format(base: &Path) -> &'static str {
    match base
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("qcow2") => "qcow2",
        Some("qcow") => "qcow",
        Some("vhdx") => "vhdx",
        Some("vmdk") => "vmdk",
        Some("vdi") => "vdi",
        Some("vpc") | Some("vhd") => "vpc",
        // ⚠ The default is `raw` and deliberately not the extension: a disk
        // image whose name lies about its format is a common thing, and
        // `qemu-img` will read a raw file's header itself.
        _ => "raw",
    }
}

/// The `qemu-img create` argv for the disposable overlay: `run.qcow2` over
/// the read-only base image, so nothing a run writes survives it.
///
/// ⛔ **The base is only ever named as `-b`, which is a read.** `qemu-img
/// create` opens the backing file read-only and never writes to it, and
/// neither does anything else in this crate: the vendor image is a source,
/// never a destination.
pub fn overlay_argv(qemu_img: &Path, base: &Path, overlay: &Path) -> Vec<String> {
    vec![
        qemu_img.to_string_lossy().to_string(),
        "create".to_string(),
        "-f".to_string(),
        "qcow2".to_string(),
        "-F".to_string(),
        base_format(base).to_string(),
        "-b".to_string(),
        base.to_string_lossy().to_string(),
        overlay.to_string_lossy().to_string(),
    ]
}

/// The `qemu-img commit` argv: fold a scratch overlay's writes into a fresh
/// standalone qcow2 at `out`.
///
/// ⛔ **`-b` is absent and the argument order carries the reason.**
/// `qemu-img commit [-b BASE] [-F FMT] FILENAME [OUTFILE]` writes the
/// overlay into `OUTFILE` when it is named. ⛔ **`OUTFILE` is a
/// `qemu-img create` of its own**, so it comes into existence as a new image
/// rather than as an existing file whose metadata is rewritten. The overlay's
/// own backing is never named, so `OUTFILE` carries no `-b` pointer at all
/// and is self-contained: the vendor image can be moved or removed and the
/// result still boots.
///
/// ⛔ **And `-f qcow2` is stated rather than left to the suffix.** `OUTFILE`
/// here is named by the caller and this crate does not choose its name, so
/// the format is given rather than inferred. `overlay_argv` does the same,
/// which is the existing pattern here and not a new one.
///
/// ⚠ The output format follows the overlay, which [`overlay_argv`] created as
/// `qcow2`, so a `qcow2` overlay over a VHDX backing commits to a `qcow2`
/// image holding the VHDX's full size. That is the cost of not writing into
/// the vendor file, and it is stated here rather than discovered later.
pub fn commit_argv(qemu_img: &Path, overlay: &Path, out: &Path) -> Vec<String> {
    vec![
        qemu_img.to_string_lossy().to_string(),
        "commit".to_string(),
        "-f".to_string(),
        "qcow2".to_string(),
        overlay.to_string_lossy().to_string(),
        out.to_string_lossy().to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(accel: Accel) -> Plan {
        Plan {
            emulator: "/usr/bin/qemu-system-x86_64".into(),
            accel,
            share: "/usr/share/qemu".into(),
            firmware_code: "/usr/share/edk2/OVMF_CODE.4m.fd".into(),
            firmware_vars: "/run/podvm/vars.fd".into(),
            root: "/run/podvm/run.qcow2".into(),
            mailbox: "/run/podvm/mailbox.img".into(),
            serial: "/run/podvm/serial.log".into(),
            monitor: "/run/podvm/mon.sock".into(),
            memory_mib: 6144,
            cpus: 4,
            emu_args: Vec::new(),
        }
    }

    fn has(a: &[String], s: &str) -> bool {
        a.iter().any(|x| x == s)
    }

    #[test]
    fn the_profile_picks_the_accelerator_and_never_defaults_one() {
        assert_eq!(accel_for(Some(Profile::Full)), Some(Accel::Kvm));
        assert_eq!(accel_for(Some(Profile::Tcg)), Some(Accel::Tcg));
        assert_eq!(accel_for(None), None);
    }

    #[test]
    fn the_cpu_model_belongs_to_the_accelerator() {
        assert_eq!(Accel::Kvm.cpu(), "host");
        assert_eq!(Accel::Tcg.cpu(), "max");
        assert_eq!(Accel::Kvm.word(), "kvm");
        assert_eq!(Accel::Tcg.word(), "tcg");
    }

    #[test]
    fn the_argv_names_the_overlay_and_the_mailbox_as_separate_disks() {
        let a = argv(&plan(Accel::Tcg));
        assert!(has(&a, "-accel") && has(&a, "tcg"));
        assert!(has(&a, "max"));
        assert!(a
            .iter()
            .any(|x| x.contains("id=root") && x.contains("run.qcow2")));
        assert!(a
            .iter()
            .any(|x| x.contains("id=mbox") && x.contains("cache=writethrough")));
        assert!(has(&a, "nvme,drive=root,serial=wqroot"));
        assert!(has(&a, "nvme,drive=mbox,serial=wqmbox"));
    }

    #[test]
    fn a_guest_run_has_no_network_and_no_display() {
        let a = argv(&plan(Accel::Kvm));
        let nic = a.iter().position(|x| x == "-nic").expect("names -nic");
        assert_eq!(a[nic + 1], "none");
        let d = a
            .iter()
            .position(|x| x == "-display")
            .expect("names -display");
        assert_eq!(a[d + 1], "none");
        // and it does not silently reboot itself forever
        assert!(has(&a, "-no-reboot"));
    }

    #[test]
    fn kvm_runs_with_the_host_cpu_and_tcg_does_not() {
        let k = argv(&plan(Accel::Kvm));
        assert!(has(&k, "kvm") && has(&k, "host"));
        let t = argv(&plan(Accel::Tcg));
        assert!(has(&t, "tcg") && !has(&t, "host"));
    }

    #[test]
    fn the_firmware_code_is_read_only_and_the_vars_are_not() {
        let a = argv(&plan(Accel::Tcg));
        let code = a
            .iter()
            .find(|x| x.contains("OVMF_CODE"))
            .expect("the code image");
        assert!(code.contains("readonly=on"), "{code}");
        let vars = a
            .iter()
            .find(|x| x.contains("vars.fd"))
            .expect("the var store");
        assert!(!vars.contains("readonly=on"), "{vars}");
    }

    #[test]
    fn the_monitor_is_qmp_and_not_the_debug_console() {
        let a = argv(&plan(Accel::Tcg));
        assert!(!has(&a, "-monitor"), "HMP is not used");
        let q = a.iter().position(|x| x == "-qmp").expect("-qmp is named");
        assert!(a[q + 1].starts_with("unix:"), "{}", a[q + 1]);
        assert!(a[q + 1].contains("server=on"), "{}", a[q + 1]);
    }

    #[test]
    fn a_caller_argument_lands_last_so_it_can_override_a_default() {
        let mut p = plan(Accel::Tcg);
        p.emu_args = vec!["-nic".into(), "user".into()];
        let a = argv(&p);
        assert_eq!(&a[a.len() - 2..], &["-nic".to_string(), "user".to_string()]);
        // and the driver's own `-nic none` is still earlier in the line.
        let first = a.iter().position(|x| x == "-nic").unwrap();
        assert_eq!(a[first + 1], "none");
    }

    #[test]
    fn a_ragged_base_extension_is_mapped_to_the_format_qemu_img_knows() {
        // ⛔ The defect this pins: `-F img` is not a qemu-img format.
        assert_eq!(base_format(Path::new("disk.img")), "raw");
        assert_eq!(base_format(Path::new("disk.bin")), "raw");
        assert_eq!(base_format(Path::new("windows.iso")), "raw");
        assert_eq!(base_format(Path::new("disk.raw")), "raw");
        assert_eq!(base_format(Path::new("disk")), "raw");
        assert_eq!(base_format(Path::new("disk.VHDX")), "vhdx");
        assert_eq!(base_format(Path::new("disk.qcow2")), "qcow2");
        assert_eq!(base_format(Path::new("disk.vmdk")), "vmdk");
        assert_eq!(base_format(Path::new("disk.vhd")), "vpc");
    }

    #[test]
    fn the_overlay_names_the_base_images_own_format() {
        let a = overlay_argv(
            Path::new("qemu-img"),
            Path::new("ValidationOS.vhdx"),
            Path::new("run.qcow2"),
        );
        assert_eq!(a[0], "qemu-img");
        let f = a.iter().position(|x| x == "-F").expect("-F is stated");
        assert_eq!(a[f + 1], "vhdx");
        let b = a.iter().position(|x| x == "-b").expect("-b is stated");
        assert_eq!(a[b + 1], "ValidationOS.vhdx");
        assert_eq!(a.last().unwrap(), "run.qcow2");
    }

    /// ⛔ The defect this pins: `setup` provisioned by committing the scratch
    /// overlay in place, so the image it left carried `-b` at the vendor file
    /// and every later run read `run.qcow2` -> `out` -> `base.vhdx`. The
    /// commit names a second file, and names no backing at all, so the image
    /// it writes stands alone. `TODO/gate.md` T-1641.
    #[test]
    fn the_commit_writes_a_separate_standalone_image() {
        let a = commit_argv(
            Path::new("qemu-img"),
            Path::new("/run/podvm-windows-1-run/run.qcow2"),
            Path::new("/images/ValidationOS.podvm.qcow2"),
        );
        assert_eq!(a[0], "qemu-img");
        assert_eq!(a[1], "commit");
        assert!(
            !has(&a, "-b"),
            "a -b here would give the provisioned image a backing pointer: {a:?}"
        );
        assert!(
            !a.iter().any(|x| x.contains("ValidationOS.vhdx")),
            "the vendor image is not named by the commit: {a:?}"
        );
        // ⛔ `commit [-b BASE] [-F FMT] FILENAME [OUTFILE]`: the overlay is
        // FILENAME and the fresh image is OUTFILE, in that order. Reversed,
        // the vendor's backing is folded into itself.
        assert_eq!(
            &a[a.len() - 2..],
            &[
                "/run/podvm-windows-1-run/run.qcow2".to_string(),
                "/images/ValidationOS.podvm.qcow2".to_string()
            ]
        );
        assert_eq!(a.last().unwrap(), "/images/ValidationOS.podvm.qcow2");
    }

    /// The scratch overlay and the provisioned image are two files, and the
    /// `setup` path creates both: a cleanup keyed on one cannot remove the
    /// other, and the vendor image is named by neither.
    #[test]
    fn the_provisioned_image_is_a_different_file_from_the_overlay_it_comes_from() {
        let overlay = Path::new("/run/podvm-windows-1-run/run.qcow2");
        let out = Path::new("/images/ValidationOS.podvm.qcow2");
        assert_ne!(overlay, out);
        let create = overlay_argv(
            Path::new("qemu-img"),
            Path::new("/images/ValidationOS.vhdx"),
            overlay,
        );
        let commit = commit_argv(Path::new("qemu-img"), overlay, out);
        assert_eq!(
            create.last().unwrap(),
            &commit[commit.len() - 2],
            "the boot writes the overlay the commit reads"
        );
        assert!(create.iter().any(|x| x.contains("ValidationOS.vhdx")));
        assert!(!commit.iter().any(|x| x.contains("ValidationOS.vhdx")));
    }
}
