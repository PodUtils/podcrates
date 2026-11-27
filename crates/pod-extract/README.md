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
   permission check of the kernel. A directory that the crate makes for an
   entry below it (a file `usr/bin/x` with no entry for `usr/bin`) gets a
   row of the same form, with owner 0 and mode 0755, the directory that
   `flatten` writes for it: the last row of a path holds, and the row of a
   path that a whiteout removed does not hold for a directory made again
   there.
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
   the tree. A directory that the walk makes for an entry below it gets
   mode 0755 through the descriptor of the descent, under each umask.
7. **Whiteouts first.** The whiteouts of a layer apply to the tree before
   the entries of that layer. Hard links resolve in the destination: a
   link whose target, or a directory above its target, is missing at its
   entry waits for the end of its layer. A target that is missing then, or
   a directory, refuses the layer (`Refusal::Link`), and so does an entry
   below a link that waits. An
   entry that is not a directory removes a directory at its path with each
   entry below it, as the newer entry hides the directory in a union file
   system.

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
not set. libarchive (`bsdtar`) writes each attribute twice by default:
`LIBARCHIVE.xattr.NAME` (the name with percent escapes, the value in
base64) and `SCHILY.xattr.NAME`. The `SCHILY.xattr.` record holds; a
`LIBARCHIVE.xattr.` record alone (`--options xattrheader=LIBARCHIVE`) is
decoded and set as each other attribute, and a record that does not decode
is named as not set.

**GNU sparse files.** GNU tar `-S --format=posix` writes a sparse file as
its regions of data and a map (`GNU.sparse.`, versions 0.0, 0.1 and 1.0;
1.0 puts the map at the start of the data). The crate writes each region at
its offset and sets the real size last, so each hole stays a hole, under
the name `GNU.sparse.name`. A map of more than 262144 regions, a map in the
data longer than 1 MiB, a region out of order or past the real size, a map
whose regions do not hold the data, a version that is not 0.0, 0.1 or 1.0,
and a map on an entry that is not a regular file refuse the layer.

**POSIX ACLs.** GNU tar `--acls`, libarchive and star write an ACL in its
text form, `SCHILY.acl.access` and `SCHILY.acl.default` (GNU tar: one entry
on each line). The crate converts the text to the binary form of the kernel
(`system.posix_acl_access`, `system.posix_acl_default`: version 2, then
each entry as tag, permissions and id, little endian, sorted as
`acl_to_xattr` sorts them) and sets it as each other attribute, with the
same counts of set and not set. When the entry has the binary form too
(GNU tar `--xattrs --acls`), the binary form holds and the text is not
read. An ACL whose entry names a user or a group with no id (GNU tar with
no `--numeric-owner`) is named as not set, with no value: the name comes
from the user database of the writer, so the crate does not map it to an
id. An ACL that is not valid (acl(5)), and an NFSv4 ACL
(`SCHILY.acl.ace`), are named as not set. The mode of the entry stays in
each case. A default ACL applies to each entry that a later write makes
in its directory, as with containerd.

**The PAX records.** Each record of a PAX header is in one class:

| Class | Keys | Where |
| --- | --- | --- |
| The crate uses it | `SCHILY.xattr.`, `LIBARCHIVE.xattr.`, `SCHILY.acl.`, `GNU.sparse.`, `path`, `linkpath` | the `Member` of the entry; a `path` or a `linkpath` with no newline byte goes to the `tar` reader too; `GNU.sparse.name` is the path of the entry |
| The `tar` reader needs it | `size` (the last), `uid`, `gid` | the `tar` reader; a `uid` or a `gid` with a newline byte refuses the layer, as the `tar` reader cuts it |
| Neither uses it | each other key: `mtime`, `atime`, `ctime`, `uname`, `gname`, `comment`, `hdrcharset`, `SCHILY.fflags`, `SCHILY.dev`, the other `LIBARCHIVE.` keys, `RHT.`, a key of a vendor | dropped, with any byte in its value |

