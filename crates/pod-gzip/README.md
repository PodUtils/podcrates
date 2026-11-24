# pod-gzip

A writer of gzip members. `Members` cuts the stream into blocks of 2 MiB
(`BLOCK`) and writes each block as one gzip member (RFC 1952, section 2.2).
A fixed pool of threads compresses the blocks while the caller writes the
next ones, and the caller's thread writes the members to the inner writer
in their order. The concatenation is one gzip file: `gzip -d`, `tar -z` and
`flate2::read::MultiGzDecoder` read it.

The bytes do not depend on the count of threads or on the sizes of the
writes, so the SHA-256 of the stream is stable for one input, one level and
one version of the backend. The crate has no pod dependency, prints
nothing, and uses std threads with the Rust backend of `flate2`
(`miniz_oxide`): no tokio, no C.

The base is podvm's writer (PodUtils/podvm#657, ask 2; `SOURCES`). The
pool replaces podvm's batch: there, the caller filled a batch of blocks
and then waited while one thread for each block compressed it, so the
caller and the threads took turns.

## Use

```rust
use std::io::Write;

let mut gz = pod_gzip::Members::new(upload, pod_gzip::host_threads());
tar.write_into(&mut gz)?;
let upload = gz.finish()?; // the last member, then a flush of `upload`
```

## API

| Item | What |
| --- | --- |
| `Members::new(inner, threads) -> Members<W>` | a writer with `threads` threads (clamped to 1 to `MAX_THREADS`) at level `FAST` |
| `Members::with_level(inner, threads, level)` | the same at `level` (0 to 9; a higher value is 9) |
| `impl Write for Members<W>` | `write` takes at most the rest of the current block; a full block goes to the pool; `flush` writes nothing (a member ends only at the end of its block) and gives an earlier error |
| `Members::finish(self) -> io::Result<W>` | the last block, each member in order, a flush of `inner`, then the threads stop; a stream with no byte is one empty member (20 bytes) |
| `Members::limit(&self) -> usize` | the most blocks in flight (given to the pool, with no member written to `inner`): `threads` + 1 |
| `Members::in_flight(&self) -> usize` | the blocks in flight now |
| `BLOCK` | 2 MiB: the input bytes of each member except the last |
| `MAX_THREADS` | 8 |
| `FAST` | 1: the level of `new` (`gzip -1`) |
| `host_threads() -> usize` | the parallelism of the host, at most `MAX_THREADS`, 1 where the kernel does not say |

## The rules

* **The same bytes.** Each block is cut at the same offset of the input.
  The header of each member is fixed: no name, MTIME 0, the XFL that
  `flate2` gives for the level (4 at level 1), OS 255. `tests/golden.rs`
  holds the SHA-256 of one input at level 1 and level 6. A change of
  `flate2` or `miniz_oxide` that changes a byte fails it: the commit of the
  change writes the new values, as for the wire vectors.
* **Bounded memory.** At most `limit()` blocks are in flight, and one more
  fills: at most `threads` + 2 blocks of input and their members are in
  memory. A write that would give one more block waits for the member of
  the oldest block and writes it. A slow inner writer or a slow pool makes
  the producer wait.
* **The producer does not wait for a batch.** A write gives its block to
  the queue and returns; it writes each member that is ready, in order.
* **Errors.** The first error of the inner writer or of a thread comes out
  of the write or of `finish` that meets it, with its `io::ErrorKind`; the
  threads stop, and each later call gives the same kind again. A panic in
  a thread is an error of its block, not a hang.
* **No thread outlives the value.** Drop closes the queue and joins each
  thread; a thread ends after its block.

## Settings

The count of threads and the level, as arguments. The crate reads no
variable and keeps no global state.

## What stays in podvm

`Upload`, `wanted` and `ROOM_TIMES` of
`PodUtils/podvm:crates/podvm/src/incus/gzip.rs`: the choice of a plain or a
gzip upload by the free space of incusd is the policy of podvm.

## Adoption map

### podvm (PodUtils/podvm#657, ask 2; PodUtils/podvm@T-2085)

| podvm | pod-gzip |
| --- | --- |
| `incus::gzip::Members::new(inner, threads)` | `pod_gzip::Members::new(inner, threads)` |
| `incus::gzip::Members::finish` | `pod_gzip::Members::finish` (also stops the threads) |
| `incus::gzip::BLOCK`, `MAX_THREADS` | `pod_gzip::BLOCK`, `pod_gzip::MAX_THREADS` |
| `incus::gzip::host_threads()` | `pod_gzip::host_threads()` |
| `Compression::fast()` | `pod_gzip::FAST` (`Members::with_level` for another level) |
| `Upload::Gzip(Members<W>)` | `Upload::Gzip(pod_gzip::Members<W>)`; `Upload` and `wanted` stay |
| the tests of the writer in `incus/import_tests.rs` | `tests/members.rs` (the same names) |

The bytes are the same as the bytes of podvm's writer at `cd51868e` for
the same input (the same blocks, the same header, the same level), so the
fingerprint of an image that podvm uploaded does not change.

## Tests

`cargo test -p pod-gzip --all-features`:

* `tests/members.rs`: each size (0, 1, 4096, `BLOCK` - 1, `BLOCK`,
  `BLOCK` + 1, three blocks and 5 bytes) at 1, 2, 3, 8 and 64 threads gives
  the input back through `MultiGzDecoder` with the same bytes; the split of
  the writes (1 byte, 4097 bytes, `BLOCK`) at 1, 2, 4 and 8 threads changes
  no byte; each block is one member with the fixed header; an empty stream
  is one empty member; `gzip -dc` of the host reads the stream back (a skip
  where the host has no `gzip`); a writer that fails (EPIPE) gives its
  error; the members reach the inner writer while the stream goes on.
* `tests/golden.rs`: the SHA-256 of one input at level 1 and level 6, at
  1 and 8 threads.
* The unit tests (`src/tests.rs`), with a compressor that a test holds:
  the producer writes while the threads compress; a fast producer waits at
  the limit; a panic of a thread is an error, not a hang; an error of the
  compressor comes out; no thread outlives the writer; a write after an
  error gives the error again.

The plant `scripts/plants/gzip-queue-unbounded.patch` removes the wait at
the limit and the bound of the queue: `a_fast_producer_waits_at_the_limit`
fails.
