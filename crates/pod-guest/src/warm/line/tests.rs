use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::{Arc, Mutex};

use super::*;
use crate::warm::Cold;
use crate::warm::tests::dir;
use crate::{Accel, warm};

/// The requests that a stand-in read.
type Log = Arc<Mutex<Vec<String>>>;

/// What a stand-in of the monitor of an emulator does.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Peer {
    /// Each transport works.
    Works,
    /// `file:` is refused, as a QEMU before 8.2 does; the pipe works.
    NoFile,
    /// Each migration fails.
    Fails,
    /// The migration writes no byte.
    Empty,
    /// The connection ends at the `migrate`.
    Leaves,
    /// The guest does not go on after the save.
    NoCont,
    /// The emulator cannot migrate a device.
    Blocked,
}

/// The path of the stream in a `migrate` URI of either transport.
fn stream_of(uri: &str) -> Option<PathBuf> {
    if let Some(p) = uri.strip_prefix("file:") {
        return Some(p.into());
    }
    let quoted = uri.strip_prefix("exec:cat > '")?.strip_suffix('\'')?;
    Some(quoted.replace("'\\''", "'").into())
}

/// A monitor on `sock` that answers each connection as `peer` says; it
/// keeps the name of each request, and each URI of a `migrate`.
fn monitor(sock: &Path, peer: Peer) -> (Log, Log) {
    let _ = std::fs::remove_file(sock);
    let l = UnixListener::bind(sock).expect("the QMP socket");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let uris = Arc::new(Mutex::new(Vec::new()));
    let (log, ulog) = (seen.clone(), uris.clone());
    std::thread::spawn(move || {
        for conn in l.incoming() {
            let Ok(conn) = conn else { return };
            let mut w = conn.try_clone().expect("a second handle");
            let _ = w.write_all(b"{\"QMP\": {\"version\": {}}}\n");
            let mut status = "none";
            for line in BufReader::new(conn).lines() {
                let Ok(line) = line else { break };
                let verb = qmp::json_member(&line, "execute").unwrap_or_default();
                log.lock().unwrap_or_else(|e| e.into_inner()).push(verb.clone());
                let mut answer = "{\"return\": {}}".to_string();
                match verb.as_str() {
                    "migrate" => {
                        if peer == Peer::Leaves {
                            return;
                        }
                        let uri = qmp::json_member(&line, "uri").unwrap_or_default();
                        ulog.lock().unwrap_or_else(|e| e.into_inner()).push(uri.clone());
                        let path = stream_of(&uri).expect("a URI of a stream");
                        if peer == Peer::NoFile && uri.starts_with("file:") {
                            answer =
                                "{\"error\": {\"class\": \"GenericError\", \"desc\": \"unknown migration protocol\"}}"
                                    .into();
                        } else if peer == Peer::Fails {
                            std::fs::write(&path, b"half").unwrap();
                            status = "failed";
                        } else {
                            let bytes: &[u8] = if peer == Peer::Empty { b"" } else { b"STATE" };
                            std::fs::write(&path, bytes).unwrap();
                            status = "completed";
                        }
                    }
                    "query-migrate" if peer == Peer::Blocked => {
                        answer = "{\"return\": {\"blocked-reasons\": [\"non-migratable device: virtio-9p\"]}}".into();
                    }
                    "query-migrate" => {
                        answer = format!(
                            "{{\"return\": {{\"status\": \"{status}\", \"error-desc\": \"the disk is full\"}}}}"
                        );
                    }
                    "cont" if peer == Peer::NoCont => {
                        answer = "{\"error\": {\"class\": \"GenericError\", \"desc\": \"no\"}}".into();
                    }
                    _ => {}
                }
                if w.write_all(format!("{answer}\n").as_bytes()).is_err() {
                    break;
                }
            }
        }
    });
    (seen, uris)
}

