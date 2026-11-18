"""The scanners of scripts/codegraph.py: the crates and their dependencies,
the items of the Rust code, the function bodies of Rust and Python, the
entries of the record and the paragraphs of the documents.

Each scanner reads files only and gives plain data. The Rust scanner takes
the code view of `scripts/check-loopback.py` (comments and literals made
blank at the same length), so that both checks read Rust in one way.
"""

from __future__ import annotations

import hashlib
import importlib.util
import io
import re
import tokenize
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

_spec = importlib.util.spec_from_file_location("check_loopback", Path(__file__).resolve().parent / "check-loopback.py")
loopback = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(loopback)

# A body shorter than this is no clone: an accessor or a one-line wrapper
# repeats by design.
MIN_BODY_LINES = 6
MIN_BODY_CHARS = 160

ITEM = re.compile(
    r"^[ \t]*(?P<vis>pub(?:\s*\([^)]*\))?\s+)?"
    r"(?:(?:default|const|async|unsafe|extern\s*(?:\"[^\"]*\")?)\s+)*"
    r"(?P<kind>fn|struct|enum|trait|type|const|static|mod|union)\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)",
    re.M,
)
MACRO = re.compile(r"^[ \t]*macro_rules!\s*(?P<name>[A-Za-z_][A-Za-z0-9_]*)", re.M)
ENTRY_ROW = re.compile(r"^\| \[(T-\d+)\]\(([a-z]+)\.md\) \| (P\d) \| ([SML]) \| (M\d) \| (\w+) \| (\w+) \| (.*) \|$")
WORD = re.compile(r"[A-Za-z][A-Za-z0-9]*")
CAMEL = re.compile(r"[A-Z]+(?=[A-Z][a-z])|[A-Z]?[a-z]+|[A-Z]+|\d+")
STOP = frozenset(
    "a an and as at be by each for from in into is it its no not of on one or that the this to with when "
    "which who what than then there their them they can may must will has have do does".split()
)


@dataclass
class Item:
    crate: str
    kind: str
    name: str
    module: str
    file: str
    line: int
    public: bool
    test: bool
    doc: str = ""
    body: str = ""  # the hash of the normalized body, for a fn with a body of size
    lines: int = 0


@dataclass
class Crate:
    name: str
    path: str
    deps: dict[str, list[str]] = field(default_factory=dict)  # kind -> pod crates
    features: list[str] = field(default_factory=list)
    summary: str = ""


@dataclass
class Entry:
    id: str
    area: str
    status: str
    title: str


def words(text: str) -> list[str]:
    """The lower-case words of a text or an identifier, split at `_`, `::`,
    `-` and the humps of a camel case name, with a short suffix cut, and
    with no stop word."""
    out = []
    for raw in WORD.findall(text.replace("_", " ")):
        for part in CAMEL.findall(raw) or [raw]:
            w = part.lower()
            if w in STOP or len(w) < 2:
                continue
            for suffix in ("ing", "ies", "es", "ed", "s"):
                if w.endswith(suffix) and len(w) - len(suffix) >= 3:
                    w = w[: -len(suffix)] + ("y" if suffix == "ies" else "")
                    break
            out.append(w)
    return out


def digest(text: str) -> str:
    return hashlib.sha256(text.encode()).hexdigest()[:16]


def crates(root: Path) -> list[Crate]:
    """Each member crate of `crates/` with its pod dependencies by kind
    (`normal`, `dev`, `build`), its features and the first paragraph of its
    README."""
    out = []
    for manifest in sorted(root.glob("crates/*/Cargo.toml")):
        data = tomllib.loads(manifest.read_text(encoding="utf-8"))
        pkg = data.get("package", {})
        c = Crate(name=pkg.get("name", manifest.parent.name), path=manifest.parent.relative_to(root).as_posix())
        for kind, key in (("normal", "dependencies"), ("dev", "dev-dependencies"), ("build", "build-dependencies")):
            names = [n for n in data.get(key, {}) if n.startswith("pod-")]
            for target in data.get("target", {}).values():
                names += [n for n in target.get(key, {}) if n.startswith("pod-") and n not in names]
            c.deps[kind] = sorted(names)
        c.features = sorted(data.get("features", {}))
        readme = manifest.parent / "README.md"
        if readme.exists():
            c.summary = first_paragraph(readme.read_text(encoding="utf-8"))
        out.append(c)
    return out


def first_paragraph(text: str) -> str:
    para = []
    for line in text.splitlines():
        if line.startswith("#") or line.startswith("|") or line.startswith("```"):
            if para:
                break
            continue
        if not line.strip():
            if para:
                break
            continue
        para.append(line.strip())
    return " ".join(para)


