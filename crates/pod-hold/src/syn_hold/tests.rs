//! The hold of a SYN over a link in the process: a stack of `pod-netstack`
//! stands in for the payload behind its wire, and a pump moves its packets
//! to the hold and back, as podnet's loop moves the packets of the TUN. The
//! dial gives an upstream in memory whose target greets and then echoes.
//! podnet's tests of its netns backend (`tests/pod_seccomp/`) run a program
//! in a namespace; the tests here that keep their names prove the same
//! behaviour of the hold.

use std::io;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicUsize, Ordering};

use pod_netstack::Subnet;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::watch;

use super::*;

mod many;

/// The gateway that the stack of the hold owns, and the address of the
/// payload (podnet's `TUN_GW` and `TUN_ADDR`).
const GATEWAY: Ipv4Addr = Ipv4Addr::new(10, 66, 255, 254);
const PAYLOAD: Ipv4Addr = Ipv4Addr::new(10, 66, 255, 255);
/// A target past the gateway (podnet's tests dial it).
const TARGET: Ipv4Addr = Ipv4Addr::new(93, 184, 216, 34);
const GREETING: &[u8] = b"HELLO-FROM-TARGET\n";
const LIMIT: Duration = Duration::from_secs(10);

type Hold = SynHold<DuplexStream, io::Error>;

fn target(port: u16) -> SocketAddrV4 {
    SocketAddrV4::new(TARGET, port)
}

/// The stack of the hold with podnet's settings, and small buffers for the
/// many flows of a test.
fn hold_config() -> StackConfig {
    StackConfig {
        subnet: Subnet { v4: GATEWAY, v4_prefix: 31, ..Subnet::default() },
        local_v4: GATEWAY,
        mtu: 1500,
        tcp_buffer: 16 * 1024,
        ..StackConfig::default()
    }
}

/// The stack of the payload: each address is on its link, as each address
/// is past the default route of the payload's TUN.
fn payload() -> NetStack {
    NetStack::over_link(StackConfig {
        subnet: Subnet { v4_prefix: 0, v6_prefix: 0, ..Subnet::default() },
        local_v4: PAYLOAD,
        mtu: 1500,
        tcp_buffer: 16 * 1024,
        ..StackConfig::default()
    })
}

/// The target of each upstream: a greeting, then an echo until the end.
async fn greet_and_echo(mut far: DuplexStream) {
    if far.write_all(GREETING).await.is_err() {
        return;
    }
    let mut buffer = vec![0u8; 16 * 1024];
    while let Ok(n) = far.read(&mut buffer).await {
        if n == 0 || far.write_all(&buffer[..n]).await.is_err() {
            break;
        }
    }
    let _ = far.shutdown().await;
}

/// An upstream in memory, with its target.
fn upstream() -> DuplexStream {
    let (near, far) = tokio::io::duplex(64 * 1024);
    tokio::spawn(greet_and_echo(far));
    near
}

/// The dial of the tests. The dial of the port of `gate` waits until the
/// gate is true; with `dead`, each dial fails.
#[derive(Clone)]
struct Stand {
    dials: Arc<watch::Sender<Vec<Syn>>>,
    refused: Arc<Mutex<Vec<(Syn, Refused)>>>,
    gate: Option<(u16, watch::Receiver<bool>)>,
    dead: bool,
}

impl Stand {
    fn new() -> Self {
        Self { dials: Arc::new(watch::Sender::new(Vec::new())), refused: Arc::default(), gate: None, dead: false }
    }

    /// A stand whose dials to `port` wait for the sender that it returns.
    fn gated(port: u16) -> (Self, watch::Sender<bool>) {
        let (open, gate) = watch::channel(false);
        (Self { gate: Some((port, gate)), ..Self::new() }, open)
    }

    fn dial_count(&self) -> usize {
        self.dials.borrow().len()
    }

