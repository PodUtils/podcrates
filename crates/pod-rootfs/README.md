# pod-rootfs

The completion of a root file system on Linux, from the host and with no
privilege. An extracted OCI root is not a working environment on a host
that refuses `mknod`, `mount` and `chown`: it has no `/dev/null`, its
resolver is the resolver of its build host, its `/etc/mtab` is a symlink
into a `/proc` that is not mounted, and on a glibc root a file that most
images do not ship decides if a program reads `/etc/passwd`. The crate
writes what the payload reads, before the payload starts. Three rules:

1. **Each write is held in the root.** `write::Root` holds a descriptor of
   the root. Each directory component resolves with
   `openat2(RESOLVE_IN_ROOT)` (`pod_extract::safety::Resolve::InRoot`): a
   symlink is followed, and the root is `/` while it is, so an absolute
   link rebases onto the root and nothing resolves outside it. The last
   component opens with `O_NOFOLLOW`, and a replacement is an unlink and a
   create with `O_EXCL`. Each read opens with `O_NONBLOCK` and reads a
   regular file of at most 8 MiB: a fifo or a device of the image does not
   stop the completion. The CA trust store of the image is the one place
   where the last component is followed, held in the root, as the crate
   adds to a file of the image there. A mode changes through an
   `O_NOFOLLOW` descriptor, never by name.
2. **Each fixup is a row.** A `Report` has one `Fixup` for each file (an
   id, the path, the `Action`, a detail, and `degraded` where the result
   is not what a host with the real thing gives), and the `Step`s: the
   commands that the caller runs inside the root (an argv; the program is
   an absolute path that the crate found in the root).
3. **A fixup does not fail the run.** A fixup that the crate cannot apply
   is a `Failed` row. The caller decides what a failed or a degraded row
   means.

The completion is idempotent, and the caller runs it before each start: a
root can be shared between containers.

## Two layers

With `Settings::upper`, the root of `complete` is the lower layer of a root
of two layers, as overlayfs mounts them (`write::Root::layers`,
`write::layers`):

* A read sees `upper` over `lower`: an entry of `upper` hides the same name
  of `lower`, a directory that both hold is one directory, and a listing is
  the union. A whiteout of `upper` (a character device 0:0) hides its name;
  an opaque directory of `upper` (`trusted.overlay.opaque` or
  `user.overlay.opaque` is `y`) hides the lower directory.
* A write goes to `upper` only. A change of a file that only `lower` holds
  copies it up first (the bytes, the owner where the process may set it,
  the mode), then changes the copy. A new entry is made in `upper`, and each
  parent directory that only `lower` holds is copied up with its mode and
  owner. A file of `lower` that a step replaces is not copied: the new file
  of `upper` hides it.
* A remove of an entry that `lower` holds is refused with
  `LayerRefusal::Remove` (`Class::Unsupported`): only a whiteout removes it,
  and the crate makes none. A new directory over a whiteout of `upper` that
  hides an entry of `lower` is refused with `LayerRefusal::Opaque`.
* `lower` is only opened for read. Two layers that hold each other are a
  `Usage` failure.

Each path of two layers resolves in the crate, one component at a time from
a descriptor of each layer: a directory opens with `O_NOFOLLOW`, and a link
of the merged view is followed in the process, held in the root (an absolute
target starts at the root, `..` at the root stays there, 40 links at most).
The kernel follows no link of a layer, so no `openat2(2)` is necessary. The
mount table of `mtab` and the space check of the device shims take the
path of `upper`, which takes the writes.

Two steps remove: the device step removes a symlink at a device path before
the node (on two layers a link of `lower` stays hidden under the new entry,
and the row is the same), and the CA directory step removes an old
certificate file of the crate (on two layers, one of `lower` is refused,
and the row of `ca-hash-dir` is a failed row with the refusal).

