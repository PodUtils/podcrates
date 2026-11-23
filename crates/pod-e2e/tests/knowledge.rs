//! The hop-knowledge table: the first hop and the relay learn no destination
//! and no service name, and the far hop learns both, with the identity of
//! its peer. The rendered table says the same.

use pod_e2e::Knowledge;
use pod_e2e::knowledge::Hop;

const CLIENT: &str = "192.0.2.10:51515";
const PAIR: &str = "relay-a pair-1";
const PEER: &str = "SHA256:client-key";
const DESTINATION: &str = "db.internal:5432";
const HOST: &str = "db.internal";
const SERVICE: &str = "postgres";

#[test]
fn the_first_hop_and_the_relay_learn_no_destination_or_service() {
    let rows = Knowledge::table(CLIENT, PAIR, PEER, DESTINATION, SERVICE);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows.iter().map(|row| row.hop).collect::<Vec<_>>(), Hop::ALL);
    for row in &rows[..2] {
        assert!(row.destination.is_none(), "{}", row.hop);
        assert!(row.service.is_none(), "{}", row.hop);
        assert!(row.volume, "{}", row.hop);
        assert!(!row.line().contains(HOST), "{}", row.hop);
        assert!(!row.line().contains(SERVICE), "{}", row.hop);
    }
    assert_eq!(rows[0].client.as_deref(), Some(CLIENT), "the first hop sees the client");
    assert!(rows[0].peer.is_none(), "the first hop proves no identity");
    assert!(rows[1].client.is_none(), "the relay sees no client address");
    assert_eq!(rows[1].pair.as_deref(), Some(PAIR), "the relay knows its pair");
    let far = &rows[2];
    assert_eq!(far.destination.as_deref(), Some(DESTINATION));
    assert_eq!(far.service.as_deref(), Some(SERVICE));
    assert_eq!(far.peer.as_deref(), Some(PEER));
    assert!(far.line().contains(DESTINATION));
    assert!(far.line().contains(SERVICE));
    assert!(far.line().contains(PEER));
}

#[test]
fn the_rendered_table_carries_the_destination_only_in_the_far_row() {
    let rows = Knowledge::table(CLIENT, PAIR, PEER, DESTINATION, SERVICE);
    let table = Knowledge::render(&rows);
    let lines: Vec<&str> = table.lines().collect();
    assert_eq!(lines.len(), 5, "a header, a separator and one row for each hop");
    assert!(lines[0].contains("Destination") && lines[0].contains("Service"));
    assert!(!lines[2].contains(HOST), "the first hop's row: {}", lines[2]);
    assert!(!lines[3].contains(HOST), "the relay's row: {}", lines[3]);
    assert!(!lines[2].contains(SERVICE) && !lines[3].contains(SERVICE));
    assert!(lines[4].contains(DESTINATION) && lines[4].contains(SERVICE));
    assert!(lines[4].contains(PEER), "the far hop knows its peer");
}
