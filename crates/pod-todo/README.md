# pod-todo

The checker of a work record in `TODO/`, as podssh, podnet and podcrates
keep it: a reader that CI runs, and a writer that moves a status, derives
the counts and moves the citations of an edited file. It has no
dependency.

## Commands

`check`, `set T-NNN open|partial|blocked|done`, `counts`, `next`,
`remap [--dry-run] FILE...`, `issues --block NUMBER | --json`, with
`--root DIR` and `--base REV`. The rules of the record are in
`TODO/RULES.md` of each repository.

`check` also holds the map of the issues (`TODO/issues.md`) to the status
of the entries: a report, a deferred issue and a tracker are open exactly
when one of their entries is not done (`src/issues.rs`). `issues` prints
the status block of an issue, or each issue with its block as JSON for
`scripts/issue-sync.py` (`src/block.rs`); it refuses a map that breaks a
rule.

## The facts of a repository

`TODO/todo.toml` gives them (`src/config.rs`): `tool`, `milestones`,
`categories`, `roadmap`, `questions`, `top_dirs`, `root_files`,
`documents` (with `crates/*/README.md` patterns), `document_dirs`,
`readings`. A tree with no file takes the facts of podssh. An id of
another record is `REPO@T-NNN` and is not looked up.

Each rule below is off until a key of `TODO/todo.toml` turns it on, so a
new rule fails no consumer at once:

| Key | Rule |
| --- | --- |
| `issue_rules = "on"` | The state of each issue against its entries (`src/issues.rs`). |
| `defect_tests = "YYYY-MM-DD"` | An entry of the category `defect` that is `done`, with a first date in `## Done` on or after the key's, names a test in `## Done`: a function in a code span (a snake_case word with two `_` or more, alone or after a path or a module path: `a_b_c`, `cli_tests::a_b_c`, `tests/x.rs::a_b_c`), or a path under `tests/`, `scripts/` or `.github/workflows/`. Else `check` gives `FILE:LINE: T-NNN: a done defect names no test in its Done section`, and `set T-NNN done` refuses the entry (`src/defects.rs`). |

## Adoption map

A consumer keeps a small member crate, so that its alias `cargo todo` and
its lock stay its own:

```toml
# crates/podnet-todo/Cargo.toml
[package]
name = "podnet-todo"
publish = false

[dependencies]
pod-todo = { git = "https://github.com/PodUtils/podcrates", tag = "v0.1.0" }
```

```rust
// crates/podnet-todo/src/main.rs
fn main() {
    pod_todo::main()
}
```

podssh dispatches its own `interop` command before it calls
`pod_todo::run`. Each consumer writes its `TODO/todo.toml`:

| Consumer | Facts that differ from the defaults |
| --- | --- |
| podssh | none (`tool = "podssh-todo"`) |
| podnet | `tool = "podnet-todo"`; milestones M0 to M9; `roadmap = "TODO/ROADMAP.md"`; `questions = "Questions for the owner"`; its top directories and root files; `documents` with `CLAUDE.md` and `crates/*/README.md`; `readings` of `docs/releases/`, `docs/reports/`, `docs/research/` |
