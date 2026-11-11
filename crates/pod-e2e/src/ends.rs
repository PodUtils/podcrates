//! The two ends of the channel as a session runs them. A node answers each
//! operator, checks its key, and only then reaches its target, so a refused
//! operator costs the target no connection and gets none of its bytes. An
//! operator checks the node's key before it proves its own. What the
//! operator asks ([`Ask`]) is a route, or of an exit node a target of its
//! own, which a node that is no exit refuses. An operator's grant used once
//! is the node's [`Redeem`]'s to judge, in the place of its admission. A
//! session let in gets the seed of its tokens of one leg in the verdict,
//! where the node's socket is to a role that registers them, and an
//! operator's end gives it to the legs of its session ([`legs`]). A node's
//! person, where the node has one, is asked after its door and after a
//! grant alike ([`person`]), and the node's event says by what each
//! operator came in.
//!
//! A node's lines are events of its [`Node::events`], one kind for each
//! step: [`EVENT_HANDSHAKE`], [`EVENT_REFUSED`], [`EVENT_GRANT`],
//! [`EVENT_TARGET`], [`EVENT_IN`] and [`EVENT_ENDED`]; the operator's one
//! fact of the handshake is [`EVENT_NO_NET`].

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Event, Events};
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream};

use crate::hold::Holds;
use crate::legs::{self, Leases};
use crate::net::Docs;
use crate::person::{self, Pass};
use crate::{Ask, Error, Refusal, initiate_with, respond_with};
use pod_identity::{Identity, PublicKey};

tokio::task_local! {
    /// Set while a node opens the target of a session that a grant let in:
    /// the grant named the route, so the route's own check of the key is
    /// done.
    static GRANTED: bool;
}

/// Whether the target that opens now is that of a session that a grant let
/// in, whose route the grant named.
pub fn granted() -> bool {
    GRANTED.try_with(|granted| *granted).unwrap_or(false)
}

/// Each direction of the pipe between the channel and the stream under it.
const PIPE: usize = 256 * 1024;
/// The limit of a node's dial of its target, within the operator's wait for
/// the verdict.
pub const TARGET_WAIT: Duration = Duration::from_secs(20);

/// The event of a session that ended in the channel's handshake.
pub const EVENT_HANDSHAKE: &str = "e2e.handshake";
/// The event of an operator that the node refused: a no of its person that
/// stands, its admission, a grant that failed, or its person.
pub const EVENT_REFUSED: &str = "e2e.refused";
/// The event of a grant that let a key in, or that the node set aside.
pub const EVENT_GRANT: &str = "e2e.grant";
/// The event of a target that refused the operator, or did not answer.
pub const EVENT_TARGET: &str = "e2e.target";
/// The event of an operator that came in, and by what.
pub const EVENT_IN: &str = "e2e.in";
/// The event of a session that ended with an error, after the handshake.
pub const EVENT_ENDED: &str = "e2e.ended";
/// The operator's event of a node that gave no document of the net.
pub const EVENT_NO_NET: &str = "e2e.no-net";

/// The event of a session that ended with an error.
pub(crate) fn ended(text: String) -> Event {
    Event::warn(EVENT_ENDED, text)
}

/// Who can reach a node, for what it asked: `Err` is why not, for the
/// node's event; the operator learns only that it is not let in.
pub type Admit = dyn Fn(&PublicKey, &Ask) -> Result<(), String> + Send + Sync;

/// A node's check of an operator's grant: the operator's key, which the
/// channel proved, what it asked, and the grant's bytes. `Ok` lets it in,
/// once, with the words of the node's event; `Err` is why not, for the
/// node's event.
pub type Redeem = dyn Fn(&PublicKey, &Ask, &[u8]) -> Result<String, String> + Send + Sync;

/// The refusal of an exit's target by a node that is no exit.
pub const NO_EXIT: &str = "this node is no exit: it lends no egress";

/// What an operator whose grant the node refused hears: the reasons stay in
/// the node's events, as they can name its files.
pub const GRANT_REFUSED: &str = "this grant does not let this key in: it was used already, its window is not now, or \
                                 it is not for this key, this node or this route; the node's log says which";

