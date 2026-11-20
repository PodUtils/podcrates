//! The acceptance registry (`acceptance` of `TODO/todo.toml`): each ask id
//! of a tracker (`A-X-14`, `G-10`) with the tests that prove it, so that a
//! release can say which asks it passes. The registry is one table of the
//! file, `| Ask | Tests |`; each test is a code span:
//!
//! * `PATH::NAME` (or `PATH::MODULE::NAME`): a test function `NAME` of the
//!   file `PATH`, with a test attribute and no `#[ignore]`;
//! * `PATH`: a file that CI runs as a test (a script, a workflow).
//!
//! `check` holds each test to the tree. `acceptance` prints the asks, or,
//! with `--passed FILE`, the asks whose tests the file names as passed (the
//! output of `cargo test`, or one name a line), as text or as JSON.

use std::collections::HashSet;
use std::path::Path;

use crate::block::json_string;
use crate::check::read;
use crate::model::Problem;
use crate::parse::{cells, lines_with_fences};

/// One ask of the registry, with its tests and its line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub id: String,
    pub tests: Vec<String>,
    pub line: usize,
}

/// The problems of the registry of the configuration, when it has one.
pub fn check(root: &Path, p: &mut Vec<Problem>) {
    if let Some(rel) = crate::config::current().acceptance.clone() {
        read_registry(root, &rel, p);
    }
}

/// Read the registry at `rel`: its asks, and each problem of its form and
/// of its tests.
pub fn read_registry(root: &Path, rel: &str, p: &mut Vec<Problem>) -> Vec<Ask> {
    let Some(text) = read(root, rel) else {
        p.push(Problem::new(rel, 0, "the acceptance registry is missing"));
        return Vec::new();
    };
    let lines = lines_with_fences(&text);
    let Some(head) = lines.iter().position(|(_, l, fenced)| !fenced && cells(l) == ["Ask", "Tests"]) else {
        p.push(Problem::new(rel, 0, "the registry has no table `| Ask | Tests |`"));
        return Vec::new();
    };
    let mut asks: Vec<Ask> = Vec::new();
    for &(no, line, _) in lines[head + 1..].iter().skip(1).take_while(|(_, l, _)| l.starts_with('|')) {
        let row = cells(line);
        let [id, tests] = row.as_slice() else {
            p.push(Problem::new(rel, no, "a row of the registry has two cells: the ask and its tests"));
            continue;
        };
        if !is_ask(id) {
            p.push(Problem::new(rel, no, format!("`{id}` is not an ask id (as `A-X-14`)")));
        }
        if let Some(first) = asks.iter().find(|a| a.id == *id) {
            p.push(Problem::new(rel, no, format!("{id} has a row already, on line {}", first.line)));
        }
        let parts: Vec<&str> = tests.split('`').collect();
        let spans: Vec<String> = parts.iter().skip(1).step_by(2).map(|s| s.to_string()).collect();
        let loose = parts.iter().step_by(2).any(|s| s.contains(|c: char| c != ',' && !c.is_whitespace()));
        if loose || parts.len().is_multiple_of(2) || spans.is_empty() {
            p.push(Problem::new(rel, no, format!("{id}: give each test in a code span, with commas between")));
        }
        for test in &spans {
            if let Some(what) = test_gap(root, test) {
                p.push(Problem::new(rel, no, format!("{id}: `{test}` {what}")));
            }
        }
        asks.push(Ask { id: id.clone(), tests: spans, line: no });
    }
    asks
}

/// An ask id: a capital letter first, then letters, digits and `-`, with a
/// `-`; an id of an entry (`T-NNN`) is not one.
fn is_ask(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_uppercase())
        && s.contains('-')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !crate::model::is_id(s)
}

