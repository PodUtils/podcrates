#!/usr/bin/env python3
"""Write the release index of a product (T-104, call C793).

  python3 -I scripts/release-index.py --project OWNER/REPO --version TAG --dir DIR
          [--date EPOCH|YYYY-MM-DDTHH:MM:SSZ] [--commit SHA] [--workflow FILE]
          [--issuer URL] [--sbom NAME|URL] [--acceptance NAME|URL] [--out FILE] NAME...
  python3 -I scripts/release-index.py --self-test

The index is `release-index.json` of the schema
`schemas/release-index.v1.schema.json`; `crates/pod-release` reads and
checks it (`pod_release::index`). Each NAME is a file of DIR: the index gives
its SHA-256, its size and the target triple in its name. The index names
`SHA256SUMS`, its bundle `SHA256SUMS.sigstore.json`, and the identity of the
signer: the workflow FILE (default `release.yml`) of OWNER/REPO at the tag.

Write the index before `SHA256SUMS`, and put its line in `SHA256SUMS`: the
signature of `SHA256SUMS` then signs the index too. The same arguments give
the same bytes (`--date` from `SOURCE_DATE_EPOCH`).

--self-test writes an index of files with known bytes, compares it with
`crates/pod-release/tests/fixtures/index/release-index.json` (the file that
the tests of `pod-release` read), and checks each refusal. `--self-test
--write` writes that file again.

Exit 0: written. Exit 2: a usage error or a file that cannot be read, with
one line on stderr.
"""

import argparse
import contextlib
import datetime
import hashlib
import io
import json
import re
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIXTURE = ROOT / "crates/pod-release/tests/fixtures/index/release-index.json"
ISSUER = "https://token.actions.githubusercontent.com"
OWN = ("SHA256SUMS", "SHA256SUMS.sigstore.json", "release-index.json")
TAG = re.compile(r"^[v0-9][0-9A-Za-z.+-]{0,63}$")
PART = re.compile(r"^[A-Za-z0-9._-]+$")
# The target triple at the end of a file name: ARCH-VENDOR-OS[-ABI], and
# `.exe` after it on Windows.
TARGET = re.compile(r"-([a-z0-9_]+-(?:unknown|apple|pc)-[a-z0-9_]+(?:-[a-z0-9_]+)?)(?:\.exe)?$")


class Usage(Exception):
    pass


def plain(name: str) -> bool:
    return bool(name) and name not in (".", "..") and "/" not in name and "\\" not in name and name.isprintable()


def date_of(text: str) -> str:
    if text.isdigit():
        moment = datetime.datetime.fromtimestamp(int(text), datetime.timezone.utc)
        return moment.strftime("%Y-%m-%dT%H:%M:%SZ")
    try:
        datetime.datetime.strptime(text, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError:
        raise Usage(f"--date {text!r}: give seconds since the epoch, or YYYY-MM-DDTHH:MM:SSZ") from None
    return text


def index(args) -> dict:
    project = args.project.split("/")
    if len(project) != 2 or not all(PART.match(p) for p in project):
        raise Usage(f"--project {args.project!r}: give OWNER/REPO")
    if not TAG.match(args.version) or ".." in args.version:
        raise Usage(f"--version {args.version!r}: give the tag of the release, as v1.2.3")
    if args.commit is not None and not re.fullmatch(r"[0-9a-f]{40}", args.commit):
        raise Usage(f"--commit {args.commit!r}: give 40 lowercase hex digits")
    if args.workflow is not None and not plain(args.workflow):
        raise Usage(f"--workflow {args.workflow!r}: give a file name of .github/workflows/")
    names = sorted(set(args.names))
    if not names:
        raise Usage("give the name of each file of the release")
    files = []
    for name in names:
        if not plain(name) or name in OWN:
            raise Usage(f"{name!r}: give the plain name of a file of the release, not {', '.join(OWN)}")
        path = Path(args.dir) / name
        try:
            body = path.read_bytes()
        except OSError as e:
            raise Usage(f"{path}: {e.strerror}") from None
        entry = {"name": name, "sha256": hashlib.sha256(body).hexdigest(), "size": len(body)}
        match = TARGET.search(name)
        if match:
            entry["target"] = match.group(1)
        files.append(entry)
    for what, value in (("sbom", args.sbom), ("acceptance", args.acceptance)):
        if value is not None and value not in names and not value.startswith("https://"):
            raise Usage(f"--{what} {value!r}: give a file of the release, or an https:// URL")
    out = {
        "schema": 1,
        "project": args.project,
        "version": args.version,
        "date": date_of(args.date),
    }
    if args.commit is not None:
        out["commit"] = args.commit
    workflow = args.workflow or "release.yml"
    out["signature"] = {
        "sums": "SHA256SUMS",
        "bundle": "SHA256SUMS.sigstore.json",
        "identity": f"https://github.com/{args.project}/.github/workflows/{workflow}@refs/tags/{args.version}",
        "issuer": args.issuer or ISSUER,
    }
    out["files"] = files
    if args.sbom is not None:
        out["sbom"] = args.sbom
    if args.acceptance is not None:
        out["acceptance"] = args.acceptance
    return out


def text(doc: dict) -> str:
    return json.dumps(doc, indent=2, ensure_ascii=False) + "\n"


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="release-index.py", description="Write the release index of a product.")
    p.add_argument("--project", required=True, help="the repository, OWNER/REPO")
    p.add_argument("--version", required=True, help="the tag of the release")
    p.add_argument("--dir", required=True, help="the directory of the files")
    p.add_argument("--date", default="0", help="seconds since the epoch, or YYYY-MM-DDTHH:MM:SSZ")
    p.add_argument("--commit", help="the commit of the tag")
    p.add_argument("--workflow", help="the release workflow (default release.yml)")
    p.add_argument("--issuer", help=f"the issuer of the OIDC token (default {ISSUER})")
    p.add_argument("--sbom", help="the SBOM: a file of the release, or an https:// URL")
    p.add_argument("--acceptance", help="the acceptance: a file of the release, or an https:// URL")
    p.add_argument("--out", help="the file to write (default: the standard output)")
    p.add_argument("names", nargs="*", help="the files of the release")
    return p


