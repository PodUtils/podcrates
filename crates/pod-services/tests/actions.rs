//! The hosts of GitHub Actions for a self-hosted runner and the pin of the
//! official runner (T-164): each host with its source and the date read,
//! the multi-tenant wildcards flagged, and a watch for each list and for
//! the age of the pin.

use pod_services::Origin;

#[test]
fn the_actions_hosts_name_their_source_and_flag_the_multi_tenant_wildcards() {
    let actions = pod_services::get("github-actions").expect("the hosts of GitHub Actions");
    let full = actions.texts("full-domains").expect("the full domains");
    let wild = actions.texts("wildcard-domains").expect("the wildcard domains");
    assert!(full.len() > 100, "{} full domains", full.len());
    for name in ["full-domains", "wildcard-domains", "multi-tenant"] {
        let fact = actions.fact(name).unwrap();
        assert!(matches!(fact.source().origin(), Origin::Url(u) if u.contains("api.github.com/meta")), "{name}");
        assert_eq!(fact.source().read().to_string(), "2026-11-11", "{name}");
    }
    // Each endpoint is one of GitHub's hosts, and each multi-tenant flag
    // names one of GitHub's wildcards.
    for endpoint in actions.endpoints() {
        assert!(full.iter().any(|h| h.as_ref() == endpoint.host()), "{}", endpoint.host());
        assert_eq!(endpoint.source().read().to_string(), "2026-11-11", "{}", endpoint.name());
    }
    for flagged in actions.texts("multi-tenant").unwrap() {
        assert!(wild.iter().any(|w| w.as_ref() == flagged.as_ref()), "{flagged} is no wildcard of GitHub");
    }
    for host in ["broker.actions.githubusercontent.com", "runner-auth.actions.githubusercontent.com"] {
        assert!(actions.endpoints().iter().any(|e| e.host() == host && e.role() == Some("control")), "{host}");
    }
}

#[test]
fn a_daily_watch_holds_each_list_to_the_meta_api() {
    let actions = pod_services::get("github-actions").unwrap();
    let pointers: Vec<(&str, &str)> = actions
        .watches()
        .iter()
        .filter(|w| w.kind() == "http-json")
        .map(|w| {
            (
                w.arg("pointer").and_then(|v| v.as_text()).unwrap_or_default(),
                w.arg("fact").and_then(|v| v.as_text()).unwrap_or_default(),
            )
        })
        .collect();
    assert!(pointers.contains(&("/domains/actions_inbound/full_domains", "full-domains")), "{pointers:?}");
    assert!(pointers.contains(&("/domains/actions_inbound/wildcard_domains", "wildcard-domains")), "{pointers:?}");
}

#[test]
fn the_pin_of_the_runner_has_a_watch_of_its_age() {
    let runner = pod_services::get("github-actions-runner").expect("the pin of actions/runner");
    let tag = runner.text("runner-tag").expect("the tag");
    assert!(tag.starts_with('v'), "{tag}");
    let watch = runner.watches().iter().find(|w| w.kind() == "github-release").expect("a github-release watch");
    assert_eq!(watch.arg("repo").and_then(|v| v.as_text()), Some("actions/runner"));
    assert_eq!(watch.arg("fact").and_then(|v| v.as_text()), Some("runner-tag"));
    // GitHub's rule: a runner is updated within 30 days of a new release.
    assert_eq!(watch.arg("max-age-days").and_then(|v| v.as_int()), Some(30));
}