def module_of(rel: Path) -> str:
    """The module path of a source file from the parts after `src`."""
    parts = list(rel.parts)
    if "src" not in parts:
        return "::".join(p.removesuffix(".rs") for p in parts)
    parts = parts[parts.index("src") + 1 :]
    if parts and parts[-1] in ("lib.rs", "main.rs", "mod.rs"):
        parts = parts[:-1]
    elif parts:
        parts[-1] = parts[-1].removesuffix(".rs")
    return "::".join(parts)


def test_spans(code: str) -> list[tuple[int, int]]:
    spans = []
    for m in loopback.TEST_MOD.finditer(code):
        spans.append((m.start(), loopback.closing(code, m.end(), "{", "}")))
    return spans


def doc_above(lines: list[str], index: int) -> str:
    """The first line of the `///` block above line `index` (0-based),
    over the attributes between them."""
    docs = []
    k = index - 1
    while k >= 0:
        s = lines[k].strip()
        if s.startswith("///"):
            docs.append(s[3:].strip())
        elif s.startswith("#[") or s.startswith("//"):
            pass
        else:
            break
        k -= 1
    docs.reverse()
    return next((d for d in docs if d), "")


def normal_body(code: str) -> str:
    """A body of the code view with each run of space as one space: the
    same tokens give the same text, whatever their layout and comments."""
    return " ".join(code.split())


MOD_DECL = re.compile(r"(?P<attrs>(?:#\[[^\]]*\]\s*)*)(?:pub(?:\([^)]*\))?\s+)?mod\s+(?P<name>\w+)\s*;")
PATH_ATTR = re.compile(r'#\[path\s*=\s*"([^"]+)"\]')


def child_modules(path: Path, text: str) -> list[tuple[Path, bool]]:
    """Each file that a `mod NAME;` of `path` names, with true for a
    module under `#[cfg(test)]`."""
    code = loopback.code_view(text)
    out = []
    here = path.parent if path.name in ("lib.rs", "main.rs", "mod.rs") else path.parent / path.stem
    for m in MOD_DECL.finditer(code):
        attrs = text[m.start("attrs") : m.end("attrs")]
        test = "cfg(test)" in attrs.replace(" ", "")
        named = PATH_ATTR.search(attrs)
        if named:
            out.append((path.parent / named.group(1), test))
            continue
        name = m.group("name")
        for cand in (here / f"{name}.rs", here / name / "mod.rs"):
            if cand.exists():
                out.append((cand, test))
                break
    return out


def test_files(crate_dir: Path) -> set[Path]:
    """Each source file of a crate that only a test build compiles: a module
    under `#[cfg(test)]` and each module below it."""
    out: set[Path] = set()
    todo = [(p, False) for p in (crate_dir / "src" / "lib.rs", crate_dir / "src" / "main.rs") if p.exists()]
    seen: set[Path] = set()
    while todo:
        path, test = todo.pop()
        if path in seen or not path.exists():
            continue
        seen.add(path)
        if test:
            out.add(path)
        for child, child_test in child_modules(path, path.read_text(encoding="utf-8", errors="replace")):
            todo.append((child, test or child_test))
    return out


def rust_items(root: Path, path: Path, crate: str, test_file: bool = False) -> list[Item]:
    text = path.read_text(encoding="utf-8", errors="replace")
    code = loopback.code_view(text)
    rel = path.relative_to(root)
    parts = rel.parts
    in_tests = test_file or "tests" in parts or "benches" in parts or path.name == "tests.rs" or path.name.endswith("_tests.rs")
    spans = test_spans(code)
    lines = text.splitlines()
    out = []
    for m in ITEM.finditer(code):
        pos = m.start("kind")
        line = code.count("\n", 0, pos) + 1
        test = in_tests or any(a <= pos < b for a, b in spans)
        item = Item(
            crate=crate,
            kind=m.group("kind"),
            name=m.group("name"),
            module=module_of(rel),
            file=rel.as_posix(),
            line=line,
            public=(m.group("vis") or "").strip() == "pub",
            test=test,
            doc=doc_above(lines, line - 1),
        )
        if item.kind == "fn":
            brace = code.find("{", m.end())
            semi = code.find(";", m.end())
            if brace >= 0 and (semi < 0 or brace < semi):
                end = loopback.closing(code, brace + 1, "{", "}")
                body = code[brace:end]
                n = sum(1 for b in body.splitlines() if b.strip())
                norm = normal_body(body)
                if n >= MIN_BODY_LINES and len(norm) >= MIN_BODY_CHARS:
                    item.body, item.lines = digest(norm), n
        out.append(item)
    for m in MACRO.finditer(code):
        line = code.count("\n", 0, m.start()) + 1
        out.append(Item(crate, "macro", m.group("name"), module_of(rel), rel.as_posix(), line, True, in_tests))
    return out


