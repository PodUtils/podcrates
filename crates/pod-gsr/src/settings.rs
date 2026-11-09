//! The facts of the relay as one value.
//!
//! Each value is a fact or an endpoint of the service file
//! `hackerschoice-gsr` of `pod-services` (`crates/pod-services/services/`).
//! A missing or mistyped value is a failure of the class `Config`.

use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use pod_services::Service;

use crate::wire::Version;

/// The id of the relay's data file in `pod-services`.
pub const SERVICE: &str = "hackerschoice-gsr";

/// The name of the endpoint of the relay in its service file.
const ENDPOINT: &str = "default";

/// The most shards that the letters `a` to `z` name.
pub(crate) const MAX_SHARDS: u8 = 26;

/// The endpoint and the facts of one relay.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The domain of the relay. The shard letter goes in front of it.
    pub domain: String,
    /// The port of the relay.
    pub port: u16,
    /// True when the host of a session is `<shard>.<domain>`.
    pub sharded: bool,
    /// The number of shards, from 1 to 26.
    pub shards: u8,
    /// The key of the address derivation.
    pub addr_key: String,
    /// The key of the SRP password derivation.
    pub srp_key: String,
    /// The number of hex characters that the SRP password keeps.
    pub password_chars: usize,
    /// The SRP user name.
    pub srp_user: String,
    /// The SRP group, as OpenSSL names it.
    pub srp_group: String,
    /// The TLS cipher suite.
    pub cipher: String,
    /// The text before the SRP password.
    pub secret_prefix: String,
    /// The text after the SRP password.
    pub secret_suffix: String,
    /// The longest secret that the service takes, in bytes.
    pub secret_max: usize,
    /// The version of the control protocol.
    pub version: Version,
    /// The time between two `PING` messages while a peer waits.
    ///
    /// The service gives its own interval, and it is the right one only when
    /// nothing sits between this host and the relay. A proxy that ends an idle
    /// tunnel decides the real limit: one on the test host ends a `CONNECT`
    /// tunnel after 15 s with no traffic, while the service's interval is 45 s.
    /// The ping therefore comes at the shorter of the two, so that a listener
    /// behind such a proxy keeps its registration.
    pub ping: Duration,
    /// The time after which the relay ends a registration with no traffic
    /// (a measurement, `idle-timeout-ms`). The crate does not use it: a
    /// caller sizes its own keepalive from it.
    pub idle: Duration,
}

impl Settings {
    /// The settings of the relay, from the built-in data file
    /// `hackerschoice-gsr` of `pod-services`.
    pub fn of_data() -> Result<Settings, Failure> {
        let service =
            pod_services::get(SERVICE).ok_or_else(|| config(format!("pod-services holds no service {SERVICE:?}")))?;
        Settings::of(service)
    }

    /// The settings of `service`.
    pub fn of(service: &Service) -> Result<Settings, Failure> {
        let endpoint = service
            .endpoint(ENDPOINT)
            .ok_or_else(|| config(format!("the service {:?} has no endpoint {ENDPOINT:?}", service.id())))?;
        if endpoint.scheme() != "tcp" {
            return Err(config(format!(
                "the endpoint {ENDPOINT:?} is {}, and the relay speaks tcp",
                endpoint.scheme()
            )));
        }
        let shards = byte(service, "shard-count")?;
        if !(1..=MAX_SHARDS).contains(&shards) {
            return Err(config(format!(
                "the fact \"shard-count\" is {shards}, and the letters a to z name 1 to {MAX_SHARDS} shards"
            )));
        }
        let password_chars = positive(service, "srp-password-hex-chars")?;
        if password_chars > 64 {
            return Err(config(format!(
                "the fact \"srp-password-hex-chars\" is {password_chars}, and a SHA-256 digest has 64"
            )));
        }
        Ok(Settings {
            domain: endpoint.host().to_string(),
            port: endpoint.port(),
            sharded: true,
            shards,
            addr_key: text(service, "addr-key")?,
            srp_key: text(service, "srp-key")?,
            password_chars,
            srp_user: text(service, "srp-user")?,
            srp_group: text(service, "srp-group")?,
            cipher: text(service, "cipher")?,
            secret_prefix: text(service, "secret-prefix")?,
            secret_suffix: text(service, "secret-suffix")?,
            secret_max: positive(service, "secret-max-bytes")?,
            version: Version { major: byte(service, "protocol-major")?, minor: byte(service, "protocol-minor")? },
            ping: ping_of(service)?,
            idle: millis(service, "idle-timeout-ms")?,
        })
    }
}

fn config(why: impl Into<String>) -> Failure {
    Failure::new(Stage::Relay, Class::Config, why)
}

fn text(service: &Service, name: &str) -> Result<String, Failure> {
    service
        .text(name)
        .map(str::to_string)
        .ok_or_else(|| config(format!("the fact {name:?} is missing or is not a text")))
}

fn number(service: &Service, name: &str) -> Result<i64, Failure> {
    service.int(name).ok_or_else(|| config(format!("the fact {name:?} is missing or is not an integer")))
}

fn byte(service: &Service, name: &str) -> Result<u8, Failure> {
    let value = number(service, name)?;
    u8::try_from(value).map_err(|_| config(format!("the fact {name:?} is {value}, and not a byte")))
}

