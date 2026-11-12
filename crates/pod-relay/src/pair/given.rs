//! A node's pair from the parts that its user gives: the relay, the name and
//! the node token, as podvm's `node --relay --name --node-token` takes them
//! (call C130). The user got the pair elsewhere, so the other two tokens and
//! the expiry are not known here.

use zeroize::Zeroizing;

use super::{Pair, PairError};
use crate::cache::valid_token;
use crate::relay::{Relay, check_node_name};

/// The expiry of a pair whose expiry the node does not know: never. A `403`
/// is then a refused token, not an expiry, and the relay's Close
/// `1001 pair expired` ends the node.
pub const UNKNOWN_EXPIRY: i64 = i64::MAX;

impl Pair {
    /// The node's part of a pair: `name` on `relay`, with `node_token`. The
    /// name and the token must have the shape that the relay gives; the
    /// error names the part and never shows the token. The connect token and
    /// the stop token are empty: such a pair runs a node, and nothing else.
    pub fn for_node(relay: Relay, name: &str, node_token: &str) -> Result<Pair, PairError> {
        check_node_name(name).map_err(PairError::BadLabel)?;
        if !valid_token(node_token) {
            return Err(PairError::BadLabel(
                "the node token does not have the shape of a token (8 to 4096 visible ASCII characters)".into(),
            ));
        }
        Ok(Pair {
            relay,
            name: name.to_string(),
            node_token: Zeroizing::new(node_token.to_string()),
            connect_token: Zeroizing::new(String::new()),
            stop_token: Zeroizing::new(String::new()),
            expires_ms: UNKNOWN_EXPIRY,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relay() -> Relay {
        Relay { host: "relay.example".into(), port: 443 }
    }

    #[test]
    fn a_node_part_keeps_its_name_and_token_and_has_no_other_token() {
        let pair = Pair::for_node(relay(), "podvm-node_1.a", "testonlynodetoken").expect("a node part");
        assert_eq!((pair.name.as_str(), pair.node_token()), ("podvm-node_1.a", "testonlynodetoken"));
        assert_eq!((pair.connect_token(), pair.stop_token()), ("", ""));
        assert_eq!(pair.expires_ms, UNKNOWN_EXPIRY);
        assert!(!format!("{pair:?}").contains("testonlynodetoken"), "Debug shows no token");
    }

    #[test]
    fn a_name_that_changes_the_path_and_a_token_of_another_shape_are_refused() {
        for name in ["", "a/b", "has space", "-lead", &"n".repeat(129)] {
            let e = Pair::for_node(relay(), name, "testonlynodetoken").expect_err(name);
            assert!(matches!(e, PairError::BadLabel(_)), "{name:?}: {e:?}");
        }
        for token in ["short", "has a blank", "tab\there12", "ünïcödé-token"] {
            let e = Pair::for_node(relay(), "n1", token).expect_err(token);
            assert!(matches!(&e, PairError::BadLabel(why) if !why.contains(token)), "{e:?}");
        }
    }
}
