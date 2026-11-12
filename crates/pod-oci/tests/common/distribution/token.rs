//! The token server of the test: the realm of a registry with `auth:
//! token`. It answers a GET over TLS with a JWT (ES256, the leaf in its
//! `x5c` header) for the scopes of the query, and only for the made-up
//! login: an ask with no login or with another one gets 401, as a real
//! token server answers. It keeps each ask, so a test can read the scope
//! and the login that the client sent.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use p256::ecdsa::SigningKey;
use p256::ecdsa::signature::Signer;

use super::tls::Pki;

/// One ask of a token.
#[derive(Clone, Debug)]
pub struct Asked {
    pub path: String,
    /// Whether the ask carried the made-up login.
    pub login: bool,
}

pub struct TokenServer {
    pub port: u16,
    pub asked: Arc<Mutex<Vec<Asked>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl TokenServer {
    pub fn start(pki: &Pki) -> TokenServer {
        let listener = TcpListener::bind((super::AT, 0)).expect("the token server binds the loopback");
        listener.set_nonblocking(true).expect("the accept loop polls");
        let port = listener.local_addr().unwrap().port();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let server = pki.server_config();
        let signer = Arc::new((pki.key.clone(), pki.leaf_der.clone()));
        let (keep, halt) = (asked.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            while !halt.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((tcp, _)) => {
                        let (server, signer, keep) = (server.clone(), signer.clone(), keep.clone());
                        std::thread::spawn(move || serve(tcp, server, &signer, &keep));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => break,
                }
            }
        });
        TokenServer { port, asked, stop, thread: Some(thread) }
    }

    /// The realm that the registry names in its challenge.
    pub fn realm(&self) -> String {
        format!("https://{}:{}/token", super::AT, self.port)
    }

    pub fn asked(&self) -> Vec<Asked> {
        self.asked.lock().unwrap().clone()
    }
}

impl Drop for TokenServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(
    tcp: std::net::TcpStream,
    server: Arc<rustls::ServerConfig>,
    signer: &(SigningKey, Vec<u8>),
    asked: &Mutex<Vec<Asked>>,
) {
    let _ = tcp.set_read_timeout(Some(pod_testkit::host::scaled(Duration::from_secs(10))));
    let Ok(conn) = rustls::ServerConnection::new(server) else { return };
    let mut stream = rustls::StreamOwned::new(conn, tcp);
    let mut head = Vec::new();
    let mut chunk = [0u8; 4096];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => head.extend_from_slice(&chunk[..n]),
        }
    }
    let text = String::from_utf8_lossy(&head).into_owned();
    let path = text.split_whitespace().nth(1).unwrap_or("").to_string();
    let login = text.lines().any(|l| {
        l.split_once(':').is_some_and(|(k, v)| {
            k.trim().eq_ignore_ascii_case("Authorization") && v.trim() == super::basic(super::USER, super::PASSWORD)
        })
    });
    asked.lock().unwrap().push(Asked { path: path.clone(), login });
    let (status, body) = if !login {
        ("401 Unauthorized", r#"{"errors":[{"code":"UNAUTHORIZED","message":"no login"}]}"#.to_string())
    } else if super::query_values(&path, "service") != [super::SERVICE] {
        ("400 Bad Request", r#"{"errors":[{"code":"DENIED","message":"another service"}]}"#.to_string())
    } else {
        let access: Vec<(String, Vec<String>)> = super::query_values(&path, "scope")
            .iter()
            .flat_map(|s| s.split(' ').map(str::to_string).collect::<Vec<_>>())
            .filter_map(|scope| {
                let mut parts = scope.splitn(3, ':');
                match (parts.next(), parts.next(), parts.next()) {
                    (Some("repository"), Some(name), Some(actions)) => {
                        Some((name.to_string(), actions.split(',').map(str::to_string).collect()))
                    }
                    _ => None,
                }
            })
            .collect();
        let token = mint(&signer.0, &signer.1, &access);
        ("200 OK", serde_json::json!({ "token": token, "expires_in": 300 }).to_string())
    };
    let answer = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(answer.as_bytes());
    stream.conn.send_close_notify();
    let _ = stream.flush();
}

/// A JWT of [`super::ISSUER`] for [`super::SERVICE`] that grants `access`
/// (a repository and its actions), valid for five minutes, signed with
/// `key` (ES256), with `leaf` in its `x5c` header.
pub fn mint(key: &SigningKey, leaf: &[u8], access: &[(String, Vec<String>)]) -> String {
    let b64 = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let header = serde_json::json!({
        "alg": "ES256",
        "typ": "JWT",
        "x5c": [base64::engine::general_purpose::STANDARD.encode(leaf)],
    });
    let access: Vec<_> = access
        .iter()
        .map(|(name, actions)| serde_json::json!({ "type": "repository", "name": name, "actions": actions }))
        .collect();
    let claims = serde_json::json!({
        "iss": super::ISSUER,
        "sub": super::USER,
        "aud": super::SERVICE,
        "exp": now + 300,
        "nbf": now,
        "iat": now,
        "jti": format!("pod-oci-test-{now}"),
        "access": access,
    });
    let signed = format!("{}.{}", b64(header.to_string().as_bytes()), b64(claims.to_string().as_bytes()));
    // JWS takes the two numbers r and s, 32 bytes each, not DER.
    let signature: p256::ecdsa::Signature = key.sign(signed.as_bytes());
    format!("{signed}.{}", b64(&signature.to_bytes()))
}
