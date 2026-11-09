//! The guest side: two `cmd.exe` scripts and the mailbox file protocol.
//! `TODO/milestones.md` T-1112.
//!
//! ⛔ **The guest is `cmd.exe` and nothing else.** There is no cross-compiled
//! Windows agent to install, no driver to sign and no binary the host has to
//! trust the guest to run: the whole guest side is the two scripts below,
//! which `cmd.exe` already knows how to execute. That is what makes the
//! driver portable, it needs a Windows image, not a toolchain.
//!
//! ⛔ **Autostart is a scheduled task, and the two mechanisms that look more
//! obvious do not work on the reference image.** `cmd.exe`'s `AutoRun`
//! registry value does not fire for the shell Validation OS starts, and
//! neither does a `Run`/`RunOnce` value, because that logon does not go
//! through `userinit.exe`'s `Run` processing. An `onstart` scheduled task as
//! `SYSTEM` does fire, so that is what [`setup_cmd`] registers.
//!
//! ⚠ **A Windows *service* was tried and refused.** `sc create` with a
//! `cmd.exe` image starts the script and then the service control manager
//! terminates it once the process fails to report `SERVICE_RUNNING`, which
//! `cmd.exe` never does, and the termination lands mid-command. Raising
//! `ServicesPipeTimeout` did not save it. The task does not have that
//! problem, so the driver does not have that code.
//!
//! ⚠ **The token is deleted *after* the result is written.** A run killed
//! between the two re-runs the command on the next attempt rather than
//! losing it, which is the failure the reference protocol has.

/// The file whose presence marks the mailbox volume. The agent probes for it
/// rather than assuming a drive letter: Windows assigns the letter, and the
/// reference image has been seen to hand the volume a different one between
/// boots.
pub const MARKER: &str = "WQMARK.TXT";
/// The token file. Its content is echoed back in the result, so a host that
/// has more than one run in flight cannot read the wrong answer.
pub const GO: &str = "WQGO.TXT";
/// The command file the host writes.
pub const CMD: &str = "WQCMD.CMD";
/// The command's standard output, captured by the agent.
pub const OUT: &str = "WQOUT.TXT";
/// The command's standard error, captured by the agent.
pub const ERR: &str = "WQERR.TXT";
/// `"<exit code> <token>"`.
pub const CODE: &str = "WQCODE.TXT";
/// The agent, as installed into `C:\Windows\System32`.
pub const AGENT: &str = "C:\\Windows\\System32\\wqagent.cmd";
/// The installer's own report, which is how a provisioning boot is read back.
pub const SETUP: &str = "SETUP.TXT";

/// The agent. Every boot runs this, and it does nothing at all unless a token
/// is waiting.
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

/// The one-time installer. It copies [`AGENT_CMD`] into the image and
/// registers the boot task; after one run the image is provisioned and every
/// later boot is automatic.
///
/// ⚠ The task scheduler service is forced to `AUTO_START` first. An `onstart`
/// task that the scheduler was not running to see is a task that never fires.
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

/// The name the agent is installed under, which is also what the task runs.
pub const AGENT_NAME: &str = "WQAGENT.CMD";
/// The installer's name on the mailbox.
pub const SETUP_NAME: &str = "WA.CMD";

/// What the host tells the guest to run: one `cmd.exe` command **line**,
/// written as a batch file so that redirection and quoting inside it are the
/// guest's own, with the exit code preserved.
///
/// ⛔ **A line, not an argv, and the difference is a defect that was found by
/// running it.** Windows has no argv at this layer: `cmd.exe` parses a line
/// where `&`, `|`, `>`, `%VAR%` and quoting are all its own operators. An
/// earlier revision of this file took a `Vec<String>` and quoted each
/// element, which turned `ver & echo hi` into one quoted *token*, `cmd.exe`
/// read that as a program named `ver & echo hi` and refused with
/// `The filename, directory name, or volume label syntax is incorrect`
/// (exit 123). The type was the bug, so the type is what changed.
///
/// ⛔ **`exit /b %errorlevel%` rather than a trailing `echo`.** The agent
/// reads `%errorlevel%` after `call`, and that is the batch's own last
/// command unless the batch ends by stating the code. Without this line a
/// command that fails returns the code of whatever a previous step left
/// behind, which is the silent-success defect this protocol exists to avoid.
pub fn command_file(command: &str) -> String {
    let mut s = String::from("@echo off\r\n");
    s.push_str(command);
    s.push_str("\r\nexit /b %errorlevel%\r\n");
    s
}

/// The guest command line for a host argv, which is what `podvm run
/// --platform windows/amd64 IMAGE cmd /c ver` hands over.
///
/// ⭐ A `cmd /c` prefix drops, because `cmd.exe` is what the guest already
/// is: `cmd /c ver` and `ver` are the same request, and leaving the prefix
/// in would run the *inner* `cmd.exe` through the outer one for no gain.
/// An empty remainder is `ver`, which is the acceptance command in
/// `TODO/milestones.md` T-1112, so the shortest possible invocation is also
/// the one the entry asks for.
pub fn command_from_argv(argv: &[String]) -> String {
    let words = match argv {
        [a, b, rest @ ..] if a == "cmd" && b == "/c" => rest,
        [] => return "ver".to_string(),
        all => all,
    };
    let line = words.join(" ");
    if line.trim().is_empty() {
        "ver".to_string()
    } else {
        line
    }
}

/// One argument in the spelling `cmd.exe` parses back. An argument with a
/// space or a tab is quoted; an embedded quote is doubled.
pub fn quote(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".to_string();
    }
    if !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    format!("\"{}\"", arg.replace('"', "\"\""))
}

