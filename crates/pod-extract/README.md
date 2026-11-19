# pod-extract

The rootless apply of the layers of an OCI image on Linux. The crate reads
each layer entry by entry in its own process and writes the tree with raw
system calls from a held directory descriptor. It does not start `tar` and
does not call the `unpack()` of a library. Seven rules:

1. **Entry level.** Each decision (ownership, permission, path) is made
   here, not by `tar` or a library.
2. **No ownership change.** `chown` to an id that the user namespace does
   not map gives `EINVAL`. The crate does not try: the ids of the image go
   into a sidecar (one JSON line for each path), with the ids that the
   kernel applied and the reason of each difference. The sidecar changes no
   permission check of the kernel.
3. **Whiteouts on the basename.** `.wh.NAME` and `.wh..wh..opq` are found on
   the basename, also at the root of a layer, where a glob `*/.wh.*` misses
   them.
4. **Containment.** The path of each entry and of each whiteout gets a
   lexical check (no `..`, no absolute path, no NUL), then each parent opens
   from the destination with `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)`,
   or with an `O_NOFOLLOW` walk on a kernel before 5.6. An entry through a
   symlink that the same layer made (`evil -> /etc`, then `evil/passwd`) is
   refused, and the layer is refused whole.
5. **Absolute symlink targets are relative to the root file system.** They
   are rebased for the check and stored with no change; a target that goes
   above the root is refused.
6. **The owner bits are widened between layers** on the directories that
   the layer wrote, so that the next layer can write. Group and other bits
   stay. The umask of the process does not change a mode: each file gets
   its mode through its descriptor after the last write, and each
   directory that the layer wrote gets the mode of the layer (with the
   owner bits) through a descriptor opened with `O_NOFOLLOW`. After the
   last layer, each directory entry of the extraction gets the mode of the
   image (the last layer that names it wins), the deepest first, through
   the same descriptor: a directory of mode 0555 in the image is 0555 in
   the tree.
7. **Whiteouts first.** The whiteouts of a layer apply to the tree before
   the entries of that layer. Hard links resolve in the destination.

**Extended attributes.** Each `SCHILY.xattr.NAME` record of an entry is
set on it: on a file through its descriptor after its last write and
before its mode (a write clears `security.capability`; the crate makes no
`chown`, which clears it too), on a directory, a hard link and a symlink
after they are made. The kernel decides: a process with no `CAP_SETFCAP`
cannot set `security.capability`. Each attribute that is not set is named,
with its path and its errno, in the stats, in the line of its layer and in
the sidecar row of its path, with its value (call C791). The bounds: a
name of 255 bytes, a value of 64 KiB, 64 KiB of names, 1 MiB of values and
1024 records for each entry; a record past a bound is named and not kept.
The marks of overlayfs (`trusted.overlay.`, `user.overlay.`) are named and
not set.

Each blob is hashed while it is read, in both passes, and the extraction is
refused when the digest of the manifest does not agree. The completion
marker is written last; each failure removes the partial tree, so a refused
or killed extraction never reads as extracted.

The crate is Linux only (`#![cfg(target_os = "linux")]`): on another target
it is empty. It reads no variable of the environment and prints nothing. It
keeps one process state: the answer of the `openat2` probe (the kernel does
not change while the process runs).

## API

