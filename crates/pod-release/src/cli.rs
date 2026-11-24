//! The command line `pod-release` (feature `cli`): its exit codes, the one
//! table of its facts, and its manual from the command tree of its parser,
//! read by `pod_manual::clap`. The help of each command and
//! `pod-release man` come from it.

use pod_manual::{Example, Exit, Group, Manual};

use crate::Error;

/// Each check holds.
pub const EXIT_OK: i32 = 0;
/// A usage error of the command line (`pod_core` class `Usage`).
pub const EXIT_USAGE: i32 = 64;
/// A file that the check needs is not there (class `NotFound`).
pub const EXIT_NOT_FOUND: i32 = 66;
/// A file cannot be read (class `Unavailable`).
pub const EXIT_UNAVAILABLE: i32 = 69;
/// A check failed: the signature, the signer, a SHA-256 or the index
/// (class `Refused`).
pub const EXIT_REFUSED: i32 = 77;

/// The exit code of `error`: the code of its class, from the one table of
/// `pod_core`. A setting of the command line that cannot be used is a usage
/// error. The command replaces no binary, so it gives no `CantWrite`; a
/// consumer that does gets 73.
pub fn code(error: &Error) -> i32 {
    match error {
        Error::Config(_) => EXIT_USAGE,
        _ => i32::from(error.class().exit_code()),
    }
}

/// The one line of a failure: the class and the reason. A setting of the
/// command line that cannot be used is a usage error.
pub fn line(error: &Error) -> String {
    let class = if matches!(error, Error::Config(_)) { "usage" } else { error.class().name() };
    format!("pod-release: {class}: {}", error.why())
}

/// The exit codes and the variables of `pod-release`, as facts.
pub fn facts() -> pod_manual::Facts {
    let exits = vec![
        Exit::new(EXIT_OK, "ok", "Each check holds."),
        Exit::new(EXIT_USAGE, "usage", "A usage error of the command line."),
        Exit::new(
            EXIT_NOT_FOUND,
            "notfound",
            "A file is not there: SHA256SUMS, its bundle, a file that you name, or a file of SHA256SUMS with --all.",
        ),
        Exit::new(EXIT_UNAVAILABLE, "unavailable", "A file is there, and pod-release cannot read it."),
        Exit::new(
            EXIT_REFUSED,
            "refused",
            "A check failed: the signature, the signer, the SHA-256 of a file, or the release index.",
        ),
    ];
    pod_manual::Facts { variables: vec![pod_manual::pager::variables()], exits, ..pod_manual::Facts::default() }
}

/// The paragraphs of DESCRIPTION.
const DESCRIPTION: &[&str] = &[
    "pod-release verifies a release of a product of PodUtils in a directory. It reads files only and uses no network. Download the files with your own tool first.",
    "pod-release verify makes three checks in this order. First, the Sigstore bundle SHA256SUMS.sigstore.json of SHA256SUMS, against the signer that you name.",
    "Then the SHA-256 of each file against SHA256SUMS. Then release-index.json against SHA256SUMS, its schema and the signer.",
    "The first failure stops the checks. pod-release writes one line on the error output that names the failure, and exits with the code of its class.",
    "With no FILE, pod-release checks each file of SHA256SUMS that the directory holds, and names the missing files. With --all, a missing file is a failure.",
    "pod-release sums makes the same checks with no signature. A dry run of a release workflow uses it before the workflow signs SHA256SUMS.",
    "pod-release index writes the files of a release index, one line for each file. Verify the release before you use its index.",
];

fn s(t: &str) -> String {
    t.to_string()
}

