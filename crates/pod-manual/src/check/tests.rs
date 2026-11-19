use super::*;
use crate::facts::Var;
use crate::sample::manual;

#[test]
fn the_sample_keeps_each_rule() {
    let m = manual();
    assert_none(texts(&m));
    assert_none(model(&m));
    assert_none(help(&m));
}

/// A command that the help does not select is a finding of [`help`].
#[test]
fn a_command_with_no_help_is_found() {
    let mut m = manual();
    m.commands[1].path = vec!["--help".into()];
    assert!(help(&m).iter().any(|f| f.what.starts_with("the help: ")), "{:?}", help(&m));
}

/// The control of [`texts`]: a fault in each kind of string is found at
/// its place in the model, not only in the text that a writer makes.
#[test]
fn a_fault_in_each_part_of_the_model_is_found() {
    type Plant = Box<dyn Fn(&mut Manual)>;
    let plants: Vec<(&str, Plant)> = vec![
        ("sample tunnel --url", Box::new(|m| m.commands[1].flags[0].help.push('\u{1F680}'))),
        ("the exit code 0", Box::new(|m| m.facts.exits[0].meaning.push_str(" See T-700."))),
        ("the topic notes", Box::new(|m| m.topics[0].blocks[0] = crate::sections::para("a \u{1b}[1m bold"))),
        ("the topic notes", Box::new(|m| m.topics[0].blocks[2] = Block::Steps { steps: vec!["do ".repeat(21)] })),
        ("sample tunnel", Box::new(|m| m.commands[1].notes.push("word ".repeat(26)))),
        ("the table keywords", Box::new(|m| m.facts.tables[0].rows[0][2].push_str(" (#181)"))),
        ("the variable HTTPS_PROXY", Box::new(|m| m.facts.variables[0].vars[0].meaning.push('\u{7}'))),
        (
            "sample tunnel",
            Box::new(|m| {
                m.commands[1].rules.push(Rule::Needs {
                    flag: "--url".into(),
                    value: None,
                    needs: vec!["C681".into()],
                    one: false,
                })
            }),
        ),
    ];
    for (at, plant) in plants {
        let mut m = manual();
        plant(&mut m);
        let found = texts(&m);
        assert!(found.iter().any(|f| f.at == at), "no finding at {at:?}: {found:?}");
    }
}

use crate::model::{Block, Rule};

#[test]
fn a_rule_that_names_no_flag_and_a_name_of_two_sections_are_found() {
    let mut m = manual();
    m.commands[1].rules.push(Rule::Needs {
        flag: "--url".into(),
        value: None,
        needs: vec!["--nothing".into()],
        one: false,
    });
    assert_eq!(model(&m).len(), 1, "{:?}", model(&m));
    let mut m = manual();
    m.topics[0].aliases.push("keywords".into());
    assert!(model(&m).iter().any(|f| f.what.contains("names two sections")), "{:?}", model(&m));
}

/// A need of one of two names is a rule; a need of one of a single name is
/// a finding, as its sentence names no choice.
#[test]
fn a_need_of_one_of_names_two_or_more() {
    let one = |needs: &[&str]| Rule::Needs {
        flag: "--url".into(),
        value: None,
        needs: needs.iter().map(|n| n.to_string()).collect(),
        one: true,
    };
    let mut m = manual();
    m.commands[1].rules.push(one(&["--socket", "NAME"]));
    assert_none(model(&m));
    assert_eq!(one(&["--socket", "NAME"]).sentence(), "--url needs one of --socket, NAME.");
    m.commands[1].rules.push(one(&["--socket"]));
    let f = model(&m);
    assert!(f.len() == 1 && f[0].what.contains("the rule of --url needs one of less than two names"), "{f:?}");
}

/// A rule may name a positional argument by its name: clap gives such
/// rules for a conflict with a positional.
#[test]
fn a_rule_may_name_an_argument() {
    let mut m = manual();
    m.commands[1].rules.push(Rule::Excludes { flags: vec!["--url".into(), "NAME".into()] });
    m.commands[1].rules.push(Rule::OneOf { flags: vec!["NAME".into(), "--socket".into()], required: true });
    assert_none(model(&m));
    m.commands[1].rules.push(Rule::Excludes { flags: vec!["--url".into(), "OTHER".into()] });
    let f = model(&m);
    assert!(f.len() == 1 && f[0].what.contains("OTHER, which is no flag and no argument"), "{f:?}");
}