    /// Wait until the hold started `count` dials.
    async fn started(&self, count: usize) {
        let mut dials = self.dials.subscribe();
        let started = tokio::time::timeout(LIMIT, dials.wait_for(|dials| dials.len() >= count)).await;
        assert!(started.is_ok(), "{count} dials did not start in the time limit");
    }
}

impl Dial for Stand {
    type Upstream = DuplexStream;
    type Error = io::Error;

    fn dial(&self, syn: Syn) -> impl Future<Output = io::Result<DuplexStream>> + Send + 'static {
        self.dials.send_modify(|dials| dials.push(syn));
        let gate = self.gate.clone().filter(|(port, _)| *port == syn.target.port());
        let dead = self.dead;
        async move {
            if let Some((_, mut gate)) = gate {
                let _ = gate.wait_for(|open| *open).await;
            }
            match dead {
                true => Err(io::Error::new(io::ErrorKind::ConnectionRefused, "the upstream is dead")),
                false => Ok(upstream()),
            }
        }
    }

    fn refused(&self, syn: Syn, refused: Refused, _: io::Error) {
        lock(&self.refused).push((syn, refused));
    }
}

/// What the link saw: the RSTs and the SYN-ACKs to the payload, by the
/// port that sent them.
#[derive(Default)]
struct Link {
    resets: AtomicUsize,
    syn_acks: Mutex<HashMap<u16, usize>>,
}

impl Link {
    fn note(&self, packet: &[u8]) {
        let header = usize::from(packet[0] & 0x0f) * 4;
        if packet.len() < header + 14 || packet[0] >> 4 != 4 || packet[9] != 6 {
            return;
        }
        let flags = packet[header + 13];
        if flags & 0x04 != 0 {
            self.resets.fetch_add(1, Ordering::Relaxed);
        }
        if flags & 0x12 == 0x12 {
            let port = u16::from_be_bytes([packet[header], packet[header + 1]]);
            *lock(&self.syn_acks).entry(port).or_insert(0) += 1;
        }
    }

    fn resets(&self) -> usize {
        self.resets.load(Ordering::Relaxed)
    }

    fn syn_acks(&self, port: u16) -> usize {
        lock(&self.syn_acks).get(&port).copied().unwrap_or(0)
    }
}

/// The state of the hold after each pass of the pump.
#[derive(Clone, Copy, Debug, Default)]
struct Seen {
    stats: Stats,
    ports: usize,
    flows: usize,
}

/// The hold and the payload over one link.
struct Router {
    payload: NetStack,
    link: Arc<Link>,
    seen: watch::Receiver<Seen>,
    stop: Arc<Notify>,
    task: JoinHandle<Hold>,
}

/// Each flow copies between the stream and its upstream until both ends
/// closed, then drops both and its entry.
fn serve(flow: Flow<DuplexStream>) {
    tokio::spawn(async move {
        let Flow { syn, mut stream, mut upstream, open } = flow;
        assert_eq!(stream.local_addr(), SocketAddr::V4(syn.target));
        let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
        drop(stream);
        drop(open);
    });
}

/// Start the pump between `payload` and a new hold. `twice` sends each SYN
/// of the payload two times. The pump waits for the payload, the activity
/// of the hold or the stop, with no timer: a wake that the hold misses
/// stops a test at its time limit.
fn router<D>(payload: NetStack, settings: Settings, dial: D, twice: bool) -> Router
where
    D: Dial<Upstream = DuplexStream, Error = io::Error> + Send + Sync + 'static,
{
    let mut hold = Hold::new(hold_config(), settings);
    let (from, to) = (payload.link_out(), payload.link_in());
    let link = Arc::new(Link::default());
    let (publish, seen) = watch::channel(Seen::default());
    let stop = Arc::new(Notify::new());
    let (counts, halt) = (link.clone(), stop.clone());
    let task = tokio::spawn(async move {
        let (mut from_payload, mut to_payload) = (VecDeque::new(), VecDeque::new());
        loop {
            while let Some(packet) = from.pop() {
                if twice && Syn::of(&packet).is_some() {
                    from_payload.push_back(packet.clone());
                }
                from_payload.push_back(packet);
            }
            hold.pump(&mut from_payload, &mut to_payload, &dial).await;
            while let Some(flow) = hold.next_flow() {
                serve(flow);
            }
            for packet in to_payload.drain(..) {
                counts.note(&packet);
                to.push(packet);
            }
            let now = Seen { stats: hold.stats(), ports: hold.listening_ports(), flows: hold.flow_count() };
            publish.send_replace(now);
            tokio::select! {
                _ = from.wait() => {}
                _ = hold.activity() => {}
                _ = halt.notified() => break,
            }
        }
        hold
    });
    Router { payload, link, seen, stop, task }
}

