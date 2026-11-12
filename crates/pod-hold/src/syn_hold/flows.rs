//! The listeners of the hold, and its flows: each stream that the stack
//! accepted, with its upstream.

use std::collections::HashSet;

use super::*;

/// A connection of the payload that the stack accepted, with its upstream.
/// The consumer copies between `stream` and `upstream`. The drop of `open`
/// ends the flow in the hold: its connection leaves the budget, and the
/// listener of its port can close.
pub struct Flow<U> {
    pub syn: Syn,
    pub stream: TcpStream,
    pub upstream: U,
    pub open: Open,
}

impl<U> std::fmt::Debug for Flow<U> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Flow").field("syn", &self.syn).field("stream", &self.stream).finish_non_exhaustive()
    }
}

/// The entry of a flow in the hold, until its drop.
pub struct Open {
    syn: Syn,
    open: Opened,
    wake: Arc<Notify>,
}

impl std::fmt::Debug for Open {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Open").field("syn", &self.syn).finish()
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        self.open.remove(&self.syn);
        // The hold closes the listener of an idle port at its next pump.
        self.wake.notify_one();
    }
}

/// The accepted connections of the hold, by their addresses, with the
/// number of the flows of each: a connection that the stack accepts again
/// while its first flow lives stays until both ended.
#[derive(Clone, Default)]
pub(super) struct Opened(Arc<Mutex<HashMap<Syn, usize>>>);

impl Opened {
    pub(super) fn contains(&self, syn: &Syn) -> bool {
        lock(&self.0).contains_key(syn)
    }

    pub(super) fn len(&self) -> usize {
        lock(&self.0).len()
    }

    fn ports(&self) -> Vec<u16> {
        lock(&self.0).keys().map(|syn| syn.target.port()).collect()
    }

    fn insert(&self, syn: Syn) {
        *lock(&self.0).entry(syn).or_insert(0) += 1;
    }

    fn remove(&self, syn: &Syn) {
        let mut open = lock(&self.0);
        if let Some(count) = open.get_mut(syn) {
            *count -= 1;
            if *count == 0 {
                open.remove(syn);
            }
        }
    }
}

/// An accepted stream whose dial started after the accept.
pub(super) struct Late<U, E> {
    syn: Syn,
    stream: TcpStream,
    open: Open,
    pub(super) task: JoinHandle<Result<U, E>>,
}

impl<U, E> SynHold<U, E>
where
    U: Send + 'static,
    E: From<JoinError> + Send + 'static,
{
    /// Open a listener on any address of `port`, when there is none. False
    /// when the stack refuses.
    pub(super) async fn listen_on(&mut self, port: u16) -> bool {
        if self.ports.contains_key(&port) {
            return true;
        }
        let Ok(mut listener) = self.stack.listen_any(port).await else {
            return false;
        };
        let tx = self.accepted_tx.clone();
        let wake = self.wake.clone();
        let task = tokio::spawn(async move {
            while let Ok(stream) = listener.accept().await {
                if tx.send(stream).is_err() {
                    break;
                }
                wake.notify_one();
            }
        });
        self.ports.insert(port, task);
        true
    }

    /// Close the listener of each port that has no SYN in a handshake and no
    /// flow. The close frees the port in the stack, so the table does not
    /// grow with the ports that the payload used.
    pub(super) fn close_idle_ports(&mut self) {
        if self.ports.is_empty() {
            return;
        }
        let mut busy: HashSet<u16> = self.handshakes.keys().map(|syn| syn.target.port()).collect();
        busy.extend(self.open.ports());
        self.ports.retain(|port, task| {
            let keep = busy.contains(port);
            if !keep {
                task.abort();
            }
            keep
        });
    }

    /// Take the packets that the stack sends to the payload.
    pub(super) fn collect(&mut self, to_payload: &mut VecDeque<Vec<u8>>) {
        while let Some(packet) = self.link_out.pop() {
            to_payload.push_back(packet);
        }
    }

    /// Make a flow of each stream that the stack accepted. Its upstream was
    /// dialed before the handshake (the usual case), else a dial starts now
    /// and the stream waits for it in the hold.
    pub(super) fn accept_flows<D: Dial<Upstream = U, Error = E>>(&mut self, dial: &D) {
        while let Ok(stream) = self.accepted_rx.try_recv() {
            let Some(syn) = Syn::of_stream(&stream) else {
                stream.abort();
                continue;
            };
            self.handshakes.remove(&syn);
            self.open.insert(syn);
            let open = Open { syn, open: self.open.clone(), wake: self.wake.clone() };
            match self.dialed.remove(&syn) {
                Some((upstream, _)) => {
                    self.stats.flows += 1;
                    self.ready.push_back(Flow { syn, stream, upstream, open });
                }
                None => {
                    self.stats.late += 1;
                    let task = self.spawn_dial(dial, syn);
                    self.late.push(Late { syn, stream, open, task });
                }
            }
        }
    }

    /// Make a flow of each accepted stream whose late dial succeeded, and
    /// abort each whose late dial failed.
    pub(super) async fn end_late_dials<D: Dial<Upstream = U, Error = E>>(&mut self, dial: &D) {
        if self.late.is_empty() {
            return;
        }
        let mut waiting = Vec::with_capacity(self.late.len());
        for late in std::mem::take(&mut self.late) {
            if !late.task.is_finished() {
                waiting.push(late);
                continue;
            }
            let Late { syn, stream, open, task } = late;
            match task.await.unwrap_or_else(|e| Err(E::from(e))) {
                Ok(upstream) => {
                    self.stats.flows += 1;
                    self.ready.push_back(Flow { syn, stream, upstream, open });
                }
                Err(error) => {
                    stream.abort();
                    self.stats.aborted += 1;
                    drop(open);
                    dial.refused(syn, Refused::Aborted, error);
                }
            }
        }
        self.late = waiting;
    }
}
