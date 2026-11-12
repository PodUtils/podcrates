//! The tree of a file into the model: the keys of GitHub's workflow schema,
//! the ids, the kinds of the values, and the syntax of each template and
//! each `if:`.

use pod_actions_expr::{Condition, is_name};

use crate::error::{Error, ErrorKind, Warning};
use crate::model::{Defaults, Job, Step, Strategy, Workflow};
use crate::yaml::{Key, Kind, Node};

pub(crate) struct Reader {
    pub warnings: Vec<Warning>,
}

fn kind_error(node: &Node, path: &str, what: &'static str) -> Error {
    Error::new(path, Some(node.mark), ErrorKind::Type(what))
}

fn map<'a>(node: &'a Node, path: &str) -> Result<&'a [(Key, Node)], Error> {
    match &node.kind {
        Kind::Map(entries) => Ok(entries),
        _ => Err(kind_error(node, path, "a mapping")),
    }
}

/// The text of a scalar; none for null.
fn text(node: &Node, path: &str) -> Result<Option<String>, Error> {
    match &node.kind {
        Kind::Null => Ok(None),
        Kind::Seq(_) | Kind::Map(_) => Err(kind_error(node, path, "a scalar")),
        _ => Ok(node.text()),
    }
}

/// A node that is not null.
fn some(node: &Node) -> Option<Node> {
    (node.kind != Kind::Null).then(|| node.clone())
}

/// A condition: its text, which must parse.
fn condition(node: &Node, path: &str) -> Result<Option<String>, Error> {
    let Some(text) = text(node, path)? else { return Ok(None) };
    Condition::parse(&text).map_err(|e| Error::new(path, Some(node.mark), ErrorKind::Expression(e)))?;
    Ok(Some(text))
}

fn id(key: &Key, path: &str) -> Result<String, Error> {
    if !is_name(&key.name) {
        return Err(Error::new(path, Some(key.mark), ErrorKind::BadId(key.name.clone())));
    }
    Ok(key.name.clone())
}

impl Reader {
    fn unknown(&mut self, path: &str, key: &Key, value: &Node, extra: &mut Vec<(Key, Node)>) {
        self.warnings.push(Warning { path: path.to_string(), key: key.name.clone(), mark: key.mark });
        extra.push((key.clone(), value.clone()));
    }

    pub(crate) fn workflow(&mut self, root: &Node) -> Result<Workflow, Error> {
        let entries = map(root, "")?;
        let mut on = None;
        let mut jobs = None;
        let mut w = Workflow {
            name: None,
            run_name: None,
            on: Node { kind: Kind::Null, mark: root.mark },
            env: None,
            defaults: None,
            permissions: None,
            concurrency: None,
            jobs: Vec::new(),
            extra: Vec::new(),
        };
        for (key, value) in entries {
            let path = key.name.as_str();
            // `on` is data with no expression; each other value can hold
            // a template, and GitHub refuses a file whose template does not
            // parse.
            if path != "on" {
                value.check_templates(path)?;
            }
            match path {
                "name" => w.name = text(value, path)?,
                "run-name" => w.run_name = text(value, path)?,
                "on" => on = Some(value.clone()),
                "env" => w.env = some(value),
                "defaults" => w.defaults = self.defaults(value, path)?,
                "permissions" => w.permissions = some(value),
                "concurrency" => w.concurrency = some(value),
                "jobs" => jobs = Some(value),
                _ => self.unknown("", key, value, &mut w.extra),
            }
        }
        w.on = on.ok_or(Error::new("", Some(root.mark), ErrorKind::Missing("on")))?;
        let jobs = jobs.ok_or(Error::new("", Some(root.mark), ErrorKind::Missing("jobs")))?;
        let list = map(jobs, "jobs")?;
        if list.is_empty() {
            return Err(kind_error(jobs, "jobs", "a mapping with at least one job"));
        }
        for (key, value) in list {
            let path = format!("jobs.{}", key.name);
            let id = id(key, &path)?;
            w.jobs.push(self.job(id, key, value, &path)?);
        }
        Ok(w)
    }

    fn defaults(&mut self, node: &Node, path: &str) -> Result<Option<Defaults>, Error> {
        if node.kind == Kind::Null {
            return Ok(None);
        }
        let mut d = Defaults { shell: None, working_directory: None, mark: node.mark, extra: Vec::new() };
        for (key, value) in map(node, path)? {
            if key.name != "run" {
                self.unknown(path, key, value, &mut d.extra);
                continue;
            }
            let run_path = format!("{path}.run");
            for (key, value) in map(value, &run_path)? {
                match key.name.as_str() {
                    "shell" => d.shell = text(value, &run_path)?,
                    "working-directory" => d.working_directory = text(value, &run_path)?,
                    _ => self.unknown(&run_path, key, value, &mut d.extra),
                }
            }
        }
        Ok(Some(d))
    }