/// The groups of a command name each command under it once, and no other.
#[test]
fn the_groups_name_each_command_once() {
    let group = |title: &str, words: &[&str]| crate::model::Group {
        title: title.into(),
        commands: words.iter().map(|w| w.to_string()).collect(),
    };
    let mut m = manual();
    m.commands[0].groups = vec![group("Publish a service", &["tunnel", "chat"]), group("Networks", &["net"])];
    assert_none(model(&m));
    m.commands[0].groups[1].commands.push("chat".into());
    m.commands[0].groups[0].commands.retain(|w| w != "tunnel");
    m.commands[0].groups[0].commands.push("nothing".into());
    let f: Vec<String> = model(&m).into_iter().map(|f| f.what).collect();
    assert_eq!(
        f,
        [
            "the command tunnel is in no group",
            "the command chat is in 2 groups",
            "a group names nothing, which is no command under this one"
        ],
        "{f:?}"
    );
}

/// The help of a command with groups and a lead: "Start here", then a
/// table for each group, before the options.
#[test]
fn the_help_shows_the_lead_and_each_group() {
    let mut m = manual();
    let root = &mut m.commands[0];
    root.lead =
        vec![crate::model::Example { command: "sample tunnel NAME".into(), meaning: "Expose a service.".into() }];
    root.groups = vec![
        crate::model::Group { title: "Publish a service".into(), commands: vec!["tunnel".into(), "chat".into()] },
        crate::model::Group { title: "Networks".into(), commands: vec!["net".into()] },
    ];
    let h = crate::help::render(&m, &[]).unwrap();
    let at = |t: &str| h.find(t).unwrap_or_else(|| panic!("no {t:?} in\n{h}"));
    assert!(at("Start here") < at("sample tunnel NAME"), "{h}");
    assert!(at("sample tunnel NAME") < at("Publish a service") && at("Publish a service") < at("Networks"), "{h}");
    assert!(at("chat") < at("Networks") && at("Networks") < at("net ") && at("net ") < at("Options"), "{h}");
    assert!(!h.contains("Commands") && !h.contains("Other commands"), "{h}");
    assert_none(texts(&m));
    m.commands[0].groups.pop();
    let h = crate::help::render(&m, &[]).unwrap();
    assert!(h.contains("Other commands"), "{h}");
}

