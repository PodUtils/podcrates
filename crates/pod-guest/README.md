# pod-guest

A disposable QEMU guest on Linux. One emulator child, a per-run qcow2
overlay over a read-only base image, and a FAT16 mailbox volume that the
host and the guest both see. There is no daemon and no libvirt. Four
rules:

1. The accelerator comes from the machine profile of `pod-hostprobe`:
   `kvm` where the `full` profile holds, `tcg` where only `tcg` holds, and
   a refusal where neither holds. TCG is slower; it is not a refusal.
2. The base image is never written. A run writes to an overlay; a
   provision converts the overlay into a new standalone image with no
   backing file.
3. The mailbox is the only channel. The guest has no network
   (`-nic none`). The host writes the command and a random token before the
   boot, and reads the exit code, stdout and stderr after the guest stops.
   A result with another token is refused.
4. A run that times out is stopped and reported, never `Ok`. The stop says
   if the emulator was reaped (`Stop::Confirmed`) or only signalled
   (`Stop::Attempted`).

Two flavours share the mailbox, the token, QMP, the per-run directory and
the stop:

* UEFI: q35, OVMF (code read-only, a copy of the variable store for each
  run), two NVMe disks (the overlay; the mailbox with
  `cache=writethrough`). A `cmd.exe` agent, which a provision boot installs
  as an `onstart` scheduled task, writes `WQREADY.TXT` when it finds the
  mailbox, runs the command in a child `cmd /c` (stdin `NUL`; an `exit` in
  the command ends the child, not the agent), writes the result and powers
  the guest off. With `WQKEEP.TXT` on the mailbox (`Mode::KeepAlive`) the
  agent keeps the guest up and polls for the next token; a last command
  with no `WQKEEP.TXT` powers the guest off.
* DOS: FreeDOS 1.4 on `pc` with SeaBIOS and two IDE disks. The crate types
  the boot keys and two lines into the console through QMP and polls the
  mailbox for the result.

The crate also gives a guest kernel its modules: it reads the squashfs
modloop of the kernel on the host and returns the modules that the guest
needs, with their dependencies ("The modloop", below).

The crate is Linux only (`#![cfg(target_os = "linux")]`): on another
target it is empty. It reads no variable of the environment and prints
nothing. At run time it starts the emulator and `qemu-img` that the caller
names. No test needs KVM or QEMU; the two tests that start a real emulator
skip where no `qemu-system-x86_64` is on `PATH` (the KVM one also where
`/dev/kvm` does not open).

## API

