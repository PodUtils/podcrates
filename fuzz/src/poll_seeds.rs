//! The seeds of the fuzz target `link_poll_wire`: the bytes of connections
//! of the poll carriers, in the form of the target (a byte of the size of
//! each read, then the bytes). The seeds on disk are the ones that this file
//! makes. Write them again with
//! `cargo test --manifest-path fuzz/Cargo.toml --lib write_the_link_poll_wire_seeds -- --ignored`.

use std::path::PathBuf;

/// A chunk of a chunked body.
fn chunk(data: &[u8]) -> Vec<u8> {
    let mut out = format!("{:x}\r\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\r\n");
    out
}

/// The name and the bytes of each seed.
fn seeds() -> Vec<(&'static str, Vec<u8>)> {
    // The requests of a dialer to a listener: an open, a POST up with its
    // body, a long poll, a simplex poll and a push of SSE.
    let mut requests = vec![13];
    for head in [
        "POST /l/open?s=0123456789abcdef0123456789abcdef HTTP/1.1\r\nHost: h\r\nContent-Length: 0\r\n\r\n",
        "POST /l/up?s=0123456789abcdef0123456789abcdef&seq=0&ack=0 HTTP/1.1\r\nHost: h\r\nContent-Type: application/octet-stream\r\nContent-Length: 5\r\n\r\nhello",
        "POST /l/down?s=0123456789abcdef0123456789abcdef&since=0&hold=25 HTTP/1.1\r\nHost: h\r\nContent-Length: 0\r\n\r\n",
        "GET /l/out?s=0123456789abcdef0123456789abcdef&since=0 HTTP/1.1\r\nHost: h\r\n\r\n",
        "GET /l/push?s=0123456789abcdef0123456789abcdef&since=0 HTTP/1.1\r\nHost: h\r\nAccept: text/event-stream\r\n\r\n",
    ] {
        requests.extend_from_slice(head.as_bytes());
    }
    // The answers of a listener: a 201, a 202 with Pod-Ack, a 200 with bytes
    // and Pod-Fin, a 204, then a push of SSE: a ping, two events, the end.
    let mut answers = vec![64];
    answers.extend_from_slice(b"HTTP/1.1 201 Created\r\nCache-Control: no-store\r\nContent-Length: 0\r\n\r\n");
    answers.extend_from_slice(b"HTTP/1.1 202 Accepted\r\nPod-Ack: 5\r\nContent-Length: 0\r\n\r\n");
    answers.extend_from_slice(b"HTTP/1.1 200 OK\r\nPod-Fin: 1\r\nContent-Length: 3\r\n\r\nabc");
    answers.extend_from_slice(b"HTTP/1.1 204 No Content\r\nCache-Control: no-store\r\n\r\n");
    answers
        .extend_from_slice(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n");
    answers.extend_from_slice(&chunk(b": ping\n\n"));
    answers.extend_from_slice(&chunk(b"id: 5\ndata: aGVsbG8=\n\nid: 10\ndata: d29y\ndata: bGQ=\n\n"));
    answers.extend_from_slice(&chunk(b"event: fin\ndata\n\n"));
    answers.extend_from_slice(b"0\r\n\r\n");
    // A chunked answer of raw bytes, with a chunk extension.
    let mut raw = vec![7];
    raw.extend_from_slice(
        b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
    );
    raw.extend_from_slice(&chunk(&[0xa5; 300]));
    raw.extend_from_slice(b"3;x=1\r\nabc\r\n0\r\n\r\n");
    // Forms that the reader refuses: a head of more than 4 KiB with its
    // end, a folded line, a bad length, a chunk with no end of line, an
    // event that is not base64.
    // Read whole: one read brings the head and its end.
    let mut long = vec![0];
    long.extend_from_slice(format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(5000)).as_bytes());
    let mut forms = vec![1];
    forms.extend_from_slice(b"GET / HTTP/1.1\r\n folded\r\n\r\n");
    let mut length = vec![2];
    length.extend_from_slice(b"POST / HTTP/1.1\r\nContent-Length: -1\r\n\r\n");
    let mut chunk_end = vec![3];
    chunk_end.extend_from_slice(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabcX\r\n");
    let mut bad_event = vec![4];
    bad_event.extend_from_slice(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
    bad_event.extend_from_slice(&chunk(b"data: !!!\n\n"));
    vec![
        ("requests", requests),
        ("answers", answers),
        ("raw-chunks", raw),
        ("long-head", long),
        ("folded", forms),
        ("bad-length", length),
        ("chunk-end", chunk_end),
        ("bad-event", bad_event),
    ]
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("seeds").join("link_poll_wire")
}

/// The seeds on disk are the ones of [`seeds`], and the target takes each.
#[test]
fn the_seeds_of_link_poll_wire_are_the_bytes_of_connections() {
    for (name, bytes) in seeds() {
        let on_disk = std::fs::read(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(on_disk == bytes, "{name} is stale: run write_the_link_poll_wire_seeds");
        crate::targets::link_poll_wire(&bytes);
    }
}

#[test]
#[ignore = "writes the seeds of link_poll_wire into fuzz/seeds/link_poll_wire"]
fn write_the_link_poll_wire_seeds() {
    std::fs::create_dir_all(dir()).unwrap();
    for (name, bytes) in seeds() {
        std::fs::write(dir().join(name), bytes).unwrap();
    }
}
