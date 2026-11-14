//! The events of `on:` with their filters, and the decision whether a
//! workflow takes an event, by the rules of GitHub's "Workflow syntax"
//! (`on.<event>.types`, `branches`, `tags`, `paths` and their `-ignore`
//! forms, `workflows` of `workflow_run`). The other keys of an event
//! (`inputs`, the list of `schedule`) are not filters here.

use crate::error::{Error, ErrorKind};
use crate::model::Workflow;
use crate::pattern::Pattern;
use crate::yaml::{Kind, Mark, Node};

/// An event to decide on. `None` is a value that the caller does not know:
/// its filter does not apply.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Trigger<'a> {
    /// The name of the event: `push`, `pull_request`, ...
    pub event: &'a str,
    /// The activity type (the `types` value), as `opened`.
    pub action: Option<&'a str>,
    /// `refs/heads/NAME` or `refs/tags/NAME`; another text is a branch
    /// name. For a pull request, the base branch.
    pub git_ref: Option<&'a str>,
    /// The changed files, from the root of the repository.
    pub paths: Option<&'a [String]>,
    /// For `workflow_run`: the name of the workflow that ran.
    pub workflow: Option<&'a str>,
}

impl<'a> Trigger<'a> {
    /// An event with no action, no ref and no paths.
    pub fn new(event: &'a str) -> Trigger<'a> {
        Trigger { event, ..Trigger::default() }
    }
}

/// The events of `on:`, in the order of the file.
#[derive(Clone, Debug, PartialEq)]
pub struct On {
    pub events: Vec<Event>,
}

/// One event of `on:` and its filters.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub name: String,
    pub mark: Mark,
    /// `types`; none takes each type, except for `pull_request` and
    /// `pull_request_target` ([`DEFAULT_PR_TYPES`]).
    pub types: Option<Vec<String>>,
    /// `branches` or `branches-ignore`.
    pub branches: Option<Filter>,
    /// `tags` or `tags-ignore`.
    pub tags: Option<Filter>,
    /// `paths` or `paths-ignore`.
    pub paths: Option<Filter>,
    /// `workflows` (of `workflow_run`): the names of the workflows, each
    /// matched whole and with case.
    pub workflows: Option<Vec<String>>,
}

/// The types of a pull request event with no `types`, as in GitHub.
pub const DEFAULT_PR_TYPES: [&str; 3] = ["opened", "synchronize", "reopened"];

/// The patterns of one filter, in their order.
#[derive(Clone, Debug, PartialEq)]
pub struct Filter {
    /// The `-ignore` form: a name that matches is left out.
    pub ignore: bool,
    pub patterns: Vec<Pattern>,
}

impl Filter {
    /// The patterns hold `name`: the last pattern that matches decides, so
    /// a later `!` removes a match and a later pattern adds it again.
    pub fn matches(&self, name: &str) -> bool {
        let mut hit = false;
        for p in &self.patterns {
            if p.matches(name) {
                hit = !p.negated;
            }
        }
        hit
    }

    /// The filter lets `name` through.
    pub fn takes(&self, name: &str) -> bool {
        self.matches(name) != self.ignore
    }
}

enum Ref<'a> {
    Branch(&'a str),
    Tag(&'a str),
}

fn split_ref(git_ref: &str) -> Ref<'_> {
    if let Some(tag) = git_ref.strip_prefix("refs/tags/") {
        Ref::Tag(tag)
    } else {
        Ref::Branch(git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref))
    }
}

impl Event {
    /// The event takes `t` (whose name is this event's).
    pub fn takes(&self, t: &Trigger) -> bool {
        if let Some(action) = t.action {
            let types_ok = match &self.types {
                Some(types) => types.iter().any(|x| x == action),
                None if matches!(self.name.as_str(), "pull_request" | "pull_request_target") => {
                    DEFAULT_PR_TYPES.contains(&action)
                }
                None => true,
            };
            if !types_ok {
                return false;
            }
        }
        if let (Some(names), Some(workflow)) = (&self.workflows, t.workflow)
            && !names.iter().any(|n| n == workflow)
        {
            return false;
        }
        let mut tag = false;
        if let Some(git_ref) = t.git_ref {
            // With a filter of one kind of ref only, the other kind does
            // not run; with neither, each ref runs.
            let ok = match split_ref(git_ref) {
                Ref::Branch(name) => self.branches.as_ref().map_or(self.tags.is_none(), |f| f.takes(name)),
                Ref::Tag(name) => {
                    tag = true;
                    self.tags.as_ref().map_or(self.branches.is_none(), |f| f.takes(name))
                }
            };
            if !ok {
                return false;
            }
        }
        // GitHub reads no path filter for a push of a tag.
        match (&self.paths, t.paths) {
            (Some(filter), Some(files)) if !tag => files.iter().any(|f| filter.takes(f)),
            _ => true,
        }
    }
}

