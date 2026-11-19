//! `pod-release`: the verifier of a release directory on the command line,
//! for the CI of a product and for an installer.

use std::io::Write;
use std::path::PathBuf;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use pod_release::cli::{EXIT_OK, EXIT_USAGE, code, line};
use pod_release::index::{self, Index};
use pod_release::sigstore::GITHUB_ISSUER;
use pod_release::verify::{self, SUMS, Which};
use pod_release::{Error, Identity, Roots, Sums};

/// A line on the standard output. A closed pipe (`pod-release index | head`)
/// ends the output, not the process with a panic.
macro_rules! out {
    ($($t:tt)*) => {{
        let _ = writeln!(std::io::stdout(), $($t)*);
    }};
}

/// Verify a release of a product of PodUtils in a directory: the signature
/// of SHA256SUMS, each file and the release index.
// The help and the manual come from the model of pod-manual: the parser of
// clap gives no help of its own.
#[derive(Parser)]
#[command(name = "pod-release", version, disable_help_flag = true, disable_help_subcommand = true)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Verify the signature of SHA256SUMS, then each file, then the release
    /// index.
    #[command(group(clap::ArgGroup::new("signer").required(true).args(["repo", "identity"])))]
    Verify {
        /// The repository of the release on GitHub, as OWNER/REPO. The
        /// signer is its release workflow at the tag.
        #[arg(long, value_name = "OWNER/REPO", requires = "tag")]
        repo: Option<String>,
        /// The tag of the release, as v0.77.0.
        #[arg(long, value_name = "TAG", requires = "repo")]
        tag: Option<String>,
        /// The file name of the release workflow in .github/workflows.
        #[arg(long, value_name = "FILE", conflicts_with = "identity", default_value = "release.yml")]
        workflow: String,
        /// The URI that the certificate of the signer must name. Use it in
        /// place of --repo and --tag.
        #[arg(long, value_name = "URI")]
        identity: Option<String>,
        /// The issuer of the OIDC token of the signer.
        #[arg(long, value_name = "URL", conflicts_with = "repo", default_value = GITHUB_ISSUER)]
        issuer: String,
        #[command(flatten)]
        files: Files,
    },
    /// Check each file and the release index against SHA256SUMS, with no
    /// signature: the check of a dry run.
    Sums {
        #[command(flatten)]
        files: Files,
    },
    /// Read a release index, and write one line for each file: its name,
    /// its target, its size and its SHA-256.
    Index {
        /// Write only the files of this target triple. A target with no file
        /// gives exit 66.
        #[arg(long, value_name = "TRIPLE")]
        target: Option<String>,
        /// The release index, release-index.json.
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
    /// The manual: each command, exit code and variable. A pager shows it
    /// on a terminal.
    Man {
        /// Write the text with no pager, also on a terminal.
        #[arg(long)]
        no_pager: bool,
        /// Write the manual as a man(7) page.
        #[arg(long, conflicts_with = "json")]
        roff: bool,
        /// Write the manual as one JSON object.
        #[arg(long)]
        json: bool,
        /// One section: a command, a topic, a table of facts.
        #[arg(value_name = "SECTION")]
        section: Vec<String>,
    },
}

/// The directory and the files of a check.
#[derive(clap::Args)]
struct Files {
    /// Each file of SHA256SUMS must be in the directory.
    #[arg(long, conflicts_with = "file")]
    all: bool,
    /// The directory that holds SHA256SUMS and the files.
    #[arg(value_name = "DIR")]
    dir: PathBuf,
    /// Check only these files of the directory, and the index when the
    /// directory holds it. Each must be in SHA256SUMS.
    #[arg(value_name = "FILE")]
    file: Vec<String>,
}

impl Files {
    fn which(&self) -> Which<'_> {
        match (self.all, self.file.is_empty()) {
            (true, _) => Which::All,
            (false, true) => Which::Present,
            (false, false) => Which::Named(&self.file),
        }
    }
}

/// The signer that the flags of `verify` name, or the text of a usage
/// error.
fn signer(
    repo: Option<String>,
    tag: Option<String>,
    workflow: &str,
    uri: Option<String>,
    issuer: String,
) -> Result<Identity, String> {
    use pod_manual::error::fault;
    let https = |flag: &str, text: &str| {
        let ok = text.starts_with("https://") && !text.chars().any(|c| c.is_whitespace() || c.is_control());
        if ok { Ok(()) } else { Err(fault(flag, &format!("{text:?} is not an https:// URI"), "give an https:// URI")) }
    };
    https("--issuer", &issuer)?;
    match (repo, tag, uri) {
        (Some(repo), Some(tag), None) => {
            let part = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
            if !matches!(repo.split_once('/'), Some((owner, name)) if part(owner) && part(name)) {
                return Err(fault(
                    "--repo",
                    &format!("{repo:?} is not OWNER/REPO"),
                    "give the repository as OWNER/REPO",
                ));
            }
            if !pod_release::version::valid_tag(&tag) {
                return Err(fault(
                    "--tag",
                    &format!("{tag:?} is not a tag"),
                    "give the tag of the release, as v0.77.0",
                ));
            }
            if !index::plain_name(workflow) {
                return Err(fault(
                    "--workflow",
                    &format!("{workflow:?} is not a file name"),
                    "give the file name alone",
                ));
            }
            Ok(Identity::github(&repo, workflow, &tag))
        }
        (None, None, Some(uri)) => https("--identity", &uri).map(|()| Identity { uri, issuer }),
        _ => Err(fault("verify", "the signer is not given", "give --repo and --tag, or --identity")),
    }
}

