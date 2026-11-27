//! The asks of PodUtils/podrunner#379 (`except` at each depth), #380 (the
//! owner and the name with no case) and #381 (`$/PATH` in a composite
//! action).

use super::*;
use crate::SelfFault;

const D: &str = "dddddddddddddddddddddddddddddddddddddddd";
const E: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn composite(steps: &[&str]) -> String {
    let steps: String = steps.iter().map(|u| format!("    - uses: {u}\n")).collect();
    format!("runs:\n  using: composite\n  steps:\n{steps}")
}

/// `o/top@v1` names the checkout and `o/mid@v1`; `o/mid@v1` names the
/// checkout in two other cases and `o/inner@v2`: the checkout is at depth
/// 1 and 2. The forge has no checkout.
fn checkout_forge() -> Fake {
    let f = forge();
    f.add("o/top@v1", D, &[("action.yml", &composite(&["actions/checkout@v4", "o/mid@v1"]))]);
    let mid = composite(&["Actions/Checkout@v5", "ACTIONS/CHECKOUT/sub@V6", "o/inner@v2"]);
    f.add("o/mid@v1", E, &[("action.yml", &mid)]);
    f
}

#[test]
fn a_name_of_except_is_not_fetched_followed_or_locked_at_each_depth() {
    // (except, uses, the keys of the commits, or the start of the error)
    let all: &[&str] = &["o/inner@v2", "o/mid@v1", "o/top@v1"];
    type Row<'a> = (&'a [&'a str], &'a [&'a str], Result<&'a [&'a str], &'a str>);
    let rows: [Row; 6] = [
        (&["actions/checkout"], &["o/top@v1"], Ok(all)),
        (&["Actions/CHECKOUT"], &["o/top@v1"], Ok(all)),
        (&["actions/checkout"], &["o/top@v1", "actions/checkout@v4"], Ok(all)),
        (&["actions/checkout"], &["o/mid@v1"], Ok(&["o/inner@v2", "o/mid@v1"])),
        (&[], &["o/top@v1"], Err("no actions/checkout@v4")),
        (&["actions/checkout-x", "actions"], &["o/top@v1"], Err("no actions/checkout@v4")),
    ];
    for (except, list, want) in rows {
        let (f, s) = (checkout_forge(), store());
        let options = Options { except };
        let r = resolve_with(&uses(list), &options, &Lock::default(), &s, &f, Mode::Online, "L");
        match (r, want) {
            (Ok(r), Ok(keys)) => {
                assert_eq!(r.commits.keys().map(String::as_str).collect::<Vec<_>>(), keys, "{except:?} {list:?}");
                assert_eq!(r.lock.pins.keys().map(String::as_str).collect::<Vec<_>>(), keys, "{except:?} {list:?}");
                assert!(!s.dir.join("actions_checkout").exists(), "{except:?}: the checkout is in the store");
                // Offline with the lock: the checkout needs no pin.
                let off = resolve_with(&uses(list), &options, &r.lock, &s, &f, Mode::Offline, "L").unwrap();
                assert_eq!(off.commits, r.commits, "{except:?} {list:?}");
            }
            (Err(e), Err(start)) => assert!(e.to_string().starts_with(start), "{except:?} {list:?}: {e}"),
            (r, want) => panic!("{except:?} {list:?}: {r:?}, want {want:?}"),
        }
        let _ = std::fs::remove_dir_all(&s.dir);
    }
    // `resolve` is `resolve_with` and no `except`.
    let (f, s) = (checkout_forge(), store());
    let e = resolve(&uses(&["o/top@v1"]), &Lock::default(), &s, &f, Mode::Online, "L").unwrap_err();
    assert!(matches!(&e, Error::Forge { repo, .. } if repo == "actions/checkout"), "{e:?}");
    let _ = std::fs::remove_dir_all(&s.dir);
}

#[test]
fn two_spellings_of_one_action_give_one_fetch_and_one_pin() {
    let (f, s) = (forge(), store());
    let list = uses(&["o/inner@v2", "O/Inner@v2", "O/INNER@v2"]);
    let r = resolve(&list, &Lock::default(), &s, &f, Mode::Online, "L").unwrap();
    assert_eq!((f.resolves.get(), f.tars.get()), (1, 1), "one resolve and one tar");
    assert_eq!(r.lock.pins.keys().collect::<Vec<_>>(), ["o/inner@v2"], "the lock keeps the first spelling");
    for (key, repo) in [("o/inner@v2", "o/inner"), ("O/Inner@v2", "O/Inner"), ("O/INNER@v2", "O/INNER")] {
        assert_eq!(r.commits[key], B, "{key}");
        assert!(s.path(repo, B).is_file(), "{repo}: the store has the tar under each spelling");
    }
    // Another spelling of a pinned action takes the pin, also offline from
    // the tar of the first spelling; the lock does not change.
    let other = store();
    std::fs::create_dir_all(&other.dir).unwrap();
    std::fs::rename(s.dir.join("o_inner"), other.dir.join("o_inner")).unwrap();
    let rows = [("o/inner@v2", Mode::Offline), ("o/Inner@v2", Mode::Offline), ("O/INNER@v2", Mode::Frozen)];
    for (u, mode) in rows {
        let again = resolve(&uses(&[u]), &r.lock, &other, &f, mode, "L").unwrap();
        assert!(!again.changed && again.lock == r.lock, "{u}: the lock changed");
        assert_eq!(again.commits.iter().collect::<Vec<_>>(), [(&u.to_string(), &B.to_string())], "{u}");
    }
    // The ref keeps its case: `@V2` is another ref.
    let e = resolve(&uses(&["o/inner@V2"]), &r.lock, &other, &f, Mode::Frozen, "L").unwrap_err();
    assert!(matches!(&e, Error::NotLocked { key, .. } if key == "o/inner@V2"), "{e:?}");
    // A moved tag names the line of the lock.
    f.add("o/inner@v2", C, &[("action.yml", NODE)]);
    let e = resolve(&uses(&["O/Inner@v2"]), &r.lock, &other, &f, Mode::Online, "L").unwrap_err();
    assert!(matches!(&e, Error::Moved { key, .. } if key == "o/inner@v2"), "{e:?}");
    // A tree of another spelling of an action of the run: no fetch.
    let (tars, resolves) = (f.tars.get(), f.resolves.get());
    let trees = add_trees(&["O/inner@v2".to_string()], r, &s, &f, Mode::Online, "L").unwrap();
    assert_eq!((f.tars.get(), f.resolves.get()), (tars, resolves));
    assert_eq!(trees.commits["O/inner@v2"], B);
    for d in [&s.dir, &other.dir] {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn the_except_of_the_workflow_and_its_checkouts_match_with_no_case() {
    let step = |u: &str| format!("      - uses: {u}\n        with: {{ repository: o/lib, ref: v2 }}\n");
    let rows = [
        ("actions/checkout@v4", true),
        ("Actions/Checkout@v4", true),
        ("ACTIONS/CHECKOUT@V4", true),
        ("actions/checkout-x@v4", false),
        ("other/checkout@v4", false),
    ];
    for (u, checkout) in rows {
        let text = format!("on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n{}", step(u));
        let w = pod_actions_workflow::parse(&text).unwrap().workflow;
        let want: &[&str] = if checkout { &[] } else { &[u] };
        assert_eq!(workflow_uses(&w, &["actions/checkout"]), want, "{u}");
        let want: &[&str] = if checkout { &["o/lib@v2"] } else { &[] };
        assert_eq!(workflow_trees(&w, "me/app").unwrap(), want, "{u}");
    }
}

/// `$/PATH` at depth 1 and 2, by tag and by commit, across two
/// repositories; `$/PATH@REF` fails by name, and so does a `$/PATH` with no
/// action.
#[test]
fn a_uses_of_the_repository_of_its_action_takes_its_ref() {
    let f = forge();
    let root = composite(&["$/sub", "$/"]);
    let sub = composite(&["$/deep/", "o/other@v3"]);
    let files = [("action.yml", root.as_str()), ("sub/action.yml", &sub), ("deep/action.yml", NODE)];
    f.add("o/self@v1", D, &files);
    f.add("o/other@v3", E, &[("action.yml", &composite(&["$//x"])), ("x/action.yml", NODE)]);
    for top in ["o/self@v1".to_string(), format!("o/self@{D}"), "O/Self/sub@v1".into()] {
        let s = store();
        let r = resolve(&uses(&[&top]), &Lock::default(), &s, &f, Mode::Online, "L").unwrap();
        let mut keys: Vec<_> = r.commits.keys().map(|k| fold(k)).collect();
        keys.sort();
        let me = fold(&top.replace("/sub@", "@"));
        assert_eq!(keys, ["o/other@v3", me.as_str()], "{top}");
        assert_eq!(r.commits["o/other@v3"], E);
        let _ = std::fs::remove_dir_all(&s.dir);
    }
    let rows = [
        (composite(&["$/sub@v2"]), Error::SelfUses { uses: "$/sub@v2".into(), fault: SelfFault::Ref }),
        (composite(&["$/none"]), Error::NoAction { uses: "o/bad/none@v1".into(), path: "none".into() }),
    ];
    for (text, want) in rows {
        let s = store();
        f.add("o/bad@v1", C, &[("action.yml", &text)]);
        let e = resolve(&uses(&["o/bad@v1"]), &Lock::default(), &s, &f, Mode::Online, "L").unwrap_err();
        assert_eq!(e, want);
        let _ = std::fs::remove_dir_all(&s.dir);
    }
    // In the list of the workflow, `$/PATH` has no action to take a ref of.
    for (u, fault) in [("$/sub", SelfFault::NoParent), ("$/sub@v1", SelfFault::Ref)] {
        let s = store();
        let e = resolve(&uses(&[u]), &Lock::default(), &s, &f, Mode::Online, "L").unwrap_err();
        assert_eq!(e, Error::SelfUses { uses: u.into(), fault }, "{u}");
        assert!(e.to_string().contains("$/PATH"), "{e}");
    }
}