impl Router {
    /// Wait until the hold shows `what`.
    async fn until(&mut self, what: &str, test: impl FnMut(&Seen) -> bool) -> Seen {
        let seen = match tokio::time::timeout(LIMIT, self.seen.wait_for(test)).await {
            Ok(Ok(seen)) => Some(*seen),
            _ => None,
        };
        seen.unwrap_or_else(|| panic!("{what}: not in the time limit ({:?})", *self.seen.borrow()))
    }

    /// Stop the pump and both stacks; the hold at the end.
    async fn stop(mut self) -> Hold {
        self.stop.notify_one();
        let mut hold = self.task.await.unwrap();
        hold.shutdown().await;
        self.payload.shutdown().await;
        hold
    }
}

/// Wait for the handshake of `stream` and the greeting of its target.
async fn greeted(stream: &mut TcpStream) {
    let handshake = tokio::time::timeout(LIMIT, stream.established()).await;
    handshake.expect("no handshake in the time limit").unwrap();
    let mut got = vec![0u8; GREETING.len()];
    let read = tokio::time::timeout(LIMIT, stream.read_exact(&mut got)).await;
    read.expect("no greeting in the time limit").unwrap();
    assert_eq!(got, GREETING);
}

/// Send `bytes` and read them back at the same time.
async fn echoes(stream: TcpStream, bytes: usize) -> TcpStream {
    let data: Vec<u8> = (0..bytes).map(|i| (i % 251) as u8).collect();
    let (mut read, mut write) = tokio::io::split(stream);
    let sent = data.clone();
    let writer = tokio::spawn(async move {
        write.write_all(&sent).await.unwrap();
        write
    });
    let mut got = vec![0u8; bytes];
    let back = tokio::time::timeout(LIMIT, read.read_exact(&mut got)).await;
    back.expect("no echo in the time limit").unwrap();
    assert!(got == data, "the echo differs");
    read.unsplit(writer.await.unwrap())
}

/// The parse of a SYN names its addresses; another packet is no SYN.
#[test]
fn a_syn_of_a_packet_names_its_addresses() {
    let mut packet = vec![0u8; 40];
    packet[0] = 0x45;
    packet[9] = 6;
    packet[12..16].copy_from_slice(&PAYLOAD.octets());
    packet[16..20].copy_from_slice(&TARGET.octets());
    packet[20..22].copy_from_slice(&40000u16.to_be_bytes());
    packet[22..24].copy_from_slice(&443u16.to_be_bytes());
    packet[33] = 0x02;
    let syn = Syn::of(&packet).unwrap();
    assert_eq!(syn, Syn { source: SocketAddrV4::new(PAYLOAD, 40000), target: target(443) });
    packet[33] = 0x12;
    assert_eq!(Syn::of(&packet), None, "a SYN-ACK");
}

