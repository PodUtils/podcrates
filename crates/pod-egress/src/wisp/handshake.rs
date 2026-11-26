//! The opening of a Wisp connection, after the upgrade. Version 2: the
//! server's INFO, this side's INFO, then the server's CONTINUE on stream 0
//! with the buffer of each stream, or its CLOSE, which refuses. Version 1:
//! the CONTINUE first, and no INFO; the protocol's document says that a
//! client that receives it first speaks version 1.

use std::time::Duration;

use super::mux::Session;
use super::packet::{self, Extension, Info, Packet, Reason, ext};
use super::{Login, VERSION, WispError};
use pod_ws::frame;
use pod_ws::session::close_code_and_reason;
use zeroize::{Zeroize, Zeroizing};

/// What the opening settled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    /// The version spoken: (1, 0) for a server that sent its CONTINUE first.
    pub version: (u8, u8),
    /// The DATA packets that the server buffers for each new stream.
    pub buffer: u32,
    /// Whether the server confirms each stream with a CONTINUE once its
    /// target's socket is open: both INFOs list stream open confirmation.
    pub confirm: bool,
    /// The server's message of the day, one line made safe.
    pub motd: Option<String>,
}

/// This side's INFO, in answer to the server's: the version that both
/// speak, and those of the server's extensions that this client takes, its
/// message of the day and stream open confirmation, neither with a payload
/// of the client's, and the password extension with `login` where the
/// server lists it. An extension that the server did not list is not sent.
/// Refused: a server that requires a password with no `login`, and one that
/// requires a key's signature with no password to send, as this client signs
/// nothing and no credential goes in a URL.
pub fn answer(server: &Info, login: Option<&Login>) -> Result<(Info, Opened), String> {
    // The first byte of the server's metadata says whether it requires it.
    let requires = |id: u8| server.extension(id).is_some_and(|m| m.first().is_some_and(|b| *b != 0));
    let password = login.filter(|_| server.extension(ext::PASSWORD).is_some());
    if password.is_none() {
        if requires(ext::PASSWORD) {
            return Err("the Wisp server requires a user name and a password, and none was given".into());
        }
        if requires(ext::KEY) {
            return Err("the Wisp server requires a key's signature, which this client does not send".into());
        }
    }
    let version = (server.major, server.minor).min(VERSION);
    let mut extensions = Vec::new();
    if let Some(login) = password {
        extensions.push(Extension { id: ext::PASSWORD, payload: login.metadata().to_vec() });
    }
    let motd = server.extension(ext::MOTD).map(super::note_text);
    if motd.is_some() {
        extensions.push(Extension { id: ext::MOTD, payload: Vec::new() });
    }
    let confirm = server.extension(ext::CONFIRM).is_some();
    if confirm {
        extensions.push(Extension { id: ext::CONFIRM, payload: Vec::new() });
    }
    let ours = Info { major: version.0, minor: version.1, extensions };
    let motd = motd.filter(|m| !m.is_empty());
    Ok((ours, Opened { version, buffer: 0, confirm, motd }))
}

/// The opening, within `timeout`.
pub(super) async fn open(session: &Session, timeout: Duration, login: Option<&Login>) -> Result<Opened, WispError> {
    match tokio::time::timeout(timeout, steps(session, login)).await {
        Ok(opened) => opened,
        Err(_) => {
            let secs = timeout.as_secs();
            Err(WispError::Opening(format!("the Wisp server did not open the connection within {secs} s")))
        }
    }
}

async fn steps(session: &Session, login: Option<&Login>) -> Result<Opened, WispError> {
    match next(session).await? {
        (0, Packet::Continue(buffer)) => Ok(Opened { version: (1, 0), buffer, confirm: false, motd: None }),
        (0, Packet::Info(info)) => {
            let (ours, mut opened) = match answer(&info, login) {
                Ok(both) => both,
                Err(why) => {
                    // A client's refusal: a CLOSE on stream 0, then the Close.
                    let refusal = packet::encode(0, &Packet::Close(Reason::INCOMPATIBLE));
                    let _ = session.send_binary(&refusal).await;
                    let _ = session.send_close(1000, "").await;
                    return Err(WispError::Opening(why));
                }
            };
            let mut ours = Packet::Info(ours);
            let info = Zeroizing::new(packet::encode(0, &ours));
            // A password is in the metadata: zeroed once it is encoded.
            if let Packet::Info(ours) = &mut ours {
                ours.extensions.iter_mut().for_each(|e| e.payload.zeroize());
            }
            let sent = session.send_binary(&info).await;
            sent.map_err(|e| WispError::Failed(format!("sending the INFO to the Wisp server failed: {e}")))?;
            match next(session).await? {
                (0, Packet::Continue(buffer)) => {
                    opened.buffer = buffer;
                    Ok(opened)
                }
                (0, Packet::Close(reason)) => Err(WispError::Refused(reason)),
                (id, other) => Err(out_of_turn(id, &other, "its CONTINUE")),
            }
        }
        (0, Packet::Close(reason)) => Err(WispError::Refused(reason)),
        (id, other) => Err(out_of_turn(id, &other, "a CONTINUE or an INFO")),
    }
}

