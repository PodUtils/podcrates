#!/usr/bin/env python3
"""Check schemas/CHANGES.md against the schema files of schemas/ (T-922).

  python3 -I scripts/check-schemas.py [--base TAG] [--no-fetch]
  python3 -I scripts/check-schemas.py diff OLD.json NEW.json

CHANGES.md has one row for each change of a schema:

  | Tag | Schema | Field | Change | What | SHA-256 |

* Tag: `vX.Y.Z`, the first tag that holds the change, or `next` for a
  change that no tag holds yet. The release sets `next` to its tag.
* Schema: `NAME.vN`, the file `schemas/NAME.vN.schema.json`.
* Field: the dotted path of the field (`trust_inputs.forge`); a branch of a
  tagged union is a part of the path (`hello.host`); `*` is the schema.
* Change: `new` (a new schema id), `added` (an optional field, or a value),
  `text` (no change of a field: words, the generator), and the changes that
  need a new id (the ruling "Schema ids"): `changed`, `removed`,
  `required`, and `withdrawn` (the file is removed).
* SHA-256: of the file at the tag; for `next`, of the file now.

The rules:

1. Each schema file has rows. The rows of one schema and one tag have one
   hash, and the rows of the last tag of a schema have the hash of the file
   now: a schema file that changes with no new row fails.
2. A tag is `next` or a tag at or below the version of the workspace, in
   order. A tag that git has gives the hash of its rows.
3. The base is the newest tag at or below the version of the workspace
   (local, or `git ls-remote` and a shallow fetch of the one tag). A field
   of a file of the base that the file now removes, makes required, or
   gives another type, value list or constant needs a new schema id: the
   same file name is a fault. Each field added since the base has an
   `added` row of a later tag. A schema new since the base has a `new` row,
   and only such a schema has `changed`, `removed` or `required` rows of a
   later tag.

`diff` prints the changes of the fields from OLD to NEW, one per line.

Exit 0: no fault. Exit 1: a fault, one line for each. Exit 2: the base
cannot be read.
"""

import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DIR = "schemas"
CHANGES = f"{DIR}/CHANGES.md"
KINDS = {"new", "added", "text", "changed", "removed", "required", "withdrawn"}
NEW_ID = {"changed", "removed", "required"}
TAG = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")
SCHEMA = re.compile(r"^([a-z0-9-]+)\.v(\d+)$")
NUMERIC = ("int", "uint", "float", "double")


def say(line: str) -> None:
    print(f"check-schemas: {line}")


# The fields of a schema.


