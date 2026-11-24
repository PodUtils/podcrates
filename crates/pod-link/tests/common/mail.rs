//! The mock mail servers of the tests: a submission server that speaks the
//! client half of SMTP (STARTTLS, or TLS from the first byte) and a
//! retrieval server that speaks the client half of IMAP, each on the
//! loopback. They present the certificate of `tests/fixtures`, and the
//! client of the test trusts it. The protocol of each handler is the part
//! that a carrier of this crate uses, and no more.

#![allow(dead_code)]

use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use pod_link::BoxIo;
use pod_link::imap::{Imap, Mode};
use pod_link::smtp::Credential;
use pod_link::smtp::Smtp;
use pod_tls::server::TlsAcceptor;
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::{TcpListener, TcpStream};

use super::cert::{acceptor, client_config};

/// The user of a session credential of the tests.
pub const USER: &str = "session-user";

/// The secret of a session credential of the tests: a made-up value, never
/// the password of an account.
pub const SECRET: &str = "made-up-session-secret";

/// The submission carrier of the tests, with the certificate of the tests
/// trusted.
pub fn smtp_carrier(mode: Mode) -> Smtp {
    let credential = Credential::new(USER, SECRET).expect("a credential of the tests");
    Smtp::new(credential, "dialer@example.invalid", "session@example.invalid").with_tls(client_config()).with_mode(mode)
}

/// The retrieval carrier of the tests, with a poll of 100 ms so that a test
/// waits one poll and no more.
pub fn imap_carrier(mode: Mode) -> Imap {
    let credential = Credential::new(USER, SECRET).expect("a credential of the tests");
    let mut imap = Imap::new(credential, "inbox", "outbox").with_tls(client_config()).with_mode(mode);
    imap.poll = Duration::from_millis(100);
    imap
}

/// The submission server of a test.
pub struct SmtpServer {
    pub addr: SocketAddr,
    /// The messages that the server took, as they came.
    pub messages: Arc<Mutex<Vec<Vec<u8>>>>,
}

/// Start a submission server with an optional `SIZE` limit, which it
/// advertises.
pub async fn smtp_server(mode: Mode, limit: Option<u64>) -> SmtpServer {
    smtp_server_with(mode, limit, limit.is_some()).await
}

/// Start a submission server whose `SIZE` limit it does not advertise: it
/// refuses a message over the limit alone.
pub async fn smtp_server_hidden(mode: Mode, limit: u64) -> SmtpServer {
    smtp_server_with(mode, Some(limit), false).await
}

/// Start a submission server with an optional `SIZE` limit, advertised or
/// not.
pub async fn smtp_server_with(mode: Mode, limit: Option<u64>, advertise: bool) -> SmtpServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a submission socket");
    let addr = listener.local_addr().expect("the address of the server");
    let messages = Arc::new(Mutex::new(Vec::new()));
    let acceptor = Some(acceptor());
    let taken = messages.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { return };
            let messages = taken.clone();
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let _ = serve_smtp(stream, mode, acceptor, limit, advertise, messages).await;
            });
        }
    });
    SmtpServer { addr, messages }
}

