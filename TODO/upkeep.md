This file holds the entries of milestone M5: the work that keeps the crates
in step with their consumers.

# T-040: The trial of podvm

**Source:** `docs/design.md`, section 8, step 7; `docs/decisions.md`,
call C16.
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** partial

## Problem

podvm uses the blocking facade of `pod-relay` and `pod-term`. Its trial
needs its repository.

## Premise

- PodUtils/podvm is public: this session reads it through the mirror and
  a clone, which a trial needs; it writes nothing there. podvm takes
  `pod-term`, `pod-tls`, `pod-proxy`, `pod-relay` (feature `blocking`),
  `pod-services`, `pod-egress`, `pod-netstack` and `pod-todo`, in this
  order (PodUtils/podvm#194).

## Approach

adoption/podvm/: the commit, the patch; `adoption.yml` runs it. The first
trial is `pod-term` (podvm's line discipline), at podvm `0249527` (C21).

## Prove

```sh
gh workflow run adoption.yml -f consumer=podvm -f crate=pod-relay
```

## Start condition

podvm gave the commit of its rename (PodUtils/podvm#194: `0249527`). The
edition of podvm is not a condition: a crate of edition 2021 takes a
dependency of edition 2024; the patch raises podvm's `rust-version`.

## Done

2026-11-06, partial. The patch of the trial of `pod-term` (`784469a`), at podvm `0249527`:
`PodUtils/podvm:crates/podvm-ssh/src/session.rs` takes `pod_term::echo` and keeps its
supervisor; podvm's own 29 tests of the discipline pass on pod-term, with
two of them changed to pod-term's Home and End (the screen's cursor moves
with the line's); clippy of podvm's workspace, rustfmt, podvm's record
gate, the toolchains and the static binary `shell` pass
(`sh scripts/adoption.sh podvm pod-term`). podvm took `pod-term` at
`v0.3.0` in podvm `75ed306` (PodUtils/podvm#197): each line of the `.run`
passed there, `session_interactive` too (12 passed against sshd); the trial
leaves the directory of podvm.

2026-11-06, partial. The trial of `pod-tls` at podvm `b5473a2`
(the patch of the trial of `pod-tls`, `87a1338`, 8 files, 119 lines in, 148 out, with no
default feature: no tokio). `podvm-image` keeps its choice of the host
bundle, its words and its verifier for a registry named insecure; pod-tls
reads the bundle alone as a `CaFile`, else the compiled-in roots, and makes
the configuration over ring. `podvm-ssh` takes a `Context` with its host
bundles and `Context::tls_over_blocking`; the handshake comes before the
WebSocket upgrade, so `mux.rs` sets the write bound before TLS. podvm drops
`rustls-pemfile` and its own `webpki-roots`; the `pod-term` tag moves to
`v0.4.0`. One test changed: `garbage_server_name_is_refused_before_the_handshake`
expects pod-tls's words (`is not a valid server name`). Each step of
`sh scripts/adoption.sh podvm pod-tls` passed: podvm-ssh 101 tests,
podvm-image 23 (`tls`, `registry`, `transport`), podvm-complete 15 (`pkg`,
`reach`), clippy, rustfmt, the record gate, the toolchains, the static
`node` and `podvm`; `mux_two_client` runs only where sshd is. A loopback
check outside the patch gave the three results of the handshake: an
unknown issuer with the hint, TLS 1.3 to a private CA, a wrong name refused.
Release sizes, x86_64 musl: `podvm` 3281976 to 3314744 bytes (+32768),
`node` 1622872 to 1676120 (+53248), `operator` 1594264 to 1635224
(+40960). Changes of behaviour: `podvm-ssh` trusts the first usable host
bundle and the compiled-in roots together (before: each host bundle, the
compiled-in roots only when none loaded); a bundle with a block that is not
base64 is passed over (pod-tls's PEM reader refuses the whole file). Gap of
pod-tls: `Words` has no form for a consumer whose only place of a CA adds
to the roots (`podvm-ssh`: the host bundle); its hint says "replace". Left:
the trial of `pod-proxy` (the synchronous face, T-063), then the other
crates of the order of PodUtils/podvm#194.

podvm took `pod-tls` 0.4.0 with no tokio in podvm `b917f97`
(PodUtils/podvm#203): each line of the `.run` passed there, `mux_two_client`
too (15 against sshd); podvm accepts the three changes of behaviour (its
call C9). The trial leaves the directory of podvm. The next hand-offs start
at podvm `b917f97` or later: podvm splits its files to 500 lines
(PodUtils/podvm@T-1674), and the files of M6 are split already.
