//! `forward-url` against a stand-in SSH forward relay on the loopback (a
//! server of russh): it takes a key or no key, a remote forward and a
//! shell, and writes its banner in chunks, each one message of the channel.
//! With a front, the banner names an HTTPS front of the stand-in for
//! `stand.test`, which gives each request to the client through a channel
//! `forwarded-tcpip`, or answers with a page of its own.

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use pod_services_watch::Finding;
use pod_testkit::stand::{Io, Stand};
use russh::keys::{Algorithm, PrivateKey};
use russh::server::{self, Auth, ChannelOpenHandle, Msg, Session, VerifiedPublicKey};
use russh::{Channel, ChannelId, ChannelMsg, MethodKind, MethodSet};
use tokio::net::TcpListener;
use tokio::sync::{mpsc as tmpsc, oneshot};

use crate::common::{NAME, endpoint, fact, file, one, text, tls_raw};

/// What the front of the stand-in does with a request.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Front {
    /// It gives the request to the client through a channel
    /// `forwarded-tcpip`, and the client's answer back.
    Forward,
    /// It answers with a page of its own.
    Page,
}

/// A request of the front and the way back of its answer.
type Ask = (Vec<u8>, mpsc::Sender<Vec<u8>>);

/// The answer of a front that serves a page of its own.
const PAGE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 19\r\nConnection: close\r\n\r\na page of the relay";

/// What the stand-in does.
#[derive(Clone, Default)]
struct Script {
    /// The banner, one chunk for each message of the channel.
    chunks: Vec<&'static [u8]>,
    /// The end of the shell after the banner.
    close: bool,
    /// The stand-in refuses a key and takes no key.
    no_key: bool,
    /// The stand-in refuses a key and no key, and takes keyboard-interactive
    /// after one round with a prompt and an empty answer (Serveo's
    /// anonymous login).
    interactive: bool,
    /// The stand-in refuses the remote forward.
    no_forward: bool,
    /// The front, and the URL of it at the end of the banner.
    front: Option<Front>,
}

/// What the stand-in saw of the client.
#[derive(Debug, Default)]
struct Seen {
    /// The user of each try of the authentication.
    users: Vec<String>,
    /// The algorithm of the key of the client.
    key: Option<String>,
    /// The address and the port of the remote forward.
    forward: Option<(String, u32)>,
    shell: bool,
    /// The requests that the front took.
    requests: usize,
}

struct Relay {
    script: Script,
    seen: Arc<Mutex<Seen>>,
    /// The port of the front, and its requests for a channel
    /// `forwarded-tcpip`.
    front: Option<(u16, tmpsc::UnboundedReceiver<Ask>)>,
}

impl server::Handler for Relay {
    type Error = russh::Error;

    async fn auth_none(&mut self, user: &str) -> Result<Auth, Self::Error> {
        self.seen.lock().unwrap().users.push(user.to_string());
        Ok(if self.script.no_key { Auth::Accept } else { Auth::reject() })
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        user: &str,
        _: &str,
        response: Option<server::Response<'a>>,
    ) -> Result<Auth, Self::Error> {
        self.seen.lock().unwrap().users.push(user.to_string());
        if !self.script.interactive {
            return Ok(Auth::reject());
        }
        Ok(match response {
            None => Auth::Partial {
                name: "".into(),
                instructions: "an anonymous tunnel".into(),
                prompts: vec![("Press Enter: ".into(), true)].into(),
            },
            Some(mut answers) => {
                if answers.next().is_some_and(|a| a.is_empty()) {
                    Auth::Accept
                } else {
                    Auth::reject()
                }
            }
        })
    }

    async fn auth_publickey(&mut self, user: &str, key: VerifiedPublicKey<'_>) -> Result<Auth, Self::Error> {
        let mut seen = self.seen.lock().unwrap();
        seen.users.push(user.to_string());
        seen.key = Some(key.algorithm.to_string());
        Ok(if self.script.no_key || self.script.interactive { Auth::reject() } else { Auth::Accept })
    }

    async fn tcpip_forward(&mut self, address: &str, port: &mut u32, _: &mut Session) -> Result<bool, Self::Error> {
        self.seen.lock().unwrap().forward = Some((address.to_string(), *port));
        Ok(!self.script.no_forward)
    }

