//! The offers of the fake: a queued job of the control API as a
//! `job-offer.v1`, the check of the facts of a host, and the answer to
//! `assigned`.

use pod_job::offer::{Event as ForgeEvent, TrustInputs};
use pod_job::source::{HostFacts, Offer, RealJob, Reply};
use pod_job::{ContainerRoute, Fact, JobOffer, SCHEMA};
use serde_json::{Map, Value, json};

use crate::github::{JOB_REF, repository_id};

/// The owner and the repository of a job that names none.
const REPOSITORY: &str = "example-org/app";
/// The `wait_deadline_s` of a job that names none.
const WAIT_DEADLINE_S: u64 = 600;

/// The offer of a queued job. The body is an object of `job-offer.v1`
/// fields, each optional, and two fields of the fake: `repository`
/// (`OWNER/REPO`) and `tier` (`trusted`, the default, or `untrusted`), which
/// give the `trust_inputs` of a job that names none. The offer has no
/// `tier`: podrunner computes it from the trust inputs.
pub(super) fn build(body: &Value, n: u64, source: &str) -> Result<JobOffer, String> {
    let Value::Object(fields) = body else { return Err("the body is not a JSON object".into()) };
    let mut fields = fields.clone();
    let repository = match fields.remove("repository") {
        None => REPOSITORY.to_string(),
        Some(Value::String(r)) => r,
        Some(_) => return Err("repository is not a text".into()),
    };
    let Some((owner, repo)) = repository.split_once('/').filter(|(o, r)| !o.is_empty() && !r.is_empty()) else {
        return Err("repository is not OWNER/REPO".into());
    };
    let trust = match fields.remove("tier").as_ref().map(|t| t.as_str()) {
        None | Some(Some("trusted")) => json!({
            "event": "push", "head_is_fork": "no", "actor_class": "member", "repo_visibility": "private",
        }),
        Some(Some("untrusted")) => json!({
            "event": "pull_request", "head_is_fork": "yes", "actor_class": "outside", "repo_visibility": "public",
        }),
        Some(_) => return Err("tier is not trusted or untrusted".into()),
    };
    let mut defaults = Map::new();
    defaults.insert("schema".into(), json!(SCHEMA));
    defaults.insert("job_id".into(), json!(format!("j-{n}")));
    defaults.insert("source".into(), json!(source));
    defaults.insert("labels".into(), json!([]));
    defaults.insert("arch".into(), json!("x86_64"));
    defaults.insert("wait_deadline_s".into(), json!(WAIT_DEADLINE_S));
    let mut inputs = trust;
    inputs["owner"] = json!(owner);
    inputs["repo"] = json!(repo);
    inputs["run_id"] = json!(n);
    // The ref and the repository id of the job in a linked fake GitHub: the
    // facts of the job-started hook there are the facts of the offer.
    inputs["ref"] = json!(JOB_REF);
    inputs["repo_id"] = json!(repository_id(&repository));
    inputs["workflow_ref"] = json!(format!("{repository}/.github/workflows/ci.yml@refs/heads/main"));
    defaults.insert("trust_inputs".into(), inputs);
    for (k, v) in defaults {
        fields.entry(k).or_insert(v);
    }
    let offer: JobOffer = serde_json::from_value(Value::Object(fields)).map_err(|e| format!("not a job offer: {e}"))?;
    offer.check(false)?;
    Ok(offer)
}

/// Whether a host with the facts `host` can run the job: its arch, and
/// the KVM and the container that the job needs. A session that sent no
/// facts gets each job.
pub(super) fn fits(host: Option<&HostFacts>, offer: &JobOffer) -> bool {
    let Some(h) = host else { return true };
    let need = offer.unit_need.clone().unwrap_or_default();
    let kind = |want: &[ContainerRoute]| h.kinds.iter().any(|k| want.contains(&k.container));
    h.arch == offer.arch
        && (!need.kvm || h.kvm)
        && (!need.container_engine || kind(&[ContainerRoute::Engine]))
        && (!need.container || kind(&[ContainerRoute::Hooks, ContainerRoute::Engine]))
}

/// The offer on the wire: the fields of `job-offer.v1` beside its `job_id`.
pub(super) fn wire(offer: &JobOffer) -> Offer {
    let mut fields = match serde_json::to_value(offer) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    };
    fields.remove("job_id");
    Offer { job_id: offer.job_id.clone(), fields }
}

/// The answer to `assigned`, after podrunner's fake source: a real job of
/// the owner of the offer is kept, with the actor and the visibility of the
/// offer, and the event, the run and the fork fact of the real job; a job
/// of another owner ends with `not-of-this-source`. A fact that the fake
/// does not know of the real job (the ref of another run, the id of another
/// repository) is empty, never the fact of the offer: podrunner keeps a
/// cache of the offer only for the scope of these facts.
pub(super) fn assigned(offer: &JobOffer, real: &RealJob) -> Reply {
    let mut t: TrustInputs = offer.trust_inputs.clone();
    let Some((owner, repo)) = real.repository.split_once('/') else {
        return end("bad-repository", "the repository is not OWNER/REPO");
    };
    if !owner.eq_ignore_ascii_case(&t.owner) {
        return end("not-of-this-source", "the job is of another owner");
    }
    // A run id is of one repository: the ref of the offer is a fact of
    // the real job only for the same repository and the same run.
    let same_repo = repo.eq_ignore_ascii_case(&t.repo);
    if !same_repo {
        t.repo_id = None;
    }
    if !same_repo || real.run_id != t.run_id {
        t.git_ref.clear();
    }
    t.repo = repo.to_string();
    t.run_id = real.run_id;
    t.event = serde_json::from_value(Value::String(real.event.clone())).unwrap_or(ForgeEvent::Unknown);
    t.head_is_fork = fork_fact(real);
    Reply::Keep { trust_inputs: t }
}