The crate is Linux only (`#![cfg(target_os = "linux")]`): on another target
it is empty. It reads no variable and prints nothing. It makes no network
request: the caller answers the question "does this mirror speak HTTPS"
(`reach::Probe`). It keeps one process state: the answer of the `openat2`
probe of `pod-extract`. With no `openat2(2)` (a kernel before 5.6, or an
emulator that does not translate it), each path of the root is refused and
each fixup is a failed row; the tests that need it stop with a skip line
under an emulator.

## The fixups

| Id | What |
| --- | --- |
| `dev-node`, `dev-shim` | `/dev/null`, `/dev/zero`, `/dev/random`, `/dev/urandom`: `mknod(2)` first; the node must open (a file system mounted `nodev` makes a node that does not); else a regular file (null: empty, cut to zero on each run; zero and the random two: 1 MiB), after a space check; a symlink at the path is removed first, a directory is a failed row |
| `dev-fd-link` | `/dev/stdin`, `/dev/stdout`, `/dev/stderr`, `/dev/fd` as links into `/proc/self/fd` where absent; a link of the image stays |
| `resolv.conf` | the resolver of the host, each time (`Settings::host_resolv_conf`) |
| `hosts` | `localhost`, the container name, the added hosts (`name:address`, split at the first colon; each part one word) |
| `identity` | a minimal `/etc/passwd` and `/etc/group` where absent; the owner write bit on a read-only file of the image; a file behind a link inside the root stays |
| `nsswitch` | on glibc, a minimal `nsswitch.conf` where absent, or `files` first on the `passwd` and `group` lines; musl is left alone |
| `mtab` | where `/etc/mtab` is absent or a link: one line for the file system of the root, from the mount table of the host (`Settings::mounts`), and each mount under the root |
| `pacman`, `pacman-keyring` | `DownloadUser` lines become comments; with no keyring, two steps (`pacman-key --init`, `--populate`) |
| `apt`, `apt-sandbox` | a drop-in with `APT::Sandbox::User "root"` |
| `zypper`, `zypper-index` | says if a RIS index is there (the source fixup changes it too) |
| `ca-bundle` | the host bundle at each usual path whose directory is there when the image has none; appended (with a marker line) when the image has one and a variable of the host named the bundle |
| `ca-hash-dir` | each named root that a hash-indexed CA directory of the image does not hold, one file for each, and a step `openssl rehash DIR` where a file of the crate has no link or changed |
| `source-scheme`, `source-reach` | `http://` package sources of apt, apk, pacman, yum, zypper (with its RIS index) and xbps to `https://`, the scheme only, where the probe of the caller says that the host speaks HTTPS; only a DNS name is asked about; the original goes to `Settings::originals` and comes back when `source_fixup` is off |

The C library (`Report::libc`) is found on disk on each architecture: a
`libc.so.6` in `lib`, `lib64`, `lib32` or their `usr` forms or in a
multiarch directory of them (`*-linux-gnu*`), a `ld-musl-*.so.1` or
`libc.musl-*.so.1` in `lib` or `usr/lib`.

## API

| Item | What |
| --- | --- |
| `complete(rootfs, &settings, &mut probe) -> Result<Report>` | each fixup, in order; the failure is a root that does not open or a name that cannot be used; with `Settings::upper`, `rootfs` is the lower layer of two |
| `Settings` | `names`, `host` (the `pod_hostprobe::Settings` of the space message), `container_name`, `add_hosts`, `source_fixup`, `host_cas`, `steps`, `host_bundle`, `host_resolv_conf`, `mounts`, `originals`, `upper` |
| `Names` | `tool` (the head comment of each written file), `comment` (before a line that the crate comments out), `apt_drop_in`, `host_ca_marker`, `host_ca_prefix`, `scratch_prefix`; a second run finds the files of the first by these names |
| `HostBundle` | `path`, `announced_by` (the variable that named it), `bytes`; the caller finds it (`pod_oci::trust::host_bundle` does) |
| `Report`, `Fixup`, `Step`, `Action`, `Libc`, `IDS` | the record; `Report::{degradations, changed, failed}`; `Action::{word, changed}`; `IDS` lists each id |
| `reach::{Probe, Answer, Fixed, is_name}` | the question of the caller (a `FnMut(&str) -> Answer` is a `Probe`), asked one time for each host; `Fixed` answers with no request |
| `write::{Root, Kind, Wrote, Reach, READ_LIMIT}` | the door: `open`, `layers`, `path`, `upper`, `written`, `kind`, `read`, `read_text`, `write`, `reach`, `mknod_char`, `char_opens`, `dir_following`, `read_following`, `write_following`, `mkdirs`, `list`, `unlink`, `symlink` |
| `write::{LayerRefusal, layer_refusal}` | what two layers refuse (`Remove`, `Opaque`), the cause of a `Class::Unsupported` failure, and its read from a `Failure` |
| `devices::{DEVICES, FD_LINKS, FILLED_BYTES, SHIM_HEADROOM_BYTES}`, `pkg::{BUNDLE_PATHS, CAPATH_DIRS, ZYPP_SERVICE_DIRS}`, `sources::{SOURCE_FILES, SOURCE_DIRS}`, `identity::{probe_libc, prepend_files}`, `net::topology` | the tables and the pure parts |

