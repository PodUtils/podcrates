#!/usr/bin/env python3
"""The one watcher of a session of PodUtils: issues, pull requests and
Actions runs, for each repository of the organization.

    python3 -I watch.py ME [--state DIR] [--every SECONDS] [--runs REPO]...
                           [--org ORG]
    python3 -I watch.py --self-test     (needs watch_test.py beside it)

ME is the name of the reader's repository (podcrates, podssh, podnet,
podvm, podrunner, ...). Each poll prints one line for each event:

* an issue or a pull request of ORG/ME (from anyone): new, commented,
  closed, reopened, merged; a pull request also when it changes;
* an issue or a pull request of another repository of ORG that is meant
  for ME: its title holds `(to ME)` or `(to all)`, or it has the label ME.
  A COORD message (its title starts with `COORD-` or holds `(to `) is also
  reported when its writer amends its body;
* each comment of such an item that is new or edited, and each review
  comment of a pull request of ORG/ME. An edit of a comment changes the
  `updated_at` of its item, so the watcher reads the comments of each item
  that changed, since the last poll;
* the trust of each item (the owner's ruling "Strangers"): an author whose
  `author_association` is OWNER, MEMBER or COLLABORATOR is of the team.
  An item of ORG/ME by another author has the tag `[outside: verify, then
  confirm or edit]`: the session of ME checks it and adds the label
  `confirmed`, edits it, or closes it. An item of another repository by an
  author outside the team is not shown until its own session adds the
  label `confirmed` (only the team can add a label). A comment of an
  author outside the team has the tag on ORG/ME, and is not shown on
  another repository;
* each Actions run of each REPO of `--runs` (default ORG/ME) that starts,
  ends (with the names of its failed jobs) or runs again.

The first poll records what exists and prints nothing for it. STATE (by
default `.work/agent` under the current directory) keeps `watch.json` and a
copy of each message in `issues/`, as GitHub deletes the issues of a
suspended account. Run it under the Monitor tool of the session and start
it again when the monitor ends.

The reads go through forge.web.pkgforge.dev/github, a keyless read-only
mirror of GitHub's REST API (https://forge.web.pkgforge.dev/llms.txt). One
search finds the issues and one the pull requests of the organization that
changed since the last poll, with a margin for the lag of the index. The
quota is shared, so the watcher polls each SECONDS (default 120) with
jitter. A read that fails gets one more short try, then the poll goes on:
a long back-off misses the end of a run. A silence of SILENCE seconds is
said once, and its end too. Python's standard library only.
"""

import hashlib
import json
import os
import random
import sys
import http.client
import time
import urllib.error
import urllib.parse
import urllib.request

MIRROR = "https://forge.web.pkgforge.dev/github"
TEAM = ("OWNER", "MEMBER", "COLLABORATOR")
CONFIRMED = "confirmed"
TRANSIENT = (403, 429, 500, 502, 503, 504)
# The conclusions of a run, a job or a step that are no failure.
GREEN = ("success", "skipped", "neutral")
TRIES = 2
SILENCE = 600
KEEP_RUNS = 200
KEEP_COMMENTS = 2000
TEAM_TAG = " [outside: verify, then confirm or edit]"


def read(path, opener=None):
    """One read of `path` from the mirror, with a changing query, as
    (document, code, retry_after, why). The document is None when the read
    failed; the code is the HTTP status of a refusal, or 0 when no answer
    came; retry_after is the `Retry-After` header of a refusal; why is the
    cause in a few words. `runs.py` builds its own waits on this read."""
    sep = "&" if "?" in path else "?"
    url = f"{MIRROR}/{path}{sep}n={random.randint(0, 1 << 40)}"
    req = urllib.request.Request(url, headers={"Accept": "application/json", "User-Agent": "podutils-watch"})
    try:
        with (opener or urllib.request.urlopen)(req, timeout=30) as r:
            return json.load(r), 200, None, ""
    except urllib.error.HTTPError as e:
        return None, e.code, (e.headers or {}).get("Retry-After"), f"HTTP {e.code}"
    # A cut answer (http.client.IncompleteRead) is a failure to try again.
    except (urllib.error.URLError, http.client.HTTPException, TimeoutError, ValueError, OSError) as e:
        return None, 0, None, type(e).__name__


