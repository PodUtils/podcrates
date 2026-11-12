use super::*;

// The end of each socket of a stack at once: a reset that the consumer asks
// for (`NetStack::reset_all`, the link of the stack is dead), and the stop of
// the reactor (a shutdown, a drop, a panic). The app then reads the cause as
// `ConnectionReset`, not a clean end, and does not wait for ever.

/// The cause of the end of one socket. The reactor sets it before it drops
/// the channels of the socket; the handle of the app reads it when a channel
/// closes.
pub(super) type Cause = Arc<OnceLock<String>>;

/// The error of an operation on a socket that the stack ended: the cause as
/// `ConnectionReset`, or `otherwise` when the socket ended with no cause.
pub(super) fn ended(cause: &Cause, otherwise: io::Error) -> io::Error {
    match cause.get() {
        Some(why) => io::Error::new(io::ErrorKind::ConnectionReset, why.clone()),
        None => otherwise,
    }
}

/// The stop of a reactor, shared with its handle: the first cause, and a
/// wake of each task that waits for the stop.
#[derive(Default)]
pub(super) struct Halt {
    reason: OnceLock<String>,
    notify: Notify,
}

impl Halt {
    /// Set the cause (the first cause stays), and wake each waiter.
    pub(super) fn mark(&self, why: &str) {
        let _ = self.reason.set(why.to_string());
        self.notify.notify_waiters();
    }

    pub(super) fn reason(&self) -> Option<String> {
        self.reason.get().cloned()
    }
}

/// A handle that resets each TCP connection and each UDP socket of a stack
/// ([`NetStack::reset_all`]). A clone goes to a task that does not hold the
/// stack, for example the pump of a link that ends.
#[derive(Clone)]
pub struct StackReset {
    pub(super) slot: Arc<Mutex<Option<String>>>,
    pub(super) kick: Arc<Notify>,
}

impl std::fmt::Debug for StackReset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StackReset").finish_non_exhaustive()
    }
}

impl StackReset {
    /// Reset each connection and each UDP socket that is open now, with
    /// `reason` ([`NetStack::reset_all`]). It does not wait.
    pub fn reset(&self, reason: impl Into<String>) {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        // Two resets before the next pass of the reactor end the same
        // sockets: the first reason stays.
        if slot.is_none() {
            *slot = Some(reason.into());
        }
        drop(slot);
        self.kick.notify_one();
    }
}

impl NetStack {
    /// Reset each TCP connection and each UDP socket that is open now, with
    /// `reason`: the link of the stack is dead, or the consumer closes it.
    /// Each RST goes out when the link takes it, and the stack frees each
    /// socket at once. The app reads the bytes that the stack gave it
    /// before, then `ConnectionReset` with `reason`; a write and a UDP
    /// receive and send fail the same way. A listener stays open. A socket
    /// that the app opens after the call is not reset. It does not wait.
    pub fn reset_all(&self, reason: impl Into<String>) {
        self.reset.reset(reason);
    }

    /// A handle for [`NetStack::reset_all`] that a task can keep.
    pub fn resetter(&self) -> StackReset {
        self.reset.clone()
    }

    /// Why the reactor stopped, after it stopped: a shutdown, the drop of
    /// the stack, or a panic. Each socket that was open then fails with this
    /// cause.
    pub fn stop_reason(&self) -> Option<String> {
        self.halt.reason()
    }

    /// Wait for the stop of the reactor, also by a panic; the cause. The
    /// future does not hold the stack.
    pub fn stopped(&self) -> impl Future<Output = String> + Send + 'static {
        let halt = self.halt.clone();
        async move {
            loop {
                let notified = halt.notify.notified();
                tokio::pin!(notified);
                // Register first: a stop between the check and the wait is
                // not lost.
                notified.as_mut().enable();
                if let Some(why) = halt.reason() {
                    return why;
                }
                notified.await;
            }
        }
    }

    /// The error of a command that the reactor did not take: it stopped.
    pub(super) fn shut_down(&self) -> io::Error {
        let why = self.halt.reason().unwrap_or_else(|| "the stack is shut down".into());
        io::Error::new(io::ErrorKind::BrokenPipe, why)
    }
}

impl Core {
    /// Run the reset that the consumer asked for, if any.
    pub(super) fn take_reset(&mut self) {
        let reason = self.reset_slot.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(reason) = reason {
            self.reset_all(&reason);
        }
    }

    /// Abort each connection and free each UDP socket, with `reason`.
    fn reset_all(&mut self, reason: &str) {
        for conn in self.conns.values() {
            self.sockets.get_mut::<tcp::Socket>(conn.handle).abort();
            let _ = conn.cause.set(reason.to_string());
        }
        // Send the RSTs now. A link that takes no packet loses them: the
        // stack does not wait for a dead link.
        self.poll_iface();
        let conns: Vec<(u32, Conn)> = self.conns.drain().collect();
        for (id, conn) in conns {
            self.drop_socket(conn.handle);
            self.emit(StackEvent::Closed { id });
            self.stats.closed.fetch_add(1, Ordering::Relaxed);
        }
        let udp: Vec<UdpConn> = self.udp.drain().map(|(_, conn)| conn).collect();
        for conn in udp {
            let _ = conn.cause.set(reason.to_string());
            self.drop_socket(conn.handle);
        }
        if !self.owned.is_empty() {
            self.release_addresses();
        }
    }
}

impl Drop for Core {
    /// The reactor stopped: a normal stop marked its cause before; with
    /// none, a panic or the end of the runtime dropped it. Each socket that
    /// is open fails with the cause: the channels close after this.
    fn drop(&mut self) {
        self.halt.mark("the reactor of the stack stopped: a panic, or the end of its runtime");
        let why = self.halt.reason().unwrap_or_default();
        for conn in self.conns.values() {
            let _ = conn.cause.set(why.clone());
        }
        for conn in self.udp.values() {
            let _ = conn.cause.set(why.clone());
        }
    }
}

impl Drop for Conn {
    /// Wake a write that waits for room in the channel from the app: it
    /// then sees the closed channel. The close comes first, so that a write
    /// between the wake and the drop does not wait again.
    fn drop(&mut self) {
        self.from_app.close();
        self.write_waker.wake();
    }
}
