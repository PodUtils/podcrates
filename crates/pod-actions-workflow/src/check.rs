//! The check of `on:` as GitHub checks a workflow file: each event is one
//! that GitHub knows, each key of an event is one that the event takes
//! (the documents "Events that trigger workflows" and "Workflow syntax"),
//! no filter has its `-ignore` form beside it, and each filter and pattern
//! reads. [`Workflow::check`] gives each fault, not only the first, so
//! that a caller can print them all; `parse` and `takes` do not change.

use crate::error::{Error, ErrorKind};
use crate::model::Workflow;
use crate::on::{filter, strings};
use crate::yaml::{Kind, Node};

/// The keys that an event of `on:` takes, by its name; `None` for an event
/// that GitHub does not know. `schedule` takes a list, not keys.
pub fn event_keys(event: &str) -> Option<&'static [&'static str]> {
    const TYPES: &[&str] = &["types"];
    const NONE: &[&str] = &[];
    Some(match event {
        "push" => &["branches", "branches-ignore", "tags", "tags-ignore", "paths", "paths-ignore"],
        "pull_request" | "pull_request_target" => &["types", "branches", "branches-ignore", "paths", "paths-ignore"],
        "merge_group" => &["types", "branches", "branches-ignore"],
        "workflow_run" => &["types", "workflows", "branches", "branches-ignore"],
        "workflow_call" => &["inputs", "outputs", "secrets"],
        "workflow_dispatch" => &["inputs"],
        "image_version" => &["names", "versions"],
        "branch_protection_rule"
        | "check_run"
        | "check_suite"
        | "discussion"
        | "discussion_comment"
        | "issue_comment"
        | "issues"
        | "label"
        | "milestone"
        | "pull_request_review"
        | "pull_request_review_comment"
        | "registry_package"
        | "release"
        | "repository_dispatch"
        | "watch" => TYPES,
        "create" | "delete" | "deployment" | "deployment_status" | "fork" | "gollum" | "page_build" | "public"
        | "status" | "schedule" => NONE,
        _ => return None,
    })
}

/// The filters that hold patterns, each with its `-ignore` form.
const FILTERS: [&str; 3] = ["branches", "tags", "paths"];

impl Workflow {
    /// Each fault of `on:` that GitHub refuses, in the order of the file;
    /// none for a file that GitHub takes. A fault is an event that GitHub
    /// does not know, a key that its event does not take (`tags` on
    /// `pull_request`), a filter beside its `-ignore` form, a filter of `!`
    /// patterns only, a bad pattern, or a value of another kind. The values
    /// of `types`, and the inputs, outputs and secrets of `workflow_call`
    /// and `workflow_dispatch`, are not checked here.
    pub fn check(&self) -> Vec<Error> {
        let mut faults = Vec::new();
        let kind = || ErrorKind::Type("an event, a list of events or a mapping of events");
        match &self.on.kind {
            Kind::String(name) => known(name, "on", &self.on, &mut faults),
            Kind::Seq(items) => {
                for (i, item) in items.iter().enumerate() {
                    match &item.kind {
                        Kind::String(name) => known(name, &format!("on[{i}]"), item, &mut faults),
                        _ => faults.push(Error::new(format!("on[{i}]"), Some(item.mark), kind())),
                    }
                }
            }
            Kind::Map(entries) => {
                for (key, value) in entries {
                    event(&key.name, key.mark, value, &mut faults);
                }
            }
            _ => faults.push(Error::new("on", Some(self.on.mark), kind())),
        }
        faults
    }
}

/// An event that GitHub does not know is a fault.
fn known(name: &str, path: &str, node: &Node, faults: &mut Vec<Error>) {
    if event_keys(name).is_none() {
        faults.push(Error::new(path, Some(node.mark), ErrorKind::UnknownEvent(name.to_string())));
    }
}

/// The faults of the event `name` and its value.
fn event(name: &str, mark: crate::yaml::Mark, value: &Node, faults: &mut Vec<Error>) {
    let path = format!("on.{name}");
    let Some(takes) = event_keys(name) else {
        faults.push(Error::new(path, Some(mark), ErrorKind::UnknownEvent(name.to_string())));
        return;
    };
    if name == "schedule" {
        return schedule(value, &path, faults);
    }
    let entries = match &value.kind {
        Kind::Null => return,
        Kind::Map(entries) => entries,
        _ => return faults.push(Error::new(path, Some(value.mark), ErrorKind::Type("null or a mapping of keys"))),
    };
    for (key, _) in entries {
        if !takes.contains(&key.name.as_str()) {
            let event = name.to_string();
            let kind = ErrorKind::KeyNotTaken { event, key: key.name.clone() };
            faults.push(Error::new(&path, Some(key.mark), kind));
        }
    }
    for f in FILTERS {
        // A filter that the event does not take is a fault above.
        if takes.contains(&f)
            && let Err(e) = filter(value, &path, f)
        {
            faults.push(e);
        }
    }
    for key in ["types", "workflows", "names", "versions"] {
        if let (true, Some(node)) = (takes.contains(&key), value.get(key))
            && let Err(e) = strings(node, &format!("{path}.{key}"), "a string or a list of strings")
        {
            faults.push(e);
        }
    }
}

/// `schedule` is a list of mappings, each with `cron` and no other key.
fn schedule(value: &Node, path: &str, faults: &mut Vec<Error>) {
    let Kind::Seq(items) = &value.kind else {
        return faults.push(Error::new(path, Some(value.mark), ErrorKind::Type("a list of `cron` entries")));
    };
    for (i, item) in items.iter().enumerate() {
        let here = format!("{path}[{i}]");
        let Kind::Map(entries) = &item.kind else {
            faults.push(Error::new(here, Some(item.mark), ErrorKind::Type("a mapping with `cron`")));
            continue;
        };
        for (key, cron) in entries {
            if key.name != "cron" {
                let kind = ErrorKind::KeyNotTaken { event: "schedule".into(), key: key.name.clone() };
                faults.push(Error::new(&here, Some(key.mark), kind));
            } else if !matches!(cron.kind, Kind::String(_)) {
                faults.push(Error::new(format!("{here}.cron"), Some(cron.mark), ErrorKind::Type("a cron line")));
            }
        }
        if item.get("cron").is_none() {
            faults.push(Error::new(here, Some(item.mark), ErrorKind::Missing("cron")));
        }
    }
}
