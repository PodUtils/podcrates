/// `PR_SET_PDEATHSIG` fires at the exit of the thread that made the child,
/// so the path that clones, enters and execs must be the process.
///
/// This reads the source, not a count of threads: a count of
/// `/proc/self/task` before and after does not work under `cargo test`,
/// which runs tests in threads of one process. The rule is that this crate
/// starts no thread, and that is what the test reads. The entry of the
/// caller (`enter` of [`super::start`]) runs on the same path: its crate
/// keeps the same rule.
#[test]
fn nothing_on_the_spawn_path_can_spawn_a_thread() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    // The needles are put together at run time and never written out here:
    // this test reads its own file, and a literal would match itself.
    let needles = [format!("thread{}spawn", "::"), format!("thread{}Builder", "::")];
    let mut looked = 0;
    let mut dirs = vec![src];
    while let Some(dir) = dirs.pop() {
        for f in std::fs::read_dir(&dir).expect("a directory of src/") {
            let path = f.expect("an entry").path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            // The one thread of the crate: the server of a foreground hold.
            // It is off this path: `start` refuses while a hold lives.
            if path.file_name().and_then(|n| n.to_str()) == Some("foreground.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("a source file");
            looked += 1;
            for (n, line) in text.lines().enumerate() {
                // A comment says the words; a comment is not code.
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for needle in &needles {
                    assert!(
                        !line.contains(needle),
                        "{}:{} spawns a thread on the path that clones and execs",
                        path.display(),
                        n + 1
                    );
                }
            }
        }
    }
    assert!(looked >= 8, "only {looked} sources were read");
}

#[test]
fn an_exit_status_is_docker_s_exit_code() {
    assert_eq!(super::exit_status(0), 0);
    assert_eq!(super::exit_status(3 << 8), 3);
    assert_eq!(super::exit_status(9), 137);
    assert_eq!(super::exit_status(15), 143);
}