Each failure is a `pod_core::Failure` at `Stage::Host`: `Policy` (a path
that leaves the root, a file over the read limit), `Usage` (a malformed
added host, a name with a line end or a `/`), `Unavailable` (no space for
the shims, from `pod_hostprobe::space`), and the class of the errno of a
refused system call, with its `io::Error` as the cause.

## Parity

Against podvm `crates/podvm-complete` at `2616bd7`:

| Capability of podvm | Here |
| --- | --- |
| [x] `complete(rootfs, &Options)` | `complete(rootfs, &settings, &mut probe)`; podvm keeps `Options` and builds the `Settings` |
| [x] `Options::{container_name, add_hosts, source_fixup, host_cas, host_resolv_conf, steps}` | the same fields of `Settings` |
| [x] The CA bundle of `podvm_image::tls::host_bundle_bytes` (the environment) | `Settings::host_bundle`, from the caller |
| [x] `/proc/self/mounts` | `Settings::mounts` |
| [x] `<rootfs>/../.complete-orig` | `Settings::originals`; with `None` no source changes |
| [x] `HOST_CA_MARKER`, `HOST_CA_FILE_PREFIX`, `99podvm`, `.podvm-mknod-`, the comment of `pacman.conf`, the head comments | `Names`, with podvm's values in podvm |
| [x] `reach::Prober` (a HEAD over `podvm_image::net`) | `reach::Probe` of the caller; podvm keeps the HEAD |
| [x] `Fixup { entry, id, ... }`, `Step { entry, ... }` | no `entry`: podvm maps each id of `IDS` to its entry |
| [x] `Report::{banner, json}` | podvm keeps them; `Report::{degradations, changed, failed}` here |
| [x] `Error::Complete(String)` | `pod_core::Failure` with its class |
| [x] Each fixup of `devices`, `identity`, `net`, `pkg`, `sources` | the same, with the fixes below |
| [x] The tests: 53 unit tests | 52 with their names (`a_report_that_changed_nothing_still_says_so` stays in podvm with the banner); the environment variables of the CA tests are `Settings::host_bundle`; 22 new |

The fixes, each with a test that fails on the code of podvm:

* a `dev` that is a symlink out of the root: the removal of a link at a
  device path and the `/dev` links went through a host path, so the
  completion removed a file and made links in a host directory that the
  image named (`a_dev_that_links_out_of_the_root_changes_nothing_on_the_host`);
* a fifo of the image at a path that the crate reads stopped the completion
  for ever (`a_fifo_reads_as_no_file_and_does_not_wait`,
  `a_fifo_at_a_configuration_path_does_not_stop_the_completion`);
* a device node on a file system mounted `nodev` was a "real" node that
  does not open (`a_node_that_does_not_open_is_replaced_by_a_shim`, as
  root);
* a grown host bundle got no rehash step
  (`a_grown_host_bundle_asks_for_a_rehash_again`);
* the C library of armv7, i686, riscv64, powerpc64le, s390x and
  loongarch64 was `unknown` (`the_c_library_is_found_on_each_architecture`);
