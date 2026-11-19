#!/usr/bin/env python3
"""Check the strings of a release binary for the text rules of a command
line (the owner's ruling "The text of a command line"; pod-manual, T-700).

  python3 -I scripts/check-strings.py [--plant] [--data DIR]... BINARY...
  python3 -I scripts/check-strings.py --self-test

The check reads the section `.rodata` of each ELF file: the section where
the compiler puts the string literals. It takes each run of text there: a
run of printable ASCII, tabs, newlines and valid UTF-8 sequences, at least
MIN characters long. It fails on:

* an emoji or a pictograph, or a bidirectional control, where the run reads
  as text around it (TEXT: a word of three letters, and half of the
  characters are ASCII letters or spaces): a run of binary bytes can hold
  a valid UTF-8 sequence of a pictograph;
* a work id with prose around it: a word of letters just before or after
  it, and PROSE (three words of letters) near it. A work id is T-NNN,
  COORD-NNN, Q-AN, CNNN or QNN as a word, #NNN as a word or after a
  lower-case name (podnet#584). `#NNN` after a name in capitals and a
  space, in parentheses or not, is the number of a standard (`PKCS #12`,
  `(PKCS) #1`), not an issue.

The data that pod-services compiles in (the values of its files, with the
sources of each fact) names the issues and the rulings that each value
comes from, by design. A run with a fault is checked again with each value
of that data removed: a fault in the data goes, a fault in the text around
it stays. --data DIR names a directory of service files (default: the
`services/` of pod-services next to this script); give the directory of the
same tag as the binary.

What it does not check, and why: a byte that is not UTF-8 and a control
character are everywhere in the binary data of the section; an escape
sequence of a terminal is in the colour table of a library (anstyle of
clap), which writes it only on a terminal; the length of a sentence cannot
be read, since the compiler joins the literals with no separator. The tests
of pod-manual check those over the model and the literals of the program.

--plant writes a copy of each binary with a planted text in its `.rodata`
(an emoji and a work id) and checks the copy: the run must then exit 1.
--self-test runs the rules over planted texts: each fault must be found,
and each control must pass.

Exit 0: no fault. Exit 1: a fault, one line for each. Exit 2: a file that
is not ELF, or that cannot be read.
"""

import re
import struct
import sys
import tempfile
import tomllib
from pathlib import Path

MIN = 6
# The services of pod-services, next to this script.
DATA = Path(__file__).resolve().parent.parent / "crates" / "pod-services" / "services"
# A value of the data shorter than this is not removed from a run: a short
# word of the data must not hide the text around it.
DATA_MIN = 8
# The characters on each side of a fault that tell text from binary data.
NEAR = 24
PROSE = re.compile(r"(?:(?:^|\s)[A-Za-z]{2,}[.,;:]?(?=\s|$).*?){3}", re.S)
WORD = re.compile(r"[(\[]?[A-Za-z]{2,}[.,;:)\]]?")
STANDARD = re.compile(r"\(?[A-Z]{2,}\)? $")
WORK_ID = re.compile(
    r"(?<![A-Za-z0-9_-])(T-\d+|COORD-\d+|Q-A\d+|[CQ]\d+|#\d+)(?![A-Za-z])"
    r"|(?<=[a-z])#\d+(?![A-Za-z])"
)
def emoji(c: int) -> bool:
    return (
        0x1F000 <= c <= 0x1FAFF or 0x2600 <= c <= 0x27BF or 0x2B00 <= c <= 0x2BFF or 0x2300 <= c <= 0x23FF
        or 0xFE00 <= c <= 0xFE0F or c in (0x200D, 0x20E3, 0x3030, 0x303D, 0x3297, 0x3299)
        or 0xE0020 <= c <= 0xE007F
    )


def bidi(c: int) -> bool:
    return 0x202A <= c <= 0x202E or 0x2066 <= c <= 0x2069 or c in (0x200E, 0x200F, 0x061C)