/// A node's end: its key, who can come in, where its events go, its door
/// for grants, its tokens of one leg, its person, its documents of the net
/// and the holds of its channels.
#[derive(Clone)]
pub struct Node {
    pub identity: Arc<Identity>,
    pub admit: Arc<Admit>,
    /// Where the node's lines go, as events of the kinds `e2e.*`.
    pub events: Arc<dyn Events>,
    /// The check of a grant used once; with none, a grant is set aside and
    /// the admission decides.
    pub redeem: Option<Arc<Redeem>>,
    /// The tokens of one leg of each session let in, which the node's
    /// socket to its role registers; each clone shares them.
    pub leases: Arc<Leases>,
    /// The node's person, asked of each key that the door or a grant let
    /// through; with none, they decide alone.
    pub gate: Option<Arc<person::Gate>>,
    /// The node's documents of the net, read for each session; with none,
    /// the node answers each offer with a plain proof.
    pub docs: Option<Arc<dyn Docs>>,
    /// The holds of the node's channels, which a handover asks; each clone,
    /// and each road of the node, shares them.
    pub holds: Arc<Holds>,
}

impl Node {
    /// A node that lets in each operator whose key proves itself: the relay's
    /// connect token decided who reaches it.
    pub fn open_to_all(identity: Arc<Identity>, events: Arc<dyn Events>) -> Node {
        let admit: Arc<Admit> = Arc::new(|_: &PublicKey, _: &Ask| Ok(()));
        Node {
            identity,
            admit,
            events,
            redeem: None,
            leases: Arc::default(),
            gate: None,
            docs: None,
            holds: Arc::default(),
        }
    }

    fn say(&self, kind: &'static str, text: String) {
        let event = match kind {
            EVENT_GRANT | EVENT_IN => Event::info(kind, text),
            _ => Event::warn(kind, text),
        };
        self.events.event(event);
    }
}

/// Serve one session's channel on `link`, the stream that the layer or the
/// relay carries: the handshake, the operator's key against the node's
/// admission, then the target from `open`, and the bytes between the two. A
/// node served so has no routes and is no exit: an operator that names a
/// route, or an exit's target, is refused.
pub async fn serve_node<L, A, O, F>(link: L, node: &Node, open: O)
where
    L: AsyncRead + AsyncWrite + Unpin,
    // A handover takes the target to the next binary.
    A: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    O: FnOnce() -> F,
    F: Future<Output = Result<A, String>>,
{
    serve_routed(link, node, |_, route: Option<String>| async move {
        match route {
            Some(route) => Err(Refusal::Other(format!("this node has no routes, so none named {route}"))),
            None => open().await.map_err(Refusal::NoTarget),
        }
    })
    .await
}

/// [`serve_node`] for a node with routes: once the operator's key is let
/// in, `open` gets it and the route that it named, and gives the target of
/// that route, or the refusal: a route that the key must not use is the
/// node's refusal, not its target's. Such a node is no exit.
pub async fn serve_routed<L, A, O, F>(link: L, node: &Node, open: O)
where
    L: AsyncRead + AsyncWrite + Unpin,
    // A handover takes the target to the next binary.
    A: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    O: FnOnce(PublicKey, Option<String>) -> F,
    F: Future<Output = Result<A, Refusal>>,
{
    serve_asked(link, node, move |key, ask: Ask| async move {
        match ask {
            Ask::Exit { .. } => Err(Refusal::Other(NO_EXIT.into())),
            Ask::Route(route) => open(key, Some(route)).await,
            Ask::Target => open(key, None).await,
        }
    })
    .await
}

