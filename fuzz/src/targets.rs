//! The body of each fuzz target. Each function runs one parser of a crate
//! on untrusted bytes and checks what the parser promises. The doc of each
//! function names its crate and its source.

use pod_proxy::env::{no_proxy_entry_at, no_proxy_matches};
use pod_proxy::http::{decode_chunked, parse_head, parse_status_line};
use pod_proxy::{Carrier, NoProxy, Proxy, authority, digest, proxy_auth};

use crate::{is_one_line, text};

mod actions;
mod e2e;
mod job;
mod link;
mod podvm;
mod relay;
mod wg;
mod wormhole;

pub use actions::{actions_expr, actions_workflow};
pub use e2e::{e2e_frame, e2e_handover, e2e_proof};
pub use job::job_offer;
pub use link::link_frame;
pub use podvm::{elf_header, extract_layer, oci_challenge, oci_manifest, squashfs_image};
pub use relay::{relay_list, relay_pool};
pub use wg::{wg_cookie, wg_response, wg_transport};
pub use wormhole::mqtt_packet;

/// Text from a peer made safe for one line (`pod_core::text::one_line`;
/// podssh `one_line`): no control or bidirectional formatting character
/// is left, whitespace is single spaces between words, and a second pass
/// changes nothing.
pub fn one_line(data: &[u8]) {
    let Some(input) = text(data) else { return };
    let safe = pod_core::text::one_line(input);
    assert!(is_one_line(&safe), "{safe:?}");
    assert_eq!(pod_core::text::one_line(&safe), safe);
}

/// A bundle of PEM certificates (`pod_tls::bundle::pem_certificates`;
/// podssh `pem_bundle`), as a CA file or `SSL_CERT_FILE` names it: any
/// bytes, no panic, and no certificate made of nothing.
pub fn pem_bundle(data: &[u8]) {
    if let Ok(certificates) = pod_tls::bundle::pem_certificates(data) {
        assert!(certificates.iter().all(|c| !c.is_empty()));
    }
}

/// An HTTP response head (`pod_proxy::http::parse_head`; podssh
/// `http_head`), as a relay or a proxy sends it: the reason is safe on one
/// line, and each header name is in lower case and each value trimmed, as
/// the readers of the head expect.
pub fn http_head(data: &[u8]) {
    let Some(head) = text(data) else { return };
    let Ok((_, reason, headers)) = parse_head(head) else { return };
    assert!(is_one_line(&reason), "{reason:?}");
    for (name, value) in headers {
        assert_eq!(name, name.trim().to_ascii_lowercase(), "{name:?}");
        assert_eq!(value, value.trim(), "{value:?}");
    }
}

/// A chunked body (`pod_proxy::http::decode_chunked`; podssh
/// `http_chunked`), as a server sends it to the GET client: a body is never
/// longer than its limit, under a small limit and under the largest.
pub fn http_chunked(data: &[u8]) {
    for max in [0, 16, 4096, usize::MAX] {
        if let Ok(Some(body)) = decode_chunked(data, max) {
            assert!(body.len() <= max, "{} bytes past the limit of {max}", body.len());
            assert!(body.len() <= data.len(), "{} bytes from {}", body.len(), data.len());
        }
    }
}

/// The answers of a SOCKS5 proxy (`pod_proxy::fuzz_exchange`, the feature
/// `fuzzing`; podssh `socks5_answer`): the first byte chooses whether the
/// URL has credentials and whether the target goes by name; the rest is
/// what the proxy says before it ends. The exchange ends at once, as the
/// proxy is always ready, and never panics.
pub fn socks5_answer(data: &[u8]) {
    let Some((&choice, answer)) = data.split_first() else { return };
    let _ = pod_proxy::fuzz_exchange(answer, choice & 1 != 0, choice & 2 != 0);
}

