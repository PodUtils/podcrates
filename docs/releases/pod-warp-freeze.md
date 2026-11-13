# The freeze of src/warp and crates/podssh-warp

The text of the COORD issue of podcrates, titled `pod-warp: the freeze of
src/warp and crates/podssh-warp (to podnet) (to podssh)`. It is posted with
`v0.37.0` (T-412 and T-413 pass; the tag holds T-480).

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
`adoption/podssh/pod-warp.patch` (T-413) makes it glue over the crate
(podssh's way out, its proxy and its resolver), with the record, the door
file and the exit codes in podssh-cli; ZeroTier's member takes
`pod-netstack`, and the edge inside WARP takes a `pod_core::Dial`. It
needs `v0.37.0`, which holds T-480 (a team's token, the base that answered).
A server of connect-ip of the user's own over HTTP/3 waits for T-481: the
patch refuses `PODSSH_WARP_SERVER` with exit 70 until then.

Each consumer takes the crate by the tag that the hand-off names
(`tag = "vX.Y.Z"`), on the day of the release.
