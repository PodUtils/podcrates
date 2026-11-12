//! The helpers of the tests of the tcp carrier: made-up identities, a way
//! out that dials the loopback and counts its dials, a way out that fails,
//! and the rustls error under a carrier's error.

use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pod_core::{BoxFuture, Class, Connected, Dial, Failure, Stage, Target};
use pod_identity::{Identity, KeyName, PublicKey};
use pod_link::CarrierError;
use pod_link::tcp::{Keys, Tcp, TlsListener};
use rustls::crypto::CryptoProvider;

/// A made-up identity: the seed is `n` in each byte.
pub fn id(n: u8) -> Arc<Identity> {
    Arc::new(Identity::from_seed(&[n; 32]))
}

pub fn ring() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// A way out that connects to an address of the loopback and counts its
/// dials.
#[derive(Default)]
pub struct Direct {
    pub dials: AtomicUsize,
    pub last: Mutex<Option<Target>>,
}

impl Dial for Direct {
    fn label(&self) -> String {
        "test-direct".into()
    }

    fn dial<'a>(
        &'a self,
        target: &'a Target,
        _deadline: tokio::time::Instant,
    ) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            self.dials.fetch_add(1, Ordering::SeqCst);
            *self.last.lock().unwrap() = Some(target.clone());
            let Target::Addr(addr) = target else {
                return Err(Failure::new(Stage::Dns, Class::NotFound, format!("{target}: the test resolves no name")));
            };
            let tcp = tokio::net::TcpStream::connect(addr)
                .await
                .map_err(|e| Failure::new(Stage::Connect, Class::of_io(e.kind()), e.to_string()))?;
            Ok(Connected::new(Box::new(tcp)))
        })
    }
}

/// A way out that fails each dial with `class`.
pub struct Fails(pub Class);

impl Dial for Fails {
    fn label(&self) -> String {
        "test-fails".into()
    }

    fn dial<'a>(
        &'a self,
        target: &'a Target,
        _deadline: tokio::time::Instant,
    ) -> BoxFuture<'a, Result<Connected, Failure>> {
        let class = self.0;
        Box::pin(async move { Err(Failure::new(Stage::Proxy, class, format!("the test refuses {target}"))) })
    }
}

/// A carrier of the identity `n` on ring, over `way`, whose listeners
/// take the keys of `clients`.
pub fn carrier(n: u8, way: Arc<dyn Dial>, clients: &[PublicKey]) -> Tcp {
    Tcp::new(ring(), id(n), way).with_clients(Keys::new(clients.iter().map(|k| KeyName::Key(*k))))
}

/// A listener of `tcp` on a free port of the loopback.
pub async fn bound(tcp: &Tcp) -> TlsListener {
    tcp.bind("127.0.0.1:0".parse().unwrap()).await.expect("bind on the loopback")
}

/// The dial address of the listener `door` whose key is `key`.
pub fn at(key: &PublicKey, door: &TlsListener) -> String {
    format!("{}@{}", key.hex(), door.local_addr())
}

/// The rustls error under an error of the carrier, when a handshake made
/// it.
pub fn rustls_error(e: &CarrierError) -> Option<rustls::Error> {
    let io = std::error::Error::source(e)?.downcast_ref::<io::Error>()?;
    io.get_ref()?.downcast_ref::<rustls::Error>().cloned()
}
