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
    ("pod-gsr", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-hold", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-e2e", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-oci", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-wg", "fuzzing"): "for the fuzz targets of fuzz/; no consumer turns it on",
    ("pod-job", "schema"): "the JsonSchema of each type, for the tests that generate the JSON Schemas; "
                           "a consumer turns it on in [dev-dependencies] only, and no binary carries schemars",
    ("pod-testkit", "screen"): "a crate of tests; it is not in a released binary",
    ("pod-testkit", "stand"): "a crate of tests; it is not in a released binary",
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
