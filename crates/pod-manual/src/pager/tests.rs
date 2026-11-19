use super::*;
use std::io::Cursor;
use std::sync::Mutex;

/// The tests that change the actions of SIGINT and SIGQUIT, which the
/// process shares, run one at a time.
static SIGNALS: Mutex<()> = Mutex::new(());

fn lines(n: usize) -> String {
    (0..n).map(|i| format!("line {i}\n")).collect()
}

fn paged(text: &str, keys: &[u8]) -> (String, String) {
    let mut keys = Cursor::new(keys.to_vec());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    page(text, PAGE_LINES, &mut keys, &mut out, &mut err).unwrap();
    (String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
}

/// The control: Enter at each prompt gives the whole text, unchanged.
#[test]
fn enter_at_each_prompt_gives_the_whole_text() {
    let text = lines(PAGE_LINES * 3 + 7);
    assert_eq!(paged(&text, b"\n\n\n").0, text);
}

#[test]
fn q_stops_after_the_first_screenful() {
    let (out, _) = paged(&lines(PAGE_LINES * 3), b"q\n");
    assert_eq!(out.lines().count(), PAGE_LINES);
}

#[test]
fn the_prompt_goes_to_stderr_only() {
    let text = lines(PAGE_LINES + 1);
    let (out, err) = paged(&text, b"\n");
    assert_eq!(out, text);
    assert!(!out.contains("more"));
    assert!(err.contains("-- more --"));
}

/// The end of the input stops the pager: with no keys there is no wait.
#[test]
fn the_end_of_the_input_stops_the_pager() {
    let (out, _) = paged(&lines(PAGE_LINES * 2), b"");
    assert_eq!(out.lines().count(), PAGE_LINES);
}

#[test]
fn only_a_terminal_on_both_ends_may_page() {
    assert!(!Tty::none().can_page());
    assert!(!Tty { stdin: true, stdout: false }.can_page());
    assert!(!Tty { stdin: false, stdout: true }.can_page());
    assert!(Tty { stdin: true, stdout: true }.can_page());
}

fn env(pager: Option<&str>, term: Option<&str>, less: bool, windows: bool) -> PagerEnv {
    PagerEnv {
        pager: pager.map(str::to_string),
        term: term.map(str::to_string),
        less: less.then(|| PathBuf::from("/usr/bin/less")),
        windows,
        rows: None,
    }
}

#[test]
fn pager_wins_and_empty_or_cat_means_none() {
    let p = |v| choose(&env(Some(v), Some("xterm"), true, false));
    assert_eq!(p("most -s"), Choice::Program(vec!["most".into(), "-s".into()]));
    assert_eq!(p(""), Choice::Direct);
    assert_eq!(p("  "), Choice::Direct);
    assert_eq!(p("cat"), Choice::Direct);
}

#[test]
fn less_needs_a_terminal_name_and_is_not_used_on_windows() {
    let less = Choice::Program(vec!["/usr/bin/less".into()]);
    assert_eq!(choose(&env(None, Some("xterm-256color"), true, false)), less);
    assert_eq!(choose(&env(None, Some("dumb"), true, false)), Choice::BuiltIn);
    assert_eq!(choose(&env(None, None, true, false)), Choice::BuiltIn);
    assert_eq!(choose(&env(None, Some("xterm"), false, false)), Choice::BuiltIn);
    assert_eq!(choose(&env(None, Some("xterm"), true, true)), Choice::BuiltIn);
}

#[test]
fn find_program_finds_only_a_runnable_file() {
    let dir = std::env::temp_dir().join(format!("pod-manual-pager-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = Some(OsString::from(dir.as_os_str()));
    assert_eq!(find_program("less", path.clone()), None);
    let file = dir.join("less");
    std::fs::write(&file, b"").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(find_program("less", path.clone()), None, "a file that cannot run");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(find_program("less", path), Some(file));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_program_that_does_not_start_is_an_error() {
    let cmd = vec![OsString::from("pod-manual-no-such-pager-program")];
    assert!(run_program(&cmd, "text").is_err());
    assert!(run_program(&[], "text").is_err());
}

/// A pager that quits at once does not make the program fail or wait, also
/// with more text than a pipe holds.
#[test]
fn a_pager_that_quits_at_once_is_not_an_error() {
    let _one = SIGNALS.lock().unwrap_or_else(|e| e.into_inner());
    let cmd: Vec<OsString> =
        if cfg!(windows) { vec!["cmd".into(), "/C".into(), "exit 0".into()] } else { vec!["true".into()] };
    run_program(&cmd, &lines(100_000)).unwrap();
}

#[cfg(unix)]
#[test]
fn a_pager_gets_the_whole_text_on_its_stdin() {
    let _one = SIGNALS.lock().unwrap_or_else(|e| e.into_inner());
    let file = std::env::temp_dir().join(format!("pod-manual-pager-out-{}", std::process::id()));
    let script = format!("cat > '{}'", file.display());
    let text = lines(10_000);
    run_program(&["sh".into(), "-c".into(), script.into()], &text).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
    let _ = std::fs::remove_file(&file);
}

/// A program holds SIGINT and SIGQUIT with `Interrupts` while its own child
/// (an editor) reads the terminal; the drop puts back each action.
#[cfg(unix)]
#[test]
fn interrupts_ignores_sigint_and_sigquit_until_its_drop() {
    let _one = SIGNALS.lock().unwrap_or_else(|e| e.into_inner());
    let action = |signal| {
        // SAFETY: a null new action only reads the current one into a
        // zeroed sigaction, a valid value of the C type.
        unsafe {
            let mut now: libc::sigaction = std::mem::zeroed();
            libc::sigaction(signal, std::ptr::null(), &mut now);
            now.sa_sigaction
        }
    };
    let before = (action(libc::SIGINT), action(libc::SIGQUIT));
    let held = Interrupts::ignore();
    assert_eq!((action(libc::SIGINT), action(libc::SIGQUIT)), (libc::SIG_IGN, libc::SIG_IGN));
    drop(held);
    assert_eq!((action(libc::SIGINT), action(libc::SIGQUIT)), before);
}
