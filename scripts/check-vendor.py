#!/usr/bin/env python3
"""Check that each fork of vendor/ is its upstream crate plus its patches.

  python3 -I scripts/check-vendor.py [--self-test]

FORKS gives each fork: the crate on crates.io, its version and the SHA-256
of the `.crate` file (the `cksum` of the crates.io index). The check gets
the `.crate` file from the cache of cargo or from static.crates.io, compares
its SHA-256, unpacks it, applies `vendor/patches/FORK/*.patch` in the order
of their names, and compares the result with `vendor/FORK` file by file. A
file that is different, missing or added is a fault. cargo's marker
`.cargo-ok` does not count.

--self-test plants a changed byte in a copy of each fork. The check must
then find the fault, and it must pass the control copy.

Exit 0: each fork agrees. Exit 1: a fault, one line for each. Exit 2: a
`.crate` file cannot be read.
"""

import hashlib
import io
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VENDOR = ROOT / "vendor"
# The fork: (crate, version, SHA-256 of the .crate file).
FORKS = {
    "h2": ("h2", "0.4.20", "7d29020232d6aa3fb1daca64c1127cf662cf97f254ae16c18c05b8ab635fc118"),
    "iroh-relay": ("iroh-relay", "1.4.2", "ffa03fd9a865148bd5d2f4a617232bcb77cf23956da50ef4f8204d6772f72c79"),
}
IGNORED = {".cargo-ok"}


class Unreadable(Exception):
    pass


def crate_bytes(crate: str, version: str, sha: str) -> bytes:
    """The `.crate` file, from the cache of cargo when it is there."""
    name = f"{crate}-{version}.crate"
    for cached in sorted(Path.home().glob(f".cargo/registry/cache/*/{name}")):
        body = cached.read_bytes()
        if hashlib.sha256(body).hexdigest() == sha:
            return body
    url = f"https://static.crates.io/crates/{crate}/{name}"
    try:
        with urllib.request.urlopen(url, timeout=60) as answer:
            body = answer.read()
    except OSError as e:
        raise Unreadable(f"{url}: {e}") from e
    got = hashlib.sha256(body).hexdigest()
    if got != sha:
        raise Unreadable(f"{url}: SHA-256 {got}, not {sha}")
    return body


def upstream_tree(fork: str, dest: Path) -> list[str]:
    """Unpack the crate of `fork` into `dest` and apply its patches. Return
    the faults of the patches."""
    crate, version, sha = FORKS[fork]
    body = crate_bytes(crate, version, sha)
    with tarfile.open(fileobj=io.BytesIO(body), mode="r:gz") as tar:
        tar.extractall(dest.parent, filter="data")
    (dest.parent / f"{crate}-{version}").rename(dest)
    faults = []
    for patch in sorted((VENDOR / "patches" / fork).glob("*.patch")):
        done = subprocess.run(["git", "apply", "--whitespace=nowarn", str(patch)], cwd=dest,
                              capture_output=True, text=True)
        if done.returncode != 0:
            faults.append(f"vendor/{fork}: {patch.name} does not apply: {done.stderr.strip()}")
    return faults


def files(tree: Path) -> dict[str, Path]:
    return {p.relative_to(tree).as_posix(): p for p in tree.rglob("*")
            if p.is_file() and p.name not in IGNORED}


def compare(fork: str, expected: Path, tree: Path) -> list[str]:
    want, have = files(expected), files(tree)
    faults = [f"vendor/{fork}/{name}: missing" for name in sorted(want.keys() - have.keys())]
    faults += [f"vendor/{fork}/{name}: not in the upstream crate or its patches"
               for name in sorted(have.keys() - want.keys())]
    faults += [f"vendor/{fork}/{name}: differs from the upstream crate plus its patches"
               for name in sorted(want.keys() & have.keys()) if want[name].read_bytes() != have[name].read_bytes()]
    return faults


def check(trees: dict[str, Path], work: Path) -> list[str]:
    faults = []
    for fork in FORKS:
        expected = work / "upstream" / fork
        if not expected.exists():
            expected.parent.mkdir(parents=True, exist_ok=True)
            faults += upstream_tree(fork, expected)
        faults += compare(fork, expected, trees[fork])
    return faults


def self_test(work: Path) -> int:
    control = check({fork: VENDOR / fork for fork in FORKS}, work)
    if control:
        print("check-vendor: the control tree fails:", *control, sep="\n  ")
        return 1
    rc = 0
    for fork in FORKS:
        copy = work / "planted" / fork
        shutil.copytree(VENDOR / fork, copy)
        victim = copy / "Cargo.toml"
        victim.write_bytes(victim.read_bytes() + b"\n")
        trees = {f: VENDOR / f for f in FORKS}
        trees[fork] = copy
        found = [f for f in check(trees, work) if f.startswith(f"vendor/{fork}/Cargo.toml")]
        print(f"{'ok' if found else 'MISSED'} planted byte in vendor/{fork}/Cargo.toml")
        rc |= 0 if found else 1
    return rc


def main(argv: list[str]) -> int:
    with tempfile.TemporaryDirectory(prefix="check-vendor-") as tmp:
        try:
            if argv == ["--self-test"]:
                return self_test(Path(tmp))
            if argv:
                print(__doc__.strip())
                return 2
            faults = check({fork: VENDOR / fork for fork in FORKS}, Path(tmp))
        except Unreadable as e:
            print(f"check-vendor: {e}")
            return 2
    for fault in faults:
        print(fault)
    if faults:
        return 1
    print(f"check-vendor: {len(FORKS)} forks agree with their upstream crates plus their patches")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
