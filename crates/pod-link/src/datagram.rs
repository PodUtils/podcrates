//! The trait of a datagram carrier: a way to send and take datagrams that
//! can be lost, come out of order or come twice. The ARQ
//! ([`crate::arq::ArqStream`]) makes one ordered byte stream of it, for a
//! [`crate::Link`].
//!
//! [`memory::pair`] gives two ends in one process, and [`lossy::Lossy`]
//! loses, reorders and duplicates the datagrams of a carrier from a seed,
//! for the tests of a consumer.

pub mod lossy;
pub mod memory;

use std::future::Future;
use std::io;
use std::pin::Pin;

/// The future of a datagram operation.
pub type DatagramFuture<'a, T> = Pin<Box<dyn Future<Output = io::Result<T>> + Send + 'a>>;

/// A carrier of datagrams. A datagram that the carrier cannot take now
/// (a full queue, no route) is a loss, not an error: the ARQ sends it
/// again. An error ends the ARQ.
pub trait Datagrams: Send + Sync + 'static {
    /// The largest datagram, in bytes.
    fn mtu(&self) -> usize;

    /// Send one datagram of at most [`Datagrams::mtu`] bytes.
    fn send<'a>(&'a self, datagram: &'a [u8]) -> DatagramFuture<'a, ()>;

    /// The next datagram. The ARQ calls it from one task at a time, and
    /// does not drop the future before it ends.
    fn recv(&self) -> DatagramFuture<'_, Vec<u8>>;
}

impl<D: Datagrams + ?Sized> Datagrams for Box<D> {
    fn mtu(&self) -> usize {
        (**self).mtu()
    }

    fn send<'a>(&'a self, datagram: &'a [u8]) -> DatagramFuture<'a, ()> {
        (**self).send(datagram)
    }

    fn recv(&self) -> DatagramFuture<'_, Vec<u8>> {
        (**self).recv()
    }
}

impl<D: Datagrams + ?Sized> Datagrams for std::sync::Arc<D> {
    fn mtu(&self) -> usize {
        (**self).mtu()
    }

    fn send<'a>(&'a self, datagram: &'a [u8]) -> DatagramFuture<'a, ()> {
        (**self).send(datagram)
    }

    fn recv(&self) -> DatagramFuture<'_, Vec<u8>> {
        (**self).recv()
    }
}
