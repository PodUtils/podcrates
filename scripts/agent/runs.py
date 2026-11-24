#!/usr/bin/env python3
"""The one watcher of the Actions runs of a repository, for each session
of PodUtils (the owner's ruling "Watchers"). It needs `watch.py` beside it.

    python3 -I runs.py [--repo OWNER/NAME] [--branch NAME] [--every SECONDS]
                       [--until SHA] [--min-runs N] [--timeout SECONDS]
                       [--once]
    python3 -I runs.py --self-test

The defaults are `--repo PodUtils/podcrates --branch main --every 60
--timeout 3600 --min-runs 1`. `--every` is at least 20 s.

With no `--until`, each poll prints one line for each run of the branch
that is new or that changed its state or its attempt. For a run that ended
red, one line follows for each failed job, with its failed steps. The
first poll prints no line for a run that ended before it.

`--until SHA` (a prefix is enough; a full SHA reads the runs of the commit
on each branch) follows the runs of that commit until N or more exist and
each one ended. Exit 0 when each one is green (success, skipped, neutral),
1 when one is red, 3 when each one was cancelled (a newer push), 2 at the
timeout.

`--once` makes one poll: it prints the state of each run of the newest
commit (of the commit of `--until`, with its exit code; 2 when a run did
not end), then exits. Exit 2 when no list of runs came. Exit 64 for a bad
argument.

The reads go through forge.web.pkgforge.dev/github (`watch.read`). A 429
or a 503 waits for its `Retry-After`; a 502 waits with backoff; another
failure is one line on stderr, and the poll goes on. Python's standard
library only. Run it as a background process with a log, beside
`watch.py`, and read the log.
"""

import email.utils
import os
import sys
import time
import urllib.parse

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import watch  # noqa: E402

TRIES = 5
KEEP = 500
USAGE = ("usage: runs.py [--repo OWNER/NAME] [--branch NAME] [--every SECONDS] [--until SHA] [--min-runs N]"
         " [--timeout SECONDS] [--once] | --self-test")


def wait_of(retry_after, default, now=None):
    """The seconds of a `Retry-After` value (seconds or an HTTP date),
    from 1 to 600; DEFAULT when the value is missing or bad."""
    if retry_after is None:
        return default
    try:
        secs = int(retry_after)
    except ValueError:
        try:
            when = email.utils.parsedate_to_datetime(retry_after).timestamp()
        except (TypeError, ValueError):
            return default
        secs = int(when - (time.time() if now is None else now))
    return max(1, min(600, secs))


def get(path, opener=None, sleep=time.sleep, err=None):
    """The JSON of `path`, or None. 429 and 503 wait for `Retry-After`,
    502 waits with backoff, each up to TRIES reads; another failure is one
    line on stderr."""
    err = err or (lambda line: print(line, file=sys.stderr, flush=True))
    delay = 5
    for _ in range(TRIES):
        doc, code, retry, why = watch.read(path, opener)
        if doc is not None:
            return doc
        if code in (429, 503):
            sleep(wait_of(retry, delay))
        elif code == 502:
            sleep(delay)
        else:
            err(f"runs.py: GET {path.split('?')[0]}: {why}")
            return None
        delay = min(delay * 2, 300)
    err(f"runs.py: GET {path.split('?')[0]}: {why} after {TRIES} reads")
    return None


def options(argv):
    opts = {"repo": "PodUtils/podcrates", "branch": "main", "every": 60, "until": None, "min-runs": 1,
            "timeout": 3600, "once": False}
    rest = list(argv)
    while rest:
        a = rest.pop(0)
        key = a[2:]
        if a == "--once":
            opts["once"] = True
        elif key in opts and a.startswith("--") and rest:
            v = rest.pop(0)
            if key in ("every", "min-runs", "timeout"):
                if not v.isdigit():
                    return None
                v = int(v)
            opts[key] = v
        else:
            return None
    opts["every"] = max(20, opts["every"])
    if opts["until"] is not None:
        opts["until"] = opts["until"].lower()
        if not opts["until"] or any(c not in "0123456789abcdef" for c in opts["until"]):
            return None
    return opts


def job_lines(repo, run, fetch):
    doc = fetch(f"repos/{repo}/actions/runs/{run['id']}/jobs?filter=latest&per_page=100")
    if not isinstance(doc, dict):
        return ["  the jobs of the run did not come"]
    return [f"  job {i} {name}: {end}" + (f" (steps: {'; '.join(steps)})" if steps else "")
            for i, name, end, steps in watch.red_jobs(doc)]


def verdict(mine, min_runs):
    """The exit code of `--until` for the runs of the commit, or None while
    a run has not ended or fewer than MIN_RUNS exist."""
    if len(mine) < min_runs or not mine or any(r.get("status") != "completed" for r in mine):
        return None
    ends = {r.get("conclusion") for r in mine}
    if ends - set(watch.GREEN) - {"cancelled"}:
        return 1
    return 3 if ends == {"cancelled"} else 0


