use super::*;
use crate::reference::Reference;

fn refused(input: &str) -> String {
    Reference::parse(input).expect_err(input).to_string()
}

#[test]
fn a_name_of_255_characters_parses_and_one_more_is_refused() {
    // `a/` + 253 characters of `b` = 255: two components.
    let ok = format!("a/{}", "b".repeat(NAME_MAX - 2));
    assert_eq!(ok.len(), NAME_MAX);
    assert!(Reference::parse(&ok).is_ok());
    let e = refused(&format!("{ok}b"));
    assert!(e.contains("the name has 256 characters") && e.contains("the most is 255"), "{e}");
    // The domain counts, and the tag does not.
    let domain = format!("reg.io/{}", "c".repeat(NAME_MAX - 7));
    assert!(Reference::parse(&format!("{domain}:1")).is_ok());
    assert!(refused(&format!("{domain}c:1")).contains("256 characters"));
    // The `library/` that the parse adds does not count.
    assert!(Reference::parse(&"d".repeat(NAME_MAX)).is_ok());
}

#[test]
fn a_repository_of_5000_characters_is_refused_with_its_length() {
    let e = refused(&format!("alpine/{}", "x".repeat(5000)));
    assert!(e.contains("5007 characters") && e.contains("255"), "{e}");
    assert!(e.len() < 300, "the error repeats the input: {}", e.len());
}

#[test]
fn a_tag_of_128_characters_parses_and_one_more_is_refused() {
    let ok = format!("alpine:{}", "t".repeat(TAG_MAX));
    assert_eq!(Reference::parse(&ok).unwrap().tag.as_deref().map(str::len), Some(TAG_MAX));
    let e = refused(&format!("{ok}t"));
    assert!(e.contains("the tag has 129 characters") && e.contains("the most is 128"), "{e}");
}

#[test]
fn a_tag_starting_with_a_dot_or_a_dash_is_refused() {
    for bad in ["alpine:.x", "alpine:-x", "alpine:.", "alpine:-", "alpine:", "alpine:a b", "alpine:a/b:c"] {
        assert!(Reference::parse(bad).is_err(), "{bad} parsed");
    }
    assert!(refused("alpine:.x").contains("tag \".x\""));
    for good in ["alpine:_x", "alpine:0", "alpine:x.y-z_w", "alpine:V1"] {
        assert!(Reference::parse(good).is_ok(), "{good} was refused");
    }
}

#[test]
fn a_component_outside_docker_grammar_is_refused_by_name() {
    for bad in ["-a", "a-", "a..b", "a._b", "a___b", "a.", "_a", ".a", "a/-b", "a/b-", "UP", "a b", "a/b__"] {
        let input = format!("reg.io/{bad}");
        let e = Reference::parse(&input).expect_err(&input).to_string();
        let part = bad.rsplit('/').next().unwrap();
        assert!(e.contains(&format!("component {part:?}")), "{bad}: {e}");
    }
    // Each separator that the grammar has.
    for good in ["a", "a0", "a.b", "a_b", "a__b", "a-b", "a---b", "a.b-c_d__e", "0", "00a--0"] {
        assert!(component_ok(good), "{good} was refused");
        assert!(Reference::parse(&format!("reg.io/x/{good}")).is_ok());
    }
    assert!(!component_ok(""));
}

#[test]
fn the_references_that_must_parse_still_parse() {
    let d = format!("sha256:{}", "0".repeat(64));
    for good in [
        "alpine",
        "docker.io/library/alpine:3.20",
        "localhost:5000/a/b",
        "myuser/my-app_2:v1.2",
        &format!("alpine@{d}"),
        &format!("ghcr.io/o/r:t@{d}"),
        // A domain has its own rules: upper case and a port are allowed.
        "Reg.Example.COM:5000/a/b",
        "[::1]:5000/a",
    ] {
        assert!(Reference::parse(good).is_ok(), "{good}: {:?}", Reference::parse(good).err());
    }
}
