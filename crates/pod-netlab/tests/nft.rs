//! The rulesets of each scenario load into a real nft, in a new user and
//! network namespace (`unshare -rn`), and their counters read back.
//! Ignored: it needs nft, unshare and user namespaces. By hand:
//! `cargo test -p pod-netlab --test nft -- --ignored`.

use std::path::PathBuf;
use std::process::Command;

use pod_netlab::plan::ruleset::{CARRIER_TABLE, TABLE, carrier_rules, counters_json, router_rules};
use pod_netlab::{Counter, Lab, parse_counters, scenarios};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pod-netlab-nft-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run `script` with the files as `$1`, `$2`, ... in a new namespace, and
/// give its output.
fn in_namespace(script: &str, files: &[PathBuf]) -> String {
    let out =
        Command::new("unshare").args(["-rn", "sh", "-c", script, "sh"]).args(files).output().expect("unshare runs");
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stderr), script);
    String::from_utf8(out.stdout).unwrap()
}

#[test]
#[ignore = "needs nft, unshare and user namespaces"]
fn each_ruleset_loads_into_nft() {
    for s in scenarios() {
        let lab = Lab::new("sn", s.name, s.knobs).unwrap();
        let dir = scratch(s.name);
        let json = dir.join("counters.json");
        let rules = dir.join("rules.nft");
        std::fs::write(&json, counters_json()).unwrap();
        std::fs::write(&rules, router_rules(&lab)).unwrap();
        let script = format!("nft -j -f \"$1\" && nft -f \"$2\" && nft -j list counters table inet {TABLE}");
        let text = in_namespace(&script, &[json, rules]);
        let counters = parse_counters(&text, TABLE).unwrap();
        for c in Counter::ALL {
            assert_eq!(counters.get(c.name()).map(|n| n.packets), Some(0), "{}: {}", s.name, c.name());
        }
        if lab.layout().has_carrier() {
            let carrier = dir.join("carrier.nft");
            std::fs::write(&carrier, carrier_rules()).unwrap();
            let listed = in_namespace(&format!("nft -f \"$1\" && nft list table inet {CARRIER_TABLE}"), &[carrier]);
            assert!(listed.contains("masquerade"), "{listed}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// A counter named `return` in nft's text is refused, so the plan gives
/// the counters as JSON (call C148).
#[test]
#[ignore = "needs nft, unshare and user namespaces"]
fn nft_text_refuses_a_counter_named_return() {
    let dir = scratch("text");
    let text = dir.join("text.nft");
    std::fs::write(&text, format!("table inet {TABLE} {{\n\tcounter return {{}}\n}}\n")).unwrap();
    let out = Command::new("unshare").args(["-rn", "nft", "-f"]).arg(&text).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("syntax error"));
    std::fs::remove_dir_all(&dir).unwrap();
}
