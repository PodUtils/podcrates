//! The lines of a wait that holds, at both ends (PodUtils/podssh#283): the
//! half that reads the application (the target, at the far end) says where
//! it waits, the half that writes to it says since when bytes wait for it,
//! and the pump looks at both while the link lives. A wait for room, for
//! the writer of the link, or for room to send again that holds for
//! [`STALL_AFTER`] gives one line, with the offsets each way and the room.
//! An application that takes no byte for as long, while bytes wait for it,
//! gives one line too. At the far end, a read of the target that gives no
//! byte for as long, while the target took bytes of the client, gives one
//! line as well. The far end's lines go to the sink of [`Watch::stall`],
//! with the kind [`STALL`]; the client's go to [`Watch::notes`], as
//! [`Note::Stall`].

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use pod_core::Event;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::Instant;

use super::super::budget::Tell;
use super::super::link::Link;
use super::super::resume::Note;
use super::{Watch, lock};

/// The kind of the event of a wait that holds.
pub const STALL: &str = "session.stall";
/// How long a wait holds before its line.
pub const STALL_AFTER: Duration = Duration::from_secs(20);
/// How often the pump looks at the waits.
const LOOK: Duration = Duration::from_secs(5);

/// Where the lines of a far end's waits go, with the session's name.
#[derive(Debug, Clone)]
pub struct Stall {
    pub tell: Tell,
    /// The short id of the session.
    pub session: String,
}

/// What the half that reads the application waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UpWait {
    /// Room for the records of a resume, within the window.
    Again,
    /// Room in the replay buffer, within the window.
    Room,
    /// Bytes of the application.
    Target,
    /// The writer of the link, which takes one batch at a time.
    Writer,
    /// Nothing: the application ended its bytes.
    Done,
}

/// The waits of the two halves that serve the application, since when, and
/// the bytes that the application took.
pub(super) struct Waits {
    up: Mutex<(UpWait, Instant)>,
    /// Since when the application took no byte while bytes wait for it;
    /// `None` while none wait.
    down: Mutex<Option<Instant>>,
    /// The bytes written to the application on this link.
    took: AtomicU64,
    /// [`Waits::took`] when the read of the application started.
    took_at_read: AtomicU64,
}

impl Waits {
    pub(super) fn new() -> Waits {
        Waits {
            up: Mutex::new((UpWait::Target, Instant::now())),
            down: Mutex::new(None),
            took: AtomicU64::new(0),
            took_at_read: AtomicU64::new(0),
        }
    }

    /// The half that reads the application waits for `wait` from now.
    pub(super) fn up(&self, wait: UpWait) {
        if wait == UpWait::Target {
            self.took_at_read.store(self.took.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        *lock(&self.up) = (wait, Instant::now());
    }

    /// Bytes wait for the application: a wait starts now, unless one goes
    /// on.
    pub(super) fn holding(&self) {
        lock(&self.down).get_or_insert_with(Instant::now);
    }

    /// `n` more bytes went to the application, and `left` still wait for it:
    /// a byte taken ends the wait, and the rest start a new one.
    pub(super) fn took(&self, n: usize, left: usize) {
        self.took.fetch_add(n as u64, Ordering::Relaxed);
        *lock(&self.down) = (left > 0).then(Instant::now);
    }
}

/// Whose waits a line says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Side {
    /// The far end, whose application is the target.
    Far,
    /// The client.
    Client,
}

/// A wait that a line says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Wait {
    /// The half that reads the application waits; the application took
    /// this many bytes since the read started.
    Up(UpWait, u64),
    /// The application took no byte while bytes wait for it.
    Held,
}

