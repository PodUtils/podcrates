#!/usr/bin/env python3
"""The merge driver of `TODO/INDEX.md` and `TODO/PROGRESS.md`.

    python3 -I scripts/agent/merge_todo.py BASE OURS THEIRS PATH
    python3 -I scripts/agent/merge_todo.py --self-test

Git calls it for a merge of the branch of an agent (`.gitattributes`;
`scripts/session-start.sh` sets `merge.podcrates-todo.driver`). It writes
the result into OURS. Exit 0 for a clean merge, 1 when a conflict stays
(with the markers of git), 2 when it cannot run.

Each agent adds rows and changes the counts in the same lines, so a text
merge conflicts at each merge. This driver merges the rows of the index
one id at a time (a side that left a row as it was takes the row of the
other side; two different changes of one row are a conflict), sorts them
by id, and derives the counts again from the rows. The rest of the text
is a normal 3-way merge (`git merge-file`), with the counts left out.
In `TODO/PROGRESS.md` the counts are OURS + THEIRS - BASE.
`cargo todo check` after the merge checks the result.
"""

import os
import re
import subprocess
import sys
import tempfile

ROW = re.compile(r"^\| \[T-(\d+)\]\(")
PHRASE = re.compile(r"(\d+) entries: (\d+) open, (\d+) partial, (\d+) blocked, (\d+) done\.")
COUNT_ROW = re.compile(r"^\| (P[0-3]|\*\*All\*\*) \|")
MARK_ROWS = "@@merge-todo rows@@"
MARK_PHRASE = "@@merge-todo counts@@"
STATUSES = ("open", "partial", "blocked", "done")


def merge_text(ours, base, theirs):
    """A 3-way merge of three texts by `git merge-file`: (text, conflicts)."""
    with tempfile.TemporaryDirectory() as tmp:
        paths = []
        for name, text in (("ours", ours), ("base", base), ("theirs", theirs)):
            path = os.path.join(tmp, name)
            with open(path, "w", encoding="utf-8", newline="") as f:
                f.write(text)
            paths.append(path)
        run = subprocess.run(
            ["git", "merge-file", "-p", "-L", "ours", "-L", "base", "-L", "theirs", *paths],
            capture_output=True,
            check=False,
        )
    if run.returncode < 0 or run.returncode > 127:
        raise OSError(run.stderr.decode(errors="replace").strip() or "git merge-file failed")
    return run.stdout.decode("utf-8"), run.returncode


def split_index(text):
    """The rows by id, and the other lines with one mark where the rows
    were and one mark for each line of counts."""
    rows, rest, marked = {}, [], False
    for line in text.splitlines(keepends=True):
        m = ROW.match(line)
        if m:
            rows[int(m.group(1))] = line
            if not marked:
                rest.append(MARK_ROWS + "\n")
                marked = True
        elif PHRASE.search(line) and line.startswith("**"):
            rest.append(MARK_PHRASE + "\n")
        elif COUNT_ROW.match(line):
            rest.append("@@merge-todo " + COUNT_ROW.match(line).group(1) + "@@\n")
        else:
            rest.append(line)
    return rows, "".join(rest)


def cells(row):
    parts = [c.strip() for c in row.strip().strip("|").split("|")]
    return parts[1], parts[5]


def counts(rows):
    """{priority or "All": [open, partial, blocked, done]}."""
    table = {p: [0, 0, 0, 0] for p in ("P0", "P1", "P2", "P3", "All")}
    for row in rows:
        priority, status = cells(row)
        if priority in table and status in STATUSES:
            table[priority][STATUSES.index(status)] += 1
            table["All"][STATUSES.index(status)] += 1
    return table


def phrase(c):
    return f"{sum(c)} entries: {c[0]} open, {c[1]} partial, {c[2]} blocked, {c[3]} done."


