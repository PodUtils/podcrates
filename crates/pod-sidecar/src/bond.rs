//! A per-flow scheduler over two or more links (rule BOND-1). A flow goes on
//! one link and stays there. It moves only when that link fails. It is never
//! a per-packet round robin, because a packet that leaves on another link
//! changes the path of a stream in the middle of it.
//!
//! The cost is stated: one flow cannot use two links, so a single session
//! gets the capacity of its best link and not the sum of all links. Rule
//! BOND-2 asks for a session within 20 percent of the sum on two links at
//! 10:1, and 10 of 11 is 91 percent.

use std::collections::BTreeMap;
use std::fmt;

/// The name of a flow: a 5-tuple or another stable key of the caller.
pub type FlowId = u64;
/// The name of a link.
pub type LinkId = u8;

/// One link of the bond.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    pub id: LinkId,
    /// The capacity in bytes for each tick. A test gives 10 and 1.
    pub capacity: u64,
    pub failed: bool,
    pub flows: usize,
    pub bytes: u64,
}

/// A failure of the scheduler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BondError {
    /// The flow is not assigned.
    UnknownFlow(FlowId),
    /// The link is not in the bond.
    UnknownLink(LinkId),
    /// The link has no capacity.
    NoCapacity(LinkId),
    /// A link with this id is in the bond twice.
    DuplicateLink(LinkId),
    /// No live link can carry the flow.
    NoLink,
}

impl fmt::Display for BondError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BondError::UnknownFlow(flow) => write!(f, "flow {flow} is not assigned"),
            BondError::UnknownLink(link) => write!(f, "link {link} is not in the bond"),
            BondError::NoCapacity(link) => write!(f, "link {link} has no capacity"),
            BondError::DuplicateLink(link) => write!(f, "link {link} is in the bond twice"),
            BondError::NoLink => f.write_str("no live link can carry the flow"),
        }
    }
}

impl std::error::Error for BondError {}

/// The bond: its links and its flows.
#[derive(Clone, Debug)]
pub struct Bond {
    links: Vec<Link>,
    flows: BTreeMap<FlowId, LinkId>,
    moves: u64,
}

impl Bond {
    /// A bond of the links and their capacities. The order of the links is
    /// the order of a tie.
    pub fn new(links: &[(LinkId, u64)]) -> Result<Bond, BondError> {
        let mut bond = Bond { links: Vec::with_capacity(links.len()), flows: BTreeMap::new(), moves: 0 };
        for (id, capacity) in links {
            if *capacity == 0 {
                return Err(BondError::NoCapacity(*id));
            }
            if bond.links.iter().any(|link| link.id == *id) {
                return Err(BondError::DuplicateLink(*id));
            }
            bond.links.push(Link { id: *id, capacity: *capacity, failed: false, flows: 0, bytes: 0 });
        }
        if bond.links.is_empty() {
            return Err(BondError::NoLink);
        }
        Ok(bond)
    }

    /// Assign a new flow to the live link with the least load for its
    /// capacity. A tie goes to the larger capacity, then to the earlier link.
    /// A flow that is already assigned keeps its link.
    pub fn assign(&mut self, flow: FlowId) -> Option<LinkId> {
        if let Some(link) = self.flows.get(&flow) {
            return Some(*link);
        }
        let id = self.best_link(None)?;
        if let Some(link) = self.links.iter_mut().find(|link| link.id == id) {
            link.flows += 1;
        }
        self.flows.insert(flow, id);
        Some(id)
    }

    /// The link of a flow.
    pub fn link_of(&self, flow: FlowId) -> Option<LinkId> {
        self.flows.get(&flow).copied()
    }

    /// Send `bytes` of `flow`. A flow whose link failed moves to the best
    /// live link, and only then.
    pub fn send(&mut self, flow: FlowId, bytes: u64) -> Result<LinkId, BondError> {
        let Some(id) = self.flows.get(&flow).copied() else {
            return Err(BondError::UnknownFlow(flow));
        };
        let failed = self.link(id).map(|link| link.failed).unwrap_or(true);
        let id = if failed {
            let next = self.best_link(Some(id)).ok_or(BondError::NoLink)?;
            self.reassign(flow, id);
            self.moves += 1;
            next
        } else {
            id
        };
        if let Some(link) = self.links.iter_mut().find(|link| link.id == id) {
            link.bytes = link.bytes.saturating_add(bytes);
        }
        Ok(id)
    }

    /// Mark a link as failed and move each of its flows to the best live
    /// link. A flow does not move for any other reason.
    pub fn fail(&mut self, id: LinkId) {
        {
            let Some(link) = self.links.iter_mut().find(|link| link.id == id) else { return };
            if link.failed {
                return;
            }
            link.failed = true;
        }
        let flows: Vec<FlowId> = self.flows.iter().filter(|(_, link)| **link == id).map(|(flow, _)| *flow).collect();
        for flow in flows {
            if let Some(next) = self.best_link(Some(id)) {
                self.reassign(flow, next);
                self.moves += 1;
            }
        }
    }

    /// The capacity of the link of a flow, when that link is live.
    pub fn estimate(&self, flow: FlowId) -> Option<u64> {
        let id = self.link_of(flow)?;
        self.link(id).filter(|link| !link.failed).map(|link| link.capacity)
    }

    /// The sum of the capacities of the live links.
    pub fn sum(&self) -> u64 {
        self.links.iter().filter(|link| !link.failed).map(|link| link.capacity).sum()
    }

    pub fn link(&self, id: LinkId) -> Option<&Link> {
        self.links.iter().find(|link| link.id == id)
    }

    pub fn links(&self) -> &[Link] {
        &self.links
    }

    pub fn moves(&self) -> u64 {
        self.moves
    }

    fn reassign(&mut self, flow: FlowId, next: LinkId) {
        if let Some(previous) = self.flows.get(&flow).copied()
            && let Some(link) = self.links.iter_mut().find(|link| link.id == previous)
        {
            link.flows = link.flows.saturating_sub(1);
        }
        if let Some(link) = self.links.iter_mut().find(|link| link.id == next) {
            link.flows += 1;
        }
        self.flows.insert(flow, next);
    }

    fn best_link(&self, except: Option<LinkId>) -> Option<LinkId> {
        let mut best: Option<&Link> = None;
        for link in self.links.iter().filter(|link| !link.failed && Some(link.id) != except) {
            let Some(current) = best else {
                best = Some(link);
                continue;
            };
            let lhs = link.flows as u128 * current.capacity as u128;
            let rhs = current.flows as u128 * link.capacity as u128;
            let take = lhs < rhs || (lhs == rhs && link.capacity > current.capacity);
            if take {
                best = Some(link);
            }
        }
        best.map(|link| link.id)
    }
}
