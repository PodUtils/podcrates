//! The guest side of the UEFI flavour: two `cmd.exe` scripts and the file
//! protocol of the mailbox.
//!
//! The guest runs `cmd.exe` and nothing else. There is no agent binary to
//! cross-compile, no driver to sign, and no binary that the host must
//! trust: the guest side is the two scripts below. The flavour needs a
//! Windows image, not a toolchain.
//!
//! The agent starts from a scheduled task. On a Validation OS image the
//! `AutoRun` value of `cmd.exe` and the `Run` and `RunOnce` values do not
//! fire: that logon does not go through the `Run` processing of
//! `userinit.exe`. An `onstart` task as `SYSTEM` fires, and [`SETUP_CMD`]
//! registers it.
//!
//! A Windows service does not work: the service control manager ends a
//! `cmd.exe` image that does not report `SERVICE_RUNNING`, and the end
//! comes during the command. A higher `ServicesPipeTimeout` does not help.
//!
//! The agent deletes the token after it writes the result. A run that is
//! killed between the two runs the command again at the next boot; it does
//! not lose it.
//!
//! The command runs in a child `cmd /c`, not with `call`: an `exit` in the
//! command ends the child, and the agent still writes the result and stops
//! the guest. The stdin of the command is `NUL`.
//!
//! The agent writes [`READY`] when it finds the mailbox, before it reads the
//! token. With [`KEEP`] on the mailbox ([`Mode::KeepAlive`]) the agent does
//! not shut the guest down: after each command it polls the mailbox for the
//! next token, about one time each second.

/// The file that marks the mailbox volume. The agent looks for it on each
/// drive letter: Windows gives the letter, and it can change between boots.
pub const MARKER: &str = "WQMARK.TXT";
/// The token file. The agent writes its content back with the result, so
/// that a host with more than one run does not read the wrong result.
pub const GO: &str = "WQGO.TXT";
/// The command file the host writes.
pub const CMD: &str = "WQCMD.CMD";
/// The standard output of the command, as the agent captured it.
pub const OUT: &str = "WQOUT.TXT";
/// The standard error of the command, as the agent captured it.
pub const ERR: &str = "WQERR.TXT";
/// `"<exit code> <token>"`.
pub const CODE: &str = "WQCODE.TXT";
/// The agent, as the installer puts it in `C:\Windows\System32`.
pub const AGENT: &str = "C:\\Windows\\System32\\wqagent.cmd";
/// The report of the installer: the result of a provision boot.
pub const SETUP: &str = "SETUP.TXT";
/// The marker that the agent writes when it finds the mailbox, before it
/// reads the token: the agent runs and the guest has booted.
pub const READY: &str = "WQREADY.TXT";
/// The content of [`READY`].
pub const READY_TEXT: &str = "WQREADY";
/// The file of the keep-alive mode: with it on the mailbox, the agent does
/// not shut the guest down and polls for the next token.
pub const KEEP: &str = "WQKEEP.TXT";

/// What the agent does after a command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// Write the result and shut the guest down. The host reads the result
    /// after the emulator exits.
    #[default]
    Once,
    /// Write the result, keep the guest up, and poll the mailbox for the
    /// next token ([`KEEP`] on the mailbox). The guest does not power off,
    /// so [`crate::run`] refuses this mode: the caller polls the mailbox.
    KeepAlive,
}

/// The agent. Each boot runs it, and it does nothing when no token waits.
///
/// `cmd /c` runs the command in a child shell, so that an `exit` in the
/// command does not end the agent. `WQRC` keeps the exit code before the
/// next line changes `%errorlevel%`.
pub const AGENT_CMD: &str = r#"@echo off
if defined WQ_ACTIVE goto :eof
set WQ_ACTIVE=1
set WQTRIES=0
:probe
set WQ=
for %%d in (D E F G H I J K L M N O P Q R S T U V W X Y Z) do if not defined WQ if exist %%d:\WQMARK.TXT set WQ=%%d:
if defined WQ goto found
set /a WQTRIES+=1
if %WQTRIES% GEQ 600 goto :eof
ping -n 2 127.0.0.1 >nul
goto probe
:found
>%WQ%\WQREADY.TXT echo WQREADY
:next
if exist %WQ%\WQGO.TXT goto take
if exist %WQ%\WQKEEP.TXT goto idle
goto :eof
:take
set WQNONCE=
for /f "usebackq delims=" %%t in ("%WQ%\WQGO.TXT") do if not defined WQNONCE set WQNONCE=%%t
if not defined WQNONCE goto :eof
cmd /c %WQ%\WQCMD.CMD < NUL > %WQ%\WQOUT.TXT 2> %WQ%\WQERR.TXT
set WQRC=%errorlevel%
>%WQ%\WQCODE.TXT echo %WQRC% %WQNONCE%
del %WQ%\WQGO.TXT >nul 2>&1
if exist %WQ%\WQKEEP.TXT goto idle
shutdown /s /t 0 /f
goto :eof
:idle
ping -n 2 127.0.0.1 >nul
goto next
"#;

