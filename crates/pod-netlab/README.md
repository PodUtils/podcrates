# pod-netlab

Network labs inside a guest that is root: NAT44, NAT66, NAT64 with
DNS64, CGNAT, IPv6 only, a firewall policy and impairment. podvm asked for
it (PodUtils/podvm#260, for PodUtils/podvm@T-1662): its issue 178 keeps
what sand-net knew, and this crate ports the layout and the knobs
unchanged, the derivation of each expected verdict from the knobs, and the
check that reads the nftables counters before and after and fails on a
flushed chain. T-207 made it, as new code from the text of issue 178.

The crate runs no command, opens no socket and reads no file. The
consumer runs the steps of a plan in the guest, runs the probes, and gives
the measurements and the output of `nft -j list counters` to the judge. It
depends on `pod-core` (the failure) and `serde_json` (the counters); no
tokio.

## Use

```rust
use pod_netlab::{Knobs, Lab, Settings, judge, parse_counters, scenario};

let knobs = scenario("hairpin-web").unwrap().knobs;           // or Knobs::parse_flags(&args)?
let lab = Lab::new("sn", "hairpin-web", knobs)?;               // checks the knobs
let settings = Settings { state_dir: "/run/netlab".into(), ping_gid: 1000 };
for step in lab.up(&settings)? { run_in_guest(step)?; }       // pod_netlab::Step
let before = parse_counters(&nft_list_counters()?, "lab")?;    // plan::ruleset::list_counters
let measured = run_probes(&pod_netlab::probes(lab.knobs()))?;  // T-345
let after = parse_counters(&nft_list_counters()?, "lab")?;
let report = judge(&lab.expect(), &before, &after, &measured);
print!("{report}");                                            // PASS and FAIL rows
for step in lab.down(&settings)? { run_in_guest(step).ok(); }
```

## API

| Item | What |
| --- | --- |
| `Knobs`, `Stack`, `Nat`, `NatFlags`, `Policy`, `Broken`, `Icmp`, `Ping`, `Family`, `Proto` | each knob of sand-net; `Knobs::check` (each range, each contradiction), `Knobs::parse_flags` and `Knobs::flags` (sand-net's flags, both ways), `nat_on`, `ping_passes` |
| `KnobError`, `Contradiction` | a refused knob set: `OutOfRange`, `BadPort`, `TooManyPorts`, `BadFlag`, and each contradiction by name |
| `Lab`, `Settings`, `LabError` | a checked knob set under a prefix and a name (`PREFIX-NAME-g`); `namespace`, `layout`, `files`, `up`, `down`, `expect` |
| `Layout`, `Role`, `layout::*` | the namespaces, the veth pairs and the addresses; the constants of each address, of the NAT64 prefix and pool, and of the ports 2200 and 2222 |
| `Step` | `Run` (an argument list, its standard input, and whether it must pass), `Write`, `Remove`, `Stop` (a pid file); `line` for a log |
| `plan::ruleset` | `Counter` (the nine names), `TABLE`, `FORWARD_CHAIN`, `counters_json`, `forward_rules`, `router_rules`, `carrier_rules`, `list_counters` |
| `Scenario`, `scenario`, `scenarios`, `scenario::NAMES` | the 27 offline scenarios, each a knob set |
| `Probe`, `probes`, `Target`, `Side`, `probe::*` | each probe with its name (`tcp4-443`, `dns-udp6`, `nat64-80`; `Probe::from_name`) and its far end; the ports and the names of the world |
| `Expectations`, `Expect`, `Verdict`, `Seen`, `Bound`, `Must` | `Lab::expect`: each probe's verdicts, what it must see, its bound, its counters, and the rule that gives them; each counter that must move or stay |
| `Counters`, `Count`, `parse_counters`, `CounterError` | the counters of one table from `nft -j list counters` (at most 1 MiB, each field checked) |
| `judge`, `Measured`, `Report`, `Row` | the check: a row for each probe, each unknown measurement and each counter |

Each error converts to a `pod_core::Failure` of the stage `Host`: a knob
or a name is `Usage`, a text of nft that does not read is `Protocol`.

## The lab

| Namespace | Device | IPv4 | IPv6 |
| --- | --- | --- | --- |
| `PREFIX-NAME-g` (guest) | `gr` | 10.77.0.2/24 | fd00:77::2/64 |
| `PREFIX-NAME-r` (router) | `rg` | 10.77.0.1/24 | fd00:77::1/64 |
| | `rw` | 198.51.100.1/24 | 2001:db8:100::1/64 |
| `PREFIX-NAME-w` (world) | `wr` | 198.51.100.10/24 | 2001:db8:100::10/64 |

The guest's link carries only the families of `--stack`. With `--cgnat`,
the router's outside device is `rc` (100.64.0.2/24, 2001:db8:101::2/64),
and a fourth namespace `PREFIX-NAME-c` has `cr` (100.64.0.1,
2001:db8:101::1) and `cw` (198.51.100.1, 2001:db8:100::1): the world's
routes stay the same (call C147). With `--nat64`, tayga runs in the router
with the prefix 2001:db8:64::/96 and the pool 192.168.255.0/24.

`Lab::up` gives, in order: the namespaces with their loopback; the
sysctls (the guest's `disable_ipv6` before its device for `--stack v4`,
`ping_group_range` to the probing gid, `tcp_mtu_probing=0` at both ends,
forwarding and `icmp_ratelimit=0` in the router and the carrier); the veth
pairs; the addresses (IPv6 with `nodad`); the links up; the removal of
127.0.0.1 for `--stack v6`; the routes (with `--mtu`, the MTU of each
route of the router); tayga; the counters as nft JSON, the chains as nft
text, the carrier's table; the guest's `resolv.conf` in `/etc/netns/`;
one echo along each hop; the netem queues of `rg` and the outside device.
`Lab::down` stops tayga, deletes each namespace and removes the files;
each of its steps may fail. The guest needs iproute2, nftables, sysctl,
iputils `ping` and tayga (for `--nat64`).

The router's table `inet lab`:

| Order | Rule of the chain `forward_policy` (policy drop) | Counter |
| --- | --- | --- |
| 1 | `ct state established,related` accept | `return` |
| 2 | each new flow from `rg` (no verdict) | `forwarded` |
| 3 | `--broken`: the family, drop or reject (port-unreachable) | `broken` |
| 4 | `ct status dnat` accept (L5) | `inbound` |
| 5 | `--nat64`: from tayga's device, accept | none |
| 6 | a new flow from the outside of each family: drop with NAT on it (L6), else accept | `inbound` |
| 7 | an echo request of the guest: `--ping` (with no flag, the policy) | `ping` |
| 8 | `--no-dns`: TCP and UDP port 53, drop | `dropped` |
| 9 | the policy: `open` accept, or the ports of each list | `open`, `tcp`, `udp` |
| 10 | each other packet, drop | `dropped` |

With `--icmp drop`, the chain `icmp_out` drops the router's frag-needed
and packet-too-big (counter `dropped`). The chains `nat_pre` (`--dnat`,
`--hairpin`) and `nat_post` (masquerade with `--nat-flags`, the NAT64 pool,
the hairpin) follow.

## The derivation

`Lab::expect` walks each probe through the rules above, from the knobs
alone (call C149). A probe of a family that the guest lacks is DENY; a
broken family is DROPPED or REFUSED; a port that the policy does not name
is DROPPED; a flow that passes is ALLOW, and must see the address of its
NAT (`whoami`), its port (`srcport`: moved under `random` and
`fully-random`), the guest's own forward (`hairpin`). A PMTU black hole
(`--mtu` with `--icmp drop`) stalls `bulk`: TIMEOUT. `rtt` is at least two
delays; `bulk` at least 90 % of its bits at the rate, and two delays;
`udp-burst` is in the mean of 100 datagrams at the loss of both directions,
plus or minus four sigma. A probe of single datagrams takes 5 tries, the
resolver 2: where loss can take each try in more than one run in 10,000,
the miss passes too. A counter that a probe moves must move; a counter
that no probe can move must stay.

## The check

`judge` gives a row for each expected probe (a missing measurement fails),
for each measurement of a probe that the lab does not have (it fails), and
for each of the nine counters: a counter that is not there (the table is
gone), that went back, that must move and stayed, or that must stay and
moved, fails. Each scenario has a counter that must move (`forwarded`), so
a flushed policy chain fails, also where the guest measured what the lab
expects. The plant `netlab-judge-ignores-counters` proves it.

## Parity with issue 178

Each line is done, or names its entry.

**Layout.** The three namespaces, the veths `gr`, `rg`, `rw`, `wr` and
their addresses: done. Documentation ranges only: done (a test holds each
address). `down` is `ip netns del` and the files: done; the state file of
a lab is the consumer's.

**Knobs.** `--stack dual|v4|v6` (v6 removes 127.0.0.1), `--nat
none|nat44|nat66|both`, `--nat-flags persistent|random|fully-random`,
`--policy open|web|web-nodns|only443|blackout`, `--tcp`, `--udp`,
`--no-dns`, `--broken v4-drop|v4-reject|v6-drop|v6-reject`, `--mtu
1280..1499`, `--icmp drop`, `--delay`, `--loss`, `--rate` (netem on the
router), `--dnat` (2222 to 2200), `--hairpin`, `--ping allow|drop`,
`--nat64` (tayga, 2001:db8:64::/96, pool 192.168.255.0/24), `--cgnat` (a
fourth namespace, 100.64.0.2): done, each with its flag. `--dns64` of the
world is a knob too. The refusals: nat64 needs v6, cgnat needs v4 and
NAT44, hairpin needs dnat: done, with six more (call C146).

**Counters.** `return`, `forwarded`, `inbound`, `broken`, `ping`, `open`,
`tcp`, `udp`, `dropped` in one `inet` table; with NAT on a family, new
inbound flows on it dropped: done.

**Scenarios.** The 27 offline ones: dual-nat, home-nat44, v6-only-nat66,
v6-only-vps, v4-vps, web-only, locked-443, corp-proxy, blackout,
broken-v4-silent, broken-v4-reject, broken-v6-silent, broken-v6-reject,
pmtu-ok, pmtu-blackhole, lossy-mobile (40 ms, 5 %, 4 Mbit/s), satellite
(300 ms, 1 Mbit/s), symmetric-nat, inbound-dnat, ping-allowed-web,
ping-blocked, nat64-dns64, nat64-web, cgnat, cgnat-dnat, hairpin-nat,
hairpin-web: done; the knob set of each one but lossy-mobile and satellite
comes from its name (call C145). The VPN and the live scenarios (12 of
39): T-348.

**Verdicts.** ALLOW, DROPPED, REFUSED, DENY, UNRESOLVED, TIMEOUT: done.

**Probes.** tcp4/6 for each port, udp, dns-udp, whoami, srcport (40001),
proxy, rtt, bulk, udp-burst (100 datagrams, 4 sigma), icmp, hairpin,
resolve, nat64-80, nat64-443, nat64-whoami, dns64, isp-whoami, inbound
from the world (and inbound-dnat): the names, the targets and the
expectations are done; the program that measures them in the guest is
T-345.

**The world.** `world.py` (the replies `you=ADDR port=PORT`, HTTP with
`/bulk`, the CONNECT proxy, the UDP echo, the DNS with DNS64) over
`pod-dns` and `pod-proxy`: T-346. The probes and the plan use its ports
and names (`probe::WORLD_TCP`, `NAME_BOTH`, `NAME_V4_ONLY`).

**The check.** Measure in the guest, derive on the host, read the
counters before and after, require each counter to move or not, fail a
flushed chain: done (`judge`, the plant). `sand net try` and `sand net
matrix` with the time of each scenario, the planted flush inside a
guest, and a cage as the guest with its port list: T-347 and podvm.

**Lessons.**

| Lesson | Where |
| --- | --- |
| L1: `icmp_ratelimit=0` in the router | `Lab::up`, test `up_sets_the_sysctls_of_the_lessons` |
| L2: a probe of UDP connects first | the expectation (REFUSED under a reject); the probe is T-345 |
| L3: one packet along each hop after `up` | `Lab::up`, before netem; test `one_packet_goes_along_each_hop_before_the_netem_queues` |
| L5: `ct status dnat` accepted first | `forward_rules`; test `dnat_passes_first_and_nat_drops_new_inbound_flows`, plant `netlab-dnat-after-new-drop` |
| L6: NAT drops new inbound flows | `forward_rules`; the same test |
| F9: no chain named `fwd` | `FORWARD_CHAIN`; test `no_chain_is_named_fwd` |
| F18: 2001:db8:64::/96, not 64:ff9b::/96 | `layout::NAT64_PREFIX`; test `each_address_is_in_a_range_for_labs` |
| F19: no tayga service of a package | the plan starts its own tayga; the image of the guest is T-347 |
| F20: `ping_group_range` is the probing gid | `Settings::ping_gid`, `Lab::up` |
| F21: the world replies with `IPV6_PKTINFO` | T-346 |
| Planted flush fails the check | `judge`; plant `netlab-judge-ignores-counters`; in a guest: T-347 |
| More than one guest per lab | T-349 |
| The lighter variant on a user-space stack | T-349 |

New here: nft 1.0.9 refuses a counter named `return` in its text, so the
counters go in as JSON (call C148).

## Adoption map (podvm)

podvm has no module of a lab: PodUtils/podvm@T-1662 takes the crate.

| podvm | `pod-netlab` |
| --- | --- |
| `podvm lab up` | `Lab::new` (the knobs of `Knobs::parse_flags`), then each `Step` of `Lab::up` in the guest |
| `podvm lab down` | each `Step` of `Lab::down` |
| `podvm lab check` | `plan::ruleset::list_counters`, the probes (T-345), `parse_counters`, `judge` |
| `podvm lab try`, `matrix` | `scenario`, `scenarios` |
| the module set of the guest | `pod-guest` (`modloop::modules`, T-206): veth, nf_tables, nft_nat, nft_masq, nft_reject_inet, nft_ct, sch_netem, tun |

## Tests

`cargo test -p pod-netlab`: the knobs (each contradiction and range, the
flags of each scenario back and forth), the plan (the names, the
addresses, the argument lists, the sysctls of the lessons, the order of
the hops and of netem, the MTU, the counters, the rules of each knob,
tayga, the resolver, `down`, the carrier), the derivation of each
scenario, the counters of nft, and the judge (a run that agrees passes;
a flushed chain, a table that is gone, a counter that went back and each
wrong answer fail). `cargo test -p pod-netlab --test nft -- --ignored`
loads the ruleset of each scenario into a real nft in a new user and
network namespace and reads its counters back. The fuzz target
`netlab_counters` reads the text of the counters.