| Item | What |
| --- | --- |
| `Settings` | `host` (the `pod_hostprobe::Settings`: the space message names its `directory_setting`), `headroom_bytes` (default `EXTRACT_HEADROOM_BYTES`, 16 MiB), `report` (the `pod_core::Report` that gets one `LAYER_EVENT` for each layer) |
| `Layer` | one layer in the order of the manifest: `digest` (`sha256:<hex>`), `media_type` (it gives the compression), `size` (of the blob), `blob` (the path in the store of the caller) |
| `Place`, `Place::under(base)` | the three paths of an extraction: `rootfs`, `sidecar`, `marker`; `under` gives `base/rootfs`, `base/.meta.jsonl`, `base/.extracted` (`ROOTFS_NAME`, `SIDECAR_NAME`, `DONE_MARKER`) |
| `extract(&settings, image, &layers, &place) -> Result<Extracted>` | the space check (blocks and inodes, before the loop), then for each layer: the whiteouts, then the entries, each pass with the digest check; the marker last; on a failure the partial tree is removed |
| `Extracted` | `rootfs`, `sidecar`, `layers`, `entries`, `removed`, `uncompressed_bytes`, `uncompressed_estimated`, `skipped`, `skipped_kinds`, `sidecar_rows`, `ownership_dropped`, `dir_modes` (the directories that the pass after the last layer set), `dirs_opened` (the closed directories of a tree that was there before, opened first), `first_dropped`, `xattrs` (`xattr::Count`: `set`, `unset`, `first_unset`) |
| `Place::{is_extracted, remove}`, `remove_tree(path)` | the marker decides, not the directory; the removal takes the marker first, never follows a symlink out of the tree, adds the owner bits to a directory that the image closed (0555) before it empties it, removes a name that is not UTF-8 by its bytes, and holds one directory open at a time (a tree deeper than `MAX_DEPTH` below the name is refused with the cause `TooDeep`) |
| `LAYER_EVENT` | `extract.layer`: `<12 hex digits>: Extracted (N entries, M whiteouts)`, then `; S extended attributes set` when the layer has some, or `; S extended attributes set, U not set (the first: NAME of PATH, ERRNO; the sidecar names each)` |
| `Refused`, `refused(&failure)` | the cause of a refused entry: `layer`, `entry`, `refusal` |
| `Mismatch`, `mismatch(&failure)` | the cause of a blob that does not agree with its digest: `what`, `want`, `got` |
| `apply` | `apply_layer(root, digest, stream, ids, sidecar)`, `LayerStats` (`entries`: the members read; `skipped`, `skipped_kinds`: the entries of a type that the crate does not make, not the root member; `dir_modes`: each directory entry of the layer and its mode; `xattrs`), `Ids::current()` |
| `modes` | `DirModes` (`new`, `record(&stats.dir_modes)`, `forget(&ops)`, `set(root, sidecar)`, `len`, `is_empty`): the mode of the image of each directory, the last layer wins, set after the last layer and read back, a mode that the kernel did not keep is one more row of the directory with `applied.mode`; `open_tree(root)`: the owner bits on each directory of a tree that was there before, with `O_NOFOLLOW`, also of a directory with no owner read, one directory open at a time, a tree deeper than `MAX_DEPTH` refused with the cause `TooDeep` |
| `layer` | `Compression::of(media_type)` (tar, `+gzip`, `.tar.gzip`, `+zstd`), `open_hashed(path, c)`, `HashedStream::digest`, `uncompressed_size(path, c, compressed)` (exact for gzip under 4 GiB, an estimate with its flag for the others) |
| `pax` | `Framed::new(stream, side)` (the stream of a layer as the `tar` reader reads it: an `x` past `PAX_MAX` (1 MiB) and an `L` or a `K` past `LONG_MAX` (4096) refused before their data, with the cause `Refusal::Member`; the records of an `x` parsed by their length; one `size` record to the `tar` reader, the last), `Side` (`next()`: the `Member` of each entry of the `tar` reader, in order; `refused()`), `Member` (`path`, `linkpath`, `xattrs`), `records(data)` |
| `remove` | `collect(layer, stream)`, `apply(root, ops)`, `Op`, `remove_at(parent_fd, name)` |
| `tree` (also at the root) | `MAX_DEPTH` (4096: the deepest directory below the start of a walk, and the most components of the path of an entry), `TooDeep` (`path`, `limit`), `too_deep(&failure)`. The walk of `open_tree` and of the removal keeps the listing and the device and inode of each level, holds one directory open, and goes up through `..` only when its device and inode are those of the level above (`TempFail` when a directory moved) |
| `safety` | `components` (more than `MAX_DEPTH` components: `Refusal::TooDeep`; an extension member past its bound or a PAX record that does not parse: `Refusal::Member`), `is_archive_root`, `rebase_symlink_target`, `Refusal`, `Dir`, `Resolve::{Auto, Walk, InRoot}`, `open_parent`, `open_parent_with` (with `InRoot` and `make_dirs`, a closed parent opens for the one make), `have_openat2`, `OwnerBits` (`add(fd)`, `was`: the owner bits of a closed directory for one step, its mode back on drop) |
| `sidecar` | `Meta` (`to_json`; `xattrs`: the attributes of the path that are not set, as `"xattrs":[{"name","value":"0x<hex>","reason"}]`, with no `value` for a record past a bound), `Sidecar` (`create`, `push`, `push_dir`, `kept_dir`, `finish`, `dropped`, `first_dropped`), `FirstDropped`, `json_string`; the last row of a path holds |
| `whiteout` | `classify`, `Kind`, `basename`, `dirname_parts`, `WH`, `OPQ` |
| `xattr` | `read(&member.xattrs, path) -> Wanted` (the records of an entry, read by their length in `pax`, inside the bounds `NAME_MAX`, `VALUE_MAX`, `NAMES_MAX`, `VALUES_MAX`, `RECORDS_MAX`; `PAX_PREFIX`, `OVERLAY`), `set_fd(fd, &wanted)`, `set_at(parent, name, &wanted)` (a descriptor with `O_NOFOLLOW`, else `lsetxattr` of `/proc/self/fd/N/NAME`) `-> Applied`, `Unset` (`path`, `name`, `value`, `reason`, `to_json`), `Count` (`add`, `merge`, `line`; the first `FIRST_NAMED` not set) |