fn names(seen: &Log) -> Vec<String> {
    seen.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// A fingerprint of a line of a direct kernel boot.
fn print(kernel: &str) -> String {
    Inputs::new()
        .with("qemu", "QEMU emulator version 11.0.0")
        .with("cpu", "tcg")
        .with("image", "sha256:aa")
        .with("kernel", kernel)
        .with("mem", "3072")
        .machine(&["-kernel".into(), "KERNEL".into(), "-virtfs".into(), "local,path=SLOT0".into()])
        .text()
}

fn notes() -> Vec<(String, String)> {
    vec![("mem-mib".into(), "3072".into())]
}

fn save_in(d: &Path, peer: Peer, transport: Transport, fallback: bool, fingerprint: &str) -> (SaveEnd, Vec<String>) {
    let sock = d.join("qmp");
    let (seen, _) = monitor(&sock, peer);
    let n = notes();
    let s = Save {
        monitor: &sock,
        stream: &d.join("state.vm"),
        record: &d.join("state.rec"),
        fingerprint,
        notes: &n,
        transport,
        fallback,
        unique: "u1",
        wait: Duration::from_secs(30),
    };
    let end = save(&s, &mut ());
    (end, names(&seen))
}

/// The save of a line: the guest stops before the stream and goes on
/// after it; the state is whole with its record, or there is none and no
/// partial file. A QEMU with no `file:` saves through the pipe where the
/// caller asks for the fallback. A device that cannot migrate stops nothing.
#[test]
fn a_save_of_a_line_leaves_a_whole_state_or_nothing() {
    pod_testkit::need_loopback!();
    let cases = [
        (Peer::Works, Transport::File, false, Ok(Transport::File), true),
        (Peer::Works, Transport::Pipe, false, Ok(Transport::Pipe), true),
        (Peer::NoFile, Transport::File, true, Ok(Transport::Pipe), true),
        (Peer::NoFile, Transport::File, false, Err((Class::Protocol, "unknown migration protocol")), true),
        (Peer::Fails, Transport::File, true, Err((Class::Protocol, "the disk is full; through a pipe")), true),
        (Peer::Empty, Transport::File, false, Err((Class::Protocol, "wrote no byte")), true),
        (Peer::Leaves, Transport::File, true, Err((Class::Unavailable, "qmp")), false),
        (Peer::NoCont, Transport::File, false, Ok(Transport::File), false),
        (Peer::Blocked, Transport::File, false, Err((Class::Unavailable, "virtio-9p")), true),
    ];
    for (peer, first, fallback, want, runs) in cases {
        let d = dir("line-save");
        // A save replaces the record of the state before, or keeps it.
        std::fs::write(d.join("state.rec"), b"the record before").unwrap();
        let p = print("vmlinuz 1");
        let (end, seen) = save_in(&d, peer, first, fallback, &p);
        let case = format!("{peer:?} {first:?} {fallback}: {:?} {seen:?}", end.saved);
        assert_eq!(end.goes_on.is_ok(), runs, "{case}");
        match (&end.saved, want) {
            (Ok(s), Ok(t)) => {
                assert_eq!((s.bytes, s.transport), (5, t), "{case}");
                let r = check(&d.join("state.vm"), &d.join("state.rec"), &p).expect("the record of the new state");
                assert_eq!((r.transport, r.note("mem-mib")), (t, Some("3072")), "{case}");
            }
            (Err(e), Err((class, text))) => {
                assert_eq!(e.class, class, "{case}");
                assert!(e.to_string().contains(text), "{case}");
                let before = std::fs::read(d.join("state.rec")).unwrap();
                assert_eq!(before, b"the record before", "{case}: a failed save keeps the state before");
            }
            _ => panic!("{case}"),
        }
        if peer == Peer::Blocked {
            assert!(!seen.iter().any(|v| v == "stop"), "{case}");
        } else if peer != Peer::Leaves {
            let at = |v: &str| seen.iter().position(|x| x == v);
            assert!(at("stop") < at("migrate"), "the guest stops before the stream: {case}");
            assert_eq!(seen.last().map(String::as_str), Some("cont"), "{case}");
        }
        let partial: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".part-"))
            .collect();
        assert!(partial.is_empty(), "{case}: a partial file stays: {partial:?}");
        let _ = std::fs::remove_dir_all(&d);
    }
}

