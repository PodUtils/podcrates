//! An upstream proxy that takes names only (T-161). Some proxy-only hosts
//! reset a CONNECT to an IP literal, or have no resolver of their own that
//! works: there the broker sends the checked name, not the checked
//! address ([`crate::Settings::names_upstream`]). The broker still checks
//! the name against each deny and resolves it once, and each address
//! passes each deny; but the upstream resolves the name again, and can
//! reach another address. So the broker asks the upstream at the start
//! whether it refuses a name of a non-public address (the canary): an
//! upstream that takes one holds a job as `advisory` only, and a consumer
//! serves no untrusted job through it.
//!
//! An allow rule under a suffix where anyone can register a name (the
//! pages, the user content and the apps of a shared host) reaches other
//! parties: the policy keeps it, and the load flags it.

use pod_core::{Class, Target};
use tokio::time::Instant;

use crate::audit::Line;
use crate::broker::Broker;

/// Each suffix under which anyone can register a name or put content: a
/// subset of the private section of the Public Suffix List, and the hosts
/// of user content of GitHub and of the stores of the clouds.
pub const OPEN_SUFFIXES: &[&str] = &[
    "github.io",
    "githubusercontent.com",
    "gitlab.io",
    "pages.dev",
    "workers.dev",
    "vercel.app",
    "netlify.app",
    "herokuapp.com",
    "fly.dev",
    "onrender.com",
    "railway.app",
    "deno.dev",
    "web.app",
    "firebaseapp.com",
    "appspot.com",
    "azurewebsites.net",
    "core.windows.net",
    "cloudfront.net",
    "s3.amazonaws.com",
    "ngrok.io",
    "ngrok-free.app",
    "trycloudflare.com",
    "glitch.me",
    "surge.sh",
    "readthedocs.io",
    "blogspot.com",
];

/// The suffix of [`OPEN_SUFFIXES`] that holds the host of a rule (`*.x.y`
/// or `x.y`), if one does.
pub fn open_suffix(host: &str) -> Option<&'static str> {
    let name = host.strip_prefix("*.").unwrap_or(host).trim_end_matches('.').to_ascii_lowercase();
    OPEN_SUFFIXES.iter().copied().find(|s| name == *s || name.ends_with(&format!(".{s}")))
}

/// What the canary found: does the upstream refuse a name of a non-public
/// address?
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamCheck {
    /// The upstream refused the canary: a name that it resolves to a
    /// non-public address does not pass.
    RefusesNonPublic,
    /// The upstream connected to the canary: a job that sends a name can
    /// reach a non-public address through it. A hold through it is
    /// `advisory`, and it serves no untrusted job.
    TakesNonPublic,
    /// No answer that says either (a timeout, an upstream that is down).
    Unknown(String),
}

impl UpstreamCheck {
    pub fn word(&self) -> &'static str {
        match self {
            UpstreamCheck::RefusesNonPublic => "refuses-non-public",
            UpstreamCheck::TakesNonPublic => "takes-non-public",
            UpstreamCheck::Unknown(_) => "unknown",
        }
    }
}

impl Broker {
    /// The canary: a CONNECT to `canary` (a name that resolves to a
    /// non-public address, `Settings::canary`) through the way out, which an
    /// upstream that checks the address it reaches refuses. The audit line
    /// `broker.upstream.canary` holds the answer.
    pub async fn check_upstream(&self) -> UpstreamCheck {
        let canary = self.settings.canary.clone();
        let wait = self.settings.connect_timeout;
        let target = Target::Name { host: canary.clone(), port: 443 };
        let dialed = tokio::time::timeout(wait, self.dial.dial(&target, Instant::now() + wait)).await;
        let check = match dialed {
            Ok(Ok(_)) => UpstreamCheck::TakesNonPublic,
            Ok(Err(f)) if matches!(f.class, Class::Refused | Class::Policy | Class::NotFound | Class::Unavailable) => {
                UpstreamCheck::RefusesNonPublic
            }
            Ok(Err(f)) => UpstreamCheck::Unknown(format!("{:?}", f.class)),
            Err(_) => UpstreamCheck::Unknown(format!("no answer in {} s", wait.as_secs())),
        };
        let level = if check == UpstreamCheck::RefusesNonPublic { "info" } else { "warn" };
        let mut line = Line::new(level, "broker.upstream.canary");
        line.set("canary", canary).set("result", check.word());
        self.audit.line(&line);
        check
    }
}

/// The default canary: `localhost` resolves to a loopback address on each
/// resolver.
pub const CANARY: &str = "localhost";
