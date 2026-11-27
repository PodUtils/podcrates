//! The names of `uses:`. GitHub matches the owner and the name of an
//! action with no case (ASCII), and the ref and the path with case. A run
//! gives each name as the workflow wrote it; the match of two names, of a
//! name of `except` and of a key of the lock folds the case of the owner
//! and the name only.

use pod_actions_workflow::{Kind, Limits, Workflow};

use crate::error::{Error, SelfFault};

/// The prefix of `uses: $/PATH`: an action of the repository of the
/// workflow, or in a composite action, of the repository of that action
/// (`SelfRepositoryAlias` of the runner).
pub const SELF_PREFIX: &str = "$/";

/// A reference of `uses:` split into `OWNER/REPO`, the path in it and the ref.
pub(crate) fn split(uses: &str) -> Option<(String, String, String)> {
    let (target, r) = uses.rsplit_once('@')?;
    let mut parts = target.splitn(3, '/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    if owner.is_empty() || repo.is_empty() || r.is_empty() {
        return None;
    }
    Some((format!("{owner}/{repo}"), parts.next().unwrap_or("").trim_matches('/').to_string(), r.to_string()))
}

/// `OWNER/REPO` of a `uses:` or of a key, with no path and no ref.
fn repo_of(uses: &str) -> &str {
    let target = uses.rsplit_once('@').map_or(uses, |(t, _)| t);
    match target.match_indices('/').nth(1) {
        Some((i, _)) => &target[..i],
        None => target,
    }
}

/// A key `OWNER/REPO[/PATH]@REF` with the owner and the name in lowercase,
/// and the path and the ref as written: two spellings of one action give
/// one key.
pub(crate) fn fold(key: &str) -> String {
    let repo = repo_of(key);
    format!("{}{}", repo.to_ascii_lowercase(), &key[repo.len()..])
}

/// `OWNER/REPO` of `uses` is a name of `except`, with no case.
pub(crate) fn excepted(except: &[&str], uses: &str) -> bool {
    let repo = repo_of(uses);
    except.iter().any(|e| e.eq_ignore_ascii_case(repo))
}

pub(crate) fn remote(uses: &str) -> bool {
    !uses.starts_with("./") && !uses.starts_with("docker://")
}

/// Each `uses:` of a repository in the steps of the workflow, but each
/// `uses:` whose `OWNER/REPO` is a name of `except`, with no case (an
/// action that the consumer gives in its own way, as a stand-in of
/// `actions/checkout`). A `uses: $/PATH` stays as written: only the
/// consumer knows the repository and the commit of the run
/// ([`self_uses`] maps it).
pub fn workflow_uses(w: &Workflow, except: &[&str]) -> Vec<String> {
    let steps = w.jobs.iter().flat_map(|j| &j.steps);
    steps.filter_map(|s| s.uses.as_deref()).filter(|u| remote(u) && !excepted(except, u)).map(String::from).collect()
}

/// `uses: $/PATH` as `OWNER/REPO/PATH@REF`, the action of `repo`
/// (`OWNER/REPO`) at `r`, as the runner resolves it: in a workflow, the
/// repository of the run at its commit; in a composite action, the
/// repository and the ref of that action. `$/` alone is the root of the
/// repository. `None` for another `uses:`; `$/PATH@REF` fails
/// ([`SelfFault::Ref`]).
pub fn self_uses(uses: &str, repo: &str, r: &str) -> Option<Result<String, Error>> {
    let path = uses.strip_prefix(SELF_PREFIX)?.trim_matches('/');
    Some(match path {
        p if p.contains('@') => Err(Error::SelfUses { uses: uses.to_string(), fault: SelfFault::Ref }),
        "" => Ok(format!("{repo}@{r}")),
        p => Ok(format!("{repo}/{p}@{r}")),
    })
}

/// Each `uses:` of a repository in the steps of a composite `action.yml`
/// of `parent` (`OWNER/REPO[/PATH]@REF`); a `$/PATH` is the repository
/// and the ref of `parent`.
pub(crate) fn composite_uses(text: &str, parent: &str) -> Result<Vec<String>, Error> {
    let fail = |reason: String| Error::Action { uses: parent.to_string(), reason };
    let root = pod_actions_workflow::yaml::load(text, Limits::default()).map_err(|e| fail(e.to_string()))?;
    let Some(runs) = root.get("runs") else { return Ok(Vec::new()) };
    if !matches!(runs.get("using").map(|n| &n.kind), Some(Kind::String(u)) if u == "composite") {
        return Ok(Vec::new());
    }
    let Some(Kind::Seq(steps)) = runs.get("steps").map(|n| &n.kind) else { return Ok(Vec::new()) };
    let (repo, r) = (repo_of(parent), parent.rsplit_once('@').map_or("", |(_, r)| r));
    let mut out = Vec::new();
    for s in steps {
        match s.get("uses").map(|n| &n.kind) {
            Some(Kind::String(u)) if remote(u) => out.push(match self_uses(u, repo, r) {
                Some(mapped) => mapped?,
                None => u.clone(),
            }),
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_case_of_the_owner_and_the_name_folds_and_the_ref_and_the_path_stay() {
        let rows = [
            ("actions/checkout@v4", "actions/checkout@v4"),
            ("Actions/Checkout@v4", "actions/checkout@v4"),
            ("ACTIONS/CHECKOUT@V4", "actions/checkout@V4"),
            ("O/R/Sub/Dir@Main", "o/r/Sub/Dir@Main"),
            ("O/R", "o/r"),
        ];
        for (key, want) in rows {
            assert_eq!(fold(key), want, "{key}");
        }
        let except = ["actions/checkout"];
        let rows = [
            ("actions/checkout@v4", true),
            ("Actions/Checkout@v4", true),
            ("ACTIONS/CHECKOUT@V4", true),
            ("actions/checkout/sub@v4", true),
            ("actions/checkout-x@v4", false),
            ("actions/cache@v4", false),
            ("other/checkout@v4", false),
        ];
        for (uses, want) in rows {
            assert_eq!(excepted(&except, uses), want, "{uses}");
        }
        assert!(excepted(&["Actions/CHECKOUT"], "actions/checkout@v4"), "the case of `except` folds too");
    }

    #[test]
    fn a_uses_of_the_repository_of_its_action() {
        let rows = [
            ("$/x/y", Some("o/r/x/y@v1")),
            ("$//x/", Some("o/r/x@v1")),
            ("$/", Some("o/r@v1")),
            ("./x", None),
            ("o/r@v1", None),
            ("docker://a", None),
            ("$x", None),
            ("x$/y", None),
        ];
        for (uses, want) in rows {
            assert_eq!(self_uses(uses, "o/r", "v1").map(Result::unwrap).as_deref(), want, "{uses}");
        }
        let e = self_uses("$/x@v2", "o/r", "v1").unwrap().unwrap_err();
        assert_eq!(e, Error::SelfUses { uses: "$/x@v2".into(), fault: SelfFault::Ref });
        assert!(e.to_string().contains("takes no @REF"), "{e}");
    }
}
