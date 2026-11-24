#!/usr/bin/env python3
"""Make the issues on GitHub agree with the work record (the owner's ruling
"Issues").

  python3 -I scripts/issue-sync.py [--root DIR] [--todo PROGRAM] [--repo OWNER/REPO] [--dry-run]
  python3 -I scripts/issue-sync.py --self-test

The record gives each issue its kind, its state and its status block
(`pod-todo issues --json`, run as PROGRAM, default `cargo run -q -p pod-todo
--`, in DIR). For each issue of the record, the script:

* writes the block into the body, between `<!-- pod-todo:state -->` and
  `<!-- /pod-todo:state -->`: it replaces the block, or appends it, and
  keeps each other byte of the body. A block that differs only in its
  date is not written again;
* closes an issue that the record says is closed, with one comment that
  names the entries and their tags (a report, a deferred issue or a
  tracker; a message and a request of a run get no comment);
* opens again an issue that the record says is open, except for HAND_HOURS
  after a person closed it on GitHub by hand: it reports that close, so that
  a session changes the record first (call C924).

It does not edit an issue whose author is outside the team
(`author_association` not OWNER, MEMBER or COLLABORATOR) and that has no
label `confirmed` (the ruling "Strangers"): it reports it. It never edits a
pull request.

It fails on drift that it cannot fix: an open issue on GitHub with no place
in the record and older than GRACE_DAYS (a newer one is a note: a session
opens a request or a message before its record can map it), a number of the record that GitHub does not have as an
issue, and a body with a broken block.

--dry-run prints each change and changes nothing. The API is
ISSUE_SYNC_API (default https://api.github.com), with GITHUB_TOKEN. The
repository is --repo, else GITHUB_REPOSITORY.

Exit 0 when GitHub agrees (or was made to agree), 1 at drift or at a write
that failed, 2 when the record or GitHub cannot be read.
"""

import argparse
import datetime
import json
import os
import shlex
import subprocess
import sys
import time
import urllib.error
import urllib.request

START = "<!-- pod-todo:state -->"
END = "<!-- /pod-todo:state -->"
DATE = "Date of the record: "
TEAM = {"OWNER", "MEMBER", "COLLABORATOR"}
CONFIRMED = "confirmed"
# The kinds whose state follows their entries: a close tells their reader why.
COMMENTED = {"report", "deferred", "tracker"}
# The kinds that their session opens and closes by hand: the sync writes the
# block only.
BY_HAND = {"reference"}


class Unreadable(Exception):
    pass


class Broken(Exception):
    pass


def with_block(body, block):
    """The body with the block in place of its old block, or appended. Each
    other byte stays."""
    body = body or ""
    starts, ends = body.count(START), body.count(END)
    if starts == 0 and ends == 0:
        if not body:
            return block
        return body + ("\n" if body.endswith("\n") else "\n\n") + block
    if starts != 1 or ends != 1:
        raise Broken(f"the body has {starts} start markers and {ends} end markers of the block")
    i, j = body.index(START), body.index(END)
    if j < i:
        raise Broken("the end marker of the block comes before its start marker")
    return body[:i] + block + body[j + len(END):]


def without_date(text):
    return "\n".join(line for line in text.replace("\r\n", "\n").split("\n") if not line.startswith(DATE))


def trusted(item):
    """True for an author of the team, or an issue that its session
    confirmed."""
    labels = [label.get("name") for label in item.get("labels") or []]
    return item.get("author_association") in TEAM or CONFIRMED in labels


def comment_text(issue):
    parts = []
    for e in issue["entries"]:
        tag = e.get("tag") or ""
        where = "in the next tag" if tag == "next" else f"in `{tag}`" if tag else "in no known tag"
        parts.append(f"{e['id']} ({where})")
    return (
        "The work record of this repository closes this issue. Each entry is done: "
        + ", ".join(parts)
        + ". The block in the body gives the state. Write a new issue if the work is not correct."
    )


GRACE_DAYS = 3
# The time that a close by a person is kept, with the record open, before the
# sync opens the issue again: a session changes the record in that time.
HAND_HOURS = 24


def age_days(item, now):
    """The age of an item in days from its `created_at`, or None."""
    try:
        made = datetime.datetime.fromisoformat((item.get("created_at") or "").replace("Z", "+00:00"))
    except ValueError:
        return None
    return (now - made).total_seconds() / 86400


def closed_by_hand(item, now):
    """(login, hours since the close) of an issue that a person closed less
    than HAND_HOURS ago, else None. A bot, an unknown closer and a close of
    another age give None."""
    closer = item.get("closed_by") or {}
    if item.get("state") != "closed" or not closer or closer.get("type") == "Bot":
        return None
    try:
        closed = datetime.datetime.fromisoformat((item.get("closed_at") or "").replace("Z", "+00:00"))
    except ValueError:
        return None
    hours = (now - closed).total_seconds() / 3600
    if 0 <= hours < HAND_HOURS:
        return closer.get("login") or "a person", hours
    return None


