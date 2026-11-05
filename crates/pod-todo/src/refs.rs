//! The references of the record: each `T-NNN` that a document names, each
//! repository path and line that an entry cites, the work order, the
//! questions that blocked entries wait for, and the milestones of the
//! roadmap. The configuration of the tree (`crate::config`) names the
//! roadmap, the documents and the paths that a citation can name.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::check::{PROGRESS, Record, read};
use crate::model::{Problem, ids_in};
use crate::parse::{code_spans, doc_section, lines_with_fences};

pub fn check(root: &Path, r: &Record, p: &mut Vec<Problem>) {
    let status: HashMap<&str, &str> = r.rows.iter().map(|row| (row.id.as_str(), row.status.as_str())).collect();
    for rel in documents(root) {
        let Some(text) = read(root, &rel) else { continue };
        for (id, line) in ids_in(&text) {
            if !status.contains_key(id.as_str()) {
                p.push(Problem::new(&rel, line, format!("{id} is not an entry of the index")));
            }
        }
        if rel.starts_with("TODO/") {
            citations(root, &rel, &text, p);
            crate::numbers::check(root, &rel, &text, p);
        }
    }
    work_order(r, &status, p);
    questions(r, p);
    roadmap(root, r, p);
}

/// The Markdown files whose `T-NNN` references must resolve, and whose
/// citations `remap` moves: the documents and the document directories of
/// the configuration, without its readings at a time.
pub fn documents(root: &Path) -> Vec<String> {
    let config = crate::config::current();
    let mut out = Vec::new();
    for pattern in &config.documents {
        match pattern.split_once("/*/") {
            Some((dir, rest)) => {
                let Ok(read) = fs::read_dir(root.join(dir)) else { continue };
                for e in read.flatten() {
                    out.push(format!("{dir}/{}/{rest}", e.file_name().to_string_lossy()));
                }
            }
            None => out.push(pattern.clone()),
        }
    }
    for dir in &config.document_dirs {
        walk(root, dir, &mut out);
    }
    out.retain(|rel| root.join(rel).is_file() && !config.is_reading(rel));
    out.sort();
    out.dedup();
    out
}

fn walk(root: &Path, rel: &str, out: &mut Vec<String>) {
    let Ok(dir) = fs::read_dir(root.join(rel)) else { return };
    for e in dir.filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().into_owned();
        let child = format!("{rel}/{name}");
        if e.path().is_dir() {
            walk(root, &child, out);
        } else if name.ends_with(".md") {
            out.push(child);
        }
    }
}

/// A code span that cites a path of this repository: the path, and the last
/// line it names (`path:N` or `path:N-M`).
pub fn citation(span: &str) -> Option<(&str, Option<usize>)> {
    if span.is_empty() || span.contains(char::is_whitespace) || span.contains(['*', '{', '}', '<', '>', '?', '[', '|'])
    {
        return None;
    }
    let (path, suffix) = match span.split_once(':') {
        Some((p, s)) => (p, Some(s)),
        None => (span, None),
    };
    let path = path.split('#').next().unwrap_or(path);
    let first = path.split('/').next().unwrap_or("");
    // Another project's file is written `owner/repo:path`, which never
    // starts with a top directory of this tree.
    let config = crate::config::current();
    let repo_path = (config.top_dirs.iter().any(|d| d == first) && path.contains('/'))
        || config.root_files.iter().any(|f| f == path);
    if !repo_path {
        return None;
    }
    let line = suffix.and_then(|s| {
        let last = s.rsplit('-').next()?;
        let ok = s.split('-').all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        ok.then(|| last.parse().ok()).flatten()
    });
    Some((path, line))
}

/// Each repository path in a code span of a record file exists, with the
/// exact case of each name, and has the line that it names. A citation that
/// quotes its line (`` `FILE:N` says "TEXT" ``) holds that text there.
fn citations(root: &Path, rel: &str, text: &str, p: &mut Vec<Problem>) {
    let lines = lines_with_fences(text);
    for (k, &(no, line, fenced)) in lines.iter().enumerate() {
        if fenced {
            continue;
        }
        for span in code_spans(line) {
            let Some((path, last)) = citation(span) else { continue };
            if !exists_exactly(root, path.trim_end_matches('/')) {
                p.push(Problem::new(rel, no, format!("`{path}` does not exist in this repository")));
                continue;
            }
            let Some(n) = last else { continue };
            let first: usize =
                span.rsplit(':').next().and_then(|s| s.split('-').next()).and_then(|s| s.parse().ok()).unwrap_or(n);
            // A range that ends before it starts, or a line 0, names no line:
            // a half-moved range looks like this.
            if first == 0 || first > n {
                p.push(Problem::new(
                    rel,
                    no,
                    format!("`{span}`: a range runs from line 1 or later to a line at or after its start"),
                ));
                continue;
            }
            let cited = fs::read_to_string(root.join(path)).unwrap_or_default();
            let count = cited.lines().count();
            if n > count {
                p.push(Problem::new(rel, no, format!("`{span}`: {path} has {count} lines")));
                continue;
            }
            let Some(quote) = quote_after(line, span, &lines[k + 1..]) else { continue };
            let (a, b) = (first.max(1), n.max(1));
            let held: Vec<&str> = cited.lines().skip(a - 1).take(b.saturating_sub(a) + 1).collect();
            if !squash(&held.join(" ")).contains(&squash(&quote)) {
                p.push(Problem::new(
                    rel,
                    no,
                    format!("`{span}` says \"{quote}\", but the cited lines do not hold that text"),
                ));
            }
        }
    }
}

