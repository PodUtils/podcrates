# pod-manual

The help and the manual of a command line from one model that the program
fills. Entry T-700 (PodUtils/podnet#584, call C681). Sources: podnet
`src/manual/` and `scripts/gen-llms.py` at `78a3911` (the base), and the
generic part of podssh `crates/podssh-cli/src/man/` and `pager.rs` at
`cfc3ae3` (`SOURCES`).

A program keeps one model of its command line: the commands, the topics,
the facts and the examples. The help of each command, `CLI man` as text,
the man page, the JSON and `llms.txt` come from it. The tests of the
program call the checks of this crate, so that a drift between the model,
the code, the parser and the files of the repository fails a test.

## API

| Item | What |
| --- | --- |
| `Manual`, `Command`, `Arg`, `Flag`, `FlagKind`, `Availability`, `Parity`, `Status`, `Rule`, `Group`, `Topic`, `Block`, `Span`, `Example` | the model: commands with other names, flags with a kind (wired, ignored, refused with what to use, not in this release with its exit code; an empty reason writes the short sentence alone) and a status against another tool, the commands that only a build with a feature has (`Command::availability`: the sentence why, and the exit code of each other build, in the help, the page and the JSON), the topics inside the section of a command (`Command::topics`: no section of their own; `man KEY` selects the command), the variable of a flag or an argument, rules between flags and arguments (excludes, needs, needs one of, one of), the commands under a command in groups (`Command::groups`) and a "Start here" block (`Command::lead`), topics of blocks |
| `Facts`, `VarGroup`, `Var`, `Use`, `Exit`, `Code`, `FileFact`, `ConfigFile`, `ConfigKey`, `Table` | the typed facts: variables in groups, exit codes (a value, a range, `128+N`, the code of the payload), files, the keys of a configuration file, the tables that a program adds |
| `facts::failure_classes()` (feature `core`) | the exit codes of the classes of `pod_core::failure` as facts |
| `Manual::named(argv0)`, `Manual::for_repository()`, `Manual::with_help_flags()`, `model::date_of(secs)` | the name of `argv[0]` (podvm as `docker`); no version and no date for a file of a repository; `-h, --help` on each command; the date of a man page from `SOURCE_DATE_EPOCH` |
| `clap::read(cmd)`, `clap::parser(cmd)` (feature `clap`) | the model of a clap tree, with the variable of each flag and argument (`env`) and its rules (a positional argument by its value name): its groups, its conflicts, and its `requires` and `requires_if` as `Needs` (a `requires` of a group as a need of one of its arguments), found by a parse of each argument alone in a copy of the tree (clap has no getter of them); the rows of its parser for `check::flags` |
| `text::render`, `roff::render`, `json::document`, `json::read`, `llms::index`, `llms::full`, `llms::from_json` | the writers: plain text, man(7), JSON and its reader, `llms.txt` and `llms-full.txt` |
| `sections::sections`, `sections::find` | the sections of the page: each command, topic and table of facts |
| `help::request(manual, args)`, `help::render`, `help::answer` | `CLI --help`, `CLI COMMAND --help`, `CLI help COMMAND`, and `--json` |
| `Request`, `body`, `run` | `CLI man [SECTION...] [--no-pager \| --roff \| --json]`, through the pager |
| `pager::{Tty, PagerEnv, choose, show, page, variables, find_program, SOURCE}` | the pager, and its variables as facts; `find_program` is `program::find` with no extension of `PATHEXT` |
| `program::{find, names, extensions, is_path, runnable, PATHEXT_DEFAULT}` | the lookup of a program in a search path, as a shell finds it; on Windows a name with no extension also matches each extension of `PATHEXT` (`.COM;.EXE;.BAT;.CMD` when it is not set). The caller gives `PATH` and `PATHEXT`; `pod-testkit` (`host::tool`) and the pager use it |
| `pager::Interrupts::ignore()` | SIGINT and SIGQUIT ignored until the value drops, then each action put back: for a child that reads the terminal (a pager, an editor) |
| `rules::{faults, char_faults, work_ids, sentences, words, Kind, Fault}` | the text rules |
| `check::{texts, generated, variables, exit_codes, flags, model, help, examples, assert_none, Finding}` | the checks that a program calls from its tests |
| `scan::{rust_sources, non_test, variables_read, exit_constants, src, CALLS, LISTS}` | the reader of the source of a program for `check::variables` and `check::exit_codes`: only the calls that read or set a variable |
| `error::fault(option, what, fix)`, `error::check` | `OPTION: what is wrong; fix: what to do`, and its rules |

## The pager

The pager runs only when stdin and stdout are a terminal and the request
is the text with no `--no-pager`. It takes `PAGER` when it is set (empty or
`cat`: no pager); else `less` where a probe of `PATH` finds it and `TERM`
names a terminal that is not `dumb`; else the built-in pager of this crate
(call C682). It starts no other program. It needs no dynamic library.

## The text rules

`rules::faults` finds an emoji or a pictograph, a control character but the
newline and the tab (an escape sequence of a terminal starts with one), a
bidirectional control, a work id (`T-NNN`, `C123`, `Q12`, `Q-A8`,
`COORD-NNN`, `#123`, `podnet#584`), and a sentence of more than 25 words
(20 in a procedure: a step of `Block::Steps` and the fix of an error).
Call C683 gives how a sentence and its kind are found. `check::texts`
applies them to each string of the model and to the help, the text and
the man page that the writers make. `scripts/check-strings.py` applies a
part of them to the strings of a release binary (call C684).

## The checks of a program

Each program calls these from its own tests (`podlab`:
`crates/pod-lab/src/bin/podlab/tests.rs`):

1. `check::texts(&manual)` and `check::model(&manual)`: the text rules,
   the rules name flags or arguments of their command, the groups of a
   command name each command under it once, each name selects one
   section.
2. `check::generated(path, &text, fix)`: a man page or `llms.txt` of the
   repository equals the text of the model now
   (`Manual::for_repository()`).
3. `check::variables(&manual, &scan::rust_sources(..), exempt)`: each
   variable that the code reads or sets is a fact, both ways. Give
   `pager::SOURCE` too when the program pages. The reader takes the first
   argument of `env::var`, `var_os`, `set_var`, `remove_var`,
   `Command::env`, `env_remove`, `getenv` and `setenv` of C, clap's
   `.env("X")` and `env = "X"`, and each name in a constant whose name ends
   in `_VARIABLE` or `_VARIABLES`: name so a variable that the program
   reads by a name that is not a literal of a call. The variable of each
   flag and argument of the model is a read of the parser. `env!` and
   `option_env!` read the build, not a run: no fact. `exempt` is for a
   variable that only a build reads and a fact that only a library reads.
4. `check::exit_codes(&manual, &given)`: each exit code that the program
   gives has a fact, both ways (`scan::exit_constants` reads the
   constants).
5. `check::flags(&manual, parser)`: each command and flag of the model
   against the parser, both ways (`clap::parser` for a clap program).
6. `check::help(&manual)`: the help of each command, as `COMMAND --help`
   answers it, holds its flags, the titles of its groups and its "Start
   here" lines. `check::examples(&manual, parse)`: the parser takes each
   example of the program.

`scripts/check-man.sh BINARY` renders `BINARY man --roff` with groff and
mandoc, with no warning, and finds each flag in both renderings as a word
of its own. Its plant is a flag of a generated name that no text of the
page holds.

`scripts/check-strings.py [--data DIR] BINARY` checks the strings of a
release binary (call C684). It removes the values of the service files of
`pod-services` from a run with a fault (`--data`: the `services/` of the
same tag as the binary).

## The help of the root

`Command::groups` puts the commands under a command in tables by purpose,
and `Command::lead` puts a "Start here" table first. The help, the text
and the man page show them before the options, so `CLI --help` can come
from the model (`podlab`).

## Parity list

- [x] podnet: the command tree read from clap (now one reader), the text
      writer, the man(7) writer, `--json`, `CLI man SECTION` with more than
      one section, `--help --json`, the pager, the typed facts, the
      both-way test of the variables, `gen-llms.py` (in Rust).
- [x] podssh: the model of blocks, the man(7) writer with its list of
      requests and its escape, the text writer with its tables, the kind of
      a flag and what to use in place of a refused one, other names of a
      command, the pager with its built-in form, the reader of the
      variables with its test-only items, the check under groff and mandoc.
- [x] New: the status of a flag or a command against another tool, rules
      between options, variables in groups, the code of the payload, the
      keys of a configuration file, the tables of a program, the name of
      `argv[0]`, the help of each command, the text rules, the checks of
      the exit codes and of the parser, the form of an error, the check of
      the strings of a binary.
- [ ] The take of each consumer, and a parser driven by the model (T-700).

## Adoption map

| Consumer | Old | New |
| --- | --- | --- |
| podnet | `src/manual/model.rs` `build(cmd)` | `pod_manual::clap::read(cmd)`, then its facts, description and examples |
| podnet | `src/manual/{text,roff,pager}.rs`, `mod.rs` `body` | `pod_manual::{text, roff, pager}`, `pod_manual::{body, run}` |
| podnet | `src/manual/facts.rs` `ENVIRONMENT`, `FILES`, `EXAMPLES`; `crate::exit::TABLE` | `Facts { variables, files, exits }`, `Manual::examples` |
| podnet | `src/manual/inventory.rs` | a `Table` of the facts |
| podnet | `help_json_request` | `pod_manual::help::request` with `--json` |
| podnet | `scripts/gen-llms.py` | `pod_manual::llms::{index, full}` and `check::generated` in a test |
| podnet | the tests of `facts.rs` | `check::variables` with `scan::rust_sources` |
| podnet | `fill_env`, the filter of the rules, and the exempt words of a form of a variable in `src/manual/mod.rs` and `src/cli/manual_tests.rs` | `clap::read` (each `env`, the rules with an argument), the reader of the calls |
| podnet | `HELP_GROUPS`, `START_HERE` and the template of `src/cli/help.rs` | `Command::groups`, `Command::lead`, `help::request` and `help::answer` |
| podssh | `crates/podssh-cli/src/man/{model,text,roff,json}.rs` | the model and the writers of this crate; its facts, notes and topics fill `Topic`, `Facts` and `Table` |
| podssh | `flags.rs` `FlagRow`, `FlagKind` | `Flag` with `FlagKind` and `Parity`; `Verb::aliases` is `Command::aliases` |
| podssh | `pager.rs` | `pod_manual::pager` |
| podssh | `scripts/interop-man.sh` | `scripts/check-man.sh` of podcrates |
| podvm | its help of docker and podman | the model with `Parity` and `Manual::names` (`docker`, `podman`) |
| podrunner | its option table and `run.toml` | the model, `Rule`, `ConfigFile`; the errors through `error::fault` |
| each | its `--help` of clap | `help::request` and `help::answer` before the parser |

## Features

| Feature | What |
| --- | --- |
| `clap` | the reader of a clap tree and the rows of its parser |
| `core` | the exit codes of the failure classes of `pod-core` |

The crate prints nothing and does not exit. Only the pager reads the
environment, when the program asks it to page.
