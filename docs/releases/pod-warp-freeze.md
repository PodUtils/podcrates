# The freeze of src/warp and crates/podssh-warp

The text of the COORD issue of podcrates, titled `pod-warp: the freeze of
src/warp and crates/podssh-warp (to podnet) (to podssh)`. Post it when
T-412 and T-413 pass and a tag holds `pod-warp`.

podcrates holds `pod-warp` (T-028, T-335 to T-339, T-406): the
registration, the identity file, the doors, MASQUE over HTTP/2 and
HTTP/3, WireGuard and the carried device, the relay leg and the race,
the stack with its settings, the proxy, the resolvers, the forward, nc,
the L4 door, `Live`, the exit, the search of the socket, the TUN and its
routes. The adoption map is in `crates/pod-warp/README.md`.

podnet: hold `src/warp/` to fixes from now on. Report each fix here;
podcrates ports it to `pod-warp`. The trial patch
`adoption/podnet/pod-warp.patch` (T-412) replaces `src/warp/` with glue
over the crate. The parts that stay in podnet are `export.rs`, `up.rs`,
`pod.rs`, `ssh_config.rs`, the instance record, the bind of the
listeners, nc's search and relay leg, and the lint of the rules.

podssh: hold `crates/podssh-warp` to fixes from now on. The trial patch
`adoption/podssh/pod-warp.patch` (T-413) moves it onto the crate.

Each consumer takes the crate by the tag that the hand-off names
(`tag = "vX.Y.Z"`), on the day of the release.
