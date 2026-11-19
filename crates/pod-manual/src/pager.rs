//! The pager of `CLI man`. It is used only when stdin and stdout are both a
//! terminal (`--no-pager` turns it off), and its choice is probed on each
//! run:
//!
//! 1. `PAGER`, when it is set. Empty, or `cat`, means no pager.
//! 2. `less`, when a probe of `PATH` finds it and `TERM` names a terminal
//!    that is not `dumb`. Not on Windows, where a console seldom has `TERM`
//!    or `less`.
//! 3. The built-in pager: a screenful at a time, Enter for the next, `q` to
//!    stop. It is code of this crate, not a program: it needs nothing from
//!    the host.
//!
//! No other program is tried. A program that does not start falls back to
//! the built-in pager. A pager program gets the text on its stdin and
//! writes to the terminal itself; the built-in pager writes the text to
//! stdout and its prompt to stderr.

use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use crate::facts::{Use, Var, VarGroup};

/// A screenful when the terminal cannot be measured.
pub const PAGE_LINES: usize = 24;

/// The source of this module, for the check of the variables of a program
/// that pages its manual (`check::variables`).
pub const SOURCE: &str = include_str!("pager.rs");

/// The variables that the pager reads, as a group of facts that a program
/// adds to its own.
pub fn variables() -> VarGroup {
    let used_by = "man";
    let var = |names: &[&str], meaning: &str| Var { used_by: used_by.to_string(), ..Var::reads(names, meaning) };
    VarGroup {
        name: "The pager".to_string(),
        vars: vec![
            var(
                &["PAGER"],
                "The pager of the manual on a terminal. An empty value or cat writes the text with no pager.",
            ),
            var(&["TERM"], "With no PAGER, less is used only when TERM names a terminal that is not dumb."),
            var(&["PATH"], "The directories where the probe looks for less."),
            var(&["LINES"], "The lines of a screen of the built-in pager, when the terminal size is not known."),
            Var {
                usage: Use::ReadsAndSets,
                ..var(&["LESS"], "The options of less. When it is not set, the pager sets FRX.")
            },
        ],
    }
}

/// Whether stdin and stdout are terminals. `false` also means "could not be
/// asked", and both mean: do not page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tty {
    pub stdin: bool,
    pub stdout: bool,
}

impl Tty {
    /// Ask the operating system, once, where the real descriptors are.
    pub fn probed() -> Tty {
        use std::io::IsTerminal as _;
        Tty { stdin: std::io::stdin().is_terminal(), stdout: std::io::stdout().is_terminal() }
    }

    /// No terminal: what tests get, so that no test can wait for a key.
    pub fn none() -> Tty {
        Tty::default()
    }

    /// A pager needs both: output to show and a keyboard to read.
    pub fn can_page(&self) -> bool {
        self.stdin && self.stdout
    }
}

/// What the choice of a pager reads from the environment.
#[derive(Clone, Debug, Default)]
pub struct PagerEnv {
    pub pager: Option<String>,
    pub term: Option<String>,
    /// `less` on `PATH`, if there is one.
    pub less: Option<PathBuf>,
    pub windows: bool,
    /// The lines of the terminal, when they are known.
    pub rows: Option<usize>,
}

impl PagerEnv {
    pub fn from_process() -> Self {
        let windows = cfg!(windows);
        PagerEnv {
            pager: std::env::var("PAGER").ok(),
            term: std::env::var("TERM").ok(),
            less: if windows { None } else { find_program("less", std::env::var_os("PATH")) },
            windows,
            rows: std::env::var("LINES").ok().and_then(|v| v.trim().parse().ok()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Write the text, with no pager.
    Direct,
    /// Start this program with the text on its stdin.
    Program(Vec<OsString>),
    BuiltIn,
}

pub fn choose(env: &PagerEnv) -> Choice {
    if let Some(pager) = &env.pager {
        let words: Vec<&str> = pager.split_whitespace().collect();
        return match words.first() {
            None | Some(&"cat") => Choice::Direct,
            Some(_) => Choice::Program(words.into_iter().map(OsString::from).collect()),
        };
    }
    let term = env.term.as_deref().map(str::trim).unwrap_or("");
    match &env.less {
        Some(less) if !env.windows && !term.is_empty() && term != "dumb" => {
            Choice::Program(vec![less.clone().into_os_string()])
        }
        _ => Choice::BuiltIn,
    }
}

/// The first `name` in `path` that is a file this process may run.
pub fn find_program(name: &str, path: Option<OsString>) -> Option<PathBuf> {
    std::env::split_paths(&path?).map(|dir| dir.join(name)).find(|p| runnable(p))
}

fn runnable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// Write `text`: through the pager when `tty` can page and `no_pager` is
/// off, else at once. A reader that went away (`CLI man | head`) is not a
/// failure.
pub fn show(text: &str, no_pager: bool, tty: Tty, keys: &mut dyn BufRead, out: &mut dyn Write, err: &mut dyn Write) {
    if no_pager || !tty.can_page() {
        let _ = out.write_all(text.as_bytes());
        let _ = out.flush();
        return;
    }
    show_with(text, &PagerEnv::from_process(), keys, out, err);
}

/// Show `text` on the terminal, with the pager that [`choose`] selects.
pub fn show_with(text: &str, env: &PagerEnv, keys: &mut dyn BufRead, out: &mut dyn Write, err: &mut dyn Write) {
    let rows = env.rows.or_else(terminal_rows).filter(|r| *r > 2).unwrap_or(PAGE_LINES);
    match choose(env) {
        Choice::Direct => {
            let _ = out.write_all(text.as_bytes());
            let _ = out.flush();
        }
        Choice::Program(cmd) => {
            if run_program(&cmd, text).is_err() {
                let _ = page(text, rows - 1, keys, out, err);
            }
        }
        Choice::BuiltIn => {
            let _ = page(text, rows - 1, keys, out, err);
        }
    }
}

/// The lines of the terminal on stdout.
#[cfg(unix)]
fn terminal_rows() -> Option<usize> {
    // SAFETY: TIOCGWINSZ writes one winsize into the value that it gets,
    // and an all-zero winsize is a valid value of the C type.
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::ioctl(1, libc::TIOCGWINSZ as _, &mut size) } == 0;
    (ok && size.ws_row > 0).then_some(usize::from(size.ws_row))
}

#[cfg(not(unix))]
fn terminal_rows() -> Option<usize> {
    None
}

/// Start `cmd` with `text` on its stdin and wait for it. `Err` only when it
/// did not start. Ctrl-C belongs to the pager while it runs, so the program
/// ignores it and does not leave the pager behind on the terminal.
pub fn run_program(cmd: &[OsString], text: &str) -> std::io::Result<()> {
    let Some((program, args)) = cmd.split_first() else {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "no pager program"));
    };
    let mut command = std::process::Command::new(program);
    command.args(args).stdin(std::process::Stdio::piped());
    if std::env::var_os("LESS").is_none() {
        // Quit when the text fits one screen, pass control characters, and
        // leave the text on the screen at the end.
        command.env("LESS", "FRX");
    }
    let mut child = command.spawn()?;
    let _ignored = Interrupts::ignore();
    if let Some(mut stdin) = child.stdin.take() {
        // A pager that quits early closes its input: not an error.
        let _ = stdin.write_all(text.as_bytes());
    }
    let _ = child.wait();
    Ok(())
}