    async fn channel_open_session(
        &mut self,
        _: Channel<Msg>,
        reply: ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn shell_request(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
        self.seen.lock().unwrap().shell = true;
        session.channel_success(channel)?;
        for chunk in &self.script.chunks {
            session.data(channel, chunk.to_vec())?;
        }
        if let Some((port, mut asks)) = self.front.take() {
            session.data(channel, format!("Your tunnel: https://{NAME}:{port}/\r\n").into_bytes())?;
            let handle = session.handle();
            tokio::spawn(async move {
                while let Some((head, back)) = asks.recv().await {
                    let Ok(mut forwarded) =
                        handle.channel_open_forwarded_tcpip("localhost", 80, "127.0.0.1", 40000).await
                    else {
                        return;
                    };
                    let _ = forwarded.data(&head[..]).await;
                    let mut answer = Vec::new();
                    while let Some(message) = forwarded.wait().await {
                        match message {
                            ChannelMsg::Data { data } => answer.extend_from_slice(&data),
                            ChannelMsg::Eof | ChannelMsg::Close => break,
                            _ => {}
                        }
                    }
                    let _ = back.send(answer);
                }
            });
        }
        if self.script.close {
            session.eof(channel)?;
            session.close(channel)?;
        }
        Ok(())
    }
}

/// The HTTPS front for [`NAME`]: it reads the head of each request, then
/// answers with a page of its own, or gives the request to the relay and
/// writes its answer.
fn serve_front(kind: Front, seen: Arc<Mutex<Seen>>, asks: tmpsc::UnboundedSender<Ask>) -> Stand {
    tls_raw(move |io: &mut dyn Io| {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") && io.read(&mut byte).is_ok_and(|n| n == 1) {
            head.push(byte[0]);
        }
        seen.lock().unwrap().requests += 1;
        let answer = match kind {
            Front::Page => PAGE.to_vec(),
            Front::Forward => {
                let (back, answer) = mpsc::channel();
                let _ = asks.send((head, back));
                answer.recv_timeout(Duration::from_secs(10)).unwrap_or_default()
            }
        };
        let _ = io.write_all(&answer);
    })
}

/// A stand-in relay for one session: its port, what it saw, and the end of
/// the session.
async fn relay(script: Script) -> (u16, Arc<Mutex<Seen>>, oneshot::Receiver<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a listener");
    let port = listener.local_addr().expect("an address").port();
    // The host key of the stand-in, made for the test.
    let key = PrivateKey::random(&mut russh::keys::key::safe_rng(), Algorithm::Ed25519).expect("a key");
    let config = Arc::new(server::Config {
        keys: vec![key],
        methods: MethodSet::from(&[MethodKind::PublicKey, MethodKind::None, MethodKind::KeyboardInteractive][..]),
        auth_rejection_time: Duration::ZERO,
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..server::Config::default()
    });
    let seen = Arc::new(Mutex::new(Seen::default()));
    // The front lives as long as the session.
    let (stand, front) = match script.front {
        Some(kind) => {
            let (asks, taken) = tmpsc::unbounded_channel();
            let stand = serve_front(kind, seen.clone(), asks);
            let port = stand.port();
            (Some(stand), Some((port, taken)))
        }
        None => (None, None),
    };
    let handler = Relay { script, seen: seen.clone(), front };
    let (ended, end) = oneshot::channel();
    tokio::spawn(async move {
        let _stand = stand;
        if let Ok((stream, _)) = listener.accept().await
            && let Ok(session) = server::run_stream(config, stream, handler).await
        {
            let _ = session.await;
        }
        let _ = ended.send(());
    });
    (port, seen, end)
}