def walk_schema(schema: dict) -> dict:
    """Each field of `schema` by its path: its types, its values (or None for
    any value), its format and whether it is required."""
    defs = schema.get("$defs", {})
    out: dict = {}

    def resolve(node, stack=()):
        while isinstance(node, dict) and str(node.get("$ref", "")).startswith("#/$defs/"):
            name = node["$ref"].split("/")[-1]
            if name in stack:
                return {}, stack
            stack = stack + (name,)
            node = defs.get(name, {})
        return (node if isinstance(node, dict) else {}), stack

    def jtype(v) -> str:
        if v is None:
            return "null"
        if isinstance(v, bool):
            return "boolean"
        if isinstance(v, int):
            return "integer"
        return {float: "number", str: "string", list: "array", dict: "object"}[type(v)]

    def types(node, stack=()) -> set:
        node, stack = resolve(node, stack)
        t = set()
        kind = node.get("type")
        t |= set(kind) if isinstance(kind, list) else ({kind} if kind else set())
        if "const" in node:
            t.add(jtype(node["const"]))
        t |= {jtype(v) for v in node.get("enum", [])}
        if "$ref" in node:
            t.add("ref:" + node["$ref"])
        for key in ("anyOf", "oneOf"):
            for b in node.get(key, []):
                t |= types(b, stack)
        if not t and "properties" in node:
            t.add("object")
        return t or {"any"}

    def values(node, stack=()):
        node, stack = resolve(node, stack)
        if "const" in node:
            return {json.dumps(node["const"])}
        branches = node.get("anyOf", []) + node.get("oneOf", [])
        if "enum" not in node and not branches:
            return {"null"} if node.get("type") == "null" else None
        got = {json.dumps(v) for v in node.get("enum", [])}
        for b in branches:
            v = values(b, stack)
            if v is None:
                return None
            got |= v
        return got

    def fmt(node) -> str | None:
        f = resolve(node)[0].get("format")
        return None if f is None or f.startswith(NUMERIC) else f

    def tag_of(branch) -> str | None:
        node = resolve(branch)[0]
        for name, sub in node.get("properties", {}).items():
            sub = resolve(sub)[0]
            if isinstance(sub.get("const"), str) and name in node.get("required", []):
                return sub["const"]
        return None

    def note(path, info):
        have = out.get(path)
        if have is None:
            out[path] = info
            return
        have["types"] |= info["types"]
        have["values"] = None if have["values"] is None or info["values"] is None else have["values"] | info["values"]
        have["required"] = have["required"] or info["required"]

    def walk(node, path, stack):
        node, stack = resolve(node, stack)
        for key in ("anyOf", "oneOf", "allOf"):
            for b in node.get(key, []):
                tag = tag_of(b) if key != "allOf" else None
                if tag:
                    note(path + (tag,), {"types": {"branch"}, "values": None, "format": None, "required": False})
                    walk(b, path + (tag,), stack)
                else:
                    walk(b, path, stack)
        required = set(node.get("required", []))
        for name, sub in node.get("properties", {}).items():
            p = path + (name,)
            note(p, {"types": types(sub, stack), "values": values(sub, stack), "format": fmt(sub),
                     "required": name in required})
            walk(sub, p, stack)
        items = node.get("items")
        if isinstance(items, dict):
            note(path + ("[]",), {"types": types(items, stack), "values": values(items, stack),
                                  "format": fmt(items), "required": False})
            walk(items, path, stack)
        extra = node.get("additionalProperties")
        if isinstance(extra, dict):
            note(path + ("*",), {"types": types(extra, stack), "values": values(extra, stack),
                                 "format": fmt(extra), "required": False})
            walk(extra, path + ("*",), stack)

    walk(schema, (), ())
    return out


def diff(old: dict, new: dict) -> list[tuple[str, str, str]]:
    """The changes of the fields from `old` to `new`: (kind, path, what)."""
    a, b = walk_schema(old), walk_schema(new)
    out = []
    for p in sorted(a):
        if p not in b and (len(p) == 1 or p[:-1] in b):
            out.append(("removed", ".".join(p), "the field is removed"))
    for p in sorted(b):
        if p not in a and (len(p) == 1 or p[:-1] in a):
            kind = "required" if b[p]["required"] else "added"
            out.append((kind, ".".join(p), "a new required field" if b[p]["required"] else "a new optional field"))
    for p in sorted(set(a) & set(b)):
        x, y = a[p], b[p]
        name = ".".join(p)
        if y["required"] and not x["required"]:
            out.append(("required", name, "the field becomes required"))
        if x["required"] and not y["required"]:
            out.append(("changed", name, "a required field becomes optional"))
        tx, ty = set(x["types"]), set(y["types"])
        if not (x["required"] and y["required"]):
            tx, ty = tx - {"null"}, ty - {"null"}
        if tx != ty:
            out.append(("changed", name, f"the type {sorted(tx)} becomes {sorted(ty)}"))
        if x["values"] is not None and y["values"] is not None:
            gone = x["values"] - y["values"] - {"null"}
            if gone:
                out.append(("changed", name, f"the values {sorted(gone)} are removed"))
            more = y["values"] - x["values"] - {"null"}
            if more:
                out.append(("added", name, f"the values {sorted(more)}"))
        elif x["values"] is None and y["values"] is not None:
            out.append(("changed", name, "a free value becomes a list of values"))
        if x["format"] != y["format"]:
            out.append(("changed", name, f"the format {x['format']} becomes {y['format']}"))
    return out


