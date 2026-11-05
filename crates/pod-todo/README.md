# pod-todo

The checker of a work record in `TODO/`, as podssh, podnet and podcrates
keep it: a reader that CI runs, and a writer that moves a status, derives
the counts and moves the citations of an edited file. It has no
dependency.

## Commands

`check`, `set T-NNN open|partial|blocked|done`, `counts`, `next`,
`remap [--dry-run] FILE...`, with `--root DIR` and `--base REV`. The rules
of the record are in `TODO/RULES.md` of each repository.

## The facts of a repository

`TODO/todo.toml` gives them (`src/config.rs`): `tool`, `milestones`,
`categories`, `roadmap`, `questions`, `top_dirs`, `root_files`,
`documents` (with `crates/*/README.md` patterns), `document_dirs`,
`readings`. A tree with no file takes the facts of podssh. An id of
another record is `REPO@T-NNN` and is not looked up.

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