/// An exit code and the token it belongs to, as [`CODE`] carries them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    pub code: i32,
    pub token: String,
}

/// Read `"<code> <token>"`. The token keeps any spaces; the code does not.
pub fn parse_code(text: &str) -> Option<Code> {
    let t = text.trim_matches(|c: char| c == '\r' || c == '\n' || c == ' ');
    let (code, rest) = match t.split_once(' ') {
        Some((c, r)) => (c, r.trim().to_string()),
        None => (t, String::new()),
    };
    let code: i32 = code.parse().ok()?;
    Some(Code { code, token: rest })
}

/// One run token: 16 bytes from the kernel's entropy, hex encoded.
///
/// ⛔ **Not a clock and not a counter.** The token is what tells one run's
/// answer from another's, and a payload that prints a completion-shaped
/// line cannot guess 128 bits of entropy, so a forged `WQCODE.TXT` never
/// matches. A timestamp would be guessable and a counter would repeat
/// across processes, and a repeated token replays a prior run's verdict.
pub fn nonce() -> String {
    use std::io::Read;
    let mut buf = [0u8; 16];
    // Fail loud where the kernel gives no entropy: a weaker fallback here
    // is exactly the replay the token exists to prevent.
    let mut f = std::fs::File::open("/dev/urandom").expect("open /dev/urandom");
    f.read_exact(&mut buf).expect("read /dev/urandom");
    let mut s = String::with_capacity(32);
    for b in buf {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_is_hex_and_fresh() {
        let a = nonce();
        let b = nonce();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "{a}");
        assert_ne!(a, b, "two runs must not share a token");
    }

    #[test]
    fn a_cmd_slash_c_prefix_drops_and_an_empty_command_is_ver() {
        let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(command_from_argv(&v(&["cmd", "/c", "ver"])), "ver");
        assert_eq!(
            command_from_argv(&v(&["cmd", "/c", "dir", "/zzz"])),
            "dir /zzz"
        );
        assert_eq!(command_from_argv(&v(&["ver"])), "ver");
        assert_eq!(command_from_argv(&v(&[])), "ver");
        assert_eq!(command_from_argv(&v(&["cmd", "/c"])), "ver");
        // a bare `cmd` is not a prefix and stays as written
        assert_eq!(command_from_argv(&v(&["cmd", "/c"])), "ver");
        assert_eq!(command_from_argv(&v(&["cmd"])), "cmd");
    }

    #[test]
    fn the_agent_waits_for_a_token_and_deletes_it_last() {
        assert!(AGENT_CMD.contains("WQGO.TXT"));
        assert!(AGENT_CMD.contains("WQCODE.TXT"));
        assert!(AGENT_CMD.contains("shutdown /s"));
        // The token is removed after the result is written, not before.
        let code = AGENT_CMD.find("WQCODE.TXT").expect("writes the code");
        let del = AGENT_CMD
            .find("del %WQ%\\WQGO.TXT")
            .expect("deletes the token");
        assert!(code < del, "code is written before the token is removed");
        // 600 tries at about a second each, so the agent outlives a slow boot.
        assert!(AGENT_CMD.contains("GEQ 600"));
    }

    #[test]
    fn the_agent_is_reentrant_and_refuses_to_recurse() {
        // `call` is what makes the batch return; a bare invocation would
        // transfer control and the token would never be removed.
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
        // ⚠ The guest powers itself off so the FAT flush happens before the
        // host reads SETUP.TXT back. A host that kills the emulator instead
        // is reading a volume the guest had not finished writing.
        assert!(SETUP_CMD.trim_end().ends_with("shutdown /s /t 0 /f"));
    }

    #[test]
    fn every_name_the_protocol_uses_is_8_3() {
        // The mailbox writer refuses anything else, so this is a real
        // constraint and not a style note.
        for n in [
            MARKER, GO, CMD, OUT, ERR, CODE, AGENT_NAME, SETUP_NAME, SETUP,
        ] {
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

    /// ⭐ A regression test for the defect this file shipped once: the line
    /// goes into the batch **verbatim**, so `cmd.exe` sees its own operators.
    /// Quoting the line as one token is what produced exit 123 against the
    /// real guest, and this fails against that revision.
    #[test]
    fn a_command_line_is_written_verbatim_and_not_as_one_quoted_token() {
        let f = command_file("ver & echo hi & dir C:\\Windows");
        let line = f.lines().nth(1).expect("the command is the second line");
        assert_eq!(line, "ver & echo hi & dir C:\\Windows");
        assert!(!line.starts_with('"'), "the line is not one quoted token");
        assert!(!f.contains("\"ver &"), "{f:?}");
        // and a caller that does need one argument quoted has the helper.
        assert_eq!(quote("C:\\Program Files\\x"), "\"C:\\Program Files\\x\"");
    }

    #[test]
    fn the_code_file_parses_into_a_code_and_a_token() {
        assert_eq!(
            parse_code("0 run-token-21\r\n"),
            Some(Code {
                code: 0,
                token: "run-token-21".into()
            })
        );
        assert_eq!(
            parse_code("1"),
            Some(Code {
                code: 1,
                token: String::new()
            })
        );
        assert_eq!(
            parse_code("  17 spaced token  "),
            Some(Code {
                code: 17,
                token: "spaced token".into()
            })
        );
        assert_eq!(parse_code("nonsense"), None);
        assert_eq!(parse_code(""), None);
    }
}
