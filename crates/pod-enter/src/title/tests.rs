use super::*;

#[test]
fn the_map_is_read_from_the_fields_of_proc_stat() {
    // 52 fields; the name holds a space and a `)`.
    let mut fields: Vec<String> = (3..=52).map(|n| n.to_string()).collect();
    fields[0] = "S".into();
    let stat = format!("77 (a b) c) {}", fields.join(" "));
    let m = map_of(&stat, 9).expect("parses");
    assert_eq!((m.start_code, m.end_code, m.start_stack), (26, 27, 28), "{m:?}");
    assert_eq!((m.start_data, m.end_data, m.start_brk, m.brk), (45, 46, 47, 9));
    assert_eq!((m.arg_start, m.arg_end, m.env_start, m.env_end), (48, 49, 50, 51));
    assert_eq!((m.auxv, m.auxv_size, m.exe_fd), (0, 0, u32::MAX));
    assert_eq!(map_of("77 (x) S 1 2", 0), None, "too few fields");
    assert_eq!(map_of("no name", 0), None);
    let t = titled(m, 1000, 6);
    assert_eq!((t.arg_start, t.arg_end, t.env_start, t.env_end), (1000, 1006, 1006, 1006));
    assert_eq!(t.start_code, m.start_code);
    assert_eq!(std::mem::size_of::<Map>(), 104, "struct prctl_mm_map");
    let threads = map_and_threads(&stat, 9).map(|(_, t)| t);
    assert_eq!(threads, Some(20), "the threads");
    let state = stat.replace(" 26 ", " x ");
    assert_eq!(map_of(&state, 9), None, "a field that is no number");
}

/// The child of [`a_hidden_process_shows_its_title_and_no_environment`]:
/// a process of its own, as `hide` wants one thread.
#[test]
#[ignore = "the child of a_hidden_process_shows_its_title_and_no_environment"]
fn hidden_child() {
    let before = std::fs::read("/proc/self/cmdline").unwrap();
    // The name is of a thread: this test runs on a thread of the
    // harness, whose other thread waits for it.
    let name = "/proc/thread-self/comm";
    let name_before = std::fs::read_to_string(name).unwrap();
    let saved = match hide_as("pod-enter-test", false) {
        Ok(saved) => saved,
        Err(e) => {
            println!("T-1112: REFUSED: {e}");
            return;
        }
    };
    let cmd = std::fs::read("/proc/self/cmdline").unwrap();
    let env = std::fs::read("/proc/self/environ").unwrap();
    assert_eq!(cmd, b"pod-enter-test\0", "the title");
    assert_eq!(std::fs::read_to_string(name).unwrap(), "pod-enter-test\n");
    assert!(env.is_empty(), "no environment");
    assert!(std::env::args_os().count() > 1, "the arguments stay");
    restore_as(&saved, false).unwrap();
    assert_eq!(std::fs::read("/proc/self/cmdline").unwrap(), before, "the restore");
    assert_eq!(std::fs::read_to_string(name).unwrap(), name_before);
}

/// In a process of its own: the title shows in `/proc/self/cmdline`,
/// the environment reads empty, `std::env::args` still gives the
/// arguments, and the restore gives the command line back. Where the
/// kernel refuses the call (no `CONFIG_CHECKPOINT_RESTORE`) the test
/// says so and passes.
#[test]
fn a_hidden_process_shows_its_title_and_no_environment() {
    // The harness has threads: the title is refused, and nothing moves.
    let before = std::fs::read("/proc/self/cmdline").unwrap();
    let e = hide("pod-enter-test").expect_err("more than one thread");
    assert_eq!(e.call, "a title with more than one thread");
    assert_eq!(std::fs::read("/proc/self/cmdline").unwrap(), before);
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "title::tests::hidden_child"])
        .args(["--nocapture", "--test-threads=1"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    if text.contains("T-1112: REFUSED") {
        return pod_testkit::host::skip_line("PR_SET_MM_MAP is refused here");
    }
    assert!(text.contains("1 passed"), "{text}");
    assert!(out.status.success(), "{text}");
}

/// The init and the relay take the title of the plan, else the product of
/// the names; a title with a NUL is refused before the fork. The relay of
/// a join and the init with no user namespace of the rung are not
/// dumpable; the init in a user namespace of the rung is (T-1113). A child
/// that stays no init and no relay takes nothing.
#[test]
fn the_title_and_the_dump_flag_follow_the_fork() {
    let mut plan = crate::tests::plain_plan();
    let own = Pid { flags: sys::CLONE_NEWPID, own: true, relay: true, parent: None };
    let user = Pid { flags: sys::CLONE_NEWPID | sys::CLONE_NEWUSER, ..own };
    let relay = Pid { relay: true, ..Pid::default() };
    assert!(Shown::of(&plan, &Pid::default()).unwrap().is_none());
    let s = Shown::of(&plan, &own).unwrap().unwrap();
    assert_eq!((s.len, s.dumpable), (plan.names.product.len() as u64 + 1, false));
    assert!(Shown::of(&plan, &user).unwrap().unwrap().dumpable);
    plan.relay_title = Some("unit-exec".to_string());
    let s = Shown::of(&plan, &relay).unwrap().unwrap();
    assert_eq!((s.len, s.dumpable), (10, false));
    assert!(s.map.is_some(), "the map of this process");
    plan.relay_title = Some("a\0b".to_string());
    let e = Shown::of(&plan, &relay).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("contains a NUL byte"), "{e}");
}