/// The steps of a restore of a saved line: the record of the save loads
/// with the same fingerprint; another kernel is a stale state with the
/// first line that differs; the restore line ends with the stream of the
/// transport of the record; the wait gives `cont` to the paused guest.
#[test]
fn a_saved_line_restores_and_a_changed_fingerprint_is_refused() {
    pod_testkit::need_loopback!();
    let d = dir("line-restore");
    let p = print("vmlinuz 1");
    let (end, _) = save_in(&d, Peer::Works, Transport::File, false, &p);
    end.saved.expect("the save");
    let (stream, record) = (d.join("state.vm"), d.join("state.rec"));
    let r = check(&stream, &record, &p).expect("the state of this fingerprint");

    let e = check(&stream, &record, &print("vmlinuz 2")).unwrap_err();
    assert_eq!(e.class, Class::Config, "{e}");
    let stale = Cold::Stale { saved: "kernel \"vmlinuz 1\"".into(), now: "kernel \"vmlinuz 2\"".into() };
    assert_eq!(warm::cold(&e), Some(&stale));

    let line = vec!["-kernel".to_string(), "/s/vmlinuz".to_string()];
    let a = incoming(&line, r.transport, &stream).unwrap();
    assert_eq!(a[..2], line[..]);
    assert_eq!(a[2..], ["-incoming".to_string(), format!("file:{}", stream.display())]);

    let sock = d.join("load");
    let _ = std::fs::remove_file(&sock);
    let l = UnixListener::bind(&sock).unwrap();
    let server = std::thread::spawn(move || {
        let (s, _) = l.accept().unwrap();
        let mut w = s.try_clone().unwrap();
        w.write_all(b"{\"QMP\": {}}\n").unwrap();
        let mut statuses = ["inmigrate", "paused"].into_iter();
        let mut seen = Vec::new();
        for line in BufReader::new(s).lines().map_while(std::result::Result::ok) {
            let verb = qmp::json_member(&line, "execute").unwrap_or_default();
            let answer = match verb.as_str() {
                "query-status" => {
                    format!("{{\"return\": {{\"status\": \"{}\"}}}}", statuses.next().unwrap_or("running"))
                }
                _ => "{\"return\": {}}".to_string(),
            };
            seen.push(verb);
            if w.write_all(format!("{answer}\n").as_bytes()).is_err() {
                break;
            }
        }
        seen
    });
    running(&sock, None, Duration::from_secs(30)).expect("the load ends");
    // The stand-in ends when the wait closes its connection.
    let seen = server.join().unwrap();
    assert_eq!(seen, ["qmp_capabilities", "query-status", "query-status", "cont"]);
    let _ = std::fs::remove_dir_all(&d);
}

