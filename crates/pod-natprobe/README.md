# pod-natprobe

`pod-natprobe` measures NAT behaviour and path properties for the PodUtils
projects. It separates mapping, filtering, port allocation, IP pooling,
hairpinning, mapping lifetime, ALG rewriting, IPv6/NAT64 and gateway port
mapping (NAT-PMP, PCP, UPnP IGD) into typed three-state observations.

## Design

* **Sans-IO core.** With `default-features = false`, the crate compiles with
  no `tokio`, no sockets and no clock reads. The state machine emits outputs
  and consumes inputs through caller-supplied seams.
* **Isolated sockets.** Mapping probes run on socket `M`; filtering probes
  run on a fresh socket `F`; port allocation and hairpin probes use fresh
  sockets. Socket contamination is prevented by construction.
* **No timeout-as-negative.** Every field is an `Obs<T>` (`Observed`,
  `Absent`, `Unknown`, `NotApplicable`). A timeout can only produce
  `Unknown`. Restrictive filtering verdicts require positive controls.
* **No process state, no output.** The crate reads no environment variable,
  keeps no global state and writes nothing to stdout or stderr. Diagnostics
  go to `tracing` and typed `pod_core::Event` values.

## Features

| Feature | Default | Contents |
| --- | --- | --- |
| `run` | yes | Async `Prober` on `tokio`, `SystemUdpFactory`, `SystemNetenv`, `pod-services` server loader |
| `report` | yes | `serde` / `serde_json` serialization of `NatReport` |
| `sim` | no | Deterministic virtual-time NAT simulator (pure `std`) |

## Profiles

* `Profile::Fast`: bounded to 600 ms; reports positive observations only and
  marks undecided questions `Unknown(Pending)`.
* `Profile::Standard`: runs positive controls, allocation sampling, pooling,
  hairpinning, IPv6/NAT64 and gateway discovery.
* `Profile::Deep`: adds opt-in mapping lifetime measurement (`RESPONSE-PORT`)
  and idle-gap counter verification.
