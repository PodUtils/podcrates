#!/usr/bin/env python3
"""Check that product code replaces a file only through the one helper.

  python3 -I scripts/check-private-replace.py [--self-test]

A replace of a file by a temporary file and a rename that does not sync the
file and its directory can leave an empty file under the name after a power
cut (T-192). The helper `pod_core::files::replace_private` does each step.
The rule: no `fs::rename` call in the product code of a crate, except in the
helper and in the files that EXCEPTIONS lists with the reason.

The scan reads the files of `scripts/check-loopback.py` (each `.rs` file
under `crates/*/src`, but no test file and no `#[cfg(test)]` module), with
its code view: a rename in a comment or a string does not count. A scan that
reads fewer than MIN_FILES files is a fault.

--self-test runs the check on temporary trees with planted faults. It exits
0 only when the check finds each fault and passes the control tree.

Exit 0: no fault. Exit 1: a fault, one line for each.
"""

import importlib.util
import re
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MIN_FILES = 200

_spec = importlib.util.spec_from_file_location("check_loopback", Path(__file__).resolve().parent / "check-loopback.py")
loopback = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(loopback)

HELPER = "crates/pod-core/src/files/replace.rs"

# A file whose rename is not a replace of a private file, with the reason.
EXCEPTIONS: dict[str, str] = {
    "crates/pod-actions-results/src/blob.rs": "the part of an upload becomes its blob in the upload directory of the service",
    "crates/pod-actions-results/src/service.rs": "a finished upload moves into the store of the service; its record goes last",
    "crates/pod-actions-results/src/cache/service.rs": "a finished cache entry moves into the store of the service; its record goes last",
    "crates/pod-actions-store/src/lock.rs": "the lock file of a workflow repository, a public file of the user's tree",
    "crates/pod-actions-store/src/store.rs": "a tarball of the cache of actions, made again when it is lost",
    "crates/pod-extract/src/drive/deep.rs": "test code: a module of the tests of drive.rs that moves a directory",
    "crates/pod-guest/src/warm/state.rs": "a directory of a warm state, not a file",
    "crates/pod-guest/src/warm/line.rs": "the stream of a saved state that the emulator writes, too large for a body in memory, and its record: each synced, then moved, then the directory synced",
    "crates/pod-guest/src/fetch.rs": "a download streamed to its part file, synced, then moved: too large for a body in memory",
    "crates/pod-release/src/keep.rs": "the kept binary and a rollback: a hard link or a copy of a binary, not a private file; the directory is synced",
    "crates/pod-release/src/replace.rs": "a new binary over the running one: written, synced and tried before the rename; Windows moves the running file aside",
    "crates/pod-supervise/src/table.rs": "its temporary file is in the staging directory that its sweep cleans; it syncs the file and the directory",
}

RENAME = re.compile(r"\bfs::rename\s*\(")


def faults_in(text: str) -> list[int]:
    """The line of each rename call of product code."""
    _, code = loopback.product(text)
    return [code.count("\n", 0, m.start()) + 1 for m in RENAME.finditer(code)]


def check(root: Path, min_files: int = MIN_FILES) -> list[str]:
    files = loopback.scanned(root)
    faults = []
    if len(files) < min_files:
        faults.append(f"the scan read {len(files)} files, fewer than {min_files}")
    seen = set()
    for path in files:
        rel = path.relative_to(root).as_posix()
        lines = faults_in(path.read_text(encoding="utf-8", errors="replace"))
        if rel == HELPER:
            continue
        if rel in EXCEPTIONS:
            seen.add(rel)
            continue
        for line in lines:
            faults.append(f"{rel}:{line}: a rename outside pod_core::files::replace_private")
    if root == ROOT:
        for rel in sorted(set(EXCEPTIONS) - seen):
            faults.append(f"{rel}: an exception for a file that is not scanned; remove it")
    return faults


def self_test() -> int:
    def plant(root: Path, rel: str, text: str):
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def control(root: Path):
        plant(root, HELPER, "fn rename(a: &Path, b: &Path) { std::fs::rename(a, b) }\n")
        plant(root, "crates/a/src/lib.rs", (
            "// std::fs::rename(a, b) in a comment\n"
            'const WORDS: &str = "fs::rename(a, b)";\n'
            "fn f() { let _ = pod_core::files::replace_private(p, b, 0o600, false); }\n"
            "#[cfg(test)]\nmod tests {\n    fn t() { std::fs::rename(a, b).unwrap(); }\n}\n"
        ))
        plant(root, "crates/a/src/tests.rs", "fn t() { std::fs::rename(a, b).unwrap(); }\n")
        plant(root, "crates/pod-guest/src/fetch.rs", "fn f() { std::fs::rename(a, b) }\n")

    cases = {
        "control": (control, 0),
        "a rename in product code": (lambda r: (control(r), plant(r, "crates/b/src/lib.rs", "fn f() { std::fs::rename(a, b) }\n")), 1),
        "a rename with use std::fs": (lambda r: (control(r), plant(r, "crates/b/src/x.rs", "fn f() { fs::rename (&a, &b)?; }\n")), 1),
        "a rename after a test module": (
            lambda r: (control(r), plant(r, "crates/b/src/y.rs", "#[cfg(test)]\nmod tests {}\nfn f() { std::fs::rename(a, b) }\n")),
            1,
        ),
    }
    failed = 0
    for name, (build, want) in cases.items():
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            build(root)
            got = check(root, min_files=1)
            if len(got) != want:
                print(f"self-test {name}: want {want} faults, got {len(got)}: {got}")
                failed += 1
    with tempfile.TemporaryDirectory() as tmp:
        if not check(Path(tmp), min_files=1):
            print("self-test an empty tree: the scan of no file passed")
            failed += 1
    if failed:
        return 1
    print(f"self-test: {len(cases) + 1} cases agree")
    return 0


def main() -> int:
    if sys.argv[1:] == ["--self-test"]:
        return self_test()
    faults = check(ROOT)
    for fault in faults:
        print(fault)
    if faults:
        return 1
    print(f"check-private-replace: {len(loopback.scanned(ROOT))} files, no rename outside the helper")
    return 0


if __name__ == "__main__":
    sys.exit(main())
