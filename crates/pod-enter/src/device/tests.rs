use super::*;

const N: Names = Names::DEFAULT;

#[test]
fn parse_defaults_guest_and_perms() {
    let m = parse("/dev/zero").unwrap();
    assert_eq!(m.host, "/dev/zero");
    assert_eq!(m.guest, "/dev/zero");
    assert!(m.read && m.write);
    assert_eq!(canon(&m), "rw");
}

#[test]
fn parse_full_shape() {
    let m = parse("/dev/zero:/dev/hostzero:rm").unwrap();
    assert_eq!(m.guest, "/dev/hostzero");
    assert!(m.read && !m.write);
    assert_eq!(canon(&m), "r");
}

#[test]
fn parse_two_parts_keep_default_perms() {
    let m = parse("/dev/null:/dev/hostnull").unwrap();
    assert_eq!(canon(&m), "rw");
}

#[test]
fn parse_write_only() {
    let m = parse("/dev/null:/dev/out:w").unwrap();
    assert!(!m.read && m.write);
}

#[test]
fn parse_refuses_shapes() {
    for bad in ["", ": : : :", "/dev/zero:/a:/b:c", "/dev/zero:relative:r", "x", "/dev/zero:/g:rx", "/dev/zero:/:r"] {
        assert!(parse(bad).is_err(), "{bad:?} must not parse");
    }
    assert!(parse("/dev/zero:/g:m").is_err());
}

#[test]
fn spec_round_trip() {
    let m = parse("/dev/zero:/dev/hostzero:rwm").unwrap();
    assert_eq!(spec_entry(&m), "/dev/zero:/dev/hostzero:rw");
    let back = specs_of(&N, &[format!("{}=/dev/zero:/dev/hostzero:rw", N.device_spec)]).unwrap();
    assert_eq!(back, vec![m]);
}

#[test]
fn specs_of_empty_and_absent() {
    assert!(specs_of(&N, &[]).unwrap().is_empty());
    assert!(specs_of(&N, &[format!("{}=", N.device_spec)]).unwrap().is_empty());
    assert!(specs_of(&N, &[format!("{}=/dev/zero:/g", N.device_spec)]).is_err());
}

#[test]
fn push_spec_scrubs_and_skips_empty() {
    let mut env = vec![format!("{}=stale", N.device_fds), "PATH=/bin".into()];
    push_spec(&N, &mut env, &[]);
    assert_eq!(env, vec!["PATH=/bin".to_string()]);
    let specs = vec![parse("/dev/zero:/z:r").unwrap()];
    push_spec(&N, &mut env, &specs);
    assert!(env.iter().any(|e| *e == format!("{}=/dev/zero:/z:r", N.device_spec)));
}