/// The lines of the files that a check read and of the missing files.
fn print_found(files: &[String], missing: &[String], index: Option<&Index>) {
    for name in files {
        match index.filter(|_| name == index::FILE) {
            Some(i) => out!("ok {name} ({} {}, {} files)", i.project, i.version, i.files.len()),
            None => out!("ok {name}"),
        }
    }
    for name in missing {
        out!("missing {name}");
    }
}

fn run(cmd: Cmd) -> Result<(), Error> {
    match cmd {
        Cmd::Verify { repo, tag, workflow, identity, issuer, files } => {
            let who = signer(repo, tag, &workflow, identity, issuer).map_err(Error::Config)?;
            let checked = verify::verify_files(&files.dir, &who, &Roots::sigstore(), files.which())?;
            print_found(&checked.files, &checked.missing, checked.index.as_ref());
            out!("verified: {checked}; signed by {} of {}", who.uri, who.issuer);
        }
        Cmd::Sums { files } => {
            let bytes = verify::read_file(&files.dir.join(SUMS))?;
            let sums = Sums::parse(&bytes).map_err(Error::Refused)?;
            let found = verify::check_files(&files.dir, &sums, files.which())?;
            print_found(&found.files, &found.missing, found.index.as_ref());
            out!(
                "checked with no signature: {SUMS} {}; {} files checked, {} missing",
                pod_release::hex(&pod_release::sha256(&bytes)),
                found.files.len(),
                found.missing.len()
            );
        }
        Cmd::Index { target, file } => {
            let text = String::from_utf8(verify::read_file(&file)?)
                .map_err(|_| Error::Refused("the release index is not text".into()))?;
            let index = index::read(&text).map_err(Error::Refused)?;
            let files: Vec<_> = index.files.iter().filter(|f| target.is_none() || f.target == target).collect();
            if let (Some(target), true) = (&target, files.is_empty()) {
                return Err(Error::NotFound(format!("the release index names no file of the target {target}")));
            }
            for f in files {
                out!("{} {} {} {}", f.name, f.target.as_deref().unwrap_or("-"), f.size, f.sha256);
            }
        }
        Cmd::Man { .. } => unreachable!("main answers man"),
    }
    Ok(())
}

/// The manual of this binary, as the name of `argv[0]` shows it.
fn manual(argv0: &str) -> pod_manual::Manual {
    pod_release::cli::manual(Cli::command()).named(argv0)
}

fn main() {
    std::process::exit(start());
}

fn start() -> i32 {
    let args: Vec<String> = std::env::args_os().map(|a| a.to_string_lossy().into_owned()).collect();
    let manual = manual(args.first().map(String::as_str).unwrap_or("pod-release"));
    if let Some(help) = pod_manual::help::request(&manual, args.get(1..).unwrap_or(&[])) {
        return match pod_manual::help::answer(&manual, &help) {
            Ok(text) => {
                let _ = std::io::stdout().write_all(text.as_bytes());
                EXIT_OK
            }
            Err(e) => {
                eprintln!("{e}");
                EXIT_USAGE
            }
        };
    }
    let cli = match Cli::command().try_get_matches().and_then(|m| Cli::from_arg_matches(&m)) {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return if e.use_stderr() { EXIT_USAGE } else { EXIT_OK };
        }
    };
    if let Cmd::Man { no_pager, roff, json, section } = cli.command {
        let section = (!section.is_empty()).then(|| section.join(" "));
        let req = pod_manual::Request { section, no_pager, roff, json };
        let (mut out, mut err) = (std::io::stdout().lock(), std::io::stderr());
        let tty = pod_manual::pager::Tty::probed();
        return match pod_manual::run(&manual, &req, tty, &mut std::io::stdin().lock(), &mut out, &mut err) {
            Ok(()) => EXIT_OK,
            Err(e) => {
                eprintln!("{e}");
                EXIT_USAGE
            }
        };
    }
    match run(cli.command) {
        Ok(()) => EXIT_OK,
        Err(e) => {
            eprintln!("{}", line(&e));
            code(&e)
        }
    }
}

// The root of a binary keeps its modules beside it: the path names the
// directory of the binary.
#[cfg(test)]
#[path = "pod-release/tests.rs"]
mod tests;
