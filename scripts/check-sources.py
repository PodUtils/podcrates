#!/usr/bin/env python3
"""Compare the imported files of each crate with the default branch of their
repository (docs/design.md, section 9: drift).

  python3 -I scripts/check-sources.py [--clone REPO=DIR]... [--plant-change] [CRATE...]
  python3 -I scripts/check-sources.py record --clone DIR --repo OWNER/REPO --commit SHA
          --crate CRATE --map SRC=DST [--map SRC=DST]... [--skip PATH]... DIR...

Each crate that holds code of another repository has crates/CRATE/SOURCES,
and a directory of scripts that holds scripts of another repository has
scripts/DIR/SOURCES (scripts/services/SOURCES), of the same form:

  repo OWNER/REPO COMMIT          the repository and the commit of the import
  dir PATH                        an imported directory: a new or a removed
                                  file in it is a change
  skip PATH                       a file or directory of a dir that the crate
                                  did not take
  file BLOB PATH -> LOCAL...      an imported file, its git blob id at COMMIT,
                                  and the files of the crate that it became
  taken TAKEN [PATH...]           the consumer took the crate at TAKEN: the
                                  files of the block (or the PATHs, files or
                                  directories) are compared with the parent
                                  of TAKEN, not with the default branch, so
                                  that a fix before the adoption is still
                                  found and the adoption itself is no change

A SOURCES file can hold more than one `repo` block. `#` starts a comment.

The check reads each directory of each block at the default branch of its
repository (`main`, `master` or another name): through the read-only mirror
of the GitHub API (CHECK_SOURCES_API, by default
https://forge.web.pkgforge.dev/github), which gives the default branch at
`GET /repos/REPO`, or through a local clone that --clone names (`git fetch`
first), whose `origin/HEAD` names it. It prints one line for each changed,
new and removed file, with the commits that touched it after the import. A
change becomes a record entry "port" (TODO/RULES.md).

A file is `removed` only when the read at a resolved branch or commit finds
no file. A branch or a commit that the check cannot resolve is a source that
cannot be read (exit 2), never a removal.

`record` writes a block from a local clone at COMMIT: each file under each
DIR that no --skip names, with the file of the crate that --map gives.

--plant-change alters one recorded blob id first: the run must then exit 1.

Exit 0: no change. Exit 1: a change. Exit 2: a source cannot be read.
"""

import argparse
import json
import os
import random
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
API = os.environ.get("CHECK_SOURCES_API", "https://forge.web.pkgforge.dev/github").rstrip("/")


class Unreadable(Exception):
    pass


class Block:
    def __init__(self, repo, commit, line):
        self.repo, self.commit, self.line = repo, commit, line
        self.dirs, self.skips, self.files = [], [], {}
        self.taken = []  # (commit, [path...]); no path: the whole block

    def taken_at(self, rel: str):
        """The commit at which the consumer took `rel`, or None."""
        for commit, paths in self.taken:
            if not paths or any(rel == p or rel.startswith(p + "/") for p in paths):
                return commit
        return None


def parse(path: Path) -> list[Block]:
    blocks = []
    for n, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        word, _, rest = line.partition(" ")
        if word == "repo":
            parts = rest.split()
            if len(parts) != 2:
                raise Unreadable(f"{path}:{n}: `repo OWNER/REPO COMMIT`")
            blocks.append(Block(parts[0], parts[1], n))
            continue
        if not blocks:
            raise Unreadable(f"{path}:{n}: a `{word}` line before a `repo` line")
        b = blocks[-1]
        if word == "dir":
            b.dirs.append(rest.strip().rstrip("/"))
        elif word == "skip":
            b.skips.append(rest.strip().rstrip("/"))
        elif word == "taken":
            parts = rest.split()
            if not parts:
                raise Unreadable(f"{path}:{n}: `taken COMMIT [PATH...]`")
            b.taken.append((parts[0], [x.rstrip("/") for x in parts[1:]]))
        elif word == "file":
            left, arrow, right = rest.partition(" -> ")
            parts = left.split()
            if not arrow or len(parts) != 2 or not right.split():
                raise Unreadable(f"{path}:{n}: `file BLOB PATH -> LOCAL...`")
            b.files[parts[1]] = (parts[0], right.split())
        else:
            raise Unreadable(f"{path}:{n}: `{word}` is not repo, dir, skip, file or taken")
    return blocks


def skipped(b: Block, rel: str) -> bool:
    return any(rel == s or rel.startswith(s + "/") for s in b.skips)


class Absent(Exception):
    """The API answered 404."""


def get_json(url: str, absent_ok: bool = False):
    """The JSON document at url. With absent_ok, a 404 raises Absent; each
    other failure is Unreadable."""
    sep = "&" if "?" in url else "?"
    url = f"{url}{sep}n={random.randrange(1 << 30)}"
    req = urllib.request.Request(url, headers={"User-Agent": "podcrates-check-sources"})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return json.load(r)
    except urllib.error.HTTPError as e:
        if absent_ok and e.code == 404:
            raise Absent(url) from e
        raise Unreadable(f"{url}: {e}") from e
    except (urllib.error.URLError, OSError, ValueError) as e:
        raise Unreadable(f"{url}: {e}") from e


