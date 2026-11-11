//! The end-to-end channel of the facade's sessions (podssh T-088), as a
//! consumer's own ends run it: on by default, with the keys of the cache
//! (podssh T-087), so that a node of the facade and an operator of the
//! consumer meet with it, as two of its ends do. Off, both ends carry the
//! bytes as they are.

use std::sync::Arc;

use pod_core::entropy::OsEntropy;
use pod_core::files::Places;
use pod_e2e::ends;
use pod_identity::{Identity, KeyName, PublicKey, file, pins};

/// The channel of a node's sessions.
#[derive(Clone, Default)]
pub enum NodeChannel {
    /// On: the node's key in the cache under its label (else the pair's
    /// name), made when missing; each operator with the connect token comes
    /// in.
    #[default]
    Default,
    /// On, with this end: its key, who may come in, and where its lines go.
    With(ends::Node),
    /// Off: the operators run with no channel too, and the relay sees each
    /// byte.
    Off,
}

impl std::fmt::Debug for NodeChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeChannel::Default => f.write_str("Default"),
            NodeChannel::With(node) => write!(f, "With({})", node.identity.public()),
            NodeChannel::Off => f.write_str("Off"),
        }
    }
}

impl NodeChannel {
    /// The node's end, for the pair of this name and this label; `None` when
    /// off. A key that cannot be read or made is an error.
    pub(super) fn end(&self, places: &Places, label: Option<&str>, name: &str) -> Result<Option<ends::Node>, String> {
        match self {
            NodeChannel::Off => Ok(None),
            NodeChannel::With(node) => Ok(Some(node.clone())),
            NodeChannel::Default => {
                let key = file::load(places, &file::node_place(label.unwrap_or(name)), &mut OsEntropy)?;
                Ok(Some(ends::Node::open_to_all(Arc::new(key.identity), Arc::new(pod_core::NoEvents))))
            }
        }
    }
}

/// The channel of an operator's session.
#[derive(Clone, Default)]
pub enum OperatorChannel {
    /// On: this client's key in the cache, made when missing, and the node's
    /// key pinned at its first sight under the pair's name; a changed key is
    /// refused.
    #[default]
    Default,
    /// On, with this key, and a node with the key that `node` names.
    With { identity: Arc<Identity>, node: KeyName },
    /// Off: to a node with no channel.
    Off,
}

impl std::fmt::Debug for OperatorChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperatorChannel::Default => f.write_str("Default"),
            OperatorChannel::With { identity, node } => write!(f, "With({}, node {node})", identity.public()),
            OperatorChannel::Off => f.write_str("Off"),
        }
    }
}

/// How an operator judges a node's key.
pub(super) type Check = Box<dyn FnOnce(&PublicKey) -> Result<(), String> + Send>;

impl OperatorChannel {
    /// This client's key and its check of the node's, for the pair `name`;
    /// `None` when off.
    pub(super) fn end(&self, places: &Places, name: &str) -> Result<Option<(Arc<Identity>, Check)>, String> {
        match self {
            OperatorChannel::Off => Ok(None),
            OperatorChannel::With { identity, node } => {
                let node = node.clone();
                let check: Check = Box::new(move |key: &PublicKey| {
                    (node.matches(key)).then_some(()).ok_or_else(|| format!("the node has the key {key}, not {node}"))
                });
                Ok(Some((identity.clone(), check)))
            }
            OperatorChannel::Default => {
                let key = file::load(places, &file::client_place(), &mut OsEntropy)?;
                let name = name.to_string();
                let places = places.clone();
                let check: Check = Box::new(move |key: &PublicKey| match pins::check(&places, &name, key) {
                    Ok(_) => Ok(()),
                    Err(why) => Err(format!("the node {name} has the key {key}, but {why}")),
                });
                Ok(Some((Arc::new(key.identity), check)))
            }
        }
    }
}