# The rows of CHANGES.md.


def rows_of(text: str, faults: list) -> list[dict]:
    out = []
    head = False
    for n, line in enumerate(text.splitlines(), 1):
        if not line.startswith("|"):
            continue
        cells = [c.strip().strip("`").strip() for c in line.strip().strip("|").split("|")]
        if cells[0] == "Tag":
            head = cells == ["Tag", "Schema", "Field", "Change", "What", "SHA-256"]
            if not head:
                faults.append(f"{CHANGES}:{n}: the head is not | Tag | Schema | Field | Change | What | SHA-256 |")
            continue
        if not head or set(line) <= set("|- "):
            continue
        if len(cells) != 6:
            faults.append(f"{CHANGES}:{n}: a row has {len(cells)} cells, not 6")
            continue
        tag, schema, field, kind, _, digest = cells
        if tag != "next" and not TAG.match(tag):
            faults.append(f"{CHANGES}:{n}: the tag {tag!r} is not vX.Y.Z or next")
        if not SCHEMA.match(schema):
            faults.append(f"{CHANGES}:{n}: the schema {schema!r} is not NAME.vN")
        if kind not in KINDS:
            faults.append(f"{CHANGES}:{n}: the change {kind!r} is not one of {sorted(KINDS)}")
        if not re.fullmatch(r"[0-9a-f]{64}|-", digest):
            faults.append(f"{CHANGES}:{n}: the SHA-256 {digest!r} is not 64 hex digits (or - for withdrawn)")
        out.append({"line": n, "tag": tag, "schema": schema, "field": field, "kind": kind, "sha": digest})
    return out


def order(tag: str) -> tuple:
    m = TAG.match(tag)
    return (int(m[1]), int(m[2]), int(m[3])) if m else (1 << 30,)


# The repository.


def run_git(*args: str) -> str | None:
    r = subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True)
    return r.stdout if r.returncode == 0 else None


def workspace_version() -> tuple:
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    m = re.search(r'^\[workspace\.package\][^\[]*?^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"', text, re.M | re.S)
    return (int(m[1]), int(m[2]), int(m[3])) if m else (0, 0, 0)


def base_tag(version: tuple, fetch: bool) -> tuple[str | None, str]:
    """The newest tag at or below `version`, and why there is none."""
    local = set((run_git("tag", "--list", "v*") or "").split())
    names = set(local)
    if fetch:
        remote = run_git("ls-remote", "--tags", "origin", "v*")
        if remote is None and not local:
            return None, "ls-remote of origin failed and no tag is local"
        for line in (remote or "").splitlines():
            names.add(line.split("refs/tags/")[-1].removesuffix("^{}"))
    tags = sorted((t for t in names if TAG.match(t) and order(t) <= version), key=order)
    if not tags:
        return None, f"no tag at or below v{'.'.join(map(str, version))}"
    tag = tags[-1]
    if tag not in local and run_git("fetch", "-q", "--no-tags", "--depth=1", "origin",
                                    f"refs/tags/{tag}:refs/tags/{tag}") is None:
        return None, f"the fetch of {tag} failed"
    return tag, ""


def at_tag(tag: str, name: str) -> str | None:
    return run_git("show", f"{tag}:{DIR}/{name}.schema.json")