def get(path, opener=None):
    """The JSON of `path` from the mirror, or None after TRIES failures."""
    for attempt in range(TRIES):
        time.sleep(attempt * (5 + random.uniform(0, 5)))
        doc, code, _, _ = read(path, opener)
        if doc is not None:
            return doc
        if code and code not in TRANSIENT:
            return None
    return None


# Issues and pull requests.


def meant(me, org, repo, item):
    """True when the item is for ME: of ORG/ME, or addressed to ME."""
    if repo == f"{org}/{me}":
        return True
    title = item.get("title") or ""
    labels = [label.get("name") for label in item.get("labels") or []]
    return f"(to {me})" in title or "(to all)" in title or me in labels


def trusted(item):
    """True when the author is of the team, or the session of the item's
    repository confirmed it with the label CONFIRMED."""
    labels = [label.get("name") for label in item.get("labels") or []]
    return item.get("author_association") in TEAM or CONFIRMED in labels


def coord(item):
    title = item.get("title") or ""
    return title.startswith("COORD-") or "(to " in title


def facts(item):
    """The facts of an item that the watcher compares between two polls."""
    pr = item.get("pull_request")
    return {
        "updated": item.get("updated_at") or "",
        "comments": int(item.get("comments") or 0),
        "state": item.get("state") or "",
        "merged": bool(pr and pr.get("merged_at")),
        "body": hashlib.sha256(((item.get("title") or "") + "\n" + (item.get("body") or "")).encode()).hexdigest()[:16],
    }


def words(before, now, item, own):
    """The event words of one item between two polls: NEW, CLOSED,
    MERGED, REOPENED, AMENDED (the title or the body of a COORD message
    changed), CHANGED (the same, of a pull request of ORG/ME). A comment is
    a line of its own (`comments_step`); a comment moves the `updated_at`
    of its item, so the text of an item is compared by its hash. An edit of
    an issue that is not a COORD message is no event (a writer keeps tables
    there)."""
    if before is None:
        return ["NEW"]
    out = []
    if before["state"] != now["state"]:
        if now["state"] == "closed":
            out.append("MERGED" if now["merged"] else "CLOSED")
        else:
            out.append("REOPENED")
    if before.get("body") not in (None, now["body"]):
        if coord(item):
            out.append("AMENDED")
        elif "pull_request" in item and own:
            out.append("CHANGED")
    return out


def issue_line(word, repo, item):
    kind = "PR" if "pull_request" in item else "issue"
    user = (item.get("user") or {}).get("login")
    tag = "" if trusted(item) else TEAM_TAG
    return f"{word} {kind} {repo}#{item.get('number')} [{item.get('state')}] by {user}{tag}: {item.get('title')}"


def keep(state_dir, repo, item):
    """A copy of the message, as GitHub deletes the issues of a suspended
    account."""
    keys = ("number", "title", "body", "state", "created_at", "updated_at", "html_url")
    copy = {k: item.get(k) for k in keys}
    copy["repo"] = repo
    copy["user"] = (item.get("user") or {}).get("login")
    copy["labels"] = [label.get("name") for label in item.get("labels") or []]
    copy["pull_request"] = "pull_request" in item
    copy["author_association"] = item.get("author_association")
    folder = os.path.join(state_dir, "issues")
    os.makedirs(folder, exist_ok=True)
    path = os.path.join(folder, f"{repo.replace('/', '_')}_{item.get('number')}.json")
    save(path, copy)


def issues_step(me, org, repo, items, seen, announce, state_dir=None):
    """The lines of one repository's issue list; `seen` maps
    `REPO#N` to the facts of the last poll."""
    out = []
    own = repo == f"{org}/{me}"
    for item in reversed(items):
        if not isinstance(item, dict) or not meant(me, org, repo, item):
            continue
        if not own and not trusted(item):
            # Not recorded: when its session confirms it, it is new here.
            continue
        key = f"{repo}#{item.get('number')}"
        now = facts(item)
        found = words(seen.get(key), now, item, own)
        seen[key] = now
        if not found:
            continue
        if state_dir:
            keep(state_dir, repo, item)
        if announce:
            out.append(issue_line(" ".join(found), repo, item))
    return out


# Comments.