class Mirror:
    """The default branch of a repository through the GitHub API."""

    def __init__(self, repo: str):
        self.repo = repo
        self.parents = {}
        self.branch = None

    def default_branch(self) -> str:
        """The default branch, read once (`GET /repos/REPO`)."""
        if self.branch is None:
            data = get_json(f"{API}/repos/{self.repo}")
            branch = data.get("default_branch") if isinstance(data, dict) else None
            if not isinstance(branch, str) or not branch:
                raise Unreadable(f"{self.repo}: the API names no default branch")
            self.branch = branch
        return self.branch

    def rev(self, taken) -> str:
        """The default branch, or the parent of the commit of an adoption."""
        if taken is None:
            return self.default_branch()
        if taken not in self.parents:
            data = get_json(f"{API}/repos/{self.repo}/commits/{taken}")
            self.parents[taken] = data["parents"][0]["sha"]
        return self.parents[taken]

    def tree(self, path: str, rev: str) -> dict:
        """{file: blob} under a directory, {path: blob} for a file, or {} when
        the path is not at rev. rev is resolved (rev()), so a 404 is the
        path's."""
        quoted = urllib.parse.quote(path)
        try:
            data = get_json(f"{API}/repos/{self.repo}/contents/{quoted}?ref={rev}", absent_ok=True)
        except Absent:
            return {}
        if isinstance(data, dict):
            if data.get("type") == "file":
                return {data["path"]: data["sha"]}
            raise Unreadable(f"{self.repo}:{path}: {data.get('message', 'not a file or a directory')}")
        out = {}
        for e in data:
            if e["type"] == "file":
                out[e["path"]] = e["sha"]
            elif e["type"] == "dir":
                out.update(self.tree(e["path"], rev))
        return out

    def commits(self, path: str, since: str, rev: str) -> list[str]:
        quoted = urllib.parse.quote(path)
        data = get_json(f"{API}/repos/{self.repo}/commits?path={quoted}&sha={rev}&per_page=20")
        out = []
        for c in data:
            if c["sha"].startswith(since) or since.startswith(c["sha"]):
                break
            subject = (c["commit"]["message"].splitlines() or [""])[0]
            out.append(f"{c['sha'][:7]} {subject[:90]}")
        return out


class Clone:
    """The default branch of a repository in a local clone (origin/HEAD)."""

    def __init__(self, path: str):
        self.path = path
        self.branch = None

    def git(self, *args) -> str:
        r = subprocess.run(["git", "-C", self.path, *args], capture_output=True, text=True)
        if r.returncode != 0:
            raise Unreadable(f"git {' '.join(args)} in {self.path}: {r.stderr.strip()}")
        return r.stdout

    def default_branch(self) -> str:
        """origin/BRANCH for the default branch: the origin/HEAD of the clone,
        else the HEAD that the remote names."""
        if self.branch is None:
            try:
                ref = self.git("symbolic-ref", "--quiet", "refs/remotes/origin/HEAD").strip()
                branch = ref.removeprefix("refs/remotes/origin/")
            except Unreadable:
                branch = ""
                for line in self.git("ls-remote", "--symref", "origin", "HEAD").splitlines():
                    if line.startswith("ref: refs/heads/") and line.endswith("\tHEAD"):
                        branch = line[len("ref: refs/heads/"):-len("\tHEAD")]
            if not branch:
                raise Unreadable(f"{self.path}: the clone names no default branch")
            # The branch must exist in the clone: a read at a ref that does
            # not exist is not a removal.
            self.git("rev-parse", "--verify", "--quiet", f"refs/remotes/origin/{branch}^{{commit}}")
            self.branch = f"origin/{branch}"
        return self.branch

    def rev(self, taken) -> str:
        """The default branch, or the parent of the commit of an adoption."""
        if taken is None:
            return self.default_branch()
        self.git("rev-parse", "--verify", "--quiet", f"{taken}^^{{commit}}")
        return f"{taken}^"

    def tree(self, path: str, rev: str) -> dict:
        """As Mirror.tree: {} when the path is not at rev."""
        out = {}
        for line in self.git("ls-tree", "-r", rev, "--", path).splitlines():
            meta, name = line.split("\t", 1)
            kind, blob = meta.split()[1:3]
            if kind == "blob":
                out[name] = blob
        return out

    def commits(self, path: str, since: str, rev: str) -> list[str]:
        return self.git("log", "--format=%h %s", f"{since}..{rev}", "--", path).splitlines()


