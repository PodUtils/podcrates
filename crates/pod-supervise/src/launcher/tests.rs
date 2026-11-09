/// ⛔ T-0603, asserted rather than commented. `PR_SET_PDEATHSIG` fires on
/// the creating THREAD's exit, so the path that clones, chroots and execs
/// has to be the process.
///
/// ⚠ **This asserts on the SOURCE, and the first version of it asserted on
/// a thread count and was flaky.** Counting `/proc/self/task` before and
/// after does not work under `cargo test`, which runs tests in threads of
/// one process and changes that count for reasons that have nothing to do
/// with podvm: it failed once in about five runs, which is a test whose
/// name claimed more than it checked. The constraint is that these two
/// crates spawn no thread at all, and that is what is read here. The
/// runtime half is clause 3 of `experiments/230-lifecycle-loop.sh`, which
/// reads `/proc/<launcher>/task` of a real detached launcher.
#[test]
fn nothing_on_the_spawn_path_can_spawn_a_thread() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/");
    // ⛔ THE NEEDLES ARE ASSEMBLED AT RUN TIME AND NEVER WRITTEN OUT HERE.
    // This test reads its own file, so a literal `thread` + `::spawn` in
    // the assertion below is a match against itself: it failed exactly that
    // way on 2026-09-09, and it is the same trap podvm-plant carries
    // three times over.
    let needles = [
        format!("thread{}spawn", "::"),
        format!("thread{}Builder", "::"),
    ];
    let mut looked = 0;
    for crate_name in ["podvm-supervise", "podvm-enter"] {
        let dir = root.join(crate_name).join("src");
        for f in std::fs::read_dir(&dir).expect("the crate's src/") {
            let path = f.expect("an entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("a source file");
            looked += 1;
            for (n, line) in text.lines().enumerate() {
                // ⚠ The doc comment above says the words; a line that is a
                // comment is not code.
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
    assert!(looked >= 4, "only {looked} sources were read");
}
