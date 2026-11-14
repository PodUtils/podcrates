use super::*;
use crate::plan::{Plan, argv};

fn plan(accel: Accel, options: Options) -> Plan {
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
        memory_mib: 4096,
        cpus: 2,
        options,
        emu_args: Vec::new(),
    }
}

/// The value after the first `flag`.
fn after<'a>(a: &'a [String], flag: &str) -> &'a str {
    let at = a.iter().position(|x| x == flag).unwrap_or_else(|| panic!("{flag} in {a:?}"));
    &a[at + 1]
}

/// Each value after `flag`, in order.
fn each<'a>(a: &'a [String], flag: &str) -> Vec<&'a str> {
    a.windows(2).filter(|w| w[0] == flag).map(|w| w[1].as_str()).collect()
}

fn user(forwards: Vec<Forward>) -> Options {
    Options { nic: Nic::User(UserNet { model: NicModel::E1000e, forwards, restrict: false }), ..Options::default() }
}

/// The default options give the line of the mailbox run, argument for
/// argument.
#[test]
fn the_default_options_give_the_mailbox_run_with_no_network() {
    let a = argv(&plan(Accel::Kvm, Options::default()));
    let want = "-M q35 -accel kvm -cpu host -smp 2 -m 4096 -L /usr/share/qemu \
        -drive if=pflash,format=raw,readonly=on,file=/usr/share/edk2/OVMF_CODE.4m.fd \
        -drive if=pflash,format=raw,file=/run/guest/vars.fd \
        -drive if=none,id=root,file=/run/guest/run.qcow2,format=qcow2 -device nvme,drive=root,serial=wqroot \
        -drive if=none,id=mbox,file=/run/guest/mailbox.img,format=raw,cache=writethrough \
        -device nvme,drive=mbox,serial=wqmbox -vga std -display none -rtc base=localtime -no-reboot \
        -nic none -serial file:/run/guest/serial.log -qmp unix:/run/guest/mon.sock,server=on,wait=off";
    assert_eq!(a.join(" "), want);
    assert!(Options::default().check().is_ok());
}

#[test]
fn a_user_nic_forwards_each_port_on_the_loopback() {
    let mut o = user(vec![Forward::local(Proto::Tcp, 3389, 3389), Forward::local(Proto::Udp, 5353, 53)]);
    let a = argv(&plan(Accel::Tcg, o.clone()));
    assert_eq!(after(&a, "-nic"), "user,model=e1000e,hostfwd=tcp:127.0.0.1:3389-:3389,hostfwd=udp:127.0.0.1:5353-:53");
    assert_eq!(each(&a, "-nic").len(), 1, "one NIC, and no `-nic none`");
    o.nic = Nic::User(UserNet { model: NicModel::VirtioNet, forwards: Vec::new(), restrict: true });
    assert_eq!(after(&argv(&plan(Accel::Tcg, o)), "-nic"), "user,model=virtio-net-pci,restrict=on");
    let open = Forward { host: Ipv4Addr::UNSPECIFIED, ..Forward::local(Proto::Tcp, 2222, 22) };
    assert_eq!(open.spec(), "tcp:0.0.0.0:2222-:22");
}

#[test]
fn a_port_0_or_a_port_of_the_host_two_times_is_refused() {
    for bad in [
        user(vec![Forward::local(Proto::Tcp, 0, 22)]),
        user(vec![Forward::local(Proto::Tcp, 2222, 0)]),
        user(vec![Forward::local(Proto::Tcp, 2222, 22), Forward::local(Proto::Tcp, 2222, 3389)]),
    ] {
        let e = bad.check().unwrap_err();
        assert_eq!(e.class, Class::Usage, "{e}");
    }
    // One port of the host for TCP and for UDP is two forwards.
    assert!(user(vec![Forward::local(Proto::Tcp, 53, 53), Forward::local(Proto::Udp, 53, 53)]).check().is_ok());
}

