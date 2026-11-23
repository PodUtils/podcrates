//! The enrolment of a device with a provider (PodUtils/podnet#486, QUIC-4
//! and QUIC-5): a trait, one adapter for Cloudflare WARP (the calls of
//! [`crate::register`]) and one for a local or static provider.
//!
//! The trait keeps an enrolment failure apart from a transport failure:
//! [`EnrolError`] names the provider's refusal, its limit of new devices,
//! its malformed answer, or a failure of the API that is not the
//! transport of the session. The client of a session takes the doors and
//! the pinned key of an [`Enrolled`], so it is bound to no provider.
//!
//! A provider that keeps no device record (a local door, a test edge)
//! gives its doors and its pinned key alone; [`crate::Warp::device`] is
//! then empty, and the account calls of the WARP API are `Usage`.

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{BoxFuture, Class, Failure, Stage, Target};

use crate::api::{Api, api_status};
use crate::device::Device;
use crate::hints::DeviceHints;
use crate::identity::{Identity, spki_from_pem};
use crate::register;

/// What a provider gives a client after an enrolment.
#[derive(Clone, Debug)]
pub struct Enrolled {
    /// The name of the provider.
    pub provider: String,
    /// The front doors, in order.
    pub doors: Vec<Target>,
    /// The key of a front door that the client pins, as an X.509 SPKI in
    /// DER.
    pub pin: Vec<u8>,
    /// The device record, for a provider that keeps one.
    pub device: Option<Device>,
}

impl Enrolled {
    /// The enrolment of a WARP device record: its MASQUE doors, its pinned
    /// peer key, and the record. A record with no door or no peer key is an
    /// enrolment still: the registration succeeded, and the session that
    /// tries to open names the missing part.
    pub fn of_device(provider: &str, device: Device) -> Enrolled {
        let doors: Vec<Target> = device.masque_endpoints().into_iter().map(Target::Addr).collect();
        let pin = device.peer_public_key_pem().and_then(|pem| spki_from_pem(pem).ok()).unwrap_or_default();
        Enrolled { provider: provider.to_string(), doors, pin, device: Some(device) }
    }
}

/// The failure of an enrolment. It is not a transport failure of the
/// session: the names of the two do not mix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnrolError {
    /// The provider refused the device.
    Refused { status: u16, detail: String },
    /// The provider limits new devices from this address (error 1015): the
    /// wait that it asks, and its words.
    Limited { retry_after: Option<Duration>, detail: String },
    /// The answer of the provider does not have the form of an enrolment.
    Protocol(String),
    /// The settings of the adapter do not fit.
    Config(String),
    /// The API of the provider did not answer.
    Unreachable(String),
    /// The API of the provider did not answer: the failure of its path,
    /// with its stage (the proxy, DNS, the connection), is the cause.
    NoAnswer(PathFailure),
}

/// The failure of the path to the API of a provider, shared: an
/// [`EnrolError`] is cloned, and its cause stays whole. Two are equal when
/// their stage, their class and their chain are equal.
#[derive(Clone, Debug)]
pub struct PathFailure(Arc<Failure>);

impl PathFailure {
    pub fn new(failure: Failure) -> PathFailure {
        PathFailure(Arc::new(failure))
    }

    pub fn failure(&self) -> &Failure {
        &self.0
    }

    /// A failure with the stage, the class and the text of the path, and
    /// the shared failure as its cause: a reader of the chain finds the
    /// stage in a `Failure`.
    fn cause(&self) -> Failure {
        let f = &self.0;
        Failure::caused(f.stage, f.class, f.text(), self.0.clone())
    }
}

impl PartialEq for PathFailure {
    fn eq(&self, other: &PathFailure) -> bool {
        let (a, b) = (&self.0, &other.0);
        (a.stage, a.class, a.chain()) == (b.stage, b.class, b.chain())
    }
}

impl Eq for PathFailure {}

impl EnrolError {
    /// The class of the failure, in the classes of this crate.
    pub fn class(&self) -> Class {
        match self {
            EnrolError::Refused { .. } => Class::Refused,
            EnrolError::Limited { .. } => Class::TempFail,
            EnrolError::Protocol(_) => Class::Protocol,
            EnrolError::Config(_) => Class::Config,
            EnrolError::Unreachable(_) | EnrolError::NoAnswer(_) => Class::Unavailable,
        }
    }

    /// The words of the failure, with the prefix of an enrolment.
    pub fn text(&self) -> String {
        match self {
            EnrolError::Refused { status, detail } => {
                format!("the provider refused the enrolment (HTTP {status}): {detail}")
            }
            EnrolError::Limited { retry_after, detail } => match retry_after {
                Some(wait) => format!(
                    "the provider limits new devices for each address ({}); retry in {} s: {detail}",
                    self.class().name(),
                    wait.as_secs()
                ),
                None => format!("the provider limits new devices for each address: {detail}"),
            },
            EnrolError::Protocol(text) => format!("the provider's enrolment answer: {text}"),
            EnrolError::Config(text) => text.clone(),
            EnrolError::Unreachable(text) => format!("the provider's API did not answer: {text}"),
            EnrolError::NoAnswer(path) => format!("the provider's API did not answer: {}", path.failure().text()),
        }
    }