**The final tree as one tar stream (`flatten`).** Use `flatten` in place
of `extract` when the tree goes to a reader of a tar stream (the image
import of incusd) and needs no copy on the disk of the caller. It reads
the same `Layer`s, each blob once, from the newest layer, and writes each
entry that no newer layer hides as it reads it, to a `Write`: the paths,
kinds, modes, owners of the image, contents and symlink targets of the
tree of `extract`, with the same refusals (a path with `..`, an entry
through a symlink of an older layer or of its layer, a target above the
root, a name past 255 bytes); with faults in two layers, `flatten` names
the fault that it reads first, from the newest layer. A directory entry can come after its
children, and a child of an older layer comes after the entry of its
directory from a newer layer. GNU tar with the arguments of incusd takes
that order, with the mode and the owner of each directory. GNU tar as a
user that is not root sets the mode of a directory when the stream leaves
it, so a directory entry whose mode takes the write or the search mode
from its owner (`usr/bin` 0555) waits: it goes out after the last layer, a
parent before its children, before the entries of `after`. The memory of
such an entry is its header and its records. A directory that the layers
make with no entry (a file below it) goes out after the last layer, with
mode 0755 and owner 0, as `extract` makes it. A GNU sparse file goes out with
its holes as zero bytes. A failure after the first byte writes one block
that is not a header in place of the end blocks: the reader fails and
does not take a short tree, and the caller discards the upload. The
memory: about 52 bytes and the name of each path, up to
`Options::paths_max` paths (`PATHS_MAX`, 8 Mi).

**The records and the entries of the caller (`flatten`).** Each entry of
the stream has the owner ids and the mode of the image (with setuid,
setgid and sticky), and the time of the image, or 0 for a directory that
the layers make with no entry: the stream depends on the layers alone.
The extended attributes that `extract` sets go into the stream as
`SCHILY.xattr.NAME` records, inside the same bounds and with the same
records named and not written (`Flattened::xattrs`): a `LIBARCHIVE.xattr.`
record decoded, a mark of overlayfs not written, and an ACL in its binary
form (`SCHILY.xattr.system.posix_acl_access`), also when the layer has
its text form. GNU tar with the arguments of incusd (`--xattrs-include=*`,
no `--acls`) sets each `SCHILY.xattr.` record, the binary ACL and
`security.capability` included (as root), and does not read
`SCHILY.acl.`. GNU tar does not set a record of a hard-link entry, and
`extract` sets the records of a link entry on its inode: so a hard-link
entry of the stream has no records, and the records of the links of an
inode go with its data entry, over the records of the same name of its
file (a newer link over an older one). Where the data went out before
the link was read (a link after its file in one layer), each record of
the link that the data entry does not have is named in
`Flattened::xattrs` and not written. `flatten_with` takes two closures: `before` writes entries
before the tree, `after` after it, each through `Entries` (`dir`, `file`,
`file_from`, `symlink`; owner 0, time 0, the path as the caller gives
it). `Options::prefix` (`rootfs/` for the unified tarball of incus) goes
before each path of the image and before the target of each hard link,
not before the target of a symlink; a directory entry of each of its
components (`Options::prefix_mode`) goes out after the entries of
`before`. An entry of the caller at a path of the image in the stream, at
a directory of the prefix, or below a file or a symlink of the image is
refused (`Usage`, the path named), not merged; an entry of `before` is
checked after the tree. The end blocks go out once, after the entries of
`after`, or not at all with `Options::end` false, for a caller that
appends more.

