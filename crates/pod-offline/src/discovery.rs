//! Local discovery: a signed announcement in a DNS-SD TXT record, and one
//! radio interface.
//!
//! The announcement names a peer, its address, its port and its Ed25519
//! key, and the key signs the words. The record is the TXT form of DNS-SD:
//! a sequence of length-prefixed strings, `v`, `n`, `a`, `p`, `k` and `s`.
//! A peer that hears an announcement with a bad signature ignores it.
//!
//! [`Radio`] is one interface, [`KissRadio`]: it puts the TXT record in a
//! KISS frame of the port [`kiss::PORT_DISCOVERY`] on any writer, a serial
//! line included. There is no second interface in this crate.

use std::collections::BTreeMap;
use std::io::Write;

use pod_identity::{Identity, PublicKey};

use crate::Error;
use crate::hex;
use crate::serial::kiss::{self, PORT_DISCOVERY};

/// The DNS-SD service of the announcement.
pub const SERVICE: &str = "_pod._udp.local";
/// The version of the record.
pub const VERSION: &str = "1";
/// The most bytes of one TXT record.
pub const TXT_MOST: usize = 1024;
const LABEL: &[u8] = b"pod-offline announce 1\n";

/// A signed announcement of one peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub key: PublicKey,
    pub signature: [u8; 64],
}

impl Announcement {
    /// Sign the words of `name`, `address` and `port` with `identity`.
    pub fn signed(identity: &Identity, name: &str, address: &str, port: u16) -> Result<Announcement, Error> {
        check_word(name, "name")?;
        check_word(address, "address")?;
        if port == 0 {
            return Err(Error::Txt { why: "the port is 0".into() });
        }
        let key = identity.public();
        let signature = identity.sign(&message(name, address, port, &key));
        Ok(Announcement { name: name.to_string(), address: address.to_string(), port, key, signature })
    }

    /// Whether the signature is the key's over the words.
    pub fn verify(&self) -> bool {
        self.key.verifies(&message(&self.name, &self.address, self.port, &self.key), &self.signature)
    }

    /// The TXT record of the announcement.
    pub fn txt(&self) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        for (key, value) in [
            ("v", VERSION.to_string()),
            ("t", SERVICE.to_string()),
            ("n", self.name.clone()),
            ("a", self.address.clone()),
            ("p", self.port.to_string()),
            ("k", self.key.hex()),
            ("s", hex::encode(&self.signature)),
        ] {
            let entry = format!("{key}={value}");
            if entry.len() > 255 {
                return Err(Error::Txt { why: format!("the field {key} is above 255 bytes") });
            }
            out.push(entry.len() as u8);
            out.extend_from_slice(entry.as_bytes());
        }
        if out.len() > TXT_MOST {
            return Err(Error::Txt { why: format!("the record is above {TXT_MOST} bytes") });
        }
        Ok(out)
    }

    /// Read a TXT record. The signature is not checked here:
    /// [`Announcement::verify`] is a second step, and [`Discovery::hear`]
    /// makes both.
    pub fn from_txt(bytes: &[u8]) -> Result<Announcement, Error> {
        let mut fields = BTreeMap::new();
        let mut at = 0;
        while at < bytes.len() {
            let len = usize::from(bytes[at]);
            at += 1;
            let end = at
                .checked_add(len)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| Error::Txt { why: "a field ends after the record".into() })?;
            let text =
                std::str::from_utf8(&bytes[at..end]).map_err(|_| Error::Txt { why: "a field is not UTF-8".into() })?;
            at = end;
            let Some((key, value)) = text.split_once('=') else {
                return Err(Error::Txt { why: format!("the field {text:?} has no equals sign") });
            };
            if fields.insert(key.to_string(), value.to_string()).is_some() {
                return Err(Error::Txt { why: format!("the field {key} comes twice") });
            }
        }
        if fields.get("v").map(String::as_str) != Some(VERSION) {
            return Err(Error::Txt { why: "the version is not 1".into() });
        }
        if fields.get("t").map(String::as_str) != Some(SERVICE) {
            return Err(Error::Txt { why: format!("the service is not {SERVICE}") });
        }
        let name = fields.remove("n").ok_or_else(|| Error::Txt { why: "no name".into() })?;
        let address = fields.remove("a").ok_or_else(|| Error::Txt { why: "no address".into() })?;
        let port = fields
            .remove("p")
            .ok_or_else(|| Error::Txt { why: "no port".into() })?
            .parse::<u16>()
            .map_err(|_| Error::Txt { why: "the port is not a number".into() })?;
        let key = fields
            .remove("k")
            .and_then(|text| PublicKey::parse(&text))
            .ok_or_else(|| Error::Txt { why: "no key".into() })?;
        let signature = fields
            .remove("s")
            .and_then(|text| hex::decode(&text))
            .and_then(|raw| <[u8; 64]>::try_from(raw).ok())
            .ok_or_else(|| Error::Txt { why: "no signature of 64 bytes".into() })?;
        if name.is_empty() || address.is_empty() || port == 0 {
            return Err(Error::Txt { why: "a field is empty".into() });
        }
        Ok(Announcement { name, address, port, key, signature })
    }
}