def poll(opts, runs, seen, first, fetch, say):
    """The lines of one list of runs (newest first); `seen` maps a run id
    to its last state."""
    repo, until = opts["repo"], opts["until"]
    for r in reversed(runs):
        state = [r.get("status"), r.get("conclusion"), r.get("run_attempt") or 1]
        before = seen.get(str(r["id"]))
        seen[str(r["id"])] = state
        if before == state:
            continue
        mine = until is not None and (r.get("head_sha") or "").startswith(until)
        if before is None and first and r.get("status") == "completed" and not mine:
            continue
        say(watch.run_line(repo, r, r.get("status")))
        if r.get("status") == "completed" and r.get("conclusion") not in watch.GREEN + ("cancelled",):
            for line in job_lines(repo, r, fetch):
                say(line)
    for key in sorted(seen, key=int)[:-KEEP]:
        del seen[key]


def main(argv, opener=None, sleep=time.sleep, clock=time.monotonic, say=None, err=None):
    if argv == ["--self-test"]:
        return self_test()
    opts = options(argv)
    if opts is None:
        print(USAGE, file=sys.stderr)
        return 64
    say = say or (lambda line: print(f"{time.strftime('%H:%M:%SZ', time.gmtime())} {line}", flush=True))
    repo, until = opts["repo"], opts["until"]

    def fetch(path):
        return get(path, opener, sleep, err)

    if until is not None and len(until) == 40:
        path = f"repos/{repo}/actions/runs?head_sha={until}&per_page=50"
    else:
        path = f"repos/{repo}/actions/runs?branch={urllib.parse.quote(opts['branch'], safe='')}&per_page=30"
    seen, first, start = {}, True, clock()
    while True:
        doc = fetch(path)
        runs = doc.get("workflow_runs") if isinstance(doc, dict) else None
        if isinstance(runs, list):
            head = until or (runs[0].get("head_sha") if runs else "")
            mine = [r for r in runs if (r.get("head_sha") or "").startswith(head or "-")]
            if opts["once"]:
                for r in mine:
                    say(watch.run_line(repo, r, r.get("status")))
                if until is None:
                    return 0
                code = verdict(mine, opts["min-runs"])
                return 2 if code is None else code
            poll(opts, runs, seen, first, fetch, say)
            first = False
            code = verdict(mine, opts["min-runs"]) if until else None
            if code is not None:
                say(f"{until}: " + {0: "GREEN", 1: "RED", 3: "each run was cancelled"}[code])
                return code
        elif opts["once"]:
            return 2
        if until and clock() - start > opts["timeout"]:
            say(f"{until}: no end within {opts['timeout']} s")
            return 2
        sleep(opts["every"])