/// The installer, for one run. It copies [`AGENT_CMD`] into the image and
/// registers the boot task. After one run the image is provisioned, and
/// each later boot starts the agent.
///
/// The installer sets the task scheduler service to `AUTO_START` first: an
/// `onstart` task fires only when the scheduler runs.
pub const SETUP_CMD: &str = r#"@echo off
set WQ=
for %%d in (D E F G H I J K L M N O P Q R S T U V W X Y Z) do if not defined WQ if exist %%d:\WQMARK.TXT set WQ=%%d:
copy /y %WQ%\WQAGENT.CMD C:\Windows\System32\wqagent.cmd >nul
sc stop wqagent >nul 2>&1
sc delete wqagent >nul 2>&1
sc config schedule start= auto >nul 2>&1
sc start schedule >nul 2>&1
schtasks /delete /tn wqagent /f >nul 2>&1
schtasks /create /tn wqagent /tr "cmd /c C:\Windows\System32\wqagent.cmd" /sc onstart /ru SYSTEM /f >nul 2>&1
echo INSTALLED %WQ% > %WQ%\SETUP.TXT
shutdown /s /t 0 /f
"#;

/// The name of the agent on the mailbox. The installer copies it to [`AGENT`].
pub const AGENT_NAME: &str = "WQAGENT.CMD";
/// The name of the installer on the mailbox.
pub const SETUP_NAME: &str = "WA.CMD";

/// The command file: one `cmd.exe` command line in a batch file, so that
/// the redirections and the quotes in it are the guest's own, and the exit
/// code stays.
///
/// A line, not an argv: Windows has no argv at this layer. `cmd.exe` parses
/// a line, and `&`, `|`, `>`, `%VAR%` and the quotes are its operators. An
/// argv with each element in quotes makes `ver & echo hi` one token, and
/// `cmd.exe` refuses it with exit 123.
///
/// The file ends with `exit /b %errorlevel%`: the agent reads the exit
/// code of the child `cmd /c`, and with no such line a command that fails
/// returns the code of an earlier step.
pub fn command_file(command: &str) -> String {
    let mut s = String::from("@echo off\r\n");
    s.push_str(command);
    s.push_str("\r\nexit /b %errorlevel%\r\n");
    s
}

/// The guest command line for a host argv (`cmd /c ver`).
///
/// A `cmd /c` prefix drops: the guest is `cmd.exe` already, and `cmd /c
/// ver` and `ver` are the same request. An empty remainder is `ver`.
pub fn command_from_argv(argv: &[String]) -> String {
    let words = match argv {
        [a, b, rest @ ..] if a == "cmd" && b == "/c" => rest,
        [] => return "ver".to_string(),
        all => all,
    };
    let line = words.join(" ");
    if line.trim().is_empty() { "ver".to_string() } else { line }
}

/// One argument in the form that `cmd.exe` parses back. An argument with a
/// space, a tab or a quote goes in quotes; a quote in it becomes two.
pub fn quote(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".to_string();
    }
    if !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    format!("\"{}\"", arg.replace('"', "\"\""))
}

/// True when the agent wrote [`READY`] to the mailbox `m`.
pub fn ready(m: &crate::Fat16) -> bool {
    m.get(READY).is_some_and(|b| String::from_utf8_lossy(&b).trim() == READY_TEXT)
}

/// An exit code and its token, as [`CODE`] holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    pub code: i32,
    pub token: String,
}

/// Read `"<code> <token>"`. The token keeps its spaces; the code has none.
pub fn parse_code(text: &str) -> Option<Code> {
    let t = text.trim_matches(|c: char| c == '\r' || c == '\n' || c == ' ');
    let (code, rest) = match t.split_once(' ') {
        Some((c, r)) => (c, r.trim().to_string()),
        None => (t, String::new()),
    };
    let code: i32 = code.parse().ok()?;
    Some(Code { code, token: rest })
}

/// One run token: 16 bytes from the random source of the system, in hex.
///
/// Not a clock and not a counter: a payload that prints a line in the form
/// of a result cannot guess 128 random bits, so a forged [`CODE`] does not
/// match. A time can be guessed, and a counter repeats across processes.
pub fn nonce() -> crate::Result<String> {
    nonce_from(&mut pod_core::entropy::OsEntropy)
}

/// [`nonce`] from the source `e`. A source that gives no bytes is a
/// failure, never a weaker token.
pub fn nonce_from(e: &mut impl pod_core::entropy::Entropy) -> crate::Result<String> {
    let mut buf = [0u8; 16];
    e.fill(&mut buf).map_err(|why| {
        pod_core::Failure::caused(pod_core::Stage::Host, pod_core::Class::Unavailable, why.to_string(), why)
    })?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests;
