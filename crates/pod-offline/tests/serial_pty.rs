//! The tests of a serial line on a pseudo-terminal pair at 1200 baud.
//!
//! A host with no `/dev/ptmx` skips each test with the reason that the
//! helper prints. A skip is not a failure.

#![cfg(unix)]

mod common;

use std::io::Write;
use std::time::{Duration, Instant};

use pod_offline::serial::arq::{self, Arq};
use pod_offline::serial::kiss;
use pod_offline::serial::{self, SerialSpec};

fn line_at(pty: &common::Pty, baud: u32) -> serial::SerialLine {
    let address = format!("serial:{},{baud}", pty.slave.display());
    let spec = SerialSpec::parse(&address).expect("an address");
    serial::open(&spec).expect("a serial line")
}

#[test]
fn a_1200_baud_echo_comes_back_inside_the_frame_time() {
    let Some(pty) = common::pty() else { return };
    let mut line = line_at(&pty, 1200);
    let mut master = pty.master;
    let echo = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = [0u8; 5];
        line.read_exact(&mut buf).expect("five bytes");
        line.write_all(&buf).expect("the echo");
        line.flush().expect("a flush");
    });
    let started = Instant::now();
    master.write_all(b"ping!").expect("a write");
    master.flush().expect("a flush");
    let back = common::read_within(&mut master, Duration::from_secs(3)).expect("the echo within 3 s");
    let took = started.elapsed();
    echo.join().expect("the echo thread");
    assert_eq!(&back[..5], b"ping!");
    assert!(
        took < arq::frame_time(1200),
        "the echo took {took:?}, and one frame at 1200 baud is {:?}",
        arq::frame_time(1200)
    );
}

#[test]
fn a_frame_above_256_bytes_is_refused_by_name() {
    let Some(pty) = common::pty() else { return };
    let line = line_at(&pty, 1200);
    let above = vec![0x5a; kiss::MAX_FRAME + 1];
    let refused = kiss::encode(kiss::PORT_AX25, &above).expect_err("a frame above 256 bytes");
    assert_eq!(refused.name(), "frame-too-large");

    let mut sender = Arq::new(1, 1200).expect("a window");
    let refused = sender.push_at(Instant::now(), &vec![0u8; arq::MAX_PAYLOAD + 1]).expect_err("a payload above 255");
    assert_eq!(refused.name(), "frame-too-large");
    let full = sender.push_at(Instant::now(), &vec![0u8; arq::MAX_PAYLOAD]).expect("the full frame").expect("a place");
    assert_eq!(full.len(), kiss::MAX_FRAME);
    drop(line);
}

#[test]
fn a_lost_frame_is_sent_again_after_the_baud_time() {
    let Some(pty) = common::pty() else { return };
    let mut line = line_at(&pty, 1200);
    let mut master = pty.master;
    let mut sender = Arq::new(1, 1200).expect("a window");
    let started = Instant::now();
    let frame = sender.push_at(started, b"lost frame").expect("a frame").expect("a place");
    let wire = kiss::encode(kiss::PORT_AX25, &frame).expect("KISS");
    line.write_all(&wire).expect("a write");
    line.flush().expect("a flush");
    let first = common::read_within(&mut master, Duration::from_secs(2)).expect("the first frame");
    assert_eq!(first, wire);

    let mut again = None;
    while started.elapsed() < Duration::from_secs(8) {
        if let Some(frame) = sender.poll_at(Instant::now()).expect("a poll") {
            let wire = kiss::encode(kiss::PORT_AX25, &frame).expect("KISS");
            line.write_all(&wire).expect("a write");
            line.flush().expect("a flush");
            again = Some((started.elapsed(), frame));
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let (took, frame_again) = again.expect("a retransmission within 8 s");
    assert_eq!(frame_again, frame);
    assert!(
        took >= arq::retransmit_after(1200),
        "the retransmission came after {took:?}, before the frame time of {:?}",
        arq::retransmit_after(1200)
    );
    assert!(took < Duration::from_secs(8));
    assert_eq!(sender.retransmissions(), 1);
    let second = common::read_within(&mut master, Duration::from_secs(2)).expect("the second frame");
    assert_eq!(second, first);
}

#[test]
fn the_settings_of_the_device_go_back_when_the_line_drops() {
    let Some(pty) = common::pty() else { return };
    use std::os::fd::AsRawFd;

    let keeper = std::fs::OpenOptions::new().read(true).write(true).open(&pty.slave).expect("the slave");
    let fd = keeper.as_raw_fd();
    // SAFETY: tcgetattr writes into the struct of this function.
    let mut original: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: the descriptor is open.
    assert_eq!(unsafe { libc::tcgetattr(fd, &mut original) }, 0);
    let mut wanted = original;
    wanted.c_lflag |= libc::ICANON | libc::ECHO;
    // SAFETY: cfsetspeed writes only into the struct passed.
    assert_eq!(unsafe { libc::cfsetspeed(&mut wanted, libc::B9600) }, 0);
    // SAFETY: a termios that this descriptor read and this function changed.
    assert_eq!(unsafe { libc::tcsetattr(fd, libc::TCSANOW, &wanted) }, 0);

    let line = line_at(&pty, 1200);
    let mut now: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: the descriptor is open.
    assert_eq!(unsafe { libc::tcgetattr(fd, &mut now) }, 0);
    // SAFETY: cfgetospeed only reads the struct.
    assert_eq!(unsafe { libc::cfgetospeed(&now) }, libc::B1200);
    assert_eq!(now.c_lflag & libc::ICANON, 0);

    // The drop of the line, not a call of restore, puts the settings back.
    drop(line);
    let mut back: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: the descriptor is open.
    assert_eq!(unsafe { libc::tcgetattr(fd, &mut back) }, 0);
    // SAFETY: cfgetospeed only reads the struct.
    assert_eq!(unsafe { libc::cfgetospeed(&back) }, libc::B9600);
    assert_ne!(back.c_lflag & libc::ICANON, 0);
}