def self_test():
    """The checks with a fake forge: each answer comes from a queue."""
    import io
    import json
    import urllib.error

    check = watch.Checks()

    def run(i, status="completed", conclusion="success", sha="abcdef0123", attempt=1):
        return {"id": i, "name": "ci", "event": "push", "head_sha": sha, "status": status,
                "conclusion": conclusion, "run_attempt": attempt}

    red = {"jobs": [{"id": 7, "name": "test", "conclusion": "failure", "steps": [
        {"name": "build", "conclusion": "success"}, {"name": "cargo test", "conclusion": "failure"},
        {"name": "upload", "conclusion": "skipped"}]}, {"id": 8, "name": "fmt", "conclusion": "success"}]}

    def forge(lists, jobs=None):
        """A fake opener: each read of the run list takes the next answer
        (a list of runs, or an HTTP code with its Retry-After)."""
        asked = []

        def opener(req, timeout):
            path = req.full_url.split("/github/", 1)[1]
            asked.append(path)
            if "/jobs" in path:
                body = jobs or {"jobs": []}
            else:
                body = lists.pop(0) if len(lists) > 1 else lists[0]
            if isinstance(body, tuple):
                raise urllib.error.HTTPError(req.full_url, body[0], "refused", {"Retry-After": body[1]}, None)
            return io.BytesIO(json.dumps(body if isinstance(body, dict) else {"workflow_runs": body}).encode())
        return opener, asked

    def go(argv, lists, jobs=None, polls=None):
        opener, asked = forge(lists, jobs)
        lines, errs, slept, now = [], [], [], [0.0]

        def sleep(s):
            slept.append(s)
            now[0] += s
            if polls is not None and len([x for x in slept if x >= 20]) >= polls:
                raise StopIteration

        try:
            code = main(argv, opener, sleep, lambda: now[0], lines.append, errs.append)
        except StopIteration:
            code = None
        return code, lines, errs, slept, asked

    old, new = run(1), run(2, "in_progress", None, "1234567aaa")
    code, lines, _, _, _ = go([], [[new, old], [run(2, sha="1234567aaa"), old]], polls=2)
    check("the first poll gives no line for an old run",
          lines[:1], ["RUN in_progress: PodUtils/podcrates ci 2 (push, 1234567)"])
    check("a run that ends is one line", lines[1:], ["RUN completed: PodUtils/podcrates ci 2 (push, 1234567): success"])
    fast = run(3, conclusion="failure", sha="1234567aaa")
    _, lines, _, _, _ = go([], [[old], [fast, old]], red, polls=2)
    check("a run that starts and ends red between two polls gives its failed jobs and steps", lines,
          ["RUN completed: PodUtils/podcrates ci 3 (push, 1234567): failure",
           "  job 7 test: failure (steps: cargo test)"])
    _, lines, _, _, _ = go([], [[old], [run(1, "queued", None, attempt=2)]], polls=2)
    check("a run again is a line", lines, ["RUN queued: PodUtils/podcrates ci 1 (push, abcdef0, attempt 2)"])
    sha = "abcdef0123" + "0" * 30
    code, lines, _, _, asked = go(["--until", sha], [[run(4, "in_progress", None, sha)], [run(4, sha=sha)]])
    check("--until: green is exit 0", (code, lines[-1]), (0, f"{sha}: GREEN"))
    check("--until with a full SHA reads the runs of the commit", asked[0].split("&n=")[0],
          f"repos/PodUtils/podcrates/actions/runs?head_sha={sha}&per_page=50")
    check("--until: an old run of the commit is a line at the first poll", lines[0],
          "RUN in_progress: PodUtils/podcrates ci 4 (push, abcdef0)")
    code, _, _, _, _ = go(["--until", "abcdef0"], [[run(5, conclusion="failure"), run(6)]], red)
    check("--until: one red run is exit 1", code, 1)
    code, _, _, _, _ = go(["--until", "abcdef0"], [[run(5, conclusion="cancelled"), run(6, conclusion="cancelled")]])
    check("--until: each run cancelled is exit 3", code, 3)
    code, _, _, _, _ = go(["--until", "abcdef0"], [[run(5, conclusion="cancelled"), run(6, conclusion="skipped")]])
    check("--until: a cancelled run beside a green run is exit 0", code, 0)
    code, lines, _, _, _ = go(["--until", "abcdef0", "--timeout", "100"], [[run(5, "queued", None)]])
    check("--until: the timeout is exit 2", (code, lines[-1]), (2, "abcdef0: no end within 100 s"))
    code, _, _, slept, _ = go(["--until", "abcdef0", "--min-runs", "2"], [[run(5)], [run(6), run(5)]])
    check("--until: --min-runs waits for more runs", (code, len(slept)), (0, 1))
    code, lines, _, _, _ = go(["--once"], [[run(9, "queued", None, "9999999aaa"), old]])
    check("--once: the state of each run of the newest commit", (code, lines),
          (0, ["RUN queued: PodUtils/podcrates ci 9 (push, 9999999)"]))
    code, _, _, _, _ = go(["--once", "--until", "abcdef0"], [[run(5, "queued", None)]])
    check("--once --until: a run that did not end is exit 2", code, 2)
    code, _, errs, _, _ = go(["--once"], [(404, None)])
    check("--once: no list is exit 2, and one line on stderr", (code, errs),
          (2, ["runs.py: GET repos/PodUtils/podcrates/actions/runs: HTTP 404"]))
    code, _, errs, slept, _ = go(["--once"], [(429, "17"), (503, "3"), [old]])
    check("429 and 503 wait for Retry-After", (code, slept, errs), (0, [17, 3], []))
    code, _, errs, slept, _ = go(["--once"], [(502, None), (502, None), (502, None), [old]])
    check("502 waits with backoff", (code, slept), (0, [5, 10, 20]))
    _, _, errs, slept, _ = go(["--once"], [(502, None)])
    check("502 gives up after five reads with one line", (slept, errs),
          ([5, 10, 20, 40, 80], ["runs.py: GET repos/PodUtils/podcrates/actions/runs: HTTP 502 after 5 reads"]))
    code, lines, errs, _, _ = go(["--every", "20"], [(403, None), [new, old], [run(2, sha="1234567aaa"), old]],
                                 polls=3)
    check("another error is one line on stderr and the poll goes on", (errs, len(lines)),
          (["runs.py: GET repos/PodUtils/podcrates/actions/runs: HTTP 403"], 2))
    check("a date in Retry-After", wait_of("Thu, 01 Jan 1970 00:01:00 GMT", 5, now=0), 60)
    check("a bad Retry-After takes the default", wait_of("soon", 5), 5)
    check("--every is at least 20 s", options(["--every", "5"])["every"], 20)
    check("a bad argument", (options(["--until", "xyz"]), options(["--every", "a"]), options(["--what"])),
          (None, None, None))
    print("runs self-test:", "passed" if check.ok else "FAILED")
    return 0 if check.ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
