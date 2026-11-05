# pod-identity

The Ed25519 identity of an end of a road: one seed for each role, its
public key shown as OpenSSH shows one (a SHA256 fingerprint) and as iroh
writes one, the key files, the pins of the keys that an end met, and the
allowlist of the keys that a node lets in, with their routes. The X25519
key of the end-to-end channel comes from the seed and is signed by the
identity.

## API

| Module | What |
| --- | --- |
| crate root | `Identity` (`from_seed`, `public`, `sign`, `dh_public`, `binding`), `PublicKey` (`parse`, `fingerprint`, `hex`, `verifies`, `signed`), `KeyName` |
| `file` | `Place` (`Cache`, `File`, `Ephemeral`), `node_place`, `client_place`, `load(places, place, entropy)`, `existing(places, place)`, `load_in`, `load_split`, `existing_in` |
| `pins` | `check(places, label, key)`, `check_in`, `check_split`, `Seen`, `PinError` |
| `allow` | `Allowlist` (`parse`, `read`, `admits`, `grants`), `admits(path, key)`, `grants(path, key, route)` |

The places come from the caller: `pod_core::files::places_from(&App, var,
cwd)`. The random bytes of a new key come from a `pod_core::entropy::Entropy`.

## Parity

| Capability of podssh `crates/podssh-relay/src/identity/` | Here |
| --- | --- |
| The identity, its key shapes and fingerprints | yes |
| The key files: made once, private, under the older names too, in another user's tree read and never made | yes |
| The pins: first sight, a changed key refused with both, an odd label not pinned | yes |
| The allowlist with routes, read again for each session | yes |
| The tests `tests/identity.rs` (10 of 11), `tests/identity_file.rs` (10), `tests/cache_owner/` (3) | yes; the test of a new pair from a pinned key needs the pairs of the relay, and moves with `pod-relay` (T-025) |

The labels of the derivation of the X25519 key name podssh's e2e channel;
they change with `pod-e2e` (T-021), in one step for both crates.

## Adoption map

| Consumer | Old path | New path |
| --- | --- | --- |
| podssh | `podssh_relay::identity::*` | `pod_identity::*` (the trial keeps the old path as a re-export, `adoption/podssh/pod-identity.patch`) |
| podssh | `identity::file::load(place, entropy)` | `pod_identity::file::load(&places, place, entropy)`, with `places = pod_core::files::places_from(&App { name: "podssh", dir_env: "PODSSH_CACHE_DIR" }, &|n| std::env::var_os(n), std::env::current_dir().ok())` |
| podssh | `identity::file::existing(place)`, `identity::pins::check(label, key)` | the same, with `&places` first |
| podssh | `session::{Entropy, OsEntropy, NoRandom}` | `pod_core::entropy::*` |
| podssh | `cache::{read_private, read_own, create_private, Others, ...}` (the generic part) | `pod_core::files::*` |
