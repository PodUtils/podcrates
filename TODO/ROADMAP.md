# Roadmap

The milestones of the work record, in order. Open work is an entry in
`TODO/`: this page names entries by id, under the milestone of their row,
and has no open checkbox. `cargo todo check` refuses an id under another
milestone, and a `- [ ]` item.

## M0: Foundation

The workspace, the record, the checks with their plants, the workflows,
`pod-core` and `pod-testkit` exist. CI is green on each push. The first
release is tagged, and the trial of `pod-todo` in podssh and podnet
passed.

## M1: Transport

`pod-identity`, `pod-term`, `pod-tls`, `pod-proxy`, `pod-dns` and
`pod-ws` are released, each with its parity list ticked and its trials
passed.

## M2: Relay

The QUIC stack is chosen. `pod-quic`, `pod-session`, `pod-e2e`,
`pod-wormhole`, `pod-netstack`, `pod-wg` and `pod-relay` are released.

## M3: Adapters

`pod-transit`, `pod-warp` and `pod-cloudflare` are released.

## M4: Roads

`pod-derp`, `pod-piping`, `pod-opentunnel`, `pod-acme`, `pod-egress`,
`pod-relay-serve`, `pod-iroh`, `pod-tor` and `pod-ts` are released, each
after its study.

## M5: Upkeep

The ports of source changes until each consumer takes its crates, the
reports of the consumers, the trial of podvm.

## M6: The crates of podvm

`pod-hostprobe`, `pod-oci`, `pod-extract`, `pod-rootfs`, `pod-enter`,
`pod-supervise` and `pod-guest`, cut from the code of podvm, are released;
podvm takes them.

## M7: The widened scope of podvm

`pod-microvm`, `pod-sud`, `pod-confine`, `pod-netlab`, `pod-pcap` and
`pod-sandprobe` are released, each after its study.

## M8: The fleet

`pod-broker`, `pod-unit`, `pod-control` and `pod-release` are released, with
the acceptance registry of `pod-todo` and the stand-ins of `pod-testkit` as
programs. They serve the asks of GitHub #35 (cfb) and #36 (githome). Each
product of the fleet (`podrunner`, `podnode` and the others) takes them from
its own repository.
