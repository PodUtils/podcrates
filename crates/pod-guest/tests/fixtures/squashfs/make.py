#!/usr/bin/env python3
"""Write the xz fixtures of the squashfs tests of pod-guest (T-206).

  python3 -I crates/pod-guest/tests/fixtures/squashfs/make.py

The test writer of `src/squashfs/writer.rs` has no xz encoder, so the xz
blocks come from Python's lzma (liblzma), as mksquashfs makes them: one xz
stream for each block, with a crc32 check.

* x86.bin: 4096 bytes of x86 code with near calls and jumps (E8, E9), one
  block of 4 KiB. x86.bcj.xz: the stream with the x86 BCJ filter, then
  LZMA2. The script checks that the filter changes the bytes, so a decoder
  that skips it gives other bytes.
* tail.bin: 1000 bytes of text. tail.xz: the stream with LZMA2 only.

The dictionary is 8 KiB: the default of the reader for a block of 4 KiB with
no compressor options.
"""

import lzma
import os

HERE = os.path.dirname(os.path.abspath(__file__))
DICT = 8192


def x86_code() -> bytes:
    """Code-like bytes: a prologue, a call, a jump and an epilogue, with
    relative targets from a fixed sequence."""
    out = bytearray()
    state = 0x2545F491
    while len(out) < 4096:
        state = (state * 1103515245 + 12345) & 0x7FFFFFFF
        target = (state % 4000) - 2000
        out += b"\x55\x48\x89\xe5"
        out += b"\xe8" + target.to_bytes(4, "little", signed=True)
        out += b"\x48\x83\xc4\x08"
        out += b"\xe9" + (-target).to_bytes(4, "little", signed=True)
        out += b"\xc9\xc3"
    return bytes(out[:4096])


def tail_text() -> bytes:
    words = b"the module of a guest kernel comes from its modloop "
    return (words * 40)[:1000]


def xz(data: bytes, filters: list) -> bytes:
    return lzma.compress(data, format=lzma.FORMAT_XZ, check=lzma.CHECK_CRC32, filters=filters)


def write(name: str, data: bytes) -> None:
    with open(os.path.join(HERE, name), "wb") as f:
        f.write(data)


def main() -> None:
    code = x86_code()
    lzma2 = {"id": lzma.FILTER_LZMA2, "dict_size": DICT}
    bcj = [{"id": lzma.FILTER_X86}, lzma2]
    stream = xz(code, bcj)
    # The filter must change the bytes: LZMA2 alone on the raw form of the
    # same chain gives the code as the filter left it.
    raw = lzma.compress(code, format=lzma.FORMAT_RAW, filters=bcj)
    unfiltered = lzma.decompress(raw, format=lzma.FORMAT_RAW, filters=[lzma2])
    assert unfiltered != code, "the x86 filter changed nothing"
    assert lzma.decompress(stream) == code
    tail = tail_text()
    write("x86.bin", code)
    write("x86.bcj.xz", stream)
    write("tail.bin", tail)
    write("tail.xz", xz(tail, [lzma2]))


if __name__ == "__main__":
    main()
