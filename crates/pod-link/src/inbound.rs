//! What a frame from the peer does to the state of the link.

use tokio::time::Instant;

use crate::error::{Error, GoAwayCode, ResetCode};
use crate::frame::Frame;
use crate::state::State;

/// The most credit that a stream or the link can hold. A peer that gives
/// more breaks the protocol (a window is at most 8 MiB).
const MAX_CREDIT: u64 = u32::MAX as u64;

impl State {
    /// Apply one frame of the peer. An error ends the link.
    pub fn on_frame(&mut self, frame: Frame) -> Result<(), Error> {
        match frame {
            Frame::Open(id) => self.on_open(id),
            Frame::Data(id, body) => self.on_data(id, body),
            Frame::Window(0, more) => {
                self.send_credit += u64::from(more);
                if self.send_credit > MAX_CREDIT {
                    return Err(Error::Protocol("the credit of the link is over 4 GiB".into()));
                }
                self.unblock();
                Ok(())
            }
            Frame::Window(id, more) => {
                let Some(slot) = self.streams.get_mut(&id) else { return self.gone(id, "WINDOW") };
                slot.send_credit += u64::from(more);
                if slot.send_credit > MAX_CREDIT {
                    return Err(Error::Protocol(format!("the credit of the stream {id} is over 4 GiB")));
                }
                let w = slot.write_waker.take();
                self.wake(w);
                Ok(())
            }
            Frame::Close(id) => {
                let Some(slot) = self.streams.get_mut(&id) else { return self.gone(id, "CLOSE") };
                slot.remote_fin = true;
                let w = slot.read_waker.take();
                self.wake(w);
                Ok(())
            }
            Frame::Reset(id, code) => {
                let Some(slot) = self.streams.get_mut(&id) else { return self.gone(id, "RESET") };
                if slot.reset.is_none() {
                    slot.reset = Some(Error::Reset { id, code: ResetCode(code), by_peer: true });
                }
                let wakers = [slot.read_waker.take(), slot.write_waker.take()];
                for w in wakers {
                    self.wake(w);
                }
                Ok(())
            }
            Frame::Ping(value) => {
                self.push_urgent(Frame::Pong(value));
                Ok(())
            }
            Frame::Pong(value) => {
                if let Some((sent, at)) = self.ping
                    && sent == value
                {
                    self.rtt = Some(at.elapsed());
                    self.ping = None;
                }
                Ok(())
            }
            Frame::GoAway(code) => {
                let e = match GoAwayCode(code) {
                    GoAwayCode::NORMAL => Error::Closed { by_peer: true },
                    other => Error::PeerGoAway(other),
                };
                self.fail(e);
                Ok(())
            }
        }
    }

    /// Bytes or a silence from the peer: the keepalive counts from now.
    pub fn heard(&mut self) {
        self.last_heard = Instant::now();
    }

    fn on_open(&mut self, id: u32) -> Result<(), Error> {
        if self.role.owns(id) || id <= self.last_peer {
            return Err(Error::Protocol(format!("an OPEN of the stream {id}: a wrong parity or an id used before")));
        }
        self.last_peer = id;
        if self.closing || self.streams.len() >= self.settings.max_streams {
            self.push_out(Frame::Reset(id, ResetCode::REFUSED.0));
            return Ok(());
        }
        self.insert_peer(id);
        self.accepted(id);
        Ok(())
    }

    fn on_data(&mut self, id: u32, body: Vec<u8>) -> Result<(), Error> {
        let n = body.len();
        if n as u64 > self.recv_left {
            return Err(Error::Protocol(format!("{n} bytes on the stream {id}, past the window of the link")));
        }
        self.recv_left -= n as u64;
        let Some(slot) = self.streams.get_mut(&id) else {
            if self.is_old(id) {
                self.discard(n);
                return Ok(());
            }
            return Err(Error::Protocol(format!("DATA on the stream {id}, which was never opened")));
        };
        if slot.remote_fin {
            return Err(Error::Protocol(format!("DATA on the stream {id} after its CLOSE")));
        }
        if n as u64 > slot.recv_left {
            return Err(Error::Protocol(format!("{n} bytes on the stream {id}, past its window")));
        }
        slot.recv_left -= n as u64;
        if slot.reset.is_some() {
            self.discard(n);
            return Ok(());
        }
        slot.buf.extend(body);
        let w = slot.read_waker.take();
        self.wake(w);
        Ok(())
    }

    /// A frame for a stream that is not in the table: dropped when the
    /// stream existed, else an error.
    fn gone(&self, id: u32, what: &str) -> Result<(), Error> {
        if self.is_old(id) {
            Ok(())
        } else {
            Err(Error::Protocol(format!("{what} on the stream {id}, which was never opened")))
        }
    }
}