/// A proxy URL (`pod_proxy::Proxy::parse`; podssh `proxy_url` and podnet
/// `proxy_url`), as a proxy variable holds it. podssh: a proxy on TCP
/// shows only its host and port, and its URL, which carries the
/// credentials again for a library, parses back to the same proxy. podnet:
/// the label of a line has no user and no password. A proxy on a Unix
/// socket has no credentials, and a variable never names a command.
pub fn proxy_url(data: &[u8]) {
    let Some(url) = text(data) else { return };
    let Ok(proxy) = Proxy::parse(url) else { return };
    match proxy.carrier() {
        Carrier::Tcp => {
            assert_eq!(proxy.to_string(), authority(&proxy.host, proxy.port));
            assert!(!proxy.label().contains('@'), "{}", proxy.label());
        }
        Carrier::Unix(path) => {
            assert!(!proxy.has_credentials());
            assert_eq!(proxy.to_string(), format!("unix:{}", path.display()));
        }
        Carrier::Exec(_) => panic!("Proxy::parse took a command from {url:?}"),
    }
    assert_eq!(Proxy::parse(&proxy.url()), Ok(proxy.clone()), "{}", proxy.url());
}

/// A `NO_PROXY` list against a host (`pod_proxy::env::no_proxy_matches`
/// and `NoProxy`; podssh `no_proxy` and podnet `no_proxy`). podssh: the
/// first line is the list, the rest the host, and `*` matches each host.
/// podnet: the list with ports, against the hosts of the API, of the edge
/// and of the loopback. An entry that excludes a port also excludes the
/// host with no port, and it is an entry of the list.
pub fn no_proxy(data: &[u8]) {
    let Some(input) = text(data) else { return };
    let (list, host) = input.split_once('\n').unwrap_or((input, ""));
    let _ = no_proxy_matches(list, host);
    assert!(no_proxy_matches("*", host), "{host:?}");
    let rules = NoProxy::parse(list);
    for (host, port) in [(host, 443), ("api.trycloudflare.com", 443), ("198.41.192.167", 7844), ("::1", 80)] {
        let entry = no_proxy_entry_at(list, host, Some(port));
        assert_eq!(rules.matches(host, port), entry.is_some(), "{host:?} {port}");
        if let Some(entry) = entry {
            assert!(list.contains(entry), "{entry:?}");
            assert!(no_proxy_matches(list, host), "{entry:?} with no port, {host:?}");
        }
        assert!(NoProxy::parse("*").matches(host, port), "{host:?}");
    }
}

/// A proxy's answer to `CONNECT` (`pod_proxy::http::parse_status_line`;
/// podssh `connect_status` and podnet `connect_response_head`), as the dial
/// reads it: any bytes to the end of the head, as lossy UTF-8. podssh: the
/// reason goes to the user's terminal, so it is safe on one line. The
/// readers of a 407 (`digest::challenge_in`, `proxy_auth::schemes`) take
/// the same head with no panic, and give 8 schemes at most.
pub fn connect_status(data: &[u8]) {
    let head = String::from_utf8_lossy(data);
    if let Ok((_, reason)) = parse_status_line(&head) {
        assert!(is_one_line(&reason), "{reason:?}");
    }
    let _ = digest::challenge_in(&head);
    let schemes = proxy_auth::schemes(&head);
    assert!(schemes.len() <= 8, "{schemes:?}");
}

/// An answer of DNS over HTTPS in RFC 8484's wire form
/// (`pod_dns::wire`, `pod_dns::https`; podssh `dns_wire`), as a resolver
/// sends it: each decoder ends on any bytes, an address has the family of
/// the type asked, an SRV target is a name in lower case that a resolver
/// can be asked for, NXDOMAIN is said for RCODE 3 of a response and for
/// nothing else, and a name read from any offset ends within the bytes, at
/// most 253 characters long. An HTTPS record's list for Encrypted Client
/// Hello is whole by its own lengths, within its bound, from an answer or
/// from a record's data alone.
pub fn dns_wire(data: &[u8]) {
    use pod_dns::{NXDOMAIN, https, wire};
    const NAME: &str = "tcp.ssh.relay.ajam.dev";
    const SRV_NAME: &str = "_v2-origintunneld._tcp.argotunnel.com";
    let rcode3 = data.len() >= 12 && (data[2] & 0x80) != 0 && (data[3] & 0x0f) == 3;
    for (kind, v6) in [(wire::A, false), (wire::AAAA, true)] {
        match wire::addrs(data, NAME, kind) {
            Ok(ips) => assert!(ips.iter().all(|ip| ip.is_ipv6() == v6), "{ips:?}"),
            Err(e) => assert_eq!(e == NXDOMAIN, rcode3, "{e}"),
        }
    }
    match wire::srv(data, SRV_NAME) {
        Ok(records) => {
            for r in records {
                let safe =
                    r.target.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-._".contains(&b));
                assert!(safe && !r.target.is_empty() && r.target.len() <= 253, "{r:?}");
            }
        }
        Err(e) => assert_eq!(e == NXDOMAIN, rcode3, "{e}"),
    }
    let whole = |list: &[u8]| https::check_list(list).is_ok() && list.len() <= https::MAX_LIST;
    match https::from_wire(data, NAME) {
        Ok(Some(found)) => assert!(whole(&found.list), "{found:?}"),
        Ok(None) => {}
        Err(e) => assert_eq!(e == NXDOMAIN, rcode3, "{e}"),
    }
    if let Ok(Some((_, list))) = https::from_rdata(data) {
        assert!(whole(&list), "{list:?}");
    }
    let start = data.first().map_or(0, |&b| usize::from(b) % data.len().max(1));
    if let Ok((name, end)) = wire::read_name(data, start) {
        assert!(end <= data.len() && name.len() <= 253, "{name:?} ends at {end}");
    }
}

