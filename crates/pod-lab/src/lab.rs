//! A run of a command in a lab.
//!
//! [`run`] starts this binary again with the lab flag of the [`Settings`]:
//! a fresh process with one thread, the lab process. The lab process sets
//! each part that it can hold ([`crate::hold`]), then measures the profile
//! inside itself ([`crate::inside`]), each row in its own probe child. It
//! tells each verdict to [`run`] on a pipe. Only when each part holds (or
//! the caller asked for the parts that hold) does it replace itself with
//! the command; else it exits and the command never runs. The command
//! keeps the standard streams of the caller, and its exit status is the one
//! that [`run`] gives.

use std::ffi::OsString;
use std::io::Read;
use std::os::fd::FromRawFd;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};

use pod_hostprobe::sys as k;

use crate::check::{PartVerdict, Seen, State, Verdict, check_with};
use crate::net::{CERT_VARIABLE, Local, PROXY_VARIABLES, Ready, Started, WayOut};
use crate::profile::{Hold, LabFact, Profile};
use crate::{Settings, hold, inside, mounts, table};

/// The choices of a run.
#[derive(Debug, Default)]
pub struct Options {
    /// Run the command in the parts that hold, and report the others. With
    /// `false`, a part that does not hold stops the run.
    pub partial: bool,
    /// The standard output of the command; `None`: the one of the caller.
    pub stdout: Option<Stdio>,
    /// The TCP ports that a connect may reach where a part refuses each
    /// TCP connect (`bind-denied`): the proxy of a host whose only way
    /// out is that proxy.
    pub allow_connect: Vec<u16>,
    /// A way out to the real network through the stand-in proxy of a
    /// network lab: a `CONNECT` that the rules of the class pass, to a name
    /// that is not of the lab, goes out directly or through the proxy of
    /// the host (its variable and its credential, which the command never
    /// gets). Only a profile with a stand-in proxy takes it.
    pub way_out: bool,
    /// The ports where the command serves on `127.0.0.1` of a network lab.
    /// After the rules of its class, the stand-in proxy sends a `CONNECT` to
    /// a name of the lab at such a port to that server; a class with a list
    /// of names sends a ClientHello with a listed name to the first port.
    /// Not 0, 443 or 3128.
    pub serve: Vec<u16>,
    /// A PEM file of the roots that check a TLS server of the command,
    /// where the proxy of the class terminates TLS for the name. With no
    /// file, such a `CONNECT` gets 502.
    pub serve_ca: Option<PathBuf>,
}

/// The run of a command in a lab.
#[derive(Debug)]
pub struct Ran {
    /// The verdict of the lab from inside, before the command.
    pub verdict: Verdict,
    /// The exit status of the command.
    pub status: ExitStatus,
}

/// Why the command did not run.
#[derive(Debug)]
pub enum RunError {
    /// A part does not hold, and the caller did not ask for a partial lab.
    Refused(Verdict),
    /// The command did not start in the lab: its errno.
    Exec { verdict: Verdict, errno: i32 },
    /// The lab process did not start, or did not speak its verdict.
    Lab(String),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Refused(v) => {
                let parts: Vec<&str> =
                    v.parts.iter().filter(|p| p.state != State::Holds).map(|p| p.part.as_str()).collect();
                write!(f, "the lab {} does not hold: {}", v.profile, parts.join(", "))
            }
            RunError::Exec { errno, .. } => {
                write!(f, "the command did not start: {} ({errno})", k::Errno(*errno).name())
            }
            RunError::Lab(why) => write!(f, "the lab did not start: {why}"),
        }
    }
}

impl std::error::Error for RunError {}

