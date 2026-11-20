//! The far end of git (T-973): `git http-backend` of the host serves a bare
//! repository with one commit, as a CGI program for each request, behind a
//! Basic challenge. Each answer names the `Authorization` it got in
//! `X-Seen`.

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

use crate::tls_common::Answer;

/// The files of the commit: a text, and a blob larger than
/// `tls_end::Bounds::max_body`, so that the pack comes to the job in
/// chunks, scrubbed as it comes.
pub const FILE: &str = "README";
pub const TEXT: &str = "the repository of the far end\n";
pub const BLOB: &str = "blob";

/// 1.5 MiB of bytes that do not compress (xorshift).
pub fn blob() -> Vec<u8> {
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    (0..3 << 19)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

/// Run `git` with `args` in `dir` and a clean environment; panic on a
/// failure.
fn git(program: &str, dir: &Path, args: &[&str]) {
    let out = Command::new(program)
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap_or_else(|e| panic!("{program}: {e}"));
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A bare repository `ROOT/owner/repo.git` with one commit on `main`.
pub fn repository(program: &str, root: &Path) {
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(work.join(FILE), TEXT).unwrap();
    std::fs::write(work.join(BLOB), blob()).unwrap();
    let bare = root.join("owner/repo.git");
    git(program, root, &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()]);
    git(program, &work, &["init", "-q", "-b", "main"]);
    git(program, &work, &["add", FILE, BLOB]);
    let who = ["-c", "user.name=far", "-c", "user.email=far@example.invalid"];
    git(program, &work, &[&who[..], &["commit", "-q", "-m", "one"]].concat());
    git(program, &work, &["push", "-q", bare.to_str().unwrap(), "main"]);
}

/// The value of the header `name` of `head`.
fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|line| {
        let (n, v) = line.split_once(": ")?;
        n.eq_ignore_ascii_case(name).then_some(v)
    })
}

/// The far end: 401 with a Basic challenge to a request with no
/// `Authorization`; else the answer of `git http-backend` for the
/// repositories under `root`.
pub fn backend(program: &str, root: &Path) -> Answer {
    let (program, root) = (program.to_string(), root.to_path_buf());
    Arc::new(move |head: &str, body: &[u8]| {
        let Some(auth) = header(head, "authorization") else {
            let challenge = "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"far\"\r\n";
            return format!("{challenge}Content-Length: 0\r\n\r\n").into_bytes();
        };
        let mut words = head.split(' ');
        let (method, target) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let mut command = Command::new(&program);
        command
            .arg("http-backend")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_PROJECT_ROOT", &root)
            .env("GIT_HTTP_EXPORT_ALL", "1")
            .env("REQUEST_METHOD", method)
            .env("PATH_INFO", path)
            .env("QUERY_STRING", query)
            .env("CONTENT_LENGTH", body.len().to_string())
            .env("REMOTE_ADDR", "192.0.2.1");
        for (name, var) in [
            ("content-type", "CONTENT_TYPE"),
            ("content-encoding", "HTTP_CONTENT_ENCODING"),
            ("git-protocol", "GIT_PROTOCOL"),
        ] {
            if let Some(value) = header(head, name) {
                command.env(var, value);
            }
        }
        let out = tokio::task::block_in_place(|| {
            let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
            child.stdin.take().unwrap().write_all(body).unwrap();
            child.wait_with_output().unwrap()
        });
        http_of_cgi(&out.stdout, auth)
    })
}

/// The answer of HTTP/1.1 for the output of a CGI program, with `X-Seen`
/// and the length of its body.
fn http_of_cgi(cgi: &[u8], auth: &str) -> Vec<u8> {
    let end = cgi.windows(4).position(|w| w == b"\r\n\r\n").map(|at| (at, at + 4));
    let end = end.or_else(|| cgi.windows(2).position(|w| w == b"\n\n").map(|at| (at, at + 2)));
    let Some((head_end, body_at)) = end else {
        return b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n".to_vec();
    };
    let mut status = "200 OK".to_string();
    let mut out = String::new();
    for line in String::from_utf8_lossy(&cgi[..head_end]).lines() {
        match line.split_once(": ") {
            Some((name, value)) if name.eq_ignore_ascii_case("status") => status = value.to_string(),
            Some(_) => out.push_str(&format!("{line}\r\n")),
            None => {}
        }
    }
    let body = &cgi[body_at..];
    let head = format!("HTTP/1.1 {status}\r\n{out}X-Seen: {auth}\r\nContent-Length: {}\r\n\r\n", body.len());
    let mut answer = head.into_bytes();
    answer.extend_from_slice(body);
    answer
}
