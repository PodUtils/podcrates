//! The mechanical checks that a program calls from its own tests. Each
//! returns its findings; [`assert_none`] fails a test with each of them.
//!
//! * [`texts`]: the text rules over each string of the model, the facts and
//!   the tables, and over the help and the text that the writers make.
//! * [`generated`]: a file of the repository (a man page, `llms.txt`)
//!   equals the text that the model gives now.
//! * [`variables`]: each variable that the source reads is a fact, and each
//!   fact is in the source.
//! * [`exit_codes`]: each exit code that the program gives has a fact, and
//!   each fact of one code is a code that it gives.
//! * [`flags`]: each command and flag of the model against the parser.
//! * [`examples`]: each example of the program against its parser.
//! * [`help`]: the help of each command, as `-h` answers it, holds its
//!   flags, its groups and its "Start here" lines.
//! * [`model`]: each rule names flags of its command, a need of one of
//!   names two or more, and each name selects one section.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::model::{Manual, Rule};
use crate::rules::Kind;

mod examples;
mod walk;

pub use examples::examples;
pub use walk::texts;

/// One finding of a check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// Where: a command, a fact, a file.
    pub at: String,
    pub what: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.at, self.what)
    }
}

pub(crate) fn finding(at: impl Into<String>, what: impl Into<String>) -> Finding {
    Finding { at: at.into(), what: what.into() }
}

/// Fail with each finding, one on each line.
#[track_caller]
pub fn assert_none(findings: Vec<Finding>) {
    if !findings.is_empty() {
        let lines: Vec<String> = findings.iter().map(Finding::to_string).collect();
        panic!("{} findings:\n{}", lines.len(), lines.join("\n"));
    }
}

/// The findings of the text rules in one text. With no kind, the text is a
/// term or a command line: no limit of words.
pub fn text(at: &str, text: &str, kind: Option<Kind>) -> Vec<Finding> {
    let faults = match kind {
        Some(k) => crate::rules::faults(text, k),
        None => {
            let mut f = crate::rules::char_faults(text);
            f.extend(crate::rules::work_ids(text).into_iter().map(crate::rules::Fault::WorkId));
            f
        }
    };
    faults.into_iter().map(|f| finding(at, f.to_string())).collect()
}

/// The file at `path` against `want`, the text that the model gives now.
/// `fix` is the instruction that writes the file again.
pub fn generated(path: &Path, want: &str, fix: &str) -> Vec<Finding> {
    let at = path.display().to_string();
    let have = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return vec![finding(at, crate::error::fault("the file", &format!("cannot be read: {e}"), fix))],
    };
    let have = String::from_utf8_lossy(&have);
    if have == want {
        return Vec::new();
    }
    let (n, h, w) = have
        .lines()
        .map(Some)
        .chain(std::iter::repeat(None))
        .zip(want.lines().map(Some).chain(std::iter::repeat(None)))
        .enumerate()
        .find(|(_, (h, w))| h != w)
        .map(|(n, (h, w))| (n + 1, h.unwrap_or("(the end)"), w.unwrap_or("(the end)")))
        .unwrap_or((have.lines().count() + 1, "(a different end)", "(the end)"));
    let what = format!("line {n} differs from the model: the file has {h:?}, the model gives {w:?}");
    vec![finding(at, crate::error::fault("the file", &what, fix))]
}

