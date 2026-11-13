//! The reader of TXT and CNAME answers (`pod_dns::txt`, PodUtils/podnet#479)
//! against a stand-in authoritative server on the loopback: over UDP the
//! server cuts an answer at the size of the query (512 bytes with no
//! EDNS0, the size of the OPT record with it) and sets the TC bit; over TCP
//! (RFC 7766, a length of two bytes before each message) the same answer
//! is whole.

use std::net::SocketAddr;
use std::time::Duration;

use pod_dns::txt::{self, CNAME, EDNS_SIZE, PLAIN_SIZE, TXT};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

const LIMIT: Duration = Duration::from_secs(10);
const NAME: &str = "mfrgg.0.t.example";
/// Eight TXT records of 120 bytes: more than 512 bytes, less than 1232.
const RECORDS: usize = 8;
const CHUNK: usize = 120;

/// The answer of the stand-in to `query`, cut at the size of the query
/// when `cut` (512 bytes, or the size of its OPT record): whole records only, with the TC bit and the counts of what it
/// sends. A TXT question gets [`RECORDS`] records of [`CHUNK`] bytes; a
/// CNAME question gets one CNAME. The server's OPT record (size 1232)
/// answers an OPT record of the query.
fn answer(query: &[u8], cut: bool) -> Vec<u8> {
    let end = 12 + query[12..].iter().position(|b| *b == 0).unwrap() + 1 + 4;
    let kind = u16::from_be_bytes([query[end - 4], query[end - 3]]);
    let edns = (query[11] == 1).then(|| usize::from(u16::from_be_bytes([query[end + 3], query[end + 4]])));
    let limit = cut.then(|| edns.unwrap_or(usize::from(PLAIN_SIZE)));
    let mut records = Vec::new();
    if kind == TXT {
        for i in 0..RECORDS {
            let mut data = vec![CHUNK as u8];
            data.extend(std::iter::repeat_n(b'a' + i as u8, CHUNK));
            records.push(record(TXT, &data));
        }
    } else {
        records.push(record(CNAME, &[4, b'n', b'e', b'x', b't', 0xc0, 12]));
    }
    let opt: Vec<u8> = if edns.is_some() { vec![0, 0, 41, 0x04, 0xd0, 0, 0, 0, 0, 0, 0] } else { Vec::new() };
    let mut out = query[..end].to_vec();
    out[2] = 0x81;
    out[3] = 0x80;
    let mut sent = 0;
    for r in &records {
        if limit.is_some_and(|l| out.len() + r.len() + opt.len() > l) {
            out[2] |= 0x02;
            break;
        }
        out.extend_from_slice(r);
        sent += 1;
    }
    out[6..8].copy_from_slice(&(sent as u16).to_be_bytes());
    out[10..12].copy_from_slice(&u16::from(edns.is_some()).to_be_bytes());
    out.extend_from_slice(&opt);
    out
}

/// A record of class IN whose owner is a pointer to the question's name.
fn record(rtype: u16, data: &[u8]) -> Vec<u8> {
    let mut out = vec![0xc0, 12];
    out.extend_from_slice(&rtype.to_be_bytes());
    out.extend_from_slice(&[0, 1, 0, 0, 0, 0]);
    out.extend_from_slice(&(data.len() as u16).to_be_bytes());
    out.extend_from_slice(data);
    out
}

/// The stand-in on UDP: one answer, cut at the size, for each query.
async fn udp_server() -> SocketAddr {
    let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let at = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 1500];
        while let Ok((n, from)) = socket.recv_from(&mut buf).await {
            let _ = socket.send_to(&answer(&buf[..n], true), from).await;
        }
    });
    at
}

/// One query over UDP and its answer, read as the answer to it.
async fn over_udp(server: SocketAddr, id: u16, kind: u16, edns: Option<u16>) -> (txt::Answer, usize) {
    let client = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client.send_to(&txt::query(id, NAME, kind, edns).unwrap(), server).await.unwrap();
    let mut buf = [0u8; 4096];
    let (n, _) = tokio::time::timeout(LIMIT, client.recv_from(&mut buf)).await.expect("an answer").unwrap();
    (txt::read(&buf[..n], id, NAME, kind).unwrap(), n)
}

/// With no EDNS0 the answer is cut at 512 bytes with the TC bit; with an
/// EDNS0 size of 1232 the same answer is whole, with the server's OPT
/// record. A CNAME answer gives its target.
#[tokio::test]
async fn the_size_of_the_query_decides_the_cut_of_the_answer() {
    pod_testkit::need_loopback!();
    let server = udp_server().await;
    let (plain, n) = over_udp(server, 0x0101, TXT, None).await;
    assert!(plain.truncated, "a cut answer has the TC bit");
    assert!(n <= usize::from(PLAIN_SIZE), "{n}");
    assert_eq!(plain.txt().len(), 3, "the whole records before the cut");
    assert_eq!(plain.edns, None);
    let (edns, n) = over_udp(server, 0x0202, TXT, Some(EDNS_SIZE)).await;
    assert!(!edns.truncated);
    assert!(n > usize::from(PLAIN_SIZE) && n <= usize::from(EDNS_SIZE), "{n}");
    assert_eq!(edns.edns, Some(EDNS_SIZE));
    let txt = edns.txt();
    assert_eq!(txt.len(), RECORDS);
    assert!(txt.iter().enumerate().all(|(i, t)| *t == vec![b'a' + i as u8; CHUNK]));
    let (cname, _) = over_udp(server, 0x0303, CNAME, Some(EDNS_SIZE)).await;
    assert_eq!(cname.cnames(), vec![format!("next.{NAME}")]);
}

/// The retry of a cut answer over TCP: the length of two bytes, then the
/// whole answer with no TC bit.
#[tokio::test]
async fn a_cut_answer_is_whole_over_tcp() {
    pod_testkit::need_loopback!();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let len = usize::from(stream.read_u16().await.unwrap());
        let mut query = vec![0u8; len];
        stream.read_exact(&mut query).await.unwrap();
        let whole = answer(&query, false);
        stream.write_u16(whole.len() as u16).await.unwrap();
        stream.write_all(&whole).await.unwrap();
    });
    let mut stream = tokio::net::TcpStream::connect(at).await.unwrap();
    let query = txt::query(0x0404, NAME, TXT, None).unwrap();
    stream.write_u16(query.len() as u16).await.unwrap();
    stream.write_all(&query).await.unwrap();
    let len = usize::from(tokio::time::timeout(LIMIT, stream.read_u16()).await.expect("an answer").unwrap());
    let mut msg = vec![0u8; len];
    stream.read_exact(&mut msg).await.unwrap();
    assert!(len > usize::from(PLAIN_SIZE), "{len}");
    let found = txt::read(&msg, 0x0404, NAME, TXT).unwrap();
    assert!(!found.truncated);
    assert_eq!(found.txt().len(), RECORDS);
}