def merge_index(base, ours, theirs):
    (rb, tb), (ro, to), (rt, tt) = split_index(base), split_index(ours), split_index(theirs)
    text, conflicts = merge_text(to, tb, tt)
    merged, lines = [], []
    for n in sorted(set(rb) | set(ro) | set(rt)):
        b, o, t = rb.get(n), ro.get(n), rt.get(n)
        if o == t or t == b:
            keep = o
        elif o == b:
            keep = t
        else:
            conflicts += 1
            lines += ["<<<<<<< ours\n", o or "", "=======\n", t or "", ">>>>>>> theirs\n"]
            merged.append(o or t)
            continue
        if keep is not None:
            merged.append(keep)
            lines.append(keep)
    table = counts(merged)
    text = text.replace(MARK_ROWS + "\n", "".join(lines), 1)
    text = text.replace(MARK_PHRASE + "\n", "**" + phrase(table["All"]) + "**\n")
    for label in ("P0", "P1", "P2", "P3", "All"):
        shown = "**All**" if label == "All" else label
        c = table[label]
        row = f"| {shown} | {c[0]} | {c[1]} | {c[2]} | {c[3]} | {sum(c)} |\n"
        text = text.replace("@@merge-todo " + shown + "@@\n", row)
    return text, conflicts


def merge_progress(base, ours, theirs):
    found = [PHRASE.search(t) for t in (base, ours, theirs)]
    if not all(found):
        return merge_text(ours, base, theirs)
    b, o, t = ([int(g) for g in m.groups()[1:]] for m in found)
    total = [o[i] + t[i] - b[i] for i in range(4)]
    blank = [PHRASE.sub(MARK_PHRASE, x, count=1) for x in (base, ours, theirs)]
    text, conflicts = merge_text(blank[1], blank[0], blank[2])
    return text.replace(MARK_PHRASE, phrase(total)), conflicts


def merge(base, ours, theirs, path):
    """Merge the three files and write the result into `ours`."""
    texts = []
    for p in (base, ours, theirs):
        with open(p, encoding="utf-8", newline="") as f:
            texts.append(f.read())
    if os.path.basename(path) == "INDEX.md":
        text, conflicts = merge_index(*texts)
    else:
        text, conflicts = merge_progress(*texts)
    with open(ours, "w", encoding="utf-8", newline="") as f:
        f.write(text)
    return 1 if conflicts else 0


def index_text(rows, prose="Rows.\n"):
    c = counts(rows)
    table = "".join(
        f"| {'**All**' if p == 'All' else p} | {c[p][0]} | {c[p][1]} | {c[p][2]} | {c[p][3]} | {sum(c[p])} |\n"
        for p in ("P0", "P1", "P2", "P3", "All")
    )
    return (
        "# Index\n\n" + prose + "\n## Entries\n\n**" + phrase(c["All"]) + "**\n\n"
        "| Priority | open | partial | blocked | done | total |\n| --- | --- | --- | --- | --- | --- |\n"
        + table
        + "\n| ID | Priority | Effort | Milestone | Category | Status | Item |\n"
        "| --- | --- | --- | --- | --- | --- | --- |\n" + "".join(rows)
    )


def row(n, status, priority="P2", title="x"):
    return f"| [T-{n:03}](upkeep.md) | {priority} | S | M5 | chore | {status} | {title} |\n"


def progress_text(rows, work="Work: T-001.\n"):
    return "# Progress\n\n`TODO/INDEX.md` holds " + phrase(counts(rows)["All"]) + "\n\n" + work


