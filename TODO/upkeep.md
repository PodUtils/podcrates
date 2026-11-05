This file holds the entries of milestone M5: the work that keeps the crates
in step with their consumers.

# T-040: The trial of podvm

**Source:** `docs/design.md`, section 8, step 7; `docs/decisions.md`,
call C16.
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** blocked

## Problem

podvm uses the blocking facade of `pod-relay` and `pod-term`. Its trial
needs its repository.

## Premise

- podvm (formerly podbox) is not in the repositories of this session.

## Approach

adoption/podvm/: the commit, the patch; `adoption.yml` runs it.

## Prove

```sh
gh workflow run adoption.yml -f consumer=podvm -f crate=pod-relay
```

## Blocker

The owner: the repository of podvm, and access for this session.
