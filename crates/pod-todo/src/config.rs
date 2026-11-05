//! The facts of one repository that the checker needs: its milestones, its
//! categories, its roadmap, the directories and files that a citation can
//! name, the documents whose ids must resolve, and the heading of the
//! questions for the owner. Each repository writes them in `TODO/todo.toml`.
//! A tree with no such file takes the defaults below.
//!
//! The file is a small subset of TOML: `key = "text"` or `key = ["a", "b"]`
//! (a list can take more lines), and `#` comments. No other form is read, so
//! the checker needs no dependency.
//!
//! The entry points of the crate (`check`, `remap`, the writer) set the
//! configuration of their tree for the thread that runs them; the readers
//! below them take it from there.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use crate::model::Problem;

/// The file of the configuration, from the root of the tree.
pub const FILE: &str = "TODO/todo.toml";

/// The facts of one repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The name in the messages and in the usage text.
    pub tool: String,
    pub milestones: Vec<String>,
    pub categories: Vec<String>,
    /// The roadmap, from the root: its milestones hold their entries.
    pub roadmap: String,
    /// A code span that starts with one of these directories, or that is one
    /// of the root files, is a citation that must exist.
    pub top_dirs: Vec<String>,
    pub root_files: Vec<String>,
    /// The files whose ids must resolve, from the root. A `*` is one name of
    /// a directory (`crates/*/README.md`).
    pub documents: Vec<String>,
    /// The directories whose Markdown files are documents too.
    pub document_dirs: Vec<String>,
    /// The directories of readings at a time (release notes, reports,
    /// studies): their paths and ids are of that time, and are not read.
    pub readings: Vec<String>,
    /// The heading of `TODO/PROGRESS.md` that holds the questions `Qn`.
    pub questions: String,
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

impl Default for Config {
    /// The facts of podssh, the source of this crate.
    fn default() -> Self {
        Self {
            tool: "pod-todo".into(),
            milestones: strings(&["M3", "M4", "M5", "M6", "M7", "M8", "M9", "M10", "backlog", "none"]),
            categories: strings(&["defect", "feature", "measurement", "release", "chore", "research", "docs"]),
            roadmap: "docs/ROADMAP.md".into(),
            top_dirs: strings(&["crates", "scripts", "docs", ".github", "TODO", "vendor"]),
            root_files: strings(&["Cargo.toml", "Cargo.lock", "README.md", "AGENTS.md", "SECURITY.md", "LICENSE"]),
            documents: strings(&["README.md", "AGENTS.md", "SECURITY.md"]),
            document_dirs: strings(&["TODO", "docs"]),
            readings: strings(&["docs/releases/"]),
            questions: "Questions for the operator".into(),
        }
    }
}

impl Config {
    /// The configuration of the tree at `root`: its file, or the defaults
    /// when it has none. A file that cannot be read gives a problem.
    pub fn load(root: &Path) -> Result<Config, Problem> {
        match std::fs::read_to_string(root.join(FILE)) {
            Ok(text) => parse(&text).map_err(|(line, what)| Problem::new(FILE, line, what)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(Problem::new(FILE, 0, format!("cannot be read: {e}"))),
        }
    }

    /// The files of `TODO/` that hold no entries.
    pub fn record_files(&self) -> Vec<String> {
        let mut out = strings(&["INDEX.md", "PROGRESS.md", "RULES.md", "issues.md"]);
        if let Some(name) = self.roadmap.strip_prefix("TODO/") {
            out.push(name.to_string());
        }
        out
    }

    /// True for a path of a reading at a time.
    pub fn is_reading(&self, rel: &str) -> bool {
        self.readings.iter().any(|dir| rel.starts_with(dir.as_str()))
    }

    pub fn has_milestone(&self, m: &str) -> bool {
        self.milestones.iter().any(|x| x == m)
    }
}

/// Read the subset of TOML of [`FILE`]. An error names its line.
pub fn parse(text: &str) -> Result<Config, (usize, String)> {
    let mut c = Config::default();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let no = i + 1;
        let line = strip_comment(lines[i]).trim().to_string();
        i += 1;
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err((no, format!("`{line}` is not `key = value`")));
        };
        let (key, mut value) = (key.trim().to_string(), value.trim().to_string());
        if value.starts_with('[') {
            while !value.contains(']') {
                let Some(more) = lines.get(i) else {
                    return Err((no, format!("the list of `{key}` has no `]`")));
                };
                value.push(' ');
                value.push_str(strip_comment(more).trim());
                i += 1;
            }
        }
        match key.as_str() {
            "tool" => c.tool = text_value(&value).ok_or((no, format!("`{key}` takes a text in quotes")))?,
            "roadmap" => c.roadmap = text_value(&value).ok_or((no, format!("`{key}` takes a text in quotes")))?,
            "questions" => c.questions = text_value(&value).ok_or((no, format!("`{key}` takes a text in quotes")))?,
            _ => {
                let list = list_value(&value).ok_or((no, format!("`{key}` takes a list of texts in quotes")))?;
                match key.as_str() {
                    "milestones" => c.milestones = list,
                    "categories" => c.categories = list,
                    "top_dirs" => c.top_dirs = list,
                    "root_files" => c.root_files = list,
                    "documents" => c.documents = list,
                    "document_dirs" => c.document_dirs = list,
                    "readings" => c.readings = list,
                    _ => return Err((no, format!("`{key}` is not a key of {FILE}"))),
                }
            }
        }
    }
    if c.milestones.is_empty() || c.categories.is_empty() {
        return Err((0, "`milestones` and `categories` need one value or more".into()));
    }
    Ok(c)
}

