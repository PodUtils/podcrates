//! A device that gives each packet to a tap of the consumer (a capture, a
//! record for a replay, a comparison of two stacks).

use smoltcp::phy::{Device, DeviceCapabilities, PacketMeta, RxToken, TxToken};
use smoltcp::time::Instant;

/// The way of a packet through a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Link to stack.
    Inbound,
    /// Stack to link.
    Outbound,
}

/// The tap of the consumer: the device calls it with the direction and the
/// bytes of each packet, an empty packet too. It runs in the task of the
/// stack: it must not block.
///
/// A closure `Fn(Direction, &[u8])` is a `PacketTap`.
pub trait PacketTap {
    fn packet(&self, direction: Direction, packet: &[u8]);
}

impl<F: Fn(Direction, &[u8])> PacketTap for F {
    fn packet(&self, direction: Direction, packet: &[u8]) {
        self(direction, packet)
    }
}

/// A device that gives each packet in both directions to a [`PacketTap`].
///
/// It wraps a device and does not read its queues, so it works over the
/// tunnel device and over the loopback device.
pub struct RecordingDevice<D: Device, T: PacketTap> {
    inner: D,
    tap: T,
}

impl<D: Device, T: PacketTap> RecordingDevice<D, T> {
    pub fn new(inner: D, tap: T) -> Self {
        Self { inner, tap }
    }

    pub fn into_inner(self) -> D {
        self.inner
    }

    pub fn tap(&self) -> &T {
        &self.tap
    }
}

impl<D: Device, T: PacketTap> Device for RecordingDevice<D, T> {
    type RxToken<'a>
        = RecordRx<'a, D::RxToken<'a>, T>
    where
        Self: 'a;
    type TxToken<'a>
        = RecordTx<'a, D::TxToken<'a>, T>
    where
        Self: 'a;

    fn receive(&mut self, timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let (rx, tx) = self.inner.receive(timestamp)?;
        Some((RecordRx { inner: rx, tap: &self.tap }, RecordTx { inner: tx, tap: &self.tap }))
    }

    fn transmit(&mut self, timestamp: Instant) -> Option<Self::TxToken<'_>> {
        let tx = self.inner.transmit(timestamp)?;
        Some(RecordTx { inner: tx, tap: &self.tap })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        self.inner.capabilities()
    }
}

/// The receive token of a [`RecordingDevice`].
pub struct RecordRx<'a, R, T> {
    inner: R,
    tap: &'a T,
}

impl<R: RxToken, T: PacketTap> RxToken for RecordRx<'_, R, T> {
    fn consume<Ret, F>(self, f: F) -> Ret
    where
        F: FnOnce(&[u8]) -> Ret,
    {
        let tap = self.tap;
        self.inner.consume(|buffer| {
            tap.packet(Direction::Inbound, buffer);
            f(buffer)
        })
    }

    fn meta(&self) -> PacketMeta {
        self.inner.meta()
    }
}

/// The transmit token of a [`RecordingDevice`].
pub struct RecordTx<'a, X, T> {
    inner: X,
    tap: &'a T,
}

impl<X: TxToken, T: PacketTap> TxToken for RecordTx<'_, X, T> {
    fn consume<Ret, F>(self, len: usize, f: F) -> Ret
    where
        F: FnOnce(&mut [u8]) -> Ret,
    {
        let tap = self.tap;
        self.inner.consume(len, |buffer| {
            let result = f(buffer);
            tap.packet(Direction::Outbound, buffer);
            result
        })
    }

    fn set_meta(&mut self, meta: PacketMeta) {
        self.inner.set_meta(meta)
    }
}