/// SIGINT and SIGQUIT ignored while the value lives, then restored: while a
/// pager or another child that reads the terminal (an editor) runs, the
/// keys are the child's. The whole action is kept and put back, its flags
/// too, so a handler that the program installed stays. Off Unix it does
/// nothing.
#[must_use = "the signals are restored when the value drops"]
pub struct Interrupts {
    #[cfg(unix)]
    previous: [libc::sigaction; 2],
}

impl Interrupts {
    /// Ignore SIGINT and SIGQUIT until the value drops.
    pub fn ignore() -> Interrupts {
        #[cfg(unix)]
        {
            // SAFETY: an all-zero sigaction is a valid value of the C type;
            // SIG_IGN with an empty mask has no preconditions, and Drop puts
            // back each action that was there.
            unsafe {
                let mut ignore: libc::sigaction = std::mem::zeroed();
                ignore.sa_sigaction = libc::SIG_IGN;
                libc::sigemptyset(&mut ignore.sa_mask);
                let mut previous: [libc::sigaction; 2] = std::mem::zeroed();
                libc::sigaction(libc::SIGINT, &ignore, &mut previous[0]);
                libc::sigaction(libc::SIGQUIT, &ignore, &mut previous[1]);
                Interrupts { previous }
            }
        }
        #[cfg(not(unix))]
        Interrupts {}
    }
}

impl Drop for Interrupts {
    fn drop(&mut self) {
        #[cfg(unix)]
        // SAFETY: restores the actions that `ignore` read.
        unsafe {
            libc::sigaction(libc::SIGINT, &self.previous[0], std::ptr::null_mut());
            libc::sigaction(libc::SIGQUIT, &self.previous[1], std::ptr::null_mut());
        }
    }
}

/// What the user typed at the prompt of the built-in pager.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// Enter, or a line that does not start with `q`: the next screenful.
    Next,
    /// A line that starts with `q` or `Q`.
    Quit,
    /// The input ended: stop, never wait.
    Eof,
}

/// Read one line of keys. A terminal sends input a line at a time, so the
/// prompt asks for Enter rather than for one key.
pub fn read_key(from: &mut dyn BufRead) -> Key {
    let mut line = Vec::new();
    match from.read_until(b'\n', &mut line) {
        Ok(0) | Err(_) => Key::Eof,
        Ok(_) => match line.iter().find(|b| !b.is_ascii_whitespace()) {
            Some(b'q' | b'Q') => Key::Quit,
            _ => Key::Next,
        },
    }
}

/// The built-in pager: `rows` lines at a time, then a prompt on `err`.
/// Paging to the end writes the same bytes as no pager.
pub fn page(
    text: &str,
    rows: usize,
    keys: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> std::io::Result<()> {
    let rows = rows.max(1);
    let lines: Vec<&str> = text.lines().collect();
    let mut start = 0;
    while start < lines.len() {
        let end = (start + rows).min(lines.len());
        for line in &lines[start..end] {
            writeln!(out, "{line}")?;
        }
        out.flush()?;
        start = end;
        if start >= lines.len() {
            break;
        }
        write!(err, "-- more -- (Enter: next page, q: quit) ")?;
        err.flush()?;
        match read_key(keys) {
            Key::Next => {}
            Key::Quit => break,
            Key::Eof => {
                writeln!(err)?;
                break;
            }
        }
    }
    out.flush()
}

#[cfg(test)]
mod tests;
