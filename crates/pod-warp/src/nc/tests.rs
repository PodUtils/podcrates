//! The TCP and the UDP pipe of `warp nc` (from podnet's `src/warp/nc.rs`).

use super::*;

#[test]
fn targets() {
    assert_eq!(target("github.com", 22), "github.com:22");
    assert_eq!(target("10.0.0.1", 22), "10.0.0.1:22");
    assert_eq!(target("2001:db8::1", 22), "[2001:db8::1]:22");
}

/// PodUtils/podnet issue 115: nc -u: each read is a datagram; the answers
/// come back; the pipe ends after the idle time when the input ended.
#[tokio::test]
async fn a_udp_pipe_sends_each_read_and_prints_each_answer() {
    let (out_tx, mut out_rx) = mpsc::channel(8);
    let (back_tx, back_rx) = mpsc::channel(8);
    let echo = tokio::spawn(async move {
        while let Some(d) = out_rx.recv().await {
            let mut answer = b"ECHO:".to_vec();
            answer.extend(d);
            if back_tx.send(answer).await.is_err() {
                return;
            }
        }
    });
    let mut output = Vec::new();
    let start = std::time::Instant::now();
    udp_pipe_with(out_tx, back_rx, &b"PING"[..], &mut output, Duration::from_millis(200)).await.unwrap();
    assert_eq!(output, b"ECHO:PING");
    assert!(start.elapsed() < Duration::from_secs(2));
    echo.abort();
}

/// The pipe carries both directions and ends when the far end closes.
#[tokio::test]
async fn the_pipe_ends_when_the_far_end_closes() {
    let (ours, mut theirs) = tokio::io::duplex(1024);
    // The input stays open (as the stdin of ssh does).
    let (_input_keep, input) = tokio::io::duplex(64);
    let (output, mut shown) = tokio::io::duplex(1024);
    let far = tokio::spawn(async move {
        theirs.write_all(b"SSH-2.0-test\r\n").await.unwrap();
        drop(theirs);
    });
    let piped = pipe_with(Box::new(ours), input, output, PipeEnd::default());
    tokio::time::timeout(Duration::from_secs(5), piped).await.expect("the pipe ends").unwrap();
    far.await.unwrap();
    let mut got = Vec::new();
    shown.read_to_end(&mut got).await.unwrap();
    assert_eq!(got, b"SSH-2.0-test\r\n");
}

/// A far end that reads the request, waits, and answers only if it saw no
/// FIN (as a server that drops a half-closed connection).
async fn picky_server(mut theirs: tokio::io::DuplexStream) -> bool {
    let mut buf = [0u8; 64];
    let n = theirs.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"GET");
    let wait = tokio::time::timeout(Duration::from_millis(300), theirs.read(&mut buf)).await;
    let fin = matches!(wait, Ok(Ok(0)));
    if !fin {
        theirs.write_all(b"REPLY").await.unwrap();
    }
    drop(theirs);
    fin
}

/// PodUtils/podnet issue 188: an input that ends at once does not lose the
/// reply.
#[tokio::test]
async fn the_reply_survives_an_input_that_ends_at_once() {
    let (ours, theirs) = tokio::io::duplex(1024);
    let (output, mut shown) = tokio::io::duplex(1024);
    let far = tokio::spawn(picky_server(theirs));
    let piped = pipe_with(Box::new(ours), &b"GET"[..], output, PipeEnd::default());
    tokio::time::timeout(Duration::from_secs(5), piped).await.expect("the pipe ends").unwrap();
    assert!(!far.await.unwrap(), "no FIN before the reply");
    let mut got = Vec::new();
    shown.read_to_end(&mut got).await.unwrap();
    assert_eq!(got, b"REPLY");
}

/// PodUtils/podnet issue 188: -N sends the FIN at the end of the input.
#[tokio::test]
async fn with_n_the_far_end_sees_the_eof() {
    let (ours, theirs) = tokio::io::duplex(1024);
    let (output, _shown) = tokio::io::duplex(1024);
    let far = tokio::spawn(picky_server(theirs));
    let end = PipeEnd { shutdown: true, idle: TCP_IDLE };
    let piped = pipe_with(Box::new(ours), &b"GET"[..], output, end);
    tokio::time::timeout(Duration::from_secs(5), piped).await.expect("the pipe ends").unwrap();
    assert!(far.await.unwrap(), "the far end saw the FIN");
}

/// PodUtils/podnet issue 188: after the end of the input, no data for the
/// idle time ends the pipe, and the far end then gets the FIN.
#[tokio::test]
async fn the_pipe_ends_after_idle_with_no_reply() {
    let (ours, mut theirs) = tokio::io::duplex(1024);
    let (output, _shown) = tokio::io::duplex(1024);
    let end = PipeEnd { shutdown: false, idle: Duration::from_millis(200) };
    let started = std::time::Instant::now();
    let piped = pipe_with(Box::new(ours), &b"x"[..], output, end);
    tokio::time::timeout(Duration::from_secs(5), piped).await.expect("the pipe ends").unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    let mut got = Vec::new();
    theirs.read_to_end(&mut got).await.unwrap();
    assert_eq!(got, b"x");
}

/// A UDP pipe through a SOCKS5 association in the stream: each payload
/// goes out in a frame to its target, and the payload of each frame that
/// comes back goes to the pipe; a name of more than 255 bytes is refused.
#[tokio::test]
async fn a_udp_pipe_through_a_stream_frames_each_datagram() {
    let (ours, mut proxy) = tokio::io::duplex(4096);
    let (out, mut back) = udp_via_stream(ours, "dns.example", 53).unwrap();
    out.send(b"query".to_vec()).await.unwrap();
    let frame = read_frame(&mut proxy).await.unwrap().unwrap();
    let (to, payload) = unwrap(&frame).unwrap();
    assert_eq!((to.to_string().as_str(), payload), ("dns.example:53", &b"query"[..]));
    let answer = wrap(b"answer", "198.51.100.7".parse().unwrap(), 53);
    write_frame(&mut proxy, &answer).await.unwrap();
    assert_eq!(back.recv().await.unwrap(), b"answer");
    let failure = udp_via_stream(tokio::io::duplex(64).0, &"a".repeat(256), 53).unwrap_err();
    assert_eq!(failure.class, Class::Usage);
}
