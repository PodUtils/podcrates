# Acceptance

This page maps each ask of a tracker that podcrates closed to the tests that
prove it. A release passes each ask of this page: the release runs only on
a commit where `ci.yml` is green, and `ci.yml` runs each test below
(`.github/workflows/release.yml`, "The conditions of a release").
`cargo todo acceptance --json` writes the list (`acceptance.json` of each
release, named by `release-index.json`). `cargo todo check` fails on a test
that is not in the tree, that has no test attribute, or that is
`#[ignore]`. `TODO/RULES.md`, "The acceptance registry", gives the form.

| Ask | Tests |
| --- | --- |
| A-X-13 | `crates/pod-release/tests/cli.rs::the_published_release_with_its_index_verifies`, `crates/pod-release/tests/cli.rs::one_changed_byte_of_the_index_is_refused`, `crates/pod-release/tests/cli.rs::another_signer_is_refused` |
| A-X-14 | `crates/pod-todo/tests/acceptance.rs::the_command_prints_the_asks_whose_tests_passed`, `crates/pod-todo/tests/acceptance.rs::each_wrong_test_is_found_on_its_line` |
| A-X-17 | `crates/pod-core/tests/exit_codes.rs::the_table_of_the_readme_is_the_code`, `crates/pod-core/tests/exit_codes.rs::the_log_schema_names_each_class` |