    /// The failure of a session that needs the enrolment, in the type of the
    /// crate.
    pub fn failure(&self) -> Failure {
        match self {
            EnrolError::NoAnswer(path) => Failure::caused(Stage::Warp, self.class(), self.text(), path.cause()),
            _ => Failure::new(Stage::Warp, self.class(), self.text()),
        }
    }
}

impl fmt::Display for EnrolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text())
    }
}

impl std::error::Error for EnrolError {}

/// A provider that enrols a device, and the doors and key that it gives.
pub trait Enrolment: Send + Sync {
    /// The name of the provider (`cloudflare`, a local name).
    fn provider(&self) -> String;

    /// Enrol `identity`, and give its doors and the key to pin.
    fn enrol<'a>(
        &'a self,
        identity: &'a Identity,
        hints: &'a DeviceHints,
    ) -> BoxFuture<'a, Result<Enrolled, EnrolError>>;
}

/// The adapter of Cloudflare WARP: the anonymous registration and the
/// enrol of the P-256 key, through the API of the caller.
#[derive(Clone)]
pub struct Cloudflare {
    pub api: Api,
}

impl Cloudflare {
    pub fn new(api: Api) -> Cloudflare {
        Cloudflare { api }
    }
}

impl Enrolment for Cloudflare {
    fn provider(&self) -> String {
        "cloudflare".into()
    }

    fn enrol<'a>(
        &'a self,
        identity: &'a Identity,
        hints: &'a DeviceHints,
    ) -> BoxFuture<'a, Result<Enrolled, EnrolError>> {
        Box::pin(async move {
            hints.check().map_err(|e| EnrolError::Config(e.text().to_string()))?;
            let device = register::create_device(&self.api, hints).await.map_err(of_api)?;
            if device.token.is_empty() {
                return Err(EnrolError::Protocol("the registration answer carried no token".into()));
            }
            let mut enrolled =
                register::enrol_masque(&self.api, &device.id, &device.token, &identity.spki_b64(), hints.name.clone())
                    .await
                    .map_err(of_api)?;
            enrolled.token = device.token;
            if enrolled.tunnel_type != "masque" {
                return Err(EnrolError::Protocol(format!(
                    "the API kept tunnel_type {:?} in place of masque",
                    enrolled.tunnel_type
                )));
            }
            Ok(Enrolled::of_device("cloudflare", enrolled))
        })
    }
}

/// The adapter of a local or static provider: the doors and the pinned key
/// of the caller, with no registration and no device record.
#[derive(Clone, Debug)]
pub struct Static {
    /// The name of the provider, for a log line and for `warp status`.
    pub name: String,
    pub doors: Vec<Target>,
    /// The key of the front door, as an X.509 SPKI in DER.
    pub pin: Vec<u8>,
}

impl Static {
    /// The provider `name` with its doors and its pinned key. A door of a
    /// name is allowed: the caller's dial resolves it.
    pub fn new(name: &str, doors: Vec<Target>, pin: Vec<u8>) -> Result<Static, EnrolError> {
        if name.trim().is_empty() {
            return Err(EnrolError::Config("a provider of the enrolment has no name".into()));
        }
        if doors.is_empty() {
            return Err(EnrolError::Config(format!("the provider {name} has no front door")));
        }
        if pin.is_empty() {
            return Err(EnrolError::Config(format!("the provider {name} has no key to pin")));
        }
        Ok(Static { name: name.to_string(), doors, pin })
    }

    /// [`Static::new`] with an address in place of a target.
    pub fn at(name: &str, door: SocketAddr, pin: Vec<u8>) -> Result<Static, EnrolError> {
        Static::new(name, vec![Target::Addr(door)], pin)
    }
}

impl Enrolment for Static {
    fn provider(&self) -> String {
        self.name.clone()
    }

    fn enrol<'a>(
        &'a self,
        _identity: &'a Identity,
        _hints: &'a DeviceHints,
    ) -> BoxFuture<'a, Result<Enrolled, EnrolError>> {
        Box::pin(async move {
            Ok(Enrolled { provider: self.name.clone(), doors: self.doors.clone(), pin: self.pin.clone(), device: None })
        })
    }
}

