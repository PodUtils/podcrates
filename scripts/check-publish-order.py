#!/usr/bin/env python3
"""Check that `cargo publish --workspace` can order the crates.

  python3 -I scripts/check-publish-order.py [--self-test]

cargo packs a crate before each crate that depends on it, and keeps each
dev-dependency that has a version. A crate P with a versioned
dev-dependency on a crate D of the workspace, where D depends on P, makes a
cycle: the pack of P asks for a version of D that is not there yet. Give
such a dev-dependency a path only (cargo drops it from the package). Exit 1
with each cycle; exit 0 with the count of crates.
"""

import json
import subprocess
import sys


def faults(packages: list[dict]) -> list[str]:
    names = {p["name"] for p in packages}
    packed = {p["name"] for p in packages if p.get("publish") != []}
    needs: dict[str, set[str]] = {p["name"]: set() for p in packages}
    for p in packages:
        for d in p["dependencies"]:
            if d["name"] in names and d.get("kind") in (None, "build"):
                needs[p["name"]].add(d["name"])

    def closure(start: str) -> set[str]:
        seen: set[str] = set()
        todo = [start]
        while todo:
            for n in needs[todo.pop()]:
                if n not in seen:
                    seen.add(n)
                    todo.append(n)
        return seen

    out = []
    for p in packages:
        if p["name"] not in packed:
            continue
        for d in p["dependencies"]:
            if d.get("kind") != "dev" or d["name"] not in names or d["req"] == "*":
                continue
            if p["name"] in closure(d["name"]):
                out.append(
                    f"{p['name']} has the versioned dev-dependency {d['name']} {d['req']}, "
                    f"and {d['name']} depends on {p['name']}: give the dev-dependency a path only"
                )
    return out


def self_test() -> int:
    def pkg(name, deps, publish=None):
        return {"name": name, "publish": publish, "dependencies": deps}

    dev = {"name": "b", "kind": "dev", "req": "^1.0"}
    cycle = [pkg("a", [dev]), pkg("b", [{"name": "a", "kind": None, "req": "^1.0"}])]
    assert len(faults(cycle)) == 1, "a versioned dev-dependency in a cycle is a fault"
    cycle[0]["dependencies"][0]["req"] = "*"
    assert not faults(cycle), "a path-only dev-dependency is no fault"
    cycle[0]["dependencies"][0]["req"] = "^1.0"
    cycle[0]["publish"] = []
    assert not faults(cycle), "a crate that is not packed is no fault"
    print("check-publish-order: self-test passed")
    return 0


def main(argv: list[str]) -> int:
    if argv[:1] == ["--self-test"]:
        return self_test()
    meta = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        capture_output=True, text=True, check=True,
    ).stdout
    packages = json.loads(meta)["packages"]
    found = faults(packages)
    for line in found:
        print(f"check-publish-order: {line}", file=sys.stderr)
    if found:
        return 1
    print(f"check-publish-order: {len(packages)} crates, no cycle of a versioned dev-dependency")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