def self_test():
    ok = True

    def expect(cond, what):
        nonlocal ok
        print(("ok    " if cond else "FAIL  ") + what)
        ok = ok and cond

    base = [row(1, "open"), row(2, "open"), row(3, "done", "P1")]
    ours = [row(1, "done"), row(2, "open"), row(3, "done", "P1"), row(7, "open")]
    theirs = [row(1, "open"), row(2, "partial"), row(3, "done", "P1"), row(5, "open", "P3")]
    want = [row(1, "done"), row(2, "partial"), row(3, "done", "P1"), row(5, "open", "P3"), row(7, "open")]
    text, n = merge_index(index_text(base), index_text(ours), index_text(theirs))
    expect(n == 0 and text == index_text(want), "new rows of both sides, adjacent changes: sorted rows, true counts")
    text, n = merge_index(index_text(base), index_text(ours, "Rows, ours.\n"), index_text(base, "Rows, theirs.\n"))
    expect(n == 1 and "<<<<<<< ours" in text and "Rows, theirs." in text, "two changes of one line of prose conflict")
    theirs2 = [row(1, "partial"), row(2, "open"), row(3, "done", "P1")]
    text, n = merge_index(index_text(base), index_text(ours), index_text(theirs2))
    expect(n == 1 and text.count("T-001") == 2 and "<<<<<<< ours\n" + row(1, "done") in text, "two changes of one row conflict")
    gone = [row(1, "open"), row(3, "done", "P1")]
    text, n = merge_index(index_text(base), index_text(gone), index_text(base))
    expect(n == 0 and "T-002" not in text and text == index_text(gone), "a row that one side removed goes")
    text, n = merge_progress(progress_text(base), progress_text(ours), progress_text(theirs))
    expect(n == 0 and text == progress_text(want), "the counts of the progress page are ours + theirs - base")
    text, n = merge_progress(progress_text(base), progress_text(ours, "Work: T-007.\n"), progress_text(theirs, "Work: T-005.\n"))
    expect(n == 1 and "<<<<<<< ours" in text, "two changes of the work order conflict")

    with tempfile.TemporaryDirectory() as tmp:
        here = os.path.abspath(__file__)

        def git(*args):
            return subprocess.run(
                ["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", *args],
                cwd=tmp, capture_output=True, text=True, check=False,
            )

        def commit(rows, msg, work="Work: T-001.\n"):
            os.makedirs(os.path.join(tmp, "TODO"), exist_ok=True)
            for name, text in (("INDEX.md", index_text(rows)), ("PROGRESS.md", progress_text(rows, work))):
                with open(os.path.join(tmp, "TODO", name), "w") as f:
                    f.write(text)
            git("add", "-A")
            git("commit", "-q", "-m", msg)

        git("init", "-q", "-b", "main")
        with open(os.path.join(tmp, ".gitattributes"), "w") as f:
            f.write("TODO/INDEX.md merge=podcrates-todo\nTODO/PROGRESS.md merge=podcrates-todo\n")
        git("config", "merge.podcrates-todo.driver", f"python3 -I {here} %O %A %B %P")
        commit(base, "base")
        git("checkout", "-q", "-b", "agent")
        commit(theirs, "agent")
        git("checkout", "-q", "main")
        commit(ours, "lead")
        run = git("merge", "-q", "--no-edit", "agent")
        with open(os.path.join(tmp, "TODO", "INDEX.md")) as f:
            got_index = f.read()
        with open(os.path.join(tmp, "TODO", "PROGRESS.md")) as f:
            got_progress = f.read()
        expect(run.returncode == 0, "git merge of the branch of an agent ends with no conflict")
        expect(got_index == index_text(want) and got_progress == progress_text(want), "git merge writes the merged rows and counts")
        git("config", "--unset", "merge.podcrates-todo.driver")
        git("reset", "-q", "--hard", "HEAD~1")
        run = git("merge", "-q", "--no-edit", "agent")
        expect(run.returncode != 0, "with no driver, the same merge conflicts")
        git("merge", "--abort")
    print("merge-todo self-test:", "passed" if ok else "FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        sys.exit(self_test())
    if len(sys.argv) != 5:
        print("usage: merge_todo.py BASE OURS THEIRS PATH | --self-test", file=sys.stderr)
        sys.exit(2)
    try:
        sys.exit(merge(*sys.argv[1:]))
    except (OSError, UnicodeDecodeError) as e:
        print(f"merge-todo: {e}", file=sys.stderr)
        sys.exit(2)
