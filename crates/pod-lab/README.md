# pod-lab

The classes of hostile hosts as profiles, the check of a profile on a
host, and a lab that holds a profile for a command. New code for the ask
of podrunner (PodUtils/podrunner#180) with the profiles of podvm
(PodUtils/podvm#424), podssh (PodUtils/podssh#268) and podnet
(PodUtils/podnet#585), with podnet's data of the network classes
(PodUtils/podnet#592); entries T-692, T-701 and T-716. `LANES.md` gives the
commands that each project runs in each profile. Linux only: on another target the
library holds its exit codes alone.

A profile is data. Each part of a profile names the rows of
`pod-hostprobe` that it must make refuse (with which errno) or allow, and
the mechanism that holds it in a lab. Thus a lab that allows what it
must refuse fails its own check.

## API

| Item | What |
| --- | --- |
| `PROFILES`, `find(profiles, name)` | the table, and a profile by its name |
| `Profile`, `Part`, `Observe`, `Expect`, `IdFact`, `LabFact`, `Hold`, `Call`, `When` | the data of a profile. `Observe`: a row, a fact of the identity, a `LabFact` that no row measures, a `LabCheck` that the lab measures beside the rows, or nothing. `Expect`: refused with one of the errno numbers, allowed, a Landlock ABI below a version, `Value` (an item `KEY VALUE` of the reason of a row of a fact) or `Shows` (the same, where another answer is unseen) |
| `Net`, `Proxy`, `NetFact`, `Ca`; module `net` | the stand-ins of a network lab, the facts that the lab measures through them, and their addresses, names and ports |
| `check(profile, &Findings) -> Verdict` | each part against the findings of `pod-hostprobe` |
| `check_report(profile, json)` (feature `report`) | the same against the JSON report of `pod_hostprobe::report` |
| `check_with(profile, &Findings, lab)` | the same, with a measurement of each `LabFact` |
| `rows(profile)`, `measure(&hostprobe::Settings, rows)` | the rows that a profile names, and a run of those rows only, each in its probe child |
| `Verdict`, `PartVerdict`, `Seen`, `State` | the verdict: each part `holds`, `fails` or `unseen`, each observation with its row and errno |
| `run(&Settings, profile, cmd, Options) -> Result<Ran, RunError>` | a command in a lab of the profile |
| `Options { partial, stdout, allow_connect, way_out, serve, serve_ca }` | run in the parts that hold, and report the others; the output of the command; the TCP ports that a connect may reach where a part refuses each TCP connect; a way out of a network lab; the ports of the servers of the command on the loopback of a network lab, and the PEM file of their roots |
| `net::check_served_ports`, `net::check_served_roots` | the checks of `serve` and `serve_ca` that `run` makes before the lab |
| `Settings { hostprobe, lab_flag, profiles }` | the flags of the probe children and of the lab process, and the table that the lab process reads |
| `dispatch(&Settings)` | the start of a binary that calls `run`: the work of a probe child or of the lab process |
| `exit_code(status)` | the code of a shell: the code, or 128 and the signal |
| `codes::{EXIT_*, facts}` | the exit codes of `podlab`; `facts()` (feature `manual`): its exit codes and its variables as the facts of a `pod-manual` model, the one table of them |
| `manual::{manual, docs, DOCS_BASE}` (feature `cli`) | the manual of `podlab`: the clap tree of its parser with the facts, the table of the profiles and the examples |

### `check`

A row that did not run (a skip of `pod-hostprobe`) never holds a part:
the part is `unseen`. A part holds only when each observation holds. One
observation that fails makes the part fail. A row that the findings do
not have, a `LabFact` and an `Observe::Nothing` are `unseen` in a check
of findings; a `LabCheck` is left out. `Expect::Value` holds where the row
is `ok` and its item has the value, and fails on another value or a
refusal. `Expect::Shows` holds the same way, and is `unseen` on another
answer: a host can hold the class in a way that the row does not read (a
filter in place of no route, a server of the file that does not answer).

The parts of the network classes that need a packet out (`proxy-reaches`,
`proxy-credential`, `port-refused`, `tls-reterminated`, `ssh-port-silent`,
`answer-per-name`, `sni-reset`, `literal-reaches`, `ipv6-reaches`) read the
rows of `pod_hostprobe::probes::NETWORK_ROWS`. These rows run only where the
caller sets `pod_hostprobe::Settings::network`: `podlab check PROFILE
--network NAME:PORT` (with `--network-other NAME`, `--network-literal
ADDRESS` and `--network-closed PORT`), or a report whose caller set it.
With no such setting the parts are `unseen`. A lab measures the same rows
against its stand-ins (`net::lab_targets`), and checks the facts of its
stand-ins beside them (`LabCheck`). `upgrade-refused` has no row.

### `run`

`run` starts the binary again with the lab flag: a lab process with one
thread. The lab process:

1. closes each descriptor from 3 up;
2. makes each mount of the profile in one new mount namespace (as root),
   or in a user namespace that maps the caller to 0 and then a second one
   that maps it back, so the command runs as its own user with no
   capability: a tmpfs on `/tmp`; a tmpfs on `/dev` with `null`, `zero`,
   `full`, `random` and `urandom` of the host and a FIFO at `/dev/tty`;
   `/etc/passwd` and `/etc/group` with no line of the caller; a bind of
   `HOME` with `noexec`, or of `/etc/ssl` read-only; an empty read-only
   tmpfs on `/proc` (last); a new network namespace, whose loopback is
   down, or (a network lab) whose loopback is up with the addresses, the
   resolver files and the listening sockets of the stand-ins;
3. starts the stand-ins of a network lab (below);
4. opens `/dev/null` at the descriptors 3 to 9, not close-on-exec, and
   starts a new session with no controlling terminal;
5. restricts itself with Landlock (no TCP port; or a path hidden: a rule
   on each entry of each directory on the way to the path, except the
   next step);
6. drops each capability: the ambient set, the bounding set, the three
   sets, then `no_new_privs`, so that an exec as root gets none back;
7. installs one seccomp filter, from the BPF program of `pod-confine`,
   after `no_new_privs` (a rule can test an argument: the family and the
   type of `socket(2)`);
8. in a network lab, sets the variables of a proxy and `SSL_CERT_FILE` of
   the class in its own environment; then measures the profile from
   inside: each row in its own probe child (which inherits the lab), the
   identity, each `LabFact` and each `LabCheck`;
9. replaces itself with the command when each part holds (or with
   `partial`). Else it exits 125, and the command does not run.

The command keeps the standard streams of the caller. `TMPDIR` is `/tmp`
where a part mounts a tmpfs there. A tmpfs on `/tmp` hides a command
under `/tmp`: give such a command as `/proc/self/exe` or from another
directory.

### A network lab

A network class (`proxy-only` and the six classes of podnet's data) is a
network namespace with no route out, in the user namespace of the mounts.
Its loopback is up, and the stand-ins of the class listen on it. Nothing
in the lab reaches the network of the host: each answer of "the outside"
comes from a stand-in.

* The stand-in process: the lab forks it two times before Landlock and
  the filter, so it is no child of the command. It holds a pidfd of the
  lab process, which is the command after the exec. It serves each
  connection in a thread, and exits when the command ends.
* Two certificate authorities, made for each run with P-256 keys of the
  kernel's random bytes: the CA of the stand-in servers (the public roots
  of the lab) and the CA of the stand-in proxy. `SSL_CERT_FILE` of the
  command is a new file of the temporary directory with the CA of the
  servers, and the CA of the proxy where the proxy terminates TLS. The
  stand-in removes the file when the command ends.
* The stand-in TLS server listens on `0.0.0.0:443`. It shows a leaf for
  the name of the ClientHello (or for the address that it reached), and
  answers each request with 200 and `pod-lab stand-in`.
* The stand-in proxy listens on `127.0.0.1:3128`. `HTTPS_PROXY`,
  `https_proxy`, `HTTP_PROXY` and `http_proxy` of the lab process name
  it, before the lab measures the profile, and the command inherits them;
  in a class with no proxy, the lab removes each variable of a proxy.
  The proxy variables of other tools (`net::vars`: global-agent of
  Node.js, npm, Yarn, a build of Docker, the Google Cloud CLI) go the same
  way: set to the stand-in proxy where the class has one, else removed. The
  options of a Java virtual machine (`JAVA_TOOL_OPTIONS`,
  `JDK_JAVA_OPTIONS`, `_JAVA_OPTIONS`) lose each option of a proxy.
  After a 200, the proxy is the server of the target itself.
* The names: `listed.test` is on the list of a class, `other.test` is not
  (RFC 6761 names). The address outside is `192.0.2.1` (RFC 5737): a
  class of direct connects puts `192.0.2.0/24` on the loopback; in another
  class no route reaches it (`ENETUNREACH`). `ipv6-only` puts
  `fd00:6c61:62::1` on the loopback.

| Profile | The stand-ins |
| --- | --- |
| `proxy-only` | the proxy asks for the credential `podlab` with a made-up password (407 with none), reaches the ports 443, 80 and 8443 (403 for another port); a seccomp filter refuses UDP |
| `proxy-tls-reterminate` | `tls-reterminated`, `ssh-port-silent` | inside the lab: TLS through the proxy to `listed.test` ends at the CA of the proxy, to `other.test` at the CA of the server; `CONNECT listed.test:22` 200, then no byte in 1 s | a network lab |
| `proxy-per-name` | `answer-per-name` | inside the lab: `CONNECT` 200 for `listed.test`, 504 for `other.test`, a reset for `192.0.2.1` | a network lab |
| `sni-allowlist` | `sni-reset` | inside the lab: a ClientHello to `192.0.2.1:443` for `listed.test` gets a ServerHello; for `other.test`, or with no name, a reset | a network lab |
| `no-proxy-no-dns` | `no-resolver`, `literal-reaches` | inside the lab: a lookup fails, no `nameserver` line, `127.0.0.1:53` refused; TLS to `192.0.2.1:443` checks the address | a network lab |
| `ws-upgrade-refused` | `upgrade-refused` | inside the lab: an HTTPS `GET` through the proxy 200, with `Upgrade: websocket` 403 | a network lab |
| `ipv6-only` | `no-ipv4-route`, `ipv6-reaches` | inside the lab: `connect(192.0.2.1:443)` ENETUNREACH, `127.0.0.1:443` reaches; `[fd00:6c61:62::1]:443` reaches | a network lab |
| `bind-netns-vm-denied` | `tcp-bind-refused`, `netns-refused`, `vm-write-refused`, `unix-allowed` | `bind(127.0.0.1:0)` EACCES and `connect(127.0.0.1:1)` allowed (inside the lab also: bind of `[::1]:0` EACCES); `clone(CLONE_NEWUSER|NEWNET)` EPERM or ENOSPC; `process_vm_writev(own pid)` EPERM; inside the lab: AF_UNIX allowed | Landlock of the TCP bind alone (ABI 4); seccomp of `unshare` and `clone` with `CLONE_NEWNET` and of `process_vm_writev`; the host |

A part whose text says "Assumed:" holds the form of the class that podssh
or podnet named, not a measurement of a real network.

#### The way out (`--way-out`)

A live lane needs the real network through the proxy of its class.
`podlab run --way-out PROFILE -- CMD` (`Options::way_out`) gives it, for a
profile with a stand-in proxy alone.

* Before the lab enters its network namespace, it starts the process of
  the way out in the network of the host. That process dials directly, or
  through the proxy of the host (`https_proxy`, `HTTPS_PROXY`,
  `all_proxy`, `ALL_PROXY`, the order of curl) with its credential.
* The stand-in proxy applies the rules of its class first (the
  credential, the ports, 403, 504, a reset, the silent port). Then it
  sends each `CONNECT` to a name that is not of the lab to the way out, on
  a socket pair. The way out answers with the descriptor of the connected
  stream (`SCM_RIGHTS`).
* A name of the lab (`listed.test`, `other.test`, `localhost`, the
  addresses outside and the loopback) stays with the stand-ins, or goes to
  a server of the command (`--serve`, below). A direct
  way out refuses a name that the resolver of the host gives a loopback
  or an unspecified address.
* Where the class terminates TLS for a listed name, the stand-in checks
  the real server with the roots of the host, and shows the command a
  leaf of the CA of the lab.
* The certificate file of the lab (`SSL_CERT_FILE` of the command) holds
  the certificates that the host trusts for its way out after the CAs of
  the lab: the file of the host's `SSL_CERT_FILE`, else its first system
  bundle, else the built-in roots of `webpki-root-certs` (a host with no
  bundle). So a command checks a real server through a spliced `CONNECT`,
  also behind a proxy of the host that signs TLS again. An `SSL_CERT_FILE`
  of the host that does not read, or that holds no certificate, refuses
  the way out with its name.
* The command and the arguments of the lab never hold the proxy of the
  host or its credential. `podlab run --way-out` and each process of the
  lab that serves are not dumpable.

#### The servers of the command (`--serve`)

A lane can run its own server in the lab, on the loopback of the lab,
and reach it through the stand-ins of its class. `podlab run --serve
PORT PROFILE -- CMD` (`Options::serve`, once for each port) gives it, in
each network class. The ports 0, 443 and 3128 are refused: the stand-ins
hold 443 and 3128.

* The stand-in proxy applies the rules of its class first (the
  credential, the ports, 403, 504, a reset, the silent port). Then it
  sends a `CONNECT` to a name of the lab (`listed.test`, `other.test`,
  `localhost`, the addresses outside and the loopback) at a served port to
  `127.0.0.1:PORT` of the lab. A name that is not of the lab, and a port
  that is not served, get the stand-ins (or the way out) as before.
* Where the class terminates TLS for the name (`listed.test` in
  `proxy-tls-reterminate` and `ws-upgrade-refused`), the proxy shows a
  leaf of its CA for the name, and checks the TLS server of the command
  with the roots of `--serve-ca FILE` (`Options::serve_ca`, PEM). The
  server of the command must show a leaf for that name. With no file, such
  a `CONNECT` gets 502. In `ws-upgrade-refused`, a request with `Upgrade`
  gets 403 and does not reach the command. Another name (`other.test`,
  the loopback) gets the bytes of the command unchanged.
* In `sni-allowlist`, the stand-in server on `192.0.2.1:443` sends a
  ClientHello with a listed name to the first served port, with the
  ClientHello. Another name, or no name, gets a reset. Where nothing listens
  on the port (the check of the lab before the command), the stand-in
  server answers.
* In a class with no proxy and no list of names (`no-proxy-no-dns`,
  `ipv6-only`), the command reaches its loopback directly: `--serve`
  changes nothing.
* A port must be on the port list of the class: `proxy-only` reaches 443,
  80 and 8443, so a server there listens on 8443.
* A spliced stream has no time limit of a read: a session can be idle.

## The profiles

`podlab list` prints the table. The parts, and what holds each one in a
lab with no root:

| Profile | Parts | Observed by | Held by |
| --- | --- | --- | --- |
| `userns-off` | `userns-refused` | `unshare(CLONE_NEWUSER)`, `clone(CLONE_NEWUSER)` and the user namespace leg refused (EPERM, ENOSPC) | seccomp |
| `bind-denied` | `tcp-bind-refused`, `tcp-connect-refused`, `unix-allowed` | `bind(127.0.0.1:0)+listen` EACCES; `connect(127.0.0.1:1)` EACCES; inside the lab: bind of `[::1]:0` EACCES, AF_UNIX socket, bind and connect allowed | Landlock (ABI 4); the host |
| `getsockname-denied` | `getsockname-refused` | inside the lab: EOPNOTSUPP | seccomp (not on targets with `socketcall`) |
| `proxy-only` | `direct-refused`, `proxy-variable`, `proxy-reaches`, `proxy-credential`, `port-refused`, `udp-refused` | `read(/proc/net/route)` and `read(/proc/net/ipv6_route)` show no default route (inside the lab also: `connect(192.0.2.1:443)` ENETUNREACH); `getenv(HTTPS_PROXY)` set, scheme `http`; inside the lab: `CONNECT listed.test:443` 200 and TLS to the CA of the server; 407 with no credential; 403 for port 22; as `udp-denied` | a network lab with the stand-in proxy; seccomp |
| `small-tmp` | `tmpdir-small` | inside the lab: `statfs` of `/tmp`, at most 1 GiB | tmpfs |
| `root-unreadable` | `root-list-refused`, `open-by-name` | `readdir(/)` EACCES; `open(/bin, O_RDONLY) by name` allowed | Landlock; the host |
| `nonroot-nocaps` | `not-root`, `no-caps`, `no-new-privs`, `seccomp-filter` | the identity; `setuid(0)`, `mknod(chr 1:3 /tmp/nodprobe)`, `ptrace(PTRACE_TRACEME)` EPERM | a caller that is not root; `no_new_privs`; the default list of `pod-confine` |
| `no-kvm` | `kvm-absent` | `open(/dev/kvm, O_RDWR)` ENOENT or EACCES | Landlock |
| `arm64-tcg` | `arm64`, `kvm-absent` | no row (T-695); as `no-kvm` | the machine tier of podvm (part 2); Landlock |
| `inherited-fds` | `fds-open` | inside the lab: 3 to 9 open, not close-on-exec | the lab process |
| `chroot-denied` | `chroot-refused` | `chroot(/tmp)` EPERM | seccomp |
| `userns-mount-denied` | `userns-made`, `userns-mount-refused` | `unshare(CLONE_NEWUSER)` and `clone(CLONE_NEWUSER)` allowed; the user namespace leg EPERM or EACCES | the host; seccomp of the mount calls |
| `cgroup-v1` | `cgroup-v1` | no row (T-695) | the machine tier of podvm (part 2) |
| `landlock-old` | `landlock-below-4` | `landlock_create_ruleset(VERSION)`: no Landlock or ABI below 4 | seccomp (`ENOSYS`: the form with no Landlock) |
| `proc-masked` | `proc-hidden` | `open /proc/self/mem O_RDONLY` ENOENT or EACCES | Landlock |
| `noexec-tmp` | `tmp-noexec` | `exec(/tmp/execprobe)` EACCES | tmpfs with `MS_NOEXEC` |
| `listen-denied` | `bind-allowed`, `listen-refused` | `bind(127.0.0.1:0)` allowed; `listen after bind(127.0.0.1:0)` EACCES or EPERM | the host; seccomp of `listen` (EACCES) |
| `udp-denied` | `udp-refused` | `socket(AF_INET, SOCK_DGRAM)+connect` EPERM or EACCES; `socket(AF_INET6, SOCK_DGRAM)+connect` EPERM, EACCES or EAFNOSUPPORT | seccomp of `socket(AF_INET or AF_INET6, SOCK_DGRAM)` (EPERM), `io_uring_setup` |
| `raw-denied` | `raw-refused` | `socket(AF_INET, SOCK_RAW)` EPERM; `socket(AF_PACKET, SOCK_RAW)` EPERM or EAFNOSUPPORT | seccomp of the raw and packet sockets (EPERM), `io_uring_setup` |
| `no-ptmx` | `ptmx-absent`, `no-ctty`, `tty-opens` | `stat(/dev/ptmx)`, `open(/dev/ptmx, O_RDWR)` and `stat(/dev/pts)` ENOENT; `tty_nr of /proc/self/stat` `tty_nr 0`; `open(/dev/tty, O_NONBLOCK)+poll` `read waits` | a tmpfs on `/dev`; a new session |
| `no-passwd-entry` | `no-id-lines` | `line of getuid() in /etc/passwd` and `line of getgid() in /etc/group` `lines 0` | binds of the two files |
| `noexec-home` | `home-noexec`, `tmp-exec` | `creat($HOME/.pod-hostprobe)` allowed with `noexec 1`; `exec(/tmp/execprobe)` allowed | a bind of `HOME` with `noexec`; the host |
| `no-proc` | `proc-absent` | `readlink(/proc/self/exe)` ENOENT, in the probe process | an empty tmpfs on `/proc` |
| `seccomp-nonewprivs` | `no-new-privs`, `no-caps`, `calls-refused` | the identity; `mknod(chr 1:3 /tmp/nodprobe)`, `unshare(CLONE_NEWUSER)`, `unshare(CLONE_NEWNS)`, `mount(2) bogus target`, `chroot(/tmp)`, `ptrace(PTRACE_TRACEME)` EPERM | `no_new_privs`; the drop of the capabilities (also for root); seccomp |
| `no-tun` | `tun-absent` | `open(/dev/net/tun, O_RDWR)` ENOENT or EACCES | Landlock |
| `no-net-admin` | `no-net-admin`, `tun-absent` | the identity (`CapEff` with no bit 12); as `no-tun` | the drop of the capabilities; Landlock |
| `ro-etc-ssl` | `etc-ssl-read-only` | `statfs(/etc/ssl)` `rdonly 1` | a read-only bind |
| `no-loopback` | `loopback-absent`, `unix-allowed` | `connect(127.0.0.1:1)` ENETUNREACH; inside the lab: AF_UNIX allowed | a network namespace; the host |
| `proxy-tls-reterminate` | `proxy-variable`, `cert-file`, `tls-reterminated`, `ssh-port-silent` | `getenv(HTTPS_PROXY)` set, scheme `http`; `getenv(SSL_CERT_FILE)+read` shows `set 1`; inside the lab: the CA of each name through the proxy, port 22 silent | a network lab |
| `proxy-per-name` | `proxy-variable`, `answer-per-name` | `getenv(HTTPS_PROXY)` set, scheme `http`; inside the lab: 200, 504 and a reset | a network lab |
| `sni-allowlist` | `no-proxy`, `sni-reset` | `getenv(HTTPS_PROXY)` `set 0`; inside the lab: a ServerHello or a reset for each name | a network lab |
| `no-proxy-no-dns` | `no-proxy`, `no-resolver`, `literal-reaches` | `getenv(HTTPS_PROXY)` `set 0`; `read(/etc/resolv.conf)` shows `nameservers 0` and `127.0.0.1:53 refused` (inside the lab also: a lookup fails); inside the lab: TLS to `192.0.2.1:443` | a network lab |
| `ws-upgrade-refused` | `proxy-variable`, `upgrade-refused` | `getenv(HTTPS_PROXY)` set, scheme `http`; inside the lab: `GET` 200, upgrade 403 | a network lab |
| `ipv6-only` | `no-ipv4-route`, `ipv6-reaches` | `read(/proc/net/route)` shows no default route and `connect(127.0.0.1:1)` allowed (inside the lab also: `connect(192.0.2.1:443)` ENETUNREACH); inside the lab: `[fd00:6c61:62::1]:443` reaches | a network lab |

The facts that no row of `pod-hostprobe` measures: the bind of `[::1]`,
AF_UNIX and `getsockname` (T-693); the size of `TMPDIR` and the inherited
descriptors (T-694); the machine of `uname(2)`, the cgroup version and the
`hidepid` form of `/proc` (T-695); the facts of the network classes that
need a packet through a proxy or to a server (T-751). The rows of the
socket and file classes (T-714, T-715) and what a host shows of its way
out with no packet out (T-745) observe each other part of the classes of
podssh and podnet. The printed texts name no entry.

## Features

* `report`: `check_report`, through the feature `report` of
  `pod-hostprobe`.
* `manual`: `codes::facts`, the facts of `podlab` for `pod-manual`.
* `cli`: the binary `podlab` (clap), `report` and `manual`. The help and
  the manual come from the model of `pod-manual` (module `manual`):
  `podlab --help`, `podlab COMMAND --help`, `podlab help COMMAND`,
  `podlab man [--no-pager] [--roff | --json] [SECTION...]`. `man/podlab.1`
  and `man/llms.txt` are generated from the model with no version and no
  date: `cargo test -p pod-lab --features cli -- --ignored
  write_the_generated_files` writes them. A build with
  `SOURCE_DATE_EPOCH` gives the date of the man page of the binary.

## Settings

`Settings::default()`: the probe child flag `--podlab-probe-child`, the
lab flag `--podlab-lab`, the table `PROFILES`. The crate prints nothing;
`podlab` prints. The lab process reads `HOME` (the bind of
`noexec-home`) and `TMPDIR` (the new files of `no-passwd-entry`, of the
resolver of `no-proxy-no-dns`, and the certificate file of a network
lab). In a network lab it sets `SSL_CERT_FILE` and the variables of a
proxy in its own environment before it measures the profile, and the
command inherits them. The rows of `pod-hostprobe` read `HOME`,
`SSL_CERT_FILE` and the variables of a proxy of the process that runs
them, in `podlab check` too.

## Tests

* `src/table/tests.rs`: each profile of the asks, a part and an
  observation in each, each row name a row of `pod-hostprobe`, each
  errno one that a filter can give, the filter of each profile compiles,
  each network class is one network lab, each part that podnet marked
  assumed says so, a row sees each part of the socket and file classes
  and the host parts of the network classes, a `LabCheck` stands beside a
  row.
* `src/net/wire.rs`, `src/net/setup.rs`: the head of an HTTP message, the
  server name of a ClientHello of rustls, the line `hosts:` of
  `/etc/nsswitch.conf`.
* `src/check/tests.rs`: `check` against made-up findings and a report.
  `src/check/rows.rs`: made-up reports of each part that a row of the
  socket, file and network classes sees, through the JSON of the report;
  a `LabCheck` counts in a lab alone.
* `src/text.rs`: each printed text and each string literal of the code:
  the rules of `pod_manual::rules`.
* `src/bin/podlab/tests.rs`: the checks of `pod-manual` over the manual of
  `podlab`: the text rules, the model against the parser of clap, the
  generated files, the variables of the code both ways, the exit codes
  both ways, the help of each command.
* `tests/cli.rs`: the binary prints its manual with no pager, one section,
  the JSON and the man page, refuses an unknown section with exit 2, and
  answers `--help`, `help COMMAND` and `--help --json` from the model.
* `tests/lab.rs` (no libtest): `inherited-fds`, `bind-denied` (with
  AF_UNIX), `small-tmp`, a profile that no lab here holds, a part whose
  check from inside fails; `tests/lab/hosts.rs`: each class of podssh and
  podnet that a lab here holds, with a payload that shows each part, and
  a class of the machine tier that the run refuses; `tests/lab/net.rs`:
  each network class with a payload that reads `HTTPS_PROXY` and
  `SSL_CERT_FILE` as a program does, then the removal of the certificate
  file; `tests/lab/serve.rs`: a server of the payload on a served port
  of `proxy-only`, `proxy-tls-reterminate` (no roots), `sni-allowlist` and
  `ws-upgrade-refused` (the roots of a CA of the test), reached through
  the stand-ins with the rules of the class. As root each case runs as
  root, then again as uid 65534. `scripts/ci/userns-tests.sh` runs it where a
  skip fails.
* `src/net/proxy/tests.rs`, `src/net/local.rs`, `src/net/relay.rs`: the
  stand-ins with a served port on the loopback of the test, the checks of
  the served ports and of their roots, and a splice that keeps an idle
  stream.
* Plants: `scripts/plants/lab-*`.