/// The wait of a load with no child: another status is a failed load, and
/// a monitor that never answers is a timeout that names its last error.
#[test]
fn the_wait_for_a_loaded_line_refuses_another_status_and_ends_at_its_bound() {
    pod_testkit::need_loopback!();
    let d = dir("line-running");
    let sock = d.join("qmp");
    let l = UnixListener::bind(&sock).unwrap();
    std::thread::spawn(move || {
        let (s, _) = l.accept().unwrap();
        let mut w = s.try_clone().unwrap();
        let _ = w.write_all(b"{\"QMP\": {}}\n");
        for line in BufReader::new(s).lines().map_while(std::result::Result::ok) {
            let answer = if line.contains("query-status") {
                "{\"return\": {\"status\": \"internal-error\"}}"
            } else {
                "{\"return\": {}}"
            };
            if w.write_all(format!("{answer}\n").as_bytes()).is_err() {
                break;
            }
        }
    });
    let e = running(&sock, None, Duration::from_secs(30)).unwrap_err();
    assert_eq!(e.class, Class::Protocol, "{e}");
    assert!(e.to_string().contains("internal-error"), "{e}");
    let e = running(&d.join("none"), None, Duration::from_millis(200)).unwrap_err();
    assert_eq!(e.class, Class::Timeout, "{e}");
    assert!(e.to_string().contains("the last error"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

/// A state with no record, no stream, a stream of another length, a
/// record that does not parse or a record of another layout does not load.
#[test]
fn only_a_whole_state_of_this_layout_loads() {
    let d = dir("line-check");
    let (stream, record) = (d.join("s.vm"), d.join("s.rec"));
    let p = print("k");
    let r = Record { transport: Transport::Pipe, bytes: 5, notes: notes(), fingerprint: p.clone() };
    let missing = |e: Failure| assert_eq!(warm::cold(&e), Some(&Cold::Missing), "{e}");
    missing(check(&stream, &record, &p).unwrap_err());
    std::fs::write(&record, r.text()).unwrap();
    missing(check(&stream, &record, &p).unwrap_err());
    std::fs::write(&stream, b"STAT").unwrap();
    missing(check(&stream, &record, &p).unwrap_err());
    std::fs::write(&stream, b"STATE").unwrap();
    assert_eq!(check(&stream, &record, &p).unwrap(), r);
    std::fs::write(&record, format!("{RECORD}\nbytes x\nfingerprint\n{p}")).unwrap();
    missing(check(&stream, &record, &p).unwrap_err());
    std::fs::write(&record, "podvm-machine-state.v1\n").unwrap();
    let e = check(&stream, &record, &p).unwrap_err();
    let old = Cold::Stale { saved: "podvm-machine-state.v1".into(), now: RECORD.into() };
    assert_eq!(warm::cold(&e), Some(&old));
    let _ = std::fs::remove_dir_all(&d);
}

/// The record holds each note and the fingerprint as it is; a note that
/// the record cannot hold, or a bad name of the partial files, is refused
/// before the emulator gets a request.
#[test]
fn the_record_holds_each_note_and_refuses_a_note_it_cannot_hold() {
    let r = Record {
        transport: Transport::File,
        bytes: 7,
        notes: vec![("mem-mib".into(), "3072".into()), ("cmdline".into(), "console=ttyS0 quiet".into())],
        fingerprint: "pod-guest warm 1\nkernel \"a b\"\n".into(),
    };
    assert_eq!(Record::parse(&r.text()), Ok(r.clone()));
    assert_eq!(r.note("cmdline"), Some("console=ttyS0 quiet"));
    assert_eq!(r.note("none"), None);
    let d = dir("line-notes");
    let sock = d.join("no-monitor");
    for (notes, unique) in [
        (vec![("two words".to_string(), "v".to_string())], "u"),
        (vec![("k".to_string(), "a\nb".to_string())], "u"),
        (Vec::new(), "a/b"),
        (Vec::new(), ""),
    ] {
        let s = Save {
            monitor: &sock,
            stream: &d.join("s"),
            record: &d.join("r"),
            fingerprint: "f",
            notes: &notes,
            transport: Transport::File,
            fallback: false,
            unique,
            wait: Duration::from_secs(1),
        };
        let end = save(&s, &mut ());
        assert_eq!(end.saved.unwrap_err().class, Class::Usage, "{notes:?} {unique:?}");
        assert!(end.goes_on.is_ok());
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// A path goes into a URI whole: a quote of a pipe stays one word of the
/// shell, and a comma of a file path is refused.
#[test]
fn a_path_goes_into_a_uri_whole() {
    let odd = Path::new("/s/it's a path");
    assert_eq!(Transport::File.save_uri(odd).unwrap(), "file:/s/it's a path");
    assert_eq!(Transport::File.load_uri(odd).unwrap(), "file:/s/it's a path");
    assert_eq!(Transport::Pipe.save_uri(odd).unwrap(), "exec:cat > '/s/it'\\''s a path'");
    assert_eq!(Transport::Pipe.load_uri(odd).unwrap(), "exec:cat '/s/it'\\''s a path'");
    assert_eq!(stream_of(&Transport::Pipe.save_uri(odd).unwrap()).as_deref(), Some(odd));
    let comma = Path::new("/s/a,b");
    assert_eq!(Transport::File.save_uri(comma).unwrap_err().class, Class::Usage);
    assert_eq!(Transport::Pipe.save_uri(comma).unwrap(), "exec:cat > '/s/a,b'");
    for t in [Transport::File, Transport::Pipe] {
        assert_eq!(Transport::of(t.word()), Some(t));
    }
    assert_eq!(Transport::of("tcp"), None);
}

/// The inputs of a line: one line for each, a value as one JSON string; a
/// file is its path, its length and its time; a digest is of its bytes.
#[test]
fn the_inputs_of_a_line_name_each_input_and_each_change_of_a_file() {
    let d = dir("line-inputs");
    let kernel = d.join("vmlinuz");
    std::fs::write(&kernel, b"code").unwrap();
    let a = Inputs::new().with("image", "sha256:aa\nmachine \"-x\"").file("kernel", &kernel).unwrap();
    let text = a.text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], warm::FORMAT);
    assert_eq!(lines[1], "image \"sha256:aa\\u000amachine \\\"-x\\\"\"");
    assert!(lines[2].starts_with(&format!("kernel \"{} 4 ", kernel.display())), "{}", lines[2]);
    assert_eq!(lines.len(), 3);
    std::fs::write(&kernel, b"new code").unwrap();
    let b = Inputs::new().with("image", "sha256:aa\nmachine \"-x\"").file("kernel", &kernel).unwrap();
    assert_ne!(a.text(), b.text(), "a new file under the same name");
    let digest = Inputs::new().digest("podvm", &kernel).unwrap();
    let want = "sha256:d7bc32e5f6760a997b53461d8102cbe8fa70bd84c550f989d54807ab1f7294a4";
    assert_eq!(digest.entries(), [("podvm".to_string(), want.to_string())], "the sha256 of `new code`");
    assert!(Inputs::new().digest("x", &d.join("none")).is_err());
    assert_eq!(cpu(Accel::Tcg).unwrap(), "tcg");
    let _ = std::fs::remove_dir_all(&d);
}

/// The fingerprint of a plan is the inputs of its line form.
#[test]
fn the_fingerprint_of_a_plan_is_the_text_of_its_inputs() {
    let f = warm::Fingerprint {
        qemu: "QEMU emulator version 11.0.0".into(),
        cpu: "tcg".into(),
        firmware: "f".into(),
        base: "sha256:b".into(),
        agent: "a".into(),
        machine: vec!["-M".into(), "q35".into()],
    };
    let want = "pod-guest warm 1\nqemu \"QEMU emulator version 11.0.0\"\ncpu \"tcg\"\nfirmware \"f\"\nbase \"sha256:b\"\n\
                agent \"a\"\nmachine \"-M\"\nmachine \"q35\"\n";
    assert_eq!(f.text(), want);
    assert_eq!(f.inputs().entries().len(), 7);
}