    fn job(&mut self, id: String, key: &Key, node: &Node, path: &str) -> Result<Job, Error> {
        let mut j = Job {
            id,
            mark: key.mark,
            name: None,
            runs_on: None,
            needs: Vec::new(),
            condition: None,
            strategy: None,
            env: None,
            defaults: None,
            timeout_minutes: None,
            cancel_timeout_minutes: None,
            continue_on_error: None,
            container: None,
            services: None,
            outputs: Vec::new(),
            environment: None,
            permissions: None,
            concurrency: None,
            snapshot: None,
            uses: None,
            with: None,
            secrets: None,
            steps: Vec::new(),
            extra: Vec::new(),
        };
        let mut steps = None;
        for (key, value) in map(node, path)? {
            let here = format!("{path}.{}", key.name);
            match key.name.as_str() {
                "name" => j.name = text(value, &here)?,
                "runs-on" => j.runs_on = some(value),
                "needs" => j.needs = needs(value, &here)?,
                "if" => j.condition = condition(value, &here)?,
                "strategy" => j.strategy = self.strategy(value, &here)?,
                "env" => j.env = some(value),
                "defaults" => j.defaults = self.defaults(value, &here)?,
                "timeout-minutes" => j.timeout_minutes = some(value),
                "cancel-timeout-minutes" => j.cancel_timeout_minutes = some(value),
                "continue-on-error" => j.continue_on_error = some(value),
                "container" => j.container = some(value),
                "services" => j.services = some(value),
                "outputs" => {
                    for (name, out) in map(value, &here)? {
                        let template = text(out, &format!("{here}.{}", name.name))?.unwrap_or_default();
                        j.outputs.push((name.name.clone(), template));
                    }
                }
                "environment" => j.environment = some(value),
                "permissions" => j.permissions = some(value),
                "concurrency" => j.concurrency = some(value),
                "snapshot" => j.snapshot = some(value),
                "uses" => j.uses = text(value, &here)?,
                "with" => j.with = some(value),
                "secrets" => j.secrets = some(value),
                "steps" => steps = Some(value),
                _ => self.unknown(path, key, value, &mut j.extra),
            }
        }
        if j.uses.is_some() {
            if steps.is_some() {
                return Err(Error::new(path, Some(key.mark), ErrorKind::Reusable("steps")));
            }
            if j.runs_on.is_some() {
                return Err(Error::new(path, Some(key.mark), ErrorKind::Reusable("runs-on")));
            }
        } else if j.runs_on.is_none() {
            return Err(Error::new(path, Some(key.mark), ErrorKind::Missing("runs-on")));
        }
        if let Some(steps) = steps {
            let here = format!("{path}.steps");
            let Kind::Seq(items) = &steps.kind else {
                return Err(kind_error(steps, &here, "a sequence"));
            };
            for (i, item) in items.iter().enumerate() {
                let step = self.step(item, &format!("{here}[{i}]"))?;
                if let Some(id) = &step.id
                    && j.steps.iter().any(|s| s.id.as_ref() == Some(id))
                {
                    return Err(Error::new(&here, Some(item.mark), ErrorKind::DuplicateStepId(id.clone())));
                }
                j.steps.push(step);
            }
        }
        Ok(j)
    }

    fn strategy(&mut self, node: &Node, path: &str) -> Result<Option<Strategy>, Error> {
        if node.kind == Kind::Null {
            return Ok(None);
        }
        let mut s = Strategy { matrix: None, fail_fast: None, max_parallel: None, mark: node.mark, extra: Vec::new() };
        for (key, value) in map(node, path)? {
            match key.name.as_str() {
                "matrix" => s.matrix = some(value),
                "fail-fast" => s.fail_fast = some(value),
                "max-parallel" => s.max_parallel = some(value),
                _ => self.unknown(path, key, value, &mut s.extra),
            }
        }
        Ok(Some(s))
    }

    fn step(&mut self, node: &Node, path: &str) -> Result<Step, Error> {
        let mut s = Step {
            id: None,
            name: None,
            condition: None,
            run: None,
            uses: None,
            with: None,
            env: None,
            shell: None,
            working_directory: None,
            continue_on_error: None,
            timeout_minutes: None,
            mark: node.mark,
            extra: Vec::new(),
        };
        for (key, value) in map(node, path)? {
            let here = format!("{path}.{}", key.name);
            match key.name.as_str() {
                "id" => {
                    let Some(text) = text(value, &here)? else { continue };
                    if !is_name(&text) {
                        return Err(Error::new(&here, Some(value.mark), ErrorKind::BadId(text)));
                    }
                    s.id = Some(text);
                }
                "name" => s.name = text(value, &here)?,
                "if" => s.condition = condition(value, &here)?,
                "run" => s.run = text(value, &here)?,
                "uses" => s.uses = text(value, &here)?,
                "with" => s.with = some(value),
                "env" => s.env = some(value),
                "shell" => s.shell = text(value, &here)?,
                "working-directory" => s.working_directory = text(value, &here)?,
                "continue-on-error" => s.continue_on_error = some(value),
                "timeout-minutes" => s.timeout_minutes = some(value),
                _ => self.unknown(path, key, value, &mut s.extra),
            }
        }
        if s.run.is_some() == s.uses.is_some() {
            return Err(Error::new(path, Some(node.mark), ErrorKind::StepKind));
        }
        Ok(s)
    }
}

/// `needs`: one id or a sequence of ids.
fn needs(node: &Node, path: &str) -> Result<Vec<String>, Error> {
    let items: Vec<&Node> = match &node.kind {
        Kind::Null => return Ok(Vec::new()),
        Kind::Seq(items) => items.iter().collect(),
        _ => vec![node],
    };
    let mut out: Vec<String> = Vec::new();
    for item in items {
        match &item.kind {
            Kind::String(s) if !out.contains(s) => out.push(s.clone()),
            Kind::String(_) => {}
            _ => return Err(kind_error(item, path, "a job id or a sequence of job ids")),
        }
    }
    Ok(out)
}