/// A line without its comment. A `#` inside quotes is text.
fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            '#' if !quoted => return &line[..i],
            _ => {}
        }
    }
    line
}

/// `"text"`, with no escape.
fn text_value(value: &str) -> Option<String> {
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    (!inner.contains(['"', '\\'])).then(|| inner.to_string())
}

/// `["a", "b"]`, with an optional comma after the last item.
fn list_value(value: &str) -> Option<Vec<String>> {
    let inner = value.strip_prefix('[')?.strip_suffix(']')?.trim();
    let mut out = Vec::new();
    for item in inner.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        out.push(text_value(item)?);
    }
    Some(out)
}

thread_local! {
    static CURRENT: RefCell<Rc<Config>> = RefCell::new(Rc::new(Config::default()));
}

/// The configuration of the tree that the thread reads now.
pub fn current() -> Rc<Config> {
    CURRENT.with(|c| c.borrow().clone())
}

/// While it lives, the thread reads with a configuration; then the one
/// before it comes back.
pub struct Scope {
    before: Rc<Config>,
}

impl Drop for Scope {
    fn drop(&mut self) {
        let before = self.before.clone();
        CURRENT.with(|c| *c.borrow_mut() = before);
    }
}

/// Read with the configuration of the tree at `root`. A file that cannot be
/// read is a problem, and the defaults stand in for it.
pub fn enter(root: &Path) -> (Scope, Option<Problem>) {
    let (config, problem) = match Config::load(root) {
        Ok(c) => (c, None),
        Err(p) => (Config::default(), Some(p)),
    };
    let before = CURRENT.with(|c| std::mem::replace(&mut *c.borrow_mut(), Rc::new(config)));
    (Scope { before }, problem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_sets_each_key() {
        let c = parse(
            "# The facts.\ntool = \"podnet-todo\"\nmilestones = [\"M0\", \"M1\",\n  \"M2\", # the last\n]\n\
             categories = [\"chore\"]\nroadmap = \"TODO/ROADMAP.md\"\nquestions = \"Questions for the owner\"\n\
             readings = [\"docs/releases/\", \"docs/research/\"]\n",
        )
        .unwrap();
        assert_eq!(c.tool, "podnet-todo");
        assert_eq!(c.milestones, ["M0", "M1", "M2"]);
        assert_eq!(c.categories, ["chore"]);
        assert_eq!(c.record_files().last().map(String::as_str), Some("ROADMAP.md"));
        assert!(c.is_reading("docs/research/x.md") && !c.is_reading("docs/design.md"));
        assert_eq!(c.questions, "Questions for the owner");
        assert_eq!(c.top_dirs, Config::default().top_dirs);
    }

    #[test]
    fn a_wrong_file_names_its_line() {
        assert_eq!(parse("tool = \"x\"\nmilstones = [\"M0\"]\n").unwrap_err().0, 2);
        assert_eq!(parse("tool = x\n").unwrap_err().0, 1);
        assert_eq!(parse("milestones = [\"M0\"\n").unwrap_err().0, 1);
        assert!(parse("milestones = []\n").is_err());
    }

    #[test]
    fn a_scope_gives_back_the_configuration_before_it() {
        let dir = std::env::temp_dir().join(format!("pod-todo-config-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("TODO")).unwrap();
        std::fs::write(dir.join(FILE), "tool = \"inner\"\n").unwrap();
        {
            let (_scope, problem) = enter(&dir);
            assert!(problem.is_none());
            assert_eq!(current().tool, "inner");
        }
        assert_eq!(current().tool, "pod-todo");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