/// An answer of DNS over HTTPS in the JSON form (`pod_dns::json`,
/// `pod_dns::https::from_json`; podnet `doh_json`), as a resolver sends it:
/// podnet's reading of each record's data as an address and as an SRV
/// record (64 records at most), with no panic; the status of an answer
/// decides NXDOMAIN for each type; an SRV target is a name in lower case
/// that a resolver can be asked for; an ECH list is whole by its own
/// lengths, within its bound.
pub fn doh_json(data: &[u8]) {
    use pod_dns::{NXDOMAIN, https, json};
    if let Ok(answer) = json::parse(data) {
        for record in answer.records.iter().take(64) {
            let _ = record.data.parse::<std::net::IpAddr>();
            if let Some(srv) = json::srv_of(&record.data) {
                assert!(is_safe_name(&srv.target), "{srv:?}");
            }
        }
        for want in [1, 28, 33, 65] {
            let said = answer.of(want).err();
            assert_eq!(said.as_deref() == Some(NXDOMAIN), answer.status == Some(3), "{said:?}");
        }
    }
    if let Ok(records) = json::parse_srv(data) {
        assert!(records.iter().all(|r| is_safe_name(&r.target)), "{records:?}");
    }
    if let Ok(Some(found)) = https::from_json(data) {
        assert!(https::check_list(&found.list).is_ok() && found.list.len() <= https::MAX_LIST, "{found:?}");
    }
}

/// A name in lower case that a resolver can be asked for: letters, digits,
/// `-`, `.` and `_`, 1 to 253 bytes.
fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-._".contains(&b))
}

/// The target of a request to the egress broker
/// (`pod_broker::target::{parse_authority, parse_absolute}`; new in
/// pod-broker, T-099): a target that reads is in its one form. Its
/// authority reads back to itself; its path is a fixed point of
/// `normal_path`, with no empty segment, no dot segment, no backslash and
/// no escape of a dot or a slash.
pub fn broker_target(data: &[u8]) {
    use pod_broker::target::{normal_path, parse_absolute, parse_authority};
    let Some(input) = text(data) else { return };
    if let Ok(authority) = parse_authority(input, None) {
        assert_eq!(parse_authority(&authority.to_string(), None).as_ref(), Ok(&authority), "{input:?}");
    }
    if let Ok(target) = parse_absolute(input) {
        let path = &target.path;
        assert_eq!(normal_path(path).as_ref(), Ok(path), "{input:?}");
        assert!(!path.contains("//") && !path.contains('\\'), "{path:?}");
        for segment in path.split('/') {
            assert!(segment != "." && segment != "..", "{path:?}");
        }
        let lower = path.to_ascii_lowercase();
        assert!(!lower.contains("%2e") && !lower.contains("%2f") && !lower.contains("%5c"), "{path:?}");
        assert_eq!(parse_authority(&target.authority.to_string(), None).as_ref(), Ok(&target.authority));
    }
}

