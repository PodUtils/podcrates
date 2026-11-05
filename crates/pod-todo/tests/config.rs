//! The configuration of a tree (`TODO/todo.toml`): each fact of it reaches
//! the reader, and a wrong file is named.

mod common;

use common::Tree;

fn has(p: &[String], what: &str) -> bool {
    p.iter().any(|x| x.contains(what))
}

/// The control tree with a configuration that names its own facts, as
/// podnet's record does: milestones M0 to M9, the roadmap in TODO/, the
/// questions for the owner, and the studies as readings.
fn configured(name: &str) -> Tree {
    let t = Tree::new(name);
    t.write(
        "TODO/todo.toml",
        "tool = \"x-todo\"\nmilestones = [\"M0\", \"M3\", \"M4\"]\n\
         categories = [\"defect\", \"feature\"]\nroadmap = \"TODO/ROADMAP.md\"\n\
         questions = \"Questions for the owner\"\ndocuments = [\"README.md\", \"crates/*/README.md\"]\n\
         readings = [\"docs/research/\"]\n",
    );
    let roadmap = t.read("docs/ROADMAP.md");
    std::fs::remove_file(t.path().join("docs/ROADMAP.md")).unwrap();
    t.write("TODO/ROADMAP.md", &roadmap);
    t.plant("TODO/area.md", "`docs/ROADMAP.md`", "`TODO/ROADMAP.md`");
    t.plant("TODO/PROGRESS.md", "## Questions for the operator", "## Questions for the owner");
    t
}

#[test]
fn the_configured_control_agrees() {
    let t = configured("config-control");
    assert_eq!(t.problems(), Vec::<String>::new());
}

#[test]
fn a_milestone_that_the_configuration_does_not_name_is_found() {
    let t = configured("config-milestone");
    t.plant("TODO/todo.toml", "\"M0\", \"M3\", \"M4\"", "\"M0\", \"M4\"");
    assert!(has(&t.problems(), "the milestone `M3` is not one of M0, M4"), "{:#?}", t.problems());
}

#[test]
fn a_category_that_the_configuration_does_not_name_is_found() {
    let t = configured("config-category");
    t.plant("TODO/todo.toml", "[\"defect\", \"feature\"]", "[\"defect\"]");
    assert!(has(&t.problems(), "the category `feature` is not one of defect"), "{:#?}", t.problems());
}

#[test]
fn the_roadmap_of_the_configuration_is_read_and_is_no_area() {
    let t = configured("config-roadmap");
    t.plant("TODO/ROADMAP.md", "- T-002.", "- [ ] Something to do.");
    assert!(has(&t.problems(), "TODO/ROADMAP.md:10: an open item"), "{:#?}", t.problems());
}

#[test]
fn the_questions_heading_of_the_configuration_holds_the_questions() {
    let t = configured("config-questions");
    t.plant("TODO/PROGRESS.md", "## Questions for the owner", "## Questions for the operator");
    assert!(has(&t.problems(), "`## Questions for the owner`"), "{:#?}", t.problems());
}

#[test]
fn a_document_pattern_reaches_each_crate_readme() {
    let t = configured("config-documents");
    t.write("crates/x/README.md", "# x\n\nT-099 is not an entry.\n");
    assert!(has(&t.problems(), "crates/x/README.md:3: T-099 is not an entry of the index"), "{:#?}", t.problems());
}

#[test]
fn a_reading_at_a_time_and_a_foreign_id_are_not_read() {
    let t = configured("config-readings");
    t.write("docs/research/old.md", "# Old\n\nT-099 of that time.\n");
    t.write("docs/design.md", "# Design\n\nThe consumer takes it (PodUtils/podssh@T-684).\n");
    assert_eq!(t.problems(), Vec::<String>::new());
}

#[test]
fn a_wrong_configuration_is_named_with_its_line() {
    let t = configured("config-wrong");
    t.plant("TODO/todo.toml", "readings = [", "readinngs = [");
    assert!(has(&t.problems(), "TODO/todo.toml:7: `readinngs` is not a key"), "{:#?}", t.problems());
}

#[test]
fn the_tool_name_is_in_the_messages() {
    let t = configured("config-tool");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = ["--root".to_string(), t.path().display().to_string(), "check".to_string()];
    assert_eq!(pod_todo::run(&args, &mut out, &mut err), 0, "{}", String::from_utf8_lossy(&err));
    assert!(String::from_utf8_lossy(&out).starts_with("x-todo: the record agrees"));
}