/// What is wrong with one test of the registry, if anything.
fn test_gap(root: &Path, test: &str) -> Option<String> {
    let (path, name) = match test.split_once("::") {
        Some((path, rest)) => (path, Some(rest)),
        None => (test, None),
    };
    let safe = !path.is_empty() && !path.starts_with('/') && !path.contains('\\');
    if !safe || path.split('/').any(|c| c == ".." || c == "." || c.is_empty()) {
        return Some("is not a path of the tree".into());
    }
    if !root.join(path).is_file() {
        return Some(format!("names `{path}`, which is not a file of the tree"));
    }
    let rest = name?;
    let word = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !rest.split("::").all(word) {
        return Some("is not `PATH::NAME`".into());
    }
    let name = rest.rsplit("::").next().unwrap_or(rest);
    let text = read(root, path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let Some(at) = lines.iter().position(|l| declares(l, name)) else {
        return Some(format!("names no `fn {name}` of `{path}`"));
    };
    let attributes: Vec<&str> = lines[..at]
        .iter()
        .rev()
        .map(|l| l.trim())
        .take_while(|l| l.starts_with("#[") || l.starts_with("//"))
        .filter(|l| l.starts_with("#["))
        .collect();
    if attributes.iter().any(|a| a.starts_with("#[ignore")) {
        return Some("is `#[ignore]`: the CI of a release does not run it".into());
    }
    if !attributes.iter().any(|a| a.contains("test")) {
        return Some("has no test attribute".into());
    }
    None
}

/// True when `line` declares the function `name`: `fn name(` or `fn name<`
/// after the qualifiers of a function.
fn declares(line: &str, name: &str) -> bool {
    let Some(at) = line.find(&format!("fn {name}")) else { return false };
    let before = &line[..at];
    let after = &line[at + 3 + name.len()..];
    (before.is_empty() || before.ends_with(' ')) && (after.starts_with('(') || after.starts_with('<'))
}

/// The names of the tests that passed, from the output of `cargo test`
/// (`test NAME ... ok`) or from one name a line (a path of a script, or
/// `PATH::NAME`).
#[derive(Debug, Default)]
pub struct Passed {
    lines: HashSet<String>,
    names: HashSet<String>,
}

impl Passed {
    pub fn of(text: &str) -> Passed {
        let mut p = Passed::default();
        for line in text.lines().map(str::trim) {
            if let Some(name) = line.strip_prefix("test ").and_then(|l| l.strip_suffix(" ... ok")) {
                p.names.insert(name.rsplit("::").next().unwrap_or(name).to_string());
            } else if !line.is_empty() && !line.starts_with("test ") {
                p.lines.insert(line.to_string());
            }
        }
        p
    }

    /// True when the test of the registry passed: its own line, or, for
    /// `PATH::NAME`, a test of `cargo test` whose last name is NAME.
    pub fn has(&self, test: &str) -> bool {
        self.lines.contains(test)
            || test
                .split_once("::")
                .is_some_and(|(_, rest)| self.names.contains(rest.rsplit("::").next().unwrap_or(rest)))
    }
}

/// The asks as the JSON of `acceptance --json`.
pub fn json(asks: &[&Ask]) -> String {
    let items: Vec<String> = asks
        .iter()
        .map(|a| {
            let tests: Vec<String> = a.tests.iter().map(|t| json_string(t)).collect();
            format!("    {{\"ask\": {}, \"tests\": [{}]}}", json_string(&a.id), tests.join(", "))
        })
        .collect();
    let body = if items.is_empty() { String::new() } else { format!("\n{}\n  ", items.join(",\n")) };
    format!("{{\n  \"schema\": 1,\n  \"asks\": [{body}]\n}}\n")
}

/// `acceptance [--json] [--passed FILE]`: the asks of the registry, or the
/// asks whose tests passed. Each ask that did not pass goes to `err`.
pub fn command(root: &Path, words: &[&str], out: &mut dyn std::io::Write, err: &mut dyn std::io::Write) -> i32 {
    let tool = crate::config::current().tool.clone();
    let (mut as_json, mut file) = (false, None);
    let mut it = words.iter();
    while let Some(w) = it.next() {
        match (*w, file) {
            ("--json", _) => as_json = true,
            ("--passed", None) => match it.next() {
                Some(f) => file = Some(*f),
                None => return usage(err, &tool),
            },
            _ => return usage(err, &tool),
        }
    }
    let Some(rel) = crate::config::current().acceptance.clone() else {
        let _ = writeln!(err, "{tool}: {} names no acceptance registry (`acceptance = \"FILE\"`)", crate::config::FILE);
        return 1;
    };
    let mut problems = Vec::new();
    let asks = read_registry(root, &rel, &mut problems);
    if !problems.is_empty() {
        problems.iter().for_each(|p| drop(writeln!(err, "{p}")));
        let _ = writeln!(err, "{tool}: the registry breaks {} rules", problems.len());
        return 1;
    }
    let passed = match file.map(std::fs::read_to_string).transpose() {
        Ok(text) => text.map(|t| Passed::of(&t)),
        Err(e) => {
            let _ = writeln!(err, "{tool}: --passed: {e}");
            return 1;
        }
    };
    let mut pass = Vec::new();
    for a in &asks {
        let failed: Vec<&String> = a.tests.iter().filter(|t| passed.as_ref().is_some_and(|p| !p.has(t))).collect();
        if failed.is_empty() {
            pass.push(a);
        } else {
            let names: Vec<String> = failed.iter().map(|t| format!("`{t}`")).collect();
            let _ = writeln!(err, "{tool}: {} does not pass: no pass of {}", a.id, names.join(", "));
        }
    }
    if as_json {
        let _ = write!(out, "{}", json(&pass));
    } else {
        pass.iter().for_each(|a| drop(writeln!(out, "{}", a.id)));
    }
    0
}

fn usage(err: &mut dyn std::io::Write, tool: &str) -> i32 {
    let _ = writeln!(err, "{tool}: acceptance takes --json and --passed FILE\n{}", crate::usage_text());
    64
}