fn out_of_turn(id: u32, packet: &Packet, wanted: &str) -> WispError {
    WispError::Opening(format!(
        "the Wisp server sent {} on stream {id} where {wanted} on stream 0 opens the connection: it may not speak Wisp",
        packet.name()
    ))
}

/// The server's next packet. A text frame is no packet: a server of
/// another kind may say why in one, and its words go in the line.
async fn next(session: &Session) -> Result<(u32, Packet), WispError> {
    let f = session.read_frame().await.map_err(|e| WispError::Failed(e.to_string()))?;
    match f.opcode {
        frame::OPCODE_BINARY => {
            let not_wisp = |why: String| WispError::Opening(format!("the server sent {why}: it may not speak Wisp"));
            packet::decode(&f.payload).map_err(not_wisp)
        }
        frame::OPCODE_CLOSE => {
            let (code, reason) = close_code_and_reason(&f.payload);
            Err(WispError::Gone { code, reason })
        }
        _ => {
            let said = super::note_text(&f.payload);
            Err(WispError::Opening(format!("the server sent text where Wisp sends a packet: {said}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(major: u8, minor: u8, extensions: Vec<Extension>) -> Info {
        Info { major, minor, extensions }
    }

    fn with(id: u8, metadata: &[u8]) -> Extension {
        Extension { id, payload: metadata.to_vec() }
    }

    #[test]
    fn the_answer_takes_the_lower_version_and_only_the_extensions_that_both_list() {
        let motd = with(ext::MOTD, b"rate limits: 500 kB/s\r\n");
        let server = info(2, 0, vec![with(ext::UDP, b""), motd, with(ext::CONFIRM, b"")]);
        let (ours, opened) = answer(&server, None).unwrap();
        assert_eq!((ours.major, ours.minor), (2, 0));
        let ids: Vec<u8> = ours.extensions.iter().map(|e| e.id).collect();
        assert_eq!(ids, [ext::MOTD, ext::CONFIRM], "no UDP: this road carries none");
        assert!(ours.extensions.iter().all(|e| e.payload.is_empty()), "no client payload");
        assert_eq!(opened.motd.as_deref(), Some("rate limits: 500 kB/s"));
        assert!(opened.confirm);
        let (ours, opened) = answer(&info(3, 7, Vec::new()), None).unwrap();
        assert_eq!(((ours.major, ours.minor), opened.version), ((2, 1), (2, 1)), "this side's newest");
        assert!(ours.extensions.is_empty() && !opened.confirm && opened.motd.is_none());
    }

    #[test]
    fn a_server_that_requires_a_credential_is_refused_and_one_that_allows_it_is_not() {
        let password = answer(&info(2, 0, vec![with(ext::PASSWORD, &[1])]), None).unwrap_err();
        assert!(password.contains("requires a user name and a password, and none was given"), "{password}");
        let key = answer(&info(2, 0, vec![with(ext::KEY, &[1, 1, 9, 9])]), None).unwrap_err();
        assert!(key.contains("a key's signature"), "{key}");
        let optional = info(2, 0, vec![with(ext::PASSWORD, &[0]), with(ext::KEY, &[0, 1])]);
        let (ours, _) = answer(&optional, None).unwrap();
        assert!(ours.extensions.is_empty(), "no password where none was given");
    }

    #[test]
    fn a_login_goes_where_the_server_lists_the_password_extension_and_nowhere_else() {
        let login = Login::new("ann", "hunter2").unwrap();
        // Required by the server, or the key required and a password allowed.
        for server in [vec![with(ext::PASSWORD, &[1])], vec![with(ext::PASSWORD, &[0]), with(ext::KEY, &[1, 1])]] {
            let (ours, _) = answer(&info(2, 0, server), Some(&login)).unwrap();
            assert_eq!(ours.extension(ext::PASSWORD), Some(&b"\x03annhunter2"[..]));
            assert!(!format!("{ours:?}").contains("hunter2"), "no Debug holds the password");
        }
        let (ours, _) = answer(&info(2, 0, vec![with(ext::MOTD, b"hi")]), Some(&login)).unwrap();
        assert_eq!(ours.extension(ext::PASSWORD), None, "a server that does not list it gets none");
        let key = answer(&info(2, 0, vec![with(ext::KEY, &[1, 1])]), Some(&login)).unwrap_err();
        assert!(key.contains("a key's signature"), "{key}");
    }
}