fn check_word(word: &str, what: &str) -> Result<(), Error> {
    if word.is_empty() {
        return Err(Error::Txt { why: format!("the {what} is empty") });
    }
    if word.chars().any(|c| c.is_control()) {
        return Err(Error::Txt { why: format!("the {what} holds a control byte") });
    }
    Ok(())
}

/// The words that the signature covers: a label of its own, then each field
/// with a NUL between, then the port in network order and the key.
fn message(name: &str, address: &str, port: u16, key: &PublicKey) -> Vec<u8> {
    let mut out = Vec::with_capacity(LABEL.len() + name.len() + address.len() + 3 + 32);
    out.extend_from_slice(LABEL);
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    out.extend_from_slice(address.as_bytes());
    out.push(0);
    out.extend_from_slice(&port.to_be_bytes());
    out.extend_from_slice(&key.0);
    out
}

/// The peers whose announcements verified.
#[derive(Debug, Default, Clone)]
pub struct Discovery {
    peers: BTreeMap<String, Announcement>,
}

impl Discovery {
    pub fn new() -> Discovery {
        Discovery::default()
    }

    /// Take one TXT record. A record that does not parse, and a record
    /// whose signature is not the key's, is ignored: it gives `false` and
    /// leaves the peers as they were.
    pub fn hear(&mut self, txt: &[u8]) -> bool {
        let Ok(announcement) = Announcement::from_txt(txt) else { return false };
        if !announcement.verify() {
            return false;
        }
        self.peers.insert(announcement.name.clone(), announcement);
        true
    }

    /// The peers, by name.
    pub fn peers(&self) -> Vec<&Announcement> {
        self.peers.values().collect()
    }

    pub fn get(&self, name: &str) -> Option<&Announcement> {
        self.peers.get(name)
    }

    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }
}

/// The one radio interface: a writer and a KISS port.
pub trait Radio {
    /// Put the TXT record on the radio.
    fn broadcast(&mut self, txt: &[u8]) -> Result<(), Error>;
}

/// The one implementation of [`Radio`]: a KISS frame on any writer. A
/// serial line, a pipe and the memory of a test are the same interface.
#[derive(Debug)]
pub struct KissRadio<W: Write> {
    out: W,
    port: u8,
}

impl<W: Write> KissRadio<W> {
    pub fn new(out: W) -> KissRadio<W> {
        KissRadio { out, port: PORT_DISCOVERY }
    }

    /// The port of the frames, [`PORT_DISCOVERY`] by default.
    pub fn with_port(mut self, port: u8) -> KissRadio<W> {
        self.port = port;
        self
    }
}

impl<W: Write> Radio for KissRadio<W> {
    fn broadcast(&mut self, txt: &[u8]) -> Result<(), Error> {
        let wire = kiss::encode(self.port, txt)?;
        self.out.write_all(&wire).map_err(|e| Error::Radio { why: e.to_string() })?;
        self.out.flush().map_err(|e| Error::Radio { why: e.to_string() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer() -> Announcement {
        Announcement::signed(&Identity::from_seed(&[11; 32]), "node-a", "10.0.0.7", 4242).unwrap()
    }

    #[test]
    fn an_announcement_keeps_its_words_through_the_txt_record() {
        let announcement = peer();
        let txt = announcement.txt().unwrap();
        let read = Announcement::from_txt(&txt).unwrap();
        assert_eq!(read, announcement);
        assert!(read.verify());
    }

    #[test]
    fn a_bad_signature_is_ignored() {
        let mut discovery = Discovery::new();
        assert!(discovery.hear(&peer().txt().unwrap()));
        assert_eq!(discovery.len(), 1);

        let mut announcement = peer();
        announcement.port = 9999;
        assert!(!announcement.verify());
        assert!(!discovery.hear(&announcement.txt().unwrap()));
        assert_eq!(discovery.len(), 1);
        assert_eq!(discovery.get("node-a").unwrap().port, 4242);
    }

    #[test]
    fn the_one_radio_puts_a_record_that_a_decoder_reads() {
        let announcement = peer();
        let mut wire = Vec::new();
        KissRadio::new(&mut wire).broadcast(&announcement.txt().unwrap()).unwrap();
        let frame = kiss::Decoder::new().push(&wire).remove(0).unwrap();
        assert_eq!(frame.port, PORT_DISCOVERY);
        let mut discovery = Discovery::new();
        assert!(discovery.hear(&frame.payload));
    }
}
