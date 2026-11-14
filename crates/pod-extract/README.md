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
   owner bits) through a descriptor opened with `O_NOFOLLOW`.
7. **Whiteouts first.** The whiteouts of a layer apply to the tree before
   the entries of that layer. Hard links resolve in the destination.

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
| `Extracted` | `rootfs`, `sidecar`, `layers`, `entries`, `removed`, `uncompressed_bytes`, `uncompressed_estimated`, `skipped`, `skipped_kinds`, `sidecar_rows`, `ownership_dropped`, `first_dropped` |
| `Place::{is_extracted, remove}`, `remove_tree(path)` | the marker decides, not the directory; the removal takes the marker first and never follows a symlink out of the tree |
| `LAYER_EVENT` | `extract.layer`: `<12 hex digits>: Extracted (N entries, M whiteouts)` |
| `Refused`, `refused(&failure)` | the cause of a refused entry: `layer`, `entry`, `refusal` |
| `Mismatch`, `mismatch(&failure)` | the cause of a blob that does not agree with its digest: `what`, `want`, `got` |
| `apply` | `apply_layer(root, digest, stream, ids, sidecar)`, `LayerStats`, `Ids::current()` |
| `layer` | `Compression::of(media_type)` (tar, `+gzip`, `.tar.gzip`, `+zstd`), `open_hashed(path, c)`, `HashedStream::digest`, `uncompressed_size(path, c, compressed)` (exact for gzip under 4 GiB, an estimate with its flag for the others) |
| `remove` | `collect(layer, stream)`, `apply(root, ops)`, `Op`, `remove_at(parent_fd, name)` |
| `safety` | `components`, `is_archive_root`, `rebase_symlink_target`, `Refusal`, `Dir`, `Resolve::{Auto, Walk, InRoot}`, `open_parent`, `open_parent_with`, `have_openat2` |
| `sidecar` | `Meta` (`to_json`), `Sidecar` (`create`, `push`, `finish`, `dropped`, `first_dropped`), `FirstDropped`, `json_string` |
| `whiteout` | `classify`, `Kind`, `basename`, `dirname_parts`, `WH`, `OPQ` |

`Resolve::InRoot` is for a writer of a FINISHED root file system (the
completion of `pod-rootfs`): it follows a symlink of a directory component,
held in the root by `RESOLVE_IN_ROOT`, and refuses on a kernel with no
`openat2`.

Each failure is a `pod_core::Failure` at `Stage::Host`. The classes:
`Policy` (a refused entry or whiteout, with the cause `Refused`), `Protocol`
(a blob whose digest does not agree, with the cause `Mismatch`),
`Unsupported` (a compression that the crate cannot read, a digest that is
not sha256), `Usage` (a name with a NUL), `Unavailable` (no space or no
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

`drive::setid::set_id_bits_survive_the_write` extracts files of mode 4755,
2755 and 6755 as the user of the test. As root it also runs the test
binary again as uid 1000 and gid 1000
(`drive::setid::worker_extracts_as_a_user`; a skip line where the host
cannot start that child): the three modes stay, and in a directory of
group 0 with `S_ISGID` the sidecar records 0755 and 4755, the modes that
the kernel kept. The plants: `extract-t581-set-id-bits-not-set-again`,
`extract-t581-kept-mode-not-recorded`.

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

## Sources

`SOURCES` lists each imported file of podvm with its blob, and the
`check-sources.py` check compares them with podvm's `main`.
