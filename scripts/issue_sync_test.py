"""The self-test of issue-sync.py: a fake API, no network.

  python3 -I scripts/issue-sync.py --self-test

Each check prints `ok` or `FAIL what`; the run exits 1 at a failure.
"""

import copy

BLOCK = "<!-- pod-todo:state -->\nThis issue is {state}.\nDate of the record: {date}.\n<!-- /pod-todo:state -->"


def block(state, date="2026-11-15"):
    return BLOCK.format(state=state, date=date)


def record(number, kind="report", state="open", date="2026-11-15", entries=None):
    entries = entries if entries is not None else [{"id": "T-082", "status": "done", "title": "x", "tag": "v0.8.0"}]
    return {"number": number, "kind": kind, "state": state, "entries": entries, "block": block(state, date)}


def item(number, body, state="open", association="MEMBER", labels=(), pull=False, created="2026-01-01T00:00:00Z",
         closed_by=None, closed_ago_hours=None):
    out = {
        "created_at": created,
        "number": number,
        "body": body,
        "state": state,
        "author_association": association,
        "labels": [{"name": name} for name in labels],
    }
    if pull:
        out["pull_request"] = {}
    if closed_by is not None:
        import datetime as dt

        closed = dt.datetime.now(dt.timezone.utc) - dt.timedelta(hours=closed_ago_hours or 0)
        out["closed_at"] = closed.strftime("%Y-%m-%dT%H:%M:%SZ")
        out["closed_by"] = {"login": closed_by[0], "type": closed_by[1]}
    return out


class Fake:
    """The issue list of a repository in memory, with each write that came."""

    def __init__(self, items):
        self.items = {i["number"]: i for i in copy.deepcopy(items)}
        self.writes = []

    def issues(self):
        return copy.deepcopy(list(self.items.values()))

    def edit(self, number, fields):
        self.writes.append(("edit", number, dict(fields)))
        self.items[number].update({k: v for k, v in fields.items() if k != "state_reason"})

    def comment(self, number, text):
        self.writes.append(("comment", number, text))


def run_sync(sync, rec, items, dry_run=False):
    api, lines = Fake(items), []
    code = sync.sync(rec, api, dry_run, pause=0, out=lines.append)
    return code, api, lines