/// The line of `wait` of `side`, held for `held`, where it gives one: each
/// wait but a read of the application, and the end. A far end also says a
/// read of a target that took bytes in the meantime; a quiet application of
/// the client is a user who types nothing.
pub(super) fn line(side: Side, wait: Wait, held: Duration, link: &Link, window: Option<usize>) -> Option<String> {
    let s = held.as_secs();
    let (app, peer) = match side {
        Side::Far => ("the target", "the client"),
        Side::Client => ("the application", "the far end"),
    };
    let what = match wait {
        Wait::Up(UpWait::Again, _) => format!("the records sent again wait for room for {s} s"),
        Wait::Up(UpWait::Room, _) => format!("the reader of {app} waits for room for {s} s"),
        Wait::Up(UpWait::Writer, _) => format!("the reader of {app} waits {s} s for the writer of the link"),
        Wait::Up(UpWait::Target, took) if took > 0 && side == Side::Far => {
            format!("the target sent no byte for {s} s, while it took {took} bytes of the client")
        }
        Wait::Up(UpWait::Target | UpWait::Done, _) => return None,
        Wait::Held => format!("{app} took no byte for {s} s, while {} bytes wait for it", link.held()),
    };
    let window = window.map_or("no window".to_string(), |w| format!("a window of {w} bytes"));
    Some(format!(
        "{what}; this side sent {} bytes, {peer} acknowledged {}; the replay buffer keeps {} bytes, room {}; \
         {window}; this side received {} bytes, {} of them not yet written to {app}",
        link.sent(),
        link.acknowledged(),
        link.kept(),
        link.room(),
        link.received(),
        link.held()
    ))
}

/// Where the lines of the waits go.
enum Sink<'a> {
    /// The far end's sink of events.
    Far(&'a Stall),
    /// The client's notes.
    Client(&'a UnboundedSender<Note>),
}

impl Sink<'_> {
    /// Say `wait`, which holds since `since`, once: `told` is the start of
    /// the last wait said.
    fn once(&self, wait: Wait, since: Instant, told: &mut Option<Instant>, state: &Mutex<Link>, window: Option<usize>) {
        let held = since.elapsed();
        if held < STALL_AFTER || *told == Some(since) {
            return;
        }
        let side = if matches!(self, Sink::Far(_)) { Side::Far } else { Side::Client };
        let Some(words) = line(side, wait, held, &lock(state), window) else { return };
        *told = Some(since);
        match self {
            Sink::Far(stall) => {
                stall.tell.events.event(Event::info(STALL, format!("session {}: {words}", stall.session)));
            }
            Sink::Client(notes) => {
                let _ = notes.send(Note::Stall { words });
            }
        }
    }
}

