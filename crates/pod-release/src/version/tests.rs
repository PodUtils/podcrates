use super::*;

fn v(text: &str) -> Version {
    Version::parse(text).unwrap_or_else(|| panic!("{text} parses"))
}

#[test]
fn versions_are_ordered_as_semver_orders_them() {
    let order = [
        "0.9.0",
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-beta.2",
        "1.0.0-beta.10",
        "1.0.0-rc.1",
        "1.0.0",
        "1.0.1",
        "1.10.0",
    ];
    for pair in order.windows(2) {
        assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
    }
    assert_eq!(v("1.0.0+abc").cmp(&v("1.0.0")), Ordering::Equal);
    for wrong in ["1.0", "1.0.0.0", "01.0.0", "1.0.0-", "1.0.0-be ta", "v1.0.0", "1.0.0-01"] {
        assert!(Version::parse(wrong).is_none(), "{wrong}");
    }
    assert_eq!(Version::of_tag("v1.0.0-beta.2").map(|v| v.tag()).as_deref(), Some("v1.0.0-beta.2"));
    assert!(Version::of_tag("1.0.0").is_none());
}

#[test]
fn the_newest_is_a_prerelease_only_when_one_is_allowed() {
    let tags: Vec<String> =
        ["v1.0.0-beta.2", "v0.9.1", "v1.0.0-beta.10", "nightly", "v1.0.0-beta.9"].map(String::from).to_vec();
    assert_eq!(newest(&tags, true).map(|v| v.tag()).as_deref(), Some("v1.0.0-beta.10"));
    assert_eq!(newest(&tags, false).map(|v| v.tag()).as_deref(), Some("v0.9.1"));
    assert_eq!(newest(&tags[3..4], true), None);
}

#[test]
fn the_api_s_list_gives_its_tags_without_drafts() {
    let json = br#"[{"tag_name":"v1.0.0-beta.3","draft":true},{"tag_name":"v1.0.0-beta.2","draft":false,"prerelease":true},{"tag_name":"v1.0.0-beta.1"},{"tag_name":"v9.0.0/../x"}]"#;
    assert_eq!(tags_of_api(json).unwrap(), ["v1.0.0-beta.2", "v1.0.0-beta.1"]);
    assert!(tags_of_api(b"{\"message\":\"API rate limit exceeded\"}").is_err());
}

/// The form of GitHub's feed, as `https://github.com/OWNER/REPO/releases.atom`
/// serves it.
#[test]
fn the_feed_gives_the_tag_of_each_entry() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:media="http://search.yahoo.com/mrss/" xml:lang="en-US">
  <id>tag:github.com,2008:https://github.com/PodUtils/podssh/releases</id>
  <link type="text/html" rel="alternate" href="https://github.com/PodUtils/podssh/releases"/>
  <title>Release notes from podssh</title>
  <entry>
    <id>tag:github.com,2008:Repository/1/v1.0.0-beta.2</id>
    <updated>2026-10-31T00:30:00Z</updated>
    <link rel="alternate" type="text/html" href="https://github.com/PodUtils/podssh/releases/tag/v1.0.0-beta.2"/>
    <title>podssh v1.0.0-beta.2</title>
    <content type="html">&lt;p&gt;notes&lt;/p&gt;</content>
  </entry>
  <entry>
    <id>tag:github.com,2008:Repository/1/v1.0.0-beta.1</id>
    <link rel="alternate" type="text/html" href="https://github.com/PodUtils/podssh/releases/tag/v1.0.0-beta.1"/>
  </entry>
</feed>"#;
    assert_eq!(tags_of_atom(xml).unwrap(), ["v1.0.0-beta.2", "v1.0.0-beta.1"]);
    assert!(tags_of_atom(b"<html>a login page</html>").unwrap().is_empty());
}

/// podnet's issue 227: a tag with a path is not a tag.
#[test]
fn atom_tags_refuses_a_path_in_a_tag() {
    let feed = br#"<link href="https://github.com/PodUtils/podnet/releases/tag/v0.3.0-beta.9/../../x"/>
<link href="https://github.com/PodUtils/podnet/releases/tag/v0.3.0-beta.8"/>"#;
    let tags = tags_of_atom(feed).unwrap();
    assert_eq!(tags, ["v0.3.0-beta.8"]);
    assert_eq!(newest(&tags, true).map(|v| v.tag()).as_deref(), Some("v0.3.0-beta.8"));
}

/// podnet's issue 227: the form of a tag.
#[test]
fn a_tag_must_be_a_tag() {
    for good in ["v0.3.0-beta.9", "v1.0.0", "0.3.0", "v1.0.0+build.1"] {
        assert!(valid_tag(good), "{good}");
    }
    let long = format!("v{}", "1".repeat(64));
    for bad in ["", "v1/../x", "v1..2", "../v1", "x1.0", "v1 2", "v1%2f", "v1?x", long.as_str()] {
        assert!(!valid_tag(bad), "{bad}");
    }
}

/// podnet's `a_glibc_build_takes_the_musl_asset`, and podssh's
/// `each_target_of_the_release_has_its_file`.
#[test]
fn a_glibc_build_takes_the_musl_asset_and_windows_takes_exe() {
    assert_eq!(release_target("x86_64-unknown-linux-gnu"), "x86_64-unknown-linux-musl");
    assert_eq!(release_target("armv7-unknown-linux-gnueabihf"), "armv7-unknown-linux-musleabihf");
    assert_eq!(release_target("arm-unknown-linux-gnueabi"), "arm-unknown-linux-musleabi");
    assert_eq!(release_target("aarch64-apple-darwin"), "aarch64-apple-darwin");
    assert_eq!(asset("podssh", "x86_64-unknown-linux-musl"), "podssh-x86_64-unknown-linux-musl");
    assert_eq!(asset("podssh", "aarch64-pc-windows-msvc"), "podssh-aarch64-pc-windows-msvc.exe");
}
