#!/usr/bin/env python3
"""Check that the static probes cover each feature of each library crate.

  python3 -I scripts/check-probe.py [--no-tree]

The probes are the packages examples/static-probe* (docs/design.md,
section 7). A feature of a crate in crates/ is covered when a probe turns it
on, directly, through another feature, or through the default features. A
feature in ALLOWED needs no probe, for the reason given there. The package
static-probe-lean builds with no tokio: unless --no-tree is given, the
check runs `cargo tree` and refuses a tokio in its tree.

Exit 1 with one line for each fault.
"""

import re
import subprocess
import sys
import tomllib
from pathlib import Path

# (crate, feature): why no probe needs it.
ALLOWED = {
    ("pod-proxy", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-cloudflare", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-cloudflare", "standin"): "the stand-ins of the quick-tunnel service and of the edge, for the tests of this "
                                   "crate and of a consumer; a consumer turns it on in [dev-dependencies] only",
    ("pod-gsr", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-link", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-tpm", "soft"): "the TPM in software of the tests of this crate and of a consumer; a consumer turns it on "
                         "in [dev-dependencies] only, and no binary carries it",
    ("pod-link", "s3-standin"): "the stand-in of the S3 API of the tests of this crate and of a consumer; "
                                "a consumer turns it on in [dev-dependencies] only, and no binary carries it",
    ("pod-hold", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-e2e", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-control", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-piping", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-opentunnel", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-egress", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-egress", "plain-ws"): "for the tests of the Access road over plain TCP to the loopback; no binary turns it on",
    ("pod-relay", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-relay", "plain-ws"): "for the tests of an embedder over plain TCP to the loopback; no binary turns it on",
    ("pod-oci", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-wg", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-actions-results", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-job", "schema"): "the JsonSchema of each type, for the tests that generate the JSON Schemas; "
                           "a consumer turns it on in [dev-dependencies] only, and no binary carries schemars",
    ("pod-broker", "schema"): "the JsonSchema of the policy, for the tests that generate broker-policy.v1 and log.v1; "
                            "no binary carries schemars",
    ("pod-hostprobe", "schema"): "report::schema, the JSON Schema of the report, for the tests of a consumer "
                                 "that validate a report; no binary carries schemars",
    ("pod-testkit", "screen"): "a crate of tests; it is not in a released binary",
    ("pod-testkit", "schema"): "a crate of tests; it is not in a released binary",
    ("pod-release", "cli"): "the binary pod-release, the verifier of a release; the static job of ci.yml and "
                            "scripts/agent/check.sh check its own binary, and no binary of a consumer carries it",
    ("pod-release", "schema"): "index::schema, the JSON Schema of the release index, for the tests that generate "
                               "release-index.v1 and for the tests of a consumer; no binary carries schemars",
    ("pod-testkit", "stand"): "a crate of tests; it is not in a released binary",
    ("pod-testkit", "refused"): "a crate of tests; it is not in a released binary",
    ("pod-testkit", "masque"): "a crate of tests; it is not in a released binary",
    ("pod-testkit", "github"): "the fake GitHub of a consumer's CI; the static job of ci.yml checks its own "
                               "binary pod-fake-github, and no binary of a consumer carries it",
    ("pod-testkit", "githome"): "the fake of githome's source of jobs for podrunner's CI; the static job of ci.yml "
                                "checks its own binary pod-fake-githome, and no binary of a consumer carries it",
    ("pod-wormhole", "standin"): "the stand-in mailbox of the tests of this crate and of a consumer; "
                                 "a consumer turns it on in [dev-dependencies] only, and no binary carries it",
    ("pod-derp", "standin"): "the stand-in DERP server of the tests of this crate and of a consumer; "
                             "a consumer turns it on in [dev-dependencies] only, and no binary carries it",
    ("pod-egress", "standin"): "the stand-in servers of via, wstunnel, VLESS, Trojan, Wisp and Shadowsocks of the "
                               "tests of this crate and of a consumer; a consumer turns it on in "
                               "[dev-dependencies] only, and no binary carries it",
    ("pod-iroh", "test-relay"): "a relay server and a server of pkarr on the loopback, and the way out of a test, "
                                "for the tests of this crate and of a consumer; a consumer turns it on in "
                                "[dev-dependencies] only, and no binary carries it",
    ("pod-ws", "plain-ws"): "for the tests of an embedder over plain TCP to the loopback; no binary turns it on",
}
LEAN = "static-probe-lean"


def load(path: Path) -> dict:
    return tomllib.loads(path.read_text(encoding="utf-8"))


def closure(features: dict, start: set[str]) -> tuple[set[str], list[tuple[str, str]]]:
    """The features of one crate that START turns on, through the table, and
    the features of other crates (`crate/feature`) that they turn on."""
    seen, todo, cross = set(), list(start), []
    while todo:
        f = todo.pop()
        if f in seen or f not in features:
            continue
        seen.add(f)
        for item in features[f]:
            if "/" in item:
                other, _, feature = item.partition("/")
                cross.append((other.rstrip("?"), feature))
            elif not item.startswith("dep:"):
                todo.append(item)
    return seen, cross


def main(argv: list[str]) -> int:
    root = Path(__file__).resolve().parent.parent
    workspace = load(root / "Cargo.toml")["workspace"]["dependencies"]
    crates = {}
    for manifest in sorted((root / "crates").glob("*/Cargo.toml")):
        data = load(manifest)
        if (manifest.parent / "src" / "lib.rs").exists():
            crates[data["package"]["name"]] = data.get("features", {})
    covered = {name: set() for name in crates}
    probes = sorted((root / "examples").glob("static-probe*/Cargo.toml"))
    for manifest in probes:
        for name, dep in load(manifest).get("dependencies", {}).items():
            if name not in crates:
                continue
            dep = {"workspace": True} if dep is True else dep
            base = workspace.get(name, {}) if dep.get("workspace") else dep
            defaults = dep.get("default-features", base.get("default-features", True))
            start = set(dep.get("features", []))
            if defaults:
                start.add("default")
            own, cross = closure(crates[name], start)
            covered[name] |= own
            for other, feature in cross:
                if other in crates:
                    # The feature of another crate, and what it turns on.
                    covered[other] |= closure(crates[other], {feature})[0]
    faults = []
    for name, features in crates.items():
        for feature in features:
            if feature != "default" and feature not in covered[name] and (name, feature) not in ALLOWED:
                faults.append(f"{name}: feature {feature} is in no static probe, and not in ALLOWED")
    for name, feature in ALLOWED:
        if feature not in crates.get(name, {}):
            faults.append(f"ALLOWED names {name} feature {feature}, which does not exist")
    if not probes:
        faults.append("no examples/static-probe*/Cargo.toml")
    if "--no-tree" not in argv:
        tree = subprocess.run(
            ["cargo", "tree", "--locked", "-p", LEAN, "-e", "normal", "--prefix", "none"],
            cwd=root, capture_output=True, text=True,
        )
        if tree.returncode != 0:
            print(f"check-probe: cargo tree failed: {tree.stderr.strip()[:300]}", file=sys.stderr)
            return 2
        if re.search(r"^tokio v", tree.stdout, re.M):
            faults.append(f"{LEAN}: tokio is in the tree; the probe must build with no tokio")
    for fault in faults:
        print(fault)
    print(f"check-probe: {len(crates)} crates, {len(faults)} faults")
    return 1 if faults else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
