//! The rows of the file classes of podssh: an absent `/dev/pts` and
//! `/proc`, the controlling terminal, the lines of the ids, `HOME` on a
//! `noexec` mount, and a read-only `/etc/ssl`. A row of a fact gives `ok`
//! with its value in its reason ([`crate::verdict::items`]); an errno of
//! the read is the denial.

use super::*;
use crate::verdict::items;

/// The errno of an I/O error, or a skip where it has none.
fn io_denied(what: &str, e: &std::io::Error) -> Outcome {
    match e.raw_os_error() {
        Some(n) => Outcome::denied(Errno(n)),
        None => Outcome::skip(None, format!("{what}: {e}")),
    }
}

pub(super) fn p_stat_pts() -> Outcome {
    match sys::stat(&c("/dev/pts")) {
        Ok(st) => Outcome::ok_with(format!("{} mode {:o}", st.kind(), st.st_mode & 0o7777)),
        // ENOENT is the answer: the row asks whether the path is there.
        Err(e) => Outcome::denied(e),
    }
}

/// In the probe process ([`Kind::Here`]): a host with no `/proc` starts
/// no child through `/proc/self/exe`. The path is not in the reason: it
/// can name a directory of the user.
pub(super) fn p_readlink_exe() -> Outcome {
    match sys::readlink(&c("/proc/self/exe")) {
        Ok(_) => Outcome::ok(),
        Err(e) => Outcome::denied(e),
    }
}

/// The field `tty_nr` of a text of `/proc/PID/stat`: the seventh field,
/// the fifth after the name in parentheses.
pub fn tty_nr(stat: &str) -> Option<i64> {
    pod_core::proc_stat::after_name(stat)?.get(4)?.parse().ok()
}

/// A probe child keeps the session and the terminal of its parent.
pub(super) fn p_tty_nr() -> Outcome {
    match std::fs::read_to_string("/proc/self/stat") {
        Err(e) => io_denied("/proc/self/stat", &e),
        Ok(text) => match tty_nr(&text) {
            Some(n) => Outcome::ok_with(items(&[("tty_nr", &n.to_string())])),
            None => Outcome::skip(None, "/proc/self/stat has no field tty_nr"),
        },
    }
}

/// An open of `/dev/tty` that does not wait, then a poll with no wait: the
/// row reads no byte, so it takes no input of a terminal. `read waits`: a
/// read would wait for data.
pub(super) fn p_tty_poll() -> Outcome {
    let flags = sys::O_RDWR | sys::O_NOCTTY | sys::O_NONBLOCK | sys::O_CLOEXEC;
    let fd = match sys::open(&c("/dev/tty"), flags, 0) {
        Ok(fd) => fd,
        // ENXIO: no controlling terminal and the node of the kernel.
        Err(e) => return Outcome::denied(e),
    };
    let mut p = [sys::PollFd { fd: fd as i32, events: sys::POLLIN, revents: 0 }];
    let r = sys::ppoll(&mut p, 0);
    let _ = sys::close(fd);
    match r {
        Ok(0) => Outcome::ok_with(items(&[("read", "waits")])),
        Ok(_) => Outcome::ok_with(items(&[("read", "ready")])),
        Err(e) => Outcome::skip(Some(e), "ppoll(2) of /dev/tty failed"),
    }
}

/// The lines of `text` (`/etc/passwd` or `/etc/group`) whose third field
/// is `id`.
pub fn lines_of(text: &str, id: u32) -> usize {
    let id = id.to_string();
    text.lines().filter(|l| l.split(':').nth(2) == Some(id.as_str())).count()
}

fn id_lines(file: &str, key: &str, nr: i64) -> Outcome {
    // SAFETY: no pointer; getuid and getgid always succeed.
    let id = unsafe { sys::syscall6(nr, 0, 0, 0, 0, 0, 0) } as u32;
    let id_text = id.to_string();
    match std::fs::read_to_string(file) {
        // No file: no line of the id.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Outcome::ok_with(items(&[(key, &id_text), ("lines", "0"), ("file", "absent")]))
        }
        Err(e) => io_denied(file, &e),
        Ok(text) => Outcome::ok_with(items(&[(key, &id_text), ("lines", &lines_of(&text, id).to_string())])),
    }
}