/// Each variable that `sources` read, against the facts of `manual`, both
/// ways. The variable of each flag and argument of the model (clap's
/// `env`) is a variable that the parser reads: it needs a fact, and it
/// holds a fact. `exempt` holds names that need no fact (a variable that
/// only a build reads), and names of facts that no source holds (a
/// variable that a library reads). Give [`crate::pager::SOURCE`] in
/// `sources` when the program pages its manual.
pub fn variables(manual: &Manual, sources: &[(PathBuf, String)], exempt: &[&str]) -> Vec<Finding> {
    let facts: Vec<&str> =
        manual.facts.variables.iter().flat_map(|g| &g.vars).flat_map(|v| &v.names).map(String::as_str).collect();
    let parsed: Vec<(String, &str)> = manual
        .commands
        .iter()
        .flat_map(|c| {
            let at = format!("{} {}", manual.name, c.name()).trim().to_string();
            let envs =
                c.flags.iter().filter_map(|f| f.env.as_deref()).chain(c.args.iter().filter_map(|a| a.env.as_deref()));
            envs.map(move |e| (at.clone(), e))
        })
        .collect();
    let mut out = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for (at, name) in &parsed {
        if !facts.contains(name) && !exempt.contains(name) && !seen.contains(name) {
            seen.push(name);
            out.push(finding(at, format!("the parser reads {name}, and no fact names it")));
        }
    }
    for (path, code) in sources {
        for name in crate::scan::variables_read(code) {
            if !facts.contains(&name.as_str()) && !exempt.contains(&name.as_str()) {
                out.push(finding(path.display().to_string(), format!("the code reads {name}, and no fact names it")));
            }
        }
    }
    for name in facts {
        let quoted = format!("\"{name}\"");
        let held = parsed.iter().any(|(_, e)| *e == name) || sources.iter().any(|(_, code)| code.contains(&quoted));
        if !exempt.contains(&name) && !held {
            out.push(finding("the facts", format!("{name} is a fact, and no source reads or sets it")));
        }
    }
    out
}

/// Each exit code in `given` (the codes that the program gives) against the
/// facts, both ways: a code with no row fails, and a row of one code that
/// the program does not give fails. A row of [`crate::Code::Payload`]
/// names the codes of a payload, not the codes of the program itself: it
/// covers no code of `given`.
pub fn exit_codes(manual: &Manual, given: &[i32]) -> Vec<Finding> {
    let exits = &manual.facts.exits;
    let mut out = Vec::new();
    for &code in given {
        if !exits.iter().any(|e| e.code.holds(code)) {
            out.push(finding("the exit codes", format!("the program gives {code}, and no fact names it")));
        }
    }
    for e in exits {
        if let crate::facts::Code::Value(n) = e.code
            && !given.contains(&n)
        {
            out.push(finding("the exit codes", format!("the fact of {n} names a code that the program does not give")));
        }
    }
    out
}

/// What the parser of a program knows of one command.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParserCommand {
    pub flags: Vec<ParserFlag>,
    /// The names of its subcommands, with no alias.
    pub subcommands: Vec<String>,
}

/// One flag of the parser.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParserFlag {
    /// `-x` and `--long`.
    pub spellings: Vec<String>,
    pub takes_value: bool,
}

/// Each command and flag of the model against the parser. `parser` gives
/// the command at a path, or `None` when the parser has no such command.
pub fn flags(manual: &Manual, parser: impl Fn(&[String]) -> Option<ParserCommand>) -> Vec<Finding> {
    let mut out = Vec::new();
    for c in &manual.commands {
        let at = format!("{} {}", manual.name, c.name()).trim().to_string();
        let Some(p) = parser(&c.path) else {
            out.push(finding(at, "the parser has no such command"));
            continue;
        };
        for f in &c.flags {
            let mut want = f.spellings();
            want.sort();
            match p.flags.iter().find(|pf| f.spellings().iter().any(|s| pf.spellings.contains(s))) {
                None => {
                    out.push(finding(&at, format!("{} is in the model, and the parser has no such flag", f.term())))
                }
                Some(pf) => {
                    let mut have = pf.spellings.clone();
                    have.sort();
                    if have != want {
                        out.push(finding(&at, format!("{} is {} in the parser", f.term(), have.join(", "))));
                    }
                    if pf.takes_value != f.value.is_some() {
                        let says = if pf.takes_value { "takes a value" } else { "takes no value" };
                        out.push(finding(&at, format!("{} {says} in the parser", f.term())));
                    }
                }
            }
        }
        for pf in &p.flags {
            if !c.flags.iter().any(|f| f.spellings().iter().any(|s| pf.spellings.contains(s))) {
                out.push(finding(
                    &at,
                    format!("the parser has {}, and the model has no such flag", pf.spellings.join(", ")),
                ));
            }
        }
        let children: Vec<String> = manual.children(&c.path).iter().filter_map(|k| k.path.last().cloned()).collect();
        for s in &p.subcommands {
            if !children.contains(s) {
                out.push(finding(&at, format!("the parser has the command {s}, and the model has none")));
            }
        }
    }
    out
}