/// A relay file: its endpoint, the rule (each of `https://` and
/// `.tunnel.test`, none of `dashboard.`), `more` facts and the watch with
/// `args`.
fn relay_file(id: &str, port: u16, more: &str, args: &str) -> String {
    let body = endpoint("ssh", "ssh", "127.0.0.1", port, "")
        + &fact("url-match", r#"["https://", ".tunnel.test"]"#)
        + &fact("url-skip", r#"["dashboard."]"#)
        + more
        + "[[watch]]\nkind = \"forward-url\"\nendpoint = \"ssh\"\n"
        + args;
    file(id, &body)
}

/// The client ended the session: the stand-in saw its end.
async fn ended(end: oneshot::Receiver<()>) {
    tokio::time::timeout(Duration::from_secs(10), end).await.expect("the end of the session").expect("an end");
}

#[tokio::test]
async fn forward_url_reads_a_url_that_two_chunks_cut() {
    pod_testkit::need_loopback!();
    let chunks: Vec<&'static [u8]> = vec![
        b"Welcome.\r\nUpgrade: https://dash",
        b"board.example.test/pro\r\nYour tunnel: \x1b[4mhttps://ab",
        b"c12.tun",
        b"nel.test\x1b[0m\r\n",
    ];
    let (port, seen, end) = relay(Script { chunks, ..Script::default() }).await;
    let report = one(&relay_file("f1", port, &fact("user", "\"http\""), "prove = false\n")).await;
    let all = text(&report);
    assert!(report.holds(), "{all}");
    let want = format!(
        "127.0.0.1:{port} as http (a key of the run): the banner names the tunnel on the host abc12.tunnel.test"
    );
    assert!(all.contains(&want), "{all}");
    ended(end).await;
    let seen = seen.lock().unwrap();
    assert_eq!(seen.users, vec!["http"]);
    assert_eq!(seen.key.as_deref(), Some("ssh-ed25519"));
    assert_eq!(seen.forward, Some(("localhost".to_string(), 80)));
    assert!(seen.shell);
}

#[tokio::test]
async fn forward_url_finds_a_banner_with_no_url_that_the_rule_accepts() {
    pod_testkit::need_loopback!();
    let chunks: Vec<&'static [u8]> =
        vec![b"Pro: https://dashboard.tunnel.test\r\n", b"Your tunnel: https://abc.other.test/?k=v\r\n"];
    let (port, seen, end) = relay(Script { chunks, close: true, ..Script::default() }).await;
    let report = one(&relay_file("f2", port, "", "")).await;
    assert_eq!(
        report.outcome.findings,
        vec![Finding::moved(
            "\"url-match\"",
            "https://, .tunnel.test",
            "a banner with no URL that the rule accepts (the relay closed the shell): \
             https://dashboard.tunnel.test, https://abc.other.test/"
        )]
    );
    let all = text(&report);
    assert!(all.contains("MOVED f2 forward-url: services/f2.toml \"url-match\" is https://, .tunnel.test"), "{all}");
    assert!(all.contains("(source: measured: a test, read 2026-11-10)"), "{all}");
    ended(end).await;
    // No fact `user`: the fixed word.
    assert_eq!(seen.lock().unwrap().users, vec!["pod-services-watch"]);
}

#[tokio::test]
async fn forward_url_fails_on_a_banner_with_no_url() {
    pod_testkit::need_loopback!();
    let chunks: Vec<&'static [u8]> = vec![b"No tunnel ", b"today.\r\n"];
    let (port, _, end) = relay(Script { chunks, close: true, ..Script::default() }).await;
    let report = one(&relay_file("f3", port, "", "")).await;
    let all = text(&report);
    let want = format!(
        "FAILED f3 forward-url: 127.0.0.1:{port} as pod-services-watch: the relay closed the shell, \
         and the banner holds no URL: \"No tunnel today.\""
    );
    assert!(all.contains(&want), "{all}");
    ended(end).await;
}

/// A relay that writes no banner: the check ends at its bound, and closes.
#[tokio::test]
async fn forward_url_ends_at_its_bound() {
    pod_testkit::need_loopback!();
    let (port, _, end) = relay(Script::default()).await;
    let report = one(&relay_file("f4", port, "", "seconds = 1\n")).await;
    let all = text(&report);
    assert!(all.contains("no URL within 1s, and the banner holds no URL"), "{all}");
    ended(end).await;
}

/// A relay that takes no key: the client offers its key, then no key, and
/// asks for the forward port of the watch.
#[tokio::test]
async fn forward_url_takes_no_key_and_the_port_of_the_watch() {
    pod_testkit::need_loopback!();
    let chunks: Vec<&'static [u8]> = vec![b"https://x.tunnel.test\r\n"];
    let (port, seen, end) = relay(Script { chunks, no_key: true, ..Script::default() }).await;
    let report = one(&relay_file("f5", port, &fact("user", "\"nokey\""), "port = 0\nprove = false\n")).await;
    let all = text(&report);
    assert!(report.holds() && all.contains("as nokey (no key): the banner names the tunnel on the host x.tunnel.test"));
    ended(end).await;
    let seen = seen.lock().unwrap();
    assert_eq!(seen.users, vec!["nokey", "nokey"]);
    assert_eq!(seen.forward, Some(("localhost".to_string(), 0)));
}

/// A relay that takes neither a key nor no key, and lets anyone in through
/// keyboard-interactive (Serveo): the client answers each prompt empty.
#[tokio::test]
async fn forward_url_takes_an_empty_keyboard_interactive_login() {
    pod_testkit::need_loopback!();
    let chunks: Vec<&'static [u8]> = vec![b"Forwarding HTTP traffic from https://y.tunnel.test\r\n"];
    let (port, seen, end) = relay(Script { chunks, interactive: true, ..Script::default() }).await;
    let report = one(&relay_file("f8", port, "", "prove = false\n")).await;
    let all = text(&report);
    assert!(
        report.holds()
            && all.contains("(keyboard-interactive, no answer): the banner names the tunnel on the host y.tunnel.test"),
        "{all}"
    );
    ended(end).await;
    assert_eq!(seen.lock().unwrap().users.len(), 4, "a key, no key, and keyboard-interactive in two rounds");
}

#[tokio::test]
async fn forward_url_fails_on_a_refused_forward_and_on_a_service_that_is_no_relay() {
    pod_testkit::need_loopback!();
    let (port, seen, end) = relay(Script { no_forward: true, ..Script::default() }).await;
    let report = one(&relay_file("f6", port, "", "")).await;
    let all = text(&report);
    assert!(
        all.contains(&format!(
            "FAILED f6 forward-url: 127.0.0.1:{port} as pod-services-watch: the remote forward of port 80:"
        )),
        "{all}"
    );
    ended(end).await;
    assert!(!seen.lock().unwrap().shell);

    let body =
        endpoint("ssh", "ssh", "127.0.0.1", port, "") + "[[watch]]\nkind = \"forward-url\"\nendpoint = \"ssh\"\n";
    let report = one(&file("f7", &(fact("user", "\"x\"") + &body))).await;
    assert!(text(&report).contains("FAILED f7 forward-url: the service is no forward relay"), "{}", text(&report));
}

/// A relay file whose rule takes the front of the stand-in.
fn front_file(id: &str, port: u16) -> String {
    let body = endpoint("ssh", "ssh", "127.0.0.1", port, "")
        + &fact("url-match", &format!(r#"["https://", "{NAME}"]"#))
        + "[[watch]]\nkind = \"forward-url\"\nendpoint = \"ssh\"\n";
    file(id, &body)
}

/// The front gives the GET of the URL to the client, which answers with
/// the word of the run.
#[tokio::test]
async fn forward_url_proves_the_url_with_a_get_through_the_forward() {
    pod_testkit::need_loopback!();
    let script = Script { chunks: vec![b"Welcome.\r\n"], front: Some(Front::Forward), ..Script::default() };
    let (port, seen, end) = relay(script).await;
    let report = one(&front_file("p1", port)).await;
    let all = text(&report);
    assert!(report.holds(), "{all}");
    assert!(all.contains(&format!("the banner names the tunnel on the host {NAME}")), "{all}");
    assert!(all.contains("gave the word of the run through the forward (1 answered)"), "{all}");
    ended(end).await;
    assert_eq!(seen.lock().unwrap().requests, 1);
}

/// The URL is a page of the relay: the front answers, and no request
/// reaches the client.
#[tokio::test]
async fn forward_url_finds_a_url_that_is_a_page_of_the_relay() {
    pod_testkit::need_loopback!();
    let script = Script { chunks: vec![b"Welcome.\r\n"], front: Some(Front::Page), ..Script::default() };
    let (port, seen, end) = relay(script).await;
    let report = one(&front_file("p2", port)).await;
    let [Finding::Moved { fact, was, now }] = &report.outcome.findings[..] else {
        panic!("one finding: {}", text(&report));
    };
    assert_eq!((fact.as_str(), was.as_str()), ("\"url-match\"", "https://, stand.test"));
    assert!(now.contains("does not reach the forward of the session"), "{now}");
    assert!(now.contains("HTTP 200 with no word of the run: \"a page of the relay\""), "{now}");
    ended(end).await;
    assert_eq!(seen.lock().unwrap().requests, 1);
}