def sha(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


# The check.


def check_rows(rows: list[dict], files: dict, faults: list) -> None:
    """Rules 1 and 2: the order of the tags, the hash of each group, and the
    hash of the file now."""
    version = workspace_version()
    last = (0,)
    for r in rows:
        if r["tag"] != "next" and order(r["tag"]) > version:
            faults.append(f"{CHANGES}:{r['line']}: the tag {r['tag']} is above the version of the workspace")
        if order(r["tag"]) < last:
            faults.append(f"{CHANGES}:{r['line']}: the rows are not in the order of their tags")
        last = max(last, order(r["tag"]))
    groups: dict = {}
    for r in rows:
        groups.setdefault(r["schema"], {}).setdefault(r["tag"], set()).add(r["sha"])
    for name in sorted(set(files) | set(groups)):
        tags = groups.get(name)
        if not tags:
            faults.append(f"{DIR}/{name}.schema.json has no row in {CHANGES}")
            continue
        for tag, hashes in sorted(tags.items()):
            if len(hashes) > 1:
                faults.append(f"{CHANGES}: the rows of {name} at {tag} have more than one SHA-256")
            if TAG.match(tag) and run_git("rev-parse", "-q", "--verify", f"refs/tags/{tag}") is not None:
                text = at_tag(tag, name)
                want = sha(text) if text is not None else "-"
                if want not in hashes:
                    faults.append(f"{CHANGES}: the SHA-256 of {name} at {tag} is {want}, not the one of its rows")
        newest = max(tags, key=order)
        now = sha(files[name]) if name in files else "-"
        if now not in tags[newest]:
            faults.append(f"{DIR}/{name}.schema.json changed with no new row in {CHANGES}: "
                          f"its SHA-256 is {now}, the rows of {newest} have {sorted(tags[newest])[0]}")


def check_ids(rows: list[dict], files: dict, base: str, faults: list) -> None:
    """Rule 3: the ids against the base."""
    later = [r for r in rows if order(r["tag"]) > order(base)]
    for name, text in files.items():
        old = at_tag(base, name)
        if old is None:
            if not any(r["schema"] == name and r["kind"] == "new" for r in later):
                faults.append(f"{name} is new since {base} and has no row new")
            continue
        m = SCHEMA.match(name)
        for kind, field, what in diff(json.loads(old), json.loads(text)):
            if kind in NEW_ID:
                faults.append(f"{name}: {field}: {what} since {base}: this needs a new schema id "
                              f"({m[1]}.v{int(m[2]) + 1}), not {name}")
            elif kind == "added" and not any(
                    r["schema"] == name and r["kind"] == "added" and r["field"] == field for r in later):
                faults.append(f"{name}: {field}: {what} since {base}, and no row added names it")
    for r in later:
        if r["kind"] in NEW_ID and at_tag(base, r["schema"]) is not None:
            faults.append(f"{CHANGES}:{r['line']}: {r['kind']} on {r['schema']}, an id of {base}: "
                          f"this change needs a new schema id")


def check(base: str | None, fetch: bool) -> tuple[list[str], int]:
    faults: list[str] = []
    path = ROOT / CHANGES
    if not path.is_file():
        return [f"{CHANGES} is missing"], 1
    rows = rows_of(path.read_text(encoding="utf-8"), faults)
    files = {p.name.removesuffix(".schema.json"): p.read_text(encoding="utf-8")
             for p in sorted((ROOT / DIR).glob("*.schema.json"))}
    check_rows(rows, files, faults)
    if base is None:
        base, why = base_tag(workspace_version(), fetch)
        if base is None:
            return faults + [f"the base cannot be read: {why}"], 2
    check_ids(rows, files, base, faults)
    return faults, (1 if faults else 0)


def main(argv: list[str]) -> int:
    if argv[:1] == ["diff"] and len(argv) == 3:
        old, new = (json.loads(Path(p).read_text(encoding="utf-8")) for p in argv[1:])
        for kind, field, what in diff(old, new):
            print(f"{kind}\t{field}\t{what}")
        return 0
    base = argv[argv.index("--base") + 1] if "--base" in argv else None
    faults, code = check(base, "--no-fetch" not in argv)
    for f in faults:
        say(f)
    if code == 0:
        say(f"{len(list((ROOT / DIR).glob('*.schema.json')))} schemas agree with {CHANGES}")
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
