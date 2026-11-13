//! Streams over a link, on the memory carrier and on the unix carrier:
//! many at once, the ids of each end, a half close, a reset, a drop.

mod common;

use std::time::Duration;

use common::{echo, limit, pattern};
use pod_link::{Error, Link, ResetCode, Settings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

/// Room for 300 streams at once.
fn many() -> Settings {
    Settings { max_streams: 512, ..Settings::default() }
}

/// 300 streams at once each send their own bytes and read them back.
async fn three_hundred_echoes(dialer: Link, listener: Link) {
    let _server = echo(listener);
    let dialer = std::sync::Arc::new(dialer);
    let mut tasks = Vec::new();
    for i in 0..300u32 {
        let stream = dialer.open().unwrap();
        tasks.push(tokio::spawn(async move {
            let data = pattern(i, 1000 + (i as usize * 997) % 70_000);
            let (mut r, mut w) = tokio::io::split(stream);
            let send = data.clone();
            let writer = tokio::spawn(async move {
                w.write_all(&send).await.unwrap();
                w.shutdown().await.unwrap();
                w
            });
            let mut back = Vec::new();
            r.read_to_end(&mut back).await.unwrap();
            let _w = writer.await.unwrap();
            assert!(back == data, "stream {i}: {} bytes back of {}", back.len(), data.len());
        }));
    }
    for t in tasks {
        timeout(limit(), t).await.expect("each echo in time").unwrap();
    }
    assert!(dialer.ended().is_none());
}

on_both!(three_hundred_echoes_over_memory, three_hundred_echoes_over_unix, many(), three_hundred_echoes);

/// The dialer opens odd ids and the listener even ids, each growing.
async fn ids_by_role(dialer: Link, listener: Link) {
    let a: Vec<u32> = (0..3).map(|_| dialer.open().unwrap()).map(|s| s.id()).collect();
    let b: Vec<u32> = (0..3).map(|_| listener.open().unwrap()).map(|s| s.id()).collect();
    assert_eq!(a, [1, 3, 5]);
    assert_eq!(b, [2, 4, 6]);
}

on_both!(ids_by_role_over_memory, ids_by_role_over_unix, Settings::default(), ids_by_role);

/// After a shutdown, the peer reads the end of the stream and can still
/// write; this end reads its answer.
async fn half_close(dialer: Link, listener: Link) {
    let mut a = dialer.open().unwrap();
    a.write_all(b"question").await.unwrap();
    a.shutdown().await.unwrap();
    let mut b = timeout(limit(), listener.accept()).await.unwrap().unwrap();
    let mut got = Vec::new();
    b.read_to_end(&mut got).await.unwrap();
    assert_eq!(got, b"question");
    b.write_all(b"answer after the end of the question").await.unwrap();
    b.shutdown().await.unwrap();
    let mut back = Vec::new();
    timeout(limit(), a.read_to_end(&mut back)).await.unwrap().unwrap();
    assert_eq!(back, b"answer after the end of the question");
    let e = a.write_all(b"more").await.unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe);
}

on_both!(half_close_over_memory, half_close_over_unix, Settings::default(), half_close);

/// A reset ends the stream at both ends with its code; the other streams
/// of the link go on.
async fn reset(dialer: Link, listener: Link) {
    let listener = std::sync::Arc::new(listener);
    let mut a = dialer.open().unwrap();
    a.write_all(b"x").await.unwrap();
    let mut b = timeout(limit(), listener.accept()).await.unwrap().unwrap();
    let mut one = [0; 1];
    b.read_exact(&mut one).await.unwrap();
    b.reset(ResetCode(77));
    let e = timeout(limit(), a.read(&mut one)).await.unwrap().unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::ConnectionReset);
    let inner = e.into_inner().unwrap().downcast::<Error>().unwrap();
    assert_eq!(*inner, Error::Reset { id: 1, code: ResetCode(77), by_peer: true });
    // Writes fail too, once the reset came.
    assert!(a.write_all(b"y").await.is_err());
    assert!(b.read(&mut one).await.is_err(), "the end that reset fails too");
    // Another stream still works.
    let _server = echo_from(listener);
    let mut c = dialer.open().unwrap();
    c.write_all(b"still here").await.unwrap();
    c.shutdown().await.unwrap();
    let mut back = Vec::new();
    timeout(limit(), c.read_to_end(&mut back)).await.unwrap().unwrap();
    assert_eq!(back, b"still here");
}

fn echo_from(link: std::sync::Arc<Link>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Ok(mut s) = link.accept().await {
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let _ = s.read_to_end(&mut buf).await;
                let _ = s.write_all(&buf).await;
                let _ = s.shutdown().await;
            });
        }
    })
}

on_both!(reset_over_memory, reset_over_unix, Settings::default(), reset);

/// A dropped stream closes: the peer reads what was written, then the end
/// of the stream, and the slot is free at both ends.
async fn drop_closes(dialer: Link, listener: Link) {
    let mut a = dialer.open().unwrap();
    a.write_all(b"last words").await.unwrap();
    drop(a);
    let mut b = timeout(limit(), listener.accept()).await.unwrap().unwrap();
    let mut got = Vec::new();
    timeout(limit(), b.read_to_end(&mut got)).await.unwrap().unwrap();
    assert_eq!(got, b"last words");
    assert_eq!(dialer.streams(), 0);
    drop(b);
    timeout(limit(), async {
        while listener.streams() > 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

on_both!(drop_closes_over_memory, drop_closes_over_unix, Settings::default(), drop_closes);

/// A close sends GOAWAY after the queued frames; the peer reads the bytes
/// before it, and both ends end as closed.
async fn close(dialer: Link, listener: Link) {
    let mut a = dialer.open().unwrap();
    // 100,000 bytes do not fit the first 16 KiB of credit: the write runs
    // beside the read.
    let writer = tokio::spawn(async move {
        a.write_all(&pattern(1, 100_000)).await.unwrap();
        a
    });
    let mut b = timeout(limit(), listener.accept()).await.unwrap().unwrap();
    let reader = tokio::spawn(async move {
        let mut got = vec![0; 100_000];
        b.read_exact(&mut got).await.map(|_| got)
    });
    let a = timeout(limit(), writer).await.unwrap().unwrap();
    timeout(limit(), dialer.close()).await.unwrap();
    assert_eq!(dialer.ended(), Some(Error::Closed { by_peer: false }));
    assert_eq!(timeout(limit(), listener.closed()).await.unwrap(), Error::Closed { by_peer: true });
    assert_eq!(timeout(limit(), reader).await.unwrap().unwrap().unwrap(), pattern(1, 100_000));
    assert!(matches!(dialer.open(), Err(Error::Closed { .. })));
    assert!(matches!(listener.accept().await, Err(Error::Closed { by_peer: true })));
    drop(a);
}

on_both!(close_over_memory, close_over_unix, Settings::default(), close);