/// The text that a citation quotes: `` `SPAN` says "TEXT" ``, where TEXT
/// can go on over the next two lines of the paragraph.
fn quote_after(line: &str, span: &str, next: &[(usize, &str, bool)]) -> Option<String> {
    let at = line.find(&format!("`{span}` says \""))? + span.len() + 9;
    let mut quote = line[at..].to_string();
    for &(_, more, fenced) in next.iter().take(2) {
        if quote.contains('"') || fenced || more.trim().is_empty() {
            break;
        }
        quote.push(' ');
        quote.push_str(more.trim());
    }
    let end = quote.find('"')?;
    Some(quote[..end].to_string())
}

/// A text with each run of white space made one space.
fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// True when each part of `rel` exists with exactly that name, so that a
/// path with the wrong case fails on Windows as it fails on Linux.
fn exists_exactly(root: &Path, rel: &str) -> bool {
    let mut at = root.to_path_buf();
    for part in rel.split('/') {
        let Ok(dir) = fs::read_dir(&at) else { return false };
        if !dir.filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy() == part) {
            return false;
        }
        at.push(part);
    }
    true
}

/// The work order names only entries that are not done.
fn work_order(r: &Record, status: &HashMap<&str, &str>, p: &mut Vec<Problem>) {
    let Some((head, body)) = doc_section(&r.progress, "Work order") else {
        p.push(Problem::new(PROGRESS, 0, "no `## Work order` section"));
        return;
    };
    let mut named = 0;
    for (no, line) in body {
        for (id, _) in ids_in(line) {
            named += 1;
            if status.get(id.as_str()) == Some(&"done") {
                p.push(Problem::new(PROGRESS, no, format!("the work order names {id}, which is done")));
            }
        }
    }
    if named == 0 && r.rows.iter().any(|row| row.status != "done") {
        p.push(Problem::new(PROGRESS, head, "the work order names no entry, but some entries are not done"));
    }
}

/// Each question `Qn` that a `## Blocker` names is in the progress record's
/// questions heading of the configuration.
fn questions(r: &Record, p: &mut Vec<Problem>) {
    let heading = crate::config::current().questions.clone();
    let asked: HashSet<String> = doc_section(&r.progress, &heading)
        .map(|(_, body)| body.iter().flat_map(|(_, l)| q_tokens(l)).collect())
        .unwrap_or_default();
    for e in &r.entries {
        let Some(b) = e.section("Blocker") else { continue };
        for q in q_tokens(&b.body) {
            if !asked.contains(&q) {
                p.push(Problem::new(
                    &format!("TODO/{}", e.file),
                    b.line,
                    format!("{}: the blocker names {q}, which is not in {PROGRESS}, `## {heading}`", e.id),
                ));
            }
        }
    }
}

/// The words `Q1`, `Q2`, ... of a text.
fn q_tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() >= 2 && w.starts_with('Q') && w[1..].bytes().all(|b| b.is_ascii_digit()))
        .map(str::to_string)
        .collect()
}

/// The roadmap keeps no list of open items (`- [ ]`): open work is an
/// entry. An entry that it names under a milestone has that milestone.
fn roadmap(root: &Path, r: &Record, p: &mut Vec<Problem>) {
    let config = crate::config::current();
    let roadmap = config.roadmap.as_str();
    // A missing roadmap must not pass as a roadmap with nothing wrong in it.
    let Some(text) = read(root, roadmap) else {
        p.push(Problem::new(
            roadmap,
            0,
            "the roadmap is missing; the milestones of the entries are checked against it",
        ));
        return;
    };
    let milestone: HashMap<&str, &str> = r.rows.iter().map(|row| (row.id.as_str(), row.milestone.as_str())).collect();
    let mut current: Option<String> = None;
    for (no, line, fenced) in lines_with_fences(&text) {
        if fenced {
            continue;
        }
        if let Some(h) = line.strip_prefix("## ") {
            // A done milestone (M0 to M2) may name the later entry that
            // finished its exit criteria; only an open milestone is checked.
            let m = h.split([':', ' ']).next().unwrap_or("");
            current = config.has_milestone(m).then(|| m.to_string());
            continue;
        }
        if line.trim_start().starts_with("- [ ]") {
            p.push(Problem::new(
                roadmap,
                no,
                "an open item `- [ ]`: open work is an entry in TODO/, named here by its id",
            ));
        }
        let Some(m) = &current else { continue };
        for (id, _) in ids_in(line) {
            if let Some(have) = milestone.get(id.as_str())
                && have != m
            {
                p.push(Problem::new(roadmap, no, format!("{id} is listed under {m}, but its milestone is {have}")));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_paths_of_this_repository_are_citations() {
        assert_eq!(citation("crates/podssh-cli/src/tree.rs:52"), Some(("crates/podssh-cli/src/tree.rs", Some(52))));
        assert_eq!(citation("crates/x.rs:36-38"), Some(("crates/x.rs", Some(38))));
        assert_eq!(citation("docs/cli.md#the-manual"), Some(("docs/cli.md", None)));
        assert_eq!(citation("Cargo.toml"), Some(("Cargo.toml", None)));
        assert_eq!(citation("crates/podssh-cli/src/flags.rs:VERBS"), Some(("crates/podssh-cli/src/flags.rs", None)));
        assert_eq!(citation("lablup/bssh:crates/bssh-russh-sftp/x.patch"), None);
        assert_eq!(citation("src/noise.rs:1-92"), None);
        assert_eq!(citation("podssh ssh -G"), None);
        assert_eq!(citation("crates/podssh-ts/src/{pipe,node}.rs"), None);
        assert_eq!(citation("crates"), None);
    }

    #[test]
    fn questions_are_words() {
        assert_eq!(q_tokens("question Q1 in `TODO/PROGRESS.md` (Q12); not QA, not Q"), ["Q1", "Q12"]);
    }
}