/// The fork fact of a real job, never the fact of the offer: the real job
/// can be of another run and another event. A head is a fork when it is
/// not the base.
fn fork_fact(real: &RealJob) -> Fact {
    match (real.head_is_fork, real.head_repository.as_deref()) {
        (Some(true), _) => Fact::Yes,
        (Some(false), _) => Fact::No,
        (None, Some("")) => Fact::Unknown,
        (None, Some(head)) if head.eq_ignore_ascii_case(&real.repository) => Fact::No,
        (None, Some(_)) => Fact::Yes,
        (None, None) => Fact::Unknown,
    }
}

fn end(code: &str, detail: &str) -> Reply {
    Reply::End { code: code.into(), detail: detail.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_of_the_control_api_is_a_job_offer() {
        let o = build(&json!({ "labels": ["podrunner"], "tier": "untrusted" }), 7, "githome").unwrap();
        assert_eq!(o.job_id.to_string(), "j-7");
        assert_eq!(o.trust_inputs.head_is_fork, Fact::Yes);
        assert!(!wire(&o).fields.contains_key("job_id"));
        assert!(build(&json!({ "tier": "local" }), 1, "githome").is_err(), "a tier of the configuration alone");
        assert!(build(&json!({ "arch": "mips" }), 1, "githome").is_err(), "an arch of no unit");
        assert!(build(&json!({ "env": {} }), 1, "githome").is_err(), "a field of no offer");
    }

    #[test]
    fn assigned_keeps_no_fork_fact_of_the_offer() {
        // The offer says "no fork"; the real job decides.
        let o = build(&json!({ "repository": "example-org/app" }), 1, "githome").unwrap();
        assert_eq!(o.trust_inputs.head_is_fork, Fact::No);
        let real = |head: Option<&str>, fork: Option<bool>| RealJob {
            run_id: 9,
            job_name: "build".into(),
            event: "pull_request".into(),
            repository: "example-org/app".into(),
            head_repository: head.map(str::to_string),
            head_is_fork: fork,
            actor: "someone".into(),
            job_id: None,
            git_ref: None,
            repo_id: None,
        };
        let fact = |r: RealJob| match assigned(&o, &r) {
            Reply::Keep { trust_inputs } => trust_inputs.head_is_fork,
            other => panic!("not kept: {other:?}"),
        };
        assert_eq!(fact(real(None, None)), Fact::Unknown, "no fact and no head");
        assert_eq!(fact(real(Some(""), None)), Fact::Unknown, "an empty head");
        assert_eq!(fact(real(Some("Example-Org/App"), None)), Fact::No, "the base, in another case");
        assert_eq!(fact(real(Some("someone/app"), None)), Fact::Yes, "another repository");
        assert_eq!(fact(real(Some("example-org/app"), Some(true))), Fact::Yes, "the fact of the hook wins");
        assert_eq!(fact(real(Some("someone/app"), Some(false))), Fact::No, "the fact of the hook wins");
    }

    /// The contract: the facts of `keep` give the ref and the repository id
    /// of the real job, empty when the source does not know one, never the
    /// value of the offer.
    #[test]
    fn assigned_gives_no_ref_or_repository_id_of_the_offer_for_another_job() {
        let o = build(&json!({ "repository": "example-org/app" }), 1, "githome").unwrap();
        let ti = &o.trust_inputs;
        assert_eq!((ti.git_ref.as_str(), ti.repo_id), (JOB_REF, Some(repository_id("example-org/app"))));
        let real = |run_id: u64, repository: &str| RealJob {
            run_id,
            job_name: "build".into(),
            event: "push".into(),
            repository: repository.into(),
            head_repository: None,
            head_is_fork: None,
            actor: "someone".into(),
            job_id: None,
            git_ref: None,
            repo_id: None,
        };
        let facts = |r: RealJob| match assigned(&o, &r) {
            Reply::Keep { trust_inputs } => (trust_inputs.git_ref, trust_inputs.repo_id, trust_inputs.run_id),
            other => panic!("not kept: {other:?}"),
        };
        let id = Some(repository_id("example-org/app"));
        assert_eq!(facts(real(1, "example-org/app")), (JOB_REF.to_string(), id, 1), "the job of the offer");
        assert_eq!(facts(real(1, "Example-Org/App")), (JOB_REF.to_string(), id, 1), "the name in another case");
        assert_eq!(facts(real(9, "example-org/app")), (String::new(), id, 9), "another run");
        assert_eq!(facts(real(1, "example-org/other")), (String::new(), None, 1), "another repository");
        assert_eq!(facts(real(9, "example-org/other")), (String::new(), None, 9), "both");
    }

    #[test]
    fn a_host_gets_only_a_job_of_its_arch_and_its_kvm() {
        let host = |arch: &str, kvm: bool| HostFacts {
            podrunner: "0".into(),
            podvm: None,
            podnet: None,
            arch: arch.into(),
            kvm,
            kinds: Vec::new(),
        };
        let o = build(&json!({ "arch": "aarch64", "unit_need": { "kvm": true } }), 1, "githome").unwrap();
        assert!(fits(None, &o));
        assert!(fits(Some(&host("aarch64", true)), &o));
        assert!(!fits(Some(&host("x86_64", true)), &o));
        assert!(!fits(Some(&host("aarch64", false)), &o));
    }
}