/// `pod-gsr`: the reader of the relay's 32-byte messages, on the bytes of
/// the input after its first two, in fragments of 1 to 32 bytes and with
/// cancelled reads (`wire::fuzz_messages` says how the first two bytes
/// choose them). A read takes no byte past the message that it gives, a
/// cancel loses no byte, and the fragments give the messages of whole reads;
/// each message takes 32 bytes of the input.
pub fn gsr_message(data: &[u8]) {
    let messages = pod_gsr::wire::fuzz_messages(data);
    assert!(messages.len() * pod_gsr::wire::MESSAGE_LEN <= data.len().saturating_sub(2), "{messages:?}");
}

/// WebSocket frames (`pod_ws::frame::decode`; podssh `ws_frame`), as each
/// side writes them: a frame consumes at most its input; a control frame
/// has FIN and at most 125 bytes (RFC 6455 section 5.5); a decoded frame,
/// encoded again by the same side, decodes to itself.
pub fn ws_frame(data: &[u8]) {
    use pod_ws::frame::{MAX_CONTROL_PAYLOAD, Role, decode, encode};
    for role in [Role::Client, Role::Server] {
        let Ok(Some((frame, used))) = decode(data, role) else { continue };
        assert!((2..=data.len()).contains(&used), "{used} bytes of {}", data.len());
        if frame.opcode >= 0x8 {
            assert!(frame.fin && frame.payload.len() <= MAX_CONTROL_PAYLOAD, "{frame:?}");
        }
        let again = encode(&frame, role, [0x37, 0xfa, 0x21, 0x3d]);
        assert_eq!(decode(&again, role).ok().flatten(), Some((frame, again.len())));
    }
}

/// The answer to the WebSocket upgrade (`pod_ws::handshake::check_response`;
/// podssh `ws_upgrade`), with the key of RFC 6455's example: any head, no
/// panic, and an error that is safe to print.
pub fn ws_upgrade(data: &[u8]) {
    let Some(head) = text(data) else { return };
    if let Err(e) = pod_ws::handshake::check_response(head, "dGhlIHNhbXBsZSBub25jZQ==") {
        let shown = e.to_string();
        assert!(!shown.contains('\u{1b}') && !shown.contains('\u{7}'), "{shown:?}");
    }
}

/// A Close frame's payload (`pod_ws::close::close_code_and_reason`; podssh
/// `close_reason`), as the relay sends it: the code is the first two bytes
/// when there are two, and the reason, which goes to the user's terminal, is
/// safe on one line.
pub fn close_reason(data: &[u8]) {
    let (code, reason) = pod_ws::close::close_code_and_reason(data);
    match data {
        [a, b, ..] => assert_eq!(code, Some(u16::from_be_bytes([*a, *b]))),
        _ => assert_eq!((code, reason.as_str()), (None, "")),
    }
    assert!(is_one_line(&reason), "{reason:?}");
}

/// The answer of a server of QUIC to the probe of UDP
/// (`pod_quic::probe::answers`; new in `pod-quic`, T-019): the first 8
/// bytes are the Destination Connection ID of the probe's packet, the rest
/// is the datagram. An answer is a long header of the version 0 with no
/// Destination, the ID as its Source, and whole versions, none of them the
/// reserved one; the probe's own packet does not answer it.
pub fn quic_probe_answer(data: &[u8]) {
    use pod_quic::probe::{RESERVED, answers, packet};
    let Some((id, datagram)) = data.split_first_chunk::<8>() else { return };
    if answers(datagram, id) {
        assert!(datagram[0] & 0x80 != 0 && datagram[1..5] == [0; 4], "{datagram:02x?}");
        assert_eq!((datagram[5], datagram[6]), (0, 8), "{datagram:02x?}");
        assert_eq!(&datagram[7..15], id);
        let versions = &datagram[15..];
        assert!(!versions.is_empty() && versions.len() % 4 == 0, "{datagram:02x?}");
        assert!(versions.chunks(4).all(|v| v != RESERVED), "{datagram:02x?}");
    }
    assert!(!answers(&packet(id), id));
}

