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
    assert!(AGENT_CMD.contains("if defined WQ_ACTIVE goto :eof"));
    let mut g = Guest::with(&[(MARKER, "m"), (GO, "tok")]);
    g.vars.insert("WQ_ACTIVE".into(), "1".into());
    g.walk(|_, _| {});
    assert!(g.events.is_empty(), "{:?}", g.events);
}

/// `call` runs the command in the shell of the agent: an `exit` in it ends
/// the agent before the result, and the run times out. A child `cmd /c`
/// ends alone.
#[test]
fn an_exit_in_the_command_ends_a_child_shell_and_not_the_agent() {
    let line = "cmd /c %WQ%\\WQCMD.CMD < NUL > %WQ%\\WQOUT.TXT 2> %WQ%\\WQERR.TXT";
    assert!(AGENT_CMD.lines().any(|l| l == line), "{AGENT_CMD}");
    assert!(!AGENT_CMD.contains("call %WQ%"), "no `call` of the command");
    // The exit code is kept on the next line, before a step changes it.
    let at = AGENT_CMD.lines().position(|l| l == line).unwrap();
    assert_eq!(AGENT_CMD.lines().nth(at + 1), Some("set WQRC=%errorlevel%"));
    assert!(AGENT_CMD.contains(">%WQ%\\WQCODE.TXT echo %WQRC% %WQNONCE%"));
}

#[test]
fn the_agent_writes_ready_before_it_reads_the_token() {
    let mut g = Guest::with(&[(MARKER, "m"), (GO, "tok-9")]);
    g.walk(|_, _| {});
    assert_eq!(g.events, ["write WQREADY.TXT", "run", "write WQCODE.TXT", "del WQGO.TXT", "shutdown"]);
    assert_eq!(g.files[READY], READY_TEXT);
    assert_eq!(g.files[CODE], "0 tok-9");
    // With no token the agent writes the marker, and the guest stays up.
    let mut idle = Guest::with(&[(MARKER, "m")]);
    idle.walk(|_, _| {});
    assert_eq!(idle.events, ["write WQREADY.TXT"]);
}

/// The default mode shuts the guest down after one command; the keep-alive
/// mode runs each next token, and a last command with no [`KEEP`] shuts the
/// guest down.
#[test]
fn the_keep_alive_mode_runs_each_next_token_and_does_not_shut_down() {
    let mut g = Guest::with(&[(MARKER, "m"), (KEEP, ""), (GO, "tok-1")]);
    g.walk(|wait, g| match wait {
        // The host writes the next token while the guest waits.
        2 => {
            g.files.insert(GO.into(), "tok-2".into());
            g.exit = 7;
        }
        4 => {
            g.files.remove(KEEP);
            g.files.insert(GO.into(), "tok-3".into());
            g.exit = 3;
        }
        _ => {}
    });
    let runs = g.events.iter().filter(|e| *e == "run").count();
    assert_eq!(runs, 3, "{:?}", g.events);
    assert_eq!(g.events.iter().filter(|e| *e == "shutdown").count(), 1, "{:?}", g.events);
    assert_eq!(g.events.last().map(String::as_str), Some("shutdown"));
    assert_eq!(g.files[CODE], "3 tok-3");
    // No shutdown before the last command.
    let first = g.events.iter().position(|e| e == "shutdown").unwrap();
    assert_eq!(g.events[..first].iter().filter(|e| *e == "run").count(), 3);
}

#[test]
fn ready_reads_the_marker_of_the_agent() {
    let mut m = crate::Fat16::new();
    assert!(!ready(&m));
    m.put(READY, b"WQREADY \r\n").unwrap();
    assert!(ready(&m));
    m.put(READY, b"other").unwrap();
    assert!(!ready(&m));
}

/// A walk of the agent over a mailbox of names and texts: each line of
/// [`AGENT_CMD`] that the agent runs, by the meaning that `cmd.exe` gives it.
/// A line that the walk does not know fails the test, so that a change of
/// the agent changes the walk too.
struct Guest {
    files: std::collections::HashMap<String, String>,
    vars: std::collections::HashMap<String, String>,
    events: Vec<String>,
    /// The exit code of the next command.
    exit: i32,
}

impl Guest {
    fn with(files: &[(&str, &str)]) -> Guest {
        Guest {
            files: files.iter().map(|(n, t)| (n.to_string(), t.to_string())).collect(),
            vars: Default::default(),
            events: Vec::new(),
            exit: 0,
        }
    }