#[test]
fn a_generated_file_must_equal_the_model() {
    let dir = std::env::temp_dir().join(format!("pod-manual-generated-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sample.1");
    let want = crate::roff::render(&manual());
    let fix = "run the writer";
    assert_eq!(generated(&path, &want, fix).len(), 1, "a missing file");
    std::fs::write(&path, &want).unwrap();
    assert_none(generated(&path, &want, fix));
    std::fs::write(&path, want.replace("\\-\\-url", "\\-\\-uri")).unwrap();
    let f = generated(&path, &want, fix);
    assert!(
        f.len() == 1 && f[0].what.contains("differs from the model") && f[0].what.ends_with("fix: run the writer"),
        "{f:?}"
    );
    std::fs::write(&path, format!("{want}more\n")).unwrap();
    assert_eq!(generated(&path, &want, fix).len(), 1, "a longer file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_variables_are_checked_both_ways() {
    let m = manual();
    let code =
        r#"std::env::var("HTTPS_PROXY"); var("https_proxy"); var("SAMPLE_PROTOCOL"); cmd.env("SAMPLE_CHILD", x);"#;
    let src = |c: &str| vec![(PathBuf::from("main.rs"), c.to_string())];
    assert_none(variables(&m, &src(code), &[]));
    let read_more = format!("{code} var(\"SAMPLE_NEW\");");
    let f = variables(&m, &src(&read_more), &[]);
    assert!(f.len() == 1 && f[0].what.contains("SAMPLE_NEW"), "{f:?}");
    assert!(variables(&m, &src(&read_more), &["SAMPLE_NEW"]).is_empty());
    let f = variables(&m, &src(&code.replace("SAMPLE_CHILD", "OTHER")), &["OTHER"]);
    assert!(f.len() == 1 && f[0].what.contains("SAMPLE_CHILD is a fact"), "{f:?}");
}

/// The variable of a flag (clap's `env`) is read by the parser: it holds
/// its fact with no read in the code, and it needs a fact.
#[test]
fn the_variable_of_a_flag_is_a_read() {
    let m = manual();
    let code = r#"var("HTTPS_PROXY"); var("https_proxy"); cmd.env("SAMPLE_CHILD", x);"#;
    let src = vec![(PathBuf::from("main.rs"), code.to_string())];
    assert_none(variables(&m, &src, &[]));
    let mut m2 = m.clone();
    for g in &mut m2.facts.variables {
        g.vars.retain(|v| !v.names.iter().any(|n| n == "SAMPLE_PROTOCOL"));
    }
    let f = variables(&m2, &src, &[]);
    assert!(f.len() == 1 && f[0].what == "the parser reads SAMPLE_PROTOCOL, and no fact names it", "{f:?}");
    assert_eq!(f[0].at, "sample tunnel");
}

/// The pager reads its variables, and its group of facts names each.
#[test]
fn the_pager_facts_and_the_pager_agree() {
    let m = Manual {
        facts: crate::facts::Facts { variables: vec![crate::pager::variables()], ..Default::default() },
        ..Manual::default()
    };
    assert_none(variables(&m, &[(PathBuf::from("pager.rs"), crate::scan::non_test(crate::pager::SOURCE))], &[]));
    let _: Var = Var::default();
}

#[test]
fn the_exit_codes_are_checked_both_ways() {
    let m = manual();
    assert_none(exit_codes(&m, &[0, 64, 66, 130]));
    let m2 =
        Manual { facts: crate::facts::Facts { exits: m.facts.exits[..4].to_vec(), ..Default::default() }, ..m.clone() };
    let f = exit_codes(&m2, &[0, 64, 3]);
    assert!(f.len() == 1 && f[0].what.contains("gives 3"), "{f:?}");
    let f = exit_codes(&m, &[0]);
    assert!(f.len() == 1 && f[0].what.contains("64"), "{f:?}");
    // The row of the payload does not hide a code of the program.
    let f = exit_codes(&m, &[0, 64, 3]);
    assert!(f.len() == 1 && f[0].what.contains("gives 3"), "{f:?}");
}

fn row(spellings: &[&str], takes_value: bool) -> ParserFlag {
    ParserFlag { spellings: spellings.iter().map(|s| s.to_string()).collect(), takes_value }
}

/// A parser that agrees with the sample, but for the change `edit`.
fn parser(edit: fn(&[String], &mut ParserCommand)) -> impl Fn(&[String]) -> Option<ParserCommand> {
    let m = manual();
    move |path: &[String]| {
        let c = m.commands.iter().find(|c| c.path == path)?;
        let mut p = ParserCommand {
            flags: c
                .flags
                .iter()
                .map(|f| ParserFlag { spellings: f.spellings(), takes_value: f.value.is_some() })
                .collect(),
            subcommands: m.children(path).iter().map(|k| k.path.last().unwrap().clone()).collect(),
        };
        edit(path, &mut p);
        Some(p)
    }
}

#[test]
fn each_flag_row_is_checked_against_the_parser() {
    let m = manual();
    assert_none(flags(&m, parser(|_, _| {})));
    type Edit = fn(&[String], &mut ParserCommand);
    let changes: [(&str, Edit); 5] = [
        ("a flag that the parser lacks", |p, c| {
            if p == ["tunnel"] {
                c.flags.remove(0);
            }
        }),
        ("a flag that the model lacks", |p, c| {
            if p == ["tunnel"] {
                c.flags.push(row(&["--new"], false));
            }
        }),
        ("a value that differs", |p, c| {
            if p == ["tunnel"] {
                c.flags[0].takes_value = false;
            }
        }),
        ("a short name that differs", |p, c| {
            if p == ["net", "ts-shims"] {
                c.flags[0] = row(&["-O", "--out"], true);
            }
        }),
        ("a command that the model lacks", |p, c| {
            if p.is_empty() {
                c.subcommands.push("new".into());
            }
        }),
    ];
    for (what, edit) in changes {
        assert_eq!(flags(&m, parser(edit)).len(), 1, "{what}");
    }
    assert_eq!(
        flags(&m, |p: &[String]| parser(|_, _| {})(p).filter(|_| p != ["chat"])).len(),
        1,
        "a command that the parser lacks"
    );
}
