//! What each hop of a session learns, as data (LEARN-1). One row for each
//! hop: the first hop, the relay, and the far hop. Each row names the fields
//! that the hop learns and leaves the others empty.
//!
//! The rows are the table that the README publishes, and [`Knowledge::render`]
//! prints it. The first hop learns the client's address, the pair that it
//! forwards to and the volume. The relay learns its pair and the volume. The
//! far hop learns the destination, the service name and the identity of its
//! peer. No hop before the far one learns the destination or the service
//! name, and no hop learns the plaintext or a key.

use std::fmt;

/// One hop of a session: the first, a relay, or the far hop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hop {
    /// The first hop, nearest the client.
    First,
    /// A relay between the first hop and the far hop.
    Relay,
    /// The far hop, where the destination is.
    Far,
}

impl Hop {
    /// Each hop, in the order of the table.
    pub const ALL: [Hop; 3] = [Hop::First, Hop::Relay, Hop::Far];

    /// Its name in the table.
    pub fn name(self) -> &'static str {
        match self {
            Hop::First => "first hop",
            Hop::Relay => "relay",
            Hop::Far => "far hop",
        }
    }
}

impl fmt::Display for Hop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What one hop learns. A field that the hop does not learn is `None`, and
/// the volume is the one field that every hop learns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Knowledge {
    pub hop: Hop,
    /// The address of the client, as the hop sees it.
    pub client: Option<String>,
    /// The pair that the relay advertises.
    pub pair: Option<String>,
    /// The bytes that cross the hop.
    pub volume: bool,
    /// The destination, `host:port`.
    pub destination: Option<String>,
    /// The name of the service at the destination.
    pub service: Option<String>,
    /// The identity of the peer, as the handshake proved it.
    pub peer: Option<String>,
}

impl Knowledge {
    /// The three rows of one session. The first hop and the relay get no
    /// destination and no service; the far hop gets both, and the peer's
    /// identity. Each hop gets its pair.
    pub fn table(client: &str, pair: &str, peer: &str, destination: &str, service: &str) -> [Knowledge; 3] {
        [
            Knowledge {
                hop: Hop::First,
                client: Some(client.into()),
                pair: Some(pair.into()),
                volume: true,
                destination: None,
                service: None,
                peer: None,
            },
            Knowledge {
                hop: Hop::Relay,
                client: None,
                pair: Some(pair.into()),
                volume: true,
                destination: None,
                service: None,
                peer: None,
            },
            Knowledge {
                hop: Hop::Far,
                client: None,
                pair: Some(pair.into()),
                volume: true,
                destination: Some(destination.into()),
                service: Some(service.into()),
                peer: Some(peer.into()),
            },
        ]
    }

    /// One row of the issue's table: the hop and each field, with `-` for a
    /// field that the hop does not learn.
    pub fn line(&self) -> String {
        let field = |value: &Option<String>| value.clone().unwrap_or_else(|| "-".into());
        let volume = if self.volume { "yes" } else { "-" };
        format!(
            "| {} | {} | {} | {volume} | {} | {} | {} |",
            self.hop.name(),
            field(&self.client),
            field(&self.pair),
            field(&self.destination),
            field(&self.service),
            field(&self.peer),
        )
    }

    /// The issue's table: one header and one row for each hop, in the order
    /// `first hop`, `relay`, `far hop`.
    pub fn render(rows: &[Knowledge]) -> String {
        let mut out = String::from(
            "| Hop | Client address | Relay pair | Volume | Destination | Service | Peer identity |\n\
             | --- | --- | --- | --- | --- | --- | --- |\n",
        );
        for row in rows {
            out.push_str(&row.line());
            out.push('\n');
        }
        out
    }
}

impl fmt::Display for Knowledge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.line())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_has_one_row_for_each_hop_and_renders_it() {
        let rows = Knowledge::table("192.0.2.10:51515", "relay-a pair-1", "client-key", "db.internal:5432", "postgres");
        let hops: Vec<Hop> = rows.iter().map(|row| row.hop).collect();
        assert_eq!(hops, Hop::ALL);
        let table = Knowledge::render(&rows);
        assert!(
            table.starts_with("| Hop | Client address | Relay pair | Volume | Destination | Service | Peer identity |")
        );
        assert_eq!(table.lines().count(), 5, "a header, a separator and three rows");
        assert!(table.contains("| first hop | 192.0.2.10:51515 | relay-a pair-1 | yes | - | - | - |"));
        assert!(table.contains("| far hop | - | relay-a pair-1 | yes | db.internal:5432 | postgres | client-key |"));
    }
}