def rodata(data: bytes) -> tuple[int, int]:
    """The offset and the size of `.rodata` in an ELF file."""
    if data[:4] != b"\x7fELF":
        raise ValueError("not an ELF file")
    wide, order = data[4] == 2, "<" if data[5] == 1 else ">"
    if wide:
        shoff, = struct.unpack_from(order + "Q", data, 0x28)
        shentsize, shnum, shstrndx = struct.unpack_from(order + "HHH", data, 0x3A)
    else:
        shoff, = struct.unpack_from(order + "I", data, 0x20)
        shentsize, shnum, shstrndx = struct.unpack_from(order + "HHH", data, 0x2E)

    def section(i):
        at = shoff + i * shentsize
        if wide:
            name, _, _, _, off, size = struct.unpack_from(order + "IIQQQQ", data, at)
        else:
            name, _, _, _, off, size = struct.unpack_from(order + "IIIIII", data, at)
        return name, off, size

    _, stroff, _ = section(shstrndx)
    for i in range(shnum):
        name, off, size = section(i)
        end = data.index(b"\0", stroff + name)
        if data[stroff + name:end] == b".rodata":
            return off, size
    raise ValueError("no .rodata section")


def runs(blob: bytes):
    """Each run of text in `blob`: (offset, text)."""
    i, n = 0, len(blob)
    start, chars = 0, []
    while i <= n:
        b = blob[i] if i < n else None
        step, ch = 1, None
        if b is not None and (0x20 <= b < 0x7F or b in (0x09, 0x0A)):
            ch = chr(b)
        elif b is not None and b >= 0xC2:
            size = 2 if b < 0xE0 else 3 if b < 0xF0 else 4 if b < 0xF5 else 0
            try:
                ch = blob[i:i + size].decode("utf-8") if size else None
                step = size
            except UnicodeDecodeError:
                ch = None
        if ch is None:
            if len(chars) >= MIN:
                yield start, "".join(chars)
            chars, start = [], i + 1
            i += 1
            continue
        if not chars:
            start = i
        chars.append(ch)
        i += step


def reads_as_text(near: str) -> bool:
    """TEXT: a word of three letters, and half of the characters are ASCII
    letters or spaces."""
    letters = sum(1 for c in near if c == " " or (c.isascii() and c.isalpha()))
    return re.search(r"[A-Za-z]{3,}", near) is not None and 2 * letters >= len(near)


def in_prose(text: str, m: re.Match) -> bool:
    """A word of letters just before or just after the token of the id,
    and PROSE near it."""
    left, right = text[:m.start()], text[m.end():]
    before = left[:len(left) - len(re.search(r"\S*$", left).group())].split()
    after = right[len(re.match(r"\S*", right).group()):].split()
    beside = (before and WORD.fullmatch(before[-1])) or (after and WORD.fullmatch(after[0]))
    near = text[max(0, m.start() - 40):m.end() + 40]
    return bool(beside) and PROSE.search(near) is not None


def text_faults(text: str) -> list[str]:
    """The faults of one run of text."""
    out = []
    for i, c in enumerate(text):
        what = "emoji or pictograph" if emoji(ord(c)) else "bidirectional control" if bidi(ord(c)) else None
        if what and reads_as_text(text[max(0, i - NEAR):i] + " " + text[i + 1:i + 1 + NEAR]):
            out.append(f"the {what} U+{ord(c):04X} in {text[max(0, i - 40):i + 40]!r}")
            break
    for m in WORK_ID.finditer(text):
        if m.group().startswith("#") and STANDARD.search(text[:m.start()]):
            continue
        if in_prose(text, m):
            out.append(f"the work id {m.group()} in {text[max(0, m.start() - 40):m.end() + 40]!r}")
    return out


def run_faults(text: str, data: list[str]) -> list[str]:
    """The faults of a run, with the values of the data removed: the
    longest first, each one in place of a NUL, which no text holds."""
    found = text_faults(text)
    if not found:
        return found
    for value in data:
        if value in text:
            text = text.replace(value, "\0")
    return text_faults(text)


def data_values(dirs: list[Path]) -> list[str]:
    """Each text value of the service files in `dirs`, and each file that
    a value names, the longest first."""
    values = set()

    def walk(v):
        if isinstance(v, str):
            values.add(v)
        elif isinstance(v, dict):
            for x in v.values():
                walk(x)
        elif isinstance(v, list):
            for x in v:
                walk(x)

    for d in dirs:
        for f in sorted(d.rglob("*")):
            if f.suffix == ".toml":
                walk(tomllib.loads(f.read_text(encoding="utf-8")))
            elif f.is_file():
                values.add(f.read_text(encoding="utf-8", errors="replace"))
    return sorted((v for v in values if len(v) >= DATA_MIN), key=len, reverse=True)


