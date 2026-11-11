# pod-supervise

Detached payloads on Linux. One launcher process for each payload holds
the lock of its container, the pidfd of the payload and a control socket,
and writes the state into a table. The verbs of the lifecycle (create,
start, kill, stop, wait, remove, the logs) work on that table and talk to
the launcher. Four rules:

1. The running state is the state of the launcher. Nothing reads it from
   `/proc`, and nothing decides "running" by a sleep and a look.
2. A launcher holds the lock of its container for its whole life. A lock
   that another process can take is a launcher that is gone: the record
   becomes `Dead`, with the time of the notice and no exit code.
3. Nothing waits on a timer. Readiness is a pipe that the `execve` of the
   payload closes; the end of the payload is `ppoll` on its pidfd; each
   wait has a bound and its own result for the bound.
4. Nothing on the path that forks, enters and execs starts a thread
   (`PR_SET_PDEATHSIG` fires at the exit of a thread).
5. The launcher is the child subreaper of the payload's tree: a
   descendant whose parent ends comes to the launcher, also after a
   `setsid` or a double fork. At the end of the payload the launcher
   kills each descendant (`Settings::kill_tree`), then writes the end
   into the record before a waiter gets the code: the reason, the run
   time, the use of the tree and the count of the processes that stayed
   (`residual_processes`: a number, or `unknown`, never 0 when it is not
   known). The crate sets no `RLIMIT_FSIZE`.

The crate is Linux only (`#![cfg(target_os = "linux")]`): on another
target it is empty. It takes the raw system calls from `pod-hostprobe`
(`sys`).

## API