def comments_step(me, org, repo, comments, items, seen, announce):
    """The lines of one repository's comment list (issue comments, and
    review comments of a pull request), each new or edited since the last
    poll. `items` holds the keys `REPO#N` of the items shown to ME;
    `seen` maps `REPO#N/ID` to the `updated_at` of the comment."""
    out = []
    own = repo == f"{org}/{me}"
    for c in reversed(comments):
        if not isinstance(c, dict):
            continue
        url = c.get("issue_url") or c.get("pull_request_url") or ""
        number = url.rstrip("/").rsplit("/", 1)[-1]
        item = f"{repo}#{number}"
        team = c.get("author_association") in TEAM
        if not own and (item not in items or not team):
            continue
        key = f"{item}/{c.get('id')}"
        stamp = c.get("updated_at") or ""
        before = seen.get(key)
        seen[key] = stamp
        if before == stamp:
            continue
        word = "new" if before is None and stamp == (c.get("created_at") or stamp) else "edited"
        if announce:
            kind = "review comment" if "pull_request_url" in c else "comment"
            user = (c.get("user") or {}).get("login")
            text = " ".join((c.get("body") or "").split())
            text = text[:100] + ("..." if len(text) > 100 else "")
            tag = "" if team else TEAM_TAG
            out.append(f"COMMENT {word}: {kind} {key} by {user}{tag}: {text}")
    for key in sorted(seen, key=lambda k: int(k.rsplit("/", 1)[1]))[:-KEEP_COMMENTS]:
        del seen[key]
    return out


# Actions runs.


def red_jobs(doc):
    """The jobs of a job list that ended and are not green, as (id, name,
    conclusion, the names of the failed steps)."""
    out = []
    for j in (doc or {}).get("jobs", []):
        if j.get("conclusion") in GREEN + (None,):
            continue
        steps = [s.get("name") for s in j.get("steps") or [] if s.get("conclusion") not in GREEN + (None,)]
        out.append((j.get("id"), j.get("name"), j.get("conclusion"), steps))
    return out


def failed_jobs(repo, run, fetch=get):
    doc = fetch(f"repos/{repo}/actions/runs/{run['id']}/jobs?filter=latest&per_page=100")
    return [name for _, name, _, _ in red_jobs(doc)]


def run_line(repo, run, word, jobs=()):
    sha = (run.get("head_sha") or "")[:7]
    attempt = run.get("run_attempt") or 1
    tail = f", attempt {attempt}" if attempt > 1 else ""
    end = f": {run.get('conclusion')}" if run.get("status") == "completed" else ""
    failed = f" (failed: {', '.join(jobs)})" if jobs else ""
    return f"RUN {word}: {repo} {run.get('name')} {run.get('id')} ({run.get('event')}, {sha}{tail}){end}{failed}"


def runs_step(repo, runs, seen, announce, fetch=get):
    """The lines of one repository's run list; `seen` maps `REPO RUN_ID`
    to [attempt, status]."""
    out = []
    for run in reversed(runs):
        key = f"{repo} {run['id']}"
        attempt, status = run.get("run_attempt") or 1, run.get("status")
        before = seen.get(key)
        word = None
        if before is None:
            word = "ended" if status == "completed" else "started"
        elif attempt > before[0]:
            word = "ended" if status == "completed" else "rerun"
        elif before[1] != "completed" and status == "completed":
            word = "ended"
        if word and announce:
            red = status == "completed" and run.get("conclusion") == "failure"
            out.append(run_line(repo, run, word, failed_jobs(repo, run, fetch) if red else []))
        seen[key] = [attempt, status]
    mine = sorted((k for k in seen if k.startswith(f"{repo} ")), key=lambda k: int(k.split()[1]))
    for key in mine[:-KEEP_RUNS]:
        del seen[key]
    return out


class Checks:
    """The checks of a self-test (`watch_test.py`, `runs.py`): one line
    for each check; `ok` is False after a failed one."""

    def __init__(self):
        self.ok = True

    def __call__(self, what, got, want):
        good = got == want
        self.ok &= good
        print(f"{'ok  ' if good else 'FAIL'} {what}" + ("" if good else f": {got!r}, not {want!r}"))


# State and the loop.


def load(path):
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError):
        return None


def save(path, data):
    tmp = f"{path}.{os.getpid()}.tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=1)
    os.replace(tmp, path)


def search(org, kind, since, pages, fetch=get):
    """The issues (kind `issue`) or pull requests (`pr`) of ORG updated at
    or after SINCE, newest first: one search for the whole organization,
    in place of a list for each repository. None when a read failed."""
    q = urllib.parse.quote(f"org:{org} is:{kind} updated:>={since}")
    out = []
    for page in range(1, pages + 1):
        doc = fetch(f"search/issues?q={q}&sort=updated&order=desc&per_page=100&page={page}")
        if not isinstance(doc, dict):
            return None
        out += doc.get("items") or []
        if len(doc.get("items") or []) < 100:
            break
    return out