def plan(record, items, now=None):
    """The changes that make GitHub agree with the record: (actions, notes,
    faults). An action is (number, fields to change, comment or None, what)."""
    now = now or datetime.datetime.now(datetime.timezone.utc)
    actions, notes, faults = [], [], []
    by_number = {int(i["number"]): i for i in items}
    known = {r["number"] for r in record}
    for n in sorted(by_number):
        item = by_number[n]
        if "pull_request" not in item and item.get("state") == "open" and n not in known:
            age = age_days(item, now)
            if age is not None and age < GRACE_DAYS:
                notes.append(f"#{n} is new and has no place in the record yet: map it (TODO/RULES.md)")
            else:
                faults.append(f"#{n} is open on GitHub and has no place in the record: map it (TODO/RULES.md)")
    for issue in sorted(record, key=lambda r: r["number"]):
        n, kind, state = issue["number"], issue["kind"], issue["state"]
        if kind == "pull" or state == "deleted":
            continue
        item = by_number.get(n)
        if item is None or "pull_request" in item:
            faults.append(f"#{n} is an issue of the record, and GitHub has no such issue")
            continue
        if not trusted(item):
            notes.append(
                f"#{n}: not changed: its author is outside the team; verify it, then add the label `{CONFIRMED}`"
            )
            continue
        body = item.get("body") or ""
        try:
            new = with_block(body, issue["block"])
        except Broken as e:
            faults.append(f"#{n}: {e}: repair the body by hand, then the sync writes the block")
            continue
        fields, what, comment = {}, [], None
        if without_date(new) != without_date(body):
            fields["body"] = new
            what.append("write the block")
        if kind in BY_HAND:
            pass
        elif state == "closed" and item.get("state") == "open":
            fields["state"], fields["state_reason"] = "closed", "completed"
            what.append("close")
            if kind in COMMENTED:
                comment = comment_text(issue)
                what.append("comment")
        elif state == "open" and item.get("state") == "closed":
            hand = closed_by_hand(item, now)
            if hand:
                notes.append(
                    f"#{n}: closed by hand by {hand[0]} {hand[1]:.1f} h ago, and the record holds it open: "
                    f"the sync opens it again after {HAND_HOURS} h; close it in the record, or ask the owner "
                    "(TODO/RULES.md, section 4)"
                )
            else:
                fields["state"], fields["state_reason"] = "open", "reopened"
                what.append("open again")
        if fields:
            actions.append((n, fields, comment, ", ".join(what)))
    return actions, notes, faults


class Api:
    def __init__(self, base, repo, token):
        self.base, self.repo, self.token = base.rstrip("/"), repo, token

    def call(self, method, path, data=None):
        url = f"{self.base}/repos/{self.repo}/{path}"
        body = None if data is None else json.dumps(data).encode()
        request = urllib.request.Request(url, data=body, method=method)
        request.add_header("Accept", "application/vnd.github+json")
        request.add_header("User-Agent", "issue-sync")
        if self.token:
            request.add_header("Authorization", f"Bearer {self.token}")
        if body is not None:
            request.add_header("Content-Type", "application/json")
        last = None
        for _ in range(3 if method == "GET" else 1):
            try:
                with urllib.request.urlopen(request, timeout=60) as response:
                    return json.load(response)
            except (urllib.error.URLError, TimeoutError, OSError, ValueError) as e:
                last = e
        raise Unreadable(f"{method} {url}: {last}")

    def issues(self):
        items = []
        for page in range(1, 1000):
            got = self.call("GET", f"issues?state=all&per_page=100&page={page}")
            if not isinstance(got, list):
                raise Unreadable(f"the issue list is not a list: {str(got)[:200]}")
            items += got
            if len(got) < 100:
                return items
        raise Unreadable("more than 999 pages of issues")

    def edit(self, number, fields):
        self.call("PATCH", f"issues/{number}", fields)

    def comment(self, number, text):
        self.call("POST", f"issues/{number}/comments", {"body": text})


def sync(record, api, dry_run, pause=1.0, out=print):
    """Print and apply the plan. Returns the exit code."""
    actions, notes, faults = plan(record, api.issues())
    for n, _, _, what in actions:
        out(f"{'would ' if dry_run else ''}change #{n}: {what}")
    for line in notes:
        out(f"note  {line}")
    failed = 0
    if not dry_run:
        for n, fields, comment, _ in actions:
            try:
                api.edit(n, fields)
                if comment:
                    api.comment(n, comment)
            except Unreadable as e:
                failed += 1
                out(f"FAIL  #{n}: the write failed: {e}")
            if pause:
                time.sleep(pause)
    for line in faults:
        out(f"FAIL  {line}")
    out(f"issue-sync: {len(actions)} changes{' to make' if dry_run else ''}, {len(notes)} notes, "
        f"{len(faults) + failed} faults")
    return 1 if faults or failed else 0


def read_record(root, program):
    argv = shlex.split(program) + ["--root", root, "issues", "--json"]
    try:
        done = subprocess.run(argv, capture_output=True, text=True, timeout=1800, cwd=root)
    except (OSError, subprocess.TimeoutExpired) as e:
        raise Unreadable(f"{program}: {e}") from e
    if done.returncode != 0:
        raise Unreadable(f"{program} issues --json exited {done.returncode}:\n{done.stderr.strip()}")
    try:
        return json.loads(done.stdout)
    except ValueError as e:
        raise Unreadable(f"{program} issues --json: not JSON: {e}") from e


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--root", default=".")
    parser.add_argument("--todo", default="cargo run -q -p pod-todo --")
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY"))
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        import importlib.util

        here = os.path.join(os.path.dirname(os.path.abspath(__file__)), "issue_sync_test.py")
        spec = importlib.util.spec_from_file_location("issue_sync_test", here)
        test = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(test)
        return test.run(sys.modules[__name__])
    token = os.environ.get("GITHUB_TOKEN", "")
    if not args.repo:
        print("issue-sync: give --repo OWNER/REPO, or set GITHUB_REPOSITORY", file=sys.stderr)
        return 2
    if not token and not args.dry_run:
        print("issue-sync: a sync needs GITHUB_TOKEN; --dry-run needs none", file=sys.stderr)
        return 2
    api = Api(os.environ.get("ISSUE_SYNC_API", "https://api.github.com"), args.repo, token)
    try:
        return sync(read_record(args.root, args.todo), api, args.dry_run)
    except Unreadable as e:
        print(f"issue-sync: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