async fn serve_smtp(
    stream: TcpStream,
    mode: Mode,
    acceptor: Option<TlsAcceptor>,
    limit: Option<u64>,
    advertise: bool,
    messages: Arc<Mutex<Vec<Vec<u8>>>>,
) -> io::Result<()> {
    let mut lines = match mode {
        Mode::Implicit => {
            let tls = acceptor
                .clone()
                .expect("an acceptor")
                .accept(Box::new(stream) as BoxIo)
                .await
                .map_err(io::Error::other)?;
            BufReader::new(Box::new(tls) as BoxIo)
        }
        Mode::Starttls => BufReader::new(Box::new(stream) as BoxIo),
    };
    write(&mut lines, b"220 mock.test ESMTP\r\n").await?;
    loop {
        let Some(line) = next(&mut lines).await else { return Ok(()) };
        let text = String::from_utf8_lossy(&line).trim_end().to_string();
        let upper = text.to_uppercase();
        if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            let mut reply = String::from("250-mock.test\r\n");
            if let (Some(limit), true) = (limit, advertise) {
                reply.push_str(&format!("250-SIZE {limit}\r\n"));
            }
            if mode == Mode::Starttls {
                reply.push_str("250-STARTTLS\r\n");
            }
            reply.push_str("250 AUTH PLAIN LOGIN\r\n");
            write(&mut lines, reply.as_bytes()).await?;
        } else if upper.starts_with("STARTTLS") {
            write(&mut lines, b"220 begin TLS\r\n").await?;
            let io = lines.into_inner();
            let tls = acceptor.clone().expect("an acceptor").accept(io).await.map_err(io::Error::other)?;
            lines = BufReader::new(Box::new(tls) as BoxIo);
        } else if upper.starts_with("AUTH PLAIN ") {
            let token = text.split_whitespace().nth(2).unwrap_or_default();
            let plain = base64::engine::general_purpose::STANDARD.decode(token).unwrap_or_default();
            let expected = format!("\0{USER}\0{SECRET}");
            if plain == expected.as_bytes() {
                write(&mut lines, b"235 authenticated\r\n").await?;
            } else {
                write(&mut lines, b"535 no\r\n").await?;
            }
        } else if upper.starts_with("MAIL FROM:") {
            let size = text
                .to_uppercase()
                .split("SIZE=")
                .nth(1)
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|value| value.parse::<u64>().ok());
            match (limit, size) {
                (Some(limit), Some(size)) if size > limit => {
                    let reply = format!("552 5.3.4 message size exceeds the limit of {limit}\r\n");
                    write(&mut lines, reply.as_bytes()).await?;
                }
                _ => write(&mut lines, b"250 sender ok\r\n").await?,
            }
        } else if upper.starts_with("RCPT TO:") {
            write(&mut lines, b"250 recipient ok\r\n").await?;
        } else if upper == "DATA" {
            write(&mut lines, b"354 end with a dot\r\n").await?;
            let mut message = Vec::new();
            loop {
                let Some(line) = next(&mut lines).await else { return Ok(()) };
                if line == b".\r\n" || line == b".\n" {
                    break;
                }
                message.extend_from_slice(&line);
            }
            if let Some(limit) = limit
                && message.len() as u64 > limit
            {
                let reply = format!("552 5.3.4 message size exceeds the limit of {limit}\r\n");
                write(&mut lines, reply.as_bytes()).await?;
                continue;
            }
            messages.lock().unwrap_or_else(|e| e.into_inner()).push(message);
            write(&mut lines, b"250 message ok\r\n").await?;
        } else if upper == "QUIT" {
            write(&mut lines, b"221 bye\r\n").await?;
            return Ok(());
        } else if upper.starts_with("NOOP") {
            write(&mut lines, b"250 ok\r\n").await?;
        } else {
            write(&mut lines, b"500 unknown\r\n").await?;
        }
    }
}

/// The retrieval server of a test.
pub struct ImapServer {
    pub addr: SocketAddr,
    /// The messages that the client appended, in order.
    pub appended: Arc<Mutex<Vec<Vec<u8>>>>,
    /// The messages that the server holds for a fetch, with the uid of each
    /// as its place plus 1.
    pub store: Arc<Mutex<Vec<Vec<u8>>>>,
}

/// Start a retrieval server with an optional `APPENDLIMIT`.
pub async fn imap_server(mode: Mode, append_limit: Option<u64>) -> ImapServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a retrieval socket");
    let addr = listener.local_addr().expect("the address of the server");
    let appended = Arc::new(Mutex::new(Vec::new()));
    let store = Arc::new(Mutex::new(Vec::new()));
    let acceptor = Some(acceptor());
    let (sent, held) = (appended.clone(), store.clone());
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { return };
            let (appended, store) = (sent.clone(), held.clone());
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let _ = serve_imap(stream, mode, acceptor, append_limit, appended, store).await;
            });
        }
    });
    ImapServer { addr, appended, store }
}

