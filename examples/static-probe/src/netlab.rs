//! pod-netlab: the plan of a scenario, its expectations, and the judge of
//! a counter text, so that the knobs, the plan, the derivation, the
//! counter reader (serde_json) and the judge link in.

pub fn netlab() -> String {
    let Some(scenario) = pod_netlab::scenario("cgnat-dnat") else { return "pod-netlab: no scenario".into() };
    let lab = match pod_netlab::Lab::new("sn", scenario.name, scenario.knobs) {
        Ok(lab) => lab,
        Err(e) => return format!("pod-netlab: {e}"),
    };
    let settings = pod_netlab::Settings { state_dir: "/run/netlab".into(), ping_gid: 0 };
    let steps = lab.up(&settings).map_or(0, |s| s.len());
    let expect = lab.expect();
    let table = pod_netlab::plan::ruleset::TABLE;
    let counters = pod_netlab::parse_counters(r#"{"nftables": []}"#, table).unwrap_or_default();
    let report = pod_netlab::judge(&expect, &counters, &counters, &Default::default());
    format!(
        "pod-netlab: {steps} steps, {} probes, {} failed rows of an empty run",
        expect.probes.len(),
        report.failures().count()
    )
}