/// A frame of a magic-wormhole mailbox and a typed code (`pod_wormhole::
/// message`, `pod_wormhole::Code::parse`; new in `pod-wormhole`, T-022): a
/// value that is no JSON object gives nothing; the words of a mailbox are
/// one safe line of at most 200 characters; a message has its three parts
/// and its body in hex, which goes back to the same bytes; a `pake` body
/// never panics; hex of any bytes reads back as they were; a code that
/// parses has 1 to 8 words, a nameplate of digits, and parses again from
/// its own text to the same code.
pub fn mailbox_message(data: &[u8]) {
    use pod_wormhole::message::{WORDS_MOST, hex, message_of, object, pake_of, unhex, words};
    use pod_wormhole::phrase::{self, Code};

    assert_eq!(unhex(&hex(data)).as_deref(), Some(data));
    let _ = pake_of(data);
    if let Some(value) = object(data) {
        for part in ["error", "motd", "type"] {
            let line = words(&value[part]);
            assert!(is_one_line(&line) && line.chars().count() <= WORDS_MOST, "{line:?}");
        }
        if let Some(message) = message_of(&value) {
            assert_eq!(unhex(&hex(&message.body)), Some(message.body.clone()));
            let _ = pake_of(&message.body);
        }
    }
    let Some(typed) = text(data) else { return };
    if let Ok(code) = Code::parse(typed) {
        assert!((1..=phrase::WORDS_MOST).contains(&code.words()), "{} words", code.words());
        assert!(phrase::nameplate_ok(code.nameplate()), "{}", code.nameplate());
        let again = Code::parse(code.for_terminal()).expect("a code parses from its own text");
        assert_eq!(again.for_terminal(), code.for_terminal());
        assert_eq!(again.password(), code.password());
    }
}

/// The bytes of one link of the resumable layer
/// (`pod_session::decode::Decoder`; new in `pod-session`, T-020), pushed in
/// pieces whose sizes the first byte sets. Each record that comes out
/// encodes again, to the same record; after an error, each call gives the
/// same error, and nothing after it is read.
pub fn session_record(data: &[u8]) {
    use pod_session::decode::Decoder;
    let Some((&step, bytes)) = data.split_first() else { return };
    let step = usize::from(step % 64) + 1;
    let mut decoder = Decoder::new();
    for piece in bytes.chunks(step) {
        decoder.push(piece);
        loop {
            match decoder.next() {
                Ok(Some(record)) => {
                    let mut again = Vec::new();
                    record.encode(&mut again).expect("a decoded record encodes");
                    let mut other = Decoder::new();
                    other.push(&again);
                    assert!(other.next().expect("its bytes decode") == Some(record), "{again:02x?}");
                    assert_eq!(other.buffered(), 0);
                }
                Ok(None) => break,
                Err(e) => {
                    decoder.push(b"\x01");
                    assert_eq!(decoder.next().unwrap_err(), e);
                    return;
                }
            }
        }
    }
}

/// The state of a far end's session that the binary before this one handed
/// over (`pod_session::handover::LayerState::decode`; new in `pod-session`,
/// T-020). A state that reads encodes to bytes that read to the same
/// bytes again.
pub fn session_handover(data: &[u8]) {
    use pod_session::handover::LayerState;
    if let Ok(state) = LayerState::decode(data) {
        let bytes = state.encode();
        let again = LayerState::decode(&bytes).expect("an encoded state reads");
        assert_eq!(*again.encode(), *bytes);
    }
}