impl On {
    /// Read `on:`: an event name, a list of names, or a mapping of each
    /// event to null or to its filters.
    pub fn read(on: &Node) -> Result<On, Error> {
        let type_error = |node: &Node, path: &str| {
            Error::new(path, Some(node.mark), ErrorKind::Type("an event, a list of events or a mapping of events"))
        };
        let mut events = Vec::new();
        match &on.kind {
            Kind::String(name) => events.push(Event::bare(name, on.mark)),
            Kind::Seq(items) => {
                for (i, item) in items.iter().enumerate() {
                    let Kind::String(name) = &item.kind else { return Err(type_error(item, &format!("on[{i}]"))) };
                    events.push(Event::bare(name, item.mark));
                }
            }
            Kind::Map(entries) => {
                for (key, value) in entries {
                    events.push(Event::read(&key.name, key.mark, value, &format!("on.{}", key.name))?);
                }
            }
            _ => return Err(type_error(on, "on")),
        }
        Ok(On { events })
    }

    /// The workflow takes `t`: its event is in `on:` and its filters let
    /// it through.
    pub fn takes(&self, t: &Trigger) -> bool {
        self.events.iter().any(|e| e.name == t.event && e.takes(t))
    }
}

impl Event {
    fn bare(name: &str, mark: Mark) -> Event {
        Event { name: name.to_string(), mark, types: None, branches: None, tags: None, paths: None, workflows: None }
    }

    fn read(name: &str, mark: Mark, node: &Node, path: &str) -> Result<Event, Error> {
        let mut e = Event::bare(name, mark);
        if !matches!(node.kind, Kind::Map(_)) {
            return Ok(e);
        }
        if let Some(types) = node.get("types") {
            let types = strings(types, &format!("{path}.types"), "a type or a list of types")?;
            e.types = Some(types.into_iter().map(|(text, _)| text).collect());
        }
        e.branches = filter(node, path, "branches")?;
        e.tags = filter(node, path, "tags")?;
        e.paths = filter(node, path, "paths")?;
        if let Some(names) = node.get("workflows") {
            let names = strings(names, &format!("{path}.workflows"), "a workflow name or a list of names")?;
            e.workflows = Some(names.into_iter().map(|(text, _)| text).collect());
        }
        Ok(e)
    }
}

/// The filter `key` or `key-ignore` of an event: both are an error, and a
/// filter of `!` patterns only too, as GitHub refuses them.
pub(crate) fn filter(event: &Node, path: &str, key: &'static str) -> Result<Option<Filter>, Error> {
    let ignore_key = format!("{key}-ignore");
    let (node, ignore, name) = match (event.get(key), event.get(&ignore_key)) {
        (Some(_), Some(other)) => {
            return Err(Error::new(path, Some(other.mark), ErrorKind::FilterWithIgnore(key)));
        }
        (Some(node), None) => (node, false, key.to_string()),
        (None, Some(node)) => (node, true, ignore_key),
        (None, None) => return Ok(None),
    };
    let here = format!("{path}.{name}");
    let mut patterns = Vec::new();
    for (text, mark) in strings(node, &here, "a pattern or a list of patterns")? {
        let p = Pattern::parse(&text).map_err(|e| Error { path: here.clone(), mark: Some(mark), ..e })?;
        patterns.push(p);
    }
    if !ignore && patterns.iter().all(|p| p.negated) {
        return Err(Error::new(here, Some(node.mark), ErrorKind::OnlyNegated(key)));
    }
    Ok(Some(Filter { ignore, patterns }))
}

/// A string or a list of strings, each with its place. A number or a
/// boolean is refused: YAML reads `1.0` as a number, so a pattern must be
/// quoted to keep its text.
pub(crate) fn strings(node: &Node, path: &str, what: &'static str) -> Result<Vec<(String, Mark)>, Error> {
    let one = |n: &Node| match &n.kind {
        Kind::String(s) => Ok((s.clone(), n.mark)),
        _ => Err(Error::new(path, Some(n.mark), ErrorKind::Type(what))),
    };
    match &node.kind {
        Kind::Seq(items) => items.iter().map(one).collect(),
        _ => Ok(vec![one(node)?]),
    }
}

impl Workflow {
    /// The events of `on:` with their filters. An error names a value that
    /// GitHub refuses: a filter with its `-ignore` form, a filter of `!`
    /// patterns only, a bad pattern, a value of another kind.
    pub fn triggers(&self) -> Result<On, Error> {
        On::read(&self.on)
    }

    /// The workflow takes the event `t`, as GitHub decides it. Each event
    /// of `on:` is read, so a bad filter of another event is an error too.
    pub fn takes(&self, t: &Trigger) -> Result<bool, Error> {
        Ok(self.triggers()?.takes(t))
    }
}
