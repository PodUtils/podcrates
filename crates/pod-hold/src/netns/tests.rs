use std::os::fd::{AsRawFd, IntoRawFd};

use super::*;

#[test]
fn a_descriptor_goes_over_the_socket_pair() {
    let (a, b) = socketpair().unwrap();
    let (r, w) = std::io::pipe().unwrap();
    send_fd(a.as_raw_fd(), w.as_raw_fd()).unwrap();
    drop(w);
    let got = recv_fd(b.as_raw_fd()).unwrap();
    write_all_fd(got.as_raw_fd(), b"held").unwrap();
    drop(got);
    let mut buf = [0u8; 8];
    assert_eq!(read_exact_fd(r.as_raw_fd(), &mut buf).unwrap(), 4);
    assert_eq!(&buf[..4], b"held");
}

// A message with no descriptor, or with two, is refused: the receiver
// keeps no descriptor that it did not ask for.
#[test]
fn a_message_with_no_descriptor_or_with_two_is_refused() {
    let (a, b) = socketpair().unwrap();
    write_all_fd(a.as_raw_fd(), b"x").unwrap();
    assert!(recv_fd(b.as_raw_fd()).unwrap_err().text().contains("no descriptor"));
    let (_r, w) = std::io::pipe().unwrap();
    let fds = [w.as_raw_fd(), w.as_raw_fd()];
    let mut byte = [0u8; 1];
    let mut iov = libc::iovec { iov_base: byte.as_mut_ptr() as *mut _, iov_len: 1 };
    let mut control = [0u64; 8];
    // SAFETY: a zeroed msghdr; the buffers live for the call.
    let sent = unsafe {
        let mut msg: libc::msghdr = std::mem::zeroed();
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = control.as_mut_ptr() as *mut _;
        msg.msg_controllen = libc::CMSG_SPACE(8) as _;
        let hdr = libc::CMSG_FIRSTHDR(&msg);
        (*hdr).cmsg_level = libc::SOL_SOCKET;
        (*hdr).cmsg_type = libc::SCM_RIGHTS;
        (*hdr).cmsg_len = libc::CMSG_LEN(8) as _;
        std::ptr::copy_nonoverlapping(fds.as_ptr().cast::<u8>(), libc::CMSG_DATA(hdr), 8);
        libc::sendmsg(a.as_raw_fd(), &msg, 0)
    };
    assert_eq!(sent, 1);
    assert!(recv_fd(b.as_raw_fd()).unwrap_err().text().contains("2 descriptors"));
    // The peer closed: no descriptor.
    drop(a);
    assert!(recv_fd(b.into_raw_fd()).is_err());
}

#[test]
fn podnets_tun_is_the_default() {
    let tun = Tun::default();
    assert_eq!(tun.name, "eth0");
    assert_eq!(tun.netmask(), std::net::Ipv4Addr::new(255, 255, 255, 254));
    assert_eq!(Tun { prefix: 0, ..Tun::default() }.netmask(), std::net::Ipv4Addr::UNSPECIFIED);
    assert_eq!(Tun { prefix: 32, ..Tun::default() }.netmask(), std::net::Ipv4Addr::BROADCAST);
    let long = Tun { name: "a-name-longer-than-ifnamsiz".into(), ..Tun::default() };
    assert_eq!(create_tun(&long).unwrap_err().class, Class::Usage);
    let ifr = ifreq_for("lo");
    assert_eq!(ifr.ifr_name[0] as u8, b'l');
    assert_eq!(ifr.ifr_name[2], 0);
}

#[test]
fn the_handshake_maps_a_user_namespace() {
    let (child, parent) = socketpair().unwrap();
    // Not a user namespace: no maps, no answer.
    write_all_fd(child.as_raw_fd(), b"n").unwrap();
    assert!(!answer(parent.as_raw_fd(), 0, 0, 0).unwrap());
    drop(child);
    assert!(answer(parent.as_raw_fd(), 0, 0, 0).is_err(), "the child died");
}
