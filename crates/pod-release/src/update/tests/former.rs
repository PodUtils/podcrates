//! Former signers: a release made before the repository moved is signed by
//! the release workflow under the former name, and its files come from the
//! current repository. The former identity holds only up to its last tag,
//! and only a chain that the roots cannot check gives the hint of newer
//! roots. The fixtures are podcrates v0.76.0 and v0.77.0, each signed by
//! `PodUtils/podcrates`; the stand-in serves them as the releases of
//! `PodUtils/podnew`.

use super::*;
use crate::sigstore::Unverified;

const NEW: &str = "PodUtils/podnew";
const OLD: &str = "PodUtils/podcrates";

/// The file `name` of the release `tag` of the fixtures.
fn release_file(tag: &str, name: &str) -> Option<Vec<u8>> {
    if !["v0.76.0", "v0.77.0"].contains(&tag) || ![SUMS, BUNDLE].contains(&name) {
        return None;
    }
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(tag).join(name)).ok()
}

/// A stand-in that serves the fixtures as the releases of `NEW` alone.
fn moved() -> Stand {
    stand::http(|req: &Request| {
        let prefix = format!("/{NEW}/releases/download/");
        let file = req.path().strip_prefix(prefix.as_str()).and_then(|rest| rest.split_once('/'));
        match file.and_then(|(tag, name)| release_file(tag, name)) {
            Some(body) => Answer::new(200, body),
            None => Answer::new(404, "Not Found"),
        }
    })
    .expect("a stand-in")
}

fn moved_source(stand: &Stand, last: Option<&str>) -> Source {
    let at = format!("http://127.0.0.1:{}", stand.port());
    let former = last.map(|last| vec![Former::new(OLD, last)]).unwrap_or_default();
    Source { api: at.clone(), web: at, former, ..Source::github(NEW, "release.yml") }
}

#[test]
fn a_former_signer_holds_up_to_its_last_tag_in_the_order_of_semver() {
    let former = Former::new(OLD, "v0.76.0");
    for tag in ["v0.76.0", "v0.75.9", "v0.76.0-rc.1", "v0.9.0"] {
        assert!(former.signed(tag), "{tag}");
    }
    for tag in ["v0.76.1", "v0.77.0", "v1.0.0-rc.1", "v0.76.0+later", "0.76.0", "latest", "v0.76", ""] {
        assert!(!former.signed(tag), "{tag}");
    }
    for last in ["main", "v0.76.0+build", "0.76.0", ""] {
        assert!(!Former::new(OLD, last).signed("v0.1.0"), "{last}");
    }
}

#[test]
fn the_identities_of_a_tag_add_a_former_signer_only_up_to_its_last_tag() {
    let source = Source { former: vec![Former::new(OLD, "v0.76.0")], ..Source::github(NEW, "release.yml") };
    let uris = |tag: &str| source.identities(tag).into_iter().map(|i| i.uri).collect::<Vec<_>>();
    assert_eq!(
        uris("v0.76.0"),
        [
            "https://github.com/PodUtils/podnew/.github/workflows/release.yml@refs/tags/v0.76.0",
            "https://github.com/PodUtils/podcrates/.github/workflows/release.yml@refs/tags/v0.76.0",
        ]
    );
    assert_eq!(uris("v0.77.0"), ["https://github.com/PodUtils/podnew/.github/workflows/release.yml@refs/tags/v0.77.0"]);
    assert!(Source::github(NEW, "release.yml").former.is_empty());
}