def run(argv: list[str]) -> int:
    args = parser().parse_args(argv)
    try:
        body = text(index(args))
    except Usage as e:
        print(f"release-index: {e}", file=sys.stderr)
        return 2
    if args.out:
        Path(args.out).write_text(body, encoding="utf-8")
    else:
        sys.stdout.write(body)
    return 0


def self_test(write: bool = False) -> int:
    faults = []
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        for name in ("podlab-x86_64-unknown-linux-musl", "podlab-aarch64-pc-windows-msvc.exe", "podlab-install.sh",
                     "sbom.spdx.json"):
            (d / name).write_bytes(f"{name}\n".encode())
        base = ["--project", "PodUtils/podcrates", "--version", "v0.77.0", "--dir", tmp,
                "--date", "1795080477", "--commit", "0" * 40, "--sbom", "sbom.spdx.json",
                "--acceptance", "https://github.com/PodUtils/podcrates/blob/v0.77.0/TODO/acceptance.json"]
        names = ["podlab-x86_64-unknown-linux-musl", "podlab-install.sh", "podlab-aarch64-pc-windows-msvc.exe",
                 "sbom.spdx.json"]
        out = d / "release-index.json"
        if run(base + ["--out", str(FIXTURE if write else out)] + names) != 0:
            faults.append("the control run failed")
        elif not write and out.read_text(encoding="utf-8") != FIXTURE.read_text(encoding="utf-8"):
            faults.append(f"the index differs from {FIXTURE.relative_to(ROOT)}: write it again with --self-test --write")
        bad = {
            "a path": base + ["../x"],
            "a missing file": base + ["nothing"],
            "the sums": base + ["SHA256SUMS"],
            "a bad tag": [a if a != "v0.77.0" else "v1/../x" for a in base] + names,
            "a bad commit": [a if a != "0" * 40 else "abc" for a in base] + names,
            "an sbom that is no file": [a if a != "sbom.spdx.json" else "x.json" for a in base] + names,
            "no file": base,
        }
        for what, argv in bad.items():
            said = io.StringIO()
            with contextlib.redirect_stderr(said):
                code = run(argv + ["--out", str(d / "bad.json")])
            if code != 2 or not said.getvalue().startswith("release-index: "):
                faults.append(f"{what}: not refused")
    for f in faults:
        print(f"release-index: self-test: {f}", file=sys.stderr)
    print(f"release-index: self-test: {'fail' if faults else 'ok'}")
    return 1 if faults else 0


if __name__ == "__main__":
    if sys.argv[1:2] == ["--self-test"]:
        sys.exit(self_test(sys.argv[2:] == ["--write"]))
    sys.exit(run(sys.argv[1:]))