fn positive(service: &Service, name: &str) -> Result<usize, Failure> {
    let value = number(service, name)?;
    match usize::try_from(value) {
        Ok(value) if value > 0 => Ok(value),
        _ => Err(config(format!("the fact {name:?} is {value}, and it must be a positive number"))),
    }
}

/// The shortest interval that a `PING` may come at.
///
/// A `PING` costs one message each way, so a value far below the service's own
/// interval would be waste; a value above the shortest idle cut of an
/// intermediary loses the registration.
pub const MIN_PING: Duration = Duration::from_secs(5);

/// The interval of a `PING`: the service's own, and no more than [`MIN_PING`]
/// unless the service asks for more.
fn ping_of(service: &Service) -> Result<Duration, Failure> {
    let own = millis(service, "ping-interval-ms")?;
    Ok(own.min(MIN_PING))
}

fn millis(service: &Service, name: &str) -> Result<Duration, Failure> {
    let value = number(service, name)?;
    match u64::try_from(value) {
        Ok(value) if value > 0 => Ok(Duration::from_millis(value)),
        _ => Err(config(format!("the fact {name:?} is {value}, and it must be a positive number of milliseconds"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pod_services::Catalog;

    fn service() -> &'static Service {
        pod_services::get(SERVICE).expect("the service of the data file")
    }

    /// The data file with one fact changed, as a service of an overlay.
    fn with_fact(name: &str, value: &str) -> Catalog {
        // Windows checks out CRLF; the search below needs LF.
        let text = include_str!("../../pod-services/services/hackerschoice-gsr.toml").replace("\r\n", "\n");
        let start = text.find(&format!("name = \"{name}\"\nvalue = ")).expect("the fact");
        let line_end = start + text[start..].find("\nvalue = ").expect("its value") + 1;
        let value_end = line_end + text[line_end..].find('\n').expect("the end of the line");
        let changed = format!("{}value = {value}{}", &text[..line_end], &text[value_end..]);
        let mut catalog = Catalog::new();
        catalog.lay("changed.toml", &changed).expect("a valid file");
        catalog
    }

    #[test]
    fn the_settings_are_the_facts_of_the_service() {
        let settings = Settings::of(service()).expect("its facts");
        assert_eq!(settings.domain, "gs.thc.org");
        assert_eq!(settings.port, 443);
        assert_eq!(settings.shards, 26);
        assert_eq!(settings.addr_key, "/kd/addr/2");
        assert_eq!(settings.srp_key, "/kd/srp/1");
        assert_eq!(settings.password_chars, 33);
        assert_eq!(settings.srp_user, "user");
        assert_eq!(settings.srp_group, "4096");
        assert_eq!(settings.cipher, "SRP-AES-256-CBC-SHA");
        assert_eq!(settings.secret_prefix, "Blah.");
        assert_eq!(settings.secret_suffix, ".blubb-SRPSEC");
        assert_eq!(settings.secret_max, 32);
        assert_eq!(settings.version, Version { major: 1, minor: 3 });
        assert_eq!(settings.ping, MIN_PING, "the service asks for 45 s, and an idle proxy cut decides");
        assert_eq!(settings.idle, Duration::from_millis(130_000));
    }

    #[test]
    fn a_shard_count_that_no_letter_names_is_a_configuration_failure() {
        for count in ["0", "27", "255"] {
            let catalog = with_fact("shard-count", count);
            let failure = Settings::of(catalog.get(SERVICE).expect("the service")).expect_err(count);
            assert_eq!((failure.stage, failure.class), (Stage::Relay, Class::Config), "{count}");
            assert!(failure.text().contains("shard-count"), "{}", failure.text());
        }
        let catalog = with_fact("shard-count", "26");
        assert_eq!(Settings::of(catalog.get(SERVICE).expect("the service")).expect("26").shards, 26);
    }

    #[test]
    fn the_ping_is_never_slower_than_a_proxy_idle_cut() {
        // The service asks for 45 s. A proxy on the test host ends an idle
        // tunnel after 15 s, so the settings must shorten it.
        let settings = Settings::of_data().expect("the data file");
        let own = settings.ping;
        assert_eq!(own, MIN_PING, "the service asks for more than MIN_PING");
        assert!(millis(service(), "ping-interval-ms").expect("the fact") > MIN_PING);
    }

    #[test]
    fn a_service_with_no_default_endpoint_is_a_configuration_failure() {
        let failure = Settings::of(pod_services::get("cloudflare-doh").expect("the service")).expect_err("no endpoint");
        assert_eq!((failure.stage, failure.class), (Stage::Relay, Class::Config));
    }

    #[test]
    fn a_service_with_no_facts_is_a_configuration_failure() {
        const STAND_IN: &str = r#"
id = "stand-in"
kind = "gsr-relay"
party = "a test"
about = "a relay with no facts"

[[endpoint]]
name = "default"
scheme = "tcp"
host = "127.0.0.1"
port = 1
source = { ruling = "a test", read = 2026-11-06 }

[[watch]]
kind = "rdap"
domain = "example.invalid"
"#;
        let mut catalog = Catalog::new();
        catalog.lay("stand-in.toml", STAND_IN).expect("a valid file");
        let failure = Settings::of(catalog.get("stand-in").expect("the service")).expect_err("no facts");
        assert_eq!(failure.class, Class::Config);
        assert!(failure.text().contains("shard-count"), "{}", failure.text());
    }
}
