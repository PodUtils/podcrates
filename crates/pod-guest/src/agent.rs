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

/// The agent. Each boot runs it, and it does nothing when no token waits.
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
if not exist %WQ%\WQGO.TXT goto :eof
set WQNONCE=
for /f "usebackq delims=" %%t in ("%WQ%\WQGO.TXT") do if not defined WQNONCE set WQNONCE=%%t
if not defined WQNONCE goto :eof
call %WQ%\WQCMD.CMD < NUL > %WQ%\WQOUT.TXT 2> %WQ%\WQERR.TXT
>%WQ%\WQCODE.TXT echo %errorlevel% %WQNONCE%
del %WQ%\WQGO.TXT >nul 2>&1
shutdown /s /t 0 /f
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
/// The file ends with `exit /b %errorlevel%`: the agent reads
/// `%errorlevel%` after `call`, and with no such line a command that fails
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
mod tests {
    use super::*;

    #[test]
    fn the_token_is_hex_and_fresh() {
        let a = nonce().unwrap();
        let b = nonce().unwrap();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "{a}");
        assert_ne!(a, b, "two runs must not share a token");
    }

    /// A source that gives no bytes is a failure, never a weaker token.
    #[test]
    fn a_dry_random_source_gives_no_token() {
        struct Dry;
        impl pod_core::entropy::Entropy for Dry {
            fn fill(&mut self, _: &mut [u8]) -> Result<(), pod_core::entropy::NoRandom> {
                Err(pod_core::entropy::NoRandom)
            }
        }
        let e = nonce_from(&mut Dry).unwrap_err();
        assert_eq!(e.class, pod_core::Class::Unavailable);
        assert!(e.to_string().contains("no random bytes"), "{e}");
    }

    #[test]
    fn a_cmd_slash_c_prefix_drops_and_an_empty_command_is_ver() {
        let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(command_from_argv(&v(&["cmd", "/c", "ver"])), "ver");
        assert_eq!(command_from_argv(&v(&["cmd", "/c", "dir", "/zzz"])), "dir /zzz");
        assert_eq!(command_from_argv(&v(&["ver"])), "ver");
        assert_eq!(command_from_argv(&v(&[])), "ver");
        assert_eq!(command_from_argv(&v(&["cmd", "/c"])), "ver");
        // A bare `cmd` is not a prefix and stays as it is.
        assert_eq!(command_from_argv(&v(&["cmd", "/c"])), "ver");
        assert_eq!(command_from_argv(&v(&["cmd"])), "cmd");
    }

    #[test]
    fn the_agent_waits_for_a_token_and_deletes_it_last() {
        assert!(AGENT_CMD.contains("WQGO.TXT"));
        assert!(AGENT_CMD.contains("WQCODE.TXT"));
        assert!(AGENT_CMD.contains("shutdown /s"));
        // The agent removes the token after it writes the result.
        let code = AGENT_CMD.find("WQCODE.TXT").expect("writes the code");
        let del = AGENT_CMD.find("del %WQ%\\WQGO.TXT").expect("deletes the token");
        assert!(code < del, "code is written before the token is removed");
        // 600 tries of about one second each: the agent waits for a slow boot.
        assert!(AGENT_CMD.contains("GEQ 600"));
    }

    #[test]
    fn the_agent_is_reentrant_and_refuses_to_recurse() {
        // `call` makes the batch return; with no `call`, control does not
        // come back and the token stays.
        assert!(AGENT_CMD.contains("call %WQ%\\WQCMD.CMD"));
        assert!(AGENT_CMD.contains("if defined WQ_ACTIVE goto :eof"));
    }

    #[test]
    fn the_installer_registers_the_boot_task_and_starts_its_scheduler() {
        assert!(SETUP_CMD.contains("schtasks /create"));
        assert!(SETUP_CMD.contains("/sc onstart"));
        assert!(SETUP_CMD.contains("/ru SYSTEM"));
        assert!(SETUP_CMD.contains("sc config schedule start= auto"));
        assert!(SETUP_CMD.contains("wqagent.cmd"));
        // The guest powers itself off, so that its FAT writes are on the
        // disk before the host reads SETUP.TXT.
        assert!(SETUP_CMD.trim_end().ends_with("shutdown /s /t 0 /f"));
    }

    #[test]
    fn every_name_the_protocol_uses_is_8_3() {
        // The mailbox writer refuses each other name.
        for n in [MARKER, GO, CMD, OUT, ERR, CODE, AGENT_NAME, SETUP_NAME, SETUP] {
            assert!(n.len() <= 12, "{n} is longer than 8.3");
            let (base, ext) = n.split_once('.').expect("has an extension");
            assert!(base.len() <= 8 && ext.len() <= 3, "{n} is not 8.3");
        }
    }

    #[test]
    fn a_command_file_preserves_the_exit_code() {
        let f = command_file("ver");
        assert!(f.contains("ver"));
        assert!(f.ends_with("exit /b %errorlevel%\r\n"), "{f:?}");
    }

    /// The line goes into the batch as it is, so that `cmd.exe` sees its own
    /// operators. A line in quotes as one token gives exit 123 in the guest.
    #[test]
    fn a_command_line_is_written_verbatim_and_not_as_one_quoted_token() {
        let f = command_file("ver & echo hi & dir C:\\Windows");
        let line = f.lines().nth(1).expect("the command is the second line");
        assert_eq!(line, "ver & echo hi & dir C:\\Windows");
        assert!(!line.starts_with('"'), "the line is not one quoted token");
        assert!(!f.contains("\"ver &"), "{f:?}");
        // A caller that needs one argument in quotes has the helper.
        assert_eq!(quote("C:\\Program Files\\x"), "\"C:\\Program Files\\x\"");
    }

    #[test]
    fn the_code_file_parses_into_a_code_and_a_token() {
        assert_eq!(parse_code("0 run-token-21\r\n"), Some(Code { code: 0, token: "run-token-21".into() }));
        assert_eq!(parse_code("1"), Some(Code { code: 1, token: String::new() }));
        assert_eq!(parse_code("  17 spaced token  "), Some(Code { code: 17, token: "spaced token".into() }));
        assert_eq!(parse_code("nonsense"), None);
        assert_eq!(parse_code(""), None);
    }
}