def compare(b: Block, source) -> list[str]:
    # Each path is read at the default branch, or before the adoption that
    # took it. rev() raises Unreadable for a ref that it cannot resolve, and
    # tree() gives {} only for a path that the resolved ref does not hold, so
    # `removed` is never the answer of a ref that is missing.
    def rev(rel: str) -> str:
        return source.rev(b.taken_at(rel))

    seen = {}
    for d in b.dirs:
        seen.update(source.tree(d, rev(d)))
    for rel in b.files:
        if not any(rel == d or rel.startswith(d + "/") for d in b.dirs):
            seen.update(source.tree(rel, rev(rel)))
    lines = []
    for rel, (blob, local) in sorted(b.files.items()):
        now = seen.get(rel)
        if now is None:
            lines.append(f"removed {b.repo}:{rel} (was {', '.join(local)})")
        elif now != blob:
            lines.append(f"changed {b.repo}:{rel} -> {', '.join(local)}")
            lines += [f"    {c}" for c in source.commits(rel, b.commit, rev(rel))]
    for rel in sorted(seen):
        if rel not in b.files and not skipped(b, rel):
            lines.append(f"new     {b.repo}:{rel}")
            lines += [f"    {c}" for c in source.commits(rel, b.commit, rev(rel))]
    return lines


def check(crates: list[str], clones: dict, plant: bool) -> int:
    # The crates, and the scripts of a directory of scripts/ (services/).
    paths = sorted(ROOT.glob("crates/*/SOURCES")) + sorted(ROOT.glob("scripts/*/SOURCES"))
    if crates:
        paths = [p for p in paths if p.parent.name in crates]
    if not paths:
        print("check-sources: no SOURCES file to read", file=sys.stderr)
        return 2
    changes = 0
    try:
        for path in paths:
            blocks = parse(path)
            if plant and blocks and blocks[0].files:
                rel = sorted(blocks[0].files)[0]
                blob, local = blocks[0].files[rel]
                blocks[0].files[rel] = ("0" * 40, local)
                plant = False
            for b in blocks:
                source = Clone(clones[b.repo]) if b.repo in clones else Mirror(b.repo)
                lines = compare(b, source)
                crate = path.parent.name
                for line in lines:
                    print(f"{crate}: {line}")
                changes += sum(1 for line in lines if not line.startswith(" "))
                taken = "".join(f", taken at {c[:7]}" + (f" ({len(ps)} paths)" if ps else "") for c, ps in b.taken)
                print(f"check-sources: {crate}: {b.repo} at {b.commit[:7]}{taken}: {len(b.files)} files, "
                      f"{sum(1 for line in lines if not line.startswith(' '))} changes")
    except Unreadable as e:
        print(f"check-sources: {e}", file=sys.stderr)
        return 2
    return 1 if changes else 0


def record(args) -> int:
    clone = Clone(args.clone)
    maps = [m.split("=", 1) for m in args.map]
    b = Block(args.repo, args.commit, 0)
    b.skips = [s.rstrip("/") for s in args.skip]
    lines = [f"repo {args.repo} {args.commit}"]
    files = {}
    dirs = 0
    for d in args.dirs:
        d = d.rstrip("/")
        found = clone.tree(d, args.commit)
        if list(found) != [d]:
            lines.append(f"dir {d}")
            dirs += 1
        files.update(found)
    lines += [f"skip {s}" for s in b.skips]
    for rel, blob in sorted(files.items()):
        if skipped(b, rel):
            continue
        local = rel
        for src, dst in maps:
            if rel == src or rel.startswith(src.rstrip("/") + "/"):
                local = dst.rstrip("/") + rel[len(src.rstrip("/")):]
                break
        lines.append(f"file {blob} {rel} -> {local}")
    out = ROOT / "crates" / args.crate / "SOURCES"
    head = ("# The files of this crate that came from another repository\n"
            "# (docs/design.md, section 9). A file line holds the git blob id at the\n"
            "# commit (`git ls-tree COMMIT -- PATH` gives it). scripts/check-sources.py\n"
            "# compares each with the default branch of its repository.\n")
    old = out.read_text(encoding="utf-8") if out.exists() else head
    out.write_text(old.rstrip("\n") + "\n\n" + "\n".join(lines) + "\n", encoding="utf-8")
    print(f"check-sources: {out.relative_to(ROOT)}: {len(lines) - dirs - len(b.skips) - 1} files")
    return 0


def main(argv: list[str]) -> int:
    if argv[:1] == ["record"]:
        p = argparse.ArgumentParser(prog="check-sources.py record")
        p.add_argument("--clone", required=True)
        p.add_argument("--repo", required=True)
        p.add_argument("--commit", required=True)
        p.add_argument("--crate", required=True)
        p.add_argument("--map", action="append", default=[])
        p.add_argument("--skip", action="append", default=[])
        p.add_argument("dirs", nargs="+")
        try:
            return record(p.parse_args(argv[1:]))
        except Unreadable as e:
            print(f"check-sources: {e}", file=sys.stderr)
            return 2
    p = argparse.ArgumentParser(prog="check-sources.py")
    p.add_argument("--clone", action="append", default=[], help="REPO=DIR")
    p.add_argument("--plant-change", action="store_true")
    p.add_argument("crates", nargs="*")
    args = p.parse_args(argv)
    clones = dict(c.split("=", 1) for c in args.clone)
    return check(args.crates, clones, args.plant_change)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
