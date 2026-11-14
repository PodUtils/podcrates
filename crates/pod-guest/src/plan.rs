//! The emulator invocation of the UEFI flavour, the accelerator, and the
//! `qemu-img` steps of the overlay and the convert.
//!
//! The accelerator comes from the machine profile of `pod-hostprobe`, and
//! the crate names it: `kvm` where the `full` profile holds, `tcg` where
//! only `tcg` holds, and a refusal where neither holds. A machine that the
//! machine tier refuses for a Linux guest is not a machine that boots this
//! guest.
//!
//! `-accel tcg` is much slower, and it is not a reason to refuse: the
//! crate runs on hosts with no `/dev/kvm`, and the caller says which one
//! ran.
//!
//! The firmware of the UEFI flavour is OVMF, and both files are necessary:
//! Windows does not boot from the legacy BIOS path here. The variable
//! store is a copy for each run: a store that two runs share carries a
//! boot entry of the last run into the next.

use std::path::{Path, PathBuf};

use pod_hostprobe::machine::Profile;

pub mod options;

pub use options::{Channel, Forward, Medium, MediumKind, Nic, NicModel, Options, Proto, UserNet, Virtio};

/// Which accelerator to ask the emulator for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accel {
    /// `/dev/kvm` holds: hardware virtualisation.
    Kvm,
    /// The translator of the emulator. Slower, and it runs on each host.
    Tcg,
}

impl Accel {
    pub fn word(self) -> &'static str {
        match self {
            Accel::Kvm => "kvm",
            Accel::Tcg => "tcg",
        }
    }

    /// The CPU model of the accelerator. `host` is valid only under KVM;
    /// under TCG the emulator refuses it, so the two are one decision.
    pub fn cpu(self) -> &'static str {
        match self {
            Accel::Kvm => "host",
            Accel::Tcg => "max",
        }
    }
}

/// The accelerator that the machine profile allows, or `None` where the
/// machine tier refuses. `None` is a refusal, never a default.
pub fn accel_for(profile: Option<Profile>) -> Option<Accel> {
    match profile {
        Some(Profile::Full) => Some(Accel::Kvm),
        Some(Profile::Tcg) => Some(Accel::Tcg),
        None => None,
    }
}

/// The description of one guest run for the emulator.
#[derive(Debug, Clone)]
pub struct Plan {
    /// The emulator binary, for example `qemu-system-x86_64`.
    pub emulator: PathBuf,
    pub accel: Accel,
    /// Where the emulator finds its firmware and option ROMs.
    pub share: PathBuf,
    /// The code image of OVMF, read-only.
    pub firmware_code: PathBuf,
    /// The variable store of OVMF: the copy of this run, which the guest
    /// changes.
    pub firmware_vars: PathBuf,
    /// The root disk of the emulator: always an overlay that
    /// [`overlay_argv`] makes, with the read-only base image as `-b`.
    ///
    /// On the run path, nothing that the run writes lives after it. On the
    /// provision path, [`convert_argv`] writes the overlay into a new
    /// standalone image with no backing file, and the overlay goes. It is
    /// never the base image.
    pub root: PathBuf,
    /// The raw FAT mailbox volume.
    pub mailbox: PathBuf,
    /// Where the guest's serial output goes.
    pub serial: PathBuf,
    /// The QMP socket: a provision types into the console through it, and
    /// a stop of a run that hangs uses it. It is in a directory of mode
    /// 0700: a process that connects to it can type into the guest.
    pub monitor: PathBuf,
    /// Guest memory in MiB.
    pub memory_mib: u64,
    pub cpus: u32,
    /// The NIC, the devices, the media, VNC, the serial socket, the
    /// channels and the Hyper-V flags of the UEFI flavour ([`argv`]); the
    /// DOS flavour does not read them. The default is the mailbox run.
    pub options: Options,
    /// More arguments of the emulator, at the end of the line, so that a
    /// caller can override each value above.
    pub emu_args: Vec<String>,
}

