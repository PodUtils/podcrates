# pod-udf

A reader of UDF that finds a file in an image and gives the byte ranges of
the file in the image, in order, and its size. A client then fetches only
those ranges, for example by HTTP Range requests: a Windows ISO of 2.5 GB
holds `ValidationOS.vhdx` of about 0.9 GB, and podvm fetches only that
file (PodUtils/podvm#350, item 2; PodUtils/podvm@T-1863).

The crate reads ECMA-167 (3rd edition) with UDF 1.02 to 2.60, as the ISO
images of Windows use them, with the metadata partition of UDF 2.50 and
later where one is present. T-591 wrote it from the specifications; it
has no code of another reader. It does no I/O of its own: the caller gives
a `ReadAt`, which reads bytes at an offset. It has no dependency, no
runtime and no feature.

## Use

```rust
use pod_udf::{Extent, Seeker, Udf};

let mut udf = Udf::open(Seeker(std::fs::File::open("windows.iso")?))?;
let extents = udf.find("/sources/boot.wim")?;
for part in &extents.parts {
    match part {
        Extent::Data { offset, len } => fetch(*offset, *len),  // Range: bytes=offset-(offset+len-1)
        Extent::Zero { len } => zeros(*len),                   // a hole: no bytes in the image
    }
}
```

A client of Range requests implements `ReadAt` itself: each call is one
range of the image. An open reads about ten blocks; a walk reads, for each
component of the path, the file entry and the bytes of the directory.

## API

| Item | What |
| --- | --- |
| `ReadAt` | `read_at(offset, buf)`: fill `buf` from `offset` of the image; a short image is `UnexpectedEof`. Implemented for `&[u8]`, `Vec<u8>` and `&mut T` |
| `Seeker(R)` | a `ReadAt` over `Read + Seek`, such as a `File` |
| `Udf::open(reader)`, `Udf::open_with(reader, limits)` | the anchor, the volume descriptors, the partition maps (with the metadata file), the file set |
| `Udf::find(path) -> Extents` | the extents of a file; the components split at `/` or `\` |
| `Udf::list(path) -> Vec<Entry>` | the entries of a directory: `name`, `directory` |
| `Udf::read(&extents, max) -> Vec<u8>` | the bytes of a file of at most `max` bytes |
| `Udf::block_size`, `revision`, `limits`, `into_inner` | the block size (2048 for an ISO), the UDF revision of the domain as BCD (0x0102, 0x0250), the limits, the reader |
| `find(reader, path)` | `Udf::open` and `find` in one call |
| `Extents` | `size`, `parts` (`Extent::Data { offset, len }`, `Extent::Zero { len }`; adjacent parts are one part); `range()`: the one range of a file of one recorded part |
| `Limits` | `depth` (64), `entries` (65,536 for each directory), `directory_bytes` (16 MiB), `extents` (65,536 allocation descriptors for each file), `descriptors` (1,024 volume descriptors) |
| `Error`, `TagFault` | `Read` (the reader, with its offset), `NotUdf`, `Descriptor` (a bad tag: identifier, checksum, version, location, CRC), `Corrupt`, `Unsupported`, `Missing`, `NotFound`, `NotADirectory`, `NotAFile`, `Path`, `Limit` |

## The rules

* **The anchor.** The anchor volume descriptor pointer at block 256, with
  a block of 2048, 512 or 4096 bytes, in this order. The logical block
  size of the volume must be the block size of the anchor.
* **The descriptors.** Each tag is checked: the checksum, the version (2
  or 3), the CRC of the descriptor, and the location (the block that the
  descriptor is in; a file identifier descriptor has no location check).
  The main volume descriptor sequence is read first; when it fails, or
  has no partition descriptor or no logical volume descriptor, the
  reserve sequence stands in. A pointer to a next extent is followed. Of
  two descriptors, the one with the higher sequence number wins.
* **The partitions.** Partition maps of type 1 and the metadata partition
  map of UDF 2.50 and later. The blocks of a metadata partition are the
  bytes of the metadata file; when the metadata file is bad, its mirror
  stands in. A virtual or a sparable partition map is `Unsupported` (a
  write-once or a rewritable medium; an ISO has neither).
* **The files.** File entries and extended file entries of strategy 4,
  with `short_ad`, `long_ad` or the data in the entry; an allocation
  extent descriptor continues a list. An `ext_ad` and strategy 4096 are
  `Unsupported`. An extent that is allocated and not recorded, or not
  allocated, is `Extent::Zero`. Allocation descriptors that end before
  the size of the file, or an extent past the end of its partition, are
  `Corrupt`.
* **The names.** OSTA compressed unicode: compression ID 8 and 254 (one
  byte for each character), 16 and 255 (UTF-16, big end first; an
  unpaired surrogate becomes U+FFFD). A component of a path matches the
  same name first, else the first name that is the same with no regard to
  case, as Windows reads the media. Deleted entries and the parent entry
  are not listed. `..` in a path is `Path`.
* **The file types.** `find` gives a file of type 5 (bytes) or 249 (real
  time); another type is `NotAFile` with its type.
* **No panic.** Each field is read with a bound check, and the limits
  bound each loop and each allocation. The fuzz target `udf_image`
  (`fuzz/`) runs the reader on untrusted images.

## Tests

`cargo test -p pod-udf`: the images come from the writer of the tests
(`src/tests/writer.rs`, written from ECMA-167 and UDF 2.60; no `mkisofs`,
`genisoimage` or `xorriso` was on the host to make an image under
`tests/data`). `src/tests.rs`: a file in a subdirectory and its range in
the image, each revision from 1.02 to 2.60 with `short_ad` and `long_ad`,
a metadata partition whose file has two extents (40 files put entries in
both), a missing path, a path through a file and a directory as a file, a
file of two extents and a file with a hole, a file in its entry, names
with no regard to case and in UTF-16, the limits, an image with no anchor
and a reader that fails, a `Seeker`, the CRC, and the sparse records of
the fuzz seeds. `src/tests/faults.rs`: a changed byte (CRC), a changed tag
checksum, a descriptor at another block, the reserve sequence for a bad
main one, the mirror for a bad metadata file, allocation extent
descriptors (a list continued, a loop to the limit, an extent past its
partition, descriptors shorter than the file), and the unsupported
structures. `write_the_fuzz_seeds` (ignored) writes the seeds of
`fuzz/seeds/udf_image`.

The plants `udf-crc-unchecked`, `udf-second-extent-dropped`,
`udf-metadata-first-extent-only`, `udf-reserve-sequence-unused`,
`udf-allocation-extent-not-followed` and `udf-depth-unbounded`
(`scripts/plants/`) prove the tests.

## Adoption map

| Consumer | Old | New |
| --- | --- | --- |
| podvm | none (PodUtils/podvm@T-1863; else winquick's `udf.rs` under `vendor/`) | `pod-udf = { git = "https://github.com/PodUtils/podcrates", tag = "vX.Y.Z" }`; a `ReadAt` over its Range client, then `Udf::find("/sources/...")` |