    fn expand(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (k, v) in &self.vars {
            out = out.replace(&format!("%{k}%"), v);
        }
        out
    }

    /// The name of a file on the mailbox in `word` (`D:\NAME`).
    fn name(word: &str) -> String {
        word.rsplit('\\').next().unwrap().trim_matches('"').to_string()
    }

    /// Walk the agent. `at_wait(n, self)` runs at the `n`th wait of one
    /// second, from 1; the walk stops after 2,000 waits.
    fn walk(&mut self, mut at_wait: impl FnMut(u32, &mut Guest)) {
        let lines: Vec<&str> = AGENT_CMD.lines().collect();
        let label = |l: &str| lines.iter().position(|x| *x == format!(":{l}")).expect(l);
        let (mut pc, mut waits) = (0usize, 0u32);
        while pc < lines.len() {
            let raw = lines[pc];
            pc += 1;
            let line = self.expand(raw);
            let jump = |target: &str| -> Option<usize> {
                if target == ":eof" { Some(lines.len()) } else { Some(label(target) + 1) }
            };
            let to = if raw.starts_with(':') || raw == "@echo off" {
                None
            } else if let Some(rest) = raw.strip_prefix("if defined ") {
                let (v, t) = rest.split_once(" goto ").unwrap();
                if self.vars.contains_key(v) { jump(t) } else { None }
            } else if let Some(rest) = raw.strip_prefix("if not defined ") {
                let (v, t) = rest.split_once(" goto ").unwrap();
                if self.vars.contains_key(v) { None } else { jump(t) }
            } else if let Some(rest) = line.strip_prefix("if exist ") {
                let (f, t) = rest.split_once(" goto ").unwrap();
                if self.files.contains_key(&Guest::name(f)) { jump(t) } else { None }
            } else if raw == "if %WQTRIES% GEQ 600 goto :eof" {
                if self.vars["WQTRIES"].parse::<u32>().unwrap() >= 600 { jump(":eof") } else { None }
            } else if let Some(t) = raw.strip_prefix("goto ") {
                jump(t)
            } else if raw == "set /a WQTRIES+=1" {
                let n: u32 = self.vars["WQTRIES"].parse().unwrap();
                self.vars.insert("WQTRIES".into(), (n + 1).to_string());
                None
            } else if raw.starts_with("for %%d in (") {
                if self.files.contains_key(MARKER) && !self.vars.contains_key("WQ") {
                    self.vars.insert("WQ".into(), "D:".into());
                }
                None
            } else if raw.starts_with("for /f ") && raw.contains("WQGO.TXT") {
                let token = self.files.get(GO).and_then(|t| t.lines().next()).unwrap_or_default().to_string();
                if !token.is_empty() && !self.vars.contains_key("WQNONCE") {
                    self.vars.insert("WQNONCE".into(), token);
                }
                None
            } else if raw.starts_with("cmd /c ") {
                self.events.push("run".into());
                self.vars.insert("errorlevel".into(), self.exit.to_string());
                None
            } else if let Some(rest) = line.strip_prefix('>') {
                let (f, text) = rest.split_once(" echo ").unwrap();
                self.events.push(format!("write {}", Guest::name(f)));
                self.files.insert(Guest::name(f), text.to_string());
                None
            } else if let Some(rest) = line.strip_prefix("del ") {
                let f = Guest::name(rest.split(' ').next().unwrap());
                self.events.push(format!("del {f}"));
                self.files.remove(&f);
                None
            } else if raw == "shutdown /s /t 0 /f" {
                self.events.push("shutdown".into());
                None
            } else if raw.starts_with("ping -n 2 ") {
                waits += 1;
                assert!(waits <= 2_000, "the agent waits with no end: {:?}", self.events);
                at_wait(waits, self);
                None
            } else if let Some((v, value)) = raw.strip_prefix("set ").and_then(|r| r.split_once('=')) {
                if value.is_empty() {
                    self.vars.remove(v);
                } else {
                    let value = self.expand(value);
                    self.vars.insert(v.to_string(), value);
                }
                None
            } else {
                panic!("the walk does not know the line {raw:?}");
            };
            if let Some(next) = to {
                pc = next;
            }
        }
    }
}

#[test]
fn the_agent_gives_up_after_600_tries_with_no_mailbox() {
    let mut g = Guest::with(&[]);
    let mut seen = 0;
    g.walk(|_, _| seen += 1);
    assert_eq!(seen, 599, "one wait between two of the 600 tries");
    assert!(g.events.is_empty(), "{:?}", g.events);
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
