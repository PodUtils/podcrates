# pod-unit

Hardened service units with no secret in them (T-102; cfb's A-S-02,
A-N-11, A-C-07, A-S-18 and A-N-12, GitHub #35). A program describes its
service once as a `Unit`; a renderer gives each form as text, for
`--print-unit`, and `install` installs and removes a unit of the user, and,
as root or an administrator, a unit of the system. The base is podssh's
`node_install/` (the texts, the quoting, the hardening of a system unit, the
steps of an install of the user and of the system); podnet's `autostart.rs` and
podrunner's `print-unit` add the line of the crontab with its log and the
start with `sd_notify`, the watchdog and the drain (`SOURCES`).

The crate is std alone (no tokio). It prints nothing and does not exit. The
command line (`--print-unit`, `--install`, `--uninstall`) stays in each
program (call C27).

## Use

```rust,ignore
use std::path::Path;
use pod_unit::{Start, Stop, Unit};

let mut unit = Unit::new("podrunner", Path::new("/home/ci/.local/bin/podrunner"), "podrunner print-unit")
    .args(["serve", "--config", "/home/ci/.config/podrunner/podrunner.toml", "--state", "/home/ci/.local/state/pr"]);
unit.description = "podrunner: CI jobs in one fresh unit for each job".into();
unit.start = Start::Notify { watchdog_s: Some(30) };
unit.stop = Stop { drain: true, timeout_s: None };

// --print-unit
print!("{}", pod_unit::systemd::render(&unit)?);

// --install, as the user
let places = pod_unit::Places::of_home(&home, &state.join("tmp"), uid, &windows_user);
let host = pod_unit::System::default();
let form = pod_unit::install::probe(&host, &places)?;
let done = pod_unit::install::install(&unit, form, &places, &host)?;
// --uninstall
let removed = pod_unit::install::remove(&unit, form, &places, &host)?;

// --install --system, as root (a unit made with .system(...))
let places = pod_unit::SystemPlaces::default();
let form = pod_unit::install::probe_system(&host)?;
let done = pod_unit::install::install_system(&unit, form, &places, &host)?;
let removed = pod_unit::install::remove_system(&unit, form, &places, &host)?;
```

A unit of the system with an account of its own, a credential and a
relaxed line:

```rust,ignore
let mut unit = Unit::new("podnet-relay-node", Path::new("/usr/local/bin/podnet"), "podnet unit print")
    .system(None) // DynamicUser=yes; Some("podnet") gives User=podnet
    .args(["relay", "node"])
    .secret_file("tunnel", Path::new("/etc/podnet/tunnel.token"), "TUNNEL_TOKEN_FILE")
    .placeholder("GITHUB_TOKEN", "PODCRED_GITHUB_TOKEN_0001")?;
unit.state_dir = Some("podnet-relay-node".into());
unit.hardening.family("AF_NETLINK", "the kernel gives the addresses of the host by netlink")?;
unit.hardening.relax("MemoryDenyWriteExecute", "the engine of rules compiles at run time")?;
```

`tests/golden/` holds the text of each form for these units.

## The forms

| Form | User (`Scope::User`) | System (`Scope::System`) |
| --- | --- | --- |
| systemd | `systemd::render`: `~/.config/systemd/user/NAME.service`, `WantedBy=default.target`, the lines of seccomp of the hardening | `systemd::render`: `/etc/systemd/system/NAME.service`, `User=NAME` or `DynamicUser=yes`, each line of the hardening, `WantedBy=multi-user.target` |
| launchd | `launchd::render`: a LaunchAgent, `~/Library/LaunchAgents/LABEL.plist` | `launchd::render`: a LaunchDaemon with `UserName` (a named user is necessary) |
| Windows | `windows::task_xml`: a task at the logon of the user and each 5 minutes, `LeastPrivilege` | `windows::service_commands`, `service_script`: a service of `sc.exe` (call C859) |
| cron | `cron::lines`, `cron::table`: `@reboot` and a watchdog each 5 minutes | none |

Each form:

* runs the program by its absolute path, with its words quoted for the
  form (`quote`);
* starts it again after each end (`Restart::always`) or after a failure,
  after `delay_s`; with no limit of starts on systemd;
* starts no loop after 64 (usage) and 78 (a configuration, or a revoked
  credential: `pod_core::Class::Revoked`), on systemd
  (`RestartPreventExitStatus=64 78`, A-C-07). launchd, the Task Scheduler
  and the service control manager have no rule of exit codes: there the
  delay bounds the loop;
* names what wrote it (`Unit::mark`) in its first comment: a removal
  removes only a file, a task or lines with that mark.

A form that cannot hold a part of a unit refuses the unit
(`Class::Unsupported`), and does not drop the part: launchd, the Task
Scheduler, a Windows service and cron have no `EnvironmentFile=` and no
`StateDirectory=`; a task has no variables and no log; a Windows service
has no directory of the run and no log.

## No secret in a unit

`Unit::check` (each renderer calls it) refuses a unit with a word that
looks like a secret, with the place of the word and the rule, never the
word (`secret`): the prefix of a known token (GitHub, GitLab, Slack, AWS,
Google, npm, PyPI, Hugging Face, DigitalOcean, `sk-`, base64 JSON as a JWT
or a tunnel token), a private key, a password in a URL, a variable whose
key names a secret (`TUNNEL_TOKEN`, not `TUNNEL_TOKEN_FILE`), a flag that
takes a secret (`--token VALUE`, not `--token-file PATH`), and a long random
word (32 characters or more of upper case, lower case and digits, 4.3 bits
or more for each). A secret is one of:

* a file (`Unit::secret_file(NAME, PATH, VARIABLE)`): on a unit of systemd
  of the system, `LoadCredential=NAME:PATH` and `VARIABLE=%d/NAME` (systemd
  copies the file, which only root may read, for the account of the
  service alone); on each other form, `VARIABLE=PATH` (a private file of the
  user);
* a placeholder of `pod-broker` (`Unit::placeholder(KEY, WORD)`, "Placeholder
  credentials" of `crates/pod-broker/README.md`): the job holds the word, and
  the broker swaps it for the value on the way to the hosts of the
  credential. The word has the form of the broker (16 to 256 of `A-Z a-z 0-9
  _ . -`); the rules of a token and of a random word still apply to it, so a
  real token given as a placeholder is refused;
* a file of variables of systemd (`Unit::env_file`), which the host keeps
  private.

## The hardening

`hardening::LINES` holds each line, on by default. A unit of the system
takes each line; a user unit takes the lines of seccomp alone
(`Need::Seccomp`), as a user manager applies a line of a mount namespace
only through a user namespace, and a host with no user namespaces then
refuses the start. `Hardening::relax(KEY, WHY)` keeps the line as a comment
with its reason; `Hardening::family` adds `AF_NETLINK`, `AF_PACKET` or
`AF_VSOCK` with its reason; `Hardening::add(KEY, VALUE)` adds a line past
the table (`PrivateUsers=yes`, `ProcSubset=pid`, `IPAddressDeny=any`, which
are not on by default: each breaks a program of PodUtils); `write`, `bind`
and `bind_read` give the paths
that `ProtectSystem=strict` and `ProtectHome=tmpfs` must let through.
`systemd::audit(TEXT, SYSTEM)` names each line that is neither in a unit
nor relaxed in it: a program checks its own unit with it, also one that a
person changed.

The unit of the system of `tests/golden/podnet-relay-node.service`, with no
relaxed line, has the exposure 2.0 or less ("OK") of
`systemd-analyze security --offline=true`; `systemd-analyze verify` accepts
each unit of the golden files. The tests run both where the host has
`systemd-analyze`, and `plutil -lint` of each plist where the host has
`plutil` (macOS); each skips with its reason elsewhere. The plist crate and
quick-xml read each plist and each task back on each host.

## The install of the user

`install::install(UNIT, FORM, PLACES, HOST)` and `install::remove` are
explicit calls; the crate chooses no directory (`Places`) and runs each
program through the caller's `Host` (`System` runs it within a limit, by
`pod_core::process::run_bounded`). `install::probe` finds the first form of
this host: `schtasks` on Windows; else `systemctl --user`, `launchctl` with
the login session `gui/UID`, then `crontab -l`.

| Form | Install | Removal |
| --- | --- | --- |
| systemd | the unit (0644), `daemon-reload`, `enable`, `restart` | `disable --now`, the file, `daemon-reload`, `reset-failed` |
| launchd | the plist (0644), `bootout`, `enable`, `bootstrap gui/UID PATH` | `bootout gui/UID/LABEL`, the file |
| task | the XML in UTF-16 in `Places::scratch`, `schtasks /Create /XML`, the file removed, `/End`, `/Run` | `/End`, `/Delete /F` |
| cron | `crontab -l`, the table with the lines, `crontab -` | `crontab -l`, the table with no lines, `crontab -` |

An install that does not start removes its file. A file or a task of
another mark is refused (`Class::Config`) and stays.

## The install of the system (call C885)

`install::install_system(UNIT, FORM, SYSTEM_PLACES, HOST)` and
`install::remove_system` install and remove a unit with `Scope::System`.
`SystemPlaces::default()` gives `/etc/systemd/system` and
`/Library/LaunchDaemons`; a test gives its own. `install::probe_system`
finds the manager: a service on Windows; else systemd when
`/run/systemd/system` is a directory, then launchd when `launchctl print
system` passes.

The checks come before any write, in this order:

1. The form holds the unit (each renderer; `Class::Usage` or
   `Class::Unsupported`). A task and cron hold a unit of the user alone.
2. The caller may change the services of the host (`Host::admin`: the
   effective uid 0, or on Windows a token that may create a service, by
   `OpenSCManagerW`). Else `Class::Refused`.
3. systemd is the manager of the host (`Host::systemd_booted`), for the form
   systemd.
4. The account of the unit is on the host and is not root (`id -u NAME`,
   which reads each user database of the host; `Class::NotFound`,
   `Class::Usage`).
5. Each file of a secret and the file of variables is a regular file, with
   no link followed, that no other may read (`Host::file_meta`): of root for
   systemd, which reads it for `LoadCredential=` and `EnvironmentFile=`; of
   the account of the daemon for launchd. A file of a secret is the caller's:
   put it in place first (`/etc/credstore/NAME`, mode 0600, of root, is the
   place of systemd). On Windows the file must be there.

| Form | Install | Removal |
| --- | --- | --- |
| systemd | the unit (0644) in `/etc/systemd/system`, `daemon-reload`, `enable`, `restart` | `disable --now`, the file, `daemon-reload`, `reset-failed` |
| launchd | the plist (0644) in `/Library/LaunchDaemons`, `bootout`, `enable`, `bootstrap system PATH` | `bootout system/LABEL`, the file |
| service | `sc.exe qdescription` (the mark), each command of `windows::service_commands`, `sc.exe start`; a service of an earlier install: `stop`, its `Environment` deleted, then `config` in place of `create` | `sc.exe stop`, `sc.exe delete` |

A unit that does not start is removed (the file, or `sc.exe delete`). A
file or a service of another mark is refused (`Class::Config`) and stays.
A removal leaves the files of the secrets and the state directory
(`StateDirectory=`): each holds the data of the program, which a new
install takes again.

`tests/system_live.rs` installs a unit with `DynamicUser=yes` and a
credential on the systemd of the host, reads its state with `systemctl
show`, and removes it; the directory of the units is then as before. It
needs root and systemd as the manager, and skips elsewhere;
`scripts/ci/root-unit-tests.sh` runs it as root in `ci.yml`, where a skip
fails.

## The Windows service (call C859)

The form of a unit of the system is the commands of `sc.exe`, which an
administrator runs: `create` with `obj= "NT SERVICE\NAME"` (a virtual
account of the service alone) or an account with no password
(`LocalService`, `NetworkService`, `NT SERVICE\...`, a managed service
account `DOMAIN\NAME$`); an account with a password is refused, as the
password would be in the command. Then `sidtype restricted` (a
write-restricted token), `privs SeChangeNotifyPrivilege` (the least
privileges), `failure` with a restart after `delay_s` and a reset after a
day, `failureflag 1` (an end with a code that is not 0 is a failure), and
the variables in the key `Environment` of the service (`reg.exe`).
`windows::service_removal` stops and deletes it. Not taken: the XML of a
service wrapper (WinSW, NSSM), a second program beside each binary, which is
not static on each target.

## The dispatcher of a Windows service (T-1026)

The service control manager starts the program and waits for its call of
`StartServiceCtrlDispatcherW`; it ends a program that does not call it with
error 1053 after 30 s. `windows::serve(NAME, WORK)` (on Windows, with
windows-sys alone) makes that call on the thread of its caller and runs
`WORK` on the thread of the service:

```rust,ignore
use std::time::Duration;
use pod_unit::windows;

// The unit of the service form gives the word: .args(["serve", "--service"]).
if args.service {
    let code = windows::serve("podrunner", move |service| {
        let mut work = match Work::start() {   // the service reports a pending start
            Ok(work) => work,
            Err(failure) => return failure.class.exit_code().into(), // 78 stays 78
        };
        let _ = service.ready();               // running: the manager may stop it
        while service.stop().try_recv().is_err() {
            work.tick();                       // until sc.exe stop, or a shutdown of the host
        }
        while work.draining() {
            let _ = service.stopping(Duration::from_secs(10)); // a pending stop, the next checkpoint
            work.tick();
        }
        work.code()                            // 0, or the exit code of the work
    })?;
        std::process::exit(code);
}
```

| Event | The report to the manager |
| --- | --- |
| the start of `serve` | `SERVICE_START_PENDING`, no control accepted, a wait hint of `WAIT_HINT` (30 s); `Service::starting(HINT)` gives the next checkpoint |
| `Service::ready` | `SERVICE_RUNNING`, a stop and a shutdown accepted |
| a stop or a shutdown of the manager | `SERVICE_STOP_PENDING` with `WAIT_HINT` at once, then `Signal::Stop` or `Signal::Shutdown` on `Service::stop` |
| `Service::stopping(HINT)` | `SERVICE_STOP_PENDING` with the next checkpoint: each step of a drain, and a stop that the work decides itself |
| the end of `WORK` with `CODE` | `SERVICE_STOPPED` with `exit_codes(CODE)`: 0 is a clean end; else `ERROR_SERVICE_SPECIFIC_ERROR` and `CODE` (78 stays 78), a failure for `failureflag 1`; a panic is `PANIC_CODE` (101) |

`serve` returns the code of `WORK` when the dispatcher ends, and the caller
exits with it. A program that the manager did not start gets `Class::Usage`.
`program_code(WIN32, SPECIFIC)` gives the code of the work back from a
status of `sc.exe query`.

`tests/service_live.rs` makes a service of its own binary with
`install::install_system` (the commands of `windows::service_commands`),
reads running, stops it with the manager, reads the pending stop with the
hint of the drain of the work, and reads the exit codes of the stopped
service (78, then 0); the removal leaves no service. It needs an
administrator; the Windows job of `nightly.yml` runs it, where a skip fails.

## The adoption map

| Consumer | Now | With pod-unit |
| --- | --- | --- |
| podrunner | `crates/podrunner/src/cli.rs`: `print_unit`, `exec_word`; `crates/podrunner/src/serve.rs`: `run` with `notify::send` and `notify::take_signal` | `Unit` with `Start::Notify`, `Stop { drain: true, timeout_s: None }`; `systemd::render` for `print-unit`; `install::probe` and `install::install` of the unit of `print_unit` for an install with no prompt (PodUtils/podrunner@T-507); `install::install_system` of the same unit with `.system(Some(USER))` for a host where the operator installs as root; on Windows, the unit of the service form with a word of the service in its arguments, and `serve::run` in `windows::serve`: `Service::ready` for `READY=1`, `Service::stop().try_recv()` for `take_signal`, `Service::stopping` for `STOPPING=1` |
| podssh | `crates/podssh-cli/src/node_install/`: `text.rs`, `system_text.rs`, `launchd.rs`, `windows.rs`, `unix.rs`, `system.rs`, `system_install.rs` | `Unit` of each node, tunnel and pipe; the hardening table and its relaxed lines of `serve` as `Hardening::relax`; `install::install_system` for `--install --system --run-as USER`; `--env-file` stays an argument; the probe of the cache of the user, the file of variables in the state directory and the lock of a run stay in podssh |
| podnet | `crates/podnet-core/src/autostart.rs`: `unit_text`, `crontab_with`, `probe` | `Unit` of `autostart run NAME --dir DIR`; `install::probe`, `install::install`, `install::remove`; the files `NAME.args` and `NAME.env` stay in podnet |
| cfb, githome | none | through podrunner and podnet: a system unit of a named user with no token, installed by `install::install_system` (A-S-02, A-N-11) |