#[test]
fn the_virtio_devices_are_named_once_each() {
    let o =
        Options { virtio: vec![Virtio::Rng, Virtio::Balloon, Virtio::Keyboard, Virtio::Tablet], ..Options::default() };
    let a = argv(&plan(Accel::Kvm, o.clone()));
    let devices = each(&a, "-device");
    for d in ["virtio-rng-pci", "virtio-balloon-pci", "virtio-keyboard-pci", "virtio-tablet-pci"] {
        assert!(devices.contains(&d), "{d} in {devices:?}");
    }
    assert!(o.check().is_ok());
    let twice = Options { virtio: vec![Virtio::Rng, Virtio::Rng], ..Options::default() };
    assert_eq!(twice.check().unwrap_err().class, Class::Usage);
}

#[test]
fn a_cdrom_is_read_only_on_its_own_ahci_port_and_a_usb_stick_has_its_format() {
    let medium = |kind, path: &str, readonly, boot| Medium { kind, path: path.into(), readonly, boot };
    let o = Options {
        media: vec![
            medium(MediumKind::Cdrom, "/m/win11.iso", false, true),
            medium(MediumKind::Cdrom, "/m/virtio-win.iso", false, false),
            medium(MediumKind::Usb, "/m/stick.qcow2", false, false),
            medium(MediumKind::Usb, "/m/answer.img", true, false),
        ],
        ..Options::default()
    };
    assert!(o.check().is_ok());
    let a = argv(&plan(Accel::Kvm, o));
    let drives = each(&a, "-drive");
    assert!(drives.contains(&"if=none,id=cd0,media=cdrom,readonly=on,format=raw,file=/m/win11.iso"), "{drives:?}");
    assert!(drives.contains(&"if=none,id=cd1,media=cdrom,readonly=on,format=raw,file=/m/virtio-win.iso"));
    assert!(drives.contains(&"if=none,id=usb0,format=qcow2,file=/m/stick.qcow2"), "{drives:?}");
    assert!(drives.contains(&"if=none,id=usb1,format=raw,readonly=on,file=/m/answer.img"), "{drives:?}");
    let devices = each(&a, "-device");
    assert!(devices.contains(&"ide-cd,drive=cd0,bus=ide.0,bootindex=0"), "{devices:?}");
    assert!(devices.contains(&"ide-cd,drive=cd1,bus=ide.1"));
    assert!(devices.contains(&"usb-storage,bus=xhci.0,drive=usb1,removable=on"));
    assert_eq!(devices.iter().filter(|d| **d == "qemu-xhci,id=xhci").count(), 1, "one USB bus");
    // The bus comes before each device on it.
    let bus = a.iter().position(|x| x == "qemu-xhci,id=xhci").unwrap();
    assert!(a.iter().position(|x| x.starts_with("usb-storage")).unwrap() > bus);
}

#[test]
fn media_that_the_machine_cannot_take_are_refused() {
    let cd = |boot| Medium { kind: MediumKind::Cdrom, path: "/m/x.iso".into(), readonly: true, boot };
    let seven = Options { media: vec![cd(false); MAX_CDROMS + 1], ..Options::default() };
    assert_eq!(seven.check().unwrap_err().class, Class::Usage);
    assert!(Options { media: vec![cd(false); MAX_CDROMS], ..Options::default() }.check().is_ok());
    let two_boots = Options { media: vec![cd(true), cd(true)], ..Options::default() };
    assert_eq!(two_boots.check().unwrap_err().class, Class::Usage);
    let no_path = Options { media: vec![Medium { path: PathBuf::new(), ..cd(false) }], ..Options::default() };
    assert_eq!(no_path.check().unwrap_err().class, Class::Usage);
}

