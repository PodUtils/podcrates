//! The CNCF distribution registry (`registry serve`) on 127.0.0.1, with a
//! configuration, a store and certificates that the test makes in a
//! scratch directory.
//!
//! The registry is the program `POD_OCI_TEST_REGISTRY_BIN`, else
//! `registry` on the path. A host with neither skips the test with the
//! reason, unless `POD_OCI_TEST_REGISTRY=required`, which
//! `scripts/ci/registry-tests.sh` sets with the pinned release.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use super::tls::Pki;

/// The bound of the start of the registry.
const LIMIT: Duration = Duration::from_secs(30);

/// `POD_OCI_TEST_REGISTRY=required`: a skip is a failure.
pub fn required() -> bool {
    std::env::var("POD_OCI_TEST_REGISTRY").is_ok_and(|v| v == "required")
}

/// The program of distribution that this host has.
fn program() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("POD_OCI_TEST_REGISTRY_BIN").filter(|n| !n.is_empty()) {
        return Some(PathBuf::from(named)).filter(|p| p.is_file());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join("registry")).find(|p| p.is_file())
}

/// How the registry asks for a login.
pub enum Auth {
    /// `auth: htpasswd` with the made-up login: a `Basic` challenge.
    Htpasswd,
    /// `auth: token` at `realm`, signed under the root of the test: a
    /// `Bearer` challenge.
    Token { realm: String },
}

pub struct Registry {
    pub port: u16,
    /// `127.0.0.1:PORT`.
    pub endpoint: String,
    /// The scratch directory: the configuration, the store, the logs and
    /// `anchor.pem`, the root of the test.
    pub dir: PathBuf,
    child: Child,
}

impl Registry {
    /// The registry started, or `None` when this host has none. `tls`:
    /// HTTPS with the leaf of `pki`; else plain HTTP.
    pub fn start(tag: &str, auth: Auth, tls: bool, pki: &Pki) -> Option<Registry> {
        let Some(program) = program() else {
            let why = "no distribution registry here (set POD_OCI_TEST_REGISTRY_BIN, or put registry on the path)";
            assert!(!required(), "POD_OCI_TEST_REGISTRY=required: {why}");
            eprintln!("did not run: {why}");
            return None;
        };
        let dir = std::env::temp_dir().join(format!("pod-oci-distribution-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("store")).expect("a scratch directory");
        let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).expect("a file of the registry");
        write("anchor.pem", &pki.anchor_pem());
        write("cert.pem", &pki.leaf_pem());
        write("key.pem", &pki.key_pem());
        write("htpasswd", &format!("{}:{}\n", super::USER, super::PASSWORD_BCRYPT));
        for attempt in 1.. {
            let port = super::free_port();
            let path = |name: &str| dir.join(name).display().to_string();
            let tls_part = if tls {
                format!("  tls:\n    certificate: {}\n    key: {}\n", path("cert.pem"), path("key.pem"))
            } else {
                String::new()
            };
            let auth_part = match &auth {
                Auth::Htpasswd => format!("  htpasswd:\n    realm: pod-oci-test\n    path: {}\n", path("htpasswd")),
                Auth::Token { realm } => format!(
                    "  token:\n    realm: {realm}\n    service: {}\n    issuer: {}\n    rootcertbundle: {}\n",
                    super::SERVICE,
                    super::ISSUER,
                    path("anchor.pem")
                ),
            };
            // No cache of descriptors: a test changes a blob in the store, and
            // the registry must read the store.
            let config = format!(
                "version: 0.1\nlog:\n  level: info\n  accesslog:\n    disabled: true\nstorage:\n  filesystem:\n    \
                 rootdirectory: {store}\nhttp:\n  addr: {at}:{port}\n{tls_part}\
                 auth:\n{auth_part}",
                store = path("store"),
                at = super::AT,
            );
            write("config.yml", &config);
            let child = Command::new(&program)
                .arg("serve")
                .arg(dir.join("config.yml"))
                .current_dir(&dir)
                .env_remove("REGISTRY_HTTP_ADDR")
                .stdin(Stdio::null())
                .stdout(std::fs::File::create(dir.join("registry.out")).unwrap())
                .stderr(std::fs::File::create(dir.join("registry.err")).unwrap())
                .spawn()
                .expect("the registry starts");
            let mut child = child;
            // A free port can go to a parallel test before the registry binds
            // it; the registry then ends, and a new port is taken.
            if listens(&mut child, port, &dir) {
                return Some(Registry { port, endpoint: format!("{}:{port}", super::AT), dir, child });
            }
            assert!(attempt < 3, "the registry did not listen: {}", said(&dir));
        }
        unreachable!()
    }

    /// What the registry wrote, for a message.
    pub fn said(&self) -> String {
        said(&self.dir)
    }

    /// The file of the store that holds the blob `digest`
    /// (`sha256:HEX`), in the layout of the filesystem driver.
    pub fn blob_file(&self, digest: &str) -> PathBuf {
        let hex = digest.strip_prefix("sha256:").expect("a SHA-256 digest");
        self.dir.join(format!("store/docker/registry/v2/blobs/sha256/{}/{hex}/data", &hex[..2]))
    }

    /// The root of the test, for `pod_tls::Trust::File`.
    pub fn anchor(&self) -> PathBuf {
        self.dir.join("anchor.pem")
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// What the registry in `dir` wrote.
fn said(dir: &std::path::Path) -> String {
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default();
    format!("{}{}", read("registry.out"), read("registry.err"))
}

/// Wait until the registry says that it listens at `port`, at most
/// [`LIMIT`]. A connect alone can reach another server of a parallel test
/// at the same port. `false`: the registry ended, as its port was taken.
fn listens(child: &mut Child, port: u16, dir: &std::path::Path) -> bool {
    let line = format!("listening on {}:{port}", super::AT);
    let deadline = std::time::Instant::now() + pod_testkit::host::scaled(LIMIT);
    loop {
        if said(dir).contains(&line) {
            return true;
        }
        if child.try_wait().is_ok_and(|status| status.is_some()) {
            return false;
        }
        assert!(std::time::Instant::now() < deadline, "the registry did not listen: {}", said(dir));
        std::thread::sleep(Duration::from_millis(100));
    }
}
