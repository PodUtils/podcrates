//! The registration against the stand-in API on the loopback, through the
//! HTTP client of the caller: the bases of the API in turn, the two calls,
//! the identity file, a refused identity, the limit of new devices, and
//! the account calls.

mod common;

use std::time::Duration;

use pod_core::{Class, Report};
use pod_proxy::client::Request;
use pod_testkit::stand::{self, Answer};
use pod_warp::store::{self, IdentityFile};
use pod_warp::{DeviceHints, Warp};

/// An API on the loopback that answers each request with `status`.
fn answering(status: u16) -> String {
    let stand = stand::http(move |_| Answer::new(status, "{}")).unwrap();
    format!("http://{}", stand.addr())
}

/// A base where nothing listens.
fn dead_base() -> String {
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", dead.local_addr().unwrap());
    drop(dead);
    base
}

/// The stand-in API and its base.
fn stand_in() -> (stand::warp::Api, String) {
    let api = stand::warp::Api::new(
        "-----BEGIN PUBLIC KEY-----\nAA==\n-----END PUBLIC KEY-----\n",
        "192.0.2.1",
        "2001:db8::1",
    );
    let served = stand::http(api.handler()).unwrap();
    (api, format!("http://{}", served.addr()))
}

/// A request of the API for `send`, as the crate makes one.
fn get() -> Request {
    let url = format!("{}/{}/reg/x", pod_warp::service::api_base(), pod_warp::service::api_version());
    Request::new("GET", url).timeout(Duration::from_secs(5))
}

/// The second base answers after a dial failure or a 5xx of the first; a
/// 4xx of the first is its answer.
#[tokio::test]
async fn the_second_api_host_answers_after_a_failure() {
    pod_testkit::need_loopback!();
    let (good, busy, refused, dead) = (answering(200), answering(503), answering(403), dead_base());
    let api = |bases: &[&String]| common::api("").with_bases(bases.iter().map(|b| b.to_string()).collect());
    let answer = api(&[&dead, &good]).send(get()).await.unwrap();
    assert_eq!(answer.status, 200);
    let answer = api(&[&busy, &good]).send(get()).await.unwrap();
    assert_eq!(answer.status, 200);
    let answer = api(&[&refused, &good]).send(get()).await.unwrap();
    assert_eq!(answer.status, 403, "a 4xx is an answer");
    let answer = api(&[&good, &busy]).send(get()).await.unwrap();
    assert_eq!(answer.status, 200);
    let err = api(&[&busy, &dead]).send(get()).await.expect_err("each base failed");
    assert!(err.chain().contains("HTTP 503"), "{}", err.chain());
}

// PodUtils/podnet@T-241: the registration POST may have made a device. Any
// answer is final; only a failure with no answer goes to the second host.
#[tokio::test]
async fn a_503_on_the_post_is_not_sent_to_the_second_host() {
    pod_testkit::need_loopback!();
    let (good, busy, dead) = (answering(200), answering(503), dead_base());
    let post = || {
        let url = format!("{}/{}/reg", pod_warp::service::api_base(), pod_warp::service::api_version());
        Request::new("POST", url).timeout(Duration::from_secs(5))
    };
    let api = |bases: Vec<String>| common::api("").with_bases(bases);
    let answer = api(vec![busy, good.clone()]).send(post()).await.unwrap();
    assert_eq!(answer.status, 503, "an answer of the service is final");
    let answer = api(vec![dead, good]).send(post()).await.unwrap();
    assert_eq!(answer.status, 200, "no answer: the second host");
}

/// A device registers with the two calls, as the Android client sends
/// them, and its record is saved with mode 0600; the next run reads the
/// file and makes no second device.
#[tokio::test]
async fn a_device_registers_enrols_and_is_resumed_from_its_file() {
    pod_testkit::need_loopback!();
    let (stand_in, base) = stand_in();
    let dir = common::scratch("register");
    let file = IdentityFile::new(dir.join("warp.json"));
    let (settings, _) = common::settings(&base, common::Moved::new(Vec::new()));
    let warp = Warp::register_or_load(settings.clone(), &file).await.unwrap();
    assert_eq!((stand_in.asked("POST"), stand_in.asked("PATCH")), (1, 1));
    assert_eq!(warp.device().tunnel_type, "masque");
    assert_eq!(warp.device().token, "stand-in-token-1", "the token of the POST is kept");
    assert_eq!(stand_in.enrolled(), [warp.identity().spki_b64()]);
    assert_eq!(warp.device().masque_endpoints().len(), 2);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&file.path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    let again = Warp::register_or_load(settings, &file).await.unwrap();
    assert_eq!(stand_in.asked("POST"), 1, "no second device");
    assert_eq!(again.identity().spki_der(), warp.identity().spki_der());
    assert_eq!(again.device().id, warp.device().id);
}