def python_items(root: Path, path: Path) -> list[Item]:
    """Each `def` of a Python file, with the hash of its tokens: no comment,
    each string as one mark."""
    text = path.read_text(encoding="utf-8", errors="replace")
    rel = path.relative_to(root).as_posix()
    try:
        toks = list(tokenize.generate_tokens(io.StringIO(text).readline))
    except (tokenize.TokenError, SyntaxError, IndentationError):
        return []
    out = []
    for i, t in enumerate(toks):
        if t.type != tokenize.NAME or t.string != "def" or i + 1 >= len(toks):
            continue
        name = toks[i + 1].string
        start_line = t.start[0]
        col = t.start[1]
        body = []
        j = i
        seen_colon = False
        while j < len(toks):
            tk = toks[j]
            if tk.type == tokenize.OP and tk.string == ":" and not seen_colon and tk.start[0] >= start_line:
                seen_colon = True
            elif seen_colon and tk.type == tokenize.DEDENT and tk.start[1] <= col and tk.start[0] > start_line:
                break
            if tk.type not in (tokenize.COMMENT, tokenize.NL, tokenize.ENCODING):
                body.append("S" if tk.type == tokenize.STRING else tk.string.strip() or tokenize.tok_name[tk.type])
            j += 1
        end_line = toks[min(j, len(toks) - 1)].start[0]
        item = Item("scripts", "def", name, Path(rel).stem, rel, start_line, not name.startswith("_"), False)
        n = end_line - start_line
        norm = " ".join(body[2:])  # the name does not count: a renamed copy is a copy
        if n >= MIN_BODY_LINES and len(norm) >= MIN_BODY_CHARS:
            item.body, item.lines = digest(norm), n
        out.append(item)
    return out


def items(root: Path) -> list[Item]:
    out = []
    for crate_dir in sorted(root.glob("crates/*")):
        if not (crate_dir / "Cargo.toml").exists():
            continue
        crate = crate_dir.name
        tests = test_files(crate_dir)
        for path in sorted(crate_dir.rglob("*.rs")):
            if "target" in path.relative_to(crate_dir).parts:
                continue
            out += rust_items(root, path, crate, path in tests)
    for path in sorted((root / "scripts").rglob("*.py")):
        out += python_items(root, path)
    return out


def entries(root: Path) -> list[Entry]:
    out = []
    index = root / "TODO" / "INDEX.md"
    if not index.exists():
        return out
    for line in index.read_text(encoding="utf-8").splitlines():
        m = ENTRY_ROW.match(line)
        if m:
            out.append(Entry(id=m.group(1), area=m.group(2), status=m.group(7), title=m.group(8)))
    return out


# The documents whose paragraphs must not repeat one another. A reading at
# a time (releases, research, reports, upstream) and the entries of the
# record keep the words of their time, and are not read.
DOCS = ["AGENTS.md", "README.md", "SECURITY.md", "docs/*.md", "TODO/RULES.md", "TODO/PROGRESS.md", "TODO/ROADMAP.md"]
MIN_PARA_WORDS = 25


def doc_files(root: Path) -> list[Path]:
    out = []
    for pattern in DOCS + ["crates/*/README.md"]:
        out += sorted(root.glob(pattern))
    return out


def paragraphs(text: str) -> list[tuple[int, str]]:
    """Each paragraph of prose (not a code block, a table or a heading) with
    its first line, as lower-case words."""
    out, para, start, fence = [], [], 0, False
    for n, line in enumerate(text.splitlines() + [""], 1):
        if line.lstrip().startswith("```"):
            fence = not fence
            continue
        if fence or line.startswith("|") or line.startswith("#"):
            continue
        if line.strip():
            if not para:
                start = n
            para.append(line)
        elif para:
            out.append((start, " ".join(w.lower() for w in WORD.findall(" ".join(para)))))
            para = []
    return out


def doc_paragraphs(root: Path) -> list[tuple[str, int, str]]:
    out = []
    for path in doc_files(root):
        rel = path.relative_to(root).as_posix()
        for line, text in paragraphs(path.read_text(encoding="utf-8")):
            if len(text.split()) >= MIN_PARA_WORDS:
                out.append((rel, line, text))
    return out


def design_deps(root: Path) -> dict[str, set[str]]:
    """The column "Pod deps" of each table row of `docs/design.md`,
    section 4, as the set of pod crates it names (`none` is the empty
    set)."""
    text = (root / "docs" / "design.md").read_text(encoding="utf-8")
    sec = text.split("## 4. The crates", 1)[-1].split("\n## 5.", 1)[0]
    out = {}
    head = None
    for line in sec.splitlines():
        if not line.startswith("|"):
            head = None
            continue
        cells = [c.strip() for c in line.strip("|").split("|")]
        if cells and cells[0] == "Crate":
            head = cells
            continue
        if head is None or set(cells[0]) <= set("- "):
            continue
        name = cells[0].strip("`")
        if "Pod deps" not in head or not name.startswith("pod-"):
            continue
        col = cells[head.index("Pod deps")]
        out[name] = set(re.findall(r"\bpod-[a-z0-9]+(?:-[a-z0-9]+)*", col.replace("`", "")))
    return out