/// Run `cmd` in a lab of `profile`. `profile` must be in
/// [`Settings::profiles`]: the lab process finds it there by its name.
pub fn run(settings: &Settings, profile: &Profile, cmd: &[OsString], opts: Options) -> Result<Ran, RunError> {
    if cmd.is_empty() {
        return Err(RunError::Lab("no command".to_string()));
    }
    if table::find(settings.profiles, profile.name).is_none() {
        return Err(RunError::Lab(format!("the profile {} is not in the profiles of the settings", profile.name)));
    }
    if opts.way_out && !has_proxy(profile) {
        return Err(RunError::Lab(format!("the profile {} has no stand-in proxy for a way out", profile.name)));
    }
    if !opts.serve.is_empty() && !profile.parts.iter().any(|p| matches!(p.hold, Hold::LabNet(_))) {
        return Err(RunError::Lab(format!("the profile {} has no network lab for a served port", profile.name)));
    }
    Local::new(opts.serve.clone(), opts.serve_ca.as_deref()).map_err(RunError::Lab)?;
    let mut fds = [0i32; 2];
    k::pipe2(&mut fds, k::O_CLOEXEC).map_err(|e| RunError::Lab(format!("pipe2: {}", e.name())))?;
    let (r, w) = (fds[0], fds[1]);
    // SAFETY: `r` is a new descriptor that nothing else owns.
    let mut reader = unsafe { std::fs::File::from_raw_fd(r) };
    let exe = if std::path::Path::new("/proc/self/exe").exists() {
        std::path::PathBuf::from("/proc/self/exe")
    } else {
        std::env::current_exe().map_err(|e| RunError::Lab(format!("the path of this binary: {e}")))?
    };
    let mut c = Command::new(exe);
    c.arg0(&settings.hostprobe.child_name)
        .arg(&settings.lab_flag)
        .arg(profile.name)
        .arg(if opts.partial { "partial" } else { "whole" })
        .arg(opts.allow_connect.iter().map(u16::to_string).collect::<Vec<_>>().join(","))
        .arg(if opts.way_out { "way-out" } else { "closed" })
        .arg(opts.serve.iter().map(u16::to_string).collect::<Vec<_>>().join(","))
        .arg(opts.serve_ca.map(PathBuf::into_os_string).unwrap_or_default())
        .arg(w.to_string())
        .arg("--")
        .args(cmd);
    if let Some(out) = opts.stdout {
        c.stdout(out);
    }
    // SAFETY: one fcntl(2) in the child before the exec; no allocation.
    unsafe {
        c.pre_exec(move || {
            k::fcntl(i64::from(w), k::F_SETFD, 0).map(|_| ()).map_err(|e| std::io::Error::from_raw_os_error(e.0))
        });
    }
    let spawned = c.spawn();
    let _ = k::close(i64::from(w));
    let mut child = spawned.map_err(|e| RunError::Lab(format!("the start of the lab process: {e}")))?;
    let mut text = String::new();
    let read = reader.read_to_string(&mut text);
    drop(reader);
    let status = child.wait().map_err(|e| RunError::Lab(format!("the wait for the lab: {e}")))?;
    read.map_err(|e| RunError::Lab(format!("the verdict of the lab: {e}")))?;
    let (verdict, end) = decode(profile, &text).map_err(RunError::Lab)?;
    match end {
        End::Go => Ok(Ran { verdict, status }),
        End::Refuse => Err(RunError::Refused(verdict)),
        End::ExecFailed(errno) => Err(RunError::Exec { verdict, errno }),
    }
}

/// The last line of the lab process.
enum End {
    Go,
    Refuse,
    ExecFailed(i32),
}