async fn serve_imap(
    stream: TcpStream,
    mode: Mode,
    acceptor: Option<TlsAcceptor>,
    append_limit: Option<u64>,
    appended: Arc<Mutex<Vec<Vec<u8>>>>,
    store: Arc<Mutex<Vec<Vec<u8>>>>,
) -> io::Result<()> {
    let mut lines = match mode {
        Mode::Implicit => {
            let tls = acceptor
                .clone()
                .expect("an acceptor")
                .accept(Box::new(stream) as BoxIo)
                .await
                .map_err(io::Error::other)?;
            BufReader::new(Box::new(tls) as BoxIo)
        }
        Mode::Starttls => BufReader::new(Box::new(stream) as BoxIo),
    };
    write(&mut lines, b"* OK [CAPABILITY IMAP4rev1 STARTTLS AUTH=PLAIN] mock ready\r\n").await?;
    loop {
        let Some(line) = next(&mut lines).await else { return Ok(()) };
        let text = String::from_utf8_lossy(&line).trim_end().to_string();
        let Some((tag, rest)) = text.split_once(' ') else { continue };
        let upper = rest.to_uppercase();
        if upper.starts_with("CAPABILITY") {
            let mut caps = String::from("* CAPABILITY IMAP4rev1 STARTTLS AUTH=PLAIN");
            if let Some(limit) = append_limit {
                caps.push_str(&format!(" APPENDLIMIT={limit}"));
            }
            caps.push_str(&format!("\r\n{tag} OK done\r\n"));
            write(&mut lines, caps.as_bytes()).await?;
        } else if upper.starts_with("STARTTLS") {
            write(&mut lines, format!("{tag} OK begin\r\n").as_bytes()).await?;
            let io = lines.into_inner();
            let tls = acceptor.clone().expect("an acceptor").accept(io).await.map_err(io::Error::other)?;
            lines = BufReader::new(Box::new(tls) as BoxIo);
        } else if upper.starts_with("LOGIN") {
            write(&mut lines, format!("{tag} OK logged in\r\n").as_bytes()).await?;
        } else if upper.starts_with("SELECT") {
            let reply = format!("* 1 EXISTS\r\n* OK [UIDVALIDITY 1] ok\r\n{tag} OK [READ-WRITE] selected\r\n");
            write(&mut lines, reply.as_bytes()).await?;
        } else if upper.starts_with("APPEND") {
            let size = rest
                .split_once('{')
                .and_then(|(_, rest)| rest.split_once('}'))
                .and_then(|(value, _)| value.parse::<usize>().ok());
            let Some(size) = size else {
                write(&mut lines, format!("{tag} BAD no literal\r\n").as_bytes()).await?;
                continue;
            };
            if let Some(limit) = append_limit
                && size as u64 > limit
            {
                let reply = format!("{tag} NO [TOOBIG {limit}] too large\r\n");
                write(&mut lines, reply.as_bytes()).await?;
                continue;
            }
            write(&mut lines, b"+ go ahead\r\n").await?;
            let mut body = vec![0u8; size];
            lines.read_exact(&mut body).await?;
            let mut end = Vec::new();
            lines.read_until(b'\n', &mut end).await?;
            appended.lock().unwrap_or_else(|e| e.into_inner()).push(body);
            write(&mut lines, format!("{tag} OK appended\r\n").as_bytes()).await?;
        } else if upper.starts_with("UID FETCH") {
            let start: u32 = rest
                .split_whitespace()
                .nth(2)
                .and_then(|range| range.split(':').next())
                .and_then(|value| value.parse().ok())
                .unwrap_or(1);
            let messages = store.lock().unwrap_or_else(|e| e.into_inner()).clone();
            for (place, message) in messages.iter().enumerate() {
                let uid = place as u32 + 1;
                if uid < start {
                    continue;
                }
                let head = format!("* {} FETCH (UID {uid} BODY[] {{{}}}\r\n", place + 1, message.len());
                write(&mut lines, head.as_bytes()).await?;
                write(&mut lines, message).await?;
                write(&mut lines, b")\r\n").await?;
            }
            write(&mut lines, format!("{tag} OK done\r\n").as_bytes()).await?;
        } else if upper.starts_with("LOGOUT") {
            write(&mut lines, format!("* BYE\r\n{tag} OK bye\r\n").as_bytes()).await?;
            return Ok(());
        } else {
            write(&mut lines, format!("{tag} BAD unknown\r\n").as_bytes()).await?;
        }
    }
}