| Item | What |
| --- | --- |
| `Settings` | `scratch_root` (the directory of the per-run directories), `scratch_prefix`, `keep` (keep each per-run directory), `marker` (the content of `WQMARK.TXT`), `oem` (the OEM name of the volume, 8 characters at most). `Settings::new(root)` gives `pod-guest-`, no keep, `pod-guest`, `PODGUEST` |
| `Plan`, `Accel`, `accel_for(Option<Profile>)` | the description of one run (emulator, accelerator, firmware, overlay, mailbox, serial log, QMP socket, memory, CPUs, more arguments); `Accel::{word, cpu}` (`kvm`/`host`, `tcg`/`max`) |
| `argv(&plan)`, `overlay_argv`, `convert_argv`, `plan::base_format` | the UEFI machine line with no program; `qemu-img create -f qcow2 -F FMT -b BASE`; `qemu-img convert -f qcow2 -O qcow2 OVERLAY OUT` (a new image with no backing file; `qemu-img commit` has no output file, PodUtils/podvm#277) |
| `Request`, `Mode`, `Outcome`, `mailbox(&s, agent, &request)`, `outcome(&fat16, token)` | the command, the token, the limit and the mode (`Once` by default; `KeepAlive` puts `WQKEEP.TXT` on the mailbox); the code, the token and the streams; the mailbox of a run; the result of a mailbox (`None`: the guest wrote no result) |
| `stage(&s, qemu_img, base, &plan, agent, &request)`, `run(&s, &plan, &request)`, `stop(&plan)` | the mailbox file and the overlay; the boot to the power-off and the result, with the error stream of the emulator in the per-run directory (a `KeepAlive` request is `Usage`: its guest does not power off); QMP `quit` |
| `provision(&s, qemu_img, base, &plan, out, wait)` | one boot that types the installer into the console, then the convert into `out`; returns `SETUP.TXT` |
| `scratch_dir(&s, tag)`, `scratch_of(&plan)`, `cleanup(&s, &plan)`, `RunGuard::arm(&s, &plan)` | the per-run directory `PREFIX` + pid + `-TAG`, mode 0700; the directory of the mailbox; its removal (only a directory with the prefix, not with `keep`); the removal on drop |
| `Stop`, `Timeout { after, stop }`, `timeout_of(&failure)` | the end of a stopped emulator; the cause of a run at its limit |
| `emulator` | `run_steps(argv)` (a tool to its end; a failure names the program and its stderr), `spawn_emulator(&plan, args, stderr)` (a process group of its own), `stop_emulator`, `stop_with` (QMP `quit`, `SIGKILL` to the group, a bounded reap), `QUIT_WAIT`, `STOP_WAIT` |
| `agent` | the file names of the protocol (`MARKER`, `GO`, `CMD`, `OUT`, `ERR`, `CODE`, `SETUP`, `READY`, `KEEP`, `AGENT_NAME`, `SETUP_NAME`, `AGENT`), `READY_TEXT`, `Mode`, `ready(&fat16)` (the agent wrote `WQREADY.TXT`), `AGENT_CMD`, `SETUP_CMD`, `command_file`, `command_from_argv`, `quote`, `Code`, `parse_code`, `nonce()`, `nonce_from(&mut Entropy)` |
| `dos` | `argv`, `stage(&s, ...)`, `run(&plan, batch, token, timeout)`, `batch_from_argv`, `DEFAULT_MEM_MIB`, `CPUS`, `DEFAULT_TIMEOUT_SECS`, the boot waits `TO_MENU_MS`, `TO_PROCEED_MS`, `TO_PROMPT_MS`, `MBOX_DRIVE`, `BAT` |
| `fat16` | `Fat16` (`new`, `with_oem`, `from_image`, `put`, `get`, `list`, `is_mailbox`, `image`), `format`, `format_with`, `oem_bytes`, the geometry (16 MiB behind an MBR at LBA 2048, 4 KiB clusters), `LABEL`, `OEM` |
| `fetch` | `Ceiling { max_bytes, fsize }`, `check_declared`, `store(reader, declared, dest, want, ceiling)` (the sha256 in hex; a per-process partial file, removed on each refusal), `partial`, `Refusal`, `refusal(&failure)` |
| `qmp` | `Monitor` (`connect`, `call`, `keys`, `quit`), `send_key`, `is_event`, `response_ok`, `key(char)` (the `send-key` name and shift for a US layout), `HOLD_MS` |
| `modloop` | `modules(reader, kernel_release, names) -> ModuleSet` (`release`, `modules`: each `Module { path, name, bytes }` in load order, `builtin`, `dep`, `size()`), `Fault`, `fault(&failure)`, `MAX_SET`, `RELEASE_DIRS` |
| `squashfs` | `Image::open(reader)` (`superblock`, `root`, `inode(reference)`, `entries(&dir)`, `lookup(path)`, `file_at(path)`, `read(&file)`, `ids`, `export(number)`), `Superblock::parse`, `Compressor`, `Inode`, `Kind`, `Dir`, `File`, `Entry`, `listing(bytes)`, `Fault`, `fault(&failure)`, the ceilings `MAX_FILE`, `MAX_LISTING`, `MAX_LINK` |

Each failure is a `pod_core::Failure` at `Stage::Host`. A failure of I/O
keeps its cause, with the text `PATH: CAUSE`. The classes: `Timeout` (a run
at its limit, with the cause `Timeout`; a QMP socket that did not appear),
`Policy` (a ceiling of the store, with the cause `Refusal`), `Protocol` (a
digest that is not the pinned one, a result that does not parse or has
another token, a guest that powered off with no result, a QMP refusal),
`Usage` (a name that is not 8.3, an OEM name that does not fit, a character
with no key, an empty argv), `Unavailable` (a tool that failed, a full
volume, no random bytes, a closed QMP connection), and the class of the I/O
kind.

The FAT16 reader does not trust the volume that the guest wrote: a cluster
outside the volume, a loop in a chain, or a size that the chain does not
hold reads as no file.

## The modloop

A distribution ships the modules of its kernel as a squashfs image beside
the kernel (Alpine: `boot/modloop-virt`, xz, blocks of 128 KiB). When
`squashfs.ko` is in that image only, the guest cannot mount it. The host
calls `modloop::modules(File::open(path)?, "6.12.115-0-virt", ["tun",
"wireguard"])` and puts the set into the initramfs or a second disk of the
guest: each `Module` under its `path`, and `dep` as `modules.dep`.

* The set: each name, and each dependency of `modules.dep`, each one time,
  in load order (each dependency before its dependents; the dependencies
  of a line from its end, as modprobe loads them). `-` and `_` are the
  same in a name, as in kmod. A name of `modules.builtin` goes into
  `builtin` and needs no file. A name that is neither is
  `Fault::Unknown(name)`, class `NotFound`. A dependency loop, a line with
  no colon, a second line of one module, a path that is absolute or has
  `.` or `..`, and a file of the release that is not UTF-8 text are faults.
  The walk keeps a stack of its own.
* The release: `modules/RELEASE` (a modloop), else `lib/modules/RELEASE`
  (a root file system). Each module file is copied as it is: `.ko`, or `.ko.gz`,
  `.ko.xz`, `.ko.zst` for the guest's kmod.
* The input is a `Read + Seek`. The reader reads the superblock, the
  tables that a call needs, and the blocks of `modules.dep`,
  `modules.builtin` and of each module, not the whole image (2.2 MB of
  modules from the 21 MB modloop of Alpine 3.22.8).

The squashfs reader is for the read path only. It takes squashfs 4.0
with xz (`xz4rust`, Rust with no unsafe code): LZMA2, with or without a
BCJ filter (x86, which mksquashfs puts on some blocks of executable code;
also ARM, ARM64, ARM-Thumb, PowerPC, IA-64, SPARC and RISC-V). Each stream is one block, with a crc32 check, and decodes
to the length that its table gives; bytes after its end are a fault, as in
the kernel. The dictionary is the one of the compressor options (2^n or
3 × 2^n, at most the block size), else the block size: a stream that asks
for more is a fault, not an allocation. A stored block needs no decoder,
so an image of another compressor reads as far as its blocks are stored.
Links are not followed; xattrs are not read.

The reader trusts nothing in the image: each offset and each size is
checked against `bytes_used`, a name of a listing is one part of a path,
and a ceiling holds before each allocation (a file 64 MiB, a listing 16
MiB, a link 4 KiB, the cache of metadata 8 MiB, a set 256 MiB). Each
failure is a `pod_core::Failure` with a cause `squashfs::Fault` or
`modloop::Fault`: `NotFound` for a path, a release or a name that is not
there, `Unsupported` for a version or a compressor, `Policy` for a
ceiling, `Usage` for a release that is not one part of a path, `Protocol`
for the rest.

## Parity

Against podvm `crates/podvm-windows` at `6cd3442`:

| Capability of podvm | Here |
| --- | --- |
| [x] `Plan`, `Accel`, `accel_for`, `argv`, `base_format`, `overlay_argv`, `commit_argv` | `plan`, the same; `commit_argv` is `convert_argv` (T-266) |
| [x] `Request`, `Outcome`, `mailbox`, `outcome`, `stage`, `run`, `stop`, `provision` | the same, with `&Settings` first where a name or a place is used |
| [x] `Stop` and its text; `Error::Timeout(d, stop)` | `Timeout { after, stop }` in the chain of a `Failure` (`timeout_of`); the text is the same |
| [x] `Error::{NoEmulator, Io, BadResult}` | `Failure` with the same text (`NoAcceleration` was not used) |
| [x] The per-run directory, `scratch_of`, `cleanup`, `RunGuard`, `SCRATCH_PREFIX` | `Settings::{scratch_root, scratch_prefix}`; `RunGuard` keeps a copy of the settings |
| [x] `PODVM_WINDOWS_KEEP`, `XDG_RUNTIME_DIR` | `Settings::keep`, `Settings::scratch_root` (podvm reads the variables) |
| [x] The marker `podvm` (`podvm-dos` for DOS), the OEM name `PODVM` | `Settings::{marker, oem}` |
| [x] `base_cache`, `dos::base_cache` (`PODVM_WINDOWS_BASE`, `PODVM_DOS_BASE`, `~/.local/share/podvm/windows/`) | stay in podvm |
| [x] `cap_status` (docker's 0 to 124) | stays in podvm |
| [x] The emulator in a process group; QMP `quit`, `SIGKILL` to the group, a bounded reap; the error stream in a file | `emulator` (public now) |
| [x] `agent` (the scripts, the names, `command_file`, `command_from_argv`, `quote`, `parse_code`, `nonce`) | the same; `nonce` gives a `Result` and takes `pod_core::entropy` |
| [x] `dos` (the machine line, the stage, the boot keys, the typed lines, the poll) | the same; a line with a character that has no key is refused before the first key (podvm typed the part before it) |
| [x] `fat16` | the same; `put` gives a `Failure`; the OEM name is a setting; a chain that leaves the volume, loops or does not hold its size reads as `None` (podvm read past the image and panicked) |
| [x] `fetch` (`Ceiling`, `check_declared`, `store`, `FetchError`) | the same; `FetchError` is the cause `Refusal` of a `Failure` |
| [x] `qmp` | the same; `keys` of `lib.rs` is `qmp::key` |
| [x] The tests: 60 unit tests | 58 with their names; the tests of `base_cache` and `cap_status` stay in podvm; 7 new |

## Adoption map (podvm)

| Old path | New path |
| --- | --- |
| `podvm_windows::{Plan, Accel, accel_for, argv, commit_argv (now convert_argv), overlay_argv, Request, Outcome, Stop, Fat16, Ceiling, Code, outcome, stop, scratch_of}` | `pod_guest::*`, re-exported by `podvm_windows` |
| `podvm_windows::{agent, fat16, fetch, plan, qmp}` | `pod_guest::{agent, fat16, fetch, plan, qmp}`, re-exported |
| `podvm_windows::{stage, run, provision, cleanup, scratch_dir, mailbox}(...)` | `pod_guest::*(&settings, ...)`; podvm keeps its functions as calls with `podvm_windows::settings()` |
| `podvm_windows::RunGuard::arm(&plan)` | `pod_guest::RunGuard::arm(&settings, &plan)`; podvm keeps `RunGuard::arm(&plan)` |
| `podvm_windows::SCRATCH_PREFIX`, `PODVM_WINDOWS_KEEP`, `XDG_RUNTIME_DIR` | `Settings::{scratch_prefix, keep, scratch_root}`, podvm's values in `settings()` |
| the marker `podvm`, the OEM name `PODVM` | `Settings::{marker, oem}`; `podvm_windows::dos::stage` gives the marker `podvm-dos` |
| `podvm_windows::Error`, `Error::Timeout(d, stop)` | `pod_core::Failure`; `podvm_windows::timeout_of(&e)` gives `Timeout { after, stop }` |
| `podvm_windows::FetchError` | `pod_guest::fetch::Refusal`, the cause of the `Failure` (`fetch::refusal`) |
| `podvm_windows::agent::nonce() -> String` | `-> Result<String>`; podvm reads a failure as a runtime error |
| `podvm_windows::keys` (private) | `pod_guest::qmp::key` |
| `podvm_windows::{base_cache, cap_status}`, `podvm_windows::dos::base_cache` | stay in podvm |
| `podvm_windows::dos::{argv, run, batch_from_argv, DEFAULT_MEM_MIB, CPUS, ...}` | `pod_guest::dos::*`, re-exported by `podvm_windows::dos` |
| `crates/podvm-windows/src/{agent,dos,emulator,fat16,fat16/tests,fetch,plan,provision,qmp,scratch}.rs` | leave podvm |

## Tests

`cargo test -p pod-guest`. The unit tests use a temporary directory each.
The tests of the agent walk `AGENT_CMD` line by line over a mailbox of
names and texts (`src/agent/tests.rs`): the order of the marker, the
command, the result and the shutdown, the keep-alive loop over three
tokens, and the 600 tries with no mailbox. A line that the walk does not
know fails the test.
The tests with a fake emulator run `/bin/sh` and skip with a line
`skipping, REASON` where it is missing. A stop test reads a script with
`/bin/sh`, so that no test executes a file that a test thread just wrote
(`ETXTBSY`). `a_real_emulator_answers_qmp_and_leaves_no_process` starts
`qemu-system-x86_64 -M none -accel tcg` (and `-accel kvm` where `/dev/kvm` opens) with a bound on each wait, checks
the QMP handshake and a request, stops it and checks that no emulator
stays; they skip as the text above says. `guest.yml` runs them on a runner with
QEMU, each week and by hand (`docs/design.md`, section 9).

The tests of the modloop build their images with the test writer of
`src/squashfs/writer.rs` (stored metadata in blocks of a chosen size, so
that an inode or a listing crosses blocks; stored, sparse and xz blocks;
fragments; an export table). Its xz blocks come from the fixtures of
`tests/fixtures/squashfs/`, which `make.py` writes with Python's lzma:
4 KiB of x86 code with the x86 BCJ filter, and 1,000 bytes with LZMA2
only. Each malformed image is a test of its fault, and 3,000 mutations of
an image read to a value or a fault. The ignored test
`write_the_fuzz_seeds` writes the seeds of the fuzz target
`squashfs_image`.

`tests/modloop_live.rs` (ignored; `live.yml` runs it each day) reads the
modloop of Alpine 3.22 that `POD_GUEST_MODLOOP` names: it walks each
directory and file, the id and export tables, counts the xz streams that
it decoded and those with the x86 filter, and takes the module set of
`nf_tables nft_nat sch_netem wireguard tun veth nf_nat`. On the
`modloop-virt` of Alpine 3.22.8 (2026-11-11): 1,016 streams, 8 with x86
BCJ, 939 files of 114,162,529 bytes; the set has 20 modules of 2,194,724
bytes for `6.12.115-0-virt`.

## Sources

`SOURCES` lists each imported file of podvm with its blob, and the
`check-sources.py` check compares them with podvm's `main`.
