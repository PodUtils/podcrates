//! The socket of a TPM in software (swtpm, the simulator of the reference),
//! against the stand-in of `standin/`: the framing of swtpm and of the
//! `mssim` interface, the bound of a response, a short, late, closed and
//! bad answer, and the bound of a command.
#![cfg(feature = "device")]

mod standin;

use std::time::Duration;

use pod_core::Class;
use pod_tpm::transport::{Framing, Limits, Socket};
use pod_tpm::{cmd, transact};
use standin::{Answer, Frame, ended, response, serve};

const WAIT: Duration = Duration::from_secs(10);

/// The tests on a socket pair and on a Unix socket of a path.
#[cfg(unix)]
mod unix {
    use super::standin::{framed, header_saying};
    use super::*;
    use pod_tpm::Transport;
    use pod_tpm::wire::{MAX_COMMAND, MAX_RESPONSE};

    fn short_limits() -> Limits {
        Limits { command: Duration::from_millis(200) }
    }

    fn frame_of(framing: Framing) -> Frame {
        match framing {
            Framing::Raw => Frame::Raw,
            Framing::Mssim => Frame::Mssim,
        }
    }

    fn pair(
        framing: Framing,
        limits: Limits,
        answers: Vec<Answer>,
    ) -> (Socket, std::sync::mpsc::Receiver<standin::Seen>) {
        let (ours, theirs) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        let socket = Socket::from_unix(ours, limits).expect("a socket").with_framing(framing);
        (socket, serve(theirs, frame_of(framing), answers))
    }

    #[test]
    fn the_socket_of_swtpm_carries_a_command_and_its_response() {
        for framing in [Framing::Raw, Framing::Mssim] {
            let command = cmd::flush_context(0x8000_0000).unwrap();
            let (mut socket, rx) = pair(framing, Limits::default(), vec![Answer::Whole(response(10))]);
            transact(&mut socket, &command).expect("a response").empty().expect("no parameter");
            drop(socket);
            let seen = ended(&rx, WAIT);
            assert_eq!(seen.commands, vec![command], "{framing:?}");
            assert!(seen.closed, "{framing:?}: the socket stays open after the drop");
        }
    }

    #[test]
    fn a_response_of_the_bound_is_read_whole() {
        for framing in [Framing::Raw, Framing::Mssim] {
            let big = response(MAX_RESPONSE);
            let (mut socket, _rx) = pair(framing, Limits::default(), vec![Answer::Whole(big.clone())]);
            assert_eq!(
                socket.submit(&[0x80, 0x01, 0, 0, 0, 10, 0, 0, 1, 0x7e]).expect("a response"),
                big,
                "{framing:?}"
            );
        }
    }

    #[test]
    fn a_response_over_the_bound_is_refused_before_it_is_read() {
        let command = cmd::flush_context(0x8000_0000).unwrap();
        let over = (MAX_RESPONSE + 1) as u32;
        let cases = [
            (Framing::Raw, header_saying(over, MAX_RESPONSE + 1 - 10)),
            (Framing::Raw, header_saying(9, 0)),
            (Framing::Mssim, framed(Frame::Mssim, &response(MAX_RESPONSE + 1))),
            (Framing::Mssim, [over.to_be_bytes().as_slice(), &[0u8; 64]].concat()),
        ];
        for (framing, bytes) in cases {
            let (mut socket, _rx) = pair(framing, Limits::default(), vec![Answer::Bytes(bytes)]);
            let e = socket.submit(&command).map(|r| r.len()).expect_err("a response over the bound");
            assert_eq!(e.class, Class::Protocol, "{framing:?}: {e}");
            assert!(e.text().contains("bytes"), "{framing:?}: {e}");
            let again = socket.submit(&command).expect_err("a broken socket");
            assert_eq!(again.class, Class::Unavailable, "{framing:?}: {again}");
        }
    }

    #[test]
    fn a_short_response_is_a_protocol_failure() {
        for framing in [Framing::Raw, Framing::Mssim] {
            let whole = framed(frame_of(framing), &response(20));
            let cut = whole[..whole.len() - 9].to_vec();
            let (mut socket, _rx) = pair(framing, Limits::default(), vec![Answer::Bytes(cut)]);
            let e = socket.submit(&cmd::flush_context(1).unwrap()).expect_err("a short response");
            assert_eq!(e.class, Class::Protocol, "{framing:?}: {e}");
        }
    }