/// UDP in a stream (`pod_session::udp::Decoder`, `framed`; new in
/// `pod-session`, T-020), pushed in pieces whose sizes the first byte sets:
/// each datagram is at most `MAX_DATAGRAM` bytes and frames back to the
/// bytes that it came from; a length over the limit is refused.
pub fn udp_frame(data: &[u8]) {
    use pod_session::udp::{Decoder, MAX_DATAGRAM, framed};
    let Some((&step, bytes)) = data.split_first() else { return };
    let step = usize::from(step % 64) + 1;
    let (mut decoder, mut used) = (Decoder::new(), 0);
    for piece in bytes.chunks(step) {
        decoder.push(piece);
        loop {
            match decoder.next_datagram() {
                Ok(Some(datagram)) => {
                    assert!(datagram.len() <= MAX_DATAGRAM);
                    let frame = framed(&datagram).expect("a decoded datagram frames");
                    assert_eq!(frame[..], bytes[used..used + frame.len()]);
                    used += frame.len();
                }
                Ok(None) => break,
                Err(_) => {
                    let len = usize::from(u16::from_be_bytes([bytes[used], bytes[used + 1]]));
                    assert!(len > MAX_DATAGRAM);
                    return;
                }
            }
        }
    }
    assert_eq!(used + decoder.pending(), bytes.len());
}
/// The address of a call in the memory of a held task
/// (`pod_hold::sockaddr::read` and `parse`; podnet
/// `crates/podnet-netns/src/seccomp/notif.rs` `parse_sockaddr`): the first
/// two bytes are the length that the task gives, the next two the
/// address; the rest is the memory, a sealed memfd. The read takes at most
/// the length and 128 bytes, and only inside the memory (a length of 0
/// reads nothing); an address comes
/// only from as many bytes as its family needs, and encodes back to them.
pub fn hold_sockaddr(data: &[u8]) {
    use pod_hold::sockaddr::{MOST, encode, parse, read};
    let Some((head, image)) = data.split_first_chunk::<4>() else { return };
    let len = u64::from(u16::from_le_bytes([head[0], head[1]]));
    let at = u64::from(u16::from_le_bytes([head[2], head[3]]));
    let fd = pod_hold::mem::memfd(image).expect("a memfd");
    let memory = pod_hold::mem::Memory::of_fd(fd, false);
    let want = (len as usize).min(MOST);
    let Ok(bytes) = read(&memory, at, len) else {
        assert!(at as usize + want > image.len(), "a read inside the memory fails");
        return;
    };
    assert_eq!(bytes.len(), want);
    // A length of 0 reads no memory.
    if want > 0 {
        assert_eq!(bytes[..], image[at as usize..at as usize + want]);
    }
    if let Some(addr) = parse(&bytes) {
        let ip = if addr.ip.is_ipv4() { 4..8 } else { 8..24 };
        assert!(bytes.len() >= ip.end, "{} bytes for {addr:?}", bytes.len());
        let back = encode(addr.family, addr.ip, addr.port);
        assert_eq!(back[..4], bytes[..4]);
        assert_eq!(back[ip.clone()], bytes[ip]);
    }
}

/// A packet that a payload sends on the TUN of the netns hold
/// (`pod_hold::packet::parse_tcp_syn` and `rst_reply_for_syn`; podnet
/// `crates/podnet-netns/src/netns/packet.rs`): a SYN has a header of 20
/// bytes or more; its RST|ACK has 40 bytes, both checksums sum to 0, the
/// addresses and ports are swapped, the acknowledgement is one past the
/// sequence, and the RST is no SYN.
pub fn hold_syn(data: &[u8]) {
    use pod_hold::packet::{ipv4_checksum, parse_tcp_syn, rst_reply_for_syn};
    let parsed = parse_tcp_syn(data);
    let rst = rst_reply_for_syn(data);
    assert_eq!(parsed.is_some(), rst.is_some());
    let (Some((src, sport, dst, dport)), Some(rst)) = (parsed, rst) else { return };
    let ihl = usize::from(data[0] & 0x0f) * 4;
    assert!(ihl >= 20 && data.len() >= ihl + 20, "a SYN with a header of {ihl} bytes");
    assert_eq!(rst.len(), 40);
    assert_eq!(ipv4_checksum(&rst[..20]), 0);
    let mut pseudo = rst[12..20].to_vec();
    pseudo.extend_from_slice(&[0, 6, 0, 20]);
    pseudo.extend_from_slice(&rst[20..40]);
    assert_eq!(ipv4_checksum(&pseudo), 0);
    assert_eq!(rst[12..16], dst.octets());
    assert_eq!(rst[16..20], src.octets());
    assert_eq!(u16::from_be_bytes([rst[20], rst[21]]), dport);
    assert_eq!(u16::from_be_bytes([rst[22], rst[23]]), sport);
    let seq = u32::from_be_bytes([data[ihl + 4], data[ihl + 5], data[ihl + 6], data[ihl + 7]]);
    assert_eq!(u32::from_be_bytes([rst[28], rst[29], rst[30], rst[31]]), seq.wrapping_add(1));
    assert!(parse_tcp_syn(&rst).is_none());
}