* `passwd`, `group` and `nsswitch.conf` of the image behind a link inside
  the root were replaced (`a_file_of_the_image_behind_a_link_is_kept`);
* a user part or an address in another form reached the probe, and a
  loopback source was changed
  (`the_probe_gets_the_host_and_not_the_user_part_or_an_address`);
* a line end in an added host or a container name wrote more lines into
  `/etc/hosts` (`a_line_end_in_an_added_host_or_a_name_is_refused`);
* a mount point with a space did not match the root
  (`a_mount_point_with_a_space_holds_the_root`);
* a byte of a CA bundle of the image that is not UTF-8 became U+FFFD
  (`the_bytes_of_the_image_bundle_survive_the_append`);
* a mode change by name followed a link put there after the check
  (`a_mode_change_does_not_follow_a_link`).

## Adoption map (podvm)

| Old path | New path |
| --- | --- |
| `podvm_complete::{devices, identity, net, pkg, sources, write}` | `pod_rootfs::*` |
| `podvm_complete::complete(rootfs, &Options)` | podvm keeps it: `pod_rootfs::complete` with `settings(rootfs, opts)` and `reach::probe` |
| `podvm_complete::Options` | podvm keeps it |
| `podvm_complete::{Report, Fixup, Step}` with `entry`, `Report::{banner, json, degradations}` | podvm keeps them, from `pod_rootfs::Report` with `entry(id)` |
| `podvm_complete::{Action, Libc}` | `pod_rootfs::{Action, Libc}`, re-exported |
| `podvm_complete::reach::Prober` | `pod_rootfs::reach::Probe`; podvm keeps `reach::probe` over `podvm_image::net` |
| `pkg::HOST_CA_MARKER`, `pkg::HOST_CA_FILE_PREFIX`, `pkg::APT_DROP_IN`, the names of the shims and the comments | `podvm_complete::names()` |
| `sources::BACKUP_DIR` | `podvm_complete::BACKUP_DIR`, `Settings::originals` |
| `podvm-extract` of `crates/podvm-complete/Cargo.toml` | leaves (`pod-rootfs` takes `pod-extract`) |
| `crates/podvm-complete/src/{devices,devices/tests,identity,net,pkg,pkg/bundle,pkg/hash_dir,pkg/tests,sources,write,write/follow,write/tests}.rs` | leave podvm |
| The pin `32d write` of podvm's gate | goes, with the test |

## Tests

`cargo test -p pod-rootfs`. Each test makes its own temporary directory
and sets no variable. No test needs the network: a probe is `Fixed` or a
closure. The device tests take the arm of the host (a real node where
`mknod` is permitted, else a shim) and check what each row claims against
the disk. The test of a `nodev` file system needs root and a mount
namespace of its own thread, and stops with a skip line elsewhere.

The tests of two layers (`src/write/layers/tests.rs`): an entry of the
upper layer hides the same name of the lower layer; a listing is the
union, a whiteout hides a name, a new directory over a whiteout of a lower
entry is refused, an opaque directory hides the lower one (a skip line
where the host refuses a whiteout or a `user.` attribute); a change of a
lower file copies it up with its mode, and its parents with theirs; a
remove of a lower entry is refused with its type; a link resolves in the
merged view; layers that hold each other are refused; and the whole
completion of a fixture image on two layers gives the tree and the rows of
the completion of one root that holds the same tree, leaves the lower layer
with its digest (with the times), and changes nothing on a second run.
`tests/two_layers.rs` is the acceptance test of podvm (PodUtils/podvm#327):
the merged view against the completion of a copy, the lower layer, and an
upper layer with no unchanged copy; the first stops with a line `SKIP:`
where `mknod(2)` is refused, as each run draws new bytes for the random
shims. The plants: `rootfs-layers-write-to-lower`,
`rootfs-layers-list-upper-only`, `rootfs-layers-lower-over-upper`.

## Sources

`SOURCES` lists each imported file of podvm with its blob, and the
`check-sources.py` check compares them with podvm's `main`.