/// The argv of the emulator for the UEFI flavour (q35, OVMF, NVMe), in
/// order, with no program.
///
/// The mailbox is a second NVMe device with `cache=writethrough`. The host
/// writes the command into the image file and reads the result from it; a
/// write-back cache can hold a result that the file does not have yet.
///
/// `-nic none`: the guest gets no network, unless [`Plan::options`] asks
/// for user-mode networking ([`Nic::User`]).
///
/// The argv does not check the options: [`Options::check`] does, and the
/// start of the emulator calls it.
pub fn argv(p: &Plan) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    let mut push = |s: &str| a.push(s.to_string());
    push("-M");
    push("q35");
    push("-accel");
    push(p.accel.word());
    push("-cpu");
    push(&p.options.cpu(p.accel));
    push("-smp");
    push(&p.cpus.to_string());
    push("-m");
    push(&p.memory_mib.to_string());
    push("-L");
    push(&p.share.to_string_lossy());
    push("-drive");
    push(&format!("if=pflash,format=raw,readonly=on,file={}", options::escape(&p.firmware_code)));
    push("-drive");
    push(&format!("if=pflash,format=raw,file={}", options::escape(&p.firmware_vars)));
    push("-drive");
    push(&format!("if=none,id=root,file={},format=qcow2", options::escape(&p.root)));
    push("-device");
    push("nvme,drive=root,serial=wqroot");
    push("-drive");
    push(&format!("if=none,id=mbox,file={},format=raw,cache=writethrough", options::escape(&p.mailbox)));
    push("-device");
    push("nvme,drive=mbox,serial=wqmbox");
    push("-vga");
    push("std");
    push("-display");
    push("none");
    push("-rtc");
    push("base=localtime");
    // A guest that reboots itself never ends its run. The agent powers the
    // machine off; each other end is the timeout of the host.
    push("-no-reboot");
    push("-nic");
    push(&p.options.nic());
    p.options.devices(&mut a);
    p.options.serial(&p.serial, &mut a);
    let mut push = |s: &str| a.push(s.to_string());
    // QMP, not HMP: a refusal is an `"error"` member, not a line on a debug
    // console.
    push("-qmp");
    push(&format!("unix:{},server=on,wait=off", p.monitor.display()));
    // Last, so that an argument of the caller overrides a default of the
    // crate.
    a.extend(p.emu_args.iter().cloned());
    a
}

/// The `qemu-img` format name of a base image. Not the extension: `.img`,
/// `.bin` and `.iso` are `raw`, and `qemu-img` refuses `-F img`.
pub fn base_format(base: &Path) -> &'static str {
    match base.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("qcow2") => "qcow2",
        Some("qcow") => "qcow",
        Some("vhdx") => "vhdx",
        Some("vmdk") => "vmdk",
        Some("vdi") => "vdi",
        Some("vpc") | Some("vhd") => "vpc",
        // The default is `raw`, not the extension: the name of a disk image
        // often does not tell its format.
        _ => "raw",
    }
}

/// The `qemu-img create` argv of the overlay: a qcow2 over the read-only
/// base image, so that nothing that a run writes lives after it.
///
/// The base is named only as `-b`: `qemu-img create` opens the backing
/// file read-only. Nothing in this crate writes the base.
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