/// [`serve_routed`] for each ask, an exit's target too: `open` gets the
/// operator's key, once the admission let it in for what it asked, and the
/// ask, and gives the target or the refusal.
pub async fn serve_asked<L, A, O, F>(link: L, node: &Node, open: O)
where
    L: AsyncRead + AsyncWrite + Unpin,
    // A handover takes the target to the next binary.
    A: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    O: FnOnce(PublicKey, Ask) -> F,
    F: Future<Output = Result<A, Refusal>>,
{
    let asked = match respond_with(link, &node.identity, node.docs.clone()).await {
        Ok(asked) => asked,
        Err(e) => {
            node.say(EVENT_HANDSHAKE, format!("a session ended in the channel's handshake: {e}"));
            return;
        }
    };
    let peer = asked.peer();
    let ask = asked.ask().clone();
    let to = ask.to_string();
    // A no of the node's person stands before the door, so that it spends
    // no grant.
    if let Some(why) = node.gate.as_ref().and_then(|gate| gate.standing(&peer)) {
        node.say(EVENT_REFUSED, format!("refused the operator key {peer}{to}: {why}"));
        asked.refuse(Refusal::NotAllowed).await;
        return;
    }
    // A grant decides alone: a key that the admission lets in, but that
    // brings a grant that fails, is refused, as it asked by the grant.
    let verdict = match (asked.grant(), &node.redeem) {
        (Some(grant), Some(redeem)) => match redeem(&peer, &ask, grant) {
            Ok(line) => Ok((true, Some(format!("the operator key {peer} came with {line}")))),
            Err(why) => Err((
                format!("refused the grant of the operator key {peer}: {why}"),
                Refusal::Other(GRANT_REFUSED.into()),
            )),
        },
        (grant, _) => {
            let aside = grant.map(|_| format!("set aside the grant of the operator key {peer}: this end reads none"));
            match (node.admit)(&peer, &ask) {
                Ok(()) => Ok((false, aside)),
                Err(why) => Err((format!("refused the operator key {peer}: {why}"), Refusal::NotAllowed)),
            }
        }
    };
    let by_grant = match verdict {
        Ok((by_grant, line)) => {
            if let Some(line) = line {
                node.say(EVENT_GRANT, line);
            }
            by_grant
        }
        Err((line, refusal)) => {
            node.say(EVENT_REFUSED, line);
            asked.refuse(refusal).await;
            return;
        }
    };
    // The node's person, after the door or the grant, whichever let the key
    // through: neither passes it, and it lets in nothing that they refused.
    // Its pass holds the key's cover while the session lives.
    let (_pass, by) = match Box::pin(person_of(node, peer, &ask, by_grant)).await {
        Ok(passed) => passed,
        Err(line) => {
            node.say(EVENT_REFUSED, line);
            asked.refuse(Refusal::NotAllowed).await;
            return;
        }
    };
    let target = match tokio::time::timeout(TARGET_WAIT, GRANTED.scope(by_grant, open(peer, ask))).await {
        Ok(Ok(target)) => target,
        Ok(Err(refusal)) => {
            node.say(EVENT_TARGET, format!("the operator key {peer} came in{to}; refused: {refusal}"));
            asked.refuse(refusal).await;
            return;
        }
        Err(_) => {
            let why = format!("it did not answer within {} s", TARGET_WAIT.as_secs());
            node.say(
                EVENT_TARGET,
                format!("the operator key {peer} came in{to}; the target could not be reached: {why}"),
            );
            asked.refuse(Refusal::NoTarget(why)).await;
            return;
        }
    };
    node.say(EVENT_IN, format!("the operator key {peer} came in{to}, by {by}"));
    // The seed of the session's tokens of one leg, registered at the role
    // first; they go with the session, as the lease does.
    let lease = node.leases.issue();
    let channel = match asked.admit_with(lease.as_ref().map(legs::Lease::seed)).await {
        Ok(channel) => channel,
        Err(e) => {
            node.events.event(ended(format!("the session of {peer} ended before it began: {e}")));
            return;
        }
    };
    // A handover holds the carry between two messages, and takes the
    // channel and its target to the next binary.
    let hold = crate::hold::current();
    let carried = channel.carry_held(target, hold.as_deref(), crate::Start::default()).await;
    crate::hold::after(carried, hold.as_deref(), peer, lease, &*node.events).await;
}