    #[test]
    fn a_bad_acknowledgement_is_a_protocol_failure() {
        let mut bytes = framed(Frame::Mssim, &response(10));
        let at = bytes.len() - 1;
        bytes[at] = 1;
        let (mut socket, _rx) = pair(Framing::Mssim, Limits::default(), vec![Answer::Bytes(bytes)]);
        let e = socket.submit(&cmd::flush_context(1).unwrap()).expect_err("a bad acknowledgement");
        assert_eq!(e.class, Class::Protocol, "{e}");
        assert!(e.text().contains("acknowledgement"), "{e}");
    }

    #[test]
    fn a_closed_socket_is_unavailable() {
        let (mut socket, _rx) = pair(Framing::Raw, Limits::default(), vec![Answer::Close]);
        let e = socket.submit(&cmd::flush_context(1).unwrap()).expect_err("a closed socket");
        assert_eq!(e.class, Class::Unavailable, "{e}");
    }

    #[test]
    fn a_late_response_is_a_timeout_and_the_socket_closes() {
        let (mut socket, rx) = pair(Framing::Raw, short_limits(), vec![Answer::Silent]);
        let e = socket.submit(&cmd::flush_context(1).unwrap()).expect_err("no answer");
        assert_eq!(e.class, Class::Timeout, "{e}");
        let seen = ended(&rx, WAIT);
        assert!(seen.closed, "the socket stays open after the time limit");
        let again = socket.submit(&cmd::flush_context(1).unwrap()).expect_err("a broken socket");
        assert_eq!(again.class, Class::Unavailable, "{again}");
    }

    #[test]
    fn a_command_out_of_its_bounds_is_refused_before_it_is_sent() {
        let (mut socket, rx) = pair(Framing::Raw, Limits::default(), vec![Answer::Whole(response(10))]);
        let mut long = response(MAX_COMMAND + 1);
        long[..2].copy_from_slice(&[0x80, 0x01]);
        for bad in [long, vec![0x80, 0x01, 0, 0]] {
            let e = socket.submit(&bad).expect_err("a command out of its bounds");
            assert_eq!(e.class, Class::Usage, "{e}");
        }
        transact(&mut socket, &cmd::flush_context(1).unwrap()).expect("the socket still works");
        drop(socket);
        assert_eq!(ended(&rx, WAIT).commands.len(), 1, "a refused command went out");
    }

    #[test]
    fn the_unix_socket_of_a_path_answers() {
        let dir = pod_testkit::host::socket_dir("pod-tpm-socket");
        std::fs::create_dir_all(&dir).expect("a directory");
        let path = dir.join("swtpm.sock");
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path).expect("bind");
        let accepted = std::thread::spawn(move || listener.accept().map(|(s, _)| s));
        let mut socket = Socket::connect_unix(&path, Limits::default()).expect("a connection");
        let rx = serve(accepted.join().unwrap().expect("accept"), Frame::Raw, vec![Answer::Whole(response(10))]);
        transact(&mut socket, &cmd::flush_context(1).unwrap()).expect("a response");
        drop(socket);
        assert!(ended(&rx, WAIT).closed);

        let missing = dir.join("none.sock");
        let e = Socket::connect_unix(&missing, Limits::default()).err().expect("no socket");
        assert_eq!(e.class, Class::Unavailable, "{e}");
        assert!(e.text().contains("none.sock"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn the_tcp_channel_of_swtpm_answers() {
    pod_testkit::need_loopback!();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().unwrap();
    let accepted = std::thread::spawn(move || listener.accept().map(|(s, _)| s));
    let mut socket = Socket::connect_tcp(addr, Limits::default()).expect("a connection").with_framing(Framing::Mssim);
    let rx = serve(accepted.join().unwrap().expect("accept"), Frame::Mssim, vec![Answer::Whole(response(10))]);
    transact(&mut socket, &cmd::flush_context(1).unwrap()).expect("a response");
    drop(socket);
    assert!(ended(&rx, WAIT).closed);

    // A port with no listener: the address of a socket that is closed.
    let gone = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let e = Socket::connect_tcp(gone, Limits::default()).err().expect("no listener");
    assert_eq!(e.class, Class::Unavailable, "{e}");
}