/// The `qemu-img convert` argv: write the chain of the overlay (the
/// overlay over its base) into a new standalone qcow2 at `out`.
///
/// `qemu-img commit` has no output file: it writes an overlay into its
/// backing file, in place, and so would write the base. `convert` reads
/// the whole chain and writes a new image with no backing file: the base
/// stays as it was, and the caller can move or remove it.
///
/// `-f qcow2` names the format of the overlay ([`overlay_argv`] makes a
/// qcow2), and `-O qcow2` the format of `out`, as the caller chooses its
/// name. A qcow2 overlay over a VHDX base gives a qcow2 image of the
/// virtual size of the VHDX.
pub fn convert_argv(qemu_img: &Path, overlay: &Path, out: &Path) -> Vec<String> {
    vec![
        qemu_img.to_string_lossy().to_string(),
        "convert".to_string(),
        "-f".to_string(),
        "qcow2".to_string(),
        "-O".to_string(),
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
            firmware_vars: "/run/guest/vars.fd".into(),
            root: "/run/guest/run.qcow2".into(),
            mailbox: "/run/guest/mailbox.img".into(),
            serial: "/run/guest/serial.log".into(),
            monitor: "/run/guest/mon.sock".into(),
            memory_mib: 6144,
            cpus: 4,
            options: Options::default(),
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
        assert!(a.iter().any(|x| x.contains("id=root") && x.contains("run.qcow2")));
        assert!(a.iter().any(|x| x.contains("id=mbox") && x.contains("cache=writethrough")));
        assert!(has(&a, "nvme,drive=root,serial=wqroot"));
        assert!(has(&a, "nvme,drive=mbox,serial=wqmbox"));
    }

    #[test]
    fn a_guest_run_has_no_network_and_no_display() {
        let a = argv(&plan(Accel::Kvm));
        let nic = a.iter().position(|x| x == "-nic").expect("names -nic");
        assert_eq!(a[nic + 1], "none");
        let d = a.iter().position(|x| x == "-display").expect("names -display");
        assert_eq!(a[d + 1], "none");
        // It does not reboot itself with no end.
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
        let code = a.iter().find(|x| x.contains("OVMF_CODE")).expect("the code image");
        assert!(code.contains("readonly=on"), "{code}");
        let vars = a.iter().find(|x| x.contains("vars.fd")).expect("the var store");
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
        // The `-nic none` of the crate is still earlier in the line.
        let first = a.iter().position(|x| x == "-nic").unwrap();
        assert_eq!(a[first + 1], "none");
    }

    #[test]
    fn a_ragged_base_extension_is_mapped_to_the_format_qemu_img_knows() {
        // `-F img` is not a qemu-img format.
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
        let a = overlay_argv(Path::new("qemu-img"), Path::new("ValidationOS.vhdx"), Path::new("run.qcow2"));
        assert_eq!(a[0], "qemu-img");
        let f = a.iter().position(|x| x == "-F").expect("-F is stated");
        assert_eq!(a[f + 1], "vhdx");
        let b = a.iter().position(|x| x == "-b").expect("-b is stated");
        assert_eq!(a[b + 1], "ValidationOS.vhdx");
        assert_eq!(a.last().unwrap(), "run.qcow2");
    }

    /// The convert names the overlay, then a second file, and no backing
    /// file, so the image that it writes stands alone. `qemu-img commit`
    /// takes one file name and writes the overlay into the base, in place
    /// (PodUtils/podvm#277); `tests/convert.rs` runs `qemu-img`.
    #[test]
    fn the_convert_writes_a_separate_standalone_image() {
        let a = convert_argv(
            Path::new("qemu-img"),
            Path::new("/run/pod-guest-1-run/run.qcow2"),
            Path::new("/images/ValidationOS.guest.qcow2"),
        );
        assert_eq!(a[..2], ["qemu-img", "convert"]);
        assert!(!has(&a, "-b") && !has(&a, "-B"), "a backing file here would give the image a backing pointer: {a:?}");
        assert!(!a.iter().any(|x| x.contains("ValidationOS.vhdx")), "the convert does not name the base image: {a:?}");
        let o = a.iter().position(|x| x == "-O").expect("-O is stated");
        assert_eq!(a[o + 1], "qcow2");
        assert_eq!(
            &a[a.len() - 2..],
            &["/run/pod-guest-1-run/run.qcow2".to_string(), "/images/ValidationOS.guest.qcow2".to_string()]
        );
    }

    /// The overlay and the provisioned image are two files, and the
    /// provision path makes both: a cleanup of one cannot remove the other,
    /// and neither names the base image.
    #[test]
    fn the_provisioned_image_is_a_different_file_from_the_overlay_it_comes_from() {
        let overlay = Path::new("/run/pod-guest-1-run/run.qcow2");
        let out = Path::new("/images/ValidationOS.guest.qcow2");
        assert_ne!(overlay, out);
        let create = overlay_argv(Path::new("qemu-img"), Path::new("/images/ValidationOS.vhdx"), overlay);
        let convert = convert_argv(Path::new("qemu-img"), overlay, out);
        assert_eq!(
            create.last().unwrap(),
            &convert[convert.len() - 2],
            "the boot writes the overlay that the convert reads"
        );
        assert!(create.iter().any(|x| x.contains("ValidationOS.vhdx")));
        assert!(!convert.iter().any(|x| x.contains("ValidationOS.vhdx")));
    }
}