def run(sync):
    failures = []

    def check(ok, what):
        print(("ok    " if ok else "FAIL  ") + what)
        if not ok:
            failures.append(what)

    # A block is appended to a body that has none; each byte of it stays.
    body = "A report.\r\n\r\n```sh\nrun\n```"
    code, api, _ = run_sync(sync, [record(20)], [item(20, body)])
    new = api.items[20]["body"]
    check(code == 0 and new == body + "\n\n" + block("open"), "the block is appended and the body is kept")

    # A block in the middle is replaced; the text before and after stays.
    body = "Head.\n\n" + block("closed", "2026-11-01") + "\n\nstate: opened at 10:00 (UTC)\n"
    code, api, _ = run_sync(sync, [record(21)], [item(21, body)])
    want = "Head.\n\n" + block("open") + "\n\nstate: opened at 10:00 (UTC)\n"
    check(code == 0 and api.items[21]["body"] == want, "the block is replaced and the rest of the body is kept")

    # A block that differs only in its date is not written again.
    body = "Head.\n\n" + block("open", "2026-11-01")
    code, api, _ = run_sync(sync, [record(22)], [item(22, body)])
    check(code == 0 and not api.writes, "a change of the date alone writes nothing")

    # A closed record closes its issue, with one comment that names each
    # entry and its tag.
    code, api, _ = run_sync(sync, [record(23, state="closed")], [item(23, "Body.")])
    edits = [w for w in api.writes if w[0] == "edit"]
    comments = [w for w in api.writes if w[0] == "comment"]
    check(
        code == 0 and len(edits) == 1 and edits[0][2].get("state") == "closed" and api.items[23]["state"] == "closed",
        "a closed record closes its issue",
    )
    check(
        len(comments) == 1 and "T-082 (in `v0.8.0`)" in comments[0][2] and api.writes[-1][0] == "comment",
        "the close has one comment that names the entry and its tag",
    )

    # A message between sessions closes with no comment.
    code, api, _ = run_sync(sync, [record(24, kind="coord", state="closed", entries=[])], [item(24, "COORD.")])
    check(
        code == 0 and api.items[24]["state"] == "closed" and not [w for w in api.writes if w[0] == "comment"],
        "a coord issue closes with no comment",
    )

    # An open record opens its closed issue again.
    code, api, _ = run_sync(sync, [record(25)], [item(25, "Body.\n\n" + block("open"), state="closed")])
    check(
        code == 0 and api.writes == [("edit", 25, {"state": "open", "state_reason": "reopened"})],
        "an open record opens its closed issue again",
    )

    # An issue that a person closed by hand while the record holds it open
    # stays closed for HAND_HOURS, with a note; the block is still written.
    person = ("someone", "User")
    code, api, lines = run_sync(sync, [record(40)], [item(40, "A.", state="closed", closed_by=person, closed_ago_hours=2)])
    check(
        code == 0
        and not [w for w in api.writes if w[2].get("state")]
        and api.items[40]["state"] == "closed"
        and api.items[40]["body"].endswith(block("open"))
        and any("#40: closed by hand by someone" in x and "close it in the record" in x for x in lines),
        "a close by a person is kept, reported, and the block is written",
    )
    code, api, lines = run_sync(sync, [record(41)], [item(41, "A.", state="closed", closed_by=person, closed_ago_hours=30)])
    check(code == 0 and api.items[41]["state"] == "open", "a close by a person is reverted after the grace")
    bot = ("github-actions[bot]", "Bot")
    code, api, lines = run_sync(sync, [record(42)], [item(42, "A.", state="closed", closed_by=bot, closed_ago_hours=1)])
    check(code == 0 and api.items[42]["state"] == "open", "a close by a bot is reverted at once")
    code, api, lines = run_sync(sync, [record(43)], [item(43, "A.", state="closed")])
    check(code == 0 and api.items[43]["state"] == "open", "a close with no closer is reverted at once")
    code, api, lines = run_sync(
        sync, [record(44)], [item(44, "A.", state="closed", closed_by=person, closed_ago_hours=2)], dry_run=True
    )
    check(
        code == 0 and not api.writes and not any("open again" in x for x in lines),
        "the dry run does not plan an open again inside the grace",
    )

    # The dry run prints each change and writes nothing.
    rec = [record(26, state="closed"), record(27)]
    code, api, lines = run_sync(sync, rec, [item(26, "A."), item(27, "B.", state="closed")], dry_run=True)
    check(code == 0 and not api.writes, "the dry run writes nothing")
    check(
        any(line.startswith("would change #26: write the block, close, comment") for line in lines)
        and any(line.startswith("would change #27: write the block, open again") for line in lines),
        "the dry run prints each change",
    )

    # An issue of a stranger is not touched until it is confirmed.
    code, api, lines = run_sync(sync, [record(28, state="closed")], [item(28, "Spam.", association="NONE")])
    check(code == 0 and not api.writes and api.items[28]["body"] == "Spam.", "a stranger's issue is not touched")
    check(any("#28: not changed: its author is outside the team" in line for line in lines), "it is reported")
    code, api, _ = run_sync(sync, [record(29)], [item(29, "Real.", association="NONE", labels=["confirmed"])])
    check(code == 0 and api.writes and api.items[29]["body"].endswith(block("open")), "a confirmed issue is synced")

    # Drift that the sync cannot fix fails the run and writes nothing for it.
    code, api, lines = run_sync(sync, [record(30)], [item(30, "A."), item(31, "New.")])
    check(code == 1 and any("#31 is open on GitHub and has no place in the record" in x for x in lines),
          "an open issue with no place in the record fails")
    import datetime as _dt
    fresh = (_dt.datetime.now(_dt.timezone.utc) - _dt.timedelta(hours=5)).strftime("%Y-%m-%dT%H:%M:%SZ")
    code, api, lines = run_sync(sync, [record(30)], [item(30, "A."), item(36, "New.", created=fresh)])
    check(code == 0 and any("#36 is new and has no place in the record yet" in x for x in lines),
          "a new issue with no place in the record yet is a note")
    code, api, lines = run_sync(sync, [record(32)], [item(32, "A " + block("open") + " " + block("open"))])
    check(code == 1 and not api.writes, "a body with two blocks fails and is not written")
    code, api, lines = run_sync(sync, [record(33)], [item(33, "", pull=True)])
    check(code == 1 and not api.writes, "a number of the record that is a pull request on GitHub fails")

    # A reference: the block only; the sync never closes or opens it.
    code, api, _ = run_sync(sync, [record(37, kind="reference", state="closed")], [item(37, "A capture.")])
    check(code == 0 and api.items[37]["state"] == "open" and api.items[37]["body"].endswith(block("closed")),
          "a reference gets its block and keeps its state")

    # A pull request and a deleted issue of the record are not touched.
    rec = [record(34, kind="pull", state="open", entries=[]), record(35, state="deleted")]
    code, api, _ = run_sync(sync, rec, [item(34, "PR", pull=True)])
    check(code == 0 and not api.writes, "a pull request and a deleted issue are not touched")

    print(f"issue-sync: self-test: {len(failures)} failures")
    return 1 if failures else 0
