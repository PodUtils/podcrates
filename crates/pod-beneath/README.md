# pod-beneath

The open of a path under a base directory with no symbolic link and nothing
above the base. The crate splits the host path at the base, opens the base,
and opens the rest under the base with
`openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)`. The kernel refuses each
link and each `..` above the base in the same call, so a link that another
process makes after a check moves no open out of the base. The result is a
descriptor, or a `Refusal` that names the case.

The crate is `no_std` and does no allocation: the caller gives the bytes of
each path and a buffer for the NUL-ended copy. The caller also gives the
system calls (`Calls`), so that an `LD_PRELOAD` interposer or a handler of
Syscall User Dispatch keeps its own trap. The crate is Linux only; on another
target it is empty. Its one dependency is `linux-raw-sys` (constants only:
the open flags, the errno numbers and the number of `openat2` of each
architecture).

T-997 (PodUtils/podvm#611) wrote it from the rule of the copies of podvm
and of `pod-enter` (`--podvm-volume-base`, PodUtils/podrunner#277 and #300).

## Use

```rust
use pod_beneath::{Libc, O_DIRECTORY, O_PATH, Refusal, open_under_base};

let mut buf = [0u8; 4096];
match open_under_base(&Libc, b"/srv/jobs", b"/srv/jobs/42/work", O_PATH | O_DIRECTORY, &mut buf) {
    Ok(fd) => bind_by_descriptor(fd),            // the caller closes fd
    Err(Refusal::NotUnderBase) => bind_by_path(), // the base does not apply
    Err(r) => refuse(r.errno(), r.text()),
}
```

## API

| Item | What |
| --- | --- |
| `split(base, host) -> Result<&[u8], Refusal>` | the path of `host` under `base`, with no `/` at its start; `.` for the base itself; a lexical test (a `..` stays, the kernel decides it) |
| `open_under_base(calls, base, host, flags, buf) -> Result<c_int, Refusal>` | the base by its path (`openat2` with no `resolve`; the caller chose the base, and it can hold links), then the rest under it by `open_beneath`; the base descriptor is closed |
| `open_beneath(calls, dirfd, rel, flags, mode, buf) -> Result<c_int, Refusal>` | one `openat2` of `rel` under `dirfd` with `RESOLVE_BENEATH \| RESOLVE_NO_SYMLINKS` and `O_CLOEXEC`; an empty `rel` is `.`; `O_CREAT` is safe: a link planted at the last name is `Refusal::Link` |
| `open_parent_beneath(calls, dirfd, rel, buf) -> Result<(c_int, &[u8]), Refusal>` | the directory of the last name (`O_PATH \| O_DIRECTORY`) and the last name, for a call that makes its file or does not follow a link at the last name; where the last name is empty, `.` or `..`, the descriptor of `rel` itself and an empty name |
| `last_name(rel) -> (&[u8], &[u8])` | the directory and the last name of `rel`, with no system call |
| `Calls` | `openat2(dirfd, &CStr, &OpenHow) -> Result<c_int, c_int>` (the errno, a positive number) and `close(fd)`; implemented for `&C` |
| `Libc` (feature `libc`, default) | `Calls` through `syscall(2)` and `close(2)` of the C library (glibc or musl) |
| `OpenHow` | `struct open_how`: `flags`, `mode`, `resolve` |
| `EAGAIN_TRIES` | 256: the tries of one open that the kernel answers with `EAGAIN` (a rename that raced a `..` of the walk) |
| `O_PATH`, `O_DIRECTORY`, `O_NOFOLLOW`, `O_CLOEXEC`, `O_CREAT`, `O_EXCL`, `O_WRONLY`, `O_RDWR`, `RESOLVE_BENEATH`, `RESOLVE_NO_SYMLINKS`, `BENEATH`, `AT_FDCWD`, `SYS_OPENAT2` | the values of the architecture of the build (`O_DIRECTORY` and `O_NOFOLLOW` differ between x86_64 and aarch64) |

### `Refusal`

| Case | Errno (`errno()`) | When |
| --- | --- | --- |
| `NotUnderBase` | `EXDEV` | `host` is not absolute, or not at or under `base`; nothing opened |
| `BaseNotAbsolute` | `EINVAL` | `base` does not start with `/` |
| `Base(errno)` | the errno | the base does not open as a directory |
| `Link` | `ELOOP` | a name on the way, or the last name, is a symbolic link |
| `DotDot` | `EXDEV` | a `..` goes above the directory of the walk; an absolute `rel` under a descriptor too |
| `NotDirectory` | `ENOTDIR` | a name on the way, or the last name with `O_DIRECTORY`, is not a directory |
| `NoOpenat2` | `ENOSYS` | the kernel has no `openat2(2)` (Linux 5.5 and older), or a filter hides it; no weaker walk replaces it |
| `TooLong` | `ENAMETOOLONG` | the path and its NUL do not fit `buf` |
| `Nul` | `EINVAL` | the path holds a NUL byte |
| `Open(errno)` | the errno | another cause: `ENOENT`, `EACCES`, `EAGAIN` after `EAGAIN_TRIES` |

`text()` gives one line for each case, with no errno and no path;
`Display` adds `(errno N)` to `Base` and `Open`. A consumer words its own
line from these, so the copies give the same causes.

## The rules

* The base opens by its path and can hold links: the caller chose it (podvm
  canonicalizes `--podvm-volume-base`). Each name after it is checked.
* No descriptor of the crate stays open after a call, except the one that
  it gives.
* No allocation and no lock: the interposer calls the crate inside a libc
  path call.
* A link inside a volume is a refusal. A caller that maps guest names (the
  walk of the interposer) resolves its own links before the call; a link
  that reaches the crate is a change of the host.

## The take of podvm

podvm takes the crate by its git tag, with `default-features = false` where
it gives its own `Calls`.

| Copy | Today | The take |
| --- | --- | --- |
| `crates/podvm/src/userns/host.rs`: `beneath`, `check_volume_under_base`, `bind_volumes` | `open(2)` of the base with the `O_DIRECTORY` value of x86_64 (on aarch64 that value is `O_DIRECT`), `syscall(437)`, `metadata` of `/proc/self/fd/N` for the directory | `open_under_base(&Libc, base, host, O_PATH \| O_DIRECTORY, &mut buf)` for the check of the loader family (`NotUnderBase` is `Ok(())`, `NotDirectory` is "is not one"); `O_PATH` alone for `bind_volumes`, whose volume can be a file |
| `crates/podvm-dispatch/src/vols.rs`: `under_base`, `beneath` | `sys::openat2` with its EAGAIN loop | a `Calls` over `sys::openat2` and `sys::close` (the trap that Syscall User Dispatch lets through); `open_under_base(&calls, base, host, O_PATH \| O_DIRECTORY, &mut buf)`; `NotUnderBase` keeps the `canonicalize` branch |
| `crates/podvm-interpose/src/map/base.rs`: `pin` | its own `openat2` and split, its errno by hand | a `Calls` over `raw_calls::raw` (feature `libc` off: the object exports `syscall`); the volume by `open_under_base(.., base, vol, O_PATH \| O_DIRECTORY, &mut [0; OUT])`; the rest by `split(vol, host)`; a call that follows by `open_beneath(.., vfd, rest, O_PATH, 0, ..)`, and on `Open(ENOENT)` for a creator by `open_parent_beneath`; a call that does not follow by `open_parent_beneath`; the errno of a refusal by `errno()` |

The test `a_path_of_a_volume_under_the_base_reaches_no_file_out_of_the_base`
(`tests/table.rs`) holds that take of `pin` as a function, with the cases
of the interposer's test.

## pod-enter

`pod-enter` takes the crate for the host side of a volume
(`VolumeBind::host_base`, `ns_volume.rs`): `split` before the fork, and
`open_under_base` before the fork and again in the child, through
`pod_hostprobe::sys::Beneath` (the `Calls` of the trap of `pod-hostprobe`,
which takes its `OpenHow` from this crate). The guest side keeps its own walk
(`openat2(RESOLVE_IN_ROOT | RESOLVE_NO_SYMLINKS)`, one name at a time, each
absent directory made): a different rule (call C835).

## Tests

* `tests/table.rs`: the check of a volume as a directory (the cases of
  podvm's `a_volume_of_the_loader_family_opens_under_its_base_as_a_directory`
  and `a_volume_opens_under_its_base_with_no_link`), the interposer's
  class of `a_volume_under_the_base_reaches_no_file_out_of_the_base`, the
  split, the buffer, the text of each refusal.
* `tests/race.rs`: a link made between the check and the call (a `Calls`
  that changes the tree just before the walk), a volume replaced by a
  link, a `..` above the base and the volume, an absent name with a
  planted link, a kernel with no `openat2` (a `Calls` that answers
  `ENOSYS`), and the tries of `EAGAIN`.

No test needs a privilege.
