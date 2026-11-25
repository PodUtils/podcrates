#!/usr/bin/env python3
"""Make a shared cargo target fit the checkout that uses it (T-1384).

    python3 -I scripts/agent/stale_target.py [--target DIR] [--dry-run]
    python3 -I scripts/agent/stale_target.py --self-test

The lead checkout and the worktrees of agents under `.claude/worktrees/`
can share one target directory. Cargo names a path crate by its path
relative to the workspace root, and finds it fresh by the times of its
files. So a build in one tree can leave artifacts that another tree takes
as fresh: a full check then runs the code of another tree.

It does two things, from the checkout of its own path:

1. Each unit whose dep-info (`deps/NAME-HASH.d`) records a
   `CARGO_MANIFEST_DIR` in another checkout of this repository (the lead
   checkout, or a worktree under `.claude/worktrees/`, also a removed one)
   loses its artifacts (`deps/NAME-HASH*`, `deps/libNAME-HASH.*`) and its
   fingerprint (`.fingerprint/PKG-HASH`). Cargo then builds it again here.
   Such a binary holds the path of the other tree (a test that reads its
   files fails when that tree goes).
2. Each file that another live checkout changed against the commit of this
   checkout, and each file that this checkout changed, gets a new time.
   Cargo then builds again each crate that holds one of these files from
   the text of this checkout.

It prints one line for each count. Exit 0 when the target fits, 1 when a
file could not be removed or touched, 64 for a bad argument. `--target`
defaults to `$CARGO_TARGET_DIR`, else `target` of the checkout.
`--dry-run` prints what it would do and changes nothing. Python's standard
library only.
"""

import os
import shutil
import subprocess
import sys
import tempfile

WORKTREES = os.path.join(".claude", "worktrees")
KEY = "# env-dep:CARGO_MANIFEST_DIR="


def owner(path, lead):
    """The checkout of `path` (the lead checkout or one of its worktrees),
    or None when `path` is outside the lead checkout."""
    path = os.path.normpath(path)
    if path != lead and not path.startswith(lead + os.sep):
        return None
    rest = os.path.relpath(path, lead).split(os.sep)
    if rest[:2] == WORKTREES.split(os.sep) and len(rest) > 2:
        return os.path.join(lead, WORKTREES, rest[2])
    return lead


def deps_dirs(target):
    """Each `deps` directory of a profile: PROFILE/deps and
    TRIPLE/PROFILE/deps."""
    found = []
    for a in sorted(os.listdir(target)) if os.path.isdir(target) else []:
        first = os.path.join(target, a)
        if os.path.isdir(os.path.join(first, "deps")):
            found.append(first)
        elif os.path.isdir(first):
            for b in sorted(os.listdir(first)):
                second = os.path.join(first, b)
                if os.path.isdir(os.path.join(second, "deps")):
                    found.append(second)
    return found


def foreign_units(profile, lead, here):
    """(stem, the other checkout) of each unit of `profile` whose dep-info
    names a CARGO_MANIFEST_DIR in a checkout other than `here`."""
    deps = os.path.join(profile, "deps")
    units = []
    for name in sorted(os.listdir(deps)):
        if not name.endswith(".d"):
            continue
        try:
            with open(os.path.join(deps, name), encoding="utf-8", errors="replace") as f:
                lines = [l for l in f if l.startswith(KEY)]
        except OSError:
            continue
        for line in lines:
            other = owner(line[len(KEY) :].strip(), lead)
            if other is not None and other != here:
                units.append((name[:-2], other))
                break
    return units


def unit_files(profile, stem):
    """The artifacts and the fingerprint of the unit `stem` (NAME-HASH)."""
    deps = os.path.join(profile, "deps")
    files = [
        os.path.join(deps, n)
        for n in sorted(os.listdir(deps))
        if n == stem or n.startswith(stem + ".") or n.startswith("lib" + stem + ".")
    ]
    hash_ = stem.rsplit("-", 1)[-1]
    fp = os.path.join(profile, ".fingerprint")
    if os.path.isdir(fp):
        files += [os.path.join(fp, n) for n in sorted(os.listdir(fp)) if n.endswith("-" + hash_)]
    return files


def remove(path):
    if os.path.isdir(path) and not os.path.islink(path):
        shutil.rmtree(path)
    else:
        os.remove(path)


def git(args, cwd):
    run = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True, check=False)
    return run.stdout if run.returncode == 0 else None


def changed_files(here):
    """Each tracked file that another live checkout or this one changed
    against the commit of this checkout (paths relative to `here`)."""
    head = (git(["rev-parse", "HEAD"], here) or "").strip()
    listing = git(["worktree", "list", "--porcelain"], here) or ""
    trees = [l[len("worktree ") :] for l in listing.splitlines() if l.startswith("worktree ")]
    others = [t for t in trees if os.path.realpath(t) != here and os.path.isdir(t)]
    if not head or not others:
        return set(), 0
    names = set()
    for tree in others + [here]:
        out = git(["diff", "--name-only", "--no-renames", head], tree)
        names.update(n for n in (out or "").splitlines() if n)
    return names, len(others)


def run(argv, say=print):
    target, dry = None, False
    args = list(argv)
    while args:
        a = args.pop(0)
        if a == "--target" and args:
            target = args.pop(0)
        elif a == "--dry-run":
            dry = True
        else:
            say("usage: stale_target.py [--target DIR] [--dry-run] | --self-test")
            return 64
    here = os.path.realpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
    common = (git(["rev-parse", "--path-format=absolute", "--git-common-dir"], here) or "").strip()
    lead = os.path.realpath(os.path.dirname(common)) if common else here
    target = os.path.realpath(target or os.environ.get("CARGO_TARGET_DIR") or os.path.join(here, "target"))
    return fit(target, lead, here, changed_files(here), dry, say)