/// A file that other users can read is refused, and no registration
/// follows it (PodUtils/podnet@T-173).
#[cfg(unix)]
#[tokio::test]
async fn a_loose_identity_leads_to_no_registration() {
    use std::os::unix::fs::PermissionsExt as _;
    pod_testkit::need_loopback!();
    let (stand_in, base) = stand_in();
    let dir = common::scratch("loose");
    let path = dir.join("warp.json");
    std::fs::write(&path, "{}").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let (settings, _) = common::settings(&base, common::Moved::new(Vec::new()));
    let err = Warp::register_or_load(settings, &IdentityFile::new(&path)).await.err().unwrap();
    assert_eq!(err.class.exit_code(), 78, "{}", err.chain());
    assert_eq!(stand_in.asked("POST"), 0);
    assert_eq!(std::fs::read(&path).unwrap(), b"{}");
}

/// PodUtils/podnet@T-173: a file of another user is a `Config` failure
/// (exit 78) and leads to no registration; with the consent of the caller
/// it reads, and nothing writes to it. A real foreign owner needs root: as
/// another user the test has no subject.
#[cfg(unix)]
#[tokio::test]
async fn a_refused_identity_leads_to_no_registration() {
    pod_testkit::need_loopback!();
    let (stand_in, base) = stand_in();
    let dir = common::scratch("foreign");
    let path = dir.join("warp.json");
    let (settings, _) = common::settings(&base, common::Moved::new(Vec::new()));
    let warp = Warp::register_or_load(settings.clone(), &IdentityFile::new(&path)).await.unwrap();
    // SAFETY: geteuid has no precondition.
    if unsafe { libc::geteuid() } != 0 || std::os::unix::fs::chown(&path, Some(54321), None).is_err() {
        return;
    }
    let before = std::fs::read(&path).unwrap();
    let err = Warp::register_or_load(settings.clone(), &IdentityFile::new(&path)).await.err().unwrap();
    assert_eq!(err.class.exit_code(), 78, "{}", err.chain());
    assert_eq!(stand_in.asked("POST"), 1, "no second registration");
    // The lock of the first registration stays; a read of a foreign file
    // takes no lock.
    std::fs::remove_file(dir.join("warp.json.lock")).unwrap();
    let mut file = IdentityFile::new(&path);
    file.foreign = pod_warp::Foreign::Allow;
    let read = Warp::register_or_load(settings, &file).await.unwrap();
    assert_eq!(read.identity().spki_der(), warp.identity().spki_der());
    assert_eq!(std::fs::read(&path).unwrap(), before, "nothing writes to a foreign file");
    assert!(!dir.join("warp.json.lock").exists(), "no lock beside a foreign file");
}

// PodUtils/podnet issue 187: a 429 of the registration names the remedy,
// and is a temporary failure (exit 75).
#[tokio::test]
async fn a_429_on_registration_names_the_remedy() {
    pod_testkit::need_loopback!();
    let (stand_in, base) = stand_in();
    stand_in.fail_post(Some(429));
    let (settings, _) = common::settings(&base, common::Moved::new(Vec::new()));
    let err = Warp::register(settings).await.err().unwrap();
    assert_eq!(err.class, Class::TempFail);
    assert!(err.to_string().contains("reuse a saved identity"), "{err}");
    assert_eq!(stand_in.asked("PATCH"), 0);
}

/// The account calls: the account with its license hidden, a license, a
/// new key, and the removal of the device.
#[tokio::test]
async fn the_account_calls_hide_the_license_and_rotate_the_key() {
    pod_testkit::need_loopback!();
    let (stand_in, base) = stand_in();
    let (settings, _) = common::settings(&base, common::Moved::new(Vec::new()));
    let mut warp = Warp::register(settings).await.unwrap();
    let account = warp.account().await.unwrap();
    assert_eq!(account["license"], "<hidden>");
    let err = warp.set_license("short").await.unwrap_err();
    assert_eq!(err.class, Class::Usage, "a key that the API refused: {}", err.chain());
    let account = warp.set_license("abcdefgh-12345678").await.unwrap();
    assert_eq!(account["license"], "<hidden>");
    assert_eq!(pod_warp::identity::warp_plus_label(&account), "yes");
    let old = warp.identity().spki_b64();
    warp.rotate_key().await.unwrap();
    assert_ne!(warp.identity().spki_b64(), old);
    assert_eq!(stand_in.enrolled(), [warp.identity().spki_b64()]);
    warp.delete().await.unwrap();
    assert_eq!(stand_in.devices(), 0);
}

/// The hints go into the bodies: the model and the locale of the POST, the
/// name of the PATCH.
#[tokio::test]
async fn the_hints_of_a_device_go_into_the_bodies() {
    pod_testkit::need_loopback!();
    let api = stand::warp::Api::new("", "192.0.2.1", "");
    let served = stand::http(api.handler()).unwrap();
    let (mut settings, _) = common::settings(&format!("http://{}", served.addr()), common::Moved::new(Vec::new()));
    settings.hints =
        DeviceHints { name: Some("ci-runner-7".into()), model: Some("Linux".into()), locale: Some("de_DE".into()) };
    settings.report = Report::default();
    Warp::register(settings).await.unwrap();
    let all = format!("{}\n{}", api.bodies("POST").join(""), api.bodies("PATCH").join(""));
    for part in [r#""model":"Linux""#, r#""locale":"de_DE""#, r#""name":"ci-runner-7""#, r#""key_type":"secp256r1""#] {
        assert!(all.contains(part), "{part} is not in {all}");
    }
    assert_eq!(store::LOCK_WAIT.as_secs(), 60);
}