/// Start a retrieval server that answers the handshake and then accepts a
/// `UID FETCH` and never answers it. The command timeout of the client is
/// what ends the session.
pub async fn imap_server_silent(mode: Mode) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a retrieval socket");
    let addr = listener.local_addr().expect("the address of the server");
    let acceptor = Some(acceptor());
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { return };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let _ = serve_imap_silent(stream, mode, acceptor).await;
            });
        }
    });
    addr
}

async fn serve_imap_silent(stream: TcpStream, mode: Mode, acceptor: Option<TlsAcceptor>) -> io::Result<()> {
    let mut lines = match mode {
        Mode::Implicit => {
            let tls = acceptor
                .clone()
                .expect("an acceptor")
                .accept(Box::new(stream) as BoxIo)
                .await
                .map_err(io::Error::other)?;
            BufReader::new(Box::new(tls) as BoxIo)
        }
        Mode::Starttls => BufReader::new(Box::new(stream) as BoxIo),
    };
    write(&mut lines, b"* OK [CAPABILITY IMAP4rev1 STARTTLS AUTH=PLAIN] mock ready\r\n").await?;
    loop {
        let Some(line) = next(&mut lines).await else { return Ok(()) };
        let text = String::from_utf8_lossy(&line).trim_end().to_string();
        let Some((tag, rest)) = text.split_once(' ') else { continue };
        let upper = rest.to_uppercase();
        if upper.starts_with("CAPABILITY") {
            let caps = format!("* CAPABILITY IMAP4rev1 STARTTLS AUTH=PLAIN\r\n{tag} OK done\r\n");
            write(&mut lines, caps.as_bytes()).await?;
        } else if upper.starts_with("STARTTLS") {
            write(&mut lines, format!("{tag} OK begin\r\n").as_bytes()).await?;
            let io = lines.into_inner();
            let tls = acceptor.clone().expect("an acceptor").accept(io).await.map_err(io::Error::other)?;
            lines = BufReader::new(Box::new(tls) as BoxIo);
        } else if upper.starts_with("LOGIN") {
            write(&mut lines, format!("{tag} OK logged in\r\n").as_bytes()).await?;
        } else if upper.starts_with("SELECT") {
            write(&mut lines, format!("* 1 EXISTS\r\n{tag} OK selected\r\n").as_bytes()).await?;
        } else if upper.starts_with("UID FETCH") {
            // Accepted, and never answered: the client's command timeout is
            // the only thing that ends this.
            tokio::time::sleep(Duration::from_secs(60)).await;
            return Ok(());
        } else if upper.starts_with("LOGOUT") {
            return Ok(());
        } else {
            write(&mut lines, format!("{tag} BAD unknown\r\n").as_bytes()).await?;
        }
    }
}

/// One line of a connection, with its ending: `None` at its end.
async fn next(lines: &mut BufReader<BoxIo>) -> Option<Vec<u8>> {
    let mut line = Vec::new();
    match lines.read_until(b'\n', &mut line).await {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line),
    }
}

async fn write(lines: &mut BufReader<BoxIo>, bytes: &[u8]) -> io::Result<()> {
    lines.write_all(bytes).await?;
    lines.flush().await
}

/// Wait until the submission server took `count` messages, and give them.
pub async fn wait_messages(server: &SmtpServer, count: usize) -> Vec<Vec<u8>> {
    for _ in 0..200 {
        let messages = server.messages.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if messages.len() >= count {
            return messages;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    server.messages.lock().unwrap_or_else(|e| e.into_inner()).clone()
}