/// A connect whose dial succeeds is accepted, and the flow carries bytes
/// both ways. The dial is a closure.
#[tokio::test]
async fn a_syn_whose_dial_succeeds_is_accepted_and_carries_bytes_both_ways() {
    let syns = Arc::new(Mutex::new(Vec::new()));
    let noted = syns.clone();
    let dial = move |syn: Syn| {
        lock(&noted).push(syn);
        async move { Ok::<_, io::Error>(upstream()) }
    };
    let mut router = router(payload(), Settings::default(), dial, false);
    let mut client = router.payload.connect(target(80)).await.unwrap();
    greeted(&mut client).await;
    let client = echoes(client, 256 * 1024).await;
    assert_eq!(
        *lock(&syns),
        [Syn { source: SocketAddrV4::new(PAYLOAD, client.local_addr().port()), target: target(80) }]
    );
    drop(client);
    router.until("the end of the flow", |seen| seen.flows == 0 && seen.ports == 0).await;
    assert_eq!(router.link.resets(), 0);
    let hold = router.stop().await;
    let stats = hold.stats();
    assert_eq!((stats.held, stats.released, stats.flows, stats.late), (1, 1, 1, 0), "{stats:?}");
}

/// podnet's test: a connect whose upstream is dead fails with
/// `ECONNREFUSED` at once (podnet's issue 190), and the stack of the hold
/// never sees its SYN.
#[tokio::test]
async fn netns_backend_refuses_connect_when_the_upstream_is_dead() {
    let stand = Stand { dead: true, ..Stand::new() };
    let router = router(payload(), Settings::default(), stand.clone(), false);
    let start = Instant::now();
    let mut client = router.payload.connect(target(443)).await.unwrap();
    let refused = tokio::time::timeout(LIMIT, client.established()).await.expect("no answer in the time limit");
    assert_eq!(refused.unwrap_err().kind(), io::ErrorKind::ConnectionRefused);
    assert!(start.elapsed() < Duration::from_secs(3), "{:?}", start.elapsed());
    assert_eq!(router.link.resets(), 1);
    assert_eq!(router.link.syn_acks(443), 0);
    let syn = Syn { source: SocketAddrV4::new(PAYLOAD, client.local_addr().port()), target: target(443) };
    assert_eq!(*lock(&stand.refused), [(syn, Refused::Reset)]);
    let hold = router.stop().await;
    assert_eq!(hold.stack().stats().accepted, 0);
    assert_eq!((hold.stats().reset, hold.listening_ports()), (1, 0));
}

/// The stack answers a SYN only after its dial ended. The SYN to port 81
/// waits for its dial; a later connect to port 80 is accepted. Had the stack
/// taken the SYN to port 81 before its dial, its SYN-ACK would be on the
/// link before the one of port 80: the link keeps the order.
#[tokio::test]
async fn the_stack_answers_a_syn_only_after_its_dial() {
    let (stand, open) = Stand::gated(81);
    let router = router(payload(), Settings::default(), stand.clone(), false);
    let mut held = router.payload.connect(target(81)).await.unwrap();
    stand.started(1).await;
    let mut other = router.payload.connect(target(80)).await.unwrap();
    greeted(&mut other).await;
    assert_eq!(router.link.syn_acks(81), 0, "a SYN-ACK before the end of the dial");
    open.send_replace(true);
    greeted(&mut held).await;
    assert!(router.link.syn_acks(81) >= 1);
    assert_eq!(router.link.resets(), 0);
    assert_eq!(stand.dial_count(), 2);
    router.stop().await;
}

/// A SYN sent again while its dial waits is held one time: one dial, and
/// one connection when the dial ends.
#[tokio::test]
async fn a_syn_sent_again_while_its_dial_waits_is_held_once() {
    let (stand, open) = Stand::gated(80);
    let mut router = router(payload(), Settings::default(), stand.clone(), true);
    let mut client = router.payload.connect(target(80)).await.unwrap();
    let seen = router.until("the SYN again", |seen| seen.stats.again >= 1).await;
    assert_eq!((seen.stats.held, stand.dial_count()), (1, 1));
    open.send_replace(true);
    greeted(&mut client).await;
    echoes(client, 4096).await;
    assert_eq!((stand.dial_count(), router.link.resets()), (1, 0));
    let hold = router.stop().await;
    assert_eq!((hold.stats().released, hold.stats().flows), (1, 1));
}