/// The manual of the command tree `cmd`, the parser of `pod-release`.
pub fn manual(cmd: clap::Command) -> Manual {
    let mut m = pod_manual::clap::read(cmd).with_help_flags();
    m.version = env!("CARGO_PKG_VERSION").to_string();
    // The date of the page: the time of a reproducible build, else none.
    let epoch = option_env!("SOURCE_DATE_EPOCH").and_then(|s| s.trim().parse().ok());
    m.date = epoch.map(pod_manual::model::date_of).unwrap_or_default();
    m.description = DESCRIPTION.iter().map(|p| s(p)).collect();
    m.facts = facts();
    let example = |command: &str, meaning: &str| Example { command: s(command), meaning: s(meaning) };
    m.examples = vec![
        example(
            "pod-release verify --repo PodUtils/podcrates --tag v0.77.0 dist",
            "Verify the release v0.77.0 of podcrates in the directory dist.",
        ),
        example(
            "pod-release verify --repo PodUtils/podcrates --tag v0.77.0 --all dist",
            "Verify each file of the release. A missing file is a failure.",
        ),
        example(
            "pod-release verify --repo PodUtils/podcrates --tag v0.77.0 dist podlab-x86_64-unknown-linux-musl",
            "Verify one file and the index, as an installer does.",
        ),
        example(
            "pod-release verify --identity https://github.com/OWNER/REPO/.github/workflows/release.yml@refs/tags/v1.0.0 dist",
            "Verify a release by the full identity of its signer.",
        ),
        example("pod-release sums --all dist", "Check each file and the index of a dry run, before the signature."),
        example(
            "pod-release index --target x86_64-unknown-linux-musl dist/release-index.json",
            "Write the files of one target from the index.",
        ),
        example("pod-release man --no-pager verify", "Write the section of pod-release verify, with no pager."),
    ];
    m.see_also = vec![
        s("cosign verify-blob: the same check of the bundle, with no check of the files and the index."),
        s("https://github.com/PodUtils/podcrates"),
    ];
    if let Some(root) = m.commands.iter_mut().find(|c| c.path.is_empty()) {
        root.lead = vec![
            example("pod-release verify --repo OWNER/REPO --tag TAG DIR", "Verify a release in a directory."),
            example("pod-release sums DIR", "Check a dry run, before the signature."),
            example("pod-release man", "Read the manual."),
        ];
        let group =
            |title: &str, words: &[&str]| Group { title: s(title), commands: words.iter().map(|w| s(w)).collect() };
        root.groups = vec![
            group("Verify a release", &["verify", "sums"]),
            group("Read a release index", &["index"]),
            group("Read the manual", &["man"]),
        ];
    }
    m
}

/// The documents of `llms.txt`. Their text is not in the binary.
pub fn docs() -> Vec<pod_manual::llms::Doc> {
    vec![pod_manual::llms::Doc {
        path: s("README.md"),
        meaning: s("The release index, the verifier, the update and the command line of pod-release."),
        body: String::new(),
    }]
}

/// The base of the links of `llms.txt`.
pub const DOCS_BASE: &str = "https://github.com/PodUtils/podcrates/blob/main/crates/pod-release/";

#[cfg(test)]
mod tests {
    use super::*;
    use pod_core::failure::Class;

    /// The exit codes are the codes of the classes of `pod_core`.
    #[test]
    fn the_exit_codes_are_the_codes_of_the_classes() {
        for (code, class) in [
            (EXIT_USAGE, Class::Usage),
            (EXIT_NOT_FOUND, Class::NotFound),
            (EXIT_UNAVAILABLE, Class::Unavailable),
            (EXIT_REFUSED, Class::Refused),
        ] {
            assert_eq!(code, i32::from(class.exit_code()), "{class:?}");
        }
        for e in [Error::Unavailable(s("x")), Error::NotFound(s("x")), Error::Refused(s("x")), Error::CantWrite(s("x"))]
        {
            assert_eq!(code(&e), i32::from(e.class().exit_code()), "{e:?}");
        }
        // A binary that could not be replaced is an output that cannot be
        // written (`EX_CANTCREAT`), not a service that is not available.
        assert_eq!((Error::CantWrite(s("x")).class(), code(&Error::CantWrite(s("x")))), (Class::CantCreate, 73));
    }
}