def faults(path: Path, data: bytes, values: list[str]) -> list[str]:
    off, size = rodata(data)
    blob = data[off:off + size]
    out = []
    for at, text in runs(blob):
        out += [f"{path}: .rodata+{at:#x}: {f}" for f in run_faults(text, values)]
    return out


def plant(data: bytes) -> bytes:
    """`data` with a planted text over the start of a long run of `.rodata`."""
    off, size = rodata(data)
    planted = "the entry T-700 is here \U0001F680 now ".encode("utf-8")
    for at, text in runs(data[off:off + size]):
        if len(text) >= len(planted) and text.isascii():
            at += off
            return data[:at] + planted + data[at + len(planted):]
    raise ValueError("no run is long enough for the plant")


def self_test() -> int:
    """Each planted fault must be found, and each control must pass."""
    data = ["PodUtils/podssh#158: after ppng.io", "the ruling Q78 of podssh, a last fallback"]
    cases = [
        ("a work id in prose", "the entry T-700 is here now", 1),
        ("an issue in prose", "See the issue (#181) for the plan of it.", 1),
        ("an issue after a word", "the fix of issue #12 is here now", 1),
        ("an issue of another repository", "the rules of PodUtils/podssh#177 hold here", 1),
        ("an emoji in prose", "Run it now \U0001F680 and wait for the end", 1),
        ("a bidirectional control in prose", "the name \u202Eof the file is here", 1),
        ("a work id beside the data", "PodUtils/podssh#158: after ppng.iothe entry T-123 is open now", 1),
        ("an emoji beside the data", "the ruling Q78 of podssh, a last fallback and \U0001F680 now go", 1),
        ("the data", "streams-request-bodyPodUtils/podssh#158: after ppng.ioonrender-piping", 0),
        ("the data twice", "the ruling Q78 of podssh, a last fallbackPodUtils/podssh#158: after ppng.io", 0),
        ("a standard number", "challengePasswordPKCS #9 challenge password (as specified for PKCS #12)", 0),
        ("a standard in parentheses", "Public-Key Cryptography Standard (PKCS) #12DSA signature generated", 0),
        ("binary bytes", "f\u2648 \\sf\u06d8", 0),
        ("a work id with no prose", "x86_64-T-123abc/C99:#12;Q4", 0),
        ("a work id with no word beside it", "the values 0x1f, T-700, 0x2e of the table here", 0),
    ]
    failed = 0
    for name, text, want in cases:
        have = len(run_faults(text, data))
        if (have > 0) != (want > 0):
            print(f"check-strings: self-test: FAIL {name}: {have} faults, want {want}")
            failed += 1
    if failed:
        return 1
    print(f"check-strings: self-test: {len(cases)} cases hold")
    return 0


def main(argv: list[str]) -> int:
    if argv == ["--self-test"]:
        return self_test()
    planted = "--plant" in argv
    dirs, paths, rest = [], [], [a for a in argv if a != "--plant"]
    while rest:
        a = rest.pop(0)
        if a == "--data" and rest:
            dirs.append(Path(rest.pop(0)))
        else:
            paths.append(Path(a))
    if not paths:
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        return 2
    try:
        values = data_values(dirs or ([DATA] if DATA.is_dir() else []))
    except (OSError, tomllib.TOMLDecodeError) as e:
        print(f"check-strings: the data: {e}", file=sys.stderr)
        return 2
    found = []
    for path in paths:
        try:
            data = path.read_bytes()
            if planted:
                with tempfile.NamedTemporaryFile(suffix=".planted", delete=False) as f:
                    f.write(plant(data))
                copy = Path(f.name)
                try:
                    found += faults(copy, copy.read_bytes(), values)
                finally:
                    copy.unlink()
            else:
                found += faults(path, data, values)
        except (OSError, ValueError, struct.error) as e:
            print(f"check-strings: {path}: {e}", file=sys.stderr)
            return 2
    for line in found:
        print(line)
    if not found:
        print(f"check-strings: {len(paths)} files, no fault")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