/// Look at `waits` while the link lives, and say each wait that holds,
/// once, where `watch` says. It never ends: the pump's other parts end the
/// link.
pub(super) async fn look(waits: &Waits, state: &Mutex<Link>, watch: &Watch) -> std::convert::Infallible {
    let sink = match (&watch.stall, &watch.notes) {
        (Some(stall), _) => Sink::Far(stall),
        (None, Some(notes)) => Sink::Client(notes),
        (None, None) => return std::future::pending().await,
    };
    let (mut told_up, mut told_down) = (None, None);
    loop {
        tokio::time::sleep(LOOK).await;
        let (wait, since) = *lock(&waits.up);
        let took = waits.took.load(Ordering::Relaxed).saturating_sub(waits.took_at_read.load(Ordering::Relaxed));
        sink.once(Wait::Up(wait, took), since, &mut told_up, state, watch.window);
        let down = *lock(&waits.down);
        if let Some(since) = down {
            sink.once(Wait::Held, since, &mut told_down, state, watch.window);
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncWriteExt;
    use tokio::sync::mpsc;

    use super::super::super::decode::Decoder;
    use super::super::super::record::Record;
    use super::super::{Carry, run};
    use super::*;

    #[test]
    fn a_wait_for_room_gives_the_offsets_and_a_quiet_application_gives_none() {
        let link = Link::new(0, 0).with_replay(1 << 20);
        let up = |wait, took| Wait::Up(wait, took);
        let secs = Duration::from_secs;
        let room = line(Side::Far, up(UpWait::Room, 0), secs(25), &link, Some(786_432)).unwrap();
        assert!(room.starts_with("the reader of the target waits for room for 25 s; this side sent 0 bytes"), "{room}");
        assert!(room.contains("; a window of 786432 bytes; this side received 0 bytes, 0 of them"), "{room}");
        assert_eq!(line(Side::Far, up(UpWait::Target, 0), secs(25), &link, None), None);
        let quiet = line(Side::Far, up(UpWait::Target, 4096), secs(25), &link, None).unwrap();
        assert!(quiet.starts_with("the target sent no byte for 25 s, while it took 4096 bytes"), "{quiet}");
        assert!(line(Side::Far, up(UpWait::Writer, 0), secs(21), &link, None).unwrap().contains("for the writer"));
        assert_eq!(line(Side::Far, up(UpWait::Done, 7), secs(99), &link, None), None);
        assert_eq!(line(Side::Client, up(UpWait::Target, 4096), secs(25), &link, None), None);
        let client = line(Side::Client, up(UpWait::Room, 0), secs(25), &link, None).unwrap();
        assert!(client.starts_with("the reader of the application waits for room for 25 s"), "{client}");
        assert!(client.contains("the far end acknowledged 0"), "{client}");
    }

    #[derive(Default)]
    struct Lines(Mutex<Vec<String>>);

    impl pod_core::Events for Lines {
        fn event(&self, event: Event) {
            assert_eq!(event.kind, STALL);
            lock(&self.0).push(event.text);
        }
    }

    /// A wait for room that holds gives one line after 20 s, not two; a new
    /// wait gives its own; a read of a target that took nothing gives none.
    #[tokio::test(start_paused = true)]
    async fn a_wait_that_holds_gives_one_line_each() {
        let lines = std::sync::Arc::new(Lines::default());
        let stall = Stall { tell: Tell::new(lines.clone(), ""), session: "0187db18".into() };
        let (waits, state) = (Waits::new(), Mutex::new(Link::new(0, 0).with_replay(1 << 20)));
        let watch = Watch::default().within(Some(786_432)).stalling(stall);
        waits.up(UpWait::Room);
        let looking = look(&waits, &state, &watch);
        tokio::pin!(looking);
        macro_rules! run {
            ($secs:expr) => {
                tokio::time::timeout(Duration::from_secs($secs), looking.as_mut())
            };
        }
        assert!(run!(16).await.is_err());
        assert!(lock(&lines.0).is_empty());
        assert!(run!(30).await.is_err());
        let got = lock(&lines.0).clone();
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(got[0].starts_with("session 0187db18: the reader of the target waits for room for 20 s"), "{got:?}");
        waits.up(UpWait::Target);
        assert!(run!(30).await.is_err());
        waits.up(UpWait::Writer);
        assert!(run!(30).await.is_err());
        let got = lock(&lines.0).clone();
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[1].contains(" s for the writer of the link"), "{got:?}");
    }

    /// The client's application that stops reading gives one note after
    /// 20 s, with the bytes held for it, and no more while the wait holds;
    /// its quiet reader gives none.
    #[tokio::test(start_paused = true)]
    async fn an_application_that_stops_reading_gives_one_note_with_the_bytes_held() {
        // The application takes 1 KiB, then reads no more.
        let (mut app, _user) = tokio::io::duplex(1024);
        let (link, mut far) = tokio::io::duplex(1 << 20);
        far.write_all(&Record::Data { offset: 0, bytes: vec![7; 32_768] }.to_bytes().unwrap()).await.unwrap();
        let (said, mut notes) = mpsc::unbounded_channel();
        let watch = Watch::default().noting(said);
        let mut carry = Carry::new(Link::new(0, 0).with_replay(1 << 20));
        let pump = run(&mut app, link, Decoder::new(), &mut carry, &watch);
        tokio::pin!(pump);
        assert!(tokio::time::timeout(Duration::from_secs(19), pump.as_mut()).await.is_err());
        assert!(notes.try_recv().is_err());
        assert!(tokio::time::timeout(Duration::from_secs(90), pump.as_mut()).await.is_err());
        let mut got = Vec::new();
        while let Ok(note) = notes.try_recv() {
            got.push(note);
        }
        let words = "the application took no byte for 20 s, while 31744 bytes wait for it; this side sent 0 bytes, \
                     the far end acknowledged 0; the replay buffer keeps 0 bytes, room 1048576; no window; this side \
                     received 32768 bytes, 31744 of them not yet written to the application";
        assert_eq!(got, vec![Note::Stall { words: words.into() }]);
    }
}
