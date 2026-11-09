//! The broker: its settings, its jobs, its way out and its audit, and the
//! listeners of the jobs.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Dial, Failure, Resolve};
use tokio::net::TcpListener;

use crate::audit::{Audit, Line, Tracing};
use crate::deny::DenyList;
use crate::jobs::{JobInfo, Jobs, Load, MAX_TTL};
use crate::tunnel::{Splice, TunnelEnd};

/// The settings of a broker, which the host fills once.
#[derive(Debug, Clone)]
pub struct Settings {
    /// The deny list of the host ([`DenyList::read`] of its deny file).
    pub deny: DenyList,
    /// The addresses of the host: a job reaches none of them.
    pub host_addresses: Vec<IpAddr>,
    /// The longest wait for the head of a request.
    pub head_timeout: Duration,
    /// The longest wait for the one resolve of a name.
    pub resolve_timeout: Duration,
    /// The longest wait for the connection to the far end (502, 504).
    pub connect_timeout: Duration,
    /// The longest TTL of a job.
    pub max_ttl: Duration,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            deny: DenyList::new(),
            host_addresses: Vec::new(),
            head_timeout: Duration::from_secs(30),
            resolve_timeout: Duration::from_secs(10),
            connect_timeout: pod_core::backoff::CONNECT_TIMEOUT,
            max_ttl: MAX_TTL,
        }
    }
}

/// The egress broker of the jobs of one host.
pub struct Broker {
    pub(crate) settings: Settings,
    /// The fixed denies, the addresses of the host and its deny file, in
    /// this order.
    pub(crate) denies: [DenyList; 3],
    pub(crate) jobs: Arc<Jobs>,
    pub(crate) resolve: Arc<dyn Resolve>,
    pub(crate) dial: Arc<dyn Dial>,
    pub(crate) audit: Arc<dyn Audit>,
    pub(crate) end: Arc<dyn TunnelEnd>,
}

impl std::fmt::Debug for Broker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Broker")
            .field("settings", &self.settings)
            .field("resolve", &self.resolve.label())
            .field("dial", &self.dial.label())
            .finish_non_exhaustive()
    }
}

impl Broker {
    /// A broker that resolves each name once with `resolve` and dials each
    /// checked address with `dial` (the upstream chain: [`upstream`]). The
    /// audit goes to `tracing` until [`Broker::with_audit`].
    pub fn new(settings: Settings, resolve: Arc<dyn Resolve>, dial: Arc<dyn Dial>) -> Broker {
        let denies = [DenyList::fixed(), DenyList::host_addresses(&settings.host_addresses), settings.deny.clone()];
        Broker {
            jobs: Jobs::new(settings.max_ttl),
            settings,
            denies,
            resolve,
            dial,
            audit: Arc::new(Tracing),
            end: Arc::new(Splice),
        }
    }

    pub fn with_audit(mut self, audit: Arc<dyn Audit>) -> Broker {
        self.audit = audit;
        self
    }

    /// The end of each allowed tunnel in place of [`Splice`] (T-100: the TLS
    /// end).
    pub fn with_tunnel_end(mut self, end: Arc<dyn TunnelEnd>) -> Broker {
        self.end = end;
        self
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Load a job, in place of a job of the same id (whose connections end).
    /// A bad job id, token, TTL or policy is a failure of the class `Usage`.
    pub fn load(&self, load: Load) -> Result<JobInfo, Failure> {
        let (job, ttl) = (load.job.clone(), load.ttl);
        match self.jobs.load(load) {
            Ok(compiled) => {
                let mut line = Line::new("info", "broker.job.load");
                line.set("job", job.as_str()).set("mode", compiled.mode().word()).set("rules", compiled.rules());
                line.set("ttl_s", ttl.as_secs());
                self.audit.line(&line);
                Ok(JobInfo { job, mode: compiled.mode(), rules: compiled.rules(), expires_in: ttl, open: 0 })
            }
            Err(e) => {
                let mut line = Line::new("warn", "broker.job.load");
                if crate::jobs::check_job_id(&job).is_ok() {
                    line.set("job", job.as_str());
                }
                line.failed(e.stage, e.class);
                self.audit.line(&line);
                Err(e)
            }
        }
    }

    /// End the job `job`: each of its connections ends and its state is
    /// gone. Whether the job was there.
    pub fn drop_job(&self, job: &str) -> bool {
        let dropped = self.jobs.drop_job(job);
        if crate::jobs::check_job_id(job).is_ok() {
            let mut line = Line::new("info", "broker.job.drop");
            line.set("job", job).set("dropped", dropped);
            self.audit.line(&line);
        }
        dropped
    }

    /// Each job, by id.
    pub fn list(&self) -> Vec<JobInfo> {
        self.jobs.list()
    }

    /// Serve the jobs on `listener` (a TCP address on loopback that the host
    /// gives), one task for each connection, until the listener fails.
    pub async fn serve_tcp(self: Arc<Self>, listener: TcpListener) -> std::io::Result<()> {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let _ = stream.set_nodelay(true);
                    let broker = self.clone();
                    tokio::spawn(async move { broker.handle(Box::new(stream)).await });
                }
                Err(e) if passing(&e) => tracing::debug!(error = %e, "an accept failed"),
                Err(e) => return Err(e),
            }
        }
    }

    /// Serve the jobs on the Unix socket `listener` ([`crate::control::bind_unix`]).
    #[cfg(unix)]
    pub async fn serve_unix(self: Arc<Self>, listener: tokio::net::UnixListener) -> std::io::Result<()> {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let broker = self.clone();
                    tokio::spawn(async move { broker.handle(Box::new(stream)).await });
                }
                Err(e) if passing(&e) => tracing::debug!(error = %e, "an accept failed"),
                Err(e) => return Err(e),
            }
        }
    }
}

/// An error of one accept that leaves the listener usable.
fn passing(e: &std::io::Error) -> bool {
    use std::io::ErrorKind as K;
    matches!(e.kind(), K::ConnectionAborted | K::ConnectionReset | K::Interrupted | K::WouldBlock)
}

/// The way out of a broker: through `proxy` (an HTTP CONNECT or a SOCKS5
/// proxy of `pod-proxy`), or direct with no proxy. The broker gives it the
/// checked address, never a name; `resolve` resolves the name of the proxy
/// only. `app` names the program in the words of a refusal.
pub fn upstream(app: &str, proxy: Option<pod_proxy::Proxy>, resolve: Arc<dyn Resolve>) -> Arc<dyn Dial> {
    let choice = match proxy {
        Some(proxy) => pod_proxy::ProxyChoice::Via(proxy),
        None => pod_proxy::ProxyChoice::Direct,
    };
    Arc::new(pod_proxy::Dialer::new(pod_proxy::Settings::new(app, choice), resolve))
}
