//! The exchange as a hop sees it, for `tunnel --inspect` (LEARN-3). The
//! view is a list of fields of the hop, each a name and a value that the hop
//! holds. [`Inspect::relay`] builds the relay's view from the prelude and the
//! bytes that the relay carries: an identifier, a length and a digest, and
//! nothing else. It takes no key, it holds no plaintext, and its `Debug`
//! prints the same fields, so neither a destination nor a key can appear in
//! a rendered field or in a debug output of the relay.

use std::fmt;

use sha2::{Digest, Sha256};

use crate::envelope::{ID, Prelude, SIZE};
use crate::knowledge::Hop;
use crate::legs::hex;

/// One field of a view: its name and the value that the hop sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: &'static str,
    pub value: String,
}

/// The exchange as one hop sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspect {
    hop: Hop,
    fields: Vec<Field>,
}

impl Inspect {
    /// An empty view of `hop`.
    pub fn new(hop: Hop) -> Inspect {
        Inspect { hop, fields: Vec::new() }
    }

    /// Add one field that this hop sees.
    pub fn observe(&mut self, name: &'static str, value: impl Into<String>) -> &mut Inspect {
        self.fields.push(Field { name, value: value.into() });
        self
    }

    /// The hop of this view.
    pub fn hop(&self) -> Hop {
        self.hop
    }

    /// The fields of this view, in the order that the hop saw them.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// The relay's view of one prelude and the bytes after it: the
    /// identifier that the relay routes by, the prelude's size and digest,
    /// and the bytes' size and digest. The call takes no key, and the view
    /// can hold no plaintext.
    pub fn relay(prelude: &Prelude, wire: &[u8]) -> Inspect {
        let mut view = Inspect::new(Hop::Relay);
        view.observe("prelude id", hex(&prelude.id()));
        view.observe("prelude", format!("{SIZE} bytes, sha256 {}", hex(&digest(prelude.bytes()))));
        view.observe("wire", format!("{} bytes, sha256 {}", wire.len(), hex(&digest(wire))));
        view
    }

    /// The view as a hop reads it, one field for each line.
    pub fn render(&self) -> String {
        let mut out = format!("tunnel --inspect, {}:\n", self.hop.name());
        for field in &self.fields {
            out.push_str(&format!("  {}: {}\n", field.name, field.value));
        }
        out
    }
}

impl fmt::Display for Inspect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}

/// The SHA-256 of `bytes`, for a view that shows no byte of them.
fn digest(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(&Sha256::digest(bytes));
    out
}

/// The identifier of a prelude of `bytes`, for a view that reads one field.
pub fn id_of(bytes: &[u8]) -> Option<[u8; ID]> {
    Prelude::of(bytes).ok().map(|prelude| prelude.id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_view_renders_each_field_of_its_hop() {
        let mut view = Inspect::new(Hop::First);
        view.observe("client", "192.0.2.10:51515").observe("volume", "10 bytes");
        assert_eq!(view.hop(), Hop::First);
        assert_eq!(view.fields().len(), 2);
        assert!(view.render().contains("client: 192.0.2.10:51515"));
        assert_eq!(id_of(&[0u8; SIZE]), Some([0u8; ID]));
        assert_eq!(id_of(&[0u8; 1]), None);
    }
}