| Item | What |
| --- | --- |
| `Settings` | `root` (the state directory), `table_lock` (the lock of each write of the table), `table_key` (the key of the version in the table file), `name_prefix` (of a generated name), `list_command` (named in the message of a name that is not found), `failed_code` (the exit code of a payload that cannot be reaped; docker's 125), `kill_tree` (kill each descendant at the end of the payload), `proc` (the mount of procfs that gives the descendants). `Settings::new(root)` gives `root/lock`, `pod_supervise`, `pod`, none, 125, true, `/proc` |
| `set_wall(&s, want, wall_ms)` | the wall bound of a container that does not run: at `wall_ms` after the start the launcher kills the payload and its tree, and the end reads `wall` |
| `end` | `End { reason, run_ms, user_ms, system_ms, max_rss_kib, residual_processes }` in `Container::end`; `Reason` (`exit`, `signal`, `wall`); `Residual` (`Count(n)` or `Unknown`, JSON a number or `"unknown"`). `user_ms`, `system_ms` and `max_rss_kib` are `getrusage(RUSAGE_CHILDREN)` of the launcher after the kill; `max_rss_kib` is the largest one process, not a sum |
| `create(&s, name, argv, env, working_dir, of) -> Container<X>` | a `created` record with the fields `of` of the caller, and its directory; a name that is in use is refused |
| `start(&s, want, enter) -> Container<X>` | forks a detached launcher and returns when the payload runs; `enter(&Container<X>, Launch) -> Result<Payload, String>` runs in the launcher and starts the payload |
| `kill`, `stop`, `wait`, `remove`, `list`, `get`, `matching` | the verbs; `stop(&s, want, ms)` is SIGTERM, a bounded wait, then SIGKILL (a zero bound is SIGKILL at once), and gives `true` when SIGKILL ended it; `wait` gives `None` at the bound, never a guess |
| `logs`, `follow`, `follow_from`, `follow_tail`, `tail_offset` | the log of the payload's stdout and stderr; a follow polls each 100 ms until a terminal state, with one last read |
| `new_id`, `generated_name(&s, id)`, `KILL_WAIT_MS` | a random id of 64 hex characters; `PREFIX_` and 12 characters of it |
| `table` | `Container<X>` (the fields of the crate; `of: X` flattened into the same JSON object, and `Deref` to `X`), `State` (`Created`, `Running`, `Exited`, `Dead`), `Table<X>`, `Any` (each unknown key kept), `read`, `update`, `reconcile`, `find`, `encode`, `decode`, `dir`, `lock_path`, `log_path`, `control_path`, `control_via_fd`, `contained`, `now` |
| `launcher::tree` | `descendants(proc, pid)` (the `children` files, else a scan of each `stat`), `parent_of(stat)`, `TREE_KILL_MS` (the bound of the kill of the tree after the end of the payload, 5 s) |
| `launcher` | `Launch { pass, log }` (the descriptors of the payload: 0 on `/dev/null`, 1 and 2 on the log; a writer to the log), `Payload { pid, keep }`, `start`, `signal`, `wait`, `WaitError { Gone, Failed }`, `exit_status`, the control lines `SIG` and `WAIT` |
| `lock` | `Lock`: `try_exclusive`, `exclusive`, `try_acquire(path, op)`, `try_acquire_noting` (a hook at the refusal), `acquire` (`ATTEMPTS` tries, `PAUSE` between), `fd`, `hand_to_payload`; `O_CLOEXEC`, closed in each child of `clone_fork`, an explicit `LOCK_UN` at the drop |

Each failure is a `pod_core::Failure` at `Stage::Host`. A failure of I/O
keeps its cause, with the text `PATH: CAUSE`. The classes: `Usage` (a
name in use, a state that refuses the verb, a prefix of two ids, a path
with a NUL), `NotFound` (no such container), `Unavailable` (no launcher
listens, a fork failed, no slot for a lock), `TempFail` (a lock held after
the bound), `Protocol` (a table that does not parse, an answer of a
launcher that does not parse).

The crate reads no variable of the environment and prints nothing. A
caller that keeps other records in `root` gives their lock as
`table_lock`, so that the two writers do not interleave. A type `X` of
fewer fields than the file drops the others at the next write: use
`table::Any` to keep each key.

## Parity

Against podvm `crates/podvm-supervise` and `crates/podvm-image/src/store/files.rs`
at `27ae993`:

| Capability of podvm | Here |
| --- | --- |
| [x] The table, `State`, the record, the paths, the write by rename under a lock, `reconcile`, `find` | `table` (the fields of podvm are `X`; the version key is a setting) |
| [x] `create`, `kill`, `stop`, `wait`, `remove`, `list`, `get` | the same verbs, with `&Settings` in place of `&Store` |
| [x] `referencing` | `matching(&s, keep)` |
| [x] The logs, the follow, the tail | `logs` |
| [x] The launcher: the double fork, the ready pipe, `/dev/null` stdio, the lock, the control socket through `/proc/self/fd`, the pidfd, the forward of SIGINT and SIGTERM, the waiters | `launcher` |
| [x] The fix of podvm#215 (`stop -t 0`): `WaitError { Gone, Failed }`, a zero bound, `KILL_WAIT_MS` | yes, with its test and a lifecycle step that fails without it |
| [x] The start of podvm: the probe, the rung, the plan of `podvm-enter`, the image hold, the devices, the ownership memo | the `enter` of the caller (stays in podvm) |
| [x] `table/memo.rs`, `MEMO_FD_VAR`, `MEMO_CHILD_FD`, `memo_path` | stay in podvm (the format of podvm-interpose) |
| [x] `Lock` of `podvm-image` | `lock::Lock`; the instrument of PodUtils/podvm@T-0215 is the `note` of `try_acquire_noting` |
| [x] `EXIT_RUNTIME_ERROR` | `Settings::failed_code` |
| [x] The tests: 13 unit tests of the moved parts, with their names (`a_table_without_the_userland_drive_still_parses` with `X`); the memo and tally tests stay in podvm | yes; 10 new unit tests; `tests/lifecycle.rs` runs 5 steps with real payloads |
| [ ] The kill of the tree that podvm asks (PodUtils/podvm@T-1655) and podrunner's shim (PodUtils/podrunner#23) | new here (T-162): the subreaper, `kill_tree`, `set_wall`, `end`; podvm's launcher takes it with the tag |

Differences: the status of a dead record says "the launcher was not
watching" (podvm: "podvm was not watching"); a removal checks its
directory against the directory of the containers, not the containment
gate of podvm's store; the times come from `pod_core::clock::rfc3339`
(the same form as podvm's `clock::now`).

## Adoption map (podvm)

| Old path | New path |
| --- | --- |
| `podvm_supervise::{create, start, kill, stop, wait, remove, list, get, logs, follow, follow_tail}(&store, ...)` | `pod_supervise::*(&settings, ...)`; podvm keeps its functions as calls with `settings(store)` |
| `podvm_supervise::referencing(&store, digest)` | `pod_supervise::matching(&settings, \|c\| c.manifest_digest == digest)` |
| `podvm_supervise::{new_id, tail_offset}` | `pod_supervise::{new_id, tail_offset}` |
| `podvm_supervise::generated_name(id)` | `pod_supervise::generated_name(&settings, id)`, `name_prefix` podvm |
| `podvm_supervise::Error(String)` | `pod_core::Failure`; podvm keeps `Error` with `From<Failure>` (the text of the chain) |
| `podvm_supervise::EXIT_RUNTIME_ERROR` (125) | `Settings::failed_code`, podvm's value `EXIT_RUNTIME_ERROR` |
| `table::Container` | `pod_supervise::table::Container<Detail>`; podvm's `Detail` holds `image`, `manifest_digest`, `rootfs`, `rung`, `userland_exec`, `completion`, `completion_degraded` and `completion_note`; a literal puts them in `of` |
| `table::Table { podvm_containers, containers }` | `Table<Detail> { version, containers }`; `Settings::table_key` = `podvm_containers` keeps the file |
| `table::{read, update, reconcile, find, dir, lock_path, log_path, control_path}` | `pod_supervise::table::*` with `&settings`; podvm keeps calls with `&Store` |
| `table::update` under `store.lock()` | `Settings::table_lock` = the store's `lock` file |
| `table::{memo_path, MEMO_FD_VAR, MEMO_CHILD_FD, memo_fd_env, memo_fd_of, open_memo, ensure_memo_file, emulated_counts, EmulatedCounts}` | stay in podvm |
| `launcher::start(store, &c, record)` | `pod_supervise::launcher::start(&settings, &c, enter)`; podvm's `launcher::enter(store, record)` opens the memo and the devices, holds the image, builds the `Plan` and spawns |
| `launcher::{signal, wait, WaitError, CTL_SIGNAL, CTL_WAIT}` | `pod_supervise::launcher::*` |
| `podvm_image::store::Lock` | `pod_supervise::lock::Lock` (re-exported by `podvm_image::store`); `Lock::try_acquire`, `Lock::acquire` become podvm's `try_lock_at`, `lock_at` (podvm's errors, the note of PodUtils/podvm@T-0215 in test builds); `held.fd` is `held.fd()` |
| `LOCK_ATTEMPTS`, `LOCK_SLEEP` of `podvm-image` | `pod_supervise::lock::{ATTEMPTS, PAUSE}` (the same 100 and 50 ms) |
| `crates/podvm-supervise/src/launcher/supervise.rs` | leaves podvm |

## Tests

`cargo test -p pod-supervise`. The unit tests use a temporary directory
each. `tests/lifecycle.rs` and `tests/tree.rs` have no libtest
(`harness = false`): the launcher forks from the test process, and a fork
from a process with more than one thread can stop in a lock that another
thread held. Each skips with a line `skipping, REASON` on a host with no
`/bin/sh` or `sleep` (and `tree` with no `/proc`). `tree` runs a
grandchild that calls `setsid` and ignores SIGTERM: the kill leaves 0
processes; with `kill_tree` false the count is 1 or more (the positive
control); with no `/proc` the count is `unknown`; a wall bound of 800 ms
ends the tree within the bound and `TREE_KILL_MS`; the payload has the
`RLIMIT_FSIZE` of its starter.

## Sources

`SOURCES` lists each imported file of podvm with its blob, and the
`check-sources.py` check compares them with podvm's `main`. The file of
podvm that stayed (`table/memo.rs`) is a `skip` line.