**The hard links of `flatten`.** Each inode goes out once, at its first
name in the stream, and each other name is a hard-link entry after it. A
link names the inode at its target at the time of the link, as `extract`
makes it: a file of the same layer before it, else the target of the
older layers, else the entry of the target after it in its layer. A link
to a file that a newer layer replaced or removed keeps the old bytes: the
file goes out under the name of the link, as it is read. A hidden file
that a link of its layer names after it went by before the link: its
layer is read a second time after the last layer, once for each such
layer (`Flattened::rereads`), with its digest checked again. So is a file
whose path a later entry of its layer replaced, when a link names its
inode through a link that has no name in the stream. The inode
has the mode, the owner and the time of its file entry; a link entry has
those of the link of the image. A link to a missing target or to a
directory, and a link through a symlink or out of the tree, refuse the
stream as `extract` refuses the layer. The memory: a group of names for
each hard link of the image, and the files of the current layer (its
hidden files, and the member of each file that went out at its own path:
about 2 MiB more for one layer of 200 000 files).

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
| `apply` | `apply_layer(root, digest, stream, ids, sidecar)`, `LayerStats` (`entries`: the members read; `skipped`, `skipped_kinds`: the entries of a type that the crate does not make, not the root member; `dir_modes`: each directory entry of the layer and its mode; `replaced`: each path where the layer made an entry that is not a directory, with the count of `dir_modes` before it; `xattrs`), `Ids::current()` |
| `modes` | `DirModes` (`new`, `record(&stats.dir_modes)`, `layer(&stats)` (the modes of the layer, and each directory that a non-directory of the layer replaced is forgotten, in the order of the layer), `forget(&ops)`, `set(root, sidecar)`, `len`, `is_empty`): the mode of the image of each directory, the last layer wins, set after the last layer and read back, a mode that the kernel did not keep is one more row of the directory with `applied.mode`; `open_tree(root)`: the owner bits on each directory of a tree that was there before, with `O_NOFOLLOW`, also of a directory with no owner read, one directory open at a time, a tree deeper than `MAX_DEPTH` refused with the cause `TooDeep` |
| `flatten` | `flatten(&layers, &mut out, &options) -> Result<Flattened>` (the final tree as one tar stream, see above), `flatten_with(&layers, &mut out, &options, before, after)` (with the entries of the caller through `Entries`: `dir(path, mode)`, `file(path, mode, data)`, `file_from(path, mode, size, reader)`, `symlink(path, target)`), `Options` (`report`: one `flatten::LAYER_EVENT` for each layer, `flatten.layer`: `<12 hex digits>: Flattened (N entries written, H hidden, W whiteouts)`; `paths_max`, default `PATHS_MAX`; `prefix`, default empty; `prefix_mode`, default 0755; `end`, default true), `Flattened` (`layers`, `entries`, `written` (the hard links included), `links` (the hard-link entries), `rereads` (the layers read a second time), `implied`, `hidden`, `whiteouts`, `skipped`, `skipped_kinds`, `xattrs` (`xattr::Count`: the records written, the records named and not written), `extra` (the directories of the prefix and the entries of the caller), `bytes`, `paths`); a refusal is the cause `Refused` with `Refusal::Stream` for a file or a link over a directory that its layer filled in the stream before it, and `Refusal::Link` for a hard link as `extract` gives it |
| `layer` | `Compression::of(media_type)` (tar, `+gzip`, `.tar.gzip`, `+zstd`), `open_hashed(path, c)`, `HashedStream::digest`, `uncompressed_size(path, c, compressed)` (exact for gzip under 4 GiB, an estimate with its flag for the others) |
| `pax` | `Framed::new(stream, side)` (the stream of a layer as the `tar` reader reads it: an `x` past `PAX_MAX` (1 MiB) and an `L` or a `K` past `LONG_MAX` (4096) refused before their data, with the cause `Refusal::Member`; the records of an `x` parsed by their length; one `size` record to the `tar` reader, the last; each class of record above), `Side` (`next()`: the `Member` of each entry of the `tar` reader, in order; `refused()`), `Member` (`path`, `linkpath`, `xattrs`: the `SCHILY.xattr.`, `LIBARCHIVE.xattr.` and `SCHILY.acl.` records; `sparse`: the `GNU.sparse.` records, `Option<sparse::Sparse>`), `records(data)` |
| `sparse` | `Sparse` (`real_size`, `map`), `Map` (`Records(Vec<Region>)`: versions 0.0 and 0.1; `Data`: version 1.0), `Region` (`offset`, `len`), `Layout` (`real_size`, `regions`: the map read and checked), `REGIONS_MAX` (262144), `MAP_MAX` (1 MiB) |
| `acl` | `to_xattr(text) -> Result<Vec<u8>, String>` (the binary form of an ACL in its text form, or why it does not convert), `attribute(kind)` (`access`: `ACCESS`, `default`: `DEFAULT`), `PAX_PREFIX`, `ENTRIES_MAX` |
| `remove` | `collect(layer, stream)`, `apply(root, ops)`, `Op`, `remove_at(parent_fd, name)` |
| `tree` (also at the root) | `MAX_DEPTH` (4096: the deepest directory below the start of a walk, and the most components of the path of an entry), `TooDeep` (`path`, `limit`), `too_deep(&failure)`. The walk of `open_tree` and of the removal keeps the listing and the device and inode of each level, holds one directory open, and goes up through `..` only when its device and inode are those of the level above (`TempFail` when a directory moved) |
| `safety` | `components` (more than `MAX_DEPTH` components: `Refusal::TooDeep`; a component of more than `COMPONENT_MAX` (255) bytes: `Refusal::TooLong`; an extension member past its bound or a PAX record that does not parse: `Refusal::Member`; a hard link whose target is missing or a directory: `Refusal::Link`, with `LINK_MISSING`), `is_archive_root`, `rebase_symlink_target`, `Refusal`, `Dir`, `Resolve::{Auto, Walk, InRoot}`, `open_parent`, `open_parent_with` (with `InRoot` and `make_dirs`, a closed parent opens for the one make), `have_openat2`, `OwnerBits` (`add(fd)`, `was`: the owner bits of a closed directory for one step, its mode back on drop) |
| `sidecar` | `Meta` (`to_json`; `xattrs`: the attributes of the path that are not set, as `"xattrs":[{"name","value":"0x<hex>","reason"}]`, with no `value` for a record past a bound), `Sidecar` (`create`, `push`, `push_dir`, `kept_dir`, `finish`, `dropped`, `first_dropped`), `FirstDropped`, `json_string`; the last row of a path holds |
| `whiteout` | `classify`, `Kind`, `basename`, `dirname_parts`, `WH`, `OPQ` |
| `xattr` | `read(&member.xattrs, path) -> Wanted` (the records of an entry, read by their length in `pax`, each ACL in its binary form, inside the bounds `NAME_MAX`, `VALUE_MAX`, `NAMES_MAX`, `VALUES_MAX`, `RECORDS_MAX`; `PAX_PREFIX`, `LIBARCHIVE_PREFIX`, `OVERLAY`), `set_fd(fd, &wanted)`, `set_at(parent, name, &wanted)` (a descriptor with `O_NOFOLLOW`, else `lsetxattr` of `/proc/self/fd/N/NAME`) `-> Applied`, `Unset` (`path`, `name`, `value`, `reason`, `to_json`), `Count` (`add`, `merge`, `line`; the first `FIRST_NAMED` not set), `of_path(path) -> io::Result<Vec<(String, Vec<u8>)>>` (each attribute of a path on disk with no follow of a symlink: the size of the list from the kernel, a new read on `ERANGE`, the bounds `NAMES_MAX` and `VALUE_MAX`, `ENOTSUP` an empty list, a name removed before its read skipped, each error with the path) |

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
`extract-t614-root-test-skips`). The test of GNU tar over a directory
that a newer layer closed
(`gnu_tar_takes_a_directory_that_a_newer_layer_closed`) runs as each
user: `flatten` writes such a directory entry after the last layer.

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
Under a umask of 077, a directory that a file below it made has mode 0755
(`a_directory_made_for_a_file_below_it_has_mode_0755_under_a_umask_of_077`),
and `tests/flatten_random.rs` runs its two comparisons again under that
umask in a child (`random_layer_sets_give_the_tree_of_extract_under_a_umask_of_077`).
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