fn clean(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

fn encode(v: &Verdict) -> String {
    let mut out = String::new();
    for p in &v.parts {
        out.push_str(&format!("part\t{}\t{}\n", clean(&p.part), p.state.word()));
        for s in &p.seen {
            let errno = s.errno.map_or("-".to_string(), |e| e.to_string());
            out.push_str(&format!("seen\t{}\t{}\t{errno}\t{}\n", s.state.word(), clean(&s.what), clean(&s.detail)));
        }
    }
    out
}

fn decode(profile: &Profile, text: &str) -> Result<(Verdict, End), String> {
    let mut parts: Vec<PartVerdict> = Vec::new();
    let mut end = None;
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.as_slice() {
            ["part", name, state] => parts.push(PartVerdict {
                part: name.to_string(),
                state: State::from_word(state).ok_or_else(|| format!("the state {state:?}"))?,
                seen: Vec::new(),
            }),
            ["seen", state, what, errno, detail] => {
                let last = parts.last_mut().ok_or("a seen line before its part")?;
                last.seen.push(Seen {
                    what: what.to_string(),
                    state: State::from_word(state).ok_or_else(|| format!("the state {state:?}"))?,
                    errno: errno.parse().ok(),
                    detail: detail.to_string(),
                });
            }
            ["go"] => end = Some(End::Go),
            ["refuse"] => end = Some(End::Refuse),
            ["exec-failed", errno] => end = Some(End::ExecFailed(errno.parse().map_err(|_| "the errno of the exec")?)),
            _ => return Err(format!("the lab said {line:?}")),
        }
    }
    let end = end.ok_or_else(|| "the lab ended with no verdict".to_string())?;
    Ok((Verdict { profile: profile.name.to_string(), parts }, end))
}

/// True where a part of `profile` is a network lab with a stand-in proxy.
pub fn has_proxy(profile: &Profile) -> bool {
    profile.parts.iter().any(|p| matches!(p.hold, Hold::LabNet(n) if n.proxy.is_some()))
}

/// The work of the lab process: `args` are the arguments after the lab
/// flag (the profile, `whole` or `partial`, the ports of a connect that
/// stays allowed, `way-out` or `closed`, the served ports, the file of
/// their roots or an empty argument, the descriptor of the verdict, `--`
/// and the command). It does not return.
pub fn lab_main(settings: &Settings, args: &[OsString]) -> ! {
    let text = |i: usize| args.get(i).and_then(|a| a.to_str()).unwrap_or("");
    // A lab process with no verdict descriptor cannot speak: the caller
    // reads "no verdict". The words stay here for a reader of the code.
    let fail = |_why: &str| -> ! { std::process::exit(crate::codes::EXIT_REFUSED) };
    let Some(profile) = table::find(settings.profiles, text(0)) else { fail("no such profile") };
    let partial = match text(1) {
        "partial" => true,
        "whole" => false,
        _ => fail("no word of a partial lab"),
    };
    let ports = |i: usize| text(i).split(',').filter(|p| !p.is_empty()).map(str::parse::<u16>).collect();
    let Ok(connect): Result<Vec<u16>, _> = ports(2) else { fail("no list of ports") };
    let way_out = match text(3) {
        "way-out" => true,
        "closed" => false,
        _ => fail("no word of a way out"),
    };
    let Ok(serve): Result<Vec<u16>, _> = ports(4) else { fail("no list of served ports") };
    let serve_ca = args.get(5).filter(|a| !a.is_empty()).map(PathBuf::from);
    let Ok(given) = text(6).parse::<i64>() else { fail("no descriptor of the verdict") };
    if text(7) != "--" || args.len() < 9 {
        fail("no command");
    }
    // The verdict goes on a descriptor above each one that a part opens,
    // close-on-exec, so that the command never holds it.
    let Ok(out) = k::fcntl(given, k::F_DUPFD_CLOEXEC, 100) else { fail("the descriptor of the verdict") };
    let _ = k::close(given);
    hold::close_from_3(out);
    let say = |line: &str| {
        let _ = k::write(out, line.as_bytes());
    };

    // The way out starts before the namespaces of the lab, in the network
    // of the host, and reads the variables of the host before the lab sets
    // its own.
    let out = match way_out {
        false => Ok(None),
        true => {
            let reterminate = profile.parts.iter().any(|p| matches!(p.hold, Hold::LabNet(n) if n.proxy_ca()));
            crate::net::start_way_out(reterminate).map(Some).map_err(|e| format!("the way out: {e}"))
        }
    };
    // The roots of the served ports: a file of the host, read before the
    // mounts of the lab.
    let served = Local::new(serve, serve_ca.as_deref()).map_err(|e| format!("the served ports: {e}"));
    let ends = out.and_then(|out| served.map(|served| (out, served)));
    let (set, net) = setup(profile, &connect, ends);
    if let Some(lab) = &net {
        environment(lab);
    }
    // The rows that send a packet out reach the stand-ins of the lab.
    let mut probe = settings.hostprobe.clone();
    if let Some(lab) = &net {
        probe.network = Some(crate::net::lab_targets(lab.net));
    }
    let findings = inside::measure(&probe, &inside::rows(profile));
    let mut verdict = check_with(profile, &findings, &|fact| match fact {
        LabFact::Net(f) => crate::net::measure(f, net.as_ref()),
        f => inside::lab_fact(f),
    });
    for (part, why) in verdict.parts.iter_mut().zip(&set) {
        if let Some(why) = why {
            part.state = State::Fails;
            part.seen.insert(
                0,
                Seen { what: "the hold".to_string(), state: State::Fails, errno: None, detail: why.clone() },
            );
        }
    }
    say(&encode(&verdict));
    if !partial && !verdict.holds() {
        say("refuse\n");
        std::process::exit(crate::codes::EXIT_REFUSED);
    }
    say("go\n");
    let mut c = Command::new(&args[8]);
    c.args(&args[9..]);
    if profile.parts.iter().any(|p| matches!(p.hold, Hold::Tmpfs { .. })) {
        c.env("TMPDIR", "/tmp");
    }
    let e = c.exec();
    let errno = e.raw_os_error().unwrap_or(0);
    say(&format!("exec-failed\t{errno}\n"));
    std::process::exit(if errno == k::ENOENT.0 { crate::codes::EXIT_NOT_FOUND } else { crate::codes::EXIT_CANNOT_EXEC })
}