/// The gate of the node's person: by what the session came in, for the
/// node's event, and the pass that holds the key's cover while the session
/// lives; `Err` is the text of the refusal. With no gate, the door or the
/// grant let it in alone.
async fn person_of(node: &Node, peer: PublicKey, ask: &Ask, by_grant: bool) -> Result<(Option<Pass>, String), String> {
    let door = if by_grant { "a grant" } else { "the door" };
    let Some(gate) = &node.gate else { return Ok((None, door.to_string())) };
    match gate.cross(peer, ask, by_grant).await {
        Ok((pass, by)) => Ok((Some(pass), format!("{door} and {by}"))),
        Err(why) => Err(format!("refused the operator key {peer}{ask}, which {door} let through: {why}")),
    }
}

/// The operator's end of one session: `app` carries the session's bytes in
/// plain, and the stream returned goes to the layer, or to the relay's leg.
/// `check` judges the node's key before this end proves its own; its `Err`
/// refuses the node. The future ends with the session, with the node's key.
/// This end gives no document of the net.
pub fn operator<A, C>(
    app: A,
    identity: Arc<Identity>,
    check: C,
) -> (DuplexStream, impl Future<Output = Result<PublicKey, Error>> + Send)
where
    A: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    C: FnOnce(&PublicKey) -> Result<(), String> + Send + 'static,
{
    operator_to(app, identity, check, None)
}

/// [`operator`] to `route` of the node, when one is named: the word of a
/// route, or of an exit's target ([`Ask::word`]).
pub fn operator_to<A, C>(
    app: A,
    identity: Arc<Identity>,
    check: C,
    route: Option<String>,
) -> (DuplexStream, impl Future<Output = Result<PublicKey, Error>> + Send)
where
    A: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    C: FnOnce(&PublicKey) -> Result<(), String> + Send + 'static,
{
    operator_granted(app, identity, check, route, None)
}

/// [`operator_to`] with `grant`, a grant used once, in this end's proof: it
/// goes only to a node whose key `check` let through.
pub fn operator_granted<A, C>(
    app: A,
    identity: Arc<Identity>,
    check: C,
    route: Option<String>,
    grant: Option<Vec<u8>>,
) -> (DuplexStream, impl Future<Output = Result<PublicKey, Error>> + Send)
where
    A: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    C: FnOnce(&PublicKey) -> Result<(), String> + Send + 'static,
{
    operator_told(app, identity, check, route, grant, None, |_| {})
}

/// What an operator hears, once, of a node that answered its offer of the
/// net's document with a plain proof: such a node lets the operator in by
/// its own door, so the session goes on, and this says so where else
/// nothing would.
pub const NO_NET: &str = "the node answered with no document of the net (it holds none, or it is of an older \
                          release); the session goes on without it";

/// [`operator_granted`], with `docs`, this end's documents of the net, and
/// `told`, which hears a fact of the handshake that ends nothing, once the
/// node let this end in: [`NO_NET`], as an event of the kind
/// [`EVENT_NO_NET`].
pub fn operator_told<A, C, T>(
    app: A,
    identity: Arc<Identity>,
    check: C,
    route: Option<String>,
    grant: Option<Vec<u8>>,
    docs: Option<Arc<dyn Docs>>,
    told: T,
) -> (DuplexStream, impl Future<Output = Result<PublicKey, Error>> + Send)
where
    A: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    C: FnOnce(&PublicKey) -> Result<(), String> + Send + 'static,
    T: FnOnce(Event) + Send + 'static,
{
    let (ours, theirs) = tokio::io::duplex(PIPE);
    let session = async move {
        let offered = initiate_with(theirs, &identity, docs).await?;
        let node = offered.peer();
        if let Err(why) = check(&node) {
            offered.reject().await;
            return Err(Error::NodeKey(why));
        }
        let unanswered = offered.net_unanswered();
        let channel = offered.accept_granted(route.as_deref(), grant.as_deref()).await?;
        if unanswered {
            told(Event::info(EVENT_NO_NET, NO_NET));
        }
        // The node's seed of the session's tokens of one leg, for the legs
        // that resume it.
        if let (Some(seed), Some(taker)) = (channel.leg_seed(), legs::current()) {
            taker.give(seed);
        }
        channel.carry(app).await?;
        Ok(node)
    };
    (ours, session)
}