/// A release before the move verifies by its former signer, and its files
/// come from the current repository: the stand-in serves no other.
#[tokio::test]
async fn a_release_before_the_move_verifies_by_its_former_signer() {
    pod_testkit::need_loopback!();
    let stand = moved();
    let fetcher = Fetcher::new(client()).allow_plain_http();
    let signed = signed(&fetcher, &moved_source(&stand, Some("v0.76.0")), &v("0.76.0")).await.expect("verified");
    assert_eq!(
        signed.identity.uri,
        "https://github.com/PodUtils/podcrates/.github/workflows/release.yml@refs/tags/v0.76.0"
    );
    assert_eq!(signed.verified.logged.index, 3992748924);
    // The control: with no former signer, the same release is refused as
    // signed by another signer, with no hint of the roots.
    match signed_or(&fetcher, &moved_source(&stand, None), "0.76.0").await {
        Err(Error::Refused(why)) => {
            assert!(why.contains("signed by a signer that this program does not accept"), "{why}");
            assert!(!why.contains("roots"), "{why}");
        }
        other => panic!("{other:?}"),
    }
}

/// The security bound: a tag after the last tag of the former signer,
/// signed by the former identity, is refused.
#[tokio::test]
async fn a_later_tag_signed_by_the_former_identity_is_refused() {
    pod_testkit::need_loopback!();
    let stand = moved();
    let fetcher = Fetcher::new(client()).allow_plain_http();
    match signed_or(&fetcher, &moved_source(&stand, Some("v0.76.0")), "0.77.0").await {
        Err(Error::Refused(why)) => {
            assert!(why.contains("PodUtils/podcrates/.github/workflows/release.yml@refs/tags/v0.77.0"), "{why}");
            assert!(why.contains("not https://github.com/PodUtils/podnew/"), "{why}");
            assert!(!why.contains("roots"), "{why}");
        }
        other => panic!("{other:?}"),
    }
    // The control: the same release with the last tag at v0.77.0.
    let source = moved_source(&stand, Some("v0.77.0"));
    signed(&fetcher, &source, &v("0.77.0")).await.expect("v0.77.0 by the former signer up to v0.77.0");
}

async fn signed_or(fetcher: &Fetcher, source: &Source, version: &str) -> Result<Signed, Error> {
    signed(fetcher, source, &v(version)).await
}

/// The hint of newer roots comes only when the chain fails.
#[tokio::test]
async fn only_a_chain_that_fails_gives_the_hint_of_the_roots() {
    pod_testkit::need_loopback!();
    let stand = moved();
    let fetcher = Fetcher::new(client()).allow_plain_http();
    let roots = Roots::sigstore();
    // The root alone, with no intermediate: the leaf's issuer is not it.
    let root_only = Roots { fulcio: roots.fulcio[roots.fulcio.len() - 1..].to_vec(), ..roots };
    let source = Source { roots: root_only, ..moved_source(&stand, Some("v0.76.0")) };
    match signed_or(&fetcher, &source, "0.76.0").await {
        Err(Error::Refused(why)) => {
            assert!(why.contains("was not issued by"), "{why}");
            assert!(why.contains("If Sigstore changed its roots"), "{why}");
        }
        other => panic!("{other:?}"),
    }
    let source = Source { workflow: "ci.yml".into(), ..moved_source(&stand, Some("v0.76.0")) };
    match signed_or(&fetcher, &source, "0.76.0").await {
        Err(Error::Refused(why)) => assert!(!why.contains("roots") && why.contains("ci.yml"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn each_kind_of_refusal_has_its_own_words() {
    let source = Source::github(NEW, "release.yml");
    let chain = refused(&Unverified::Chain("X".into()), &source, "v1.0.0");
    assert!(chain.contains("If Sigstore changed its roots") && chain.contains("/PodUtils/podnew/releases/tag/v1.0.0"));
    let identity = refused(&Unverified::Identity("X".into()), &source, "v1.0.0");
    assert_eq!(
        identity,
        "SHA256SUMS of v1.0.0 is signed by a signer that this program does not accept: X; nothing was replaced"
    );
    let bundle = refused(&Unverified::Bundle("X".into()), &source, "v1.0.0");
    assert_eq!(bundle, "the signature of SHA256SUMS: X; nothing was replaced");
}
