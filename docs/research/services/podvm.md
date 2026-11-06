# The services of other parties in podvm: the inventory

The inventory of podvm at `ecf61c0` of its branch `migrate/podvm`, after
the rename `98112cb` (podbox is podvm). The podcrates session read it on
2026-11-06 for T-054, because podvm did not send its own inventory. Each
line names the file and lines in `PodUtils/podvm`, what the place holds
or does, the source of its values, and the code that reads it. "T-047"
marks a place of the relay contract, which goes to `pod-relay`, not to
`pod-services`. "(in pod-services: FILE)" marks a value that
`crates/pod-services/services/FILE` holds now. The scan excludes
`references/`, `vendor/` and `docs/history/`.

No hardcoded hosts in podvm for: DNS or DoH resolvers (the completion
copies the host's `resolv.conf`, `crates/podvm-complete/src/net.rs`),
NTP, STUN, DERP, iroh, mailboxes, forwarders, ACME, Cloudflare, WARP or
quick tunnels. The relay has no default in the Rust code: `--relay` (or
`PODSSH_RELAY` for the two helper binaries) is required, and only the
experiments name the owner's relay (section E). The `windows fetch` URL
and its digest are arguments; no default guest URL is in the Rust code.

## A. Lists of hosts in the Rust code

- `crates/podvm-image/src/reference.rs:16-22` DEFAULT_DOMAIN `docker.io`
  (the name that a bare reference resolves to), DEFAULT_ENDPOINT
  `registry-1.docker.io` (the host that answers `/v2/`), OFFICIAL_PREFIX
  `library` (docker's rule; the comment gives no URL); `:163-171`
  `Reference::endpoint` maps `docker.io` and `index.docker.io` to the
  endpoint; read by `registry.rs` (each request), `store.rs:159` (the
  printed name), `podvm images`, `podvm pull`, `podvm run`.
- `crates/podvm-image/src/credentials.rs:62,279,361` build
  `https://{host}/v2/` as the key of a credential helper; `:452-460`
  `server_host` takes `https://index.docker.io/v1/` and `index.docker.io`
  as one key.
- `crates/podvm/src/images.rs:646,658` (help text), `:686-689` and
  `:836-838`: `podvm login` and `podvm logout` default the server to
  `https://index.docker.io/v1/` ("docker's canonical key"); read by
  `credentials::store_login` and the erase path.
- `crates/podvm-image/src/registry.rs:647-690`: the token endpoint is not
  hardcoded. podvm takes the realm from the `WWW-Authenticate: Bearer`
  challenge of the registry, and refuses a realm that is not `https://`.
  `auth.docker.io/token` and `registry.docker.io` are in the test at
  `:1042-1045` only.
- `crates/podvm-measure/src/workload.rs:36-39` ALPINE_REF
  `public.ecr.aws/docker/library/alpine@sha256:3e9b4b68...`, KURL
  `https://dl-cdn.alpinelinux.org/alpine/v3.22/releases/x86_64/netboot/vmlinuz-virt`,
  KERNEL_SHA256 `6b58e5d7...` ("every input pinned, as in the script's 146
  shape"); read by the guest half of `podvm-measure` (`qemu-system-x86_64`
  boots the kernel).
- `crates/podvm-ssh/src/mux.rs:5-31`: the comment cites the relay's own
  document, fetched 2026-09-28 from `https://tcp.ssh.relay.ajam.dev/index.md`
  (version `r12`), and the paths `/v1/node/{name}`, `/v1/connect/{name}`,
  `/v1/status/{name}`, `/v1/stop/{name}`, `/v1/pair`; `:124-145`
  PROTOCOL_VERSION `reverse-v1`, HELLO_VERSION 1, MAX_PAYLOAD 65536 and the
  waits (T-047). The host is a comment only (in pod-services:
  `ajam-relay.toml`, endpoint `default`). `:155-235` parse the user's
  origin: `wss://host[:port]`, `ws://` on loopback only.
- `crates/podvm-ssh/src/bin/node.rs:62`, `bin/operator.rs:58` read
  `PODSSH_RELAY`; `crates/podvm/src/remote/mod.rs:52-58`, `serve.rs:15`,
  `connect.rs:16` take `--relay <wss>` with no default.
- `crates/podvm-complete/src/reach.rs:94-113` `probe`: a HEAD to
  `https://{host}/` for each host of the image's package sources (the
  hosts come from the image, not from podvm), through the environment's
  proxy; read by `sources.rs:191-225` (the rewrite of `http://` to
  `https://`, "rewrite the scheme, never the host", `:11-12`).
  `deb.debian.org` is in the tests of `sources.rs:281-338` and
  `reach.rs:123-125` only.
- `crates/podvm/src/windows/args.rs:15-29,58-59,106-113`: `podvm windows
  fetch --url <url> --sha256 <hex> --max-bytes <n>`, all from the user;
  `crates/podvm/src/windows/setup.rs:33-34` the ureq agent;
  `crates/podvm-windows/src/fetch.rs` the ceilings and the digest check.
- `crates/podvm-windows/src/dos.rs:27-42` the FreeDOS base at
  `PODVM_DOS_BASE` or `~/.local/share/podvm/windows/freedos-base.img`;
  `crates/podvm/src/windows/dos.rs:61-62,116` and `windows/doctor.rs:63`
  (text) send the user to `experiments/369-windows-tcg-dos.sh` for the
  fetch of "FreeDOS 1.4 LiteUSB (pinned sha256)" (section B).

## B. Guest images and their digests

- The Alpine netboot kernel: `https://dl-cdn.alpinelinux.org/alpine/v3.22/releases/x86_64/netboot/vmlinuz-virt`,
  SHA-256 `6b58e5d779e44e57c9efa20232da18650415eceb9d4f544e5c165c1f392c5d51`;
  in `crates/podvm-measure/src/workload.rs:37-39` and
  `experiments/146-podvm-initramfs.sh:54-55`, `147-podvm-exec.sh:52-53`,
  `360-perf-harness.sh:49-50`, `361-guest-usernet.sh:45-46`,
  `390-machine-ssh.sh:50-51`; the results repeat it
  (`experiments/results/podvm-initramfs.txt:7`, `podvm-exec.txt:7`,
  `perf-kvm.txt:13`, `perf-lane.txt:13`, `tcg-profile.txt:142`,
  `tcg-workload-spread.txt:8`, `guest-usernet.txt:7`, `machine-ssh.txt:5`).
- The Alpine netboot modules: `.../v3.22/releases/x86_64/netboot/modloop-virt`,
  SHA-256 `9a5252fa3e49f908fa7bc796a38b7ce57c9a930c91670324278a352effa5c30f`;
  `experiments/361-guest-usernet.sh:47-48`.
- `experiments/290-microvm.sh:48-49,89-96`: `vmlinuz-virt` and
  `initramfs-virt` of `dl-cdn.alpinelinux.org/alpine/$ALPINE_REL/...`,
  `PODVM_MVM_ALPINE` default `v3.21`, with no digest.
- FreeDOS: `experiments/369-windows-tcg-dos.sh:42-43` FD_URL
  `https://download.freedos.org/1.4/FD14-LiteUSB.zip`, FD_SHA256
  `857dcd2ebf9d3d094320154db5fb5b830acba6fb98f981a95a0ca7ab3350338b`;
  `:134-155` the fetch with a ceiling, the check, the base;
  `experiments/results/windows-tcg-dos.txt`.
- Windows ValidationOS: `experiments/371-validationos-stream.sh:38-42` URL
  `https://software-static.download.prss.microsoft.com/dbazure/26100.9278.260824-2119.ge_release_svc_prod3_amd64fre_en-us_VALIDATIONOS.iso`,
  ISO_LEN 2460880896, VHDX_LBA 1010, VHDX_LEN 910163968, VHDX_SHA256
  `063442aa...` (read from the ISO's UDF entries, `:10-18`); `:82-182` the
  range reads; `experiments/results/validationos-vhdx-host.txt`.
- The OCI images of the guest initramfs: ALPINE_REF
  `public.ecr.aws/docker/library/alpine@sha256:3e9b4b68...` in
  `workload.rs:36`, `experiments/146:53`, `361:44`, `390:49`.
- dropbear source for the guest SSH server:
  `experiments/390-machine-ssh.sh:52-53`, `391-machine-bridge.sh:35`
  `https://github.com/mkj/dropbear` at commit `59870ad4...`.

## C. Lists of pinned OCI images (the test corpus)

- `scripts/common/distro-matrix.sh:47-58` DISTRO_ROWS_NSSWITCH: Docker Hub
  short names (`alpine:3.22`, `alpine:3.20`, `alpine:3.10`,
  `voidlinux/voidlinux-musl`, `debian:11`, `debian:12`, `ubuntu:20.04`,
  `rockylinux:8`, `opensuse/leap:15.6`, `fedora:42`, `archlinux:latest`),
  each with a manifest digest, read 2026-09-08; `:88-96` qualifies them to
  `docker.io/library/...`.
- `scripts/common/distro-matrix.sh:74-85` DISTRO_ROWS_M5: ten rows on
  `public.ecr.aws` (`docker/library/alpine:3.20`,
  `debian/debian:bookworm-slim`, `ubuntu/ubuntu:24.04`), `ghcr.io`
  (`pkgforge-dev/archlinux:latest`, `void-linux/void-musl:latest`),
  `quay.io` (`almalinuxorg/almalinux:9`, `rockylinux/rockylinux:9`,
  `:9-minimal`, `fedora/fedora:42`) and `registry.opensuse.org`
  (`opensuse/leap:15.6`), each with a digest from `docker buildx
  imagetools inspect` on 2026-09-09; `:64-68` "no Docker Hub" (its rate
  limit falls on a shared address). Read by
  `experiments/95-podman-vfs-ignorechown.sh`, `125-across-distributions.sh`,
  `155-proc-absence.sh`, `240-distro-sweep.sh`, `245-interpose-sweep.sh`
  and `crates/podvm-gate/src/main.rs` (the gate's check of the rows).
- The driver image `public.ecr.aws/debian/debian:bookworm-slim@sha256:833d7afe...`
  in `experiments/150:64`, `152:37`, `240:89`, `270:46`, `372:30` and
  others; `public.ecr.aws/docker/library/alpine:3.20@sha256:d9e853e8...` in
  `crates/podvm/src/windows/mod.rs:371` (a test) and
  `experiments/384-windows-lane-v6.sh:15`; the aarch64 pin
  `alpine:3.20@sha256:45e09956...` in `experiments/367-qemu-user-aarch64.sh:28-29`.
- `experiments/150-image-acquisition.sh:32-40`, `270-multiarch-image.sh:9-11,81`:
  `ghcr.io/pkgforge-dev/archlinux:latest` (`PODVM_TEST_IMAGE`,
  `PODVM_MULTIARCH_IMAGE`), chosen because ghcr has no anonymous quota
  (measured 2026-09-09); `crates/podvm/tests/detached_stdio.rs:163` the
  same default; `experiments/250-negative-tests.sh:49`
  `registry.opensuse.org/opensuse/leap:15.6`.
- `scripts/dev-lane.sh:59`, `scripts/windows/run-in-base.sh:39`,
  `scripts/windows/run-via-host-podman.sh:25`: the build image
  `docker.io/library/rust:1.98.1-bookworm` (`PODVM_LANE_IMAGE`,
  `PODVM_IMAGE`).
- `crates/podvm-gate/src/main.rs:1200-1210` `search_docker_io` and
  `crates/podvm-gate/src/plant.rs:1305-1344`: a gate rule that refuses a
  `docker.io` image or an unqualified name in a `Prove:` line of the
  record; `main.rs:220-230` the list of short names.

## D. Trust data

- `crates/podvm-image/src/tls.rs:34-46` BUNDLE_ENV (`SSL_CERT_FILE`,
  `CURL_CA_BUNDLE`, `REQUESTS_CA_BUNDLE`) and BUNDLE_PATHS (five host
  paths); `:227-230` `webpki_roots::TLS_SERVER_ROOTS` when the host has
  no bundle. Read by each HTTPS request of podvm (`registry.rs`,
  `reach.rs`, `windows/setup.rs`) and by `podvm-complete/src/pkg.rs:208-222`
  (the CA bundle that the completion puts into an image).
- `crates/podvm-ssh/src/tls.rs:23-30` HOST_BUNDLES (four paths), `:53`
  `webpki-roots` as the fallback; read by the relay legs.
- `Cargo.toml:64-76` pins `webpki-roots = "1"`. These are paths of the
  host and the root store of a crate, not a host of another party:
  `pod-tls` holds the same (`SYSTEM_BUNDLES`), not `pod-services`.

## E. Code that measures or watches

- The owner's relay, live: `experiments/387-mux-two-client.sh:26-27` RELAY
  `https://tcp.ssh.relay.ajam.dev`, WSS `wss://tcp.ssh.relay.ajam.dev`
  (in pod-services: `ajam-relay.toml`, endpoint `default`); `:51`
  `GET /health?detail=1`, `:70` `POST /v1/pair`, `:88-89`
  `GET /v1/status/{name}` with each token, `:104` `PODSSH_RELAY`;
  `experiments/389-remote-ssh.sh:31-32,209-230` the same for
  `podvm remote ssh serve` and `connect`;
  `experiments/results/mux-two-client.txt:12,77` (the relay's hello:
  `reverse-v1`, frame cap 65504, 64 sessions) (T-047).
- The registries: `experiments/150-image-acquisition.sh` (a pull from
  ghcr), `270-multiarch-image.sh`, `125-across-distributions.sh` and
  `240-distro-sweep.sh` (each row of section C),
  `372-pull-ceiling.sh`.
- The guest images: `experiments/146`, `147`, `290`, `360`, `361`, `390`
  (the Alpine kernel), `369` (FreeDOS), `371` (ValidationOS: HEAD for the
  length, range reads of one extent).
- The isolation checks of `experiments/180-registry-fixture.sh:277`,
  `190-parallel-layers.sh:360`, `200-registry-auth.sh:244`:
  `getent hosts index.docker.io` must fail inside the sandbox (a name
  that must not resolve, not a service that podvm uses).
- No code of podvm watches a service on a schedule. The experiments that
  CI runs (`gate.yml:163,248,258`: 110, 393, 400) read no service of
  another party.

## F. Workflows

- `gate.yml:41,121,180,228`, `nightly.yml:71`: `bootstrap-env.sh` fetches
  zig from `ziglang.org` (section G).
- `nightly.yml:98-101`: `apt-get install qemu-user-static` for the cross
  smoke tests (`:35-53` the emulators); `:174-183` cosign (the
  installer action, `cosign sign-blob` of each file).
- `remote-items.yml` (weekly, and on each change of a workflow): runs
  `check-remote-items.sh` against the GitHub API.
- `secrets.yml:54-61`: trufflehog from
  `https://github.com/trufflesecurity/trufflehog/releases/download/v${VERSION}`,
  with PINNED_SHA256 `40377e65...`.

## G. Scripts

- `scripts/common/bootstrap-env.sh:64-70` ZIG_VERSION `0.16.0`,
  ZIG_SHA256 `70e49664...` (from `https://ziglang.org/download/index.json`);
  `:313` the URL `https://ziglang.org/download/$ZIG_VERSION/zig-x86_64-linux-$ZIG_VERSION.tar.xz`.
- `scripts/common/check-remote-items.sh:175-220` (`gh api`: commits, tags,
  latest releases of each pinned action), `:199` and
  `check-remote-items.ps1:200` `https://raw.githubusercontent.com/$action/$sha/action.yml`.
- `scripts/common/mine-repo.sh:35-45,105-106` PROXY
  `https://api.gh.pkgforge.dev` (a GitHub API proxy, measured 2026-08-28),
  CONTROL `pkgforge-dev/reverse-proxies`; `:550` `git clone
  https://github.com/$TARGET.git`; `mine-repo.ps1:83,449` the same. They
  fill `references/`.
- `experiments/180-registry-fixture.sh:34-41`, `190-parallel-layers.sh:44-48`,
  `200-registry-auth.sh:30-34`: the zot registry binary v2.1.21 from
  `github.com/project-zot/zot/releases/download/...` with its SHA-256 and
  its checksum file; ZOT_PROXY `https://api.rv.pkgforge.dev/` when the
  direct download fails (measured 2026-09-22). The registry is local.
- `experiments/152-nix-acceptance.sh:33-37,119`: the Nix tarball
  `https://nixos.org/releases/nix/nix-2.2.2/nix-2.2.2-x86_64-linux.tar.bz2`,
  `https://github.com/NixOS/nixpkgs/archive/refs/tags/22.05.tar.gz`, the
  substituter `https://cache.nixos.org`.
- `scripts/verify-release.sh`: a shim to `podvm-verify` (section I).
- `scripts/doctor/`, `scripts/windows/kvm-guest.py`, `kvm-watchdog.py`:
  no host of another party.

Test fixtures with documentation names (`example.com`, `registry.example`,
`cdn.example`, `registry.invalid`, `mirror.example`, `reg.io`, `x`) and
private or loopback addresses (`127.0.0.1`, `192.168.122.1`, `10.1.2.3`)
are fixtures only. The test values `ghcr.io` in
`crates/podvm-image/src/transport.rs:331-399` and `reference.rs:329-350`,
and the image names in `crates/podvm-image/src/store.rs` tests, are
parse examples.

## H. What moves to pod-services

One file for each service (`docs/design.md`, section 4, `pod-services`).
New kinds: `oci-registry`, `guest-image`. New watch kinds for T-050:
`oci-ping` (`GET /v2/` answers 200, or 401 with a `Bearer` realm that is
`https://`), `oci-digest` (the tag of a fact resolves to its digest, or
the digest still pulls), `http-sha256` (the file at the endpoint has the
SHA-256 of a fact; a range for the ValidationOS extent).

- `ajam-relay.toml` (exists): no new endpoint. podvm uses its `default`
  host. The paths `/v1/pair`, `/v1/node`, `/v1/connect`, `/v1/status`,
  `/v1/stop` and the hello facts of `reverse-v1` go to `pod-relay`
  (T-047), not to the file. The experiments 387 and 389 read the host
  from the file.
- `docker-hub-registry.toml` (new): endpoint `registry-1.docker.io:443`
  path `/v2/`; facts `domain` `docker.io`, `aliases` [`index.docker.io`],
  `official-prefix` `library`, `credential-key`
  `https://index.docker.io/v1/`; the source: docker's rules, as
  `reference.rs:16-22` and `images.rs:686-689` state them. The realm
  stays dynamic: a fact `token-realm` `https://auth.docker.io/token` only
  for the watch. Watch `oci-ping`.
- `ecr-public-registry.toml` (new): `public.ecr.aws:443`, `/v2/`; facts
  of the pinned images of sections B and C on it (alpine `3e9b4b68...`,
  `d9e853e8...`, `45e09956...`; debian `833d7afe...`; ubuntu
  `41938337...`). Watches `oci-ping`, `oci-digest`.
- `ghcr-registry.toml` (new): `ghcr.io:443`, `/v2/`; facts
  `pkgforge-dev/archlinux` `b2507f19...`, `void-linux/void-musl`
  `af9308c7...`; the fact "no anonymous quota" (measured 2026-09-09).
- `quay-registry.toml` (new): `quay.io:443`, `/v2/`; almalinux, rocky,
  rocky-minimal, fedora with their digests.
- `opensuse-registry.toml` (new): `registry.opensuse.org:443`, `/v2/`;
  leap 15.6 `e4d84bbc...`.
- `alpine-netboot.toml` (new, kind `guest-image`): endpoint
  `dl-cdn.alpinelinux.org:443` path `/alpine/v3.22/releases/x86_64/netboot/`;
  facts `vmlinuz-virt-sha256` `6b58e5d7...`, `modloop-virt-sha256`
  `9a5252fa...`. Watch `http-sha256`. podvm-measure reads it in place of
  `workload.rs:37-39`.
- `freedos-image.toml` (new, kind `guest-image`):
  `download.freedos.org:443` path `/1.4/FD14-LiteUSB.zip`; fact `sha256`
  `857dcd2e...`. A future default of `podvm windows run --guest dos`.
- `microsoft-validationos.toml` (new, kind `guest-image`):
  `software-static.download.prss.microsoft.com:443` with the path of the
  ISO; facts `iso-bytes`, `vhdx-lba`, `vhdx-bytes`, `vhdx-sha256`.
  Watch `http-sha256` on the range.
- The Docker Hub short-name rows of DISTRO_ROWS_NSSWITCH are facts of
  `docker-hub-registry.toml`: their committed readings name these
  digests.
- The experiments read the hosts and the pins from these files (a
  `scripts/services/` export, as for podssh and podnet), not from their
  own variables.

## I. What stays in podvm

- The release identity (the owner's ruling "Services: what stays"):
  `crates/podvm-release/src/identity.rs:15,35-43` (the signer is the old
  repository of podbox up to beta 13, then the repository; the workflow
  `nightly.yml@refs/tags/{tag}`), `verify.rs:83` (`GH_REPO`, default
  `PodUtils/podvm`), `verify.rs:150-165` (cosign, the issuer
  `https://token.actions.githubusercontent.com`), `publish.rs:22,460-461`
  REPO, `notes.rs:137`, `nightly.yml:172-183`, `scripts/verify-release.sh`.
- The user's own data: the `--relay` origin and tokens, the registries
  and credentials of the user (`credentials.rs`, `--insecure-registry`,
  `$PODVM_INSECURE_REGISTRIES`), the `windows fetch --url` and `--sha256`,
  the hosts of an image's package sources (`reach.rs`, `sources.rs`).
- The parse rules of a reference, the transport policy, the realm
  handling and the fetch ceilings: core logic of `pod-oci` (T-056) and
  `pod-guest` (T-061), not data.
- The trust paths and `webpki-roots` (section D): `pod-tls`.
- The supply chain of the build and of the tests (T-046): zig of
  `bootstrap-env.sh`, trufflehog, the actions of `check-remote-items.sh`,
  `qemu-user-static` and cosign of the workflows, the zot binary, the
  Nix tarball, nixpkgs and `cache.nixos.org`, dropbear at its commit, the
  build image `rust:1.98.1-bookworm`.
- The upkeep of the repository: `mine-repo.sh` and `mine-repo.ps1`
  (`api.gh.pkgforge.dev`), `remote-items.yml`, the gate rule of
  `podvm-gate` on `docker.io` in `Prove:` lines.
- On the line, for the owner: the test corpus of section C. It is test
  data of podvm, but each row is a host of another party and a digest
  that a watch can check. The proposal above moves the hosts and the
  digests, and keeps the row names and the libc column in
  `distro-matrix.sh`.
