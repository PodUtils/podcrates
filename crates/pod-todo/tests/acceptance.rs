//! The acceptance registry (`acceptance` of `TODO/todo.toml`): `check`
//! holds each test of an ask to the tree, and `acceptance` prints the asks
//! whose tests passed.

mod common;

use common::Tree;

const REGISTRY: &str = "# Acceptance

The asks and their tests.

| Ask | Tests |
| --- | --- |
| A-X-14 | `crates/x/tests/third.rs::an_ask_passes_with_its_tests`, `scripts/accept.sh` |
| G-10 | `crates/x/src/lib.rs::unit::a_unit_test_of_the_ask` |
";

const THIRD: &str = "#[test]\nfn third() {}\n\n/// The ask.\n#[test]\nfn an_ask_passes_with_its_tests() {}\n\n\
                     #[test]\n#[ignore]\nfn a_live_test() {}\n\nfn a_helper() {}\n";

fn registry(name: &str) -> Tree {
    let t = Tree::new(name);
    t.write("TODO/todo.toml", "acceptance = \"TODO/ACCEPTANCE.md\"\n");
    t.write("TODO/ACCEPTANCE.md", REGISTRY);
    t.write("crates/x/tests/third.rs", THIRD);
    t.write("scripts/accept.sh", "exit 0\n");
    let lib = t.read("crates/x/src/lib.rs");
    t.write("crates/x/src/lib.rs", &format!("{lib}mod unit {{\n    #[tokio::test(flavor = \"current_thread\")]\n    async fn a_unit_test_of_the_ask() {{}}\n}}\n"));
    t
}

#[test]
fn the_control_registry_agrees() {
    assert_eq!(registry("acc-control").problems(), Vec::<String>::new());
}

#[test]
fn each_wrong_test_is_found_on_its_line() {
    for (name, old, new, what) in [
        ("acc-file", "crates/x/tests/third.rs::", "crates/x/tests/fourth.rs::", "which is not a file of the tree"),
        ("acc-fn", "an_ask_passes_with_its_tests`", "an_ask_that_is_gone`", "names no `fn an_ask_that_is_gone`"),
        ("acc-ignored", "an_ask_passes_with_its_tests`", "a_live_test`", "is `#[ignore]`"),
        ("acc-helper", "an_ask_passes_with_its_tests`", "a_helper`", "has no test attribute"),
        ("acc-script", "`scripts/accept.sh`", "`scripts/gone.sh`", "which is not a file of the tree"),
        ("acc-up", "`scripts/accept.sh`", "`../accept.sh`", "is not a path of the tree"),
        ("acc-span", ", `scripts/accept.sh`", " and scripts/accept.sh", "give each test in a code span"),
        ("acc-id", "| G-10 |", "| T-010 |", "`T-010` is not an ask id"),
        ("acc-twice", "| G-10 |", "| A-X-14 |", "A-X-14 has a row already, on line 7"),
    ] {
        let t = registry(name);
        t.plant("TODO/ACCEPTANCE.md", old, new);
        let p = t.problems();
        assert!(p.iter().any(|x| x.starts_with("TODO/ACCEPTANCE.md:") && x.contains(what)), "{name}: {p:#?}");
    }
}

#[test]
fn a_registry_that_the_configuration_names_must_exist() {
    let t = registry("acc-missing");
    std::fs::remove_file(t.path().join("TODO/ACCEPTANCE.md")).unwrap();
    assert_eq!(t.problems(), ["TODO/ACCEPTANCE.md: the acceptance registry is missing"]);
    t.write("TODO/ACCEPTANCE.md", "# Acceptance\n\nNo table.\n");
    assert_eq!(t.problems(), ["TODO/ACCEPTANCE.md: the registry has no table `| Ask | Tests |`"]);
}

#[test]
fn the_command_prints_the_asks_whose_tests_passed() {
    let t = registry("acc-command");
    let (code, out, err) = t.run(&["acceptance"]);
    assert_eq!((code, out.as_str()), (0, "A-X-14\nG-10\n"), "{err}");
    // The output of `cargo test`, and a script that CI ran.
    t.write(
        "passed.txt",
        "running 2 tests\ntest an_ask_passes_with_its_tests ... ok\ntest unit::a_unit_test_of_the_ask ... FAILED\n\
         test result: FAILED. 1 passed; 1 failed\nscripts/accept.sh\n",
    );
    let passed = t.path().join("passed.txt").display().to_string();
    let (code, out, err) = t.run(&["acceptance", "--passed", &passed]);
    assert_eq!((code, out.as_str()), (0, "A-X-14\n"), "{err}");
    assert!(
        err.contains("G-10 does not pass: no pass of `crates/x/src/lib.rs::unit::a_unit_test_of_the_ask`"),
        "{err}"
    );
    let (code, out, _) = t.run(&["acceptance", "--json", "--passed", &passed]);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "{\n  \"schema\": 1,\n  \"asks\": [\n    {\"ask\": \"A-X-14\", \"tests\": [\"crates/x/tests/third.rs::an_ask_passes_with_its_tests\", \"scripts/accept.sh\"]}\n  ]\n}\n"
    );
}

#[test]
fn the_command_refuses_a_registry_that_breaks_a_rule() {
    let t = registry("acc-refuse");
    t.plant("TODO/ACCEPTANCE.md", "`scripts/accept.sh`", "`scripts/gone.sh`");
    let (code, out, err) = t.run(&["acceptance", "--json"]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.contains("the registry breaks 1 rules"), "{err}");
    assert_eq!(t.run(&["acceptance", "--passed"]).0, 64);
    assert_eq!(t.run(&["acceptance", "--what"]).0, 64);
    let t = Tree::new("acc-no-key");
    let (code, _, err) = t.run(&["acceptance"]);
    assert_eq!(code, 1);
    assert!(err.contains("names no acceptance registry"), "{err}");
}

#[test]
fn with_no_key_a_registry_file_is_an_area_file() {
    // No key: the file is not read as a registry, so a tree that has an
    // area file of the same name keeps it.
    let t = Tree::new("acc-area");
    assert_eq!(t.problems(), Vec::<String>::new());
    assert!(!pod_todo::config::Config::default().record_files().contains(&"ACCEPTANCE.md".to_string()));
}