#[test]
fn vnc_a_tablet_a_serial_socket_and_channels_go_to_their_sockets() {
    let o = Options {
        vnc: Some("/run/guest/vnc.sock".into()),
        usb_tablet: true,
        serial_socket: Some("/run/guest/serial.sock".into()),
        channels: vec![Channel { name: QGA_CHANNEL.into(), socket: "/run/guest/qga.sock".into() }],
        ..Options::default()
    };
    assert!(o.check().is_ok());
    let a = argv(&plan(Accel::Tcg, o));
    assert_eq!(after(&a, "-vnc"), "unix:/run/guest/vnc.sock");
    assert_eq!(after(&a, "-display"), "none", "VNC is the only display");
    let devices = each(&a, "-device");
    assert!(devices.contains(&"usb-tablet,bus=xhci.0") && devices.contains(&"qemu-xhci,id=xhci"), "{devices:?}");
    assert!(devices.contains(&"virtio-serial-pci,id=vser"));
    assert!(devices.contains(&"virtserialport,bus=vser.0,chardev=chan0,name=org.qemu.guest_agent.0"));
    let chardevs = each(&a, "-chardev");
    assert!(chardevs.contains(&"socket,id=chan0,path=/run/guest/qga.sock,server=on,wait=off"), "{chardevs:?}");
    // The serial line goes to the socket, and the file keeps a copy.
    assert!(chardevs.contains(
        &"socket,id=serial0,path=/run/guest/serial.sock,server=on,wait=off,logfile=/run/guest/serial.log,logappend=on"
    ));
    assert_eq!(each(&a, "-serial"), ["chardev:serial0"]);
}

#[test]
fn a_socket_path_or_a_channel_name_that_does_not_fit_is_refused() {
    let long = PathBuf::from(format!("/{}", "s".repeat(MAX_SOCKET_PATH)));
    let fits = PathBuf::from(format!("/{}", "s".repeat(MAX_SOCKET_PATH - 1)));
    assert!(Options { vnc: Some(fits), ..Options::default() }.check().is_ok());
    for o in [
        Options { vnc: Some(long.clone()), ..Options::default() },
        Options { serial_socket: Some(PathBuf::new()), ..Options::default() },
        Options { channels: vec![Channel { name: "qga".into(), socket: long }], ..Options::default() },
        Options { channels: vec![Channel { name: "a,b".into(), socket: "/q".into() }], ..Options::default() },
        Options { channels: vec![Channel { name: String::new(), socket: "/q".into() }], ..Options::default() },
        Options {
            channels: vec![
                Channel { name: "q".into(), socket: "/a".into() },
                Channel { name: "q".into(), socket: "/b".into() },
            ],
            ..Options::default()
        },
    ] {
        let e = o.check().unwrap_err();
        assert_eq!(e.class, Class::Usage, "{e}");
    }
}

#[test]
fn the_hyperv_flags_go_on_the_cpu_line_under_kvm_alone() {
    let o = Options { hyperv: true, ..Options::default() };
    let kvm = argv(&plan(Accel::Kvm, o.clone()));
    let cpu = after(&kvm, "-cpu");
    assert!(cpu.starts_with("host,hv_relaxed,"), "{cpu}");
    for f in HYPERV_FLAGS {
        assert!(cpu.split(',').any(|x| x == *f), "{f} in {cpu}");
    }
    // TCG has no Hyper-V enlightenments: the CPU line stays the model.
    assert_eq!(after(&argv(&plan(Accel::Tcg, o)), "-cpu"), "max");
}

/// A comma ends a value of an option list of the emulator: a path with a
/// comma is written with two.
#[test]
fn a_comma_in_a_path_is_doubled_in_an_option_list() {
    let mut p = plan(Accel::Tcg, Options::default());
    p.root = "/run/a,b/run.qcow2".into();
    p.options.media = vec![Medium { kind: MediumKind::Cdrom, path: "/m/x,y.iso".into(), readonly: true, boot: false }];
    let drives = each(&argv(&p), "-drive").into_iter().map(str::to_string).collect::<Vec<_>>();
    assert!(drives.contains(&"if=none,id=root,file=/run/a,,b/run.qcow2,format=qcow2".to_string()), "{drives:?}");
    assert!(drives.iter().any(|d| d.ends_with("file=/m/x,,y.iso")), "{drives:?}");
}

/// The start of the emulator checks the options first: a bad option starts
/// no process.
#[test]
fn the_start_of_the_emulator_refuses_options_that_do_not_check() {
    let mut p = plan(Accel::Tcg, user(vec![Forward::local(Proto::Tcp, 0, 22)]));
    p.emulator = "/nonexistent/qemu-system-x86_64".into();
    let e = crate::emulator::spawn_uefi(&p, std::process::Stdio::null()).unwrap_err();
    assert_eq!(e.class, Class::Usage, "the check, not the missing program: {e}");
}
