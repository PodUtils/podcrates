# The lanes of the projects

A lane is the list of commands that a project runs in each profile before
a release. Each command runs in a lab:

```sh
podlab run PROFILE -- COMMAND...
```

`podlab run` stops with exit 125 when a part of the profile does not
hold, and the command does not run. `podlab list` gives each profile and
what holds each part. A profile whose parts are "not here" needs the
machine tier of podvm: its lane waits for that tier.

A network class (`proxy-only`, `proxy-tls-reterminate`, `proxy-per-name`,
`sni-allowlist`, `no-proxy-no-dns`, `ws-upgrade-refused`, `ipv6-only`) is
a network namespace with no route out and stand-ins on its loopback
(`README.md`, "A network lab"). A command there reaches only the
stand-ins: the stand-in proxy on `127.0.0.1:3128` (`HTTPS_PROXY`) and the
stand-in TLS server on port 443, whose CA is in `SSL_CERT_FILE`. Point a
command at the names `listed.test` and `other.test`, or at `192.0.2.1`,
to see the class; a name of the real network gets the answer of the
class for a name that is not on its list. The lab of a real way out is
T-746 and T-747.

A command that needs its own server (a relay, an SSH server) runs it on
`127.0.0.1:PORT` of the lab and gives `--serve PORT`: the stand-in proxy
sends a `CONNECT` to a name of the lab at that port to the server, with
the rules of the class (`README.md`, "The servers of the command"). Point
the client at `listed.test:PORT` to see the class act on the bytes; give
`--serve-ca FILE` with the CA of the server where the class terminates
TLS.

A row of a failure (a dead relay, a refused proxy, no DNS, a rejected
token) says the end that it wants:

```sh
podlab run --expect-exit CODE --expect-stderr TEXT PROFILE -- COMMAND...
```

`podlab run` gives 0 where the command ends with CODE and its error
output holds TEXT, and 4 with one line for each difference. A row that
wants a failure and gets a success, or gets another failure, does not
pass. Each flag alone checks its part alone. Give each lane a row of a
failure for each refusal that its manual names.

The sources: PodUtils/podssh#268 (podssh), PodUtils/podnet#585 (podnet),
PodUtils/podrunner#180 (podrunner) and PodUtils/podvm#424 (podvm).

## podssh

The commands of a release, in each profile below:

1. `podssh doctor`.
2. `podssh ssh` and `podssh proxy`, through the relay and through a node.
3. `podssh cp` of 200 MiB, in each direction.
4. `podssh serve` with `vi`, `less` and Ctrl-C.
5. `podssh pipe`.
6. A session that continues after a kill of its relay link.

| Profile | Held here | The commands |
| --- | --- | --- |
| `bind-denied` | yes | 1, 2, 5, 6, with `--allow-connect` of the port of the proxy |
| `listen-denied` | yes | 1, 2, 5, 6 |
| `udp-denied` | yes | 1, 2, 3 |
| `no-ptmx` | yes | 1, 4 |
| `no-passwd-entry` | yes | 1, 2, 3 |
| `noexec-home` | yes | 1, 2, 3 |
| `no-proc` | yes | 1, 2, 5 |
| `seccomp-nonewprivs` | yes | 1, 2, 3, 4, 5 |
| `ro-etc-ssl` | yes | 1, 2 |
| `no-tun` | yes | 1, 2 |
| `proxy-only` | yes: stand-ins | 1 to 6 |
| `proxy-tls-reterminate` | yes: stand-ins | 1, 2 |
| `proxy-per-name` | yes: stand-ins | 1, 2 |
| `sni-allowlist` | yes: stand-ins | 1, 2 |
| `no-proxy-no-dns` | yes: stand-ins | 1, 2 |
| `ws-upgrade-refused` | yes: stand-ins | 1, 2, 6 |

An example:

```sh
podlab run no-ptmx -- podssh doctor
podlab run seccomp-nonewprivs -- podssh doctor
podlab run --allow-connect 3128 bind-denied -- podssh ssh -T git@github.com
podlab run proxy-tls-reterminate -- podssh doctor
```