def fit(target, lead, here, changed, dry, say):
    names, trees = changed
    rc = 0
    units = 0
    for profile in deps_dirs(target):
        for stem, _other in foreign_units(profile, lead, here):
            units += 1
            for path in unit_files(profile, stem):
                try:
                    if not dry:
                        remove(path)
                except OSError as e:
                    say(f"stale-target: {path}: {e.strerror}")
                    rc = 1
    verb = "would remove" if dry else "removed"
    say(f"stale-target: {verb} {units} units built in another checkout ({target})")
    touched = 0
    for name in sorted(names):
        path = os.path.join(here, name)
        if not os.path.isfile(path):
            continue
        touched += 1
        try:
            if not dry:
                os.utime(path)
        except OSError as e:
            say(f"stale-target: {path}: {e.strerror}")
            rc = 1
    verb = "would touch" if dry else "touched"
    say(f"stale-target: {verb} {touched} files that {trees} other checkouts or this one changed")
    return rc


def self_test():
    ok = True

    def expect(cond, what):
        nonlocal ok
        print(("ok    " if cond else "FAIL  ") + what)
        ok = ok and cond

    with tempfile.TemporaryDirectory() as tmp:
        lead = os.path.join(tmp, "repo")
        wt = os.path.join(lead, WORKTREES, "agent")
        gone = os.path.join(lead, WORKTREES, "gone")
        target = os.path.join(tmp, "target")
        for profile in ("debug", os.path.join("x86_64-unknown-linux-musl", "release")):
            os.makedirs(os.path.join(target, profile, "deps"))
            os.makedirs(os.path.join(target, profile, ".fingerprint"))

        def unit(profile, stem, pkg, manifest, lib=False):
            p = os.path.join(target, profile)
            with open(os.path.join(p, "deps", stem + ".d"), "w") as f:
                f.write(f"{p}/deps/{stem}: crates/x/src/lib.rs\n\ncrates/x/src/lib.rs:\n\n")
                if manifest:
                    f.write(f"{KEY}{manifest}\n")
            name = ("lib" + stem + ".rlib") if lib else stem
            open(os.path.join(p, "deps", name), "w").close()
            os.makedirs(os.path.join(p, ".fingerprint", pkg + "-" + stem.rsplit("-", 1)[1]))

        unit("debug", "pod_job-aaaa", "pod-job", os.path.join(wt, "crates", "pod-job"))
        unit("debug", "pod_core-bbbb", "pod-core", os.path.join(gone, "crates", "pod-core"), lib=True)
        unit("debug", "pod_time-cccc", "pod-time", os.path.join(lead, "crates", "pod-time"))
        unit("debug", "serde-dddd", "serde", "/home/u/.cargo/registry/src/serde-1.0.0", lib=True)
        unit("debug", "plain-eeee", "plain", None)
        musl = os.path.join("x86_64-unknown-linux-musl", "release")
        unit(musl, "pod_link-ffff", "pod-link", os.path.join(wt, "crates", "pod-link"))

        lines = []
        rc = fit(target, lead, lead, (set(), 0), True, lines.append)
        expect(rc == 0 and "would remove 3 units" in lines[0], "a dry run counts the units of other checkouts")
        expect(os.path.exists(os.path.join(target, "debug", "deps", "pod_job-aaaa")), "a dry run removes nothing")
        lines = []
        rc = fit(target, lead, lead, (set(), 0), False, lines.append)
        d = os.path.join(target, "debug")
        expect(rc == 0 and "removed 3 units" in lines[0], "the lead checkout removes the units of worktrees")
        expect(not os.path.exists(os.path.join(d, "deps", "pod_job-aaaa")), "the test binary of a worktree goes")
        expect(not os.path.exists(os.path.join(d, ".fingerprint", "pod-job-aaaa")), "its fingerprint goes")
        expect(not os.path.exists(os.path.join(d, "deps", "libpod_core-bbbb.rlib")), "a unit of a removed worktree goes")
        expect(not os.path.exists(os.path.join(target, musl, "deps", "pod_link-ffff")), "a unit of a target triple goes")
        expect(os.path.exists(os.path.join(d, "deps", "pod_time-cccc")), "a unit of this checkout stays")
        expect(os.path.exists(os.path.join(d, "deps", "libserde-dddd.rlib")), "a unit of the registry stays")
        expect(os.path.exists(os.path.join(d, "deps", "plain-eeee")), "a unit with no manifest dir stays")
        lines = []
        rc = fit(target, lead, wt, (set(), 0), False, lines.append)
        expect("removed 1 units" in lines[0], "a worktree removes the units of the lead checkout")
        expect(not os.path.exists(os.path.join(d, "deps", "pod_time-cccc")), "the unit of the lead checkout goes")

        os.makedirs(os.path.join(lead, "crates", "x", "src"))
        src = os.path.join(lead, "crates", "x", "src", "lib.rs")
        open(src, "w").close()
        os.utime(src, (1000, 1000))
        lines = []
        rc = fit(target, lead, lead, ({"crates/x/src/lib.rs", "removed.rs"}, 1), False, lines.append)
        expect(rc == 0 and "touched 1 files that 1 other" in lines[1], "a changed file that exists is touched")
        expect(os.stat(src).st_mtime > 1000, "the touch gives the file a new time")
        expect(owner("/elsewhere", lead) is None, "a path outside the checkout has no owner")
        expect(owner(lead + "x/crates", lead) is None, "a sibling directory with the same prefix has no owner")
        expect(run(["--bad"], lambda _l: None) == 64, "a bad argument exits 64")
    print("stale-target self-test:", "passed" if ok else "FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        sys.exit(self_test())
    sys.exit(run(sys.argv[1:]))