/// The environment of the lab process in a network lab, before it
/// measures the profile: no proxy of the caller, the stand-in proxy where
/// the class has one, and the certificate file of the lab. The rows of the
/// profile read it, and the command inherits it.
fn environment(lab: &Started) {
    // SAFETY: the lab process has one thread; the stand-ins are another
    // process.
    unsafe {
        for v in PROXY_VARIABLES {
            std::env::remove_var(v);
        }
        let current: Vec<(String, String)> = std::env::vars().collect();
        let current = current.iter().map(|(k, v)| (k.as_str(), v.as_str()));
        for change in crate::net::vars::changes(current, lab.net.proxy.as_ref()) {
            match change {
                crate::net::vars::Change::Remove(k) => std::env::remove_var(k),
                crate::net::vars::Change::Set(k, v) => std::env::set_var(k, v),
            }
        }
        if let Some(p) = &lab.net.proxy {
            let url = crate::net::proxy_url(p);
            for v in &PROXY_VARIABLES[..4] {
                std::env::set_var(v, &url);
            }
        }
        std::env::set_var(CERT_VARIABLE, &lab.cert_file);
    }
}

/// Set each part, in the order: the mount namespace (and the stand-ins of
/// a network lab), the descriptors and the session, Landlock and
/// `no_new_privs`, the drop of the capabilities, then the one seccomp
/// filter last, so that the filter does not refuse a call of the setup.
/// One entry for each part: `Some(why)` where the part could not be set;
/// and the network lab that runs.
fn setup(
    profile: &Profile,
    connect: &[u16],
    ends: Result<(Option<WayOut>, Local), String>,
) -> (Vec<Option<String>>, Option<Started>) {
    let mut why: Vec<Option<String>> = profile.parts.iter().map(|p| hold::cannot(p.hold)).collect();
    let mut net = None;
    // Each mount in one namespace.
    let at: Vec<usize> =
        (0..profile.parts.len()).filter(|&i| why[i].is_none() && mounts::is_mount(profile.parts[i].hold)).collect();
    if !at.is_empty() {
        let holds: Vec<Hold> = at.iter().map(|&i| profile.parts[i].hold).collect();
        match mounts::holds(&holds) {
            Ok(made) => {
                for (&i, r) in at.iter().zip(made.each) {
                    why[i] = r.err();
                }
                net = start(profile, made.net, ends, &mut why);
            }
            Err(e) => at.iter().for_each(|&i| why[i] = Some(e.clone())),
        }
    }
    let mut each = |rank: u8, f: &dyn Fn(Hold) -> Option<Result<(), String>>| {
        for (i, p) in profile.parts.iter().enumerate() {
            if why[i].is_some() || order(p.hold) != rank {
                continue;
            }
            if let Some(Err(e)) = f(p.hold) {
                why[i] = Some(e);
            }
        }
    };
    each(1, &|h| match h {
        Hold::Fds { last } => Some(hold::fds(last)),
        Hold::NoCtty => Some(hold::no_ctty()),
        _ => None,
    });
    each(2, &|h| match h {
        Hold::LandlockTcp => Some(hold::landlock_tcp(connect)),
        Hold::LandlockBind => Some(hold::landlock_bind()),
        Hold::Hide { path, rights } => Some(hold::hide(path, rights)),
        Hold::NoNewPrivs => Some(k::set_no_new_privs().map(|_| ()).map_err(|e| format!("prctl: {}", e.name()))),
        _ => None,
    });
    // After Landlock: a ruleset of a hidden path reads each directory on the
    // way, which root does with its capabilities.
    each(3, &|h| match h {
        Hold::DropCaps => Some(hold::drop_caps()),
        _ => None,
    });
    // One filter for each rule of each part that is still set.
    let rules: Vec<_> = profile
        .parts
        .iter()
        .zip(&why)
        .filter(|(_, w)| w.is_none())
        .flat_map(|(p, _)| hold::deny_rules(p.hold))
        .collect();
    if !rules.is_empty()
        && let Err(e) = hold::seccomp(&rules)
    {
        for (i, p) in profile.parts.iter().enumerate() {
            if order(p.hold) == 4 && why[i].is_none() {
                why[i] = Some(e.clone());
            }
        }
    }
    (why, net)
}