def stamp(t):
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(t))


def options(argv):
    opts = {"me": None, "state": ".work/agent", "every": 120, "runs": [], "org": "PodUtils"}
    rest = list(argv)
    while rest:
        a = rest.pop(0)
        if a in ("--state", "--every", "--runs", "--org") and rest:
            v = rest.pop(0)
            if a == "--runs":
                opts["runs"].append(v)
            else:
                opts[a[2:]] = int(v) if a == "--every" else v
        elif not a.startswith("-") and opts["me"] is None:
            opts["me"] = a
        else:
            return None
    return opts if opts["me"] else None


def main(argv):
    opts = options(argv[1:])
    if opts is None:
        print("usage: watch.py ME [--state DIR] [--every SECONDS] [--runs REPO]... [--org ORG] | --self-test",
              file=sys.stderr)
        return 64
    me, org, state_dir = opts["me"], opts["org"], opts["state"]
    runs = opts["runs"] or [f"{org}/{me}"]
    os.makedirs(state_dir, exist_ok=True)
    path = os.path.join(state_dir, "watch.json")
    state = load(path)
    announce = state is not None
    state = state or {"issues": {}, "runs": {}}
    cmts = state.setdefault("comments", {})
    last, said = time.monotonic(), False
    while True:
        # A margin of 10 min: the search index lags behind a change.
        start = time.time() - 600
        since = state.get("since") if isinstance(state.get("since"), str) else stamp(time.time() - 30 * 86400)
        # The first search records 30 days and prints nothing, also over
        # a state of an older watcher that knew fewer items.
        first = not isinstance(state.get("since"), str)
        pages, speak = (5, False) if first else (1, announce)
        answered, found = 0, []
        for kind in ("issue", "pr"):
            got = search(org, kind, since, pages)
            answered += got is not None
            found += got or []
        searched = answered == 2
        changed = []
        for repo in sorted({(i.get("repository_url") or "").split("/repos/")[-1] for i in found}):
            items = [i for i in found if (i.get("repository_url") or "").endswith(f"/repos/{repo}")]
            before = {k: dict(v) for k, v in state["issues"].items() if k.startswith(f"{repo}#")}
            for text in issues_step(me, org, repo, items, state["issues"], speak, state_dir):
                print(text, flush=True)
            changed += [(repo, i) for i in items if f"{repo}#{i.get('number')}" in state["issues"]
                        and before.get(f"{repo}#{i.get('number')}") != state["issues"][f"{repo}#{i.get('number')}"]]
        for repo, item in changed if speak else []:
            if not item.get("comments") and not ("pull_request" in item and repo == f"{org}/{me}"):
                continue
            lists = [f"repos/{repo}/issues/{item['number']}/comments?since={since}&per_page=100"]
            if "pull_request" in item and repo == f"{org}/{me}":
                lists.append(f"repos/{repo}/pulls/{item['number']}/comments?since={since}&per_page=100")
            for one in lists:
                got = get(one)
                answered += isinstance(got, list)
                for text in comments_step(me, org, repo, got or [], state["issues"], cmts, True):
                    print(text, flush=True)
        if searched:
            state["since"] = stamp(start)
        for repo in runs:
            doc = get(f"repos/{repo}/actions/runs?per_page=30")
            if isinstance(doc, dict):
                answered += 1
                # The first read of a repository records its runs and prints nothing.
                known = any(k.startswith(f"{repo} ") for k in state["runs"])
                for text in runs_step(repo, doc.get("workflow_runs", []), state["runs"], announce and known):
                    print(text, flush=True)
        if answered and said:
            print("WATCH: the mirror answers again", flush=True)
        elif not answered and not said and time.monotonic() - last > SILENCE:
            print(f"WATCH: no answer from the mirror for {SILENCE // 60} min; the watcher goes on trying", flush=True)
        said = not answered and (said or time.monotonic() - last > SILENCE)
        if answered:
            last = time.monotonic()
            save(path, state)
            announce = True
        time.sleep(opts["every"] + random.uniform(0, 10))


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        # The checks are in watch_test.py beside this file.
        sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
        from watch_test import self_test

        sys.exit(self_test())
    sys.exit(main(sys.argv))