`bind-denied` refuses each TCP connect. podssh's host refuses each bind
and lets a connect reach its proxy alone (PodUtils/podssh#278):
`--allow-connect PORT` lets a connect reach that port, so the lab is the
target sandbox of podssh with no root. Give the port of the proxy that
`HTTPS_PROXY` names.

podssh runs these commands today with `scripts/release-short.sh BINARY
--net`, its release check, and `scripts/test_in_box.sh` with
`scripts/sandbox-check.sh`. Its Podman box is the first form of
`proxy-only`, `no-ptmx` and `seccomp-nonewprivs`.

## podnet

| Profile | Held here | The commands |
| --- | --- | --- |
| `bind-denied` | yes | `net shim` with a dynamic client (.NET, Python), `PODNET_VNET_FORWARD`; `link` over the carriers of AF_UNIX |
| `udp-denied` | yes | `tunnel` (the fallback to HTTP/2), `warp` (TCP), `wg:FILE` over `warp` |
| `raw-denied` | yes | `link` over `icmp`, `pod --allow-raw icmp` |
| `no-net-admin` | yes | `pod run` (the seccomp and the netns-raw backends), `warp proxy` |
| `no-loopback` | yes | `doctor`, `net proxy --listen unix:PATH` |
| `proxy-only` | yes: stand-ins | `doctor`, `tunnel --quick`, `warp proxy` (MASQUE over TCP 443), `relay`, `send` and `receive` |
| `proxy-tls-reterminate` | yes: stand-ins | `doctor`, `--ca-file "$SSL_CERT_FILE" doctor`, `tunnel --quick` |
| `proxy-per-name` | yes: stand-ins | `doctor`, `tunnel --quick`, `send FILE` |
| `sni-allowlist` | yes: stand-ins | `doctor`, `tunnel --quick` |
| `no-proxy-no-dns` | yes: stand-ins | `doctor`, `tunnel --quick`, `--resolve NAME=192.0.2.1 doctor` |
| `ws-upgrade-refused` | yes: stand-ins | `doctor`, `relay node --to 127.0.0.1:22`, `tunnel --quick` |
| `ipv6-only` | yes: stand-ins, on a kernel with IPv6 | `doctor`, `tunnel`, `warp proxy` |

An example:

```sh
podlab run no-loopback -- podnet net proxy --listen unix:/tmp/podnet.sock
podlab run no-net-admin -- podnet warp proxy
```

The network classes come from podnet's data (PodUtils/podnet#592,
`docs/lab-profiles/` of podnet at `dbd432c`); the expected result of each
command is in its file there. `podlab run proxy-only -- podnet doctor`
gives podnet its proxy row with a credential, with no `--allow-connect`.

## podrunner

podrunner runs its installer and a corpus of real projects in each of its
profiles before each release (its user-path gate):

```sh
podlab run PROFILE -- sh install.sh
podlab run PROFILE -- podrunner run JOB
```

Its profiles: `userns-off`, `bind-denied`, `getsockname-denied`,
`proxy-only`, `small-tmp`, `root-unreadable`, `nonroot-nocaps`, `no-kvm`,
`arm64-tcg` and `inherited-fds`. Until it takes the crate, its lanes are
the scripts `scripts/lab/PROFILE.sh` of podrunner.

## podvm

podvm runs a payload in a lab of each of its profiles:

```sh
podlab run PROFILE -- podvm run IMAGE CMD
```

Its profiles: `chroot-denied`, `userns-mount-denied`, `cgroup-v1`,
`landlock-old`, `proc-masked`, `noexec-tmp` and `wsl2`, with
`bind-denied`, `getsockname-denied`, `userns-off`, `small-tmp` and
`inherited-fds`. Its own host form is `podvm lab run PROFILE -- CMD`. The
form of the machine tier is `podvm run --podvm-tier=machine --podvm-lab
PROFILE IMAGE CMD`: a guest with the knobs of the profile, for
`cgroup-v1`, `arm64-tcg`, `landlock-old` with an ABI from 1 to 3, and
`wsl2`. `wsl2` is not in the table of the crate: only the machine tier
holds it.
