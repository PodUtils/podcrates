This file holds the entries of milestone M5: the work that keeps the crates
in step with their consumers.

# T-040: The trial of podvm

**Source:** `docs/design.md`, section 8, step 7; `docs/decisions.md`,
call C16.
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** open

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

adoption/podvm/: the commit, the patch; `adoption.yml` runs it.

## Prove

```sh
gh workflow run adoption.yml -f consumer=podvm -f crate=pod-relay
```

## Start condition

podvm gave the commit of its rename from `podbox-*` to `podvm-*`
(PodUtils/podvm#194), and moved to the stable Rust and edition 2024.
