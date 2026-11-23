// coturn on the loopback for the tests of `tests/coturn.rs` and
// `tests/coturn_options.rs`: the made-up login of `scripts/ci/turn-tests.sh`,
// the variables that name where coturn listens, and the settings of a
// client of it.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use pod_turn::{Client, Credentials, Event, Settings, Transport};

use super::{LIMIT, Loopback, Shared5Tuple};

/// The made-up login of `scripts/ci/turn-tests.sh`: coturn's `--user` and
/// `--realm`.
pub const USER: &str = "pod-turn-test";
pub const PASSWORD: &str = "made-up-coturn-password";
pub const REALM_NAME: &str = "coturn.test";
/// The name of coturn's certificate.
pub const NAME: &str = "coturn.test";

/// Where coturn listens, the root of its certificate, and where the coturn
/// that redirects each Allocate to it listens (`--alternate-server` and
/// `--tls-alternate-server`).
pub struct Coturn {
    pub udp: SocketAddr,
    pub tls: SocketAddr,
    pub ca: PathBuf,
    pub redirect_udp: SocketAddr,
    pub redirect_tls: SocketAddr,
}

/// The variables that name coturn; each must be set.
const VARIABLES: [&str; 5] = [
    "POD_TURN_TEST_COTURN_UDP",
    "POD_TURN_TEST_COTURN_TLS",
    "POD_TURN_TEST_COTURN_CA",
    "POD_TURN_TEST_COTURN_REDIRECT_UDP",
    "POD_TURN_TEST_COTURN_REDIRECT_TLS",
];

/// coturn of the variables, or `None` with a line when they name none.
/// `POD_TURN_TEST_COTURN=required` makes no coturn a failure.
pub fn coturn() -> Option<Coturn> {
    let values: Vec<Option<String>> =
        VARIABLES.iter().map(|name| std::env::var(name).ok().filter(|v| !v.is_empty())).collect();
    let Some(values) = values.into_iter().collect::<Option<Vec<String>>>() else {
        let why = format!("{} do not each name coturn", VARIABLES.join(", "));
        assert!(
            std::env::var("POD_TURN_TEST_COTURN").as_deref() != Ok("required"),
            "POD_TURN_TEST_COTURN=required: no coturn here: {why}"
        );
        println!("skipped: {why}");
        return None;
    };
    let address = |v: &String| v.parse::<SocketAddr>().expect("HOST:PORT of coturn, an IP address");
    Some(Coturn {
        udp: address(&values[0]),
        tls: address(&values[1]),
        ca: PathBuf::from(&values[2]),
        redirect_udp: address(&values[3]),
        redirect_tls: address(&values[4]),
    })
}

/// The plain TCP listener of coturn for the RFC 6062 test, or `None` with a
/// line when the variable names none. `POD_TURN_TEST_COTURN=required`
/// makes no listener a failure.
pub fn coturn_tcp() -> Option<SocketAddr> {
    let value = std::env::var("POD_TURN_TEST_COTURN_TCP").ok().filter(|v| !v.is_empty());
    let Some(value) = value else {
        assert!(
            std::env::var("POD_TURN_TEST_COTURN").as_deref() != Ok("required"),
            "POD_TURN_TEST_COTURN=required: POD_TURN_TEST_COTURN_TCP names no coturn"
        );
        println!("skipped: POD_TURN_TEST_COTURN_TCP names no coturn");
        return None;
    };
    Some(value.parse::<SocketAddr>().expect("HOST:PORT of coturn, an IP address"))
}

pub async fn within<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(pod_testkit::host::scaled(LIMIT), future).await.expect("the test's bound")
}

/// The settings of a client over UDP through `socket`, with the realm that
/// coturn must offer.
pub fn udp_settings(coturn: &Coturn, socket: &Arc<pod_core::SystemDatagrams>, user: &str, password: &str) -> Settings {
    let source = Arc::new(Shared5Tuple(socket.clone()));
    let mut settings = Settings::udp(pod_core::Target::Addr(coturn.udp), Credentials::new(user, password), source);
    settings.realm = Some(REALM_NAME.into());
    settings.software = Some("pod-turn-coturn-test".into());
    settings
}

/// The settings of a client over TLS: the loopback dial, and the root of
/// the script alone, or the default roots.
pub fn tls_settings(coturn: &Coturn, own_root: bool) -> Settings {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("pod-turn-test")));
    let target = pod_core::Target::Addr(coturn.tls);
    let mut settings = Settings::tls(target, Credentials::new(USER, PASSWORD), Arc::new(Loopback), Arc::new(context));
    if let Transport::Tls { trust, server_name, .. } = &mut settings.transport {
        if own_root {
            *trust = pod_tls::Trust::File(coturn.ca.clone());
        }
        *server_name = NAME.into();
    }
    settings.realm = Some(REALM_NAME.into());
    settings
}

pub async fn bound_socket() -> Arc<pod_core::SystemDatagrams> {
    Arc::new(pod_core::SystemDatagrams::bind("127.0.0.1:0".parse().unwrap()).await.unwrap())
}

/// The next event must be `echo:` and `sent` from `peer`, on `channel`.
pub async fn echoed(client: &mut Client, peer: SocketAddr, channel: Option<u16>, sent: &[u8]) {
    match within(client.recv()).await.unwrap() {
        Event::Data { peer: from, channel: on, data } => {
            assert_eq!((from, on), (peer, channel));
            assert_eq!(data, [b"echo:".as_slice(), sent].concat());
        }
        other => panic!("{other:?}"),
    }
}
