//! The rule that a done defect names its test (`defect_tests` of
//! `TODO/todo.toml`): the reader and the writer refuse a done defect from
//! the date of the key whose Done section names no test; with no key, or
//! before the date, nothing changes. T-003 of the control record is a done
//! defect of 2026-10-08 whose Done names no test.

mod common;

use common::Tree;

const NO_TEST: &str = "T-003: a done defect names no test in its Done section";

fn with_key(name: &str, date: &str) -> Tree {
    let t = Tree::new(name);
    t.write("TODO/todo.toml", &format!("defect_tests = \"{date}\"\n"));
    t
}

#[test]
fn a_done_defect_from_the_date_with_no_test_is_found() {
    let t = with_key("defects-found", "2026-10-08");
    assert_eq!(t.problems(), [format!("TODO/area.md:86: {NO_TEST}")]);
}

#[test]
fn with_no_key_or_before_the_date_nothing_changes() {
    assert_eq!(Tree::new("defects-no-key").problems(), Vec::<String>::new());
    assert_eq!(with_key("defects-before", "2026-10-09").problems(), Vec::<String>::new());
}

#[test]
fn a_test_in_either_form_satisfies_the_rule() {
    for (name, named) in [
        ("defects-path", "`crates/x/tests/third.rs` passed."),
        ("defects-function", "`third::a_wait_is_bounded` passed."),
    ] {
        let t = with_key(name, "2026-10-08");
        t.plant("TODO/area.md", "`cargo test -p x -- third` passed.", named);
        assert_eq!(t.problems(), Vec::<String>::new(), "{named}");
    }
}

#[test]
fn an_entry_of_another_category_is_not_read() {
    let t = with_key("defects-feature", "2026-10-08");
    t.plant(
        "TODO/area.md",
        "**Category:** defect\n**Milestone:** M3\n**Priority:** P1\n**Effort:** S\n**Status:** done",
        "**Category:** feature\n**Milestone:** M3\n**Priority:** P1\n**Effort:** S\n**Status:** done",
    );
    t.plant("TODO/INDEX.md", "| P1 | S | M3 | defect | done |", "| P1 | S | M3 | feature | done |");
    assert_eq!(t.problems(), Vec::<String>::new());
}

#[test]
fn set_refuses_done_for_a_defect_that_names_no_test() {
    // From a date after T-003's: the rule reads T-001 alone.
    let t = with_key("defects-set", "2026-11-01");
    let done = "\n## Done\n\n2026-11-20, commit `1111111`: the wait is bounded.\n";
    t.plant(
        "TODO/area.md",
        "# a comment in a fence, not an entry\n```\n",
        &format!("# a comment in a fence, not an entry\n```\n{done}"),
    );
    let (code, _, err) = t.run(&["set", "T-001", "done"]);
    assert_eq!(code, 1);
    assert!(err.contains("T-001: a done defect names no test in its Done section"), "{err}");
    assert!(t.read("TODO/area.md").contains("**Status:** open"), "nothing was written");
    t.plant("TODO/area.md", "the wait is bounded.", "`cargo test -p x --test third` passes `x::a_wait_is_bounded`.");
    t.plant("TODO/PROGRESS.md", "1. T-001.", "1. T-002.");
    let (code, out, err) = t.run(&["set", "T-001", "done"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(t.read("TODO/area.md").contains("**Status:** done"), "the status moved");
}
