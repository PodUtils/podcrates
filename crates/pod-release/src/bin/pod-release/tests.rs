//! The checks of pod-manual over the manual of `pod-release`: the text
//! rules, the parser, the generated files, the variables and the exit
//! codes.

use std::path::{Path, PathBuf};

use clap::CommandFactory as _;

use pod_manual::check::{
    ParserFlag, assert_none, examples, exit_codes, flags, generated, help, model, texts, variables,
};
use pod_manual::{Manual, Rule, llms, roff, scan};

fn manual() -> Manual {
    super::manual("pod-release")
}

fn dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The generated files of the repository, each with its text now.
fn generated_files() -> Vec<(PathBuf, String)> {
    let m = manual().for_repository();
    vec![
        (dir().join("man/pod-release.1"), roff::render(&m)),
        (dir().join("man/llms.txt"), llms::index(&m, &pod_release::cli::docs(), pod_release::cli::DOCS_BASE)),
    ]
}

const WRITE: &str = "run cargo test -p pod-release --features cli -- --ignored write_the_generated_files";

#[test]
fn each_text_of_the_manual_keeps_the_rules() {
    let m = manual();
    assert_none(texts(&m));
    assert_none(model(&m));
    assert_eq!(m.commands.len(), 5, "{:?}", m.commands.iter().map(|c| c.name()).collect::<Vec<_>>());
}

/// The model and the parser of clap agree. The binary answers `-h` and
/// `--help` of each command itself, from the model.
#[test]
fn the_manual_agrees_with_the_parser() {
    let parser = pod_manual::clap::parser(super::Cli::command());
    assert_none(flags(&manual(), |path: &[String]| {
        let mut c = parser(path)?;
        c.flags.push(ParserFlag { spellings: vec!["-h".into(), "--help".into()], takes_value: false });
        Some(c)
    }));
}

#[test]
fn the_generated_files_equal_the_model() {
    for (path, want) in generated_files() {
        assert_none(generated(&path, &want, WRITE));
    }
}

/// Writes the generated files from the model.
#[test]
#[ignore = "it writes the files of the repository"]
fn write_the_generated_files() {
    std::fs::create_dir_all(dir().join("man")).unwrap();
    for (path, want) in generated_files() {
        std::fs::write(&path, want).unwrap();
    }
}

/// Each variable that the code reads or sets is a fact, and each fact is
/// in the code; the pager of pod-manual reads its own.
#[test]
fn each_variable_of_the_code_is_a_fact() {
    let mut sources = scan::rust_sources(&[scan::src(env!("CARGO_MANIFEST_DIR"))]);
    sources.push((PathBuf::from("pod-manual/src/pager.rs"), scan::non_test(pod_manual::pager::SOURCE)));
    assert_none(variables(&manual(), &sources, &[]));
}

/// Each exit code that the code gives has a fact, and each fact of one code
/// is a code that it gives.
#[test]
fn each_exit_code_has_a_fact() {
    let given: Vec<i32> =
        scan::exit_constants(include_str!("../../cli.rs"), "EXIT_").into_iter().map(|(_, n)| n).collect();
    assert_eq!(given, [0, 64, 66, 69, 77]);
    assert_none(exit_codes(&manual(), &given));
}

/// The parser takes each example of the manual.
#[test]
fn the_parser_takes_each_example() {
    let parse = |args: &[String]| {
        let argv = std::iter::once("pod-release".to_string()).chain(args.iter().cloned());
        super::Cli::command().try_get_matches_from(argv).map(drop).map_err(|e| e.to_string())
    };
    assert_none(examples(&manual(), parse));
}

/// The help of each command, as the binary answers `-h`.
#[test]
fn the_help_of_each_command_comes_from_the_model() {
    let m = manual();
    assert_none(help(&m));
    let root = m.root().expect("the root");
    assert!(root.groups.len() >= 3 && root.lead.len() >= 3, "{root:?}");
}

/// The rules of the signer and of the files are in the manual: one signer
/// is required, `--tag` and `--repo` need each other, the workflow is of
/// `--repo` and the issuer of `--identity`, and `--all` excludes the names
/// of files.
#[test]
fn the_rules_of_the_manual_are_the_rules_of_the_parser() {
    let m = manual();
    let rules = |name: &str| m.commands.iter().find(|c| c.name() == name).map(|c| c.rules.clone()).unwrap();
    let verify = rules("verify");
    let needs =
        |flag: &str, need: &str| Rule::Needs { flag: flag.into(), value: None, needs: vec![need.into()], one: false };
    for rule in [
        Rule::OneOf { flags: vec!["--repo".into(), "--identity".into()], required: true },
        needs("--repo", "--tag"),
        needs("--tag", "--repo"),
        Rule::Excludes { flags: vec!["--identity".into(), "--workflow".into()] },
        Rule::Excludes { flags: vec!["--issuer".into(), "--repo".into()] },
        Rule::Excludes { flags: vec!["--all".into(), "FILE".into()] },
    ] {
        assert!(verify.contains(&rule), "no {rule:?} in {verify:?}");
    }
    assert_eq!(verify.len(), 6, "{verify:?}");
    assert!(rules("sums").contains(&Rule::Excludes { flags: vec!["--all".into(), "FILE".into()] }));
}
