use super::*;
use crate::reach::{Answer, Fixed};
use crate::testing::{Scratch, no_in_root};

fn rewrite_with(text: &str, yes: bool) -> (String, Vec<String>) {
    let mut probe = Fixed(yes);
    rewrite(text, &mut Prober::new(&mut probe))
}

/// The scheme, and nothing else: the host, the port, the path, the suite
/// and the components stay.
#[test]
fn only_the_scheme_changes() {
    let src = "deb http://deb.debian.org/debian bookworm main contrib\n";
    let (got, hosts) = rewrite_with(src, true);
    assert_eq!(got, "deb https://deb.debian.org/debian bookworm main contrib\n");
    assert_eq!(hosts, vec!["deb.debian.org"]);
}

#[test]
fn a_port_survives_and_the_host_is_the_key() {
    let (got, hosts) = rewrite_with("baseurl=http://mirror.example:8080/x\n", true);
    assert_eq!(got, "baseurl=https://mirror.example:8080/x\n");
    assert_eq!(hosts, vec!["mirror.example"]);
}

/// A cache of the caller on loopback stays.
#[test]
fn a_literal_address_and_localhost_are_left_alone() {
    for s in ["deb http://127.0.0.1:8080/debian bookworm main\n", "deb http://localhost/debian bookworm main\n"] {
        let (got, hosts) = rewrite_with(s, true);
        assert_eq!(got, s);
        assert!(hosts.is_empty());
    }
}

/// A mirror that does not speak HTTPS keeps its scheme.
#[test]
fn a_mirror_that_does_not_speak_https_is_left_alone() {
    let src = "deb http://deb.debian.org/debian bookworm main\n";
    let (got, hosts) = rewrite_with(src, false);
    assert_eq!(got, src);
    assert!(hosts.is_empty());
}

#[test]
fn an_https_source_is_already_right() {
    let src = "deb https://deb.debian.org/debian bookworm main\n";
    let (got, hosts) = rewrite_with(src, true);
    assert_eq!(got, src);
    assert!(hosts.is_empty());
}

/// The probe gets the host of the URL and nothing else. podvm's code took
/// the text before the first `:` as the host: for `http://user@127.0.0.1/`
/// it asked about `user@127.0.0.1`, which is no address in its rule, and
/// changed a loopback source; for `http://user:pw@mirror.example/` it asked
/// about `user`, which sends the user part to the resolver.
#[test]
fn the_probe_gets_the_host_and_not_the_user_part_or_an_address() {
    let mut asked: Vec<String> = Vec::new();
    let mut probe = |h: &str| {
        asked.push(h.to_string());
        Answer::Https
    };
    let mut p = Prober::new(&mut probe);
    let src = "a http://user@127.0.0.1/x\nb http://user:pw@mirror.example/y\nc http://u@0x7f000001/\n\
               d http://127.1/\ne http://evil.example\\@internal/\nf http://[::1]:8080/\n";
    let (got, hosts) = rewrite(src, &mut p);
    drop(p);
    assert_eq!(asked, ["mirror.example"]);
    assert_eq!(hosts, ["mirror.example"]);
    assert_eq!(
        got,
        "a http://user@127.0.0.1/x\nb https://user:pw@mirror.example/y\nc http://u@0x7f000001/\n\
         d http://127.1/\ne http://evil.example\\@internal/\nf http://[::1]:8080/\n"
    );
}

/// Rule 3 from end to end: a change, then a run with the fixup off, and the
/// file holds the bytes of the image again.
#[test]
fn no_source_fixup_undoes_an_earlier_rewrite() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("src");
    std::fs::create_dir_all(d.at("rootfs/etc/apt")).unwrap();
    let original = "deb http://deb.debian.org/debian bookworm main\n";
    std::fs::write(d.at("rootfs/etc/apt/sources.list"), original).unwrap();
    let root = crate::write::Root::open(&d.at("rootfs")).unwrap();
    let originals = d.at(".originals");

    let on = Settings { originals: Some(originals.clone()), ..Settings::default() };
    let fs = run(&root, &on, &mut Fixed(true)).unwrap();
    assert_eq!(fs[0].action, Action::Rewrote, "{fs:#?}");
    assert!(std::fs::read_to_string(d.at("rootfs/etc/apt/sources.list")).unwrap().contains("https://"));

    let off = Settings { source_fixup: false, ..on };
    let fs = run(&root, &off, &mut Fixed(true)).unwrap();
    assert_eq!(fs[0].action, Action::Restored);
    assert_eq!(std::fs::read_to_string(d.at("rootfs/etc/apt/sources.list")).unwrap(), original);
}

/// With no directory for the originals, no source changes: rule 3 needs
/// them.
#[test]
fn no_place_for_the_originals_changes_no_source() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("src-noplace");
    std::fs::create_dir_all(d.at("etc/apt")).unwrap();
    let original = "deb http://deb.debian.org/debian bookworm main\n";
    std::fs::write(d.at("etc/apt/sources.list"), original).unwrap();
    let fs = run(&d.root(), &Settings::default(), &mut Fixed(true)).unwrap();
    assert_eq!(fs[0].action, Action::Skipped, "{fs:#?}");
    assert_eq!(std::fs::read_to_string(d.at("etc/apt/sources.list")).unwrap(), original);
}