pub(super) fn p_uid_line() -> Outcome {
    id_lines("/etc/passwd", "uid", sys::SYS_GETUID)
}

pub(super) fn p_gid_line() -> Outcome {
    id_lines("/etc/group", "gid", sys::SYS_GETGID)
}

/// `1`, `0`, or `unknown` for a mount flag of `statfs(2)`.
fn flag(s: &sys::Statfs, bit: u64) -> &'static str {
    match s.flag(bit) {
        Some(true) => "1",
        Some(false) => "0",
        None => "unknown",
    }
}

/// `HOME` of the probe process, as a program of the host reads it: the
/// `noexec` flag of its mount, then a new file in it. The path is not in
/// the reason: it names the user.
pub(super) fn p_home_creat() -> Outcome {
    let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) else {
        return Outcome::skip(None, "HOME is not set");
    };
    let home = home.to_string_lossy().into_owned();
    let Some(dir) = CBuf::new(&home) else {
        return Outcome::skip(None, "HOME holds a NUL");
    };
    let noexec = match sys::statfs(&dir) {
        Ok(s) => flag(&s, sys::ST_NOEXEC),
        Err(e) => return Outcome { reason: "statfs(2) of HOME refused".to_string(), ..Outcome::denied(e) },
    };
    let path = format!("{}/.pod-hostprobe-{}", home.trim_end_matches('/'), std::process::id());
    let Some(file) = CBuf::new(&path) else {
        return Outcome::skip(None, "HOME holds a NUL");
    };
    let reason = items(&[("noexec", noexec)]);
    match sys::open(&file, sys::O_WRONLY | sys::O_CREAT | sys::O_EXCL | sys::O_CLOEXEC, 0o600) {
        Err(sys::EEXIST) => Outcome::skip(Some(sys::EEXIST), "the scratch file of the probe is in HOME already"),
        Err(e) => Outcome { reason, ..Outcome::denied(e) },
        Ok(fd) => {
            let _ = sys::close(fd);
            match sys::unlink(&file) {
                Ok(_) => Outcome::ok_with(reason),
                Err(e) => Outcome::skip(Some(e), "made a scratch file in HOME and could not unlink it"),
            }
        }
    }
}

pub(super) fn p_statfs_ssl() -> Outcome {
    match sys::statfs(&c("/etc/ssl")) {
        Ok(s) => Outcome::ok_with(items(&[("rdonly", flag(&s, sys::ST_RDONLY)), ("noexec", flag(&s, sys::ST_NOEXEC))])),
        Err(e) => Outcome::denied(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_field_tty_nr_follows_the_name_in_parentheses() {
        // A name with a space and a parenthesis does not move the fields.
        let stat = "4242 (a b) c) S 1 4242 4242 34816 4242 4194560 0";
        assert_eq!(tty_nr(stat), Some(34816));
        assert_eq!(tty_nr("1 (init) S 0 1 1 0 -1"), Some(0));
        assert_eq!(tty_nr("1 (init) S 0"), None);
        assert_eq!(tty_nr("no parenthesis"), None);
    }

    #[test]
    fn a_line_of_an_id_is_its_third_field() {
        let passwd =
            "root:x:0:0:root:/root:/bin/sh\nnobody:x:65534:65534::/:/bin/false\nuser:x:1000:0::/home/user:/bin/sh\n";
        assert_eq!(lines_of(passwd, 0), 1);
        assert_eq!(lines_of(passwd, 65534), 1);
        assert_eq!(lines_of(passwd, 1000), 1);
        assert_eq!(lines_of(passwd, 1001), 0);
        // The gid field of a passwd line is not the third field.
        assert_eq!(lines_of("a:x:5:1000::/:/bin/sh", 1000), 0);
        assert_eq!(lines_of("", 0), 0);
    }
}
