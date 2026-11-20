//! A vectored write through a [`Stream`] gives each buffer to the stream under
//! it in one call. rustls writes its records with one vectored write; a
//! stream that sends only the first buffer of each call stalled an upload of
//! podssh over TLS (`resume.cut.up.sftp`). The shell of a command keeps each
//! backslash of a Windows path.

use pod_proxy::Stream;
use tokio::io::{AsyncReadExt, AsyncWrite as _, AsyncWriteExt};

const PARTS: [&[u8]; 3] = [b"first ", b"second ", b"third"];

/// One vectored write of `PARTS`: the stream says that it writes vectored,
/// and the call takes each buffer. The far end then reads them in order.
async fn one_call_takes_each_buffer(stream: &mut Stream, far: &mut (impl AsyncReadExt + Unpin)) {
    let whole = PARTS.concat();
    let bufs: Vec<std::io::IoSlice<'_>> = PARTS.iter().map(|p| std::io::IoSlice::new(p)).collect();
    assert!(stream.is_write_vectored(), "{stream:?} does not say that it writes vectored");
    let n = stream.write_vectored(&bufs).await.expect("a vectored write");
    assert_eq!(n, whole.len(), "one vectored write of {stream:?} took only {n} bytes");
    let mut got = vec![0; whole.len()];
    far.read_exact(&mut got).await.expect("the bytes at the far end");
    assert_eq!(got, whole);
}

#[tokio::test]
async fn a_vectored_write_over_tcp_takes_each_buffer() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("its address");
    let (client, server) = tokio::join!(tokio::net::TcpStream::connect(addr), listener.accept());
    let mut stream = Stream::Tcp(client.expect("a connection"));
    let (mut far, _) = server.expect("an accept");
    one_call_takes_each_buffer(&mut stream, &mut far).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_vectored_write_over_a_unix_socket_takes_each_buffer() {
    let (near, mut far) = tokio::net::UnixStream::pair().expect("a socket pair");
    one_call_takes_each_buffer(&mut Stream::Unix(near), &mut far).await;
}

/// The command `cat` gives back what it reads: the far end is the stream.
#[cfg(unix)]
#[tokio::test]
async fn a_vectored_write_to_a_command_takes_each_buffer() {
    let child = pod_proxy::ChildIo::spawn("cat").expect("the command cat");
    let mut stream = Stream::Exec(Box::new(child));
    let whole = PARTS.concat();
    let bufs: Vec<std::io::IoSlice<'_>> = PARTS.iter().map(|p| std::io::IoSlice::new(p)).collect();
    assert!(stream.is_write_vectored());
    assert_eq!(stream.write_vectored(&bufs).await.expect("a vectored write"), whole.len());
    let mut got = vec![0; whole.len()];
    stream.read_exact(&mut got).await.expect("the bytes from cat");
    assert_eq!(got, whole);
}

/// The command runs in the shell of the system: `sh` on Unix, `cmd.exe` on
/// Windows, where `sh` removes each backslash of a path that has no quotes.
#[tokio::test]
async fn the_shell_of_a_command_keeps_each_backslash() {
    let path = r"C:\Users\RUNNER~1\AppData\Local\Temp\pipe.py";
    let command = if cfg!(windows) { format!("echo {path}") } else { format!("printf '%s\\n' '{path}'") };
    let mut child = pod_proxy::ChildIo::spawn(&command).expect("the command");
    let mut got = String::new();
    child.read_to_string(&mut got).await.expect("the output of the command");
    assert_eq!(got.trim_end(), path, "{command}");
}