/// Start the stand-ins of the network lab of `profile` on the sockets of
/// `ready`, with the way out and the served ports of `ends`. Where they do
/// not start, or the way out did not, each part of the network lab gets the
/// reason.
fn start(
    profile: &Profile,
    ready: Option<Ready>,
    ends: Result<(Option<WayOut>, Local), String>,
    why: &mut [Option<String>],
) -> Option<Started> {
    let (ready, net) =
        (ready?, profile.parts.iter().find_map(|p| if let Hold::LabNet(n) = p.hold { Some(n) } else { None })?);
    match ends.and_then(|(out, served)| crate::net::start(ready, net, out, served)) {
        Ok(lab) => Some(lab),
        Err(e) => {
            for (i, p) in profile.parts.iter().enumerate() {
                if matches!(p.hold, Hold::LabNet(_)) && why[i].is_none() {
                    why[i] = Some(e.clone());
                }
            }
            None
        }
    }
}

/// The rank of a hold in [`setup`].
fn order(h: Hold) -> u8 {
    match h {
        _ if mounts::is_mount(h) => 0,
        Hold::Fds { .. } | Hold::NoCtty => 1,
        Hold::LandlockTcp | Hold::LandlockBind | Hold::Hide { .. } | Hold::NoNewPrivs => 2,
        Hold::DropCaps => 3,
        Hold::Seccomp(_) | Hold::ConfineDefaults => 4,
        _ => 5,
    }
}

/// The exit code of a status, as a shell gives it: the code, or 128 and
/// the number of the signal.
pub fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}