/// The help of each command of `manual`, as `COMMAND --help` answers it:
/// the request selects the command, and the text holds each flag of the
/// command, the title of each group and each command of its lead.
pub fn help(manual: &Manual) -> Vec<Finding> {
    let mut out = Vec::new();
    for c in &manual.commands {
        let at = format!("{} {}", manual.name, c.name());
        let at = at.trim();
        let mut args = c.path.clone();
        args.push("--help".into());
        let text = match crate::help::request(manual, &args) {
            Some(req) if req.path == c.path => crate::help::answer(manual, &req),
            _ => Err("--help does not select this command".into()),
        };
        let text = match text {
            Ok(text) => text,
            Err(why) => {
                out.push(finding(at, format!("the help: {why}")));
                continue;
            }
        };
        let terms = c.flags.iter().map(|f| f.term());
        let titles = c.groups.iter().map(|g| g.title.clone());
        for term in terms.chain(titles).chain(c.lead.iter().map(|e| e.command.clone())) {
            if !text.contains(&term) {
                out.push(finding(at, format!("the help has no {term}")));
            }
        }
    }
    out
}

/// The rules of the model itself: each rule names flags or arguments of
/// its command; a need of one of names two or more; the groups of a command name each command under it once,
/// and no other; each name of a section selects one section.
pub fn model(manual: &Manual) -> Vec<Finding> {
    let mut out = Vec::new();
    for c in &manual.commands {
        let at = format!("{} {}", manual.name, c.name());
        let at = at.trim();
        for r in &c.rules {
            for name in r.names() {
                if c.flag(name).is_none() && c.arg(name).is_none() {
                    out.push(finding(at, format!("the rule names {name}, which is no flag and no argument")));
                }
            }
            // A need of one of a single name is a need of that name: the
            // sentence "needs one of" must name a choice.
            if let Rule::Needs { flag, needs, one: true, .. } = r
                && needs.len() < 2
            {
                out.push(finding(at, format!("the rule of {flag} needs one of less than two names; give two or more")));
            }
        }
        // Each exit code of the model is a row of the table of exit codes.
        let exits = &manual.facts.exits;
        let mut codes: Vec<(String, i32)> = Vec::new();
        if let crate::model::Availability::Feature { code, .. } = c.availability {
            codes.push(("the availability".into(), code));
        }
        for f in &c.flags {
            if let crate::model::FlagKind::NotInRelease { code, .. } = f.kind {
                codes.push((f.spelling(), code));
            }
        }
        for (what, code) in codes.into_iter().filter(|_| !exits.is_empty()) {
            if !crate::facts::covers(exits, code) {
                out.push(finding(at, format!("{what} exits {code}, which no row of EXIT STATUS names")));
            }
        }
        for k in &c.topics {
            if !manual.topics.iter().any(|t| t.key == *k) {
                out.push(finding(at, format!("the topic {k} is no topic of the manual")));
            }
            let owners = manual.commands.iter().filter(|o| o.topics.contains(k)).count();
            if owners > 1 {
                out.push(finding(at, format!("the topic {k} is inside {owners} commands")));
            }
        }
        if c.groups.is_empty() {
            continue;
        }
        let children: Vec<String> = manual.children(&c.path).iter().filter_map(|k| k.path.last().cloned()).collect();
        let listed: Vec<&String> = c.groups.iter().flat_map(|g| &g.commands).collect();
        for w in &children {
            match listed.iter().filter(|l| **l == w).count() {
                1 => {}
                0 => out.push(finding(at, format!("the command {w} is in no group"))),
                n => out.push(finding(at, format!("the command {w} is in {n} groups"))),
            }
        }
        for l in listed.iter().filter(|l| !children.contains(l)) {
            out.push(finding(at, format!("a group names {l}, which is no command under this one")));
        }
    }
    let all = crate::sections::sections(manual);
    let mut seen: Vec<String> = Vec::new();
    for s in &all {
        for name in std::iter::once(&s.key).chain(&s.aliases) {
            let name = name.to_ascii_lowercase();
            if seen.contains(&name) {
                out.push(finding("the sections", format!("{name} names two sections")));
            }
            seen.push(name);
        }
        let back = crate::sections::find(manual, &all, &s.key);
        if back.map(|b| &b.key) != Some(&s.key) {
            out.push(finding("the sections", format!("{} does not select its own section", s.key)));
        }
    }
    out
}

#[cfg(test)]
mod tests;