`pax::tests::each_class_of_pax_record_goes_to_its_reader_with_a_newline_byte_or_none`
(PodUtils/podvm#643): a record of each class, with a newline byte in its
value and with none, goes to the member, to the `tar` reader, to both or to
neither, or refuses the layer.
`pax::tests::a_layer_of_gnu_tar_with_acls_reads_and_each_acl_is_one_attribute`:
the records of GNU tar 1.35 `--format=posix --xattrs --acls` read, and each
ACL is one attribute. `acl::tests` convert the text forms of GNU tar and
libarchive to the bytes that Linux gave for the same ACL, and name the
cause of each ACL that does not convert.
`drive::acls::the_acls_of_gnu_tar_are_set_or_named_and_the_modes_stay`: an
access ACL and a default ACL are on disk (or named, where a probe of the
same ACL fails), an ACL with a name is named, and the modes stay. The
plants: `extract-t1086-acl-text-dropped`,
`extract-t1086-sparse-not-refused`.

`sparse::tests` (T-1087): the records of GNU tar 1.35 `-S --format=posix`
for each version (0.0, 0.1, 1.0) extract a file of 1 MiB with its content,
its name and its holes
(`each_version_of_a_gnu_sparse_file_of_pax_extracts_with_its_holes`); the
records give the map and the name, and none goes to the `tar` reader; a
map that does not hold (19 cases) and a map on a symlink refuse the layer
and write no file. The plant: `extract-t1087-sparse-map-not-applied`.

`drive::xattrs` (T-1088): a `LIBARCHIVE.xattr.` record alone is read from
its base64 value with its name decoded
(`a_libarchive_xattr_record_alone_is_read_from_its_base64_value`), the
`SCHILY.xattr.` record of the same name holds, a record that does not
decode is named and not set, and a layer of `LIBARCHIVE.xattr.` records
alone sets or names its attribute. The plant:
`extract-t1088-libarchive-xattr-dropped`.

`drive::the_root_member_is_not_a_skipped_entry` (PodUtils/podvm#352): a
layer with `./` and a file has 2 entries and 0 skipped; a layer with `./`
and a fifo has 1 skipped, named `Fifo`. The plant:
`extract-t599-root-member-skipped`.

## Sources

`SOURCES` lists each imported file of podvm with its blob, and the
`check-sources.py` check compares them with podvm's `main`.