/// The failure of an API call as an enrolment failure, by its status: a
/// 1015 or a 429 is the provider's limit; another 4xx is a refusal; no
/// status is the API, not the enrolment: an answer that does not decode is
/// the protocol, and no answer keeps the failure of the path.
fn of_api(failure: Failure) -> EnrolError {
    let detail = failure.text().to_string();
    match api_status(&failure) {
        Some(429) => EnrolError::Limited { retry_after: None, detail },
        Some(status @ 400..=499) => EnrolError::Refused { status, detail },
        Some(status @ 500..=599) => EnrolError::Unreachable(format!("HTTP {status}: {detail}")),
        _ if failure.class == Class::TempFail => EnrolError::Limited { retry_after: None, detail },
        _ if failure.class == Class::Protocol => EnrolError::Protocol(detail),
        _ => EnrolError::NoAnswer(PathFailure::new(failure)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn identity() -> Identity {
        Identity::generate().unwrap()
    }

    /// A provider of a local door passes the same enrolment as the WARP
    /// adapter, and keeps no device record.
    #[tokio::test]
    async fn a_static_provider_gives_its_door_and_key() {
        let pin = identity().spki_der().to_vec();
        let provider = Static::at("edge.test", "127.0.0.1:9443".parse().unwrap(), pin.clone()).unwrap();
        assert_eq!(provider.provider(), "edge.test");
        let enrolled = provider.enrol(&identity(), &DeviceHints::default()).await.unwrap();
        assert_eq!(enrolled.provider, "edge.test");
        assert_eq!(enrolled.doors, vec![Target::Addr("127.0.0.1:9443".parse().unwrap())]);
        assert_eq!(enrolled.pin, pin);
        assert!(enrolled.device.is_none());
        // A provider without a door, a key or a name is a configuration
        // failure, named apart from the transport.
        let empty = Static::new("edge.test", vec![], pin.clone()).unwrap_err();
        assert_eq!(empty.class(), Class::Config);
        assert!(empty.text().contains("no front door"), "{}", empty.text());
        assert!(Static::new("", vec![Target::Addr("127.0.0.1:1".parse().unwrap())], pin.clone()).is_err());
        assert!(Static::at("edge.test", "127.0.0.1:1".parse().unwrap(), vec![]).is_err());
    }

    /// An API answer of a status maps to the named failures of an
    /// enrolment, not to the failure of a session's transport.
    #[test]
    fn the_failures_of_an_enrolment_are_named() {
        let refused = EnrolError::Refused { status: 403, detail: "no".into() };
        assert_eq!(refused.class(), Class::Refused);
        assert!(refused.text().contains("HTTP 403"), "{}", refused.text());
        let limited = EnrolError::Limited { retry_after: Some(Duration::from_secs(120)), detail: "1015".into() };
        assert_eq!(limited.class(), Class::TempFail);
        assert!(limited.text().contains("120 s"), "{}", limited.text());
        let protocol = EnrolError::Protocol("no door".into());
        assert_eq!(protocol.class(), Class::Protocol);
        let unreachable = EnrolError::Unreachable("no answer".into());
        assert_eq!(unreachable.failure().class, Class::Unavailable);
        assert!(unreachable.failure().chain().contains("did not answer"));
        assert_eq!(EnrolError::Config("x".into()).class(), Class::Config);
        // The text of the failure of a session is the text of the
        // enrolment, and never the word of a transport stage.
        let failure = refused.failure();
        assert_eq!(failure.stage, Stage::Warp);
        assert!(!failure.chain().contains("QUIC"), "{}", failure.chain());
    }

    #[test]
    fn the_api_status_maps_to_the_named_failure() {
        let api = |status: u16| {
            let failure = crate::api::ApiError { status, body: "body".into() }.failure("POST /reg");
            of_api(failure)
        };
        assert!(matches!(api(429), EnrolError::Limited { .. }));
        assert!(matches!(api(403), EnrolError::Refused { status: 403, .. }));
        assert!(matches!(api(503), EnrolError::Unreachable(_)));
        let transport = Failure::new(Stage::Connect, Class::Timeout, "no TCP");
        assert!(matches!(of_api(transport), EnrolError::NoAnswer(_)));
    }

    /// An enrolment through a dead proxy keeps the failure of the proxy, with
    /// its stage, in the chain; an answer that does not decode is the
    /// protocol (PodUtils/podnet#707, ask 9).
    #[test]
    fn an_enrolment_through_a_dead_proxy_keeps_the_stage_of_the_proxy() {
        let dial = pod_proxy::DialError::ProxyUnreachable {
            proxy: "127.0.0.1:1".into(),
            detail: "Connection refused (os error 111)".into(),
        };
        let text = format!("POST https://api.test/v0a/reg: {dial}");
        let path = Failure::caused(Stage::Proxy, Class::Unavailable, text, dial);
        let enrol = of_api(path);
        assert_eq!(enrol.clone(), enrol);
        let failure = enrol.failure();
        assert_eq!((failure.stage, failure.class), (Stage::Warp, Class::Unavailable));
        let mut stages = Vec::new();
        let mut next: Option<&(dyn std::error::Error + 'static)> = Some(&failure);
        while let Some(e) = next {
            stages.extend(e.downcast_ref::<Failure>().map(|f| f.stage));
            next = e.source();
        }
        assert_eq!(stages, [Stage::Warp, Stage::Proxy], "{}", failure.chain());
        let chain = failure.chain();
        assert_eq!(chain.matches("127.0.0.1:1").count(), 1, "{chain}");
        assert!(chain.starts_with("the provider's API did not answer: POST https://api.test/v0a/reg"), "{chain}");
        let answer = Failure::new(Stage::Warp, Class::Protocol, "POST /reg: the WARP API answer does not decode");
        assert!(matches!(of_api(answer), EnrolError::Protocol(_)));
    }
}