`Resolve::InRoot` is for a writer of a FINISHED root file system (the
completion of `pod-rootfs`): it follows a symlink of a directory component,
held in the root by `RESOLVE_IN_ROOT`, and refuses on a kernel with no
`openat2`.

Each failure is a `pod_core::Failure` at `Stage::Host`. The classes:
`Policy` (a refused entry or whiteout, with the cause `Refused`), `Protocol`
(a blob whose digest does not agree, with the cause `Mismatch`),
`Unsupported` (a compression that the crate cannot read, a digest that is
not sha256; a tree deeper than `MAX_DEPTH`, with the cause `TooDeep`),
`TempFail` (a directory that moved during a walk of the tree), `Usage` (a
name with a NUL), `Unavailable` (no space or no
inodes, from `pod_hostprobe::space`), and the class of the errno of a
refused system call, with its `io::Error` as the cause and its name in the
text.

## Parity

Against podvm `crates/podvm-extract` at `e8e5beb`:

| Capability of podvm | Here |
| --- | --- |
| [x] `apply`, `layer`, `remove`, `safety`, `sidecar`, `whiteout` | the same modules and names |
| [x] `extract(store, manifest, digest, out)` | `extract(&settings, image, &layers, &place)`: the caller gives the blob paths and the place; podvm keeps `paths(store, digest)` and the glue over its `Store` |
| [x] `is_extracted`, `remove_extracted`, `DONE_MARKER`, `EXTRACT_HEADROOM_BYTES` | `Place::{is_extracted, remove}`, `remove_tree`, the same constants |
| [x] The line of each layer on `out` | the event `extract.layer` with the same text |
| [x] `space::require` before the loop, with `$PODVM_STORE` in the message | `pod_hostprobe::space::require` with `Settings::host` |
| [x] `Error::{Refused, Compression, Extract, Io, Image}` | `pod_core::Failure` with the same texts; `Refused` and `Mismatch` are typed causes |
| [x] The digest check of each pass | the same; a digest that is not sha256 is refused by name (podvm gave a mismatch) |
| [x] The ids of the process | the same, masked to 32 bits for a 32-bit target |
| [x] A file mode without owner write | the same; when the kernel refuses the final `fchmod`, the sidecar records the mode that stayed (podvm recorded none) |
| [x] A file mode with a bit of `0o7000` (4755, 2755, 6755) | set again with `fchmod` after the last write, as a write by a process with no `CAP_FSETID` clears `S_ISUID` and `S_ISGID` (podvm lost them as a user that is not root, PodUtils/podvm#342); the mode on the descriptor is read back, and a mode that the kernel did not keep (`S_ISGID` of a group that the process is not in) goes into the sidecar as `applied.mode` |
| [x] The umask of the process | the mode of the image stays under each umask: each file gets `fchmod` after the last write, also when its mode is the mode of the open, and each directory that a layer wrote gets `mode | 0o700` in the permission pass, also when nothing is widened (podvm cut 0755 to 0750 under a umask of 027); a later layer that names a directory again sets its mode. The pass opens each directory with `O_DIRECTORY` and `O_NOFOLLOW` and calls `fchmod`: `fchmodat` of the name followed a symlink that a later entry of the layer put there, to a file of the host |
| [x] The mode of a directory after the last layer | the mode of the image, set after the last layer (podvm left the widened owner bits: 0555 stayed 0755, PodUtils/podvm#354); a directory entry over a symlink or a file of an earlier layer removes that name and makes the directory (podvm kept the symlink); a directory that exists gets the owner bits, so an extraction over a closed tree passes |
| [x] The root member of an archive (`./`) | an entry that names the destination: it counts in `entries` and not in `skipped` (podvm counted it as a skipped `ArchiveRoot`, PodUtils/podvm#352) |
| [x] Whiteouts | the same; a whiteout gets the lexical check, and a whiteout of `.` or `..` refuses the layer (podvm removed the entries of the directory above the destination for `.wh...`) |
| [x] The tests: 47 unit tests | 47 with their names (the store fixtures are blob files and a `Place`); 3 new |

## Adoption map (podvm)

| Old path | New path |
| --- | --- |
| `podvm_extract::{apply, layer, remove, safety, sidecar, whiteout}` | `pod_extract::*`, re-exported by `podvm_extract` |
| `podvm_extract::{Extracted, DONE_MARKER, EXTRACT_HEADROOM_BYTES}` | `pod_extract::*`, re-exported |
| `podvm_extract::extract(store, manifest, digest, out)` | podvm keeps it: the layers of the manifest as `Layer`s with `store.blob_path`, `Place::under(<store>/rootfs/<digest>)`, `podvm_probe::settings()` as `Settings::host`, each event written to `out` when the extraction ends |
| `podvm_extract::{paths, is_extracted, remove_extracted}` | podvm keeps them over `Place` and `remove_tree` |
| `podvm_extract::Error` | podvm keeps `Error { Extract(Failure), Image(podvm_image::Error) }` |
| `podvm_extract::safety::{Dir, Refusal, Resolve, open_parent_with, have_openat2, components}` (podvm-complete) | `pod_extract::safety::*`, re-exported; `Dir::open` gives a `Failure` |
| `tar`, `flate2`, `ruzstd`, `sha2`, `serde`, `serde_json` of `crates/podvm-extract/Cargo.toml` | leave podvm-extract (`pod-extract` has the first four) |
| `crates/podvm-extract/src/{apply,drive,drive/records,layer,remove,safety,safety/tests,safety/walk,sidecar,whiteout}.rs` | leave podvm |

## Tests

`cargo test -p pod-extract`. Each test makes its own temporary directory.
The tests make real tar layers in memory (a hostile name goes into the
header past the check of `tar::Builder`), run the real extraction and look
at the tree. No test needs root, a namespace or the network. The walk and
`openat2` refuse the same traversal on one kernel; the test that asserts
`openat2` skips under an emulator that does not translate it.

The tests with a worker of uid 1000 below run that worker only as root.
CI runs them as root (`scripts/ci/root-uid1000-tests.sh`, a step of
`ci.yml`), and a skip line fails the step (T-614; the plant
`extract-t614-root-test-skips`).

`drive::setid::set_id_bits_survive_the_write` extracts files of mode 4755,
2755 and 6755 as the user of the test. As root it also runs the test
binary again as uid 1000 and gid 1000
(`drive::setid::worker_extracts_as_a_user`; a skip line where the host
cannot start that child): the three modes stay, and in a directory of
group 0 with `S_ISGID` the sidecar records 0755 and 4755, the modes that
the kernel kept. A directory of 2755 there keeps 0755, and one more row of the directory
records `applied.mode` 0755 (T-605). The plants:
`extract-t581-set-id-bits-not-set-again`,
`extract-t581-kept-mode-not-recorded`,
`extract-t605-kept-dir-mode-not-recorded`.

`drive::umask::the_modes_of_the_image_stay_under_a_umask_of_027` runs the
test binary again (`drive::umask::worker_extracts_under_a_umask_of_027`),
as the umask belongs to the whole process: under a umask of 027, the
modes 0755, 0644, 0775 and 0664 of files and directories stay, and the
sidecar claims no other mode. A later layer sets the mode of a directory
again (`a_later_layer_sets_the_mode_of_a_directory_again`), and the
permission pass does not follow a symlink at the name of a directory to
a file of the host
(`the_permission_pass_does_not_follow_a_symlink_at_the_name_of_a_directory`).
The plants: `extract-t597-file-mode-set-only-when-it-differs`,
`extract-t597-pass-only-widens`, `extract-t597-pass-follows-a-symlink`.

`drive::dirmodes` (PodUtils/podvm#354): after the last layer, a directory
has the mode of the image (0555 and 1777 stay, and a later layer that
names a directory again closes it), through `extract`
(`each_directory_has_the_mode_of_the_image_after_the_last_layer`). A
directory entry over a symlink of an earlier layer makes the directory,
and the target of the symlink outside the tree does not change
(`a_directory_entry_over_a_symlink_of_an_earlier_layer_is_a_directory`).
A directory that a whiteout removed does not get its old mode
(`a_directory_that_a_whiteout_removed_does_not_get_its_old_mode`). A
second extraction over a tree that the first closed passes, a whiteout
in a closed directory applies, and `Place::remove` removes the closed
tree (`a_second_extraction_over_a_closed_tree_passes`; as root it runs
`worker_extracts_over_a_closed_tree` as uid 1000). The plants:
`extract-t598-no-pass-after-the-last-layer`,
`extract-t598-directory-kept-over-a-symlink`,
`extract-t598-existing-directory-not-opened`,
`extract-t598-whiteout-keeps-the-old-mode`,
`extract-t598-whiteout-parent-not-opened`,
`extract-t598-removal-does-not-open-a-closed-directory`. An extraction
over a tree with a directory of mode 0311 passes, an entry under a closed
parent that the layer does not name lands, and `Place::remove` removes
the tree (`an_extraction_over_a_tree_that_its_owner_cannot_read_passes`;
as root it runs `worker_extracts_over_a_tree_it_cannot_read` as uid
1000). The plants: `extract-t606-tree-not-opened`,
`extract-t606-no-open-without-read`.

`drive::deep` (T-615): `open_tree` and `remove_tree` walk a tree of 200
levels under a soft `RLIMIT_NOFILE` of 64
(`a_tree_deeper_than_the_descriptor_limit_is_walked_and_removed` runs the
test binary again, `worker_walks_under_a_low_descriptor_limit`, as the
limit belongs to the whole process); a tree past the depth limit is
refused with `TooDeep` by both, and the refused removal keeps the tree
(`a_tree_past_the_depth_limit_is_refused`); a name that is not UTF-8 is
removed (`a_name_that_is_not_utf8_is_removed`); a directory that moved
stops the way up (`a_directory_that_moved_stops_the_walk`). An entry
with more than `MAX_DEPTH` components is refused
(`safety::tests::a_path_deeper_than_the_walk_is_refused`). The plants:
`extract-t615-walk-holds-each-level`, `extract-t615-no-depth-limit`,
`extract-t615-path-depth-not-refused`, `extract-t615-name-read-as-text`,
`extract-t615-moved-parent-followed`.

`drive::xattrs::extended_attributes_are_set_or_named` (T-957): a layer
with `security.capability` on a file and `user.test` on a file of mode
0444, a directory of mode 0555, a symlink and a hard link. A probe of the
same name on the same kind of object gives the answer: each attribute is
on disk when the probe set it, else named in the stats, the line and the
sidecar row with its value. A file system with no user attributes gives a
skip line. As root, `security.capability` must set, and the test binary
runs again as uid 1000 (`drive::xattrs::worker_extracts_as_a_user`),
where it must be named; CI runs it as root
(`scripts/ci/root-uid1000-tests.sh`).
`drive::xattrs::a_record_past_a_bound_is_named_and_not_kept`: a long name,
a long value, an empty name and the marks of overlayfs. The plant:
`extract-xattrs-dropped`.

`pax::tests` (T-961): the records parse by their length, so a value with a
newline byte round-trips and the bytes after it are not records; a `path`
with a newline byte comes from the side; the last `size` record frames the
member for both readers; a PAX header of 2 MiB and a GNU long name of
8 KiB are refused before the reader reads their data, in both passes; a
global header is an entry. The plant: `extract-t961-pax-header-unbounded`.

`drive::the_root_member_is_not_a_skipped_entry` (PodUtils/podvm#352): a
layer with `./` and a file has 2 entries and 0 skipped; a layer with `./`
and a fifo has 1 skipped, named `Fifo`. The plant:
`extract-t599-root-member-skipped`.

## Sources

`SOURCES` lists each imported file of podvm with its blob, and the
`check-sources.py` check compares them with podvm's `main`.
