use super::*;

fn table(specs: &[&str], default: Action) -> Table {
    let specs: Vec<String> = specs.iter().map(|s| s.to_string()).collect();
    Table::build(&specs, None, default, &NoLint, false).unwrap().0
}

/// PodUtils/podnet issue 140: the rules of --l4 need no lookup; the
/// built-in rules apply.
#[test]
fn a_static_decision_uses_the_rules_and_the_builtins() {
    let t = table(&["domain:ads.example=reject", "cidr:10.0.0.0/8=direct"], Action::Tunnel);
    let name = |n: &str| Host::Domain(n.into());
    let ip = |i: &str| Host::Ip(i.parse().unwrap());
    assert_eq!(t.decide_static(&name("ads.example")), Action::Reject);
    assert_eq!(t.decide_static(&name("localhost")), Action::Reject);
    assert_eq!(t.decide_static(&ip("10.1.2.3")), Action::Direct);
    assert_eq!(t.decide_static(&ip("127.0.0.1")), Action::Reject);
    assert_eq!(t.decide_static(&name("example.org")), Action::Tunnel);
    let direct = table(&[], Action::Direct);
    assert_eq!(direct.decide_static(&name("example.org")), Action::Direct);
}

#[test]
fn the_first_rule_that_matches_wins() {
    let t = table(
        &[
            "domain:.corp.example=direct",
            "domain:ads.example=reject",
            "cidr:10.0.0.0/8=direct,cidr:2001:db8::/32=reject",
        ],
        Action::Tunnel,
    );
    assert_eq!(t.rules.len(), 4);
    assert_eq!(t.for_name("git.corp.example"), Some(Action::Direct));
    assert_eq!(t.for_name("ADS.example."), Some(Action::Reject));
    assert_eq!(t.for_name("example.com"), None);
    assert!(t.has_cidr());
    assert_eq!(t.for_ip("10.1.2.3".parse().unwrap()), Action::Direct);
    assert_eq!(t.for_ip("2001:db8::1".parse().unwrap()), Action::Reject);
    assert_eq!(t.for_ip("1.1.1.1".parse().unwrap()), Action::Tunnel);
    assert_eq!(t.for_addresses(&["1.1.1.1".parse().unwrap(), "10.0.0.9".parse().unwrap()]), Action::Direct);
    assert_eq!(table(&[], Action::Reject).for_ip("1.1.1.1".parse().unwrap()), Action::Reject);
}

#[test]
fn the_built_in_rules_refuse_the_host() {
    let t = table(&[], Action::Tunnel);
    for ip in ["127.0.0.1", "::1", "0.0.0.0", "169.254.169.254", "fe80::1", "::ffff:127.0.0.1"] {
        assert_eq!(t.for_ip(ip.parse().unwrap()), Action::Reject, "{ip}");
    }
    assert_eq!(t.for_name("localhost"), Some(Action::Reject));
    assert_eq!(t.for_name("a.localhost"), Some(Action::Reject));
    assert_eq!(t.for_ip("192.168.1.1".parse().unwrap()), Action::Tunnel);
    // A rule of the operator comes first.
    let t = table(&["cidr:127.0.0.0/8=direct", "domain:localhost=direct"], Action::Tunnel);
    assert_eq!(t.for_ip("127.0.0.1".parse().unwrap()), Action::Direct);
    assert_eq!(t.for_name("localhost"), Some(Action::Direct));
}

#[test]
fn bad_rules_and_the_rules_file() {
    for bad in ["domain:a", "domain:a=maybe", "country:NL=direct", "cidr:10.0.0.0/40=direct", "=direct"] {
        let err = Rule::parse(bad, "-r 1").unwrap_err();
        assert_eq!(err.class, Class::Usage, "{bad}");
    }
    let dir = crate::test_dir("rules");
    let file = dir.join("rules");
    std::fs::write(&file, "# rules\ndomain:.lan=direct\n\ncidr:192.0.2.0/24=reject # test\n").unwrap();
    let (t, _) =
        Table::build(&["domain:x.example=reject".into()], Some(&file), Action::Tunnel, &NoLint, false).unwrap();
    assert_eq!(t.rules.len(), 3);
    assert_eq!(t.rules[0].spec, "domain:x.example=reject");
    assert_eq!(t.rules[0].place, "-r 1");
    assert!(t.rules[2].place.ends_with("rules:4"), "{}", t.rules[2].place);
    assert_eq!(t.for_name("nas.lan"), Some(Action::Direct));
    std::fs::write(&file, "domain:.lan=direct\ndomain:.lan=sideways\n").unwrap();
    let err = Table::build(&[], Some(&file), Action::Tunnel, &NoLint, false).unwrap_err();
    assert!(err.to_string().contains("rules:2"), "{err}");
    let refused = Refused("a:1".into()).failure();
    assert_eq!(refused.class, Class::Policy);
}

/// PodUtils/podnet@T-242: the lint of the caller sees each rule, with its
/// matcher, its action in the normal form and its place; a refusal of the
/// lint is a usage failure (exit 64), and its warnings come back. A shadowed
/// `reject` passes when the caller allows it.
#[test]
fn the_lint_of_the_caller_judges_the_table() {
    // A lint of the test: a later rule with the matcher of an earlier one is
    // shadowed; a shadowed `reject` is refused unless `allow`.
    let lint = |items: &[LintItem<'_>], allow: bool| -> Result<Vec<String>, String> {
        let mut warnings = Vec::new();
        for (j, later) in items.iter().enumerate() {
            if let Some(earlier) = items[..j].iter().find(|e| e.matcher == later.matcher && e.action != later.action) {
                let text = format!("the rule {} ({}) is shadowed by {}", later.place, later.spec, earlier.place);
                if later.action == "reject" && !allow {
                    return Err(text);
                }
                warnings.push(text);
            }
        }
        Ok(warnings)
    };
    let specs = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let shadowed = specs(&["domain:.ads.example=direct", "domain:.ads.example=block"]);
    let err = Table::build(&shadowed, None, Action::Tunnel, &lint, false).unwrap_err();
    assert_eq!(err.class.exit_code(), 64);
    assert!(err.to_string().contains("-r 2 (domain:.ads.example=block) is shadowed by -r 1"), "{err}");
    let (table, warnings) = Table::build(&shadowed, None, Action::Tunnel, &lint, true).unwrap();
    assert_eq!(table.rules.len(), 2);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let other = specs(&["domain:.ads.example=reject", "domain:.ads.example=direct"]);
    let (_, warnings) = Table::build(&other, None, Action::Tunnel, &lint, false).unwrap();
    assert_eq!(warnings.len(), 1, "a shadowed direct is a warning");
}

/// The rules of a caller are a value: a set replaces the table and counts a
/// reload, and the counters count each action.
#[test]
fn the_rules_are_a_value_of_the_caller() {
    let rules = Rules::new(table(&[], Action::Tunnel));
    assert_eq!(rules.current().default, Action::Tunnel);
    rules.set(table(&[], Action::Reject));
    assert_eq!(rules.current().default, Action::Reject);
    rules.counters.count(Action::Reject);
    assert_eq!(rules.counters.reject.load(Relaxed), 1);
    assert_eq!(rules.counters.reloads.load(Relaxed), 1);
}
