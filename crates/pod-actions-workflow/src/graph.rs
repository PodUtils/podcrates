//! The graph of `needs`: the edges, a topological order (a tie goes by the
//! order of the file), and the ancestors of a job. A need that is not a job
//! and a cycle are errors by name.

use crate::error::{Error, ErrorKind};
use crate::model::Workflow;

/// The needs of the jobs of a workflow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Graph {
    ids: Vec<String>,
    needs: Vec<Vec<usize>>,
    order: Vec<usize>,
}

impl Graph {
    pub fn new(workflow: &Workflow) -> Result<Graph, Error> {
        let ids: Vec<String> = workflow.jobs.iter().map(|j| j.id.clone()).collect();
        let mut needs = Vec::with_capacity(ids.len());
        for job in &workflow.jobs {
            let mut list = Vec::new();
            for need in &job.needs {
                let Some(i) = ids.iter().position(|id| id == need) else {
                    let path = format!("jobs.{}.needs", job.id);
                    return Err(Error::new(path, Some(job.mark), ErrorKind::UnknownNeed(need.clone())));
                };
                list.push(i);
            }
            needs.push(list);
        }
        let mut placed = vec![false; ids.len()];
        let mut order = Vec::with_capacity(ids.len());
        while order.len() < ids.len() {
            let next = (0..ids.len()).find(|&j| !placed[j] && needs[j].iter().all(|&n| placed[n]));
            match next {
                Some(j) => {
                    placed[j] = true;
                    order.push(j);
                }
                None => {
                    let cycle = cycle(&needs, &placed);
                    let first = &workflow.jobs[cycle[0]];
                    let names = cycle.into_iter().map(|j| ids[j].clone()).collect();
                    let path = format!("jobs.{}.needs", first.id);
                    return Err(Error::new(path, Some(first.mark), ErrorKind::Cycle(names)));
                }
            }
        }
        Ok(Graph { ids, needs, order })
    }

    /// The ids in a topological order: each job after its needs, and a tie
    /// in the order of the file.
    pub fn order(&self) -> impl Iterator<Item = &str> {
        self.order.iter().map(|&j| self.ids[j].as_str())
    }

    /// Each edge as `(need, job)`, in the order of the jobs of the file.
    pub fn edges(&self) -> Vec<(&str, &str)> {
        let mut out = Vec::new();
        for (j, list) in self.needs.iter().enumerate() {
            for &n in list {
                out.push((self.ids[n].as_str(), self.ids[j].as_str()));
            }
        }
        out
    }

    /// The direct needs of `job`, in their order.
    pub fn needs(&self, job: &str) -> Vec<&str> {
        self.index(job).map_or_else(Vec::new, |j| self.needs[j].iter().map(|&n| self.ids[n].as_str()).collect())
    }

    /// The jobs that `job` needs, and their needs, to the end: in the
    /// topological order.
    pub fn ancestors(&self, job: &str) -> Vec<&str> {
        let Some(j) = self.index(job) else { return Vec::new() };
        let mut seen = vec![false; self.ids.len()];
        let mut todo = self.needs[j].clone();
        while let Some(n) = todo.pop() {
            if !seen[n] {
                seen[n] = true;
                todo.extend(&self.needs[n]);
            }
        }
        self.order.iter().filter(|&&n| seen[n]).map(|&n| self.ids[n].as_str()).collect()
    }

    pub(crate) fn index(&self, job: &str) -> Option<usize> {
        self.ids.iter().position(|id| id == job)
    }
}

/// A cycle among the jobs that are not placed: from the first of them, go
/// along the needs that are not placed until a job comes again.
fn cycle(needs: &[Vec<usize>], placed: &[bool]) -> Vec<usize> {
    let Some(mut j) = (0..placed.len()).find(|&j| !placed[j]) else { return Vec::new() };
    let mut path: Vec<usize> = Vec::new();
    loop {
        if let Some(start) = path.iter().position(|&p| p == j) {
            let mut out = path[start..].to_vec();
            out.push(j);
            return out;
        }
        path.push(j);
        match needs[j].iter().find(|&&n| !placed[n]) {
            Some(&n) => j = n,
            None => return path,
        }
    }
}
